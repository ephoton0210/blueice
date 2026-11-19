// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Rasterization: a `blueice_paint::Frame` (a display list) -> actual
//! RGBA8 pixels, per `phase-4-human-rendering-path/PLAN.md` (the step
//! `research/paint.md` explicitly left for later, since both reference
//! engines keep it a distinct stage from paint itself).
//!
//! Font loading/parsing lives in `blueice-font`, shared with
//! `blueice-layout` so both stages agree on exactly how wide a given
//! run of text is (see that crate's module docs for the real bug this
//! fixed -- layout used to approximate widths independently of what
//! got rasterized here, and the two disagreed visibly for bold text).
//! This crate only rasterizes: given the font `blueice-font` loaded,
//! walk each glyph's bitmap into the pixel buffer. Text positioning
//! still has one approximation of its own: layout doesn't compute real
//! ascent/descent metrics (see `blueice-layout::text`'s module docs),
//! so this module assumes a fixed ascent ratio (`ASSUMED_ASCENT_RATIO`)
//! for baseline placement rather than per-font-file real metrics.
//! Colors are composited assuming everything drawn is either fully
//! opaque or blended with simple linear alpha -- `blueice-css`'s color
//! parsing never actually produces a non-255 alpha today (no
//! `rgba()`/`hsla()` functions are in the MVP CSS value scope), so this
//! is forward-looking correctness, not yet exercised by real content.

mod shapes;

use blueice_font::{font_for_char_in, web::WebFontId};
use blueice_paint::{Color, Frame, PaintCommand};


/// Where an upright character's baseline is in its cell, as a fraction of the font size: the
/// em box of an ideograph hangs from the top of the cell.
const UPRIGHT_ASCENT_RATIO: f64 = 0.86;
#[derive(Clone, Copy)]
struct PixelClip {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl PixelClip {
    fn intersect(self, rect: blueice_paint::Rect) -> Self {
        if ![rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|value| value.is_finite())
            || rect.width <= 0.0
            || rect.height <= 0.0
        {
            return Self {
                right: self.left,
                bottom: self.top,
                ..self
            };
        }
        let left = self.left.max(rect.x.ceil() as i64);
        let top = self.top.max(rect.y.ceil() as i64);
        Self {
            left,
            top,
            right: self
                .right
                .min((rect.x + rect.width).ceil() as i64)
                .max(left),
            bottom: self
                .bottom
                .min((rect.y + rect.height).ceil() as i64)
                .max(top),
        }
    }
}

mod round_clip;

const BACKGROUND: [u8; 4] = [255, 255, 255, 255];

struct TextRun<'a> {
    x: f64,
    y: f64,
    text: &'a str,
    color: Color,
    font_size_px: f64,
    bold: bool,
    italic: bool,
    face: Option<WebFontId>,
    rtl: bool,
    orient: blueice_paint::TextOrient,
    breadth: f64,
    features: blueice_font::FeaturesId,
}

impl TextRun<'_> {
    /// How far to smear and slant a glyph of `font` to give the run the weight and slant it asks
    /// for: a page's own face is made bold and slanted when the run asks, and so is a bundled face
    /// that exists in Regular only (the CJK and script faces). Bundled DejaVu has its own.
    fn synthesized_style(&self, font: blueice_font::Glyphs) -> (i64, f64) {
        let synthesize = self.face.is_some() || blueice_font::is_regular_only_fallback(font);
        let smear = if synthesize && self.bold {
            (self.font_size_px / 20.0).max(1.0).round() as i64
        } else {
            0
        };
        (smear, if synthesize && self.italic { 0.2 } else { 0.0 })
    }
}

/// An RGBA8, row-major pixel buffer -- the engine's final rendered
/// output for one frame, ready to hand to a frontend's presentation
/// surface (a `softbuffer` window for the reference frontend; a
/// platform-native texture import for a later real frontend, per
/// `research/frontend-ipc.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl From<PixelClip> for shapes::Clip {
    fn from(c: PixelClip) -> Self {
        shapes::Clip {
            left: c.left,
            top: c.top,
            right: c.right,
            bottom: c.bottom,
        }
    }
}

impl Pixmap {
    fn canvas(&mut self) -> shapes::Canvas<'_> {
        shapes::Canvas {
            width: self.width,
            height: self.height,
            pixels: &mut self.pixels,
        }
    }

    fn filled(width: u32, height: u32, color: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            pixels.extend_from_slice(&color);
        }
        Pixmap {
            width,
            height,
            pixels,
        }
    }

    fn blank(width: u32, height: u32) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            pixels.extend_from_slice(&BACKGROUND);
        }
        Pixmap {
            width,
            height,
            pixels,
        }
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ]
    }

    fn set_pixel_blended(&mut self, x: i64, y: i64, color: [u8; 4], clip: PixelClip) {
        let [r, g, b, a] = color;
        if x < clip.left || y < clip.top || x >= clip.right || y >= clip.bottom || a == 0 {
            return;
        }
        let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
        if a == 255 {
            self.pixels[idx..idx + 4].copy_from_slice(&[r, g, b, 255]);
            return;
        }
        let a = a as u32;
        let inv = 255 - a;
        for (i, channel) in [r, g, b].into_iter().enumerate() {
            let existing = self.pixels[idx + i] as u32;
            self.pixels[idx + i] = ((channel as u32 * a + existing * inv) / 255) as u8;
        }
    }

    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Color, clip: PixelClip) {
        let Color::Rgba(r, g, b, a) = color else {
            return;
        };
        let (x0, y0) = (
            (x.round() as i64).max(clip.left),
            (y.round() as i64).max(clip.top),
        );
        let (x1, y1) = (
            ((x + w).round() as i64).min(clip.right),
            ((y + h).round() as i64).min(clip.bottom),
        );
        for py in y0..y1 {
            for px in x0..x1 {
                self.set_pixel_blended(px, py, [r, g, b, a], clip);
            }
        }
    }

    fn draw_text(&mut self, run: TextRun<'_>, clip: PixelClip) {
        let Color::Rgba(r, g, b, a) = run.color else {
            return;
        };
        // A page's own face puts the baseline where its own ascent says (layout aligned the
        // line with the same figure); the bundled fonts keep the assumed ratio.
        let ascent = blueice_font::baseline_offset(run.font_size_px, run.bold, run.italic, run.face);
        let baseline_y = run.y + ascent;
        // A page face that lacks the weight or slant asked for is made bold by smearing each
        // glyph sideways and slanted by shearing it about the baseline.
        if run.orient != blueice_paint::TextOrient::Horizontal {
            self.draw_vertical_text(&run, ascent, clip, (r, g, b, a));
            return;
        }
        // Arabic and Hebrew are shaped with the font's own tables: joining forms, ligatures,
        // kerning and marks put on their letters. The glyphs come out in screen order.
        if let Some(glyphs) = blueice_font::shape_text(
            run.text,
            run.bold,
            run.italic,
            run.face,
            run.features,
            run.font_size_px as f32,
            run.rtl,
        ) {
            let mut pen_x = run.x;
            for glyph in glyphs {
                let (smear, slant) = run.synthesized_style(glyph.font);
                let (metrics, bitmap) = glyph.font.rasterize_indexed(glyph.id, run.font_size_px as f32);
                let bitmap_top = baseline_y - f64::from(glyph.dy) - (metrics.ymin as f64 + metrics.height as f64);
                let bitmap_left = pen_x + f64::from(glyph.dx) + metrics.xmin as f64;
                for row in 0..metrics.height {
                    for col in 0..metrics.width {
                        let coverage = bitmap[row * metrics.width + col] as u32;
                        if coverage == 0 {
                            continue;
                        }
                        let py = (bitmap_top + row as f64).round() as i64;
                        let shear = (baseline_y - py as f64) * slant;
                        let px = (bitmap_left + col as f64 + shear).round() as i64;
                        for extra in 0..=smear {
                            self.set_pixel_blended(
                                px + extra,
                                py,
                                [r, g, b, ((coverage * a as u32) / 255) as u8],
                                clip,
                            );
                        }
                    }
                }
                pen_x += f64::from(glyph.advance);
            }
            return;
        }
        let mut pen_x = run.x;
        // Characters of other scripts are drawn one at a time; a right-to-left run is drawn
        // backwards, from its last character to its first, brackets mirrored.
        let shaped = blueice_font::shape_in(run.text, run.bold, run.italic, run.face);
        let drawn = if run.rtl {
            std::borrow::Cow::Owned(blueice_font::visual_order(&shaped))
        } else {
            shaped
        };
        for ch in drawn.chars() {
            // Per-character, not once for the whole run: a run can mix
            // scripts (e.g. a CJK sentence with an English word in it),
            // and `blueice-font`'s CJK fallback only kicks in for the
            // specific characters the primary face can't cover.
            let f = font_for_char_in(ch, run.bold, run.italic, run.face);
            let (smear, slant) = run.synthesized_style(f);
            let (metrics, bitmap) = f.rasterize(ch, run.font_size_px as f32);
            let bitmap_top = baseline_y - (metrics.ymin as f64 + metrics.height as f64);
            let bitmap_left = pen_x + metrics.xmin as f64;
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let coverage = bitmap[row * metrics.width + col] as u32;
                    if coverage == 0 {
                        continue;
                    }
                    let py = (bitmap_top + row as f64).round() as i64;
                    let shear = (baseline_y - py as f64) * slant;
                    let px = (bitmap_left + col as f64 + shear).round() as i64;
                    for extra in 0..=smear {
                        self.set_pixel_blended(
                            px + extra,
                            py,
                            [r, g, b, ((coverage * a as u32) / 255) as u8],
                            clip,
                        );
                    }
                }
            }
            pen_x += metrics.advance_width as f64;
        }
    }

    /// A few characters set side by side in the room of one upright character (`text-combine-
    /// upright`): the run is drawn horizontally, squeezed sideways to one em when it is wider
    /// (the glyphs keep their height), and centred in the box across the line and in the em-high
    /// cell at its top.
    fn draw_combined_text(
        &mut self,
        run: &TextRun<'_>,
        clip: PixelClip,
        (r, g, b, a): (u8, u8, u8, u8),
    ) {
        let em = run.font_size_px;
        let shaped = blueice_font::shape_in(run.text, run.bold, run.italic, run.face);
        let natural: f64 = shaped
            .chars()
            .map(|c| {
                font_for_char_in(c, run.bold, run.italic, run.face)
                    .metrics(c, em as f32)
                    .advance_width as f64
            })
            .sum();
        let squeeze = if natural > em { em / natural } else { 1.0 };
        let mut pen_x = run.x + run.breadth / 2.0 - natural * squeeze / 2.0;
        let baseline = run.y + em * UPRIGHT_ASCENT_RATIO;
        for ch in shaped.chars() {
            let f = font_for_char_in(ch, run.bold, run.italic, run.face);
            let (smear, _) = run.synthesized_style(f);
            let (metrics, bitmap) = f.rasterize(ch, em as f32);
            if metrics.width > 0 && metrics.height > 0 {
                // Each destination column is the mean of the source columns it covers.
                let dest_width = ((metrics.width as f64 * squeeze).round() as usize).max(1);
                let left = pen_x + metrics.xmin as f64 * squeeze;
                for row in 0..metrics.height {
                    for dc in 0..dest_width {
                        let from = dc as f64 * metrics.width as f64 / dest_width as f64;
                        let to = (dc + 1) as f64 * metrics.width as f64 / dest_width as f64;
                        let (first, last) = (from.floor() as usize, (to.ceil() as usize).min(metrics.width));
                        let mut sum = 0.0;
                        for col in first..last {
                            let overlap = (to.min((col + 1) as f64) - from.max(col as f64)).max(0.0);
                            sum += overlap * f64::from(bitmap[row * metrics.width + col]);
                        }
                        let coverage = (sum / (to - from)).round() as u32;
                        if coverage == 0 {
                            continue;
                        }
                        let px = (left + dc as f64).round() as i64;
                        let py = (baseline - (metrics.ymin as f64 + metrics.height as f64) + row as f64).round() as i64;
                        for extra in 0..=smear {
                            self.set_pixel_blended(
                                px + extra,
                                py,
                                [r, g, b, ((coverage * a as u32) / 255) as u8],
                                clip,
                            );
                        }
                    }
                }
            }
            pen_x += metrics.advance_width as f64 * squeeze;
        }
    }

    /// Text in a vertical line. Sideways, the run is the horizontal text turned a quarter
    /// clockwise: its baseline is a vertical line `ascent` inside the right edge of the run's
    /// box, the letters' tops face right and the text runs down from the box's top. Upright, each
    /// character is drawn as it is on a horizontal line, one below the other, centred across the
    /// box, in its vertical presentation form where it has one.
    fn draw_vertical_text(
        &mut self,
        run: &TextRun<'_>,
        ascent: f64,
        clip: PixelClip,
        (r, g, b, a): (u8, u8, u8, u8),
    ) {
        if run.orient == blueice_paint::TextOrient::Combined {
            self.draw_combined_text(run, clip, (r, g, b, a));
            return;
        }
        let ccw = run.orient == blueice_paint::TextOrient::SidewaysCcw;
        let sideways = run.orient != blueice_paint::TextOrient::Upright;
        let shaped = blueice_font::shape_in(run.text, run.bold, run.italic, run.face);
        // Turned clockwise the letters' tops face right, so the baseline is `ascent` inside the
        // right edge and the text runs down from the top of the box; turned the other way the
        // tops face left, the baseline is `ascent` inside the left edge and the text climbs from
        // the bottom (`run.y`).
        let baseline_x = if ccw { run.x + ascent } else { run.x + run.breadth - ascent };
        // Upright text is set as a whole from the font's vertical layout: its `vert` alternates,
        // pair kerning (`vkrn`), proportional widths (`vpal`) and per-glyph offsets.
        if !sideways {
            let size = run.font_size_px as f32;
            if let Some(glyphs) = blueice_font::vertical_glyphs(
                run.text,
                run.bold,
                run.italic,
                run.face,
                run.features,
                size,
            ) {
                let mut pen = run.y;
                for glyph in glyphs {
                    let (smear, _) = run.synthesized_style(glyph.font);
                    let (metrics, bitmap) = glyph.font.rasterize_indexed(glyph.id, size);
                    let origin_x = run.x + run.breadth / 2.0 + f64::from(glyph.dx);
                    let origin_y = pen + f64::from(glyph.dy);
                    for row in 0..metrics.height {
                        for col in 0..metrics.width {
                            let coverage = bitmap[row * metrics.width + col] as u32;
                            if coverage == 0 {
                                continue;
                            }
                            let dx = metrics.xmin as f64 + col as f64;
                            let up = (metrics.ymin as f64 + metrics.height as f64) - row as f64;
                            let (px, py) =
                                ((origin_x + dx).round() as i64, (origin_y - up).round() as i64);
                            for extra in 0..=smear {
                                self.set_pixel_blended(
                                    px + extra,
                                    py,
                                    [r, g, b, ((coverage * a as u32) / 255) as u8],
                                    clip,
                                );
                            }
                        }
                    }
                    pen += f64::from(glyph.advance);
                }
                return;
            }
        }
        let mut pen = run.y;
        for original in shaped.chars() {
            let size = run.font_size_px as f32;
            let f = font_for_char_in(original, run.bold, run.italic, run.face);
            let (smear, _) = run.synthesized_style(f);
            // An upright character is set from the font's own vertical layout (`vmtx`, `VORG`,
            // `vert`): the glyph it has for a vertical line, how far the pen moves, and where the
            // glyph hangs from the pen.
            let cell = match f {
                blueice_font::Glyphs::Static(font) if !sideways => {
                    blueice_font::vertical_cell(font, original).map(|cell| (font, cell))
                }
                _ => None,
            };
            // Without an alternate in the font, a presentation form of the character, if some
            // face has it.
            let form = blueice_font::vertical_form(original);
            let use_form = !sideways
                && form != original
                && !cell.is_some_and(|(_, c)| c.alternate)
                && font_for_char_in(form, run.bold, run.italic, run.face).has_glyph(form);
            let (metrics, bitmap, advance, origin_x, origin_y) = match cell {
                Some((font, cell)) if !use_form => {
                    let (metrics, bitmap) = font.rasterize_indexed(cell.id, size);
                    (
                        metrics,
                        bitmap,
                        f64::from(cell.advance * size),
                        run.x + run.breadth / 2.0 + f64::from(cell.dx * size),
                        pen + f64::from(cell.dy * size),
                    )
                }
                _ => {
                    let ch = if use_form { form } else { original };
                    let glyphs = font_for_char_in(ch, run.bold, run.italic, run.face);
                    let (metrics, bitmap) = glyphs.rasterize(ch, size);
                    let advance = metrics.advance_width as f64;
                    // Where this character's origin (on its baseline) is.
                    let (origin_x, origin_y) = if sideways {
                        (baseline_x, pen)
                    } else {
                        (
                            run.x + run.breadth / 2.0 - advance / 2.0,
                            pen + run.font_size_px * UPRIGHT_ASCENT_RATIO,
                        )
                    };
                    (metrics, bitmap, advance, origin_x, origin_y)
                }
            };
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let coverage = bitmap[row * metrics.width + col] as u32;
                    if coverage == 0 {
                        continue;
                    }
                    let dx = metrics.xmin as f64 + col as f64;
                    let up = (metrics.ymin as f64 + metrics.height as f64) - row as f64;
                    let (px, py) = if ccw {
                        ((origin_x - up).round() as i64, (origin_y - dx - 1.0).round() as i64)
                    } else if sideways {
                        ((origin_x + up - 1.0).round() as i64, (origin_y + dx).round() as i64)
                    } else {
                        ((origin_x + dx).round() as i64, (origin_y - up).round() as i64)
                    };
                    for extra in 0..=smear {
                        self.set_pixel_blended(
                            px + extra,
                            py,
                            [r, g, b, ((coverage * a as u32) / 255) as u8],
                            clip,
                        );
                    }
                }
            }
            if ccw {
                pen -= advance;
            } else {
                pen += advance;
            }
        }
    }

    /// Encodes this pixmap as a PNG file -- not needed by the reference
    /// frontend itself (it blits straight from `pixels`), but a
    /// deliberately-kept public capability: it's how this crate's own
    /// correctness can be checked by actually looking at the image
    /// (during development, or for a future automated screenshot
    /// comparison) rather than only by per-pixel assertions.
    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&self.pixels)
            .map_err(std::io::Error::other)
    }
}

/// Rasterizes `frame`'s paint commands, in order, onto a white
/// `frame.width` x `frame.height` canvas.
pub fn rasterize(frame: &Frame) -> Pixmap {
    let mut pixmap = Pixmap::blank(frame.width.ceil() as u32, frame.height.ceil() as u32);
    draw_commands(&mut pixmap, &frame.commands);
    pixmap
}

/// `a` applied after `b`, for `[a, b, c, d, e, f]` affine matrices.
fn multiply(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

/// The index of the `PopBlur` that closes the `PushBlur` at `open`.
fn matching_pop_blur(commands: &[PaintCommand], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, command) in commands.iter().enumerate().skip(open) {
        match command {
            PaintCommand::PushBlur { .. } => depth += 1,
            PaintCommand::PopBlur => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    commands.len()
}

/// The index of the `PopTransform` that closes the `PushTransform` at `open`.
fn matching_pop_transform(commands: &[PaintCommand], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, command) in commands.iter().enumerate().skip(open) {
        match command {
            PaintCommand::PushTransform { .. } => depth += 1,
            PaintCommand::PopTransform => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    commands.len()
}

/// A group drawn on its own: premultiplied colour and coverage per pixel, 0 to 1. The group is
/// drawn twice, over black and over white; what shows through of each recovers both (over
/// black a pixel is colour * alpha, over white it is that plus 1 - alpha).
struct Layer {
    width: usize,
    height: usize,
    /// `r, g, b, a` per pixel, colour premultiplied by `a`.
    data: Vec<f32>,
}

impl Layer {
    fn render(width: u32, height: u32, group: &[PaintCommand]) -> Layer {
        let mut over_black = Pixmap::filled(width, height, [0, 0, 0, 255]);
        let mut over_white = Pixmap::filled(width, height, [255, 255, 255, 255]);
        draw_commands(&mut over_black, group);
        draw_commands(&mut over_white, group);
        let mut data = Vec::with_capacity(width as usize * height as usize * 4);
        for (black, white) in over_black.pixels.chunks_exact(4).zip(over_white.pixels.chunks_exact(4)) {
            let spread: f32 = (0..3).map(|k| f32::from(white[k]) - f32::from(black[k])).sum::<f32>() / (3.0 * 255.0);
            let alpha = (1.0 - spread).clamp(0.0, 1.0);
            for &c in &black[..3] {
                data.push(f32::from(c) / 255.0);
            }
            data.push(alpha);
        }
        Layer { width: width as usize, height: height as usize, data }
    }

    /// A Gaussian blur of standard deviation `sigma` pixels, over all four channels.
    fn blur(&mut self, sigma: f64) {
        let radius = (sigma * 3.0).ceil().max(1.0) as isize;
        let mut kernel: Vec<f32> = (-radius..=radius)
            .map(|i| (-((i * i) as f64) / (2.0 * sigma * sigma)).exp() as f32)
            .collect();
        let total: f32 = kernel.iter().sum();
        kernel.iter_mut().for_each(|k| *k /= total);
        let (w, h) = (self.width as isize, self.height as isize);
        let mut scratch = vec![0.0f32; self.data.len()];
        // Horizontal pass into `scratch`, then vertical back into `data`.
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for (k, weight) in kernel.iter().enumerate() {
                    let sx = x + k as isize - radius;
                    if sx < 0 || sx >= w {
                        continue;
                    }
                    let i = ((y * w + sx) * 4) as usize;
                    for (c, slot) in acc.iter_mut().enumerate() {
                        *slot += self.data[i + c] * weight;
                    }
                }
                let o = ((y * w + x) * 4) as usize;
                scratch[o..o + 4].copy_from_slice(&acc);
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for (k, weight) in kernel.iter().enumerate() {
                    let sy = y + k as isize - radius;
                    if sy < 0 || sy >= h {
                        continue;
                    }
                    let i = ((sy * w + x) * 4) as usize;
                    for (c, slot) in acc.iter_mut().enumerate() {
                        *slot += scratch[i + c] * weight;
                    }
                }
                let o = ((y * w + x) * 4) as usize;
                self.data[o..o + 4].copy_from_slice(&acc);
            }
        }
    }

    /// The layer's pixel at `(x, y)` as `[premultiplied rgb in 0..255, alpha]`.
    fn texel(&self, x: i64, y: i64) -> Option<[f64; 4]> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let i = (y as usize * self.width + x as usize) * 4;
        Some([
            f64::from(self.data[i]) * 255.0,
            f64::from(self.data[i + 1]) * 255.0,
            f64::from(self.data[i + 2]) * 255.0,
            f64::from(self.data[i + 3]),
        ])
    }
}

/// Blends a premultiplied colour of coverage `alpha` over pixel `(x, y)`.
fn blend_premultiplied(pixmap: &mut Pixmap, x: i64, y: i64, color: [f64; 3], alpha: f64) {
    if alpha <= 0.002 {
        return;
    }
    let i = ((y as u32 * pixmap.width + x as u32) * 4) as usize;
    for (k, c) in color.iter().enumerate() {
        let blended = c + f64::from(pixmap.pixels[i + k]) * (1.0 - alpha);
        pixmap.pixels[i + k] = blended.round().clamp(0.0, 255.0) as u8;
    }
}

/// Draws `group` blurred by a Gaussian of standard deviation `sigma`.
fn draw_blurred(pixmap: &mut Pixmap, group: &[PaintCommand], sigma: f64, clip: PixelClip) {
    if group.is_empty() || !(sigma.is_finite() && sigma > 0.0) {
        return draw_commands_clipped(pixmap, group, clip);
    }
    let mut layer = Layer::render(pixmap.width, pixmap.height, group);
    layer.blur(sigma);
    for y in clip.top.max(0)..clip.bottom.min(i64::from(pixmap.height)) {
        for x in clip.left.max(0)..clip.right.min(i64::from(pixmap.width)) {
            if let Some([r, g, b, a]) = layer.texel(x, y) {
                blend_premultiplied(pixmap, x, y, [r, g, b], a);
            }
        }
    }
}

/// An unblurred, untransformed group: just draws it, within `clip`.
fn draw_commands_clipped(pixmap: &mut Pixmap, group: &[PaintCommand], clip: PixelClip) {
    let mut with_clip = vec![PaintCommand::PushClip {
        rect: blueice_paint::Rect {
            x: clip.left as f64,
            y: clip.top as f64,
            width: (clip.right - clip.left) as f64,
            height: (clip.bottom - clip.top) as f64,
        },
    }];
    with_clip.extend(group.iter().cloned());
    with_clip.push(PaintCommand::PopClip);
    draw_commands(pixmap, &with_clip);
}

/// Draws `group` through `matrix` onto `pixmap`, carrying its pixels across with a bilinear
/// sample (see [`Layer`]).
fn draw_transformed(pixmap: &mut Pixmap, group: &[PaintCommand], matrix: [f64; 6], clip: PixelClip) {
    let [a, b, c, d, e, f] = matrix;
    let det = a * d - b * c;
    if group.is_empty() || !det.is_finite() || det.abs() < 1e-9 || clip.right <= clip.left || clip.bottom <= clip.top {
        return;
    }
    let (w, h) = (f64::from(pixmap.width), f64::from(pixmap.height));
    let layer = Layer::render(pixmap.width, pixmap.height, group);
    // The destination pixels the source rectangle can reach.
    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
    let min_x = corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min).floor().max(clip.left as f64) as i64;
    let max_x = corners.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max).ceil().min(clip.right as f64) as i64;
    let min_y = corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min).floor().max(clip.top as f64) as i64;
    let max_y = corners.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max).ceil().min(clip.bottom as f64) as i64;
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    let (ie, i_f) = (-(ia * e + ic * f), -(ib * e + id * f));
    for y in min_y..max_y {
        for x in min_x..max_x {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (u, v) = (ia * px + ic * py + ie - 0.5, ib * px + id * py + i_f - 0.5);
            let (x0, y0) = (u.floor(), v.floor());
            let (fx, fy) = (u - x0, v - y0);
            // Outside the source rectangle counts as transparent, so edges fade.
            let mut sum = [0.0f64; 4];
            for (dx, dy, weight) in [
                (0, 0, (1.0 - fx) * (1.0 - fy)),
                (1, 0, fx * (1.0 - fy)),
                (0, 1, (1.0 - fx) * fy),
                (1, 1, fx * fy),
            ] {
                if let Some(t) = layer.texel(x0 as i64 + dx, y0 as i64 + dy) {
                    for (slot, value) in sum.iter_mut().zip(t) {
                        *slot += value * weight;
                    }
                }
            }
            blend_premultiplied(pixmap, x, y, [sum[0], sum[1], sum[2]], sum[3]);
        }
    }
}

fn draw_commands(pixmap: &mut Pixmap, commands: &[PaintCommand]) {
    let mut clip = PixelClip {
        left: 0,
        top: 0,
        right: i64::from(pixmap.width),
        bottom: i64::from(pixmap.height),
    };
    let mut clips = Vec::new();
    // One entry per open clip: what a rounded clip must restore when it closes.
    let mut rounds: Vec<Option<round_clip::Corners>> = Vec::new();
    let mut index = 0;
    while index < commands.len() {
        let command = &commands[index];
        index += 1;
        match command {
            PaintCommand::PushTransform { matrix } => {
                let end = matching_pop_transform(commands, index - 1);
                draw_transformed(pixmap, &commands[index..end], *matrix, clip);
                index = end + 1;
            }
            PaintCommand::PopTransform | PaintCommand::PopBlur => {}
            PaintCommand::PushBlur { radius } => {
                let end = matching_pop_blur(commands, index - 1);
                draw_blurred(pixmap, &commands[index..end], *radius, clip);
                index = end + 1;
            }
            PaintCommand::PushClip { rect } => {
                clips.push(clip);
                rounds.push(None);
                clip = clip.intersect(*rect);
            }
            PaintCommand::PushRoundClip { rect, radii } => {
                rounds.push(round_clip::Corners::save(pixmap, *rect, radii));
                clips.push(clip);
                clip = clip.intersect(*rect);
            }
            PaintCommand::PopClip => {
                if let Some(previous) = clips.pop() {
                    clip = previous;
                }
                if let Some(Some(corners)) = rounds.pop() {
                    corners.restore_outside_the_arcs(pixmap);
                }
            }
            PaintCommand::Rect { rect, color } => {
                pixmap.fill_rect(rect.x, rect.y, rect.width, rect.height, *color, clip)
            }
            PaintCommand::BorderEdge { rect, color } => {
                pixmap.fill_rect(rect.x, rect.y, rect.width, rect.height, *color, clip)
            }
            PaintCommand::RoundRect { rect, radii, color } => {
                pixmap
                    .canvas()
                    .fill_rrect(*rect, radii, *color, clip.into())
            }
            PaintCommand::RoundBorder {
                rect,
                radii,
                width,
                color,
            } => pixmap
                .canvas()
                .border_ring(*rect, radii, *width, *color, clip.into()),
            PaintCommand::Shadow {
                rect,
                radii,
                dx,
                dy,
                blur,
                spread,
                color,
            } => {
                pixmap
                    .canvas()
                    .shadow(*rect, radii, *dx, *dy, *blur, *spread, *color, clip.into())
            }
            PaintCommand::Gradient {
                rect,
                radii,
                gradient,
            } => pixmap
                .canvas()
                .gradient(*rect, radii, gradient, clip.into()),
            PaintCommand::Image { rect, image, alpha } => {
                pixmap.canvas().image(*rect, image, *alpha, clip.into())
            }
            PaintCommand::Text {
                x,
                y,
                text,
                color,
                font_size_px,
                bold,
                italic,
                face,
                rtl,
                orient,
                breadth,
                features,
            } => pixmap.draw_text(
                TextRun {
                    x: *x,
                    y: *y,
                    text,
                    color: *color,
                    font_size_px: *font_size_px,
                    bold: *bold,
                    italic: *italic,
                    face: face.map(WebFontId::from_u32),
                    rtl: *rtl,
                    orient: *orient,
                    breadth: *breadth,
                    features: blueice_font::FeaturesId::from_u32(*features),
                },
                clip,
            ),
        }
    }
}

fn scale_radii(radii: blueice_paint::Radii, scale: f64) -> blueice_paint::Radii {
    radii.map(|(x, y)| (x * scale, y * scale))
}

/// Rasterize a CSS viewport directly at the requested pixel density. Clipping
/// and glyph rasterization happen after the transform; no full-page bitmap or
/// enlarged low-resolution image is allocated. The caller supplies physical
/// edges separately from the CSS transform, so zoom division/multiplication
/// cannot change the advertised bitmap dimensions. Output is capped at 4096².
pub fn rasterize_viewport(
    frame: &Frame,
    scroll_y: f64,
    pixel_width: u32,
    pixel_height: u32,
    scale: f64,
) -> std::io::Result<Pixmap> {
    rasterize_viewport_culled(frame, scroll_y, pixel_width, pixel_height, scale, true)
}

fn rasterize_viewport_culled(
    frame: &Frame,
    scroll_y: f64,
    pixel_width: u32,
    pixel_height: u32,
    scale: f64,
    cull: bool,
) -> std::io::Result<Pixmap> {
    if ![scroll_y, scale].iter().all(|v| v.is_finite())
        || scroll_y < 0.0
        || scale <= 0.0
        || !(1..=4096).contains(&pixel_width)
        || !(1..=4096).contains(&pixel_height)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Raster viewport exceeds finite pixel limits",
        ));
    }
    let rect = |r: blueice_paint::Rect| blueice_paint::Rect {
        x: r.x * scale,
        y: (r.y - scroll_y) * scale,
        width: r.width * scale,
        height: r.height * scale,
    };
    // Only what can touch the visible band is translated and drawn; a page
    // thousands of pixels tall costs what one screenful does.
    let top = scroll_y;
    let bottom = scroll_y + f64::from(pixel_height) / scale;
    let commands = frame
        .commands
        .iter()
        .filter(|command| !cull || may_reach_band(command, top, bottom))
        .cloned()
        .map(|command| match command {
            PaintCommand::PushClip { rect: r } => PaintCommand::PushClip { rect: rect(r) },
            PaintCommand::PushRoundClip { rect: r, radii } => PaintCommand::PushRoundClip {
                rect: rect(r),
                radii: scale_radii(radii, scale),
            },
            PaintCommand::PopClip => PaintCommand::PopClip,
            PaintCommand::PopTransform => PaintCommand::PopTransform,
            PaintCommand::PopBlur => PaintCommand::PopBlur,
            PaintCommand::PushBlur { radius } => PaintCommand::PushBlur { radius: radius * scale },
            PaintCommand::PushTransform { matrix } => {
                // Page space -> viewport pixels is `p' = scale * (p - (0, scroll_y))`; the
                // matrix must act the same way on the already mapped points.
                let view = [scale, 0.0, 0.0, scale, 0.0, -scroll_y * scale];
                let unview = [1.0 / scale, 0.0, 0.0, 1.0 / scale, 0.0, scroll_y];
                PaintCommand::PushTransform {
                    matrix: multiply(multiply(view, matrix), unview),
                }
            }
            PaintCommand::Rect { rect: r, color } => PaintCommand::Rect {
                rect: rect(r),
                color,
            },
            PaintCommand::BorderEdge { rect: r, color } => PaintCommand::BorderEdge {
                rect: rect(r),
                color,
            },
            PaintCommand::RoundRect {
                rect: r,
                radii,
                color,
            } => PaintCommand::RoundRect {
                rect: rect(r),
                radii: scale_radii(radii, scale),
                color,
            },
            PaintCommand::RoundBorder {
                rect: r,
                radii,
                width,
                color,
            } => PaintCommand::RoundBorder {
                rect: rect(r),
                radii: scale_radii(radii, scale),
                width: width * scale,
                color,
            },
            PaintCommand::Shadow {
                rect: r,
                radii,
                dx,
                dy,
                blur,
                spread,
                color,
            } => PaintCommand::Shadow {
                rect: rect(r),
                radii: scale_radii(radii, scale),
                dx: dx * scale,
                dy: dy * scale,
                blur: blur * scale,
                spread: spread * scale,
                color,
            },
            PaintCommand::Image {
                rect: r,
                image,
                alpha,
            } => PaintCommand::Image {
                rect: rect(r),
                image,
                alpha,
            },
            PaintCommand::Gradient {
                rect: r,
                radii,
                gradient,
            } => PaintCommand::Gradient {
                rect: rect(r),
                radii: scale_radii(radii, scale),
                gradient,
            },
            PaintCommand::Text {
                x,
                y,
                text,
                color,
                font_size_px,
                bold,
                italic,
                face,
                rtl,
                orient,
                breadth,
                features,
            } => PaintCommand::Text {
                x: x * scale,
                y: (y - scroll_y) * scale,
                text,
                color,
                font_size_px: font_size_px * scale,
                bold,
                italic,
                face,
                rtl,
                orient,
                breadth: breadth * scale,
                features,
            },
        })
        .collect();
    Ok(rasterize(&Frame {
        width: f64::from(pixel_width),
        height: f64::from(pixel_height),
        commands,
    }))
}

/// Whether `command` can change a pixel of the band `top..bottom` (in page
/// coordinates). Clip pushes and pops are always kept so they stay balanced;
/// everything else is judged by a deliberately generous vertical extent.
fn may_reach_band(command: &PaintCommand, top: f64, bottom: f64) -> bool {
    let overlaps =
        |y: f64, height: f64, margin: f64| y - margin <= bottom && y + height + margin >= top;
    match command {
        PaintCommand::PushClip { .. }
        | PaintCommand::PushRoundClip { .. }
        | PaintCommand::PopClip
        | PaintCommand::PushTransform { .. }
        | PaintCommand::PopTransform
        | PaintCommand::PushBlur { .. }
        | PaintCommand::PopBlur => true,
        PaintCommand::Rect { rect, .. }
        | PaintCommand::BorderEdge { rect, .. }
        | PaintCommand::RoundRect { rect, .. }
        | PaintCommand::RoundBorder { rect, .. }
        | PaintCommand::Image { rect, .. }
        | PaintCommand::Gradient { rect, .. } => overlaps(rect.y, rect.height, 1.0),
        PaintCommand::Shadow {
            rect,
            dy,
            blur,
            spread,
            ..
        } => overlaps(
            rect.y,
            rect.height,
            dy.abs() + blur.abs() * 2.0 + spread.abs() + 1.0,
        ),
        // The text origin may be a top or a baseline and glyphs can overhang
        // it: allow a few font heights either side.
        PaintCommand::Text {
            y, font_size_px, ..
        } => overlaps(*y - font_size_px * 2.0, font_size_px * 4.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_paint::Rect;

    #[test]
    fn a_viewport_matches_the_same_rows_of_a_full_page_raster() {
        let rect = |x: f64, y: f64, width: f64, height: f64| Rect {
            x,
            y,
            width,
            height,
        };
        let color = Color::Rgba(30, 90, 200, 255);
        let mut commands = Vec::new();
        for i in 0..40 {
            let y = f64::from(i) * 25.0;
            commands.push(PaintCommand::Rect {
                rect: rect(10.0, y, 80.0, 12.0),
                color,
            });
            commands.push(PaintCommand::Text {
                x: 12.0,
                y: y + 2.0,
                text: format!("row {i}"),
                color: Color::Rgba(0, 0, 0, 255),
                font_size_px: 14.0,
                bold: false,
                italic: false,
                face: None,
                rtl: false,
                orient: blueice_paint::TextOrient::Horizontal,
                breadth: 0.0,
                features: 0,
            });
        }
        let page = frame(100.0, 1000.0, commands.clone());
        let full = rasterize(&page);
        for scroll in [0u32, 130, 333, 360, 700] {
            let view = rasterize_viewport(&page, f64::from(scroll), 100, 200, 1.0).unwrap();
            let start = (scroll * 100 * 4) as usize;
            let end = start + 200 * 100 * 4;
            assert_eq!(view.pixels, full.pixels[start..end], "scroll {scroll}");
        }
    }

    #[test]
    fn skipping_commands_outside_the_band_changes_no_pixel() {
        let mut commands = Vec::new();
        for i in 0..40 {
            let y = f64::from(i) * 25.0;
            commands.push(PaintCommand::Text {
                x: 12.0,
                y,
                text: format!("row {i}"),
                color: Color::Rgba(0, 0, 0, 255),
                font_size_px: 14.0,
                bold: false,
                italic: false,
                face: None,
                rtl: false,
                orient: blueice_paint::TextOrient::Horizontal,
                breadth: 0.0,
                features: 0,
            });
        }
        commands.push(PaintCommand::Shadow {
            rect: Rect {
                x: 20.0,
                y: 300.0,
                width: 60.0,
                height: 30.0,
            },
            radii: [(0.0, 0.0); 4],
            dx: 0.0,
            dy: 40.0,
            blur: 12.0,
            spread: 4.0,
            color: Color::Rgba(0, 0, 0, 200),
        });
        let page = frame(100.0, 1000.0, commands);
        for scroll in [0.0, 130.0, 333.5, 360.0, 700.0] {
            let culled = rasterize_viewport_culled(&page, scroll, 100, 200, 1.0, true).unwrap();
            let all = rasterize_viewport_culled(&page, scroll, 100, 200, 1.0, false).unwrap();
            assert_eq!(culled.pixels, all.pixels, "scroll {scroll}");
        }
    }

    fn frame(width: f64, height: f64, commands: Vec<PaintCommand>) -> Frame {
        Frame {
            width,
            height,
            commands,
        }
    }

    fn rect_cmd(x: f64, y: f64, w: f64, h: f64, color: Color) -> PaintCommand {
        PaintCommand::Rect { rect: Rect { x, y, width: w, height: h }, color }
    }

    #[test]
    fn a_rotated_group_is_drawn_at_its_turned_position() {
        // A 40x10 red bar at (10, 20), turned 90 degrees about its own centre (30, 25), is
        // a 10x40 bar from (25, 5) to (35, 45).
        let (cx, cy) = (30.0, 25.0);
        let m = [0.0, 1.0, -1.0, 0.0, cx + cy, cy - cx];
        let f = frame(
            60.0,
            60.0,
            vec![
                PaintCommand::PushTransform { matrix: m },
                rect_cmd(10.0, 20.0, 40.0, 10.0, Color::Rgba(255, 0, 0, 255)),
                PaintCommand::PopTransform,
            ],
        );
        let p = rasterize(&f);
        assert_eq!(p.get_pixel(30, 10), [255, 0, 0, 255], "inside the turned bar");
        assert_eq!(p.get_pixel(30, 40), [255, 0, 0, 255], "inside the turned bar");
        assert_eq!(p.get_pixel(15, 25), BACKGROUND, "where the unturned bar was");
        assert_eq!(p.get_pixel(45, 25), BACKGROUND, "where the unturned bar was");
    }

    #[test]
    fn a_scaled_group_grows_and_keeps_its_colour() {
        let f = frame(
            40.0,
            40.0,
            vec![
                PaintCommand::PushTransform { matrix: [2.0, 0.0, 0.0, 2.0, 0.0, 0.0] },
                rect_cmd(5.0, 5.0, 5.0, 5.0, Color::Rgba(0, 0, 255, 255)),
                PaintCommand::PopTransform,
            ],
        );
        let p = rasterize(&f);
        assert_eq!(p.get_pixel(15, 15), [0, 0, 255, 255]);
        assert_eq!(p.get_pixel(5, 5), BACKGROUND);
        assert_eq!(p.get_pixel(25, 25), BACKGROUND);
    }

    #[test]
    fn a_translucent_group_blends_with_what_is_behind_it() {
        let f = frame(
            20.0,
            20.0,
            vec![
                rect_cmd(0.0, 0.0, 20.0, 20.0, Color::Rgba(0, 0, 0, 255)),
                PaintCommand::PushTransform { matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] },
                rect_cmd(5.0, 5.0, 10.0, 10.0, Color::Rgba(255, 255, 255, 128)),
                PaintCommand::PopTransform,
            ],
        );
        let p = rasterize(&f);
        let [r, ..] = p.get_pixel(10, 10);
        assert!((120..=136).contains(&r), "half white over black is mid grey, got {r}");
        assert_eq!(p.get_pixel(2, 2), [0, 0, 0, 255]);
    }

    #[test]
    fn a_viewport_raster_applies_a_transform_in_scrolled_scaled_space() {
        let m = [1.0, 0.0, 0.0, 1.0, 10.0, 0.0];
        let f = frame(
            100.0,
            200.0,
            vec![
                PaintCommand::PushTransform { matrix: m },
                rect_cmd(0.0, 100.0, 10.0, 10.0, Color::Rgba(0, 128, 0, 255)),
                PaintCommand::PopTransform,
            ],
        );
        let p = rasterize_viewport(&f, 100.0, 40, 20, 2.0).expect("a raster");
        // Page (10..20, 100..110) is viewport pixels (20..40, 0..20) at 2x.
        assert_eq!(p.get_pixel(30, 10), [0, 128, 0, 255]);
        assert_eq!(p.get_pixel(10, 10), BACKGROUND);
    }

    #[test]
    fn a_blurred_group_spreads_its_colour_beyond_its_edge() {
        let f = frame(
            60.0,
            60.0,
            vec![
                PaintCommand::PushBlur { radius: 3.0 },
                rect_cmd(20.0, 20.0, 20.0, 20.0, Color::Rgba(0, 0, 0, 255)),
                PaintCommand::PopBlur,
            ],
        );
        let p = rasterize(&f);
        let [centre, ..] = p.get_pixel(30, 30);
        let [outside, ..] = p.get_pixel(18, 30);
        let [far, ..] = p.get_pixel(5, 30);
        assert!(centre < 10, "the middle stays dark, got {centre}");
        assert!(outside < 250 && outside > 5, "the edge has bled outwards, got {outside}");
        assert_eq!(far, 255, "far away is untouched");
        let [on_edge, ..] = p.get_pixel(20, 30);
        assert!((100..=155).contains(&on_edge), "a blurred edge is half way, got {on_edge}");
    }

    #[test]
    fn a_zero_radius_blur_draws_the_group_unchanged() {
        let f = frame(
            20.0,
            20.0,
            vec![
                PaintCommand::PushBlur { radius: 0.0 },
                rect_cmd(5.0, 5.0, 10.0, 10.0, Color::Rgba(0, 0, 0, 255)),
                PaintCommand::PopBlur,
            ],
        );
        let p = rasterize(&f);
        assert_eq!(p.get_pixel(10, 10), [0, 0, 0, 255]);
        assert_eq!(p.get_pixel(4, 10), BACKGROUND);
    }

    #[test]
    fn blank_frame_is_all_white() {
        let pixmap = rasterize(&frame(4.0, 4.0, vec![]));
        assert_eq!(pixmap.width, 4);
        assert_eq!(pixmap.height, 4);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(pixmap.get_pixel(x, y), [255, 255, 255, 255]);
            }
        }
    }

    #[test]
    fn opaque_rect_fills_exactly_its_bounds() {
        let pixmap = rasterize(&frame(
            10.0,
            10.0,
            vec![PaintCommand::Rect {
                rect: Rect {
                    x: 2.0,
                    y: 3.0,
                    width: 4.0,
                    height: 2.0,
                },
                color: Color::Rgba(255, 0, 0, 255),
            }],
        ));
        assert_eq!(pixmap.get_pixel(2, 3), [255, 0, 0, 255]);
        assert_eq!(pixmap.get_pixel(5, 4), [255, 0, 0, 255]);
        assert_eq!(
            pixmap.get_pixel(6, 3),
            [255, 255, 255, 255],
            "one pixel past the right edge stays background"
        );
        assert_eq!(
            pixmap.get_pixel(2, 5),
            [255, 255, 255, 255],
            "one pixel past the bottom edge stays background"
        );
        assert_eq!(
            pixmap.get_pixel(1, 3),
            [255, 255, 255, 255],
            "one pixel before the left edge stays background"
        );
    }

    #[test]
    fn later_commands_paint_over_earlier_ones() {
        let pixmap = rasterize(&frame(
            10.0,
            10.0,
            vec![
                PaintCommand::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    color: Color::Rgba(255, 0, 0, 255),
                },
                PaintCommand::Rect {
                    rect: Rect {
                        x: 2.0,
                        y: 2.0,
                        width: 2.0,
                        height: 2.0,
                    },
                    color: Color::Rgba(0, 0, 255, 255),
                },
            ],
        ));
        assert_eq!(pixmap.get_pixel(2, 2), [0, 0, 255, 255]);
        assert_eq!(pixmap.get_pixel(0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn semi_transparent_rect_blends_with_the_background() {
        let pixmap = rasterize(&frame(
            4.0,
            4.0,
            vec![PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 4.0,
                    height: 4.0,
                },
                color: Color::Rgba(0, 0, 0, 128),
            }],
        ));
        let [r, g, b, a] = pixmap.get_pixel(0, 0);
        assert!(
            r < 255 && r > 0,
            "blended halfway between black and white, not fully either"
        );
        assert_eq!(
            (r, g, b, a),
            (r, r, r, 255),
            "gray, and still fully opaque as far as the canvas is concerned"
        );
    }

    #[test]
    fn text_command_paints_some_non_background_pixels_in_its_line() {
        let pixmap = rasterize(&frame(
            200.0,
            30.0,
            vec![PaintCommand::Text {
                x: 0.0,
                y: 0.0,
                text: "Hi".to_string(),
                color: Color::Rgba(0, 0, 0, 255),
                font_size_px: 16.0,
                bold: false,
                italic: false,
                face: None,
                rtl: false,
                orient: blueice_paint::TextOrient::Horizontal,
                breadth: 0.0,
                features: 0,
            }],
        ));
        let has_ink = (0..pixmap.height)
            .any(|y| (0..pixmap.width).any(|x| pixmap.get_pixel(x, y) != [255, 255, 255, 255]));
        assert!(has_ink, "rendering \"Hi\" must actually darken some pixels");
    }

    #[test]
    fn empty_text_paints_nothing() {
        let pixmap = rasterize(&frame(
            20.0,
            20.0,
            vec![PaintCommand::Text {
                x: 0.0,
                y: 0.0,
                text: String::new(),
                color: Color::Rgba(0, 0, 0, 255),
                font_size_px: 16.0,
                bold: false,
                italic: false,
                face: None,
                rtl: false,
                orient: blueice_paint::TextOrient::Horizontal,
                breadth: 0.0,
                features: 0,
            }],
        ));
        for y in 0..pixmap.height {
            for x in 0..pixmap.width {
                assert_eq!(pixmap.get_pixel(x, y), [255, 255, 255, 255]);
            }
        }
    }

    #[test]
    fn different_characters_advance_the_pen_so_they_dont_overlap() {
        let one_char = rasterize(&frame(
            200.0,
            30.0,
            vec![PaintCommand::Text {
                x: 0.0,
                y: 0.0,
                text: "M".to_string(),
                color: Color::Rgba(0, 0, 0, 255),
                font_size_px: 20.0,
                bold: false,
                italic: false,
                face: None,
                rtl: false,
                orient: blueice_paint::TextOrient::Horizontal,
                breadth: 0.0,
                features: 0,
            }],
        ));
        let two_char = rasterize(&frame(
            200.0,
            30.0,
            vec![PaintCommand::Text {
                x: 0.0,
                y: 0.0,
                text: "MM".to_string(),
                color: Color::Rgba(0, 0, 0, 255),
                font_size_px: 20.0,
                bold: false,
                italic: false,
                face: None,
                rtl: false,
                orient: blueice_paint::TextOrient::Horizontal,
                breadth: 0.0,
                features: 0,
            }],
        ));
        let ink_extent = |p: &Pixmap| -> u32 {
            (0..p.width)
                .rev()
                .find(|&x| (0..p.height).any(|y| p.get_pixel(x, y) != [255, 255, 255, 255]))
                .unwrap_or(0)
        };
        assert!(
            ink_extent(&two_char) > ink_extent(&one_char),
            "two characters must occupy more horizontal space than one"
        );
    }

    #[test]
    fn zero_alpha_color_paints_nothing() {
        let pixmap = rasterize(&frame(
            4.0,
            4.0,
            vec![PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 4.0,
                    height: 4.0,
                },
                color: Color::Rgba(0, 0, 0, 0),
            }],
        ));
        assert_eq!(pixmap.get_pixel(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn save_png_round_trips_through_a_real_decoder() {
        let pixmap = rasterize(&frame(
            3.0,
            3.0,
            vec![PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                color: Color::Rgba(10, 20, 30, 255),
            }],
        ));
        let path =
            std::env::temp_dir().join(format!("blueice-raster-test-{}.png", std::process::id()));
        pixmap.save_png(&path).unwrap();

        let file = std::fs::File::open(&path).unwrap();
        let decoder = png::Decoder::new(file);
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(info.width, 3);
        assert_eq!(info.height, 3);
        assert_eq!(&buf[0..4], &[10, 20, 30, 255]);
    }
}
