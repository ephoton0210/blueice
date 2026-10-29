// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A small `CanvasRenderingContext2D`: a straight-alpha RGBA bitmap that
//! scripts fill with rectangles, draw pictures onto and read pixels back from.
//! No paths or text yet -- those calls are accepted and draw nothing -- so
//! pages that use a canvas to read an image's pixels or to composite pictures
//! work, and everything else degrades to an empty canvas rather than failing.

use super::dom_bridge::Reply;
use super::net::{base64_decode, base64_encode};

/// Largest bitmap a script may ask for, in pixels.
const MAX_PIXELS: u64 = 16_000_000;

pub(crate) struct Canvas2d {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub canvas: blueice_dom::NodeId,
    pub dirty: bool,
}

type Affine = [f64; 6];

fn num(args: &[Reply], i: usize) -> f64 {
    match args.get(i) {
        Some(Reply::Number(n)) if n.is_finite() => *n,
        _ => 0.0,
    }
}

fn text(args: &[Reply], i: usize) -> &str {
    match args.get(i) {
        Some(Reply::Text(s)) => s,
        _ => "",
    }
}

fn affine(args: &[Reply], at: usize) -> Affine {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    if let Some(Reply::Text(s)) = args.get(at) {
        if let Ok(v) = serde_json::from_str::<Vec<f64>>(s) {
            if v.len() == 6 && v.iter().all(|x| x.is_finite()) {
                m.copy_from_slice(&v);
            }
        }
    }
    m
}

/// The inverse of `m`, or `None` for a singular matrix.
fn invert(m: &Affine) -> Option<Affine> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
}

fn apply(m: &Affine, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Source-over of one straight-alpha pixel onto `dst`.
fn over(dst: &mut [u8], src: [u8; 4], alpha: f64) {
    let sa = f64::from(src[3]) / 255.0 * alpha;
    if sa <= 0.0 {
        return;
    }
    let da = f64::from(dst[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    for c in 0..3 {
        let v = (f64::from(src[c]) * sa + f64::from(dst[c]) * da * (1.0 - sa)) / out_a;
        dst[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

impl Canvas2d {
    pub fn new(canvas: blueice_dom::NodeId, w: u32, h: u32) -> Option<Canvas2d> {
        if u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        Some(Canvas2d {
            w,
            h,
            rgba: vec![0; (w * h * 4) as usize],
            canvas,
            dirty: true,
        })
    }

    /// The bounding box of the device-space quad `m` makes of `(x, y, w, h)`,
    /// clipped to the bitmap.
    fn bounds(&self, m: &Affine, x: f64, y: f64, w: f64, h: f64) -> (i64, i64, i64, i64) {
        let corners = [
            apply(m, x, y),
            apply(m, x + w, y),
            apply(m, x, y + h),
            apply(m, x + w, y + h),
        ];
        let min = |f: fn(&(f64, f64)) -> f64| corners.iter().map(f).fold(f64::INFINITY, f64::min);
        let max =
            |f: fn(&(f64, f64)) -> f64| corners.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
        (
            (min(|c| c.0).floor() as i64).max(0),
            (min(|c| c.1).floor() as i64).max(0),
            (max(|c| c.0).ceil() as i64).min(i64::from(self.w)),
            (max(|c| c.1).ceil() as i64).min(i64::from(self.h)),
        )
    }

    fn fill_rect(
        &mut self,
        m: &Affine,
        (x, y, w, h): (f64, f64, f64, f64),
        color: [u8; 4],
        alpha: f64,
        clear: bool,
    ) {
        let Some(inv) = invert(m) else { return };
        let (x0, y0, x1, y1) = self.bounds(m, x, y, w, h);
        for py in y0..y1 {
            for px in x0..x1 {
                let (u, v) = apply(&inv, px as f64 + 0.5, py as f64 + 0.5);
                if u < x.min(x + w) || u >= x.max(x + w) || v < y.min(y + h) || v >= y.max(y + h) {
                    continue;
                }
                let i = ((py as u32 * self.w + px as u32) * 4) as usize;
                if clear {
                    self.rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
                } else {
                    over(&mut self.rgba[i..i + 4], color, alpha);
                }
            }
        }
        self.dirty = true;
    }

    /// Draws `src` (width, height, straight-alpha RGBA) from `from` onto `to`.
    #[allow(clippy::too_many_arguments)]
    fn draw_image(
        &mut self,
        m: &Affine,
        src: (u32, u32, &[u8]),
        from: (f64, f64, f64, f64),
        to: (f64, f64, f64, f64),
        alpha: f64,
    ) {
        let (sw_full, sh_full, data) = src;
        let (sx, sy, sw, sh) = from;
        let (dx, dy, dw, dh) = to;
        if sw == 0.0
            || sh == 0.0
            || dw == 0.0
            || dh == 0.0
            || data.len() < (sw_full * sh_full * 4) as usize
        {
            return;
        }
        let Some(inv) = invert(m) else { return };
        let (x0, y0, x1, y1) = self.bounds(m, dx, dy, dw, dh);
        let sample = |fx: f64, fy: f64| -> [f64; 4] {
            // Bilinear over the source rectangle, clamped to the picture.
            let fx = (fx - 0.5)
                .clamp(sx, sx + sw - 1.0)
                .clamp(0.0, f64::from(sw_full) - 1.0);
            let fy = (fy - 0.5)
                .clamp(sy, sy + sh - 1.0)
                .clamp(0.0, f64::from(sh_full) - 1.0);
            let (ix, iy) = (fx.floor() as usize, fy.floor() as usize);
            let (tx, ty) = (fx - ix as f64, fy - iy as f64);
            let at = |x: usize, y: usize| -> [f64; 4] {
                let x = x.min(sw_full as usize - 1);
                let y = y.min(sh_full as usize - 1);
                let i = (y * sw_full as usize + x) * 4;
                let a = f64::from(data[i + 3]) / 255.0;
                // Premultiplied while mixing, so a clear neighbour does not bleed colour.
                [
                    f64::from(data[i]) * a,
                    f64::from(data[i + 1]) * a,
                    f64::from(data[i + 2]) * a,
                    a,
                ]
            };
            let (p00, p10, p01, p11) = (
                at(ix, iy),
                at(ix + 1, iy),
                at(ix, iy + 1),
                at(ix + 1, iy + 1),
            );
            let mut out = [0.0; 4];
            for c in 0..4 {
                out[c] = (p00[c] * (1.0 - tx) + p10[c] * tx) * (1.0 - ty)
                    + (p01[c] * (1.0 - tx) + p11[c] * tx) * ty;
            }
            out
        };
        for py in y0..y1 {
            for px in x0..x1 {
                let (u, v) = apply(&inv, px as f64 + 0.5, py as f64 + 0.5);
                let (tu, tv) = ((u - dx) / dw, (v - dy) / dh);
                if !(0.0..1.0).contains(&tu) || !(0.0..1.0).contains(&tv) {
                    continue;
                }
                let p = sample(sx + tu * sw, sy + tv * sh);
                if p[3] <= 0.0 {
                    continue;
                }
                let color = [
                    (p[0] / p[3]).round().clamp(0.0, 255.0) as u8,
                    (p[1] / p[3]).round().clamp(0.0, 255.0) as u8,
                    (p[2] / p[3]).round().clamp(0.0, 255.0) as u8,
                    (p[3] * 255.0).round().clamp(0.0, 255.0) as u8,
                ];
                let i = ((py as u32 * self.w + px as u32) * 4) as usize;
                over(&mut self.rgba[i..i + 4], color, alpha);
            }
        }
        self.dirty = true;
    }

    fn read(&self, x: i64, y: i64, w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0u8; (w * h * 4) as usize];
        for row in 0..i64::from(h) {
            let sy = y + row;
            if sy < 0 || sy >= i64::from(self.h) {
                continue;
            }
            for col in 0..i64::from(w) {
                let sx = x + col;
                if sx < 0 || sx >= i64::from(self.w) {
                    continue;
                }
                let s = ((sy as u32 * self.w + sx as u32) * 4) as usize;
                let d = ((row as u32 * w + col as u32) * 4) as usize;
                out[d..d + 4].copy_from_slice(&self.rgba[s..s + 4]);
            }
        }
        out
    }

    fn write(&mut self, x: i64, y: i64, w: u32, h: u32, data: &[u8]) {
        for row in 0..i64::from(h) {
            let dy = y + row;
            if dy < 0 || dy >= i64::from(self.h) {
                continue;
            }
            for col in 0..i64::from(w) {
                let dx = x + col;
                if dx < 0 || dx >= i64::from(self.w) {
                    continue;
                }
                let d = ((dy as u32 * self.w + dx as u32) * 4) as usize;
                let s = ((row as u32 * w + col as u32) * 4) as usize;
                self.rgba[d..d + 4].copy_from_slice(&data[s..s + 4]);
            }
        }
        self.dirty = true;
    }

    pub fn resize(&mut self, w: u32, h: u32) -> Result<(), String> {
        if u64::from(w) * u64::from(h) > MAX_PIXELS {
            return Err("canvas is too large".into());
        }
        self.w = w;
        self.h = h;
        self.rgba = vec![0; (w * h * 4) as usize];
        self.dirty = true;
        Ok(())
    }

    /// Operations: `fillRect`/`clearRect` (x y w h style alpha transform),
    /// `drawImage` (w h pixels sx sy sw sh dx dy dw dh alpha transform),
    /// `getImageData` (x y w h), `putImageData` (pixels w h dx dy),
    /// `resize` (w h).
    pub fn call(&mut self, op: &str, a: &[Reply]) -> Result<Reply, String> {
        Ok(match op {
            "fillRect" | "clearRect" => {
                let color = blueice_css::parse_css_color(text(a, 4))
                    .map_or([0, 0, 0, 255], |(r, g, b, al)| [r, g, b, al]);
                let alpha = if a.get(5).is_some() {
                    num(a, 5).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let m = affine(a, 6);
                self.fill_rect(
                    &m,
                    (num(a, 0), num(a, 1), num(a, 2), num(a, 3)),
                    color,
                    alpha,
                    op == "clearRect",
                );
                Reply::Undefined
            }
            "drawImage" => {
                let (w, h) = (num(a, 0) as u32, num(a, 1) as u32);
                if u64::from(w) * u64::from(h) > MAX_PIXELS {
                    return Err("picture is too large".into());
                }
                let data = base64_decode(text(a, 2)).ok_or("bad picture data")?;
                let m = affine(a, 12);
                self.draw_image(
                    &m,
                    (w, h, &data),
                    (num(a, 3), num(a, 4), num(a, 5), num(a, 6)),
                    (num(a, 7), num(a, 8), num(a, 9), num(a, 10)),
                    num(a, 11).clamp(0.0, 1.0),
                );
                Reply::Undefined
            }
            "getImageData" => {
                let (w, h) = (num(a, 2) as u32, num(a, 3) as u32);
                if u64::from(w) * u64::from(h) > MAX_PIXELS {
                    return Err("region is too large".into());
                }
                Reply::Text(base64_encode(&self.read(
                    num(a, 0) as i64,
                    num(a, 1) as i64,
                    w,
                    h,
                )))
            }
            "putImageData" => {
                let (w, h) = (num(a, 1) as u32, num(a, 2) as u32);
                let data = base64_decode(text(a, 0)).ok_or("bad pixel data")?;
                if data.len() < (u64::from(w) * u64::from(h) * 4) as usize {
                    return Err("pixel data is too short".into());
                }
                self.write(num(a, 3) as i64, num(a, 4) as i64, w, h, &data);
                Reply::Undefined
            }
            "resize" => {
                self.resize(num(a, 0).max(1.0) as u32, num(a, 1).max(1.0) as u32)?;
                Reply::Undefined
            }
            other => return Err(format!("unknown canvas operation '{other}'")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Reply {
        Reply::Text(s.to_string())
    }
    fn n(x: f64) -> Reply {
        Reply::Number(x)
    }
    fn canvas(w: u32, h: u32) -> Canvas2d {
        Canvas2d::new(blueice_dom::NodeId::from_u64(1), w, h).unwrap()
    }
    fn px(c: &Canvas2d, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * c.w + x) * 4) as usize;
        [c.rgba[i], c.rgba[i + 1], c.rgba[i + 2], c.rgba[i + 3]]
    }

    #[test]
    fn a_filled_rectangle_covers_exactly_its_pixels() {
        let mut c = canvas(8, 8);
        c.call("fillRect", &[n(2.0), n(2.0), n(3.0), n(2.0), t("#ff0000")])
            .unwrap();
        assert_eq!(px(&c, 2, 2), [255, 0, 0, 255]);
        assert_eq!(px(&c, 4, 3), [255, 0, 0, 255]);
        assert_eq!(px(&c, 5, 3), [0, 0, 0, 0]);
        assert_eq!(px(&c, 2, 4), [0, 0, 0, 0]);
        assert!(c.dirty);
    }

    #[test]
    fn translucent_paint_blends_over_what_is_there_and_clear_removes_it() {
        let mut c = canvas(4, 4);
        c.call("fillRect", &[n(0.0), n(0.0), n(4.0), n(4.0), t("#0000ff")])
            .unwrap();
        c.call(
            "fillRect",
            &[n(0.0), n(0.0), n(4.0), n(4.0), t("#ff0000"), n(0.5)],
        )
        .unwrap();
        let p = px(&c, 1, 1);
        assert!(
            (i32::from(p[0]) - 128).abs() <= 1 && (i32::from(p[2]) - 127).abs() <= 1 && p[3] == 255,
            "{p:?}"
        );
        c.call("clearRect", &[n(0.0), n(0.0), n(2.0), n(4.0)])
            .unwrap();
        assert_eq!(px(&c, 1, 1), [0, 0, 0, 0]);
        assert_eq!(px(&c, 3, 1)[3], 255);
    }

    #[test]
    fn a_transform_moves_and_scales_the_rectangle() {
        let mut c = canvas(10, 10);
        c.call(
            "fillRect",
            &[
                n(0.0),
                n(0.0),
                n(2.0),
                n(2.0),
                t("lime"),
                n(1.0),
                t("[2,0,0,2,3,3]"),
            ],
        )
        .unwrap();
        assert_eq!(px(&c, 3, 3), [0, 255, 0, 255]);
        assert_eq!(px(&c, 6, 6), [0, 255, 0, 255]);
        assert_eq!(px(&c, 7, 7), [0, 0, 0, 0]);
        assert_eq!(px(&c, 2, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn a_picture_is_drawn_at_its_size_scaled_and_cropped() {
        let mut c = canvas(4, 4);
        // 2x1: left red, right blue.
        let src = base64_encode(&[255, 0, 0, 255, 0, 0, 255, 255]);
        c.call(
            "drawImage",
            &[
                n(2.0),
                n(1.0),
                t(&src),
                n(0.0),
                n(0.0),
                n(2.0),
                n(1.0),
                n(0.0),
                n(0.0),
                n(4.0),
                n(2.0),
                n(1.0),
            ],
        )
        .unwrap();
        assert_eq!(px(&c, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&c, 3, 1), [0, 0, 255, 255]);
        assert_eq!(px(&c, 0, 3), [0, 0, 0, 0]);
        // Only the right half, stretched over the whole canvas.
        let mut d = canvas(4, 4);
        d.call(
            "drawImage",
            &[
                n(2.0),
                n(1.0),
                t(&src),
                n(1.0),
                n(0.0),
                n(1.0),
                n(1.0),
                n(0.0),
                n(0.0),
                n(4.0),
                n(4.0),
                n(1.0),
            ],
        )
        .unwrap();
        assert_eq!(px(&d, 0, 0), [0, 0, 255, 255]);
        assert_eq!(px(&d, 3, 3), [0, 0, 255, 255]);
    }

    #[test]
    fn pixels_read_back_what_was_written_and_outside_is_transparent() {
        let mut c = canvas(4, 4);
        let two = base64_encode(&[1, 2, 3, 255, 4, 5, 6, 255]);
        c.call("putImageData", &[t(&two), n(2.0), n(1.0), n(1.0), n(1.0)])
            .unwrap();
        let Reply::Text(back) = c
            .call("getImageData", &[n(0.0), n(1.0), n(4.0), n(1.0)])
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            base64_decode(&back).unwrap(),
            vec![0, 0, 0, 0, 1, 2, 3, 255, 4, 5, 6, 255, 0, 0, 0, 0]
        );
        let Reply::Text(edge) = c
            .call("getImageData", &[n(-1.0), n(0.0), n(2.0), n(1.0)])
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(base64_decode(&edge).unwrap(), vec![0; 8]);
    }

    #[test]
    fn resizing_clears_and_oversized_requests_are_refused() {
        let mut c = canvas(2, 2);
        c.call("fillRect", &[n(0.0), n(0.0), n(2.0), n(2.0), t("red")])
            .unwrap();
        c.call("resize", &[n(3.0), n(1.0)]).unwrap();
        assert_eq!((c.w, c.h, px(&c, 0, 0)), (3, 1, [0, 0, 0, 0]));
        assert!(c.call("resize", &[n(100_000.0), n(100_000.0)]).is_err());
        assert!(Canvas2d::new(blueice_dom::NodeId::from_u64(1), 100_000, 100_000).is_none());
        assert!(c
            .call(
                "getImageData",
                &[n(0.0), n(0.0), n(100_000.0), n(100_000.0)]
            )
            .is_err());
        assert!(c.call("nonsense", &[]).is_err());
    }

    #[test]
    fn a_singular_transform_draws_nothing() {
        let mut c = canvas(4, 4);
        c.call(
            "fillRect",
            &[
                n(0.0),
                n(0.0),
                n(4.0),
                n(4.0),
                t("red"),
                n(1.0),
                t("[0,0,0,0,0,0]"),
            ],
        )
        .unwrap();
        assert_eq!(px(&c, 1, 1), [0, 0, 0, 0]);
    }
}
