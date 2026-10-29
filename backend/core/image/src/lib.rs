// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decoding of the image formats pages use -- PNG, JPEG, GIF (first frame),
//! WebP and SVG -- into plain RGBA pixels. Pure computation on bytes already
//! fetched and reviewed elsewhere: no I/O, no network, and bounded output
//! (see [`MAX_SIDE`]/[`MAX_PIXELS`]) so a hostile image cannot claim memory.

use std::io::Cursor;

/// Longest side, in pixels, of any image this crate will produce.
pub const MAX_SIDE: u32 = 8192;
/// Most pixels in any image this crate will produce.
pub const MAX_PIXELS: u64 = 32 * 1024 * 1024;

/// A decoded image: `rgba` is straight (non-premultiplied) 8-bit RGBA,
/// `width * height * 4` bytes.
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// The size the picture has in layout: its pixel size, except an SVG
    /// that was drawn at a multiple of its own size for sharpness.
    natural: (f64, f64),
    premultiplied: std::sync::OnceLock<Vec<u8>>,
}

impl RasterImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        RasterImage {
            width,
            height,
            rgba,
            natural: (f64::from(width), f64::from(height)),
            premultiplied: std::sync::OnceLock::new(),
        }
    }

    /// The same pixels, laid out as `natural` CSS pixels.
    pub fn with_natural(mut self, natural: (f64, f64)) -> Self {
        self.natural = natural;
        self
    }

    /// The size layout gives the picture when nothing else says.
    pub fn natural(&self) -> (f64, f64) {
        self.natural
    }

    /// The pixels with colour multiplied by alpha, computed once: what
    /// compositing wants.
    pub fn premultiplied(&self) -> &[u8] {
        self.premultiplied.get_or_init(|| {
            let mut out = self.rgba.clone();
            for p in out.chunks_exact_mut(4) {
                let a = u32::from(p[3]);
                if a != 255 {
                    for c in &mut p[..3] {
                        *c = ((u32::from(*c) * a + 127) / 255) as u8;
                    }
                }
            }
            out
        })
    }
}

impl Clone for RasterImage {
    fn clone(&self) -> Self {
        RasterImage::new(self.width, self.height, self.rgba.clone()).with_natural(self.natural)
    }
}

impl PartialEq for RasterImage {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.rgba == other.rgba
    }
}

impl std::fmt::Debug for RasterImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RasterImage({}x{})", self.width, self.height)
    }
}

fn within_limits(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_SIDE
        && height <= MAX_SIDE
        && u64::from(width) * u64::from(height) <= MAX_PIXELS
}

fn finish(width: u32, height: u32, rgba: Vec<u8>) -> Option<RasterImage> {
    (within_limits(width, height) && rgba.len() == width as usize * height as usize * 4)
        .then(|| RasterImage::new(width, height, rgba))
}

/// The picture as a PNG file.
pub fn encode_png(image: &RasterImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&image.rgba).ok()?;
    }
    Some(out)
}

/// Which format `bytes` are, by their leading bytes (never by file name or a
/// server's say-so alone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    WebP,
    Svg,
}

pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::WebP)
    } else {
        let head = &bytes[..bytes.len().min(2048)];
        let text = String::from_utf8_lossy(head);
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        (trimmed.starts_with("<svg")
            || (trimmed.starts_with("<?xml") && text.contains("<svg"))
            || (trimmed.starts_with("<!") && text.contains("<svg")))
        .then_some(Format::Svg)
    }
}

/// Decodes `bytes` in whatever format they turn out to be. An SVG is
/// rendered at its own size.
pub fn decode(bytes: &[u8]) -> Option<RasterImage> {
    match sniff(bytes)? {
        Format::Png => decode_png(bytes),
        Format::Jpeg => decode_jpeg(bytes),
        Format::Gif => decode_gif(bytes),
        Format::WebP => decode_webp(bytes),
        Format::Svg => svg::render_svg(std::str::from_utf8(bytes).ok()?, None),
    }
}

/// The size a decoded or rendered image would have, without allocating
/// pixels where the format allows it.
pub fn intrinsic_size(bytes: &[u8]) -> Option<(u32, u32)> {
    match sniff(bytes)? {
        Format::Svg => {
            let (w, h) = svg::svg_size(std::str::from_utf8(bytes).ok()?)?;
            Some((w.ceil() as u32, h.ceil() as u32))
        }
        _ => decode(bytes).map(|i| (i.width, i.height)),
    }
}

fn decode_png(bytes: &[u8]) -> Option<RasterImage> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if !within_limits(info.width, info.height) {
        return None;
    }
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buffer).ok()?;
    let data = &buffer[..frame.buffer_size()];
    let rgba = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    finish(frame.width, frame.height, rgba)
}

fn decode_jpeg(bytes: &[u8]) -> Option<RasterImage> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    let (w, h) = (u32::from(info.width), u32::from(info.height));
    if !within_limits(w, h) {
        return None;
    }
    let pixels = decoder.decode().ok()?;
    let rgba = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        jpeg_decoder::PixelFormat::L16 => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], 255])
            .collect(),
        // Adobe CMYK is stored inverted.
        jpeg_decoder::PixelFormat::CMYK32 => pixels
            .chunks_exact(4)
            .flat_map(|p| {
                let k = u32::from(p[3]);
                let c = |v: u8| (u32::from(v) * k / 255) as u8;
                [c(p[0]), c(p[1]), c(p[2]), 255]
            })
            .collect(),
    };
    finish(w, h, rgba)
}

fn decode_gif(bytes: &[u8]) -> Option<RasterImage> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(Cursor::new(bytes)).ok()?;
    let (sw, sh) = (u32::from(decoder.width()), u32::from(decoder.height()));
    if !within_limits(sw, sh) {
        return None;
    }
    let frame = decoder.read_next_frame().ok()??;
    let mut canvas = vec![0u8; sw as usize * sh as usize * 4];
    let (fx, fy) = (u32::from(frame.left), u32::from(frame.top));
    let (fw, fh) = (u32::from(frame.width), u32::from(frame.height));
    for row in 0..fh.min(sh.saturating_sub(fy)) {
        let src = (row * fw) as usize * 4;
        let dst = ((fy + row) * sw + fx) as usize * 4;
        let len = (fw.min(sw.saturating_sub(fx)) as usize) * 4;
        canvas[dst..dst + len].copy_from_slice(&frame.buffer[src..src + len]);
    }
    finish(sw, sh, canvas)
}

fn decode_webp(bytes: &[u8]) -> Option<RasterImage> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
    let (w, h) = decoder.dimensions();
    if !within_limits(w, h) {
        return None;
    }
    let mut buffer = vec![0u8; decoder.output_buffer_size()?];
    decoder.read_image(&mut buffer).ok()?;
    let rgba = if decoder.has_alpha() {
        buffer
    } else {
        buffer
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    };
    finish(w, h, rgba)
}

#[cfg(feature = "svg")]
pub use svg::{render_svg, svg_size};

/// Without the `svg` feature an SVG is a format this build recognises but
/// cannot draw.
#[cfg(not(feature = "svg"))]
mod svg {
    use super::RasterImage;
    pub fn svg_size(_: &str) -> Option<(f64, f64)> {
        None
    }
    pub fn render_svg(_: &str, _: Option<(u32, u32)>) -> Option<RasterImage> {
        None
    }
}

#[cfg(feature = "svg")]
mod svg {
    use super::{finish, within_limits, RasterImage};

    fn svg_tree(source: &str) -> Option<resvg::usvg::Tree> {
        resvg::usvg::Tree::from_str(source, &resvg::usvg::Options::default()).ok()
    }

    /// The natural size of an SVG document (its `width`/`height`, or its
    /// `viewBox`).
    pub fn svg_size(source: &str) -> Option<(f64, f64)> {
        let tree = svg_tree(source)?;
        let size = tree.size();
        Some((f64::from(size.width()), f64::from(size.height())))
    }

    /// Renders an SVG document at `size` pixels (its own size when `None`),
    /// scaling the drawing to fill it.
    pub fn render_svg(source: &str, size: Option<(u32, u32)>) -> Option<RasterImage> {
        let tree = svg_tree(source)?;
        let natural = tree.size();
        let (w, h) = size.unwrap_or((
            natural.width().ceil().max(1.0) as u32,
            natural.height().ceil().max(1.0) as u32,
        ));
        if !within_limits(w, h) {
            return None;
        }
        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
        let transform = resvg::tiny_skia::Transform::from_scale(
            w as f32 / natural.width(),
            h as f32 / natural.height(),
        );
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        // tiny-skia holds premultiplied pixels; images are straight alpha.
        let mut rgba = pixmap.take();
        for p in rgba.chunks_exact_mut(4) {
            let a = u32::from(p[3]);
            if a != 0 && a != 255 {
                for c in &mut p[..3] {
                    *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        finish(w, h, rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = include_bytes!("../tests/fixtures/red_blue.png");
    const JPG: &[u8] = include_bytes!("../tests/fixtures/red_blue.jpg");
    const GIF: &[u8] = include_bytes!("../tests/fixtures/red_blue.gif");

    fn px(image: &RasterImage, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        [
            image.rgba[i],
            image.rgba[i + 1],
            image.rgba[i + 2],
            image.rgba[i + 3],
        ]
    }

    fn near(a: [u8; 4], b: [u8; 4], tolerance: i32) -> bool {
        a.iter()
            .zip(b)
            .all(|(x, y)| (i32::from(*x) - i32::from(y)).abs() <= tolerance)
    }

    #[test]
    fn formats_are_told_apart_by_their_bytes() {
        assert_eq!(sniff(PNG), Some(Format::Png));
        assert_eq!(sniff(JPG), Some(Format::Jpeg));
        assert_eq!(sniff(GIF), Some(Format::Gif));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(Format::WebP));
        assert_eq!(sniff(b"  <svg xmlns='x'/>"), Some(Format::Svg));
        assert_eq!(sniff(b"<?xml version='1.0'?><svg/>"), Some(Format::Svg));
        assert_eq!(sniff(b"<html></html>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn png_decodes_to_straight_rgba() {
        let image = decode(PNG).unwrap();
        assert_eq!((image.width, image.height), (8, 4));
        assert_eq!(px(&image, 1, 1), [255, 0, 0, 255]);
        assert_eq!(px(&image, 6, 3), [0, 0, 255, 255]);
    }

    #[test]
    fn jpeg_decodes_within_compression_error() {
        let image = decode(JPG).unwrap();
        assert_eq!((image.width, image.height), (8, 4));
        assert!(
            near(px(&image, 1, 1), [255, 0, 0, 255], 40),
            "{:?}",
            px(&image, 1, 1)
        );
        assert!(near(px(&image, 6, 2), [0, 0, 255, 255], 40));
    }

    #[test]
    fn gif_decodes_its_first_frame() {
        let image = decode(GIF).unwrap();
        assert_eq!((image.width, image.height), (8, 4));
        assert!(near(px(&image, 1, 1), [255, 0, 0, 255], 8));
        assert!(near(px(&image, 6, 1), [0, 0, 255, 255], 8));
    }

    #[test]
    fn corrupt_and_truncated_data_decodes_to_nothing() {
        assert!(decode(&PNG[..PNG.len() / 2]).is_none());
        assert!(decode(&JPG[..40]).is_none());
        assert!(decode(b"GIF89a\x01").is_none());
        assert!(decode(b"RIFF\x04\0\0\0WEBP").is_none());
        assert!(decode(b"not an image").is_none());
    }

    const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='10' viewBox='0 0 20 10'>\
        <rect width='10' height='10' fill='#ff0000'/><rect x='10' width='10' height='10' fill='#0000ff'/></svg>";

    #[test]
    fn svg_renders_at_its_size_and_scaled() {
        let image = render_svg(SVG, None).unwrap();
        assert_eq!((image.width, image.height), (20, 10));
        assert_eq!(px(&image, 2, 5), [255, 0, 0, 255]);
        assert_eq!(px(&image, 17, 5), [0, 0, 255, 255]);
        let big = render_svg(SVG, Some((60, 30))).unwrap();
        assert_eq!(px(&big, 10, 15), [255, 0, 0, 255]);
        assert_eq!(px(&big, 50, 15), [0, 0, 255, 255]);
        assert_eq!(svg_size(SVG), Some((20.0, 10.0)));
        assert_eq!(intrinsic_size(SVG.as_bytes()), Some((20, 10)));
    }

    #[test]
    fn svg_transparency_stays_straight_alpha() {
        let half = "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'>\
            <rect width='2' height='2' fill='#ff0000' fill-opacity='0.5'/></svg>";
        let image = decode(half.as_bytes()).unwrap();
        let p = px(&image, 0, 0);
        assert!(
            p[0] > 240 && p[1] == 0 && (120..136).contains(&p[3]),
            "{p:?}"
        );
    }

    #[test]
    fn oversized_and_invalid_svg_are_refused() {
        assert!(render_svg(SVG, Some((MAX_SIDE + 1, 10))).is_none());
        assert!(render_svg("<svg", None).is_none());
        assert!(
            decode(b"<svg xmlns='http://www.w3.org/2000/svg' width='0' height='0'/>").is_none()
        );
    }

    #[test]
    fn intrinsic_size_of_a_raster_is_its_pixels() {
        assert_eq!(intrinsic_size(PNG), Some((8, 4)));
        assert_eq!(intrinsic_size(b"junk"), None);
    }

    #[test]
    fn a_picture_survives_a_png_round_trip() {
        let image = RasterImage::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 128]);
        let png = encode_png(&image).unwrap();
        assert_eq!(sniff(&png), Some(Format::Png));
        assert_eq!(decode(&png).unwrap(), image);
    }
}
