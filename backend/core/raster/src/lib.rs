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

use blueice_font::{ascent_descent_in, font_for_char_in, web::WebFontId};
use blueice_paint::{Color, Frame, PaintCommand};

const ASSUMED_ASCENT_RATIO: f64 = 0.8;

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
        let ascent = match run.face {
            Some(_) => ascent_descent_in(run.font_size_px, run.bold, run.italic, run.face).0,
            None => run.font_size_px * ASSUMED_ASCENT_RATIO,
        };
        let baseline_y = run.y + ascent;
        // A page face that lacks the weight or slant asked for is made bold by smearing each
        // glyph sideways and slanted by shearing it about the baseline.
        let synthesized = run.face.is_some();
        let smear = if synthesized && run.bold { (run.font_size_px / 20.0).max(1.0).round() as i64 } else { 0 };
        let slant = if synthesized && run.italic { 0.2 } else { 0.0 };
        if run.orient != blueice_paint::TextOrient::Horizontal {
            self.draw_vertical_text(&run, ascent, smear, clip, (r, g, b, a));
            return;
        }
        let mut pen_x = run.x;
        // Arabic letters take the form their neighbours call for; a right-to-left run is then
        // drawn backwards, from its last character to its first, brackets mirrored.
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
        smear: i64,
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
        smear: i64,
        clip: PixelClip,
        (r, g, b, a): (u8, u8, u8, u8),
    ) {
        if run.orient == blueice_paint::TextOrient::Combined {
            self.draw_combined_text(run, smear, clip, (r, g, b, a));
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
        let mut pen = run.y;
        for original in shaped.chars() {
            let ch = if sideways {
                original
            } else {
                // The vertical form, where some face can draw it.
                let form = blueice_font::vertical_form(original);
                if form != original && font_for_char_in(form, run.bold, run.italic, run.face).has_glyph(form) {
                    form
                } else {
                    original
                }
            };
            let f = font_for_char_in(ch, run.bold, run.italic, run.face);
            let (metrics, bitmap) = f.rasterize(ch, run.font_size_px as f32);
            let advance = metrics.advance_width as f64;
            // Where this character's origin (on its baseline) is, and how a bitmap pixel maps
            // from its offset (right of the origin, up from the baseline) to the page.
            let (origin_x, origin_y) = if sideways {
                (baseline_x, pen)
            } else {
                (
                    run.x + run.breadth / 2.0 - advance / 2.0,
                    pen + run.font_size_px * UPRIGHT_ASCENT_RATIO,
                )
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
    let mut clip = PixelClip {
        left: 0,
        top: 0,
        right: i64::from(pixmap.width),
        bottom: i64::from(pixmap.height),
    };
    let mut clips = Vec::new();
    // One entry per open clip: what a rounded clip must restore when it closes.
    let mut rounds: Vec<Option<round_clip::Corners>> = Vec::new();
    for command in &frame.commands {
        match command {
            PaintCommand::PushClip { rect } => {
                clips.push(clip);
                rounds.push(None);
                clip = clip.intersect(*rect);
            }
            PaintCommand::PushRoundClip { rect, radii } => {
                rounds.push(round_clip::Corners::save(&pixmap, *rect, radii));
                clips.push(clip);
                clip = clip.intersect(*rect);
            }
            PaintCommand::PopClip => {
                if let Some(previous) = clips.pop() {
                    clip = previous;
                }
                if let Some(Some(corners)) = rounds.pop() {
                    corners.restore_outside_the_arcs(&mut pixmap);
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
                },
                clip,
            ),
        }
    }
    pixmap
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
        | PaintCommand::PopClip => true,
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
