// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Right-to-left runs and Arabic joining are drawn the way the shaped text would be.

use blueice_paint::{Color, Frame, PaintCommand};
use blueice_raster::rasterize;

fn drawn(text: &str, rtl: bool) -> Vec<u8> {
    let frame = Frame {
        width: 120.0,
        height: 40.0,
        commands: vec![PaintCommand::Text {
            x: 4.0,
            y: 4.0,
            text: text.to_string(),
            color: Color::Rgba(0, 0, 0, 255),
            font_size_px: 20.0,
            bold: false,
            italic: false,
            face: None,
            rtl,
            orient: blueice_paint::TextOrient::Horizontal,
            breadth: 0.0,
            features: 0,
        }],
    };
    rasterize(&frame).pixels.clone()
}

fn inked(pixels: &[u8]) -> usize {
    pixels.chunks(4).filter(|p| p[0] < 255).count()
}

#[test]
fn a_right_to_left_run_is_drawn_from_its_last_character_to_its_first() {
    let text = "\u{5d0}\u{5d1}\u{5d2}";
    let reversed = "\u{5d2}\u{5d1}\u{5d0}";
    assert!(inked(&drawn(text, true)) > 50);
    assert_eq!(drawn(text, true), drawn(reversed, false));
    assert_ne!(drawn(text, true), drawn(text, false));
}

#[test]
fn brackets_in_a_right_to_left_run_face_the_other_way() {
    // visual order of "(a" is "a)"
    assert_eq!(drawn("(a", true), drawn("a)", false));
}

#[test]
fn joined_arabic_letters_are_drawn_in_their_joining_forms() {
    let letters = "\u{628}\u{628}";
    let forms = "\u{FE91}\u{FE90}";
    assert_eq!(drawn(letters, false), drawn(forms, false));
    // read right to left on screen, the final form is on the left
    assert_eq!(drawn(letters, true), drawn("\u{FE90}\u{FE91}", false));
}

#[test]
fn a_mark_is_drawn_on_its_letter_not_after_it() {
    let letter = ink_set("\u{628}", true);
    let with_mark = ink_set("\u{628}\u{64E}", true);
    let mark: std::collections::BTreeSet<_> = with_mark.difference(&letter).copied().collect();
    assert!(mark.len() > 3, "the mark is drawn");
    let (left, right) = (letter.iter().map(|p| p.0).min().unwrap(), letter.iter().map(|p| p.0).max().unwrap());
    let top = letter.iter().map(|p| p.1).min().unwrap();
    assert!(mark.iter().all(|p| p.0 >= left - 2 && p.0 <= right + 2), "within the letter's width: {left}..{right}");
    assert!(mark.iter().all(|p| p.1 < top + 2), "above the body of the letter");
}

#[test]
fn the_fonts_own_ligatures_and_kerning_decide_the_width_of_arabic() {
    // shaped as the font says, a word is as wide as its glyph advances
    let text = "\u{628}\u{644}\u{627}\u{633}";
    let width = blueice_font::measure_text_width(text, 20.0, false, false);
    let glyphs = blueice_font::shape_for_drawing(text, false, false, None, 20.0, true).unwrap();
    let sum: f32 = glyphs.iter().map(|g| g.advance).sum();
    assert!((width - f64::from(sum)).abs() < 0.01);
}

fn ink_set(text: &str, rtl: bool) -> std::collections::BTreeSet<(i64, i64)> {
    let frame = Frame {
        width: 120.0,
        height: 60.0,
        commands: vec![PaintCommand::Text {
            x: 10.0,
            y: 10.0,
            text: text.to_string(),
            color: Color::Rgba(0, 0, 0, 255),
            font_size_px: 32.0,
            bold: false,
            italic: false,
            face: None,
            rtl,
            orient: blueice_paint::TextOrient::Horizontal,
            breadth: 0.0,
            features: 0,
        }],
    };
    let pixmap = rasterize(&frame);
    let mut out = std::collections::BTreeSet::new();
    for y in 0..pixmap.height as usize {
        for x in 0..pixmap.width as usize {
            if pixmap.pixels[(y * pixmap.width as usize + x) * 4] < 128 {
                out.insert((x as i64, y as i64));
            }
        }
    }
    out
}
