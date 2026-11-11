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
