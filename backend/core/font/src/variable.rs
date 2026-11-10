// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A variable font at one weight: glyph outlines moved by the font's variation tables
//! (`ttf-parser`), drawn into coverage bitmaps (`ab_glyph_rasterizer`) with the same metrics
//! shape `fontdue` gives static fonts.

use ab_glyph_rasterizer::{point, Point, Rasterizer};
use fontdue::{LineMetrics, Metrics, OutlineBounds};
use ttf_parser::{Face, Tag};

/// The `wght` axis of a font file: `(minimum, default, maximum)`, or `None` if it has none.
pub(crate) fn weight_axis(data: &[u8]) -> Option<(f32, f32, f32)> {
    let face = Face::parse(data, 0).ok()?;
    face.variation_axes()
        .into_iter()
        .find(|a| a.tag == Tag::from_bytes(b"wght"))
        .map(|a| (a.min_value, a.def_value, a.max_value))
}

pub struct VarInstance {
    face: Face<'static>,
    scale_per_px: f32,
}

struct Outline {
    raster: Rasterizer,
    scale: f32,
    xmin: f32,
    ytop: f32,
    start: Point,
    last: Point,
}

impl Outline {
    fn at(&self, x: f32, y: f32) -> Point {
        point(x * self.scale - self.xmin, self.ytop - y * self.scale)
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.at(x, y);
        self.last = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.raster.draw_line(self.last, p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.at(x1, y1), self.at(x, y));
        self.raster.draw_quad(self.last, c, p);
        self.last = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, p) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.raster.draw_cubic(self.last, a, b, p);
        self.last = p;
    }
    fn close(&mut self) {
        self.raster.draw_line(self.last, self.start);
        self.last = self.start;
    }
}

impl VarInstance {
    /// `data` at weight `wght`. `None` if the file does not parse or has no such axis.
    pub(crate) fn new(data: &'static [u8], wght: f32) -> Option<VarInstance> {
        let mut face = Face::parse(data, 0).ok()?;
        face.set_variation(Tag::from_bytes(b"wght"), wght)?;
        let upem = f32::from(face.units_per_em());
        (upem > 0.0).then(|| VarInstance { face, scale_per_px: 1.0 / upem })
    }

    pub fn has_glyph(&self, c: char) -> bool {
        self.face.glyph_index(c).is_some()
    }

    fn scale(&self, px: f32) -> f32 {
        px * self.scale_per_px
    }

    fn advance(&self, c: char, px: f32) -> f32 {
        let id = self.face.glyph_index(c).unwrap_or(ttf_parser::GlyphId(0));
        f32::from(self.face.glyph_hor_advance(id).unwrap_or(0)) * self.scale(px)
    }

    /// The pixel box of `c`'s outline: `(xmin, ymin, width, height)`, `ymin` being the lowest
    /// row above the baseline, as `fontdue` reports.
    fn pixel_box(&self, c: char, px: f32) -> Option<(i32, i32, usize, usize)> {
        let id = self.face.glyph_index(c)?;
        let b = self.face.glyph_bounding_box(id)?;
        let s = self.scale(px);
        let (x0, x1) = ((f32::from(b.x_min) * s).floor() as i32, (f32::from(b.x_max) * s).ceil() as i32);
        let (y0, y1) = ((f32::from(b.y_min) * s).floor() as i32, (f32::from(b.y_max) * s).ceil() as i32);
        let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
        (w > 0 && h > 0 && w * h <= 4_000_000).then_some((x0, y0, w, h))
    }

    pub fn metrics(&self, c: char, px: f32) -> Metrics {
        let (xmin, ymin, width, height) = self.pixel_box(c, px).unwrap_or((0, 0, 0, 0));
        Metrics {
            xmin,
            ymin,
            width,
            height,
            advance_width: self.advance(c, px),
            advance_height: 0.0,
            bounds: OutlineBounds { xmin: xmin as f32, ymin: ymin as f32, width: width as f32, height: height as f32 },
        }
    }

    pub fn rasterize(&self, c: char, px: f32) -> (Metrics, Vec<u8>) {
        let metrics = self.metrics(c, px);
        let (Some(id), true) = (self.face.glyph_index(c), metrics.width > 0) else {
            return (metrics, Vec::new());
        };
        let mut outline = Outline {
            raster: Rasterizer::new(metrics.width, metrics.height),
            scale: self.scale(px),
            xmin: metrics.xmin as f32,
            ytop: (metrics.ymin + metrics.height as i32) as f32,
            start: point(0.0, 0.0),
            last: point(0.0, 0.0),
        };
        self.face.outline_glyph(id, &mut outline);
        let mut bitmap = vec![0_u8; metrics.width * metrics.height];
        outline.raster.for_each_pixel(|i, a| bitmap[i] = (a.clamp(0.0, 1.0) * 255.0).round() as u8);
        (metrics, bitmap)
    }

    pub fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        let s = self.scale(px);
        let ascent = f32::from(self.face.ascender()) * s;
        let descent = f32::from(self.face.descender()) * s;
        let gap = f32::from(self.face.line_gap()) * s;
        Some(LineMetrics { ascent, descent, line_gap: gap, new_line_size: ascent - descent + gap })
    }
}
