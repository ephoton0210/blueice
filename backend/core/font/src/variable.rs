// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A variable font at one weight: glyph outlines moved by the font's variation tables
//! (`ttf-parser`), drawn into coverage bitmaps (`ab_glyph_rasterizer`) with the same metrics
//! shape `fontdue` gives static fonts.

use ab_glyph_rasterizer::{point, Point, Rasterizer};
use fontdue::{LineMetrics, Metrics, OutlineBounds};
use ttf_parser::{Face, Tag};

/// One variation axis of a font file, in the axis's own units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    pub tag: [u8; 4],
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

/// The axes this engine drives: weight, width, slant, optical size and italic.
const DRIVEN: [&[u8; 4]; 5] = [b"wght", b"wdth", b"slnt", b"opsz", b"ital"];

/// The driven axes of a font file (none for a static font).
pub(crate) fn axes(data: &[u8]) -> Vec<Axis> {
    let Ok(face) = Face::parse(data, 0) else { return Vec::new() };
    face.variation_axes()
        .into_iter()
        .filter_map(|a| {
            let tag = a.tag.to_bytes();
            DRIVEN.contains(&&tag).then_some(Axis { tag, min: a.min_value, default: a.def_value, max: a.max_value })
        })
        .collect()
}

/// What a text asks of a variable face's axes. Each is `None` when the text does not ask (the
/// face's default is used). Weight is the CSS number; width is a percentage, slant a angle in
/// degrees (negative leans right, as `slnt` does) and optical size a size in points, all three
/// in tenths; `ital` switches an `ital` axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Variation {
    pub wght: Option<u16>,
    pub wdth: Option<u16>,
    pub slnt: Option<i16>,
    pub opsz: Option<u16>,
    pub ital: Option<bool>,
}

impl Variation {
    pub const NONE: Variation = Variation { wght: None, wdth: None, slnt: None, opsz: None, ital: None };

    pub(crate) fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    /// The request as `axes` can meet it: an axis the face lacks is dropped, a value is
    /// clamped to the axis's range, and an italic switch stands in for a slant.
    pub fn resolve(&self, axes: &[Axis]) -> Variation {
        let find = |tag: &[u8; 4]| axes.iter().find(|a| &a.tag == tag);
        let tenths = |a: &Axis, v: f32| (v.clamp(a.min * 10.0, a.max * 10.0)).round();
        let mut out = Variation::NONE;
        if let (Some(a), Some(w)) = (find(b"wght"), self.wght) {
            out.wght = Some(f32::from(w).clamp(a.min, a.max).round() as u16);
        }
        if let (Some(a), Some(v)) = (find(b"wdth"), self.wdth) {
            out.wdth = Some(tenths(a, f32::from(v)) as u16);
        }
        if let (Some(a), Some(v)) = (find(b"slnt"), self.slnt) {
            out.slnt = Some(tenths(a, f32::from(v)) as i16);
        }
        if let (Some(a), Some(v)) = (find(b"opsz"), self.opsz) {
            out.opsz = Some(tenths(a, f32::from(v)) as u16);
        }
        if let (Some(_), Some(on)) = (find(b"ital"), self.ital) {
            out.ital = Some(on);
            if on {
                out.slnt = None;
            }
        }
        out
    }
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
    /// `data` with `v` (already [resolved](Variation::resolve) against its axes) applied. `None`
    /// if the file does not parse or an axis cannot be set.
    pub(crate) fn new(data: &'static [u8], v: &Variation) -> Option<VarInstance> {
        let mut face = Face::parse(data, 0).ok()?;
        let mut set = |tag: &[u8; 4], value: Option<f32>| match value {
            Some(value) => face.set_variation(Tag::from_bytes(tag), value).map(|_| ()),
            None => Some(()),
        };
        set(b"wght", v.wght.map(f32::from))?;
        set(b"wdth", v.wdth.map(|x| f32::from(x) / 10.0))?;
        set(b"slnt", v.slnt.map(|x| f32::from(x) / 10.0))?;
        set(b"opsz", v.opsz.map(|x| f32::from(x) / 10.0))?;
        set(b"ital", v.ital.map(|on| if on { 1.0 } else { 0.0 }))?;
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
