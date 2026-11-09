// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AVIF still images -> RGBA8 pixels.

/// A decoded picture: straight (non-premultiplied) RGBA, 8 bits per channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Whether `bytes` start like an AVIF file (an `ftyp` box naming `avif`/`avis`).
pub fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let end = size.clamp(16, bytes.len());
    // Major brand, then (after the minor version) the compatible brands.
    let brands = std::iter::once(&bytes[8..12]).chain(bytes[16..end].chunks_exact(4));
    brands
        .into_iter()
        .any(|brand| brand == b"avif" || brand == b"avis")
}

/// Decodes a still AVIF of at most `max_pixels` pixels. Malformed input is `None`; a file the
/// container parser trips over (it asserts on some truncated files) is too, not a panic.
pub fn decode(bytes: &[u8], max_pixels: u32) -> Option<Decoded> {
    std::panic::catch_unwind(|| decode_unguarded(bytes, max_pixels))
        .ok()
        .flatten()
}

fn decode_unguarded(bytes: &[u8], max_pixels: u32) -> Option<Decoded> {
    if !is_avif(bytes) {
        return None;
    }
    let data = avif_parse::read_avif(&mut std::io::Cursor::new(bytes)).ok()?;
    let color = av1::decode(&data.primary_item, max_pixels)?;
    let alpha = match &data.alpha_item {
        Some(item) => {
            let plane = av1::decode(item, max_pixels)?;
            // The alpha image is the same size as the colour image.
            if (plane.width, plane.height) != (color.width, color.height) {
                return None;
            }
            Some(plane)
        }
        None => None,
    };
    Some(convert::to_rgba(
        &color,
        alpha.as_ref(),
        data.premultiplied_alpha,
    ))
}

/// What an AV1 frame is, as planes of samples (10/12-bit samples keep their depth).
pub(crate) struct Frame {
    pub width: u32,
    pub height: u32,
    /// Chroma sampling: `(horizontal, vertical)` halving; `None` for monochrome.
    pub chroma: Option<(bool, bool)>,
    pub bit_depth: u32,
    pub full_range: bool,
    /// ITU-T H.273 matrix coefficients.
    pub matrix: u32,
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
}

mod av1 {
    //! The one place that talks to the AV1 decoder's C-shaped interface. Everything `unsafe`
    //! is in `decode`; it hands back owned samples and keeps no pointer into the decoder.

    use super::Frame;
    use rav1d::include::dav1d::data::Dav1dData;
    use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
    use rav1d::include::dav1d::picture::Dav1dPicture;
    use rav1d::src::lib::{
        dav1d_close, dav1d_data_create, dav1d_default_settings, dav1d_get_picture, dav1d_open,
        dav1d_picture_unref, dav1d_send_data,
    };
    use std::mem::MaybeUninit;
    use std::ptr::NonNull;

    const PIXEL_LAYOUT_I400: u32 = 0;
    const PIXEL_LAYOUT_I420: u32 = 1;
    const PIXEL_LAYOUT_I422: u32 = 2;

    pub(super) fn decode(obu: &[u8], max_pixels: u32) -> Option<Frame> {
        if obu.is_empty() {
            return None;
        }
        // SAFETY: each pointer passed below is to a live local of the type the function
        // documents; the context is closed on every path, the picture unreferenced once
        // obtained, and the sample planes are only read within `stride * height` bytes
        // that the decoder reports for the picture it still owns.
        unsafe {
            let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
            dav1d_default_settings(NonNull::new(settings.as_mut_ptr())?);
            let mut settings = settings.assume_init();
            settings.n_threads = 1;
            settings.max_frame_delay = 1;
            settings.frame_size_limit = max_pixels;
            settings.apply_grain = 1;
            let mut context: Option<Dav1dContext> = None;
            if dav1d_open(NonNull::new(&mut context), NonNull::new(&mut settings)).0 != 0 {
                return None;
            }
            let frame = decode_with(&mut context, obu);
            dav1d_close(NonNull::new(&mut context));
            frame
        }
    }

    /// # Safety
    /// `context` holds an open decoder.
    unsafe fn decode_with(context: &mut Option<Dav1dContext>, obu: &[u8]) -> Option<Frame> {
        let mut data = MaybeUninit::<Dav1dData>::zeroed();
        // SAFETY: `data` is writable; the buffer it is given is `obu.len()` bytes long.
        let buffer = unsafe { dav1d_data_create(NonNull::new(data.as_mut_ptr()), obu.len()) };
        if buffer.is_null() {
            return None;
        }
        // SAFETY: the buffer was just created with exactly this length.
        unsafe { std::ptr::copy_nonoverlapping(obu.as_ptr(), buffer, obu.len()) };
        // SAFETY: created and filled above.
        let mut data = unsafe { data.assume_init() };
        let again = -libc::EAGAIN;
        loop {
            if data.sz > 0 {
                // SAFETY: `context` is open and `data` is the buffer made above.
                let sent = unsafe { dav1d_send_data(*context, NonNull::new(&mut data)) };
                if sent.0 < 0 && sent.0 != again {
                    return None;
                }
            }
            let mut picture = MaybeUninit::<Dav1dPicture>::zeroed();
            // SAFETY: `picture` is writable.
            let got = unsafe { dav1d_get_picture(*context, NonNull::new(picture.as_mut_ptr())) };
            if got.0 == 0 {
                // SAFETY: the decoder wrote a whole picture.
                let mut picture = unsafe { picture.assume_init() };
                // SAFETY: this picture is complete and not yet unreferenced.
                let frame = unsafe { read_picture(&picture) };
                // SAFETY: unreferenced exactly once.
                unsafe { dav1d_picture_unref(NonNull::new(&mut picture)) };
                return frame;
            }
            if got.0 != again || data.sz == 0 {
                return None;
            }
        }
    }

    /// # Safety
    /// `picture` is a complete picture the decoder still owns.
    unsafe fn read_picture(picture: &Dav1dPicture) -> Option<Frame> {
        let (width, height) = (
            u32::try_from(picture.p.w).ok()?,
            u32::try_from(picture.p.h).ok()?,
        );
        let bit_depth = u32::try_from(picture.p.bpc).ok()?;
        if width == 0 || height == 0 || !matches!(bit_depth, 8 | 10 | 12) {
            return None;
        }
        let chroma = match picture.p.layout {
            PIXEL_LAYOUT_I400 => None,
            PIXEL_LAYOUT_I420 => Some((true, true)),
            PIXEL_LAYOUT_I422 => Some((true, false)),
            _ => Some((false, false)),
        };
        // SAFETY: the sequence header travels with the picture.
        let header = unsafe { picture.seq_hdr?.as_ref() };
        let read = |index: usize, w: u32, h: u32| -> Option<Vec<u16>> {
            let base = picture.data[index]?.as_ptr() as *const u8;
            let stride = picture.stride[usize::from(index > 0)];
            let bytes = if bit_depth == 8 { 1 } else { 2 };
            let mut out = Vec::with_capacity((w * h) as usize);
            for row in 0..h as isize {
                // SAFETY: row `row` of a plane `h` rows tall starts `row * stride` bytes in
                // and holds `w` samples of `bytes` bytes.
                let line = unsafe { base.offset(row * stride) };
                for column in 0..w as usize {
                    // SAFETY: within the row, as above (2-byte samples are aligned by the decoder).
                    let sample = unsafe {
                        if bytes == 1 {
                            u16::from(*line.add(column))
                        } else {
                            (line.add(column * 2) as *const u16).read_unaligned()
                        }
                    };
                    out.push(sample);
                }
            }
            Some(out)
        };
        let y = read(0, width, height)?;
        let (u, v) = match chroma {
            None => (Vec::new(), Vec::new()),
            Some((halve_x, halve_y)) => {
                let cw = if halve_x { width.div_ceil(2) } else { width };
                let ch = if halve_y { height.div_ceil(2) } else { height };
                (read(1, cw, ch)?, read(2, cw, ch)?)
            }
        };
        Some(Frame {
            width,
            height,
            chroma,
            bit_depth,
            full_range: header.color_range != 0,
            matrix: header.mtrx,
            y,
            u,
            v,
        })
    }
}

mod convert {
    //! Samples to straight RGBA8.

    use super::{Decoded, Frame};

    /// ITU-T H.273 MatrixCoefficients: identity (GBR), BT.709, BT.601-ish and BT.2020.
    const MC_IDENTITY: u32 = 0;
    const MC_BT709: u32 = 1;
    const MC_BT2020_NCL: u32 = 9;

    /// `(Kr, Kb)` for a matrix; unspecified is BT.601, as AVIF defines.
    fn coefficients(matrix: u32) -> (f64, f64) {
        match matrix {
            MC_BT709 => (0.2126, 0.0722),
            MC_BT2020_NCL | 10 => (0.2627, 0.0593),
            _ => (0.299, 0.114),
        }
    }

    pub(super) fn to_rgba(color: &Frame, alpha: Option<&Frame>, premultiplied: bool) -> Decoded {
        let (width, height) = (color.width as usize, color.height as usize);
        let max = f64::from((1u32 << color.bit_depth) - 1);
        let scale = f64::from(1u32 << (color.bit_depth - 8));
        let (kr, kb) = coefficients(color.matrix);
        let kg = 1.0 - kr - kb;
        // Limited range puts black at 16 and white at 235 (and chroma 16..240), in 8-bit units.
        let (y_offset, y_span, c_span) = if color.full_range {
            (0.0, max, max)
        } else {
            (16.0 * scale, 219.0 * scale, 224.0 * scale)
        };
        let alpha_scale = alpha.map(|a| {
            let m = f64::from((1u32 << a.bit_depth) - 1);
            if a.full_range {
                (0.0, m)
            } else {
                (
                    16.0 * f64::from(1u32 << (a.bit_depth - 8)),
                    219.0 * f64::from(1u32 << (a.bit_depth - 8)),
                )
            }
        });
        let mut rgba = Vec::with_capacity(width * height * 4);
        for py in 0..height {
            for px in 0..width {
                let y = f64::from(color.y[py * width + px]);
                let (r, g, b) = match color.chroma {
                    None => {
                        let l = ((y - y_offset) / y_span).clamp(0.0, 1.0);
                        (l, l, l)
                    }
                    Some((halve_x, halve_y)) => {
                        let cw = if halve_x { width.div_ceil(2) } else { width };
                        let (cx, cy) = (
                            if halve_x { px / 2 } else { px },
                            if halve_y { py / 2 } else { py },
                        );
                        let u = f64::from(color.u[cy * cw + cx]);
                        let v = f64::from(color.v[cy * cw + cx]);
                        if color.matrix == MC_IDENTITY {
                            // GBR: the planes are green, blue and red themselves.
                            let unit = |s: f64| ((s - y_offset) / y_span).clamp(0.0, 1.0);
                            (unit(v), unit(y), unit(u))
                        } else {
                            let luma = ((y - y_offset) / y_span).clamp(0.0, 1.0);
                            let cb = (u - (max + 1.0) / 2.0) / c_span;
                            let cr = (v - (max + 1.0) / 2.0) / c_span;
                            let r = luma + 2.0 * (1.0 - kr) * cr;
                            let b = luma + 2.0 * (1.0 - kb) * cb;
                            let g = (luma - kr * r - kb * b) / kg;
                            (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
                        }
                    }
                };
                let a = match (alpha, alpha_scale) {
                    (Some(plane), Some((offset, span))) => {
                        ((f64::from(plane.y[py * width + px]) - offset) / span).clamp(0.0, 1.0)
                    }
                    _ => 1.0,
                };
                let (mut r, mut g, mut b) = (r, g, b);
                if premultiplied && a > 0.0 {
                    r = (r / a).min(1.0);
                    g = (g / a).min(1.0);
                    b = (b / a).min(1.0);
                }
                for channel in [r, g, b, a] {
                    rgba.push((channel * 255.0).round() as u8);
                }
            }
        }
        Decoded {
            width: color.width,
            height: color.height,
            rgba,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made by macOS `sips` from a 64 x 48 PNG (colour only: `sips` drops the alpha).
    const GRADIENT: &[u8] = include_bytes!("../tests/data/gradient_opaque.avif");
    /// The same picture encoded by `ravif`, with an alpha item (alpha 90 for x < 8, else 255).
    const WITH_ALPHA: &[u8] = include_bytes!("../tests/data/gradient_with_alpha.avif");

    fn pixel(d: &Decoded, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * d.width + x) * 4) as usize;
        [d.rgba[i], d.rgba[i + 1], d.rgba[i + 2], d.rgba[i + 3]]
    }

    fn near(actual: u8, expected: u8, tolerance: u8) -> bool {
        actual.abs_diff(expected) <= tolerance
    }

    #[test]
    fn an_avif_file_is_recognised_by_its_ftyp_brand() {
        assert!(is_avif(GRADIENT));
        assert!(!is_avif(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_avif(b""));
        // A different ISO media brand is not AVIF.
        assert!(!is_avif(b"\0\0\0\x14ftypheic\0\0\0\0heic"));
    }

    #[test]
    fn a_gradient_decodes_to_its_size_and_colours() {
        for (label, file) in [("sips", GRADIENT), ("ravif", WITH_ALPHA)] {
            let d = decode(file, 1 << 20).expect("decodes");
            assert_eq!((d.width, d.height), (64, 48), "{label}");
            assert_eq!(d.rgba.len(), 64 * 48 * 4, "{label}");
            // The source was r = 4x, g = 40, b = 5y (lossy compression and the YUV round trip
            // allow some drift).
            let mid = pixel(&d, 32, 24);
            assert!(near(mid[0], 128, 28), "{label} red {mid:?}");
            assert!(near(mid[1], 40, 28), "{label} green {mid:?}");
            assert!(near(mid[2], 120, 28), "{label} blue {mid:?}");
            assert!(near(mid[3], 255, 4), "{label} opaque {mid:?}");
            // Colour moves with x and y the way the gradient does.
            assert!(pixel(&d, 60, 24)[0] > pixel(&d, 12, 24)[0] + 100, "{label}");
            assert!(pixel(&d, 32, 44)[2] > pixel(&d, 32, 4)[2] + 100, "{label}");
        }
    }

    #[test]
    fn a_file_without_an_alpha_item_is_opaque() {
        let d = decode(GRADIENT, 1 << 20).unwrap();
        assert!(d.rgba.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn an_alpha_item_becomes_the_alpha_channel_and_colour_stays_straight() {
        let d = decode(WITH_ALPHA, 1 << 20).unwrap();
        let strip = pixel(&d, 2, 24);
        assert!(near(strip[3], 90, 12), "translucent strip {strip:?}");
        // Straight alpha: the strip's red is its own (r = 8), not scaled down by 90/255.
        assert!(strip[0] < 40, "{strip:?}");
        assert!(near(pixel(&d, 32, 24)[3], 255, 4));
        assert!(near(pixel(&d, 10, 24)[3], 255, 4));
    }

    #[test]
    fn garbage_truncated_and_oversized_files_are_refused() {
        assert!(decode(b"", 1 << 20).is_none());
        assert!(decode(b"not an image at all", 1 << 20).is_none());
        assert!(decode(&GRADIENT[..GRADIENT.len() / 2], 1 << 20).is_none());
        // 64 x 48 = 3072 pixels is over a 3000-pixel limit.
        assert!(decode(GRADIENT, 3000).is_none());
        assert!(decode(GRADIENT, 3072).is_some());
        // Every prefix of a real file is refused or decoded, never a panic.
        for cut in (0..WITH_ALPHA.len()).step_by(7) {
            let _ = decode(&WITH_ALPHA[..cut], 1 << 20);
        }
    }
}
