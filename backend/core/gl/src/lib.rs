// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A software WebGL: GLSL ES shaders interpreted per vertex and per fragment,
//! rasterized into RGBA8 buffers. Pure computation on bytes, no I/O.
//!
//! [`Context`] is the whole of a `WebGLRenderingContext` / `WebGL2RenderingContext`
//! as plain Rust: resources are integer names, calls are methods. The
//! JavaScript binding layer owns nothing but those names.

// Pixel, lane and matrix loops index several parallel arrays by one counter, and a
// GL entry point mirrors the API's own long parameter lists.
#![allow(
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::manual_memcpy
)]

mod draw;
pub mod glsl;
pub mod tex;

use glsl::ast::{Scalar, Storage, Ty};
use glsl::interp::Program;
use glsl::value::{zero, V};
use std::collections::HashMap;
use tex::{gl as g, Level, Texture};

pub const ARRAY_BUFFER: u32 = 0x8892;
pub const ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
pub const TEXTURE_2D: u32 = 0x0DE1;
pub const TEXTURE_CUBE_MAP: u32 = 0x8513;
pub const TEXTURE_CUBE_MAP_POSITIVE_X: u32 = 0x8515;
pub const FRAMEBUFFER: u32 = 0x8D40;
pub const READ_FRAMEBUFFER: u32 = 0x8CA8;
pub const DRAW_FRAMEBUFFER: u32 = 0x8CA9;
pub const RENDERBUFFER: u32 = 0x8D41;
pub const VERTEX_SHADER: u32 = 0x8B31;
pub const FRAGMENT_SHADER: u32 = 0x8B30;
pub const COLOR_ATTACHMENT0: u32 = 0x8CE0;
pub const DEPTH_ATTACHMENT: u32 = 0x8D00;
pub const DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
pub const FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
pub const FRAMEBUFFER_INCOMPLETE_ATTACHMENT: u32 = 0x8CD6;
pub const FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT: u32 = 0x8CD7;

pub const NO_ERROR: u32 = 0;
pub const INVALID_ENUM: u32 = 0x0500;
pub const INVALID_VALUE: u32 = 0x0501;
pub const INVALID_OPERATION: u32 = 0x0502;

pub const DEPTH_TEST: u32 = 0x0B71;
pub const BLEND: u32 = 0x0BE2;
pub const CULL_FACE: u32 = 0x0B44;
pub const SCISSOR_TEST: u32 = 0x0C11;
pub const STENCIL_TEST: u32 = 0x0B90;
pub const POLYGON_OFFSET_FILL: u32 = 0x8037;

pub const COLOR_BUFFER_BIT: u32 = 0x4000;
pub const DEPTH_BUFFER_BIT: u32 = 0x0100;
pub const STENCIL_BUFFER_BIT: u32 = 0x0400;

#[derive(Clone, Debug)]
pub struct Attrib {
    pub enabled: bool,
    pub size: u32,
    pub ty: u32,
    pub normalized: bool,
    pub stride: u32,
    pub offset: u32,
    pub buffer: u32,
    pub divisor: u32,
    /// `vertexAttribIPointer`: integers stay integers.
    pub integer: bool,
    /// The value used while the array is disabled.
    pub constant: [f32; 4],
}

impl Default for Attrib {
    fn default() -> Self {
        Attrib {
            enabled: false,
            size: 4,
            ty: 0x1406,
            normalized: false,
            stride: 0,
            offset: 0,
            buffer: 0,
            divisor: 0,
            integer: false,
            constant: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Vao {
    pub attribs: Vec<Attrib>,
    pub element_buffer: u32,
}

impl Vao {
    fn new() -> Vao {
        Vao {
            attribs: vec![Attrib::default(); 16],
            element_buffer: 0,
        }
    }
}

struct ShaderObj {
    kind: u32,
    source: String,
    compiled: Option<Result<Program, String>>,
}

/// One active uniform, as `getActiveUniform` reports it.
#[derive(Clone, Debug)]
pub struct UniformInfo {
    pub name: String,
    pub ty: Ty,
    /// Elements: 1 unless an array.
    pub size: u32,
    pub base_location: i32,
}

#[derive(Clone, Debug)]
pub struct AttribInfo {
    pub name: String,
    pub ty: Ty,
    pub location: u32,
    pub vs_slot: usize,
}

#[derive(Clone, Debug)]
pub struct Varying {
    pub vs_slot: usize,
    pub fs_slot: usize,
    pub ty: Ty,
    pub flat: bool,
}

struct Linked {
    vs: Program,
    fs: Program,
    uniforms: Vec<UniformInfo>,
    attributes: Vec<AttribInfo>,
    varyings: Vec<Varying>,
    /// Fragment outputs: (color index, fs slot).
    outputs: Vec<(usize, usize)>,
    /// Current uniform values by name.
    values: HashMap<String, V>,
}

struct ProgramObj {
    shaders: Vec<u32>,
    linked: Option<Linked>,
    log: String,
    bound_attribs: HashMap<String, u32>,
}

#[derive(Clone, Debug)]
pub struct Renderbuffer {
    pub w: u32,
    pub h: u32,
    pub format: u32,
    pub color: Vec<f32>,
    pub depth: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Attachment {
    None,
    Texture { id: u32, face: usize, level: usize },
    Renderbuffer(u32),
}

#[derive(Clone, Debug)]
pub struct Fbo {
    pub color: Attachment,
    pub depth: Attachment,
}

#[derive(Clone, Debug)]
pub struct State {
    pub viewport: [i32; 4],
    pub scissor: [i32; 4],
    pub scissor_test: bool,
    pub depth_test: bool,
    pub depth_func: u32,
    pub depth_mask: bool,
    pub depth_range: [f32; 2],
    pub blend: bool,
    pub blend_src_rgb: u32,
    pub blend_dst_rgb: u32,
    pub blend_src_alpha: u32,
    pub blend_dst_alpha: u32,
    pub blend_eq_rgb: u32,
    pub blend_eq_alpha: u32,
    pub blend_color: [f32; 4],
    pub cull: bool,
    pub cull_mode: u32,
    pub front_face: u32,
    pub color_mask: [bool; 4],
    pub clear_color: [f32; 4],
    pub clear_depth: f32,
    pub line_width: f32,
}

pub struct Context {
    pub width: u32,
    pub height: u32,
    /// The default framebuffer: RGBA8, rows from the bottom (GL's origin).
    color: Vec<u8>,
    depth: Vec<f32>,
    pub premultiplied_alpha: bool,
    pub has_alpha: bool,
    next_id: u32,
    buffers: HashMap<u32, Vec<u8>>,
    array_buffer: u32,
    vaos: HashMap<u32, Vao>,
    vao: u32,
    textures: HashMap<u32, Texture>,
    tex_bindings: HashMap<(usize, u32), u32>,
    active_unit: usize,
    shaders: HashMap<u32, ShaderObj>,
    programs: HashMap<u32, ProgramObj>,
    current_program: u32,
    framebuffers: HashMap<u32, Fbo>,
    renderbuffers: HashMap<u32, Renderbuffer>,
    draw_fb: u32,
    read_fb: u32,
    bound_rb: u32,
    pub state: State,
    pub error: u32,
    pub flip_y: bool,
    pub premultiply: bool,
    /// The longest one draw call may shade fragments for.
    pub draw_budget: std::time::Duration,
    /// Draws so far that ran out of that time.
    pub slow_draws: u32,
}

impl Context {
    pub fn new(width: u32, height: u32) -> Context {
        let (w, h) = (width.max(1), height.max(1));
        let mut vaos = HashMap::new();
        vaos.insert(0, Vao::new());
        Context {
            width: w,
            height: h,
            color: vec![0; (w * h * 4) as usize],
            depth: vec![1.0; (w * h) as usize],
            premultiplied_alpha: true,
            has_alpha: true,
            next_id: 1,
            buffers: HashMap::new(),
            array_buffer: 0,
            vaos,
            vao: 0,
            textures: HashMap::new(),
            tex_bindings: HashMap::new(),
            active_unit: 0,
            shaders: HashMap::new(),
            programs: HashMap::new(),
            current_program: 0,
            framebuffers: HashMap::new(),
            renderbuffers: HashMap::new(),
            draw_fb: 0,
            read_fb: 0,
            bound_rb: 0,
            state: State {
                viewport: [0, 0, w as i32, h as i32],
                scissor: [0, 0, w as i32, h as i32],
                scissor_test: false,
                depth_test: false,
                depth_func: 0x0201,
                depth_mask: true,
                depth_range: [0.0, 1.0],
                blend: false,
                blend_src_rgb: 1,
                blend_dst_rgb: 0,
                blend_src_alpha: 1,
                blend_dst_alpha: 0,
                blend_eq_rgb: 0x8006,
                blend_eq_alpha: 0x8006,
                blend_color: [0.0; 4],
                cull: false,
                cull_mode: 0x0405,
                front_face: 0x0901,
                color_mask: [true; 4],
                clear_color: [0.0; 4],
                clear_depth: 1.0,
                line_width: 1.0,
            },
            error: NO_ERROR,
            flip_y: false,
            premultiply: false,
            draw_budget: std::time::Duration::from_millis(1500),
            slow_draws: 0,
        }
    }

    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn fail(&mut self, code: u32) {
        if self.error == NO_ERROR {
            self.error = code;
        }
    }

    pub fn get_error(&mut self) -> u32 {
        std::mem::replace(&mut self.error, NO_ERROR)
    }

    /// Resizes the drawing buffer (which clears it).
    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        self.width = w;
        self.height = h;
        self.color = vec![0; (w * h * 4) as usize];
        self.depth = vec![1.0; (w * h) as usize];
        self.state.viewport = [0, 0, w as i32, h as i32];
        self.state.scissor = [0, 0, w as i32, h as i32];
    }

    /// The drawing buffer as straight-alpha RGBA, top row first (how pictures
    /// are stored), ready to composite onto a page.
    pub fn picture(&self) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let src = &self.color[(h - 1 - y) * w * 4..(h - y) * w * 4];
            let dst = &mut out[y * w * 4..(y + 1) * w * 4];
            for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                let a = u32::from(s[3]);
                if !self.has_alpha {
                    d.copy_from_slice(&[s[0], s[1], s[2], 255]);
                } else if self.premultiplied_alpha && a != 0 && a != 255 {
                    for c in 0..3 {
                        d[c] = ((u32::from(s[c]) * 255 + a / 2) / a).min(255) as u8;
                    }
                    d[3] = s[3];
                } else {
                    d.copy_from_slice(s);
                }
            }
        }
        out
    }

    // ---------------------------------------------------------- buffers

    pub fn create_buffer(&mut self) -> u32 {
        let id = self.id();
        self.buffers.insert(id, Vec::new());
        id
    }
    pub fn delete_buffer(&mut self, id: u32) {
        self.buffers.remove(&id);
    }
    pub fn bind_buffer(&mut self, target: u32, id: u32) {
        match target {
            ARRAY_BUFFER => self.array_buffer = id,
            ELEMENT_ARRAY_BUFFER => {
                let v = self.vao;
                self.vaos.entry(v).or_default().element_buffer = id;
            }
            _ => self.fail(INVALID_ENUM),
        }
    }
    fn bound_buffer(&self, target: u32) -> u32 {
        match target {
            ARRAY_BUFFER => self.array_buffer,
            _ => self.vaos.get(&self.vao).map_or(0, |v| v.element_buffer),
        }
    }
    pub fn buffer_data(&mut self, target: u32, data: Vec<u8>) {
        let id = self.bound_buffer(target);
        match self.buffers.get_mut(&id) {
            Some(b) => *b = data,
            None => self.fail(INVALID_OPERATION),
        }
    }
    pub fn buffer_sub_data(&mut self, target: u32, offset: usize, data: &[u8]) {
        let id = self.bound_buffer(target);
        match self.buffers.get_mut(&id) {
            Some(b) if offset + data.len() <= b.len() => {
                b[offset..offset + data.len()].copy_from_slice(data)
            }
            Some(_) => self.fail(INVALID_VALUE),
            None => self.fail(INVALID_OPERATION),
        }
    }

    // ------------------------------------------------------------- VAOs

    pub fn create_vertex_array(&mut self) -> u32 {
        let id = self.id();
        self.vaos.insert(id, Vao::new());
        id
    }
    pub fn delete_vertex_array(&mut self, id: u32) {
        if id != 0 {
            self.vaos.remove(&id);
            if self.vao == id {
                self.vao = 0;
            }
        }
    }
    pub fn bind_vertex_array(&mut self, id: u32) {
        if id == 0 || self.vaos.contains_key(&id) {
            self.vao = id;
        } else {
            self.fail(INVALID_OPERATION);
        }
    }
    fn attrib_mut(&mut self, index: u32) -> Option<&mut Attrib> {
        let v = self.vao;
        self.vaos.get_mut(&v)?.attribs.get_mut(index as usize)
    }
    pub fn enable_vertex_attrib_array(&mut self, index: u32, enabled: bool) {
        match self.attrib_mut(index) {
            Some(a) => a.enabled = enabled,
            None => self.fail(INVALID_VALUE),
        }
    }
    pub fn vertex_attrib_pointer(
        &mut self,
        index: u32,
        size: u32,
        ty: u32,
        normalized: bool,
        stride: u32,
        offset: u32,
        integer: bool,
    ) {
        let buffer = self.array_buffer;
        match self.attrib_mut(index) {
            Some(a) if (1..=4).contains(&size) => {
                a.size = size;
                a.ty = ty;
                a.normalized = normalized;
                a.stride = stride;
                a.offset = offset;
                a.buffer = buffer;
                a.integer = integer;
            }
            _ => self.fail(INVALID_VALUE),
        }
    }
    pub fn vertex_attrib_divisor(&mut self, index: u32, divisor: u32) {
        match self.attrib_mut(index) {
            Some(a) => a.divisor = divisor,
            None => self.fail(INVALID_VALUE),
        }
    }
    pub fn vertex_attrib_constant(&mut self, index: u32, value: [f32; 4]) {
        match self.attrib_mut(index) {
            Some(a) => a.constant = value,
            None => self.fail(INVALID_VALUE),
        }
    }

    // ---------------------------------------------------------- shaders

    pub fn create_shader(&mut self, kind: u32) -> u32 {
        if kind != VERTEX_SHADER && kind != FRAGMENT_SHADER {
            self.fail(INVALID_ENUM);
            return 0;
        }
        let id = self.id();
        self.shaders.insert(
            id,
            ShaderObj {
                kind,
                source: String::new(),
                compiled: None,
            },
        );
        id
    }
    pub fn shader_source(&mut self, id: u32, source: &str) {
        if let Some(s) = self.shaders.get_mut(&id) {
            s.source = source.to_string();
        }
    }
    pub fn compile_shader(&mut self, id: u32) {
        if let Some(s) = self.shaders.get_mut(&id) {
            s.compiled = Some(Program::compile(&s.source));
        }
    }
    pub fn shader_compiled(&self, id: u32) -> bool {
        matches!(
            self.shaders.get(&id).and_then(|s| s.compiled.as_ref()),
            Some(Ok(_))
        )
    }
    pub fn shader_log(&self, id: u32) -> String {
        match self.shaders.get(&id).and_then(|s| s.compiled.as_ref()) {
            Some(Err(e)) => format!("ERROR: {e}\n"),
            _ => String::new(),
        }
    }
    pub fn shader_source_of(&self, id: u32) -> String {
        self.shaders
            .get(&id)
            .map(|s| s.source.clone())
            .unwrap_or_default()
    }
    pub fn delete_shader(&mut self, id: u32) {
        self.shaders.remove(&id);
    }

    // --------------------------------------------------------- programs

    pub fn create_program(&mut self) -> u32 {
        let id = self.id();
        self.programs.insert(
            id,
            ProgramObj {
                shaders: Vec::new(),
                linked: None,
                log: String::new(),
                bound_attribs: HashMap::new(),
            },
        );
        id
    }
    pub fn attach_shader(&mut self, program: u32, shader: u32) {
        if let Some(p) = self.programs.get_mut(&program) {
            if !p.shaders.contains(&shader) {
                p.shaders.push(shader);
            }
        }
    }
    pub fn detach_shader(&mut self, program: u32, shader: u32) {
        if let Some(p) = self.programs.get_mut(&program) {
            p.shaders.retain(|s| *s != shader);
        }
    }
    pub fn bind_attrib_location(&mut self, program: u32, index: u32, name: &str) {
        if let Some(p) = self.programs.get_mut(&program) {
            p.bound_attribs.insert(name.to_string(), index);
        }
    }
    pub fn delete_program(&mut self, id: u32) {
        self.programs.remove(&id);
        if self.current_program == id {
            self.current_program = 0;
        }
    }
    pub fn use_program(&mut self, id: u32) {
        if id == 0 || self.programs.contains_key(&id) {
            self.current_program = id;
        } else {
            self.fail(INVALID_OPERATION);
        }
    }
    pub fn program_linked(&self, id: u32) -> bool {
        self.programs.get(&id).is_some_and(|p| p.linked.is_some())
    }
    pub fn program_log(&self, id: u32) -> String {
        self.programs
            .get(&id)
            .map(|p| p.log.clone())
            .unwrap_or_default()
    }

    pub fn link_program(&mut self, id: u32) {
        let Some(p) = self.programs.get(&id) else {
            return;
        };
        let mut vs = None;
        let mut fs = None;
        for s in &p.shaders {
            match self.shaders.get(s) {
                Some(ShaderObj {
                    kind: VERTEX_SHADER,
                    compiled: Some(Ok(prog)),
                    ..
                }) => vs = Some(prog.clone()),
                Some(ShaderObj {
                    kind: FRAGMENT_SHADER,
                    compiled: Some(Ok(prog)),
                    ..
                }) => fs = Some(prog.clone()),
                Some(_) => {
                    let p = self.programs.get_mut(&id).expect("checked");
                    p.linked = None;
                    p.log = "ERROR: a shader is not compiled\n".into();
                    return;
                }
                None => {}
            }
        }
        let bound = p.bound_attribs.clone();
        let result = match (vs, fs) {
            (Some(vs), Some(fs)) => link(vs, fs, &bound),
            _ => Err("a program needs a vertex and a fragment shader".to_string()),
        };
        let p = self.programs.get_mut(&id).expect("checked");
        match result {
            Ok(linked) => {
                p.linked = Some(linked);
                p.log.clear();
            }
            Err(e) => {
                p.linked = None;
                p.log = format!("ERROR: {e}\n");
            }
        }
    }

    pub fn active_uniforms(&self, program: u32) -> Vec<UniformInfo> {
        self.programs
            .get(&program)
            .and_then(|p| p.linked.as_ref())
            .map(|l| l.uniforms.clone())
            .unwrap_or_default()
    }
    pub fn active_attribs(&self, program: u32) -> Vec<AttribInfo> {
        self.programs
            .get(&program)
            .and_then(|p| p.linked.as_ref())
            .map(|l| l.attributes.clone())
            .unwrap_or_default()
    }
    pub fn attrib_location(&self, program: u32, name: &str) -> i32 {
        self.programs
            .get(&program)
            .and_then(|p| p.linked.as_ref())
            .and_then(|l| l.attributes.iter().find(|a| a.name == name))
            .map_or(-1, |a| a.location as i32)
    }
    pub fn uniform_location(&self, program: u32, name: &str) -> i32 {
        let Some(l) = self.programs.get(&program).and_then(|p| p.linked.as_ref()) else {
            return -1;
        };
        // `name` or `name[i]`.
        let (base, element) = match name.split_once('[') {
            Some((b, rest)) => (b, rest.trim_end_matches(']').parse::<i32>().ok()),
            None => (name, None),
        };
        for u in &l.uniforms {
            let u_base = u.name.trim_end_matches("[0]");
            if u_base == base {
                return match element {
                    Some(i) if i >= 0 && (i as u32) < u.size => u.base_location + i,
                    None => u.base_location,
                    _ => -1,
                };
            }
            // A field of a struct uniform: `light.color`.
            if base.starts_with(&format!("{u_base}.")) || base.starts_with(&format!("{u_base}[")) {
                return u.base_location;
            }
        }
        -1
    }

    /// Sets uniform `location` of the current program from raw component values.
    pub fn set_uniform(
        &mut self,
        location: i32,
        lanes: &[f32],
        ints: Option<&[i32]>,
        matrix: Option<(usize, usize)>,
    ) {
        if location < 0 {
            return;
        }
        let Some(p) = self.programs.get_mut(&self.current_program) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let Some(l) = p.linked.as_mut() else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let Some(u) = l
            .uniforms
            .iter()
            .find(|u| location >= u.base_location && location < u.base_location + u.size as i32)
            .cloned()
        else {
            return;
        };
        let element = (location - u.base_location) as usize;
        let name = u.name.trim_end_matches("[0]").to_string();
        let elem_ty = match &u.ty {
            Ty::Array(inner, _) => (**inner).clone(),
            t => t.clone(),
        };
        let count_per = match (&elem_ty, matrix) {
            (_, Some((c, r))) => c * r,
            (Ty::Scalar(_), _) => 1,
            (Ty::Vec(_, n), _) => *n as usize,
            (Ty::Mat(c, r), _) => (*c * *r) as usize,
            (Ty::Sampler(_), _) => 1,
            _ => 1,
        };
        let total = ints.map_or(lanes.len(), <[i32]>::len);
        let elements = (total / count_per.max(1)).max(1);
        let make = |i: usize| -> V {
            let f = |j: usize| lanes.get(i * count_per + j).copied().unwrap_or(0.0);
            let n = |j: usize| {
                ints.and_then(|a| a.get(i * count_per + j).copied())
                    .unwrap_or(0)
            };
            match &elem_ty {
                Ty::Scalar(Scalar::Float) => V::f(f(0)),
                Ty::Scalar(Scalar::Int) => V::i(ints.map_or(f(0) as i32, |_| n(0))),
                Ty::Scalar(Scalar::UInt) => V::u(ints.map_or(f(0) as u32, |_| n(0) as u32)),
                Ty::Scalar(Scalar::Bool) => V::b(ints.map_or(f(0) != 0.0, |_| n(0) != 0)),
                Ty::Vec(Scalar::Float, k) => V::vec(&(0..*k as usize).map(f).collect::<Vec<_>>()),
                Ty::Vec(kind, k) => V::of_kind(
                    *kind,
                    &(0..*k as usize)
                        .map(|j| ints.map_or(f(j) as i32, |_| n(j)) as u32)
                        .collect::<Vec<_>>(),
                ),
                Ty::Mat(c, r) => {
                    let mut d = [0.0f32; 16];
                    for (j, slot) in d.iter_mut().enumerate().take((*c * *r) as usize) {
                        *slot = f(j);
                    }
                    V::Mat {
                        cols: *c,
                        rows: *r,
                        d,
                    }
                }
                Ty::Sampler(kind) => V::Sampler(n(0).max(0) as u32, *kind),
                _ => V::Void,
            }
        };
        let sizes = u.size as usize;
        if matches!(u.ty, Ty::Array(..)) {
            let entry = l
                .values
                .entry(name)
                .or_insert_with(|| V::Arr((0..sizes).map(|_| zero(&elem_ty, &[])).collect()));
            if let V::Arr(items) = entry {
                for i in 0..elements {
                    if element + i < items.len() {
                        items[element + i] = make(i);
                    }
                }
            }
        } else {
            l.values.insert(name, make(0));
        }
    }

    // --------------------------------------------------------- textures

    pub fn create_texture(&mut self) -> u32 {
        let id = self.id();
        // The target (2D or cube) is fixed by the first bind.
        self.textures.insert(id, Texture::new(false));
        id
    }
    pub fn delete_texture(&mut self, id: u32) {
        self.textures.remove(&id);
        self.tex_bindings.retain(|_, v| *v != id);
    }
    pub fn active_texture(&mut self, unit: u32) {
        self.active_unit = unit as usize;
    }
    pub fn bind_texture(&mut self, target: u32, id: u32) {
        if target != TEXTURE_2D
            && target != TEXTURE_CUBE_MAP
            && target != 0x806F
            && target != 0x8C1A
        {
            self.fail(INVALID_ENUM);
            return;
        }
        if id != 0 {
            let Some(t) = self.textures.get_mut(&id) else {
                self.fail(INVALID_OPERATION);
                return;
            };
            if target == TEXTURE_CUBE_MAP && !t.cube {
                *t = Texture::new(true);
            }
        }
        self.tex_bindings.insert((self.active_unit, target), id);
    }
    fn bound_texture_id(&self, target: u32) -> u32 {
        let target =
            if (TEXTURE_CUBE_MAP_POSITIVE_X..TEXTURE_CUBE_MAP_POSITIVE_X + 6).contains(&target) {
                TEXTURE_CUBE_MAP
            } else {
                target
            };
        self.tex_bindings
            .get(&(self.active_unit, target))
            .copied()
            .unwrap_or(0)
    }
    pub fn tex_parameter(&mut self, target: u32, name: u32, value: u32) {
        let id = self.bound_texture_id(target);
        let Some(t) = self.textures.get_mut(&id) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        match name {
            0x2800 => t.mag_filter = value,
            0x2801 => t.min_filter = value,
            0x2802 => t.wrap_s = value,
            0x2803 => t.wrap_t = value,
            0x884C => t.compare = value == 0x884E,
            _ => {}
        }
    }
    /// `texImage2D` with pixel bytes (`None` allocates blank storage).
    #[allow(clippy::too_many_arguments)]
    pub fn tex_image_2d(
        &mut self,
        target: u32,
        level: u32,
        internal: u32,
        w: u32,
        h: u32,
        format: u32,
        ty: u32,
        bytes: Option<&[u8]>,
    ) {
        let id = self.bound_texture_id(target);
        let (flip, premultiply) = (self.flip_y, self.premultiply);
        let Some(t) = self.textures.get_mut(&id) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        if w > 8192 || h > 8192 {
            self.fail(INVALID_VALUE);
            return;
        }
        let face = if t.cube {
            target.wrapping_sub(TEXTURE_CUBE_MAP_POSITIVE_X) as usize
        } else {
            0
        };
        let data = match bytes {
            Some(b) => match tex::decode(w, h, format, ty, internal, b, flip, premultiply) {
                Some(l) => l,
                None => {
                    self.fail(INVALID_OPERATION);
                    return;
                }
            },
            None => Level::blank(w, h),
        };
        t.internal_format = internal;
        t.depth = matches!(format, g::DEPTH_COMPONENT | g::DEPTH_STENCIL);
        t.set_level(face, level as usize, data);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn tex_sub_image_2d(
        &mut self,
        target: u32,
        level: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        format: u32,
        ty: u32,
        bytes: &[u8],
    ) {
        let id = self.bound_texture_id(target);
        let (flip, premultiply) = (self.flip_y, self.premultiply);
        let Some(t) = self.textures.get_mut(&id) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let face = if t.cube {
            target.wrapping_sub(TEXTURE_CUBE_MAP_POSITIVE_X) as usize
        } else {
            0
        };
        let internal = t.internal_format;
        let Some(src) = tex::decode(w, h, format, ty, internal, bytes, flip, premultiply) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let Some(dst) = t
            .faces
            .get_mut(face)
            .and_then(|c| c.get_mut(level as usize))
        else {
            self.fail(INVALID_OPERATION);
            return;
        };
        if x + w > dst.w || y + h > dst.h {
            self.fail(INVALID_VALUE);
            return;
        }
        for row in 0..h as usize {
            let d = (((y as usize + row) * dst.w as usize) + x as usize) * 4;
            let s = row * w as usize * 4;
            dst.data[d..d + w as usize * 4].copy_from_slice(&src.data[s..s + w as usize * 4]);
        }
    }
    pub fn generate_mipmap(&mut self, target: u32) {
        let id = self.bound_texture_id(target);
        match self.textures.get_mut(&id) {
            Some(t) => t.generate_mipmaps(),
            None => self.fail(INVALID_OPERATION),
        }
    }

    // ----------------------------------------------------- framebuffers

    pub fn create_framebuffer(&mut self) -> u32 {
        let id = self.id();
        self.framebuffers.insert(
            id,
            Fbo {
                color: Attachment::None,
                depth: Attachment::None,
            },
        );
        id
    }
    pub fn delete_framebuffer(&mut self, id: u32) {
        self.framebuffers.remove(&id);
        if self.draw_fb == id {
            self.draw_fb = 0;
        }
        if self.read_fb == id {
            self.read_fb = 0;
        }
    }
    pub fn bind_framebuffer(&mut self, target: u32, id: u32) {
        if id != 0 && !self.framebuffers.contains_key(&id) {
            self.fail(INVALID_OPERATION);
            return;
        }
        match target {
            READ_FRAMEBUFFER => self.read_fb = id,
            DRAW_FRAMEBUFFER => self.draw_fb = id,
            _ => {
                self.read_fb = id;
                self.draw_fb = id;
            }
        }
    }
    pub fn create_renderbuffer(&mut self) -> u32 {
        let id = self.id();
        self.renderbuffers.insert(
            id,
            Renderbuffer {
                w: 0,
                h: 0,
                format: g::RGBA8,
                color: Vec::new(),
                depth: Vec::new(),
            },
        );
        id
    }
    pub fn delete_renderbuffer(&mut self, id: u32) {
        self.renderbuffers.remove(&id);
    }
    pub fn bind_renderbuffer(&mut self, id: u32) {
        self.bound_rb = id;
    }
    pub fn renderbuffer_storage(&mut self, format: u32, w: u32, h: u32) {
        let Some(rb) = self.renderbuffers.get_mut(&self.bound_rb) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        if w > 8192 || h > 8192 {
            self.fail(INVALID_VALUE);
            return;
        }
        rb.w = w;
        rb.h = h;
        rb.format = format;
        let depth_format = matches!(
            format,
            g::DEPTH_COMPONENT16
                | g::DEPTH_COMPONENT24
                | g::DEPTH_COMPONENT32F
                | g::DEPTH24_STENCIL8
                | 0x84F9
                | 0x8D48
        );
        if depth_format {
            rb.depth = vec![1.0; (w * h) as usize];
            rb.color = Vec::new();
        } else {
            rb.color = vec![0.0; (w * h * 4) as usize];
            rb.depth = Vec::new();
        }
    }
    pub fn framebuffer_texture(
        &mut self,
        attachment: u32,
        texture: u32,
        face: usize,
        level: usize,
    ) {
        let target = self.draw_fb;
        let Some(f) = self.framebuffers.get_mut(&target) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let a = if texture == 0 {
            Attachment::None
        } else {
            Attachment::Texture {
                id: texture,
                face,
                level,
            }
        };
        match attachment {
            COLOR_ATTACHMENT0 => f.color = a,
            DEPTH_ATTACHMENT | DEPTH_STENCIL_ATTACHMENT => f.depth = a,
            _ => {}
        }
    }
    pub fn framebuffer_renderbuffer(&mut self, attachment: u32, renderbuffer: u32) {
        let target = self.draw_fb;
        let Some(f) = self.framebuffers.get_mut(&target) else {
            self.fail(INVALID_OPERATION);
            return;
        };
        let a = if renderbuffer == 0 {
            Attachment::None
        } else {
            Attachment::Renderbuffer(renderbuffer)
        };
        match attachment {
            COLOR_ATTACHMENT0 => f.color = a,
            DEPTH_ATTACHMENT | DEPTH_STENCIL_ATTACHMENT => f.depth = a,
            _ => {}
        }
    }
    pub fn check_framebuffer_status(&self) -> u32 {
        if self.draw_fb == 0 {
            return FRAMEBUFFER_COMPLETE;
        }
        match self.framebuffers.get(&self.draw_fb) {
            Some(f) if f.color != Attachment::None || f.depth != Attachment::None => {
                FRAMEBUFFER_COMPLETE
            }
            Some(_) => FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT,
            None => FRAMEBUFFER_INCOMPLETE_ATTACHMENT,
        }
    }

    // ------------------------------------------------------------ state

    pub fn enable(&mut self, cap: u32, on: bool) {
        match cap {
            DEPTH_TEST => self.state.depth_test = on,
            BLEND => self.state.blend = on,
            CULL_FACE => self.state.cull = on,
            SCISSOR_TEST => self.state.scissor_test = on,
            STENCIL_TEST | POLYGON_OFFSET_FILL | 0x0BD0 | 0x809E | 0x80A0 | 0x8C89 => {}
            _ => self.fail(INVALID_ENUM),
        }
    }
    pub fn is_enabled(&self, cap: u32) -> bool {
        match cap {
            DEPTH_TEST => self.state.depth_test,
            BLEND => self.state.blend,
            CULL_FACE => self.state.cull,
            SCISSOR_TEST => self.state.scissor_test,
            _ => false,
        }
    }

    /// `clear(mask)` on the bound draw framebuffer.
    pub fn clear(&mut self, mask: u32) {
        let s = self.state.clone();
        let mut target = self.take_target();
        let (w, h) = (target.w as i32, target.h as i32);
        let (x0, y0, x1, y1) = if s.scissor_test {
            (
                s.scissor[0].max(0),
                s.scissor[1].max(0),
                (s.scissor[0] + s.scissor[2]).min(w),
                (s.scissor[1] + s.scissor[3]).min(h),
            )
        } else {
            (0, 0, w, h)
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * w + x) as usize;
                if mask & COLOR_BUFFER_BIT != 0 {
                    target.write_color(i, s.clear_color, s.color_mask);
                }
                if mask & DEPTH_BUFFER_BIT != 0 && s.depth_mask {
                    if let Some(d) = target.depth.as_mut() {
                        d[i] = s.clear_depth;
                    }
                }
            }
        }
        self.return_target(target);
    }

    /// `readPixels` from the read framebuffer as RGBA8, rows from the bottom.
    pub fn read_pixels(&self, x: i32, y: i32, w: u32, h: u32) -> Vec<u8> {
        let (fw, fh, get): (i32, i32, Box<dyn Fn(usize) -> [f32; 4]>) = if self.read_fb == 0 {
            (
                self.width as i32,
                self.height as i32,
                Box::new(|i| {
                    let p = &self.color[i * 4..i * 4 + 4];
                    [
                        f32::from(p[0]) / 255.0,
                        f32::from(p[1]) / 255.0,
                        f32::from(p[2]) / 255.0,
                        f32::from(p[3]) / 255.0,
                    ]
                }),
            )
        } else {
            match self
                .framebuffers
                .get(&self.read_fb)
                .map(|f| f.color.clone())
            {
                Some(Attachment::Texture { id, face, level }) => {
                    match self.textures.get(&id).and_then(|t| t.level(face, level)) {
                        Some(l) => {
                            let data = l.data.clone();
                            (
                                l.w as i32,
                                l.h as i32,
                                Box::new(move |i| {
                                    [
                                        data[i * 4],
                                        data[i * 4 + 1],
                                        data[i * 4 + 2],
                                        data[i * 4 + 3],
                                    ]
                                }),
                            )
                        }
                        None => return vec![0; (w * h * 4) as usize],
                    }
                }
                Some(Attachment::Renderbuffer(id)) => match self.renderbuffers.get(&id) {
                    Some(rb) if !rb.color.is_empty() => {
                        let data = rb.color.clone();
                        (
                            rb.w as i32,
                            rb.h as i32,
                            Box::new(move |i| {
                                [
                                    data[i * 4],
                                    data[i * 4 + 1],
                                    data[i * 4 + 2],
                                    data[i * 4 + 3],
                                ]
                            }),
                        )
                    }
                    _ => return vec![0; (w * h * 4) as usize],
                },
                _ => return vec![0; (w * h * 4) as usize],
            }
        };
        let mut out = vec![0u8; (w * h * 4) as usize];
        for row in 0..h as i32 {
            for col in 0..w as i32 {
                let (sx, sy) = (x + col, y + row);
                if sx < 0 || sy < 0 || sx >= fw || sy >= fh {
                    continue;
                }
                let px = get((sy * fw + sx) as usize);
                let o = ((row as u32 * w + col as u32) * 4) as usize;
                for c in 0..4 {
                    out[o + c] = (px[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }
        out
    }
}

/// Matches the stages' interfaces and numbers attributes and uniforms.
fn link(vs: Program, fs: Program, bound: &HashMap<String, u32>) -> Result<Linked, String> {
    // Attributes: explicit bindings, layout locations, then the lowest free.
    let mut attributes: Vec<AttribInfo> = Vec::new();
    let mut taken: Vec<u32> = Vec::new();
    let vs_attribs: Vec<(usize, &glsl::interp::Global)> = vs
        .globals
        .iter()
        .enumerate()
        .filter(|(_, g)| matches!(g.storage, Storage::Attribute | Storage::In))
        .collect();
    for (slot, global) in &vs_attribs {
        let fixed = bound.get(&global.name).copied().or(global.location);
        if let Some(loc) = fixed {
            taken.push(loc);
            attributes.push(AttribInfo {
                name: global.name.clone(),
                ty: global.ty.clone(),
                location: loc,
                vs_slot: *slot,
            });
        }
    }
    for (slot, global) in &vs_attribs {
        if attributes.iter().any(|a| a.vs_slot == *slot) {
            continue;
        }
        let mut loc = 0;
        while taken.contains(&loc) {
            loc += 1;
        }
        taken.push(loc);
        attributes.push(AttribInfo {
            name: global.name.clone(),
            ty: global.ty.clone(),
            location: loc,
            vs_slot: *slot,
        });
    }
    if attributes.iter().any(|a| a.location >= 16) {
        return Err("too many vertex attributes".into());
    }

    // Varyings: every fragment input needs a vertex output of the same name.
    let mut varyings = Vec::new();
    for (fs_slot, fg) in fs.globals.iter().enumerate() {
        if !matches!(fg.storage, Storage::Varying | Storage::In) {
            continue;
        }
        let Some((vs_slot, _)) = vs.globals.iter().enumerate().find(|(_, vg)| {
            vg.name == fg.name && matches!(vg.storage, Storage::Varying | Storage::Out)
        }) else {
            // A declaration the fragment shader never reads needs no match.
            if !fs.reads(&fg.name) {
                continue;
            }
            return Err(format!(
                "varying '{}' is not written by the vertex shader",
                fg.name
            ));
        };
        varyings.push(Varying {
            vs_slot,
            fs_slot,
            ty: fg.ty.clone(),
            flat: fg.flat,
        });
    }

    // Uniforms, in a common numbering.
    let mut uniforms: Vec<UniformInfo> = Vec::new();
    let mut next_location = 0i32;
    for prog in [&vs, &fs] {
        for g in &prog.globals {
            if g.storage != Storage::Uniform
                || uniforms
                    .iter()
                    .any(|u| u.name.trim_end_matches("[0]") == g.name)
            {
                continue;
            }
            let (ty, size, name) = match &g.ty {
                Ty::Array(inner, n) => ((**inner).clone(), *n as u32, format!("{}[0]", g.name)),
                t => (t.clone(), 1, g.name.clone()),
            };
            let full_ty = if size > 1 || name.ends_with("[0]") {
                Ty::Array(Box::new(ty), size as usize)
            } else {
                ty
            };
            uniforms.push(UniformInfo {
                name,
                ty: full_ty,
                size,
                base_location: next_location,
            });
            next_location += size as i32;
        }
    }

    // Fragment outputs.
    let mut outputs = Vec::new();
    for (slot, g) in fs.globals.iter().enumerate() {
        if g.storage == Storage::Out {
            outputs.push((g.location.unwrap_or(outputs.len() as u32) as usize, slot));
        }
    }

    let mut values = HashMap::new();
    for u in &uniforms {
        let name = u.name.trim_end_matches("[0]").to_string();
        let ty = match &u.ty {
            Ty::Array(..) if u.size >= 1 => u.ty.clone(),
            t => t.clone(),
        };
        values.insert(name, zero(&ty, &[]));
    }
    Ok(Linked {
        vs,
        fs,
        uniforms,
        attributes,
        varyings,
        outputs,
        values,
    })
}

pub(crate) struct Target {
    pub w: u32,
    pub h: u32,
    pub color: ColorStore,
    pub depth: Option<Vec<f32>>,
    pub source: TargetSource,
}

pub(crate) enum ColorStore {
    None,
    Bytes(Vec<u8>),
    Floats(Vec<f32>),
}

pub(crate) enum TargetSource {
    Default,
    Texture {
        id: u32,
        face: usize,
        level: usize,
        depth_texture: Option<(u32, usize, usize)>,
    },
    Renderbuffer {
        color: Option<u32>,
        depth: Option<u32>,
    },
}

impl Target {
    pub fn write_color(&mut self, pixel: usize, c: [f32; 4], mask: [bool; 4]) {
        match &mut self.color {
            ColorStore::Bytes(b) => {
                for k in 0..4 {
                    if mask[k] {
                        b[pixel * 4 + k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            }
            ColorStore::Floats(f) => {
                for k in 0..4 {
                    if mask[k] {
                        f[pixel * 4 + k] = c[k];
                    }
                }
            }
            ColorStore::None => {}
        }
    }
}

impl Context {
    /// Moves the draw framebuffer's storage out of its owners for a draw, so
    /// the draw can write it while other textures are being read.
    pub(crate) fn take_target(&mut self) -> Target {
        if self.draw_fb != 0 {
            if let Some(fbo) = self.framebuffers.get(&self.draw_fb).cloned() {
                let mut target = Target {
                    w: 0,
                    h: 0,
                    color: ColorStore::None,
                    depth: None,
                    source: TargetSource::Renderbuffer {
                        color: None,
                        depth: None,
                    },
                };
                match fbo.color {
                    Attachment::Texture { id, face, level } => {
                        if let Some(t) = self.textures.get_mut(&id) {
                            if let Some(l) = t.faces.get_mut(face).and_then(|c| c.get_mut(level)) {
                                target.w = l.w;
                                target.h = l.h;
                                target.color = ColorStore::Floats(std::mem::take(&mut l.data));
                                target.source = TargetSource::Texture {
                                    id,
                                    face,
                                    level,
                                    depth_texture: None,
                                };
                            }
                        }
                    }
                    Attachment::Renderbuffer(id) => {
                        if let Some(rb) = self.renderbuffers.get_mut(&id) {
                            target.w = rb.w;
                            target.h = rb.h;
                            target.color = ColorStore::Floats(std::mem::take(&mut rb.color));
                            target.source = TargetSource::Renderbuffer {
                                color: Some(id),
                                depth: None,
                            };
                        }
                    }
                    Attachment::None => {}
                }
                match fbo.depth {
                    Attachment::Renderbuffer(id) => {
                        if let Some(rb) = self.renderbuffers.get_mut(&id) {
                            if target.w == 0 {
                                target.w = rb.w;
                                target.h = rb.h;
                            }
                            if rb.w == target.w && rb.h == target.h {
                                target.depth = Some(std::mem::take(&mut rb.depth));
                                if let TargetSource::Renderbuffer { depth, .. } = &mut target.source
                                {
                                    *depth = Some(id)
                                }
                            }
                        }
                    }
                    Attachment::Texture { id, face, level } => {
                        if let Some(l) = self.textures.get(&id).and_then(|t| t.level(face, level)) {
                            if target.w == 0 {
                                target.w = l.w;
                                target.h = l.h;
                            }
                            if l.w == target.w && l.h == target.h {
                                let depth: Vec<f32> =
                                    l.data.chunks_exact(4).map(|p| p[0]).collect();
                                target.depth = Some(depth);
                                match &mut target.source {
                                    TargetSource::Texture { depth_texture, .. } => {
                                        *depth_texture = Some((id, face, level))
                                    }
                                    TargetSource::Renderbuffer { .. } => {
                                        target.source = TargetSource::Texture {
                                            id: 0,
                                            face: 0,
                                            level: 0,
                                            depth_texture: Some((id, face, level)),
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Attachment::None => {}
                }
                return target;
            }
        }
        Target {
            w: self.width,
            h: self.height,
            color: ColorStore::Bytes(std::mem::take(&mut self.color)),
            depth: Some(std::mem::take(&mut self.depth)),
            source: TargetSource::Default,
        }
    }

    pub(crate) fn return_target(&mut self, target: Target) {
        match (target.source, target.color) {
            (TargetSource::Default, ColorStore::Bytes(b)) => {
                self.color = b;
                self.depth = target.depth.unwrap_or_default();
            }
            (
                TargetSource::Texture {
                    id,
                    face,
                    level,
                    depth_texture,
                },
                color,
            ) => {
                if let ColorStore::Floats(data) = color {
                    if let Some(l) = self
                        .textures
                        .get_mut(&id)
                        .and_then(|t| t.faces.get_mut(face))
                        .and_then(|c| c.get_mut(level))
                    {
                        l.data = data;
                    }
                }
                if let (Some((tid, f, lv)), Some(depth)) = (depth_texture, target.depth) {
                    if let Some(l) = self
                        .textures
                        .get_mut(&tid)
                        .and_then(|t| t.faces.get_mut(f))
                        .and_then(|c| c.get_mut(lv))
                    {
                        for (px, d) in l.data.chunks_exact_mut(4).zip(depth) {
                            px[0] = d;
                            px[1] = d;
                            px[2] = d;
                        }
                    }
                }
            }
            (TargetSource::Renderbuffer { color, depth }, c) => {
                if let (Some(id), ColorStore::Floats(data)) = (color, c) {
                    if let Some(rb) = self.renderbuffers.get_mut(&id) {
                        rb.color = data;
                    }
                }
                if let (Some(id), Some(d)) = (depth, target.depth) {
                    if let Some(rb) = self.renderbuffers.get_mut(&id) {
                        rb.depth = d;
                    }
                }
            }
            _ => {}
        }
    }
}
