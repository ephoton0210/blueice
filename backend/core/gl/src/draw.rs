// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A draw call: vertex shading, primitive assembly and clipping, then
//! rasterization and fragment shading in horizontal bands on several threads
//! (each band sees every primitive in submission order, so blending and depth
//! behave as if drawn serially).

use super::glsl::ast::{SamplerKind, Scalar, StructDef, Ty};
use super::glsl::builtins::Textures;
use super::glsl::interp::Machine;
use super::glsl::value::{zero, V};
use super::tex::Texture;
use super::*;

/// Window-space triangles smaller than this (twice the area) are dropped.
const MIN_AREA: f64 = 1e-12;
/// Most primitives one draw call may assemble: bounds memory and time.
/// A context whose draws keep running out of time stops drawing.
const MAX_SLOW_DRAWS: u32 = 3;
const MAX_PRIMITIVES: usize = 2_000_000;

struct TexAccess<'a> {
    textures: &'a HashMap<u32, Texture>,
    bindings: &'a HashMap<(usize, u32), u32>,
}

impl TexAccess<'_> {
    fn texture(&self, unit: u32, kind: SamplerKind) -> Option<&Texture> {
        let target = match kind {
            SamplerKind::Cube | SamplerKind::CubeShadow => TEXTURE_CUBE_MAP,
            SamplerKind::S3D => 0x806F,
            SamplerKind::S2DArray => 0x8C1A,
            _ => TEXTURE_2D,
        };
        let id = *self.bindings.get(&(unit as usize, target))?;
        self.textures.get(&id).filter(|t| t.complete())
    }
}

impl Textures for TexAccess<'_> {
    fn sample(
        &self,
        unit: u32,
        kind: SamplerKind,
        c: [f32; 4],
        lod: Option<f32>,
        _bias: f32,
    ) -> [f32; 4] {
        let Some(t) = self.texture(unit, kind) else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        match kind {
            SamplerKind::Cube | SamplerKind::CubeShadow => t.sample_cube(c[0], c[1], c[2], lod),
            SamplerKind::S2DShadow => {
                let d = t.sample_2d(0, c[0], c[1], lod)[0];
                let lit = if c[2] <= d { 1.0 } else { 0.0 };
                [lit, lit, lit, 1.0]
            }
            SamplerKind::S3D | SamplerKind::S2DArray => [0.0, 0.0, 0.0, 1.0],
            _ => t.sample_2d(0, c[0], c[1], lod),
        }
    }
    fn fetch(&self, unit: u32, kind: SamplerKind, c: [i32; 3], lod: i32) -> [f32; 4] {
        let Some(t) = self.texture(unit, kind) else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        match t.level(0, lod.max(0) as usize) {
            Some(l) => {
                let x = c[0].clamp(0, l.w as i32 - 1) as usize;
                let y = c[1].clamp(0, l.h as i32 - 1) as usize;
                let i = (y * l.w as usize + x) * 4;
                [l.data[i], l.data[i + 1], l.data[i + 2], l.data[i + 3]]
            }
            None => [0.0, 0.0, 0.0, 1.0],
        }
    }
    fn size(&self, unit: u32, kind: SamplerKind, lod: i32) -> [i32; 3] {
        match self
            .texture(unit, kind)
            .and_then(|t| t.level(0, lod.max(0) as usize))
        {
            Some(l) => [l.w as i32, l.h as i32, 1],
            None => [1, 1, 1],
        }
    }
}

fn flatten(v: &V, out: &mut Vec<u32>) {
    match v {
        V::Vec { n, c, .. } => out.extend_from_slice(&c[..*n as usize]),
        V::Mat { cols, rows, d } => {
            out.extend(d[..(*cols * *rows) as usize].iter().map(|x| x.to_bits()))
        }
        V::Struct(_, fields) => fields.iter().for_each(|f| flatten(f, out)),
        V::Arr(items) => items.iter().for_each(|f| flatten(f, out)),
        _ => {}
    }
}

fn unflatten(ty: &Ty, lanes: &[u32], at: &mut usize, structs: &[StructDef]) -> V {
    match ty {
        Ty::Scalar(k) => {
            let v = V::Vec {
                k: *k,
                n: 1,
                c: [lanes[*at], 0, 0, 0],
            };
            *at += 1;
            v
        }
        Ty::Vec(k, n) => {
            let mut c = [0u32; 4];
            c[..*n as usize].copy_from_slice(&lanes[*at..*at + *n as usize]);
            *at += *n as usize;
            V::Vec { k: *k, n: *n, c }
        }
        Ty::Mat(cols, rows) => {
            let mut d = [0.0f32; 16];
            for (i, slot) in d.iter_mut().enumerate().take((*cols * *rows) as usize) {
                *slot = f32::from_bits(lanes[*at + i]);
            }
            *at += (*cols * *rows) as usize;
            V::Mat {
                cols: *cols,
                rows: *rows,
                d,
            }
        }
        Ty::Struct(name) => {
            let (index, def) = structs
                .iter()
                .enumerate()
                .find(|(_, s)| &s.name == name)
                .expect("struct exists");
            V::Struct(
                index,
                def.fields
                    .iter()
                    .map(|(_, t)| unflatten(t, lanes, at, structs))
                    .collect(),
            )
        }
        Ty::Array(inner, n) => V::Arr(
            (0..*n)
                .map(|_| unflatten(inner, lanes, at, structs))
                .collect(),
        ),
        _ => V::Void,
    }
}

/// Which lanes of a varying interpolate smoothly (floats not marked `flat`).
fn smooth_lanes(ty: &Ty, flat: bool, structs: &[StructDef], out: &mut Vec<bool>) {
    match ty {
        Ty::Scalar(k) => out.push(*k == Scalar::Float && !flat),
        Ty::Vec(k, n) => out.extend((0..*n).map(|_| *k == Scalar::Float && !flat)),
        Ty::Mat(c, r) => out.extend((0..*c * *r).map(|_| !flat)),
        Ty::Struct(name) => {
            if let Some(def) = structs.iter().find(|s| &s.name == name) {
                for (_, t) in &def.fields {
                    smooth_lanes(t, flat, structs, out);
                }
            }
        }
        Ty::Array(inner, n) => (0..*n).for_each(|_| smooth_lanes(inner, flat, structs, out)),
        _ => {}
    }
}

#[derive(Clone)]
struct ClipVtx {
    pos: [f64; 4],
    lanes: Vec<u32>,
}

struct Vertex {
    pos: [f32; 4],
    lanes: Vec<u32>,
    point_size: f32,
}

struct Tri {
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
    inv_w: [f64; 3],
    lanes: [Vec<u32>; 3],
    front: bool,
    /// Twice the signed area (positive: counter-clockwise).
    area: f64,
}

struct Point {
    x: f64,
    y: f64,
    z: f64,
    size: f64,
    lanes: Vec<u32>,
}

struct Segment {
    a: (f64, f64, f64, f64),
    b: (f64, f64, f64, f64),
    lanes: [Vec<u32>; 2],
}

enum Prim {
    Tri(Tri),
    Point(Point),
    Line(Segment),
}

fn lerp_vertex(a: &ClipVtx, b: &ClipVtx, t: f64, smooth: &[bool], provoking: &[u32]) -> ClipVtx {
    let mut pos = [0.0; 4];
    for i in 0..4 {
        pos[i] = a.pos[i] + (b.pos[i] - a.pos[i]) * t;
    }
    let lanes = (0..a.lanes.len())
        .map(|i| {
            if smooth[i] {
                let (x, y) = (
                    f64::from(f32::from_bits(a.lanes[i])),
                    f64::from(f32::from_bits(b.lanes[i])),
                );
                ((x + (y - x) * t) as f32).to_bits()
            } else {
                provoking[i]
            }
        })
        .collect();
    ClipVtx { pos, lanes }
}

/// Clips a polygon against `w + sign*coord >= 0` for one axis.
fn clip_plane(
    poly: &[ClipVtx],
    axis: usize,
    sign: f64,
    smooth: &[bool],
    provoking: &[u32],
) -> Vec<ClipVtx> {
    let dist = |v: &ClipVtx| v.pos[3] + sign * v.pos[axis];
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (a, b) = (&poly[i], &poly[(i + 1) % poly.len()]);
        let (da, db) = (dist(a), dist(b));
        if da >= 0.0 {
            out.push(a.clone());
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(lerp_vertex(a, b, t, smooth, provoking));
        }
    }
    out
}

fn depth_passes(func: u32, frag: f32, stored: f32) -> bool {
    match func {
        0x200 => false,
        0x201 => frag < stored,
        0x202 => frag == stored,
        0x203 => frag <= stored,
        0x204 => frag > stored,
        0x205 => frag != stored,
        0x206 => frag >= stored,
        _ => true,
    }
}

fn blend_factor(
    f: u32,
    src: [f32; 4],
    dst: [f32; 4],
    constant: [f32; 4],
    channel: usize,
    alpha_only: bool,
) -> f32 {
    let rgb = |c: [f32; 4]| c[channel];
    match f {
        0 => 0.0,
        1 => 1.0,
        0x300 => {
            if alpha_only {
                src[3]
            } else {
                rgb(src)
            }
        }
        0x301 => 1.0 - if alpha_only { src[3] } else { rgb(src) },
        0x302 => src[3],
        0x303 => 1.0 - src[3],
        0x304 => dst[3],
        0x305 => 1.0 - dst[3],
        0x306 => {
            if alpha_only {
                dst[3]
            } else {
                rgb(dst)
            }
        }
        0x307 => 1.0 - if alpha_only { dst[3] } else { rgb(dst) },
        0x308 => {
            if channel == 3 {
                1.0
            } else {
                src[3].min(1.0 - dst[3])
            }
        }
        0x8001 => {
            if alpha_only {
                constant[3]
            } else {
                rgb(constant)
            }
        }
        0x8002 => {
            1.0 - if alpha_only {
                constant[3]
            } else {
                rgb(constant)
            }
        }
        0x8003 => constant[3],
        0x8004 => 1.0 - constant[3],
        _ => 1.0,
    }
}

fn blend(s: &State, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0; 4];
    for c in 0..4 {
        let alpha = c == 3;
        let (sf, df, eq) = if alpha {
            (s.blend_src_alpha, s.blend_dst_alpha, s.blend_eq_alpha)
        } else {
            (s.blend_src_rgb, s.blend_dst_rgb, s.blend_eq_rgb)
        };
        let a = src[c] * blend_factor(sf, src, dst, s.blend_color, c, false);
        let b = dst[c] * blend_factor(df, src, dst, s.blend_color, c, false);
        out[c] = match eq {
            0x800A => a - b,
            0x800B => b - a,
            0x8007 => src[c].min(dst[c]),
            0x8008 => src[c].max(dst[c]),
            _ => a + b,
        };
    }
    out
}

/// A horizontal band of the target.
struct Band<'a> {
    y0: usize,
    w: usize,
    color: BandColor<'a>,
    depth: Option<&'a mut [f32]>,
}

enum BandColor<'a> {
    None,
    Bytes(&'a mut [u8]),
    Floats(&'a mut [f32]),
}

impl Band<'_> {
    fn read(&self, local: usize) -> [f32; 4] {
        match &self.color {
            BandColor::Bytes(b) => {
                let p = &b[local * 4..local * 4 + 4];
                [
                    f32::from(p[0]) / 255.0,
                    f32::from(p[1]) / 255.0,
                    f32::from(p[2]) / 255.0,
                    f32::from(p[3]) / 255.0,
                ]
            }
            BandColor::Floats(f) => [
                f[local * 4],
                f[local * 4 + 1],
                f[local * 4 + 2],
                f[local * 4 + 3],
            ],
            BandColor::None => [0.0; 4],
        }
    }
    fn write(&mut self, local: usize, c: [f32; 4], mask: [bool; 4]) {
        match &mut self.color {
            BandColor::Bytes(b) => {
                for k in 0..4 {
                    if mask[k] {
                        b[local * 4 + k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            }
            BandColor::Floats(f) => {
                for k in 0..4 {
                    if mask[k] {
                        f[local * 4 + k] = c[k];
                    }
                }
            }
            BandColor::None => {}
        }
    }
}

struct Fragment<'a> {
    x: usize,
    y: usize,
    z: f32,
    front: bool,
    lanes: &'a [u32],
    point_coord: [f32; 2],
}

/// The fragment stage for one band: a shader machine and the pipeline state.
struct FragmentStage<'a> {
    machine: Machine<'a>,
    state: &'a State,
    varyings: &'a [Varying],
    fs_structs: &'a [StructDef],
    color_slot: Option<usize>,
    data_slot: Option<usize>,
    out_slot: Option<usize>,
    depth_slot: Option<usize>,
    frag_coord: usize,
    front_facing: usize,
    point_coord: usize,
    writes_depth: bool,
    deadline: std::time::Instant,
    expired: bool,
}

impl FragmentStage<'_> {
    /// True once the draw's time budget is spent (checked per row).
    fn out_of_time(&mut self) -> bool {
        if !self.expired && std::time::Instant::now() >= self.deadline {
            self.expired = true;
        }
        self.expired
    }

    /// Shades one fragment and writes it; false if it was rejected.
    fn run(&mut self, band: &mut Band<'_>, f: &Fragment<'_>) {
        // A shader this slow must not hold a draw past its budget by a whole row.
        if self.out_of_time() {
            return;
        }
        let st = self.state;
        let local = (f.y - band.y0) * band.w + f.x;
        // Early depth test when the shader cannot change the outcome.
        let early = !self.writes_depth;
        if early && st.depth_test {
            if let Some(d) = band.depth.as_deref() {
                if !depth_passes(st.depth_func, f.z, d[local]) {
                    return;
                }
            }
        }
        let mut at = 0;
        for v in self.varyings {
            let value = unflatten(&v.ty, f.lanes, &mut at, self.fs_structs);
            self.machine.set(v.fs_slot, value);
        }
        self.machine.set(
            self.frag_coord,
            V::vec4(f.x as f32 + 0.5, f.y as f32 + 0.5, f.z, 1.0),
        );
        self.machine.set(self.front_facing, V::b(f.front));
        self.machine.set(self.point_coord, V::vec(&f.point_coord));
        match self.machine.run_main() {
            Ok(false) => {}
            // A discard, or a shader that failed: nothing is drawn.
            _ => return,
        }
        let color = self
            .out_slot
            .or(self.color_slot)
            .and_then(|s| self.machine.get(s).floats())
            .or_else(|| {
                self.data_slot.and_then(|s| match self.machine.get(s) {
                    V::Arr(items) => items.first().and_then(V::floats),
                    _ => None,
                })
            })
            .map_or([0.0, 0.0, 0.0, 1.0], |c| {
                let mut out = [0.0, 0.0, 0.0, 1.0];
                for (o, x) in out.iter_mut().zip(c) {
                    *o = x;
                }
                out
            });
        let mut z = f.z;
        if self.writes_depth {
            if let Some(d) = self.depth_slot.and_then(|s| self.machine.get(s).as_f32()) {
                z = d;
            }
            if st.depth_test {
                if let Some(d) = band.depth.as_deref() {
                    if !depth_passes(st.depth_func, z, d[local]) {
                        return;
                    }
                }
            }
        }
        let out = if st.blend {
            blend(st, color, band.read(local))
        } else {
            color
        };
        band.write(local, out, st.color_mask);
        if st.depth_test && st.depth_mask {
            if let Some(d) = band.depth.as_deref_mut() {
                d[local] = z;
            }
        }
    }
}

impl Context {
    pub fn draw_arrays(&mut self, mode: u32, first: u32, count: u32, instances: u32) {
        if first.checked_add(count).is_none() || count > 16_000_000 {
            self.fail(INVALID_VALUE);
            return;
        }
        let indices: Vec<u32> = (first..first + count).collect();
        self.draw(mode, indices, instances);
    }

    pub fn draw_elements(&mut self, mode: u32, count: u32, ty: u32, offset: u32, instances: u32) {
        let buffer = self.vaos.get(&self.vao).map_or(0, |v| v.element_buffer);
        let Some(data) = self.buffers.get(&buffer) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let size = match ty {
            0x1401 => 1usize,
            0x1403 => 2,
            0x1405 => 4,
            _ => {
                self.fail(INVALID_ENUM);
                return;
            }
        };
        let start = offset as usize;
        let end = start.saturating_add(count as usize * size);
        if end > data.len() {
            self.fail(INVALID_OPERATION);
            return;
        }
        let indices: Vec<u32> = data[start..end]
            .chunks_exact(size)
            .map(|c| match size {
                1 => u32::from(c[0]),
                2 => u32::from(u16::from_le_bytes([c[0], c[1]])),
                _ => u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
            })
            .collect();
        self.draw(mode, indices, instances);
    }

    fn draw(&mut self, mode: u32, indices: Vec<u32>, instances: u32) {
        let instances = instances.max(1);
        if indices.is_empty() || self.slow_draws >= MAX_SLOW_DRAWS {
            return;
        }
        let Some(program) = self
            .programs
            .get(&self.current_program)
            .filter(|p| p.linked.is_some())
        else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let linked = program.linked.as_ref().expect("filtered");
        if indices.len() as u64 * u64::from(instances) > 16_000_000 {
            self.fail(INVALID_OPERATION);
            return;
        }
        let access = TexAccess {
            textures: &self.textures,
            bindings: &self.tex_bindings,
        };

        let profile = std::env::var("BI_GL_PROFILE").is_ok();
        let t0 = std::time::Instant::now();
        // ---- vertex stage ----
        let vao = self.vaos.get(&self.vao).cloned().unwrap_or_default();
        let total = indices.len() * instances as usize;
        let threads = worker_count(total, 256);
        let chunk = total.div_ceil(threads);
        let mut vertices: Vec<Option<Vertex>> = (0..total).map(|_| None).collect();
        let failed = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            for (n, out) in vertices.chunks_mut(chunk).enumerate() {
                let (linked, vao, buffers, access, indices, failed) =
                    (linked, &vao, &self.buffers, &access, &indices, &failed);
                scope.spawn(move || {
                    let Ok(mut m) = Machine::new(&linked.vs, access) else {
                        failed.store(true, std::sync::atomic::Ordering::Relaxed);
                        return;
                    };
                    apply_uniforms(&mut m, linked, true);
                    let slots = (
                        linked.vs.slot("gl_Position"),
                        linked.vs.slot("gl_PointSize"),
                        linked.vs.slot("gl_VertexID"),
                        linked.vs.slot("gl_InstanceID"),
                        linked.vs.slot("gl_VertexIndex"),
                        linked.vs.slot("gl_InstanceIndex"),
                    );
                    for (k, slot) in out.iter_mut().enumerate() {
                        let flat = n * chunk + k;
                        let (instance, i) = (flat / indices.len(), flat % indices.len());
                        let index = indices[i];
                        for a in &linked.attributes {
                            let value = fetch_attrib(
                                vao,
                                buffers,
                                a.location as usize,
                                index,
                                instance as u32,
                                &a.ty,
                            );
                            m.set(a.vs_slot, value);
                        }
                        for (s, v) in [
                            (slots.2, index as i32),
                            (slots.3, instance as i32),
                            (slots.4, index as i32),
                            (slots.5, instance as i32),
                        ] {
                            if let Some(s) = s {
                                m.set(s, V::i(v));
                            }
                        }
                        if let Some(s) = slots.1 {
                            m.set(s, V::f(1.0));
                        }
                        if m.run_main().is_err() {
                            failed.store(true, std::sync::atomic::Ordering::Relaxed);
                            return;
                        }
                        let pos = slots
                            .0
                            .and_then(|s| m.get(s).floats())
                            .filter(|p| p.len() == 4)
                            .map_or([0.0; 4], |p| [p[0], p[1], p[2], p[3]]);
                        let mut lanes = Vec::new();
                        for v in &linked.varyings {
                            flatten(m.get(v.vs_slot), &mut lanes);
                        }
                        let point_size = slots.1.and_then(|s| m.get(s).as_f32()).unwrap_or(1.0);
                        *slot = Some(Vertex {
                            pos,
                            lanes,
                            point_size,
                        });
                    }
                });
            }
        });
        if failed.load(std::sync::atomic::Ordering::Relaxed) {
            self.fail(INVALID_OPERATION);
            return;
        }
        let vertices: Vec<Vertex> = vertices.into_iter().flatten().collect();

        let t_vertex = t0.elapsed();
        // ---- assembly, clipping, viewport transform ----
        let mut smooth = Vec::new();
        for v in &linked.varyings {
            smooth_lanes(&v.ty, v.flat, &linked.fs.structs, &mut smooth);
        }
        let s = self.state.clone();
        let vp = s.viewport;
        let (near, far) = (f64::from(s.depth_range[0]), f64::from(s.depth_range[1]));
        let to_window = |p: &[f64; 4]| -> (f64, f64, f64, f64) {
            let inv_w = 1.0 / p[3];
            (
                f64::from(vp[0]) + (p[0] * inv_w * 0.5 + 0.5) * f64::from(vp[2]),
                f64::from(vp[1]) + (p[1] * inv_w * 0.5 + 0.5) * f64::from(vp[3]),
                near + (far - near) * (p[2] * inv_w * 0.5 + 0.5),
                inv_w,
            )
        };
        let mut prims: Vec<Prim> = Vec::new();
        let per_instance = indices.len();
        for instance in 0..instances as usize {
            let vs = &vertices[instance * per_instance..(instance + 1) * per_instance];
            let push_tri = |a: usize, b: usize, c: usize, prims: &mut Vec<Prim>| {
                if prims.len() >= MAX_PRIMITIVES {
                    return;
                }
                let make = |v: &Vertex| ClipVtx {
                    pos: v.pos.map(f64::from),
                    lanes: v.lanes.clone(),
                };
                let provoking = vs[c].lanes.clone();
                let mut poly = vec![make(&vs[a]), make(&vs[b]), make(&vs[c])];
                for (axis, sign) in [(2usize, 1.0), (2, -1.0)] {
                    poly = clip_plane(&poly, axis, sign, &smooth, &provoking);
                    if poly.len() < 3 {
                        return;
                    }
                }
                for k in 1..poly.len() - 1 {
                    let tri = [&poly[0], &poly[k], &poly[k + 1]];
                    if tri.iter().any(|v| v.pos[3] <= 0.0) {
                        continue;
                    }
                    let w = tri.map(|v| to_window(&v.pos));
                    let area = (w[1].0 - w[0].0) * (w[2].1 - w[0].1)
                        - (w[2].0 - w[0].0) * (w[1].1 - w[0].1);
                    if area.abs() < MIN_AREA || !area.is_finite() {
                        continue;
                    }
                    let front = (area > 0.0) == (s.front_face == 0x0901);
                    if s.cull {
                        let culled = match s.cull_mode {
                            0x0404 => front,
                            0x0405 => !front,
                            _ => true,
                        };
                        if culled {
                            continue;
                        }
                    }
                    prims.push(Prim::Tri(Tri {
                        x: [w[0].0, w[1].0, w[2].0],
                        y: [w[0].1, w[1].1, w[2].1],
                        z: [w[0].2, w[1].2, w[2].2],
                        inv_w: [w[0].3, w[1].3, w[2].3],
                        lanes: [
                            tri[0].lanes.clone(),
                            tri[1].lanes.clone(),
                            tri[2].lanes.clone(),
                        ],
                        front,
                        area,
                    }));
                }
            };
            match mode {
                0x0004 => {
                    for t in 0..vs.len() / 3 {
                        push_tri(t * 3, t * 3 + 1, t * 3 + 2, &mut prims);
                    }
                }
                0x0005 => {
                    for t in 0..vs.len().saturating_sub(2) {
                        // Alternate winding so every triangle keeps its facing.
                        if t % 2 == 0 {
                            push_tri(t, t + 1, t + 2, &mut prims);
                        } else {
                            push_tri(t + 1, t, t + 2, &mut prims);
                        }
                    }
                }
                0x0006 => {
                    for t in 1..vs.len().saturating_sub(1) {
                        push_tri(0, t, t + 1, &mut prims);
                    }
                }
                0x0000 => {
                    for v in vs {
                        if v.pos[3] > 0.0 && v.pos[2].abs() <= v.pos[3] {
                            let w = to_window(&v.pos.map(f64::from));
                            prims.push(Prim::Point(Point {
                                x: w.0,
                                y: w.1,
                                z: w.2,
                                size: f64::from(v.point_size.max(1.0)),
                                lanes: v.lanes.clone(),
                            }));
                        }
                    }
                }
                0x0001..=0x0003 => {
                    let pairs: Vec<(usize, usize)> = match mode {
                        0x0001 => (0..vs.len() / 2).map(|i| (i * 2, i * 2 + 1)).collect(),
                        0x0003 => (0..vs.len().saturating_sub(1))
                            .map(|i| (i, i + 1))
                            .collect(),
                        _ => (0..vs.len()).map(|i| (i, (i + 1) % vs.len())).collect(),
                    };
                    for (a, b) in pairs {
                        let (va, vb) = (&vs[a], &vs[b]);
                        let mut poly = [
                            ClipVtx {
                                pos: va.pos.map(f64::from),
                                lanes: va.lanes.clone(),
                            },
                            ClipVtx {
                                pos: vb.pos.map(f64::from),
                                lanes: vb.lanes.clone(),
                            },
                        ];
                        let provoking = vb.lanes.clone();
                        let mut ok = true;
                        for (axis, sign) in [(2usize, 1.0), (2, -1.0)] {
                            // A segment is clipped like a degenerate polygon.
                            let (da, db) = (
                                poly[0].pos[3] + sign * poly[0].pos[axis],
                                poly[1].pos[3] + sign * poly[1].pos[axis],
                            );
                            if da < 0.0 && db < 0.0 {
                                ok = false;
                                break;
                            }
                            if da < 0.0 {
                                poly[0] = lerp_vertex(
                                    &poly[0],
                                    &poly[1],
                                    da / (da - db),
                                    &smooth,
                                    &provoking,
                                );
                            } else if db < 0.0 {
                                poly[1] = lerp_vertex(
                                    &poly[0],
                                    &poly[1],
                                    da / (da - db),
                                    &smooth,
                                    &provoking,
                                );
                            }
                        }
                        if ok && poly[0].pos[3] > 0.0 && poly[1].pos[3] > 0.0 {
                            let (wa, wb) = (to_window(&poly[0].pos), to_window(&poly[1].pos));
                            prims.push(Prim::Line(Segment {
                                a: wa,
                                b: wb,
                                lanes: [poly[0].lanes.clone(), poly[1].lanes.clone()],
                            }));
                        }
                    }
                }
                _ => {
                    self.fail(INVALID_ENUM);
                    return;
                }
            }
        }

        let t_assembly = t0.elapsed();
        let n_prims = prims.len();
        // ---- fragment stage ----
        let mut target = self.take_target();
        let (tw, th) = (target.w as usize, target.h as usize);
        if tw == 0 || th == 0 {
            self.return_target(target);
            return;
        }
        let access = TexAccess {
            textures: &self.textures,
            bindings: &self.tex_bindings,
        };
        let linked = self.programs[&self.current_program]
            .linked
            .as_ref()
            .expect("checked");
        let (clip_x0, clip_y0, clip_x1, clip_y1) = {
            let mut r = (0i64, 0i64, tw as i64, th as i64);
            let v = (
                i64::from(vp[0]),
                i64::from(vp[1]),
                i64::from(vp[0]) + i64::from(vp[2]),
                i64::from(vp[1]) + i64::from(vp[3]),
            );
            r = (r.0.max(v.0), r.1.max(v.1), r.2.min(v.2), r.3.min(v.3));
            if s.scissor_test {
                let sc = (
                    i64::from(s.scissor[0]),
                    i64::from(s.scissor[1]),
                    i64::from(s.scissor[0]) + i64::from(s.scissor[2]),
                    i64::from(s.scissor[1]) + i64::from(s.scissor[3]),
                );
                r = (r.0.max(sc.0), r.1.max(sc.1), r.2.min(sc.2), r.3.min(sc.3));
            }
            r
        };
        let threads = worker_count(tw * th, 20_000).min(th.max(1));
        // Many thin bands taken from a shared queue, so work that is all in the middle
        // of the picture (a globe) still keeps every thread busy.
        let band_rows = th.div_ceil(threads * 8).clamp(1, 32);
        // Each primitive goes to just the bands its rows touch, in submission order.
        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); th.div_ceil(band_rows)];
        for (index, prim) in prims.iter().enumerate() {
            let (lo, hi) = match prim {
                Prim::Tri(t) => (
                    t.y.iter().cloned().fold(f64::INFINITY, f64::min),
                    t.y.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                ),
                Prim::Point(p) => (p.y - p.size / 2.0, p.y + p.size / 2.0),
                Prim::Line(l) => (l.a.1.min(l.b.1), l.a.1.max(l.b.1) + 1.0),
            };
            let first = (lo.floor().max(0.0) as usize) / band_rows;
            let last =
                ((hi.ceil().max(0.0) as usize).saturating_sub(1) / band_rows).min(bins.len() - 1);
            for bin in bins.iter_mut().take(last + 1).skip(first) {
                bin.push(index as u32);
            }
        }
        let bins = &bins;
        let prims = &prims;
        let deadline = std::time::Instant::now() + self.draw_budget;
        let timed_out = &std::sync::atomic::AtomicBool::new(false);
        let smooth = &smooth;
        {
            let (color_chunks, depth_chunks): (Vec<BandColor>, Vec<Option<&mut [f32]>>) = {
                let colors: Vec<BandColor> = match &mut target.color {
                    ColorStore::Bytes(b) => b
                        .chunks_mut(band_rows * tw * 4)
                        .map(BandColor::Bytes)
                        .collect(),
                    ColorStore::Floats(f) => f
                        .chunks_mut(band_rows * tw * 4)
                        .map(BandColor::Floats)
                        .collect(),
                    ColorStore::None => (0..th.div_ceil(band_rows))
                        .map(|_| BandColor::None)
                        .collect(),
                };
                let depths: Vec<Option<&mut [f32]>> = match &mut target.depth {
                    Some(d) => d.chunks_mut(band_rows * tw).map(Some).collect(),
                    None => (0..colors.len()).map(|_| None).collect(),
                };
                (colors, depths)
            };
            let queue =
                std::sync::Mutex::new(color_chunks.into_iter().zip(depth_chunks).enumerate());
            let queue = &queue;
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    let (state, access) = (&s, &access);
                    scope.spawn(move || {
                        let Ok(mut machine) = Machine::new(&linked.fs, access) else {
                            return;
                        };
                        apply_uniforms(&mut machine, linked, false);
                        let fs = &linked.fs;
                        let writes_depth =
                            fs.slot("gl_FragDepth").is_some() && shader_writes(fs, "gl_FragDepth");
                        let mut stage = FragmentStage {
                            machine,
                            state,
                            varyings: &linked.varyings,
                            fs_structs: &fs.structs,
                            color_slot: fs.slot("gl_FragColor"),
                            data_slot: fs.slot("gl_FragData"),
                            out_slot: linked
                                .outputs
                                .iter()
                                .find(|(i, _)| *i == 0)
                                .map(|(_, s)| *s),
                            depth_slot: fs.slot("gl_FragDepth"),
                            frag_coord: fs.slot("gl_FragCoord").expect("built in"),
                            front_facing: fs.slot("gl_FrontFacing").expect("built in"),
                            point_coord: fs.slot("gl_PointCoord").expect("built in"),
                            writes_depth,
                            deadline,
                            expired: false,
                        };
                        let mut lanes_buf: Vec<u32> = Vec::new();
                        loop {
                            let next = queue.lock().ok().and_then(|mut q| q.next());
                            let Some((n, (color, depth))) = next else {
                                break;
                            };
                            if stage.out_of_time() {
                                break;
                            }
                            let y0 = n * band_rows;
                            let rows = band_rows.min(th - y0);
                            let mut band = Band {
                                y0,
                                w: tw,
                                color,
                                depth,
                            };
                            let band_clip = (
                                clip_x0,
                                clip_y0.max(y0 as i64),
                                clip_x1,
                                clip_y1.min((y0 + rows) as i64),
                            );
                            for &index in &bins[n] {
                                match &prims[index as usize] {
                                    Prim::Tri(t) => raster_triangle(
                                        t,
                                        band_clip,
                                        &mut stage,
                                        &mut band,
                                        smooth,
                                        &mut lanes_buf,
                                    ),
                                    Prim::Point(p) => {
                                        raster_point(p, band_clip, &mut stage, &mut band)
                                    }
                                    Prim::Line(l) => raster_line(
                                        l,
                                        band_clip,
                                        &mut stage,
                                        &mut band,
                                        smooth,
                                        &mut lanes_buf,
                                    ),
                                }
                            }
                        }
                        if stage.expired {
                            timed_out.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                }
            });
        }
        self.return_target(target);
        if profile {
            eprintln!(
                "GLPROFILE vertices={} prims={n_prims} vertex={t_vertex:?} assembly={:?} fragment={:?}",
                total,
                t_assembly - t_vertex,
                t0.elapsed() - t_assembly
            );
        }
        if timed_out.load(std::sync::atomic::Ordering::Relaxed) {
            self.slow_draws += 1;
        }
    }
}

/// True when the shader's source assigns `name` (so depth must be tested after
/// shading rather than before).
fn shader_writes(program: &super::glsl::interp::Program, name: &str) -> bool {
    program.writes(name)
}

fn worker_count(items: usize, per_thread: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(16);
    (items / per_thread.max(1)).clamp(1, cores)
}

fn apply_uniforms(m: &mut Machine<'_>, linked: &Linked, _vertex: bool) {
    for (name, value) in &linked.values {
        m.set_by_name(name, value.clone());
    }
}

fn fetch_attrib(
    vao: &Vao,
    buffers: &HashMap<u32, Vec<u8>>,
    location: usize,
    index: u32,
    instance: u32,
    ty: &Ty,
) -> V {
    let Some(a) = vao.attribs.get(location) else {
        return zero(ty, &[]);
    };
    let mut c: [f32; 4];
    let mut raw = [0i64; 4];
    let mut have_raw = false;
    if a.enabled {
        if let Some(data) = buffers.get(&a.buffer) {
            let size = match a.ty {
                0x1400 | 0x1401 => 1usize,
                0x1402 | 0x1403 | 0x140B => 2,
                _ => 4,
            };
            let stride = if a.stride == 0 {
                size * a.size as usize
            } else {
                a.stride as usize
            };
            let element = instance.checked_div(a.divisor).unwrap_or(index);
            let base = a.offset as usize + stride * element as usize;
            c = [0.0, 0.0, 0.0, 1.0];
            for k in 0..a.size as usize {
                let at = base + k * size;
                let Some(bytes) = data.get(at..at + size) else {
                    continue;
                };
                let (value, int) = match a.ty {
                    0x1400 => {
                        let v = bytes[0] as i8;
                        (
                            if a.normalized {
                                (f32::from(v) / 127.0).max(-1.0)
                            } else {
                                f32::from(v)
                            },
                            i64::from(v),
                        )
                    }
                    0x1401 => (
                        if a.normalized {
                            f32::from(bytes[0]) / 255.0
                        } else {
                            f32::from(bytes[0])
                        },
                        i64::from(bytes[0]),
                    ),
                    0x1402 => {
                        let v = i16::from_le_bytes([bytes[0], bytes[1]]);
                        (
                            if a.normalized {
                                (f32::from(v) / 32767.0).max(-1.0)
                            } else {
                                f32::from(v)
                            },
                            i64::from(v),
                        )
                    }
                    0x1403 => {
                        let v = u16::from_le_bytes([bytes[0], bytes[1]]);
                        (
                            if a.normalized {
                                f32::from(v) / 65535.0
                            } else {
                                f32::from(v)
                            },
                            i64::from(v),
                        )
                    }
                    0x1404 => {
                        let v = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                        (v as f32, i64::from(v))
                    }
                    0x1405 => {
                        let v = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                        (v as f32, i64::from(v))
                    }
                    0x140B => {
                        let v = u16::from_le_bytes([bytes[0], bytes[1]]);
                        (half(v), 0)
                    }
                    _ => {
                        let v = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                        (v, 0)
                    }
                };
                c[k] = value;
                raw[k] = int;
            }
            have_raw = a.integer;
        } else {
            c = a.constant;
        }
    } else {
        c = a.constant;
    }
    match ty {
        Ty::Scalar(Scalar::Float) => V::f(c[0]),
        Ty::Scalar(Scalar::Int) => V::i(if have_raw { raw[0] as i32 } else { c[0] as i32 }),
        Ty::Scalar(Scalar::UInt) => V::u(if have_raw { raw[0] as u32 } else { c[0] as u32 }),
        Ty::Vec(Scalar::Float, n) => V::vec(&c[..*n as usize]),
        Ty::Vec(k, n) => V::of_kind(
            *k,
            &(0..*n as usize)
                .map(|i| {
                    if have_raw {
                        raw[i] as u32
                    } else {
                        c[i] as i32 as u32
                    }
                })
                .collect::<Vec<_>>(),
        ),
        Ty::Mat(cols, rows) => {
            // A matrix attribute spans one location per column, each a vector.
            let mut d = [0.0f32; 16];
            for column in 0..(*cols as usize).min(4) {
                let col = fetch_attrib(
                    vao,
                    buffers,
                    location + column,
                    index,
                    instance,
                    &Ty::Vec(Scalar::Float, *rows),
                );
                if let Some(values) = col.floats() {
                    for (r, v) in values.iter().enumerate().take(4) {
                        d[column * *rows as usize + r] = *v;
                    }
                }
            }
            V::Mat {
                cols: *cols,
                rows: *rows,
                d,
            }
        }
        other => zero(other, &[]),
    }
}

fn half(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let frac = f32::from(h & 0x3ff);
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => sign * (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

fn raster_triangle(
    t: &Tri,
    clip: (i64, i64, i64, i64),
    stage: &mut FragmentStage<'_>,
    band: &mut Band<'_>,
    smooth: &[bool],
    lanes: &mut Vec<u32>,
) {
    let min_x =
        t.x.iter()
            .cloned()
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(clip.0 as f64) as i64;
    let max_x =
        t.x.iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(clip.2 as f64) as i64;
    let min_y =
        t.y.iter()
            .cloned()
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(clip.1 as f64) as i64;
    let max_y =
        t.y.iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(clip.3 as f64) as i64;
    if min_x >= max_x || min_y >= max_y {
        return;
    }
    let sign = if t.area > 0.0 { 1.0 } else { -1.0 };
    let area = t.area * sign;
    // Edge i is opposite vertex i.
    let edge = |a: usize, b: usize, px: f64, py: f64| -> f64 {
        sign * ((t.x[b] - t.x[a]) * (py - t.y[a]) - (t.y[b] - t.y[a]) * (px - t.x[a]))
    };
    let top_left = |a: usize, b: usize| -> bool {
        let (dx, dy) = ((t.x[b] - t.x[a]) * sign, (t.y[b] - t.y[a]) * sign);
        // After orienting counter-clockwise, a left edge goes down, a top edge goes left... in
        // GL's y-up window space those are `dy < 0` and `dy == 0 && dx < 0`.
        dy < 0.0 || (dy == 0.0 && dx < 0.0)
    };
    let (tl0, tl1, tl2) = (top_left(1, 2), top_left(2, 0), top_left(0, 1));
    let nlanes = t.lanes[0].len();
    for y in min_y..max_y {
        if stage.out_of_time() {
            return;
        }
        for x in min_x..max_x {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (e0, e1, e2) = (edge(1, 2, px, py), edge(2, 0, px, py), edge(0, 1, px, py));
            if !((e0 > 0.0 || (e0 == 0.0 && tl0))
                && (e1 > 0.0 || (e1 == 0.0 && tl1))
                && (e2 > 0.0 || (e2 == 0.0 && tl2)))
            {
                continue;
            }
            let b = [e0 / area, e1 / area, e2 / area];
            let z = (b[0] * t.z[0] + b[1] * t.z[1] + b[2] * t.z[2]) as f32;
            if !(0.0..=1.0).contains(&z) && !stage.state.depth_test {
                // Out of range depth is clipped by the depth range only when testing.
            }
            // Perspective-correct weights.
            let pw = [b[0] * t.inv_w[0], b[1] * t.inv_w[1], b[2] * t.inv_w[2]];
            let sum = pw[0] + pw[1] + pw[2];
            let l = [pw[0] / sum, pw[1] / sum, pw[2] / sum];
            lanes.clear();
            for i in 0..nlanes {
                if smooth[i] {
                    let v = l[0] * f64::from(f32::from_bits(t.lanes[0][i]))
                        + l[1] * f64::from(f32::from_bits(t.lanes[1][i]))
                        + l[2] * f64::from(f32::from_bits(t.lanes[2][i]));
                    lanes.push((v as f32).to_bits());
                } else {
                    lanes.push(t.lanes[2][i]);
                }
            }
            stage.run(
                band,
                &Fragment {
                    x: x as usize,
                    y: y as usize,
                    z,
                    front: t.front,
                    lanes,
                    point_coord: [0.0, 0.0],
                },
            );
        }
    }
}

fn raster_point(
    p: &Point,
    clip: (i64, i64, i64, i64),
    stage: &mut FragmentStage<'_>,
    band: &mut Band<'_>,
) {
    let half = p.size / 2.0;
    let (x0, x1) = (
        (p.x - half).floor().max(clip.0 as f64) as i64,
        (p.x + half).ceil().min(clip.2 as f64) as i64,
    );
    let (y0, y1) = (
        (p.y - half).floor().max(clip.1 as f64) as i64,
        (p.y + half).ceil().min(clip.3 as f64) as i64,
    );
    for y in y0..y1 {
        if stage.out_of_time() {
            return;
        }
        for x in x0..x1 {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            if (px - p.x).abs() > half || (py - p.y).abs() > half {
                continue;
            }
            let coord = [
                ((px - p.x) / p.size + 0.5) as f32,
                (0.5 - (py - p.y) / p.size) as f32,
            ];
            stage.run(
                band,
                &Fragment {
                    x: x as usize,
                    y: y as usize,
                    z: p.z as f32,
                    front: true,
                    lanes: &p.lanes,
                    point_coord: coord,
                },
            );
        }
    }
}

fn raster_line(
    l: &Segment,
    clip: (i64, i64, i64, i64),
    stage: &mut FragmentStage<'_>,
    band: &mut Band<'_>,
    smooth: &[bool],
    lanes: &mut Vec<u32>,
) {
    let (dx, dy) = (l.b.0 - l.a.0, l.b.1 - l.a.1);
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0);
    let mut last = (i64::MIN, i64::MIN);
    for i in 0..=(steps as i64) {
        let t = i as f64 / steps;
        let (x, y) = (
            (l.a.0 + dx * t).floor() as i64,
            (l.a.1 + dy * t).floor() as i64,
        );
        if (x, y) == last || x < clip.0 || x >= clip.2 || y < clip.1 || y >= clip.3 {
            continue;
        }
        last = (x, y);
        let z = (l.a.2 + (l.b.2 - l.a.2) * t) as f32;
        // Perspective-correct along the segment.
        let (wa, wb) = (l.a.3 * (1.0 - t), l.b.3 * t);
        let tp = wb / (wa + wb);
        lanes.clear();
        for k in 0..l.lanes[0].len() {
            if smooth[k] {
                let (a, b) = (
                    f64::from(f32::from_bits(l.lanes[0][k])),
                    f64::from(f32::from_bits(l.lanes[1][k])),
                );
                lanes.push(((a + (b - a) * tp) as f32).to_bits());
            } else {
                lanes.push(l.lanes[1][k]);
            }
        }
        stage.run(
            band,
            &Fragment {
                x: x as usize,
                y: y as usize,
                z,
                front: true,
                lanes,
                point_coord: [0.0, 0.0],
            },
        );
    }
}
