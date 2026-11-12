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
            rtl: false,
            orient: blueice_paint::TextOrient::Horizontal,
            breadth: 0.0,
            features: 0,
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

#[test]
fn a_set_of_faces_is_drawn_character_by_character() {
    const BOLD: &[u8] = include_bytes!("../../font/tests/data/roboto-latin-700.woff2");
    let regular = blueice_font::web::register(ROBOTO).unwrap();
    let bold = blueice_font::web::register(BOLD).unwrap();
    // `A` from the bold face, everything else from the regular one.
    let set = blueice_font::web::register_set(&[(bold, vec![(0x41, 0x41)]), (regular, vec![])]).unwrap();
    let drawn = |face: u32, string: &str| rasterize(&text(Some(face), string));
    // `a` is in no bold range: the set draws it exactly as the regular face does.
    assert_eq!(drawn(set.as_u32(), "a"), drawn(regular.as_u32(), "a"));
    // `A` is the bold face's: drawn exactly as the bold face does, and not as the regular one.
    assert_eq!(drawn(set.as_u32(), "A"), drawn(bold.as_u32(), "A"));
    assert_ne!(drawn(set.as_u32(), "A"), drawn(regular.as_u32(), "A"));
}

#[test]
fn a_face_without_bold_or_italic_is_smeared_and_slanted_by_drawing() {
    let id = blueice_font::web::register(ROBOTO).unwrap().as_u32();
    let styled = |bold, italic| {
        let mut frame = text(Some(id), "Hi");
        if let PaintCommand::Text { bold: b, italic: i, .. } = &mut frame.commands[0] {
            (*b, *i) = (bold, italic);
        }
        frame
    };
    let count = |frame: &Frame| {
        let p = rasterize(frame);
        (0..p.height).flat_map(|y| (0..p.width).map(move |x| (x, y))).filter(|&(x, y)| p.get_pixel(x, y)[0] < 128).count()
    };
    let plain = count(&styled(false, false));
    assert!(count(&styled(true, false)) > plain + plain / 10, "bold has more ink");
    // Slanting moves the top of the glyphs right and leaves the baseline row where it was.
    let (l0, _, r0, _) = ink(&styled(false, false));
    let (l1, _, r1, _) = ink(&styled(false, true));
    assert!(r1 > r0 && l1 >= l0, "{l0}..{r0} vs {l1}..{r1}");
    assert!(count(&styled(false, true)).abs_diff(plain) < plain / 5, "same amount of ink");
}
