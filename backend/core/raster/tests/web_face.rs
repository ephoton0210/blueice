// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text drawn with a face the page brought, not the bundled font.

use blueice_paint::{Color, Frame, PaintCommand};
use blueice_raster::rasterize;

const ROBOTO: &[u8] = include_bytes!("../../font/tests/data/roboto-latin-400.woff2");

fn text(face: Option<u32>, string: &str) -> Frame {
    Frame {
        width: 120.0,
        height: 60.0,
        commands: vec![PaintCommand::Text {
            x: 4.0,
            y: 4.0,
            text: string.to_string(),
            color: Color::Rgba(0, 0, 0, 255),
            font_size_px: 32.0,
            bold: false,
            italic: false,
            face,
        }],
    }
}

/// `(left, top, right, bottom)` of the dark pixels.
fn ink(frame: &Frame) -> (i64, i64, i64, i64) {
    let pixmap = rasterize(frame);
    let mut bounds = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for y in 0..pixmap.height {
        for x in 0..pixmap.width {
            if pixmap.get_pixel(x, y)[0] < 128 {
                bounds.0 = bounds.0.min(i64::from(x));
                bounds.1 = bounds.1.min(i64::from(y));
                bounds.2 = bounds.2.max(i64::from(x));
                bounds.3 = bounds.3.max(i64::from(y));
            }
        }
    }
    bounds
}

#[test]
fn a_run_with_a_face_is_drawn_in_that_face_and_not_in_the_bundled_font() {
    let face = blueice_font::web::register(ROBOTO).unwrap().as_u32();
    let own = rasterize(&text(Some(face), "Hag"));
    let bundled = rasterize(&text(None, "Hag"));
    let differing = own
        .pixels
        .iter()
        .zip(&bundled.pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert!(differing > 200, "{differing} bytes differ");
    assert_ne!(ink(&text(Some(face), "Hag")), ink(&text(None, "Hag")));
}

#[test]
fn the_baseline_comes_from_the_faces_own_ascent() {
    let id = blueice_font::web::register(ROBOTO).unwrap();
    let (ascent, _) = blueice_font::ascent_descent_in(32.0, false, false, Some(id));
    // `H` stands on the baseline: its lowest ink row is the row above `y + ascent`.
    let (_, _, _, bottom) = ink(&text(Some(id.as_u32()), "H"));
    let expected = (4.0 + ascent).round() as i64;
    assert!((bottom - expected).abs() <= 1, "bottom {bottom}, baseline {expected}");
}

#[test]
fn a_character_the_face_lacks_is_still_drawn_from_the_bundled_fonts() {
    let face = blueice_font::web::register(ROBOTO).unwrap().as_u32();
    let (left, _, right, _) = ink(&text(Some(face), "中"));
    assert!(right > left + 10, "the CJK character is drawn by the fallback: {left}..{right}");
}
