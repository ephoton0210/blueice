// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Texture storage and filtering. Texels are kept as RGBA `f32`, whatever
//! the upload format was, so sampling has one path.

pub mod gl {
    pub const NEAREST: u32 = 0x2600;
    pub const LINEAR: u32 = 0x2601;
    pub const NEAREST_MIPMAP_NEAREST: u32 = 0x2700;
    pub const LINEAR_MIPMAP_NEAREST: u32 = 0x2701;
    pub const NEAREST_MIPMAP_LINEAR: u32 = 0x2702;
    pub const LINEAR_MIPMAP_LINEAR: u32 = 0x2703;
    pub const REPEAT: u32 = 0x2901;
    pub const CLAMP_TO_EDGE: u32 = 0x812F;
    pub const MIRRORED_REPEAT: u32 = 0x8370;
    pub const ALPHA: u32 = 0x1906;
    pub const RGB: u32 = 0x1907;
    pub const RGBA: u32 = 0x1908;
    pub const LUMINANCE: u32 = 0x1909;
    pub const LUMINANCE_ALPHA: u32 = 0x190A;
    pub const RED: u32 = 0x1903;
    pub const RG: u32 = 0x8227;
    pub const DEPTH_COMPONENT: u32 = 0x1902;
    pub const DEPTH_STENCIL: u32 = 0x84F9;
    pub const UNSIGNED_BYTE: u32 = 0x1401;
    pub const UNSIGNED_SHORT: u32 = 0x1403;
    pub const UNSIGNED_INT: u32 = 0x1405;
    pub const FLOAT: u32 = 0x1406;
    pub const HALF_FLOAT: u32 = 0x140B;
    pub const HALF_FLOAT_OES: u32 = 0x8D61;
    pub const UNSIGNED_SHORT_4_4_4_4: u32 = 0x8033;
    pub const UNSIGNED_SHORT_5_5_5_1: u32 = 0x8034;
    pub const UNSIGNED_SHORT_5_6_5: u32 = 0x8363;
    pub const R8: u32 = 0x8229;
    pub const RG8: u32 = 0x822B;
    pub const RGB8: u32 = 0x8051;
    pub const RGBA8: u32 = 0x8058;
    pub const SRGB8: u32 = 0x8C41;
    pub const SRGB8_ALPHA8: u32 = 0x8C43;
    pub const R16F: u32 = 0x822D;
    pub const RG16F: u32 = 0x822F;
    pub const RGB16F: u32 = 0x881B;
    pub const RGBA16F: u32 = 0x881A;
    pub const R32F: u32 = 0x822E;
    pub const RG32F: u32 = 0x8230;
    pub const RGB32F: u32 = 0x8815;
    pub const RGBA32F: u32 = 0x8814;
    pub const DEPTH_COMPONENT16: u32 = 0x81A5;
    pub const DEPTH_COMPONENT24: u32 = 0x81A6;
    pub const DEPTH_COMPONENT32F: u32 = 0x8CAC;
    pub const DEPTH24_STENCIL8: u32 = 0x88F0;
}

#[derive(Clone, Debug)]
pub struct Level {
    pub w: u32,
    pub h: u32,
    /// RGBA, row-major from the bottom row (GL's origin).
    pub data: Vec<f32>,
}

impl Level {
    pub fn blank(w: u32, h: u32) -> Level {
        Level {
            w,
            h,
            data: vec![0.0; (w * h * 4) as usize],
        }
    }
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.clamp(0, self.w as i64 - 1) as usize;
        let y = y.clamp(0, self.h as i64 - 1) as usize;
        let i = (y * self.w as usize + x) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Texture {
    /// One entry per face (six for a cube map); each a chain of mip levels.
    pub faces: Vec<Vec<Level>>,
    pub cube: bool,
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub min_filter: u32,
    pub mag_filter: u32,
    pub internal_format: u32,
    /// A depth texture (its value is in the red channel).
    pub depth: bool,
    pub compare: bool,
}

impl Texture {
    pub fn new(cube: bool) -> Texture {
        Texture {
            faces: vec![Vec::new(); if cube { 6 } else { 1 }],
            cube,
            wrap_s: gl::REPEAT,
            wrap_t: gl::REPEAT,
            min_filter: gl::NEAREST_MIPMAP_LINEAR,
            mag_filter: gl::LINEAR,
            internal_format: gl::RGBA8,
            depth: false,
            compare: false,
        }
    }

    pub fn set_level(&mut self, face: usize, level: usize, data: Level) {
        let last = self.faces.len() - 1;
        let chain = &mut self.faces[face.min(last)];
        while chain.len() <= level {
            chain.push(Level::blank(0, 0));
        }
        chain[level] = data;
    }

    pub fn level(&self, face: usize, level: usize) -> Option<&Level> {
        self.faces
            .get(face)?
            .get(level)
            .filter(|l| l.w > 0 && l.h > 0)
    }

    /// Whether a sampler can read it: every face has a base level.
    pub fn complete(&self) -> bool {
        (0..self.faces.len()).all(|f| self.level(f, 0).is_some())
    }

    fn uses_mips(&self) -> bool {
        self.min_filter >= gl::NEAREST_MIPMAP_NEAREST && self.min_filter <= gl::LINEAR_MIPMAP_LINEAR
    }

    /// Rebuilds the chain below level 0 of every face by averaging 2x2 blocks.
    pub fn generate_mipmaps(&mut self) {
        for chain in &mut self.faces {
            let Some(base) = chain.first().cloned() else {
                continue;
            };
            chain.truncate(1);
            let mut prev = base;
            while prev.w > 1 || prev.h > 1 {
                let (w, h) = ((prev.w / 2).max(1), (prev.h / 2).max(1));
                let mut next = Level::blank(w, h);
                for y in 0..h {
                    for x in 0..w {
                        let mut acc = [0.0f32; 4];
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let t = prev.texel((x * 2 + dx) as i64, (y * 2 + dy) as i64);
                            for c in 0..4 {
                                acc[c] += t[c] * 0.25;
                            }
                        }
                        let i = ((y * w + x) * 4) as usize;
                        next.data[i..i + 4].copy_from_slice(&acc);
                    }
                }
                chain.push(next.clone());
                prev = next;
            }
        }
    }

    fn wrap(coord: f32, size: u32, mode: u32) -> f64 {
        let n = size as f64;
        let c = coord as f64 * n;
        match mode {
            gl::CLAMP_TO_EDGE => c.clamp(0.0, n),
            gl::MIRRORED_REPEAT => {
                let period = 2.0 * n;
                let m = c.rem_euclid(period);
                if m < n {
                    m
                } else {
                    period - m
                }
            }
            _ => c.rem_euclid(n),
        }
    }

    fn bilinear(&self, level: &Level, u: f32, v: f32, linear: bool) -> [f32; 4] {
        let (x, y) = (
            Self::wrap(u, level.w, self.wrap_s),
            Self::wrap(v, level.h, self.wrap_t),
        );
        if !linear {
            return level.texel(x.floor() as i64, y.floor() as i64);
        }
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        // Wrapping applies to the neighbouring texels too.
        let tap = |ix: f64, iy: f64| -> [f32; 4] {
            let wx = match self.wrap_s {
                gl::CLAMP_TO_EDGE => ix.clamp(0.0, level.w as f64 - 1.0),
                gl::MIRRORED_REPEAT => {
                    let period = 2.0 * level.w as f64;
                    let m = ix.rem_euclid(period);
                    if m < level.w as f64 {
                        m
                    } else {
                        period - 1.0 - m
                    }
                }
                _ => ix.rem_euclid(level.w as f64),
            };
            let wy = match self.wrap_t {
                gl::CLAMP_TO_EDGE => iy.clamp(0.0, level.h as f64 - 1.0),
                gl::MIRRORED_REPEAT => {
                    let period = 2.0 * level.h as f64;
                    let m = iy.rem_euclid(period);
                    if m < level.h as f64 {
                        m
                    } else {
                        period - 1.0 - m
                    }
                }
                _ => iy.rem_euclid(level.h as f64),
            };
            level.texel(wx as i64, wy as i64)
        };
        let (a, b, c, d) = (
            tap(x0, y0),
            tap(x0 + 1.0, y0),
            tap(x0, y0 + 1.0),
            tap(x0 + 1.0, y0 + 1.0),
        );
        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = a[i] * (1.0 - tx) + b[i] * tx;
            let bottom = c[i] * (1.0 - tx) + d[i] * tx;
            out[i] = top * (1.0 - ty) + bottom * ty;
        }
        out
    }

    /// A 2D read at normalized `(u, v)`. `lod` picks a mip level when given;
    /// otherwise level 0 is read (derivatives are not available here).
    pub fn sample_2d(&self, face: usize, u: f32, v: f32, lod: Option<f32>) -> [f32; 4] {
        let Some(base) = self.level(face, 0) else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        let magnify = lod.is_none_or(|l| l <= 0.0);
        let filter = if magnify {
            self.mag_filter
        } else {
            self.min_filter
        };
        let linear = matches!(
            filter,
            gl::LINEAR | gl::LINEAR_MIPMAP_NEAREST | gl::LINEAR_MIPMAP_LINEAR
        );
        let level = match lod {
            Some(l) if l > 0.0 && self.uses_mips() => {
                let chain_len = self.faces[face].len();
                let i = (l.round() as usize).min(chain_len.saturating_sub(1));
                self.level(face, i).unwrap_or(base)
            }
            _ => base,
        };
        self.bilinear(level, u, v, linear)
    }

    /// A cube-map read along direction `(x, y, z)`.
    pub fn sample_cube(&self, x: f32, y: f32, z: f32, lod: Option<f32>) -> [f32; 4] {
        let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if x > 0.0 {
                (0, -z, -y, ax)
            } else {
                (1, z, -y, ax)
            }
        } else if ay >= az {
            if y > 0.0 {
                (2, x, z, ay)
            } else {
                (3, x, -z, ay)
            }
        } else if z > 0.0 {
            (4, x, -y, az)
        } else {
            (5, -x, -y, az)
        };
        if ma == 0.0 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let (u, v) = (0.5 * (sc / ma + 1.0), 0.5 * (tc / ma + 1.0));
        // GL's t axis points the other way from the texel rows used here.
        self.sample_2d(face, u, 1.0 - v, lod)
    }
}

fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let frac = (h & 0x3ff) as f32;
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

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Components per pixel of an upload `format`.
fn channels(format: u32) -> usize {
    match format {
        gl::RGBA => 4,
        gl::RGB => 3,
        gl::LUMINANCE_ALPHA | gl::RG => 2,
        gl::DEPTH_STENCIL => 1,
        _ => 1,
    }
}

/// Decodes `bytes` (an upload of `w` x `h` pixels in `format`/`ty`) to RGBA floats.
/// `None` when the data is too short or the combination is unsupported.
pub fn decode(
    w: u32,
    h: u32,
    format: u32,
    ty: u32,
    internal: u32,
    bytes: &[u8],
    flip_y: bool,
    premultiply: bool,
) -> Option<Level> {
    let (w_, h_) = (w as usize, h as usize);
    let n = channels(format);
    let packed = matches!(
        ty,
        gl::UNSIGNED_SHORT_4_4_4_4 | gl::UNSIGNED_SHORT_5_5_5_1 | gl::UNSIGNED_SHORT_5_6_5
    );
    let bytes_per = match ty {
        gl::UNSIGNED_BYTE => n,
        gl::FLOAT => 4 * n,
        gl::HALF_FLOAT | gl::HALF_FLOAT_OES | gl::UNSIGNED_SHORT => 2 * n,
        gl::UNSIGNED_INT => 4 * n,
        _ if packed => 2,
        _ => return None,
    };
    // Rows are unpacked 4-byte aligned by default.
    let row_bytes = (w_ * bytes_per + 3) & !3;
    if bytes.len() < row_bytes * h_.saturating_sub(1) + w_ * bytes_per {
        return None;
    }
    let srgb = matches!(internal, gl::SRGB8 | gl::SRGB8_ALPHA8);
    let mut level = Level::blank(w, h);
    for row in 0..h_ {
        let src_row = &bytes[row * row_bytes..];
        let dst_y = if flip_y { h_ - 1 - row } else { row };
        for x in 0..w_ {
            let px = &src_row[x * bytes_per..];
            let get = |i: usize| -> f32 {
                match ty {
                    gl::UNSIGNED_BYTE => f32::from(px[i]) / 255.0,
                    gl::FLOAT => {
                        f32::from_le_bytes([px[4 * i], px[4 * i + 1], px[4 * i + 2], px[4 * i + 3]])
                    }
                    gl::HALF_FLOAT | gl::HALF_FLOAT_OES => {
                        half_to_f32(u16::from_le_bytes([px[2 * i], px[2 * i + 1]]))
                    }
                    gl::UNSIGNED_SHORT => {
                        f32::from(u16::from_le_bytes([px[2 * i], px[2 * i + 1]])) / 65535.0
                    }
                    gl::UNSIGNED_INT => {
                        u32::from_le_bytes([px[4 * i], px[4 * i + 1], px[4 * i + 2], px[4 * i + 3]])
                            as f32
                            / 4294967295.0
                    }
                    _ => 0.0,
                }
            };
            let mut c = if packed {
                let v = u16::from_le_bytes([px[0], px[1]]);
                match ty {
                    gl::UNSIGNED_SHORT_4_4_4_4 => [
                        f32::from(v >> 12) / 15.0,
                        f32::from((v >> 8) & 15) / 15.0,
                        f32::from((v >> 4) & 15) / 15.0,
                        f32::from(v & 15) / 15.0,
                    ],
                    gl::UNSIGNED_SHORT_5_5_5_1 => [
                        f32::from(v >> 11) / 31.0,
                        f32::from((v >> 6) & 31) / 31.0,
                        f32::from((v >> 1) & 31) / 31.0,
                        f32::from(v & 1),
                    ],
                    _ => [
                        f32::from(v >> 11) / 31.0,
                        f32::from((v >> 5) & 63) / 63.0,
                        f32::from(v & 31) / 31.0,
                        1.0,
                    ],
                }
            } else {
                match format {
                    gl::RGBA => [get(0), get(1), get(2), get(3)],
                    gl::RGB => [get(0), get(1), get(2), 1.0],
                    gl::LUMINANCE => [get(0), get(0), get(0), 1.0],
                    gl::LUMINANCE_ALPHA => [get(0), get(0), get(0), get(1)],
                    gl::ALPHA => [0.0, 0.0, 0.0, get(0)],
                    gl::RED => [get(0), 0.0, 0.0, 1.0],
                    gl::RG => [get(0), get(1), 0.0, 1.0],
                    gl::DEPTH_COMPONENT | gl::DEPTH_STENCIL => [get(0), get(0), get(0), 1.0],
                    _ => return None,
                }
            };
            if srgb {
                for ch in c.iter_mut().take(3) {
                    *ch = srgb_to_linear(*ch);
                }
            }
            if premultiply {
                let alpha = c[3];
                for ch in c.iter_mut().take(3) {
                    *ch *= alpha;
                }
            }
            let i = (dst_y * w_ + x) * 4;
            level.data[i..i + 4].copy_from_slice(&c);
        }
    }
    Some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex_2x2() -> Texture {
        // Rows from the bottom: (red, green), (blue, white).
        let bytes = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let level = decode(
            2,
            2,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            gl::RGBA8,
            &bytes,
            false,
            false,
        )
        .unwrap();
        let mut t = Texture::new(false);
        t.set_level(0, 0, level);
        t.mag_filter = gl::NEAREST;
        t.min_filter = gl::NEAREST;
        t
    }

    #[test]
    fn nearest_sampling_reads_the_texel_under_the_coordinate() {
        let t = tex_2x2();
        assert_eq!(t.sample_2d(0, 0.25, 0.25, None), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(t.sample_2d(0, 0.75, 0.25, None), [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(t.sample_2d(0, 0.25, 0.75, None), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(t.sample_2d(0, 0.75, 0.75, None), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn linear_sampling_blends_neighbours() {
        let mut t = tex_2x2();
        t.mag_filter = gl::LINEAR;
        t.wrap_s = gl::CLAMP_TO_EDGE;
        t.wrap_t = gl::CLAMP_TO_EDGE;
        // The middle of the texture is the average of all four texels.
        let mid = t.sample_2d(0, 0.5, 0.5, None);
        assert!(
            (mid[0] - 0.5).abs() < 1e-5
                && (mid[1] - 0.5).abs() < 1e-5
                && (mid[2] - 0.5).abs() < 1e-5
        );
        // At a texel centre it is that texel.
        assert_eq!(t.sample_2d(0, 0.25, 0.25, None), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn wrap_modes_differ_outside_the_unit_square() {
        let mut t = tex_2x2();
        t.wrap_s = gl::REPEAT;
        assert_eq!(t.sample_2d(0, 1.25, 0.25, None), [1.0, 0.0, 0.0, 1.0]);
        t.wrap_s = gl::CLAMP_TO_EDGE;
        assert_eq!(t.sample_2d(0, 1.25, 0.25, None), [0.0, 1.0, 0.0, 1.0]);
        t.wrap_s = gl::MIRRORED_REPEAT;
        assert_eq!(t.sample_2d(0, 1.25, 0.25, None), [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn mipmaps_average_and_explicit_lod_selects_a_level() {
        let mut t = tex_2x2();
        t.min_filter = gl::NEAREST_MIPMAP_NEAREST;
        t.generate_mipmaps();
        assert_eq!(t.faces[0].len(), 2);
        let top = t.sample_2d(0, 0.5, 0.5, Some(1.0));
        assert!((top[0] - 0.5).abs() < 1e-5 && (top[3] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn upload_formats_decode_to_rgba() {
        let l = decode(
            1,
            1,
            gl::LUMINANCE_ALPHA,
            gl::UNSIGNED_BYTE,
            gl::LUMINANCE_ALPHA,
            &[128, 255, 0, 0],
            false,
            false,
        )
        .unwrap();
        assert!(
            (l.data[0] - 128.0 / 255.0).abs() < 1e-6 && l.data[1] == l.data[0] && l.data[3] == 1.0
        );
        let l = decode(
            1,
            1,
            gl::RGB,
            gl::UNSIGNED_SHORT_5_6_5,
            gl::RGB,
            &[0x00, 0xf8],
            false,
            false,
        )
        .unwrap();
        assert_eq!(l.data, vec![1.0, 0.0, 0.0, 1.0]);
        let f = 0.25f32.to_le_bytes();
        let l = decode(1, 1, gl::RED, gl::FLOAT, gl::R32F, &f, false, false).unwrap();
        assert_eq!(l.data[0], 0.25);
        // Half float 1.5 is 0x3E00.
        let l = decode(
            1,
            1,
            gl::RED,
            gl::HALF_FLOAT,
            gl::R16F,
            &[0x00, 0x3e, 0, 0],
            false,
            false,
        )
        .unwrap();
        assert_eq!(l.data[0], 1.5);
        assert!(
            decode(
                2,
                2,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                gl::RGBA8,
                &[0; 4],
                false,
                false
            )
            .is_none(),
            "too short"
        );
    }

    #[test]
    fn flip_premultiply_and_srgb_apply_on_upload() {
        let bytes = [10, 0, 0, 255, 200, 0, 0, 255];
        let l = decode(
            1,
            2,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            gl::RGBA8,
            &bytes,
            true,
            false,
        )
        .unwrap();
        assert!(l.data[0] > 0.7, "the top row became the bottom row");
        let half = [255, 255, 255, 128];
        let l = decode(
            1,
            1,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            gl::RGBA8,
            &half,
            false,
            true,
        )
        .unwrap();
        assert!((l.data[0] - 128.0 / 255.0).abs() < 1e-6);
        let mid = [128, 128, 128, 255];
        let l = decode(
            1,
            1,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            gl::SRGB8_ALPHA8,
            &mid,
            false,
            false,
        )
        .unwrap();
        assert!(
            (l.data[0] - 0.2158).abs() < 1e-3,
            "sRGB mid grey is about 0.216 linear"
        );
    }

    #[test]
    fn cube_sampling_picks_the_face_by_the_major_axis() {
        let mut t = Texture::new(true);
        for face in 0..6 {
            let mut l = Level::blank(1, 1);
            l.data = vec![face as f32 / 10.0, 0.0, 0.0, 1.0];
            t.set_level(face, 0, l);
        }
        t.mag_filter = gl::NEAREST;
        assert!(t.complete());
        assert_eq!(t.sample_cube(1.0, 0.1, 0.1, None)[0], 0.0);
        assert_eq!(t.sample_cube(-1.0, 0.1, 0.1, None)[0], 0.1);
        assert_eq!(t.sample_cube(0.1, 1.0, 0.1, None)[0], 0.2);
        assert_eq!(t.sample_cube(0.1, 0.1, -1.0, None)[0], 0.5);
    }

    #[test]
    fn an_empty_texture_reads_opaque_black() {
        let t = Texture::new(false);
        assert!(!t.complete());
        assert_eq!(t.sample_2d(0, 0.5, 0.5, None), [0.0, 0.0, 0.0, 1.0]);
    }
}
