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

use blueice_font::font_for;
use blueice_paint::{Color, Frame, PaintCommand};

const ASSUMED_ASCENT_RATIO: f64 = 0.8;
const BACKGROUND: [u8; 4] = [255, 255, 255, 255];

struct TextRun<'a> {
    x: f64,
    y: f64,
    text: &'a str,
    color: Color,
    font_size_px: f64,
    bold: bool,
    italic: bool,
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

impl Pixmap {
    fn blank(width: u32, height: u32) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            pixels.extend_from_slice(&BACKGROUND);
        }
        Pixmap { width, height, pixels }
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * self.width + x) * 4) as usize;
        [self.pixels[idx], self.pixels[idx + 1], self.pixels[idx + 2], self.pixels[idx + 3]]
    }

    fn set_pixel_blended(&mut self, x: i64, y: i64, r: u8, g: u8, b: u8, a: u8) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 || a == 0 {
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

    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Color) {
        let Color::Rgba(r, g, b, a) = color else { return };
        let (x0, y0) = (x.round() as i64, y.round() as i64);
        let (x1, y1) = ((x + w).round() as i64, (y + h).round() as i64);
        for py in y0..y1 {
            for px in x0..x1 {
                self.set_pixel_blended(px, py, r, g, b, a);
            }
        }
    }

    fn draw_text(&mut self, run: TextRun<'_>) {
        let Color::Rgba(r, g, b, a) = run.color else { return };
        let f = font_for(run.bold, run.italic);
        let baseline_y = run.y + run.font_size_px * ASSUMED_ASCENT_RATIO;
        let mut pen_x = run.x;
        for ch in run.text.chars() {
            let (metrics, bitmap) = f.rasterize(ch, run.font_size_px as f32);
            let bitmap_top = baseline_y - (metrics.ymin as f64 + metrics.height as f64);
            let bitmap_left = pen_x + metrics.xmin as f64;
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let coverage = bitmap[row * metrics.width + col] as u32;
                    if coverage == 0 {
                        continue;
                    }
                    let px = (bitmap_left + col as f64).round() as i64;
                    let py = (bitmap_top + row as f64).round() as i64;
                    self.set_pixel_blended(px, py, r, g, b, ((coverage * a as u32) / 255) as u8);
                }
            }
            pen_x += metrics.advance_width as f64;
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
        writer.write_image_data(&self.pixels).map_err(std::io::Error::other)
    }
}

/// Rasterizes `frame`'s paint commands, in order, onto a white
/// `frame.width` x `frame.height` canvas.
pub fn rasterize(frame: &Frame) -> Pixmap {
    let mut pixmap = Pixmap::blank(frame.width.ceil() as u32, frame.height.ceil() as u32);
    for command in &frame.commands {
        match command {
            PaintCommand::Rect { rect, color } => pixmap.fill_rect(rect.x, rect.y, rect.width, rect.height, *color),
            PaintCommand::BorderEdge { rect, color } => pixmap.fill_rect(rect.x, rect.y, rect.width, rect.height, *color),
            PaintCommand::Text { x, y, text, color, font_size_px, bold, italic } => {
                pixmap.draw_text(TextRun { x: *x, y: *y, text, color: *color, font_size_px: *font_size_px, bold: *bold, italic: *italic })
            }
        }
    }
    pixmap
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_paint::Rect;

    fn frame(width: f64, height: f64, commands: Vec<PaintCommand>) -> Frame {
        Frame { width, height, commands }
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
            vec![PaintCommand::Rect { rect: Rect { x: 2.0, y: 3.0, width: 4.0, height: 2.0 }, color: Color::Rgba(255, 0, 0, 255) }],
        ));
        assert_eq!(pixmap.get_pixel(2, 3), [255, 0, 0, 255]);
        assert_eq!(pixmap.get_pixel(5, 4), [255, 0, 0, 255]);
        assert_eq!(pixmap.get_pixel(6, 3), [255, 255, 255, 255], "one pixel past the right edge stays background");
        assert_eq!(pixmap.get_pixel(2, 5), [255, 255, 255, 255], "one pixel past the bottom edge stays background");
        assert_eq!(pixmap.get_pixel(1, 3), [255, 255, 255, 255], "one pixel before the left edge stays background");
    }

    #[test]
    fn later_commands_paint_over_earlier_ones() {
        let pixmap = rasterize(&frame(
            10.0,
            10.0,
            vec![
                PaintCommand::Rect { rect: Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }, color: Color::Rgba(255, 0, 0, 255) },
                PaintCommand::Rect { rect: Rect { x: 2.0, y: 2.0, width: 2.0, height: 2.0 }, color: Color::Rgba(0, 0, 255, 255) },
            ],
        ));
        assert_eq!(pixmap.get_pixel(2, 2), [0, 0, 255, 255]);
        assert_eq!(pixmap.get_pixel(0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn semi_transparent_rect_blends_with_the_background() {
        let pixmap = rasterize(&frame(4.0, 4.0, vec![PaintCommand::Rect { rect: Rect { x: 0.0, y: 0.0, width: 4.0, height: 4.0 }, color: Color::Rgba(0, 0, 0, 128) }]));
        let [r, g, b, a] = pixmap.get_pixel(0, 0);
        assert!(r < 255 && r > 0, "blended halfway between black and white, not fully either");
        assert_eq!((r, g, b, a), (r, r, r, 255), "gray, and still fully opaque as far as the canvas is concerned");
    }

    #[test]
    fn text_command_paints_some_non_background_pixels_in_its_line() {
        let pixmap = rasterize(&frame(
            200.0,
            30.0,
            vec![PaintCommand::Text { x: 0.0, y: 0.0, text: "Hi".to_string(), color: Color::Rgba(0, 0, 0, 255), font_size_px: 16.0, bold: false, italic: false }],
        ));
        let has_ink = (0..pixmap.height).any(|y| (0..pixmap.width).any(|x| pixmap.get_pixel(x, y) != [255, 255, 255, 255]));
        assert!(has_ink, "rendering \"Hi\" must actually darken some pixels");
    }

    #[test]
    fn empty_text_paints_nothing() {
        let pixmap = rasterize(&frame(20.0, 20.0, vec![PaintCommand::Text { x: 0.0, y: 0.0, text: String::new(), color: Color::Rgba(0, 0, 0, 255), font_size_px: 16.0, bold: false, italic: false }]));
        for y in 0..pixmap.height {
            for x in 0..pixmap.width {
                assert_eq!(pixmap.get_pixel(x, y), [255, 255, 255, 255]);
            }
        }
    }

    #[test]
    fn different_characters_advance_the_pen_so_they_dont_overlap() {
        let one_char = rasterize(&frame(200.0, 30.0, vec![PaintCommand::Text { x: 0.0, y: 0.0, text: "M".to_string(), color: Color::Rgba(0, 0, 0, 255), font_size_px: 20.0, bold: false, italic: false }]));
        let two_char = rasterize(&frame(200.0, 30.0, vec![PaintCommand::Text { x: 0.0, y: 0.0, text: "MM".to_string(), color: Color::Rgba(0, 0, 0, 255), font_size_px: 20.0, bold: false, italic: false }]));
        let ink_extent = |p: &Pixmap| -> u32 { (0..p.width).rev().find(|&x| (0..p.height).any(|y| p.get_pixel(x, y) != [255, 255, 255, 255])).unwrap_or(0) };
        assert!(ink_extent(&two_char) > ink_extent(&one_char), "two characters must occupy more horizontal space than one");
    }

    #[test]
    fn zero_alpha_color_paints_nothing() {
        let pixmap = rasterize(&frame(4.0, 4.0, vec![PaintCommand::Rect { rect: Rect { x: 0.0, y: 0.0, width: 4.0, height: 4.0 }, color: Color::Rgba(0, 0, 0, 0) }]));
        assert_eq!(pixmap.get_pixel(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn save_png_round_trips_through_a_real_decoder() {
        let pixmap = rasterize(&frame(3.0, 3.0, vec![PaintCommand::Rect { rect: Rect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }, color: Color::Rgba(10, 20, 30, 255) }]));
        let path = std::env::temp_dir().join(format!("blueice-raster-test-{}.png", std::process::id()));
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
