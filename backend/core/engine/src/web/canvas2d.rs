// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A small `CanvasRenderingContext2D`: a straight-alpha RGBA bitmap that
//! scripts fill with rectangles, draw pictures onto and read pixels back from.
//! Text is drawn and measured in the fonts the page has (`font`, `fillText`, `measureText`).
//! No paths yet -- those calls are accepted and draw nothing -- so
//! pages that use a canvas to read an image's pixels or to composite pictures
//! work, and everything else degrades to an empty canvas rather than failing.

use super::canvas_path::{self, Cap, Join, StrokeStyle, Subpath};
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

/// The subpaths a script sends: `[{"p":[x0,y0,x1,y1,...],"c":closed}, ...]`, bounded.
fn parse_paths(json: &str) -> Vec<Subpath> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut budget = 200_000_usize;
    let mut out = Vec::new();
    for item in list.iter().take(4096) {
        let numbers: Vec<f64> = item["p"].as_array().map_or(Vec::new(), |a| a.iter().filter_map(|n| n.as_f64()).collect());
        let points: Vec<(f64, f64)> = numbers.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        if points.len() > budget {
            break;
        }
        budget -= points.len();
        out.push(Subpath { points, closed: item["c"].as_i64().unwrap_or(0) != 0 });
    }
    out
}

/// One glyph's coverage bitmap and where its top-left lies in user space.
struct Placed {
    x: f64,
    y: f64,
    w: usize,
    h: usize,
    coverage: Vec<u8>,
}

/// The widest half-line `strokeText` draws, so a huge `lineWidth` stays cheap.
const MAX_STROKE_RADIUS: f64 = 16.0;

/// What a fill or stroke paints with: a colour or a gradient, in the context's user space.
enum Paint {
    Solid([u8; 4]),
    Linear { p: [f64; 4], stops: Vec<(f64, [u8; 4])> },
    Radial { p: [f64; 6], stops: Vec<(f64, [u8; 4])> },
}

impl Paint {
    /// A CSS colour, or `grad:` and the JSON a script's gradient object describes itself with.
    fn parse(text: &str) -> Paint {
        let Some(json) = text.strip_prefix("grad:") else {
            return Paint::Solid(
                blueice_css::parse_css_color(text).map_or([0, 0, 0, 255], |(r, g, b, a)| [r, g, b, a]),
            );
        };
        let transparent = Paint::Solid([0, 0, 0, 0]);
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return transparent };
        let nums: Vec<f64> =
            v["p"].as_array().map_or(Vec::new(), |a| a.iter().filter_map(|n| n.as_f64()).collect());
        let mut stops: Vec<(f64, [u8; 4])> = v["s"]
            .as_array()
            .map_or(Vec::new(), |a| a.iter().filter_map(Self::stop).collect());
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        match (v["t"].as_str(), nums.len()) {
            (Some("linear"), 4) => Paint::Linear { p: [nums[0], nums[1], nums[2], nums[3]], stops },
            (Some("radial"), 6) => {
                Paint::Radial { p: [nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]], stops }
            }
            _ => transparent,
        }
    }

    fn stop(v: &serde_json::Value) -> Option<(f64, [u8; 4])> {
        let pair = v.as_array()?;
        let offset = pair.first()?.as_f64()?.clamp(0.0, 1.0);
        let (r, g, b, a) = blueice_css::parse_css_color(pair.get(1)?.as_str()?)?;
        Some((offset, [r, g, b, a]))
    }

    fn ramp(stops: &[(f64, [u8; 4])], t: f64) -> [u8; 4] {
        let (Some(first), Some(last)) = (stops.first(), stops.last()) else { return [0, 0, 0, 0] };
        if t <= first.0 {
            return first.1;
        }
        if t >= last.0 {
            return last.1;
        }
        for pair in stops.windows(2) {
            let ((t0, c0), (t1, c1)) = (pair[0], pair[1]);
            if t >= t0 && t <= t1 {
                let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                let mix = |i: usize| (f64::from(c0[i]) + (f64::from(c1[i]) - f64::from(c0[i])) * k).round() as u8;
                return [mix(0), mix(1), mix(2), mix(3)];
            }
        }
        last.1
    }

    /// The colour at the user-space point `(x, y)`.
    fn at(&self, x: f64, y: f64) -> [u8; 4] {
        match self {
            Paint::Solid(c) => *c,
            Paint::Linear { p, stops } => {
                let (dx, dy) = (p[2] - p[0], p[3] - p[1]);
                let len2 = dx * dx + dy * dy;
                if len2 == 0.0 {
                    return [0, 0, 0, 0];
                }
                Self::ramp(stops, ((x - p[0]) * dx + (y - p[1]) * dy) / len2)
            }
            Paint::Radial { p, stops } => {
                // The largest s whose circle (centre and radius moving from the first circle to
                // the second) passes through the point and has a radius of at least 0.
                let (cx, cy, dr) = (p[3] - p[0], p[4] - p[1], p[5] - p[2]);
                let (dx, dy) = (x - p[0], y - p[1]);
                let a = cx * cx + cy * cy - dr * dr;
                let b = dx * cx + dy * cy + p[2] * dr;
                let c = dx * dx + dy * dy - p[2] * p[2];
                let valid = |s: f64| p[2] + s * dr >= 0.0;
                let s = if a.abs() < 1e-12 {
                    if b.abs() < 1e-12 { None } else { Some(c / (2.0 * b)).filter(|s| valid(*s)) }
                } else {
                    let disc = b * b - a * c;
                    if disc < 0.0 {
                        None
                    } else {
                        let root = disc.sqrt();
                        let (r1, r2) = ((b + root) / a, (b - root) / a);
                        let (hi, lo) = (r1.max(r2), r1.min(r2));
                        if valid(hi) { Some(hi) } else if valid(lo) { Some(lo) } else { None }
                    }
                };
                s.map_or([0, 0, 0, 0], |s| Self::ramp(stops, s))
            }
        }
    }
}

/// A canvas `font` string, resolved.
struct CanvasFont {
    size: f64,
    bold: bool,
    italic: bool,
    face: Option<blueice_font::web::WebFontId>,
}

/// A `font-stretch` keyword as a percentage in tenths.
fn stretch_keyword(word: &str) -> Option<u16> {
    Some(match word {
        "ultra-condensed" => 500,
        "extra-condensed" => 625,
        "condensed" => 750,
        "semi-condensed" => 875,
        "semi-expanded" => 1125,
        "expanded" => 1250,
        "extra-expanded" => 1500,
        "ultra-expanded" => 2000,
        _ => return None,
    })
}

impl CanvasFont {
    /// `[style] [weight] size[/line-height] family[, family]...`; anything else is the default
    /// `10px sans-serif`.
    fn parse(spec: &str, fonts: &crate::fonts::FontCatalog) -> CanvasFont {
        let default = CanvasFont { size: 10.0, bold: false, italic: false, face: None };
        let (mut weight, mut italic, mut stretch) = (400_u16, false, 1000_u16);
        let mut words = spec.split_whitespace();
        let mut size = None;
        for word in words.by_ref() {
            let lower = word.to_ascii_lowercase();
            let head = lower.split('/').next().unwrap_or("");
            let unit = |u: &str, k: f64| head.strip_suffix(u).and_then(|n| n.parse::<f64>().ok()).map(|n| n * k);
            let px = unit("px", 1.0)
                .or_else(|| unit("pt", 4.0 / 3.0))
                .or_else(|| unit("rem", 16.0))
                .or_else(|| unit("em", 16.0))
                .or_else(|| unit("%", 0.16));
            if let Some(px) = px {
                size = Some(px);
                break;
            }
            if let Some(s) = stretch_keyword(&lower) {
                stretch = s;
                continue;
            }
            match lower.as_str() {
                "italic" | "oblique" => italic = true,
                "bold" | "bolder" => weight = 700,
                "lighter" => weight = 300,
                "normal" | "small-caps" => {}
                n => match n.parse::<u16>() {
                    Ok(w) if (1..=1000).contains(&w) => weight = w,
                    _ => return default,
                },
            }
        }
        let Some(size) = size.filter(|s| s.is_finite() && *s > 0.0) else {
            return default;
        };
        let families: Vec<String> = words
            .collect::<Vec<_>>()
            .join(" ")
            .split(',')
            .map(|f| f.trim().trim_matches(|c| c == '"' || c == '\'').to_ascii_lowercase())
            .filter(|f| !f.is_empty())
            .collect();
        CanvasFont {
            size: size.min(1000.0),
            bold: weight >= 600,
            italic,
            face: if families.is_empty() {
                None
            } else {
                // A canvas follows the optical size of its font size, like `font-optical-sizing: auto`.
                let mut req = crate::fonts::FaceRequest::new(weight, italic);
                req.size = (size * 10.0).round().clamp(1.0, 10000.0) as u16;
                req.optical = true;
                req.stretch = stretch;
                fonts.face_for_request(&families.join(","), &req).0
            },
        }
    }

    fn width(&self, text: &str) -> f64 {
        blueice_font::measure_text_width_in(text, self.size, self.bold, self.italic, self.face)
    }
}

impl Canvas2d {
    /// The glyphs of `text` placed in user space (top-left of each bitmap), and the matrix to
    /// draw them with (squeezed when `max_width` is smaller than the text).
    #[allow(clippy::too_many_arguments)]
    fn place_text(
        text: &str,
        (x, y): (f64, f64),
        font: &CanvasFont,
        m: &Affine,
        (align, baseline): (&str, &str),
        max_width: f64,
    ) -> (Vec<Placed>, Affine) {
        let width = font.width(text);
        let (ascent, descent) =
            blueice_font::ascent_descent_in(font.size, font.bold, font.italic, font.face);
        let mut m = *m;
        if max_width >= 0.0 && width > max_width && width > 0.0 {
            // Wider than allowed: squeezed horizontally about the anchor.
            let s = max_width / width;
            let squeeze = [s, 0.0, 0.0, 1.0, x * (1.0 - s), 0.0];
            m = [
                m[0] * squeeze[0],
                m[1] * squeeze[0],
                m[2],
                m[3],
                m[0] * squeeze[4] + m[4],
                m[1] * squeeze[4] + m[5],
            ];
        }
        let left = match align {
            "center" => x - width / 2.0,
            "right" | "end" => x - width,
            _ => x,
        };
        let base = match baseline {
            "top" => y + ascent,
            "hanging" => y + ascent * 0.8,
            "middle" => y + (ascent - descent) / 2.0,
            "bottom" | "ideographic" => y - descent,
            _ => y,
        };
        let mut pen = left;
        let mut out = Vec::new();
        for ch in text.chars() {
            let face = blueice_font::font_for_char_in(ch, font.bold, font.italic, font.face);
            let (metrics, bitmap) = face.rasterize(ch, font.size as f32);
            let (gx, gy) = (pen + f64::from(metrics.xmin), base - f64::from(metrics.ymin) - metrics.height as f64);
            pen += f64::from(metrics.advance_width);
            if metrics.width > 0 && metrics.height > 0 {
                out.push(Placed { x: gx, y: gy, w: metrics.width, h: metrics.height, coverage: bitmap });
            }
        }
        (out, m)
    }

    /// Paints `coverage` (a `w` by `h` mask whose top-left is the user-space point `at`) with
    /// `paint` through the matrix `m`.
    fn paint_mask(&mut self, m: &Affine, at: (f64, f64), (w, h): (usize, usize), coverage: &[u8], paint: &Paint, alpha: f64) {
        let Some(inv) = invert(m) else { return };
        let (x0, y0, x1, y1) = self.bounds(m, at.0, at.1, w as f64, h as f64);
        for py in y0..y1 {
            for px in x0..x1 {
                let (u, v) = apply(&inv, px as f64 + 0.5, py as f64 + 0.5);
                let (cx, cy) = ((u - at.0).floor(), (v - at.1).floor());
                if cx < 0.0 || cy < 0.0 || cx >= w as f64 || cy >= h as f64 {
                    continue;
                }
                let c = f64::from(coverage[cy as usize * w + cx as usize]) / 255.0;
                if c > 0.0 {
                    let i = ((py as u32 * self.w + px as u32) * 4) as usize;
                    over(&mut self.rgba[i..i + 4], paint.at(u, v), alpha * c);
                }
            }
        }
        self.dirty = true;
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_text(
        &mut self,
        text: &str,
        at: (f64, f64),
        font: &CanvasFont,
        paint: &Paint,
        alpha: f64,
        m: &Affine,
        place: (&str, &str),
        max_width: f64,
    ) {
        let (glyphs, m) = Self::place_text(text, at, font, m, place, max_width);
        for g in &glyphs {
            self.paint_mask(&m, (g.x, g.y), (g.w, g.h), &g.coverage, paint, alpha);
        }
    }

    /// The outline of the text, `line_width` wide and centred on the glyph edges: what lies
    /// within half a line of the edge, found by growing and shrinking the glyph coverage.
    #[allow(clippy::too_many_arguments)]
    fn stroke_text(
        &mut self,
        text: &str,
        at: (f64, f64),
        font: &CanvasFont,
        paint: &Paint,
        alpha: f64,
        m: &Affine,
        place: (&str, &str),
        max_width: f64,
        line_width: f64,
    ) {
        let (glyphs, m) = Self::place_text(text, at, font, m, place, max_width);
        let radius = (line_width / 2.0).clamp(0.0, MAX_STROKE_RADIUS);
        if glyphs.is_empty() || radius <= 0.0 {
            return;
        }
        let pad = radius.ceil() as i64 + 1;
        let left = glyphs.iter().map(|g| g.x).fold(f64::INFINITY, f64::min).floor() as i64 - pad;
        let top = glyphs.iter().map(|g| g.y).fold(f64::INFINITY, f64::min).floor() as i64 - pad;
        let right = glyphs.iter().map(|g| g.x + g.w as f64).fold(f64::NEG_INFINITY, f64::max).ceil() as i64 + pad;
        let bottom = glyphs.iter().map(|g| g.y + g.h as f64).fold(f64::NEG_INFINITY, f64::max).ceil() as i64 + pad;
        let (w, h) = ((right - left) as usize, (bottom - top) as usize);
        if w * h > 4_000_000 {
            return;
        }
        let mut shape = vec![0u8; w * h];
        for g in &glyphs {
            let (ox, oy) = (g.x.round() as i64 - left, g.y.round() as i64 - top);
            for row in 0..g.h {
                for col in 0..g.w {
                    let i = (oy as usize + row) * w + ox as usize + col;
                    shape[i] = shape[i].max(g.coverage[row * g.w + col]);
                }
            }
        }
        let r = radius.ceil() as i64;
        let offsets: Vec<(i64, i64)> = (-r..=r)
            .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| ((dx * dx + dy * dy) as f64) <= radius * radius + 0.25)
            .collect();
        let mut band = vec![0u8; w * h];
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let (mut grown, mut shrunk) = (0u8, 255u8);
                for &(dx, dy) in &offsets {
                    let (sx, sy) = (x + dx, y + dy);
                    let v = if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
                        0
                    } else {
                        shape[sy as usize * w + sx as usize]
                    };
                    grown = grown.max(v);
                    shrunk = shrunk.min(v);
                }
                band[y as usize * w + x as usize] = grown.min(255 - shrunk);
            }
        }
        self.paint_mask(&m, (left as f64, top as f64), (w, h), &band, paint, alpha);
    }

    /// Paints `polygons` (user space) through `m` with `paint`.
    fn paint_polygons(
        &mut self,
        polygons: &[Vec<canvas_path::Point>],
        m: &Affine,
        even_odd: bool,
        paint: &Paint,
        alpha: f64,
    ) {
        let Some(inv) = invert(m) else { return };
        let device: Vec<Vec<canvas_path::Point>> = polygons
            .iter()
            .map(|poly| poly.iter().map(|&(x, y)| apply(m, x, y)).collect())
            .collect();
        let Some(mask) = canvas_path::rasterize(&device, even_odd, self.w, self.h) else { return };
        for row in 0..mask.h {
            for col in 0..mask.w {
                let c = mask.coverage[row * mask.w + col];
                if c == 0 {
                    continue;
                }
                let (px, py) = (mask.x + col as i64, mask.y + row as i64);
                let (u, v) = apply(&inv, px as f64 + 0.5, py as f64 + 0.5);
                let i = ((py as u32 * self.w + px as u32) * 4) as usize;
                over(&mut self.rgba[i..i + 4], paint.at(u, v), alpha * f64::from(c) / 255.0);
            }
        }
        self.dirty = true;
    }

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
        paint: &Paint,
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
                    over(&mut self.rgba[i..i + 4], paint.at(u, v), alpha);
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
    #[cfg(test)]
    pub fn call(&mut self, op: &str, a: &[Reply]) -> Result<Reply, String> {
        self.call_with_fonts(op, a, &crate::fonts::FontCatalog::default())
    }

    /// [`call`](Self::call), with the faces scripts added to the page for `font` to name.
    pub fn call_with_fonts(
        &mut self,
        op: &str,
        a: &[Reply],
        fonts: &crate::fonts::FontCatalog,
    ) -> Result<Reply, String> {
        Ok(match op {
            "fillText" | "strokeText" => {
                // text, x, y, font, paint, alpha, matrix, align, baseline, maxWidth (<0: none),
                // lineWidth (strokeText)
                let font = CanvasFont::parse(text(a, 3), fonts);
                let paint = Paint::parse(text(a, 4));
                let max = if a.get(9).is_some() { num(a, 9) } else { -1.0 };
                let (at, alpha, m) = ((num(a, 1), num(a, 2)), num(a, 5).clamp(0.0, 1.0), affine(a, 6));
                let place = (text(a, 7), text(a, 8));
                if op == "fillText" {
                    self.fill_text(text(a, 0), at, &font, &paint, alpha, &m, place, max);
                } else {
                    self.stroke_text(text(a, 0), at, &font, &paint, alpha, &m, place, max, num(a, 10));
                }
                Reply::Undefined
            }
            // text, font, align, baseline: width, then the ink's extents from the alignment point
            // (left and right either side, ascent above the baseline, descent below) and the
            // font's own, as comma-separated numbers.
            "measureBox" => {
                let font = CanvasFont::parse(text(a, 1), fonts);
                let (glyphs, _) = Self::place_text(
                    text(a, 0),
                    (0.0, 0.0),
                    &font,
                    &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    (text(a, 2), text(a, 3)),
                    -1.0,
                );
                let min = |f: fn(&Placed) -> f64| glyphs.iter().map(f).fold(f64::INFINITY, f64::min);
                let max = |f: fn(&Placed) -> f64| glyphs.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
                let (left, right, ascent, descent) = if glyphs.is_empty() {
                    (0.0, 0.0, 0.0, 0.0)
                } else {
                    (-min(|g| g.x), max(|g| g.x + g.w as f64), -min(|g| g.y), max(|g| g.y + g.h as f64))
                };
                let (fa, fd) = blueice_font::ascent_descent_in(font.size, font.bold, font.italic, font.face);
                Reply::Text(format!("{},{left},{right},{ascent},{descent},{fa},{fd}", font.width(text(a, 0))))
            }
            "measureText" => {
                let font = CanvasFont::parse(text(a, 1), fonts);
                Reply::Number(font.width(text(a, 0)))
            }
            // path (JSON), paint, alpha, matrix, even-odd (fillPath); and line width, cap, join,
            // miter limit (strokePath).
            "fillPath" | "strokePath" => {
                let paths = parse_paths(text(a, 0));
                let (paint, alpha, m) = (Paint::parse(text(a, 1)), num(a, 2).clamp(0.0, 1.0), affine(a, 3));
                if op == "fillPath" {
                    let even_odd = num(a, 4) != 0.0;
                    self.paint_polygons(&canvas_path::fill_polygons(&paths), &m, even_odd, &paint, alpha);
                } else {
                    let style = StrokeStyle {
                        width: num(a, 4),
                        cap: match text(a, 5) {
                            "round" => Cap::Round,
                            "square" => Cap::Square,
                            _ => Cap::Butt,
                        },
                        join: match text(a, 6) {
                            "round" => Join::Round,
                            "bevel" => Join::Bevel,
                            _ => Join::Miter,
                        },
                        miter_limit: if a.get(7).is_some() { num(a, 7).max(1.0) } else { 10.0 },
                    };
                    self.paint_polygons(&canvas_path::stroke_polygons(&paths, &style), &m, false, &paint, alpha);
                }
                Reply::Undefined
            }
            "fillRect" | "clearRect" => {
                let paint = Paint::parse(text(a, 4));
                let alpha = if a.get(5).is_some() {
                    num(a, 5).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let m = affine(a, 6);
                self.fill_rect(
                    &m,
                    (num(a, 0), num(a, 1), num(a, 2), num(a, 3)),
                    &paint,
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

    fn ink(c: &Canvas2d) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for y in 0..c.h {
            for x in 0..c.w {
                if px(c, x, y)[3] > 0 {
                    out.push((x, y));
                }
            }
        }
        out
    }

    fn fill_text(c: &mut Canvas2d, args: &[Reply], fonts: &crate::fonts::FontCatalog) {
        c.call_with_fonts("fillText", args, fonts).unwrap();
    }

    /// text, x, y, font, color, alpha, matrix, align, baseline, maxWidth
    fn text_args(s: &str, x: f64, y: f64, font: &str, align: &str) -> Vec<Reply> {
        vec![
            t(s), n(x), n(y), t(font), t("#ff0000"), n(1.0),
            t("[1,0,0,1,0,0]"), t(align), t("alphabetic"), n(-1.0),
        ]
    }

    #[test]
    fn filled_text_puts_ink_on_the_baseline_in_the_requested_colour() {
        let mut c = canvas(120, 60);
        let none = crate::fonts::FontCatalog::default();
        fill_text(&mut c, &text_args("Hi", 10.0, 40.0, "20px sans-serif", "start"), &none);
        let ink = ink(&c);
        assert!(!ink.is_empty());
        assert!(ink.iter().all(|&(x, y)| x >= 8 && (20..=42).contains(&y)), "{ink:?}");
        assert!(ink.iter().any(|&(x, y)| px(&c, x, y) == [255, 0, 0, 255]), "solid red inside the stems");
        // Bigger text covers more.
        let mut big = canvas(120, 60);
        fill_text(&mut big, &text_args("Hi", 10.0, 50.0, "40px sans-serif", "start"), &none);
        assert!(super::tests::ink(&big).len() > 2 * ink.len());
    }

    #[test]
    fn alignment_moves_the_ink_to_either_side_of_the_point() {
        let none = crate::fonts::FontCatalog::default();
        let extent = |align: &str| {
            let mut c = canvas(200, 40);
            fill_text(&mut c, &text_args("Hello", 100.0, 30.0, "20px sans-serif", align), &none);
            let i = ink(&c);
            (i.iter().map(|p| p.0).min().unwrap(), i.iter().map(|p| p.0).max().unwrap())
        };
        let (l0, l1) = extent("left");
        let (r0, r1) = extent("right");
        let (c0, c1) = extent("center");
        assert!(l0 >= 98 && l1 > 100);
        assert!(r1 <= 102 && r0 < 100);
        assert!(c0 < 100 && c1 > 100);
        assert!((i64::from(l1 - l0) - i64::from(r1 - r0)).abs() <= 2);
    }

    #[test]
    fn measuring_text_uses_the_real_widths_of_the_font_asked_for() {
        let mut c = canvas(10, 10);
        let none = crate::fonts::FontCatalog::default();
        let mut w = |font: &str| match c.call_with_fonts("measureText", &[t("Hello"), t(font)], &none).unwrap() {
            Reply::Number(w) => w,
            other => panic!("{other:?}"),
        };
        let expect = blueice_font::measure_text_width("Hello", 16.0, false, false);
        assert!((w("16px sans-serif") - expect).abs() < 0.01);
        assert!((w("32px sans-serif") - 2.0 * expect).abs() < 0.01);
        assert!(w("bold 16px sans-serif") > expect);
    }

    #[test]
    fn a_family_the_script_added_is_measured_and_drawn() {
        let bytes = include_bytes!("../../../font/tests/data/roboto-latin-400.woff2");
        let mut catalog = crate::fonts::FontCatalog::default();
        assert!(catalog.add_script_font(&crate::fonts::ScriptFont {
            family: "Dyn".into(), weight_min: 400, weight_max: 400, italic: false, stretch_min: 1000, stretch_max: 1000, ranges: vec![], bytes: bytes.to_vec(),
        }));
        let mut c = canvas(10, 10);
        let mut w = |font: &str| match c.call_with_fonts("measureText", &[t("Hello"), t(font)], &catalog).unwrap() {
            Reply::Number(w) => w,
            other => panic!("{other:?}"),
        };
        let own = blueice_font::measure_text_width_in("Hello", 16.0, false, false, catalog.face_for("dyn", 400, false));
        assert!((w("16px \"Dyn\", serif") - own).abs() < 0.01);
        assert!((w("16px sans-serif") - own).abs() > 0.5);
    }

    const RED_TO_BLUE: &str =
        r##"grad:{"t":"linear","p":[0,0,100,0],"s":[[0,"#ff0000"],[1,"#0000ff"]]}"##;

    #[test]
    fn a_linear_gradient_fills_a_rectangle_with_colours_along_its_line() {
        let mut c = canvas(100, 4);
        c.call(
            "fillRect",
            &[n(0.0), n(0.0), n(100.0), n(4.0), t(RED_TO_BLUE), n(1.0), t("[1,0,0,1,0,0]")],
        )
        .unwrap();
        let (left, mid, right) = (px(&c, 2, 1), px(&c, 50, 1), px(&c, 97, 1));
        assert!(left[0] > 240 && left[2] < 15, "{left:?}");
        assert!((120..136).contains(&mid[0]) && (120..136).contains(&mid[2]), "{mid:?}");
        assert!(right[0] < 15 && right[2] > 240, "{right:?}");
        assert_eq!(left[3], 255);
    }

    #[test]
    fn a_radial_gradient_runs_from_its_inner_to_its_outer_circle() {
        let spec = r##"grad:{"t":"radial","p":[20,20,0,20,20,20],"s":[[0,"#ffffff"],[1,"#000000"]]}"##;
        let mut c = canvas(40, 40);
        c.call("fillRect", &[n(0.0), n(0.0), n(40.0), n(40.0), t(spec), n(1.0), t("[1,0,0,1,0,0]")]).unwrap();
        assert!(px(&c, 20, 20)[0] > 245, "{:?}", px(&c, 20, 20));
        let mid = px(&c, 30, 20)[0];
        assert!((110..145).contains(&mid), "{mid}");
        assert!(px(&c, 1, 20)[0] < 25, "{:?}", px(&c, 1, 20));
        assert_eq!(px(&c, 39, 39)[0], 0, "beyond the outer circle the last stop holds");
    }

    #[test]
    fn text_filled_with_a_gradient_takes_the_colour_under_each_pixel() {
        let mut c = canvas(100, 40);
        let none = crate::fonts::FontCatalog::default();
        let mut args = text_args("HHHHH", 0.0, 30.0, "30px sans-serif", "start");
        args[4] = t(RED_TO_BLUE);
        fill_text(&mut c, &args, &none);
        let ink = ink(&c);
        let (first, last) = (ink.iter().min_by_key(|p| p.0).unwrap(), ink.iter().max_by_key(|p| p.0).unwrap());
        let (a, b) = (px(&c, first.0, first.1), px(&c, last.0, last.1));
        assert!(a[0] > a[2] + 100, "left of the text is red: {a:?}");
        assert!(b[2] > b[0] + 50, "right of the text is blue: {b:?}");
    }

    #[test]
    fn stroked_text_draws_the_outline_and_leaves_the_inside_alone() {
        let none = crate::fonts::FontCatalog::default();
        let mut filled = canvas(80, 100);
        fill_text(&mut filled, &text_args("I", 20.0, 90.0, "90px sans-serif", "start"), &none);
        let stroke = |width: f64| {
            let mut c = canvas(80, 100);
            let mut args = text_args("I", 20.0, 90.0, "90px sans-serif", "start");
            args.push(n(width));
            c.call_with_fonts("strokeText", &args, &none).unwrap();
            c
        };
        let thin = stroke(2.0);
        let thick = stroke(6.0);
        let (fa, ta, ha) = (ink(&filled), ink(&thin), ink(&thick));
        assert!(!ta.is_empty());
        assert!(ta.len() * 10 < fa.len() * 8, "an outline is lighter than the fill: {} vs {}", ta.len(), fa.len());
        assert!(ha.len() > ta.len(), "a wider line covers more");
        // The middle of the stem is solid in the fill and empty in the thin outline.
        let (cx, cy) = {
            let xs: Vec<u32> = fa.iter().map(|p| p.0).collect();
            ((xs.iter().min().unwrap() + xs.iter().max().unwrap()) / 2, 60)
        };
        assert_eq!(px(&filled, cx, cy)[3], 255);
        assert_eq!(px(&thin, cx, cy)[3], 0, "the inside of a thin outline is not painted");
        // The outline lies on the glyph's edge: it reaches just outside the filled ink too.
        let left_fill = fa.iter().map(|p| p.0).min().unwrap();
        assert!(ta.iter().map(|p| p.0).min().unwrap() < left_fill, "half the line is outside");
    }

    #[test]
    fn canvas_text_in_a_variable_face_follows_the_optical_size_of_its_font_size() {
        let bytes = include_bytes!("../../../font/tests/data/inter-latin-opsz.woff2");
        let mut catalog = crate::fonts::FontCatalog::default();
        assert!(catalog.add_script_font(&crate::fonts::ScriptFont {
            family: "Opt".into(), weight_min: 100, weight_max: 900, italic: false, stretch_min: 1000, stretch_max: 1000, ranges: vec![], bytes: bytes.to_vec(),
        }));
        let mut c = canvas(10, 10);
        let mut w = |font: &str| match c.call_with_fonts("measureText", &[t("Hamburgefonstiv"), t(font)], &catalog).unwrap() {
            Reply::Number(w) => w,
            other => panic!("{other:?}"),
        };
        let (small, large) = (w("14px Opt"), w("28px Opt"));
        assert!((large / small - 2.0).abs() > 0.01, "{small} {large}: the large size is drawn in the display cut");
    }

    #[test]
    fn a_canvas_font_shorthand_can_ask_for_a_width() {
        let bytes = include_bytes!("../../../font/tests/data/opensans-latin-wdth.woff2");
        let mut catalog = crate::fonts::FontCatalog::default();
        assert!(catalog.add_script_font(&crate::fonts::ScriptFont {
            family: "Wide".into(), weight_min: 300, weight_max: 800, italic: false, stretch_min: 750, stretch_max: 1000,
            ranges: vec![], bytes: bytes.to_vec(),
        }));
        let mut c = canvas(10, 10);
        let mut w = |font: &str| match c.call_with_fonts("measureText", &[t("Hamburgefonstiv"), t(font)], &catalog).unwrap() {
            Reply::Number(w) => w,
            other => panic!("{other:?}"),
        };
        let (normal, condensed) = (w("20px Wide"), w("condensed 20px Wide"));
        assert!(condensed < normal * 0.9, "{condensed} vs {normal}");
        assert!((w("ultra-expanded 20px Wide") - normal).abs() < 0.01, "held to the declared range");
    }

    #[test]
    fn measuring_a_box_reports_where_the_ink_lies_around_the_alignment_point() {
        let mut c = canvas(10, 10);
        let none = crate::fonts::FontCatalog::default();
        let mut measure = |text: &str, align: &str, baseline: &str| -> Vec<f64> {
            match c.call_with_fonts("measureBox", &[t(text), t("40px sans-serif"), t(align), t(baseline)], &none).unwrap() {
                Reply::Text(s) => s.split(',').map(|n| n.parse().unwrap()).collect(),
                other => panic!("{other:?}"),
            }
        };
        let h = measure("H", "left", "alphabetic");
        let g = measure("g", "left", "alphabetic");
        let (width, ascent, descent, font_ascent) = (h[0], h[3], h[4], h[5]);
        assert!(ascent > 20.0 && ascent < font_ascent, "cap height lies under the font ascent: {h:?}");
        assert!(descent < 1.5, "H sits on the baseline: {h:?}");
        assert!(g[4] > 5.0, "g hangs below it: {g:?}");
        assert!(h[2] > 20.0 && h[2] <= width + 1.0, "the right edge is inside the advance: {h:?}");
        let centred = measure("H", "center", "alphabetic");
        assert!(centred[1] > 8.0 && centred[1] <= width / 2.0 && centred[2] > 8.0 && centred[2] <= width / 2.0, "ink lies either side of the centre, inside the half advance: {centred:?}");
        let top = measure("H", "left", "top");
        assert!(top[3] < 2.0 && top[4] > 20.0, "with the top baseline the ink hangs below: {top:?}");
        assert_eq!(measure("", "left", "alphabetic")[1..5], [0.0; 4]);
    }
}
