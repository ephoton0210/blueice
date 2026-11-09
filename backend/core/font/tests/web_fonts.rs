// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fonts a page brings with it: WOFF2, WOFF and plain sfnt files decode into faces that
//! text can be measured and drawn with, next to the bundled ones.

use blueice_font::web::{self, MAX_FONT_BYTES};
use blueice_font::{
    ascent_descent, ascent_descent_in, font_for_char, font_for_char_in, measure_text_width,
    measure_text_width_in,
};

const WOFF2: &[u8] = include_bytes!("data/roboto-latin-400.woff2");
const WOFF2_BOLD: &[u8] = include_bytes!("data/roboto-latin-700.woff2");
const WOFF: &[u8] = include_bytes!("data/roboto-latin-400.woff");
const TTF: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
/// A real OpenType file, but 16 MB: over what a page may bring.
const HUGE_OTF: &[u8] = include_bytes!("../assets/NotoSansTC-Regular.otf");

#[test]
fn woff2_woff_and_plain_sfnt_files_register_as_faces() {
    for (label, bytes) in [("woff2", WOFF2), ("woff", WOFF), ("ttf", TTF)] {
        assert!(web::register(bytes).is_some(), "{label}");
    }
}

#[test]
fn garbage_truncated_and_oversized_files_are_refused_not_panicked_on() {
    assert!(web::register(b"").is_none());
    assert!(web::register(b"not a font").is_none());
    assert!(web::register(b"wOF2").is_none());
    for cut in (0..WOFF2.len()).step_by(997) {
        let _ = web::register(&WOFF2[..cut]);
    }
    for cut in (0..WOFF.len()).step_by(997) {
        let _ = web::register(&WOFF[..cut]);
    }
    // Flipped bytes inside the compressed stream must not panic either.
    for at in (40..WOFF2.len()).step_by(1499) {
        let mut damaged = WOFF2.to_vec();
        damaged[at] ^= 0xFF;
        let _ = web::register(&damaged);
    }
    let huge = vec![0u8; MAX_FONT_BYTES + 1];
    assert!(web::register(&huge).is_none());
    assert!(HUGE_OTF.len() > MAX_FONT_BYTES);
    assert!(web::register(HUGE_OTF).is_none(), "a real font over the size limit");
}

#[test]
fn the_same_file_is_one_face_however_often_it_is_registered() {
    let a = web::register(WOFF2).unwrap();
    let b = web::register(WOFF2).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, web::register(WOFF2_BOLD).unwrap());
}

#[test]
fn text_is_measured_and_drawn_with_the_face_it_asks_for() {
    let roboto = web::register(WOFF2).unwrap();
    let bundled = measure_text_width("Hello world", 16.0, false, false);
    let own = measure_text_width_in("Hello world", 16.0, false, false, Some(roboto));
    assert!(own > 0.0 && (own - bundled).abs() > 0.5, "{own} vs {bundled}");
    // The sum of its own advances, exactly.
    let face = font_for_char_in('H', false, false, Some(roboto));
    let expected: f32 = "Hello world"
        .chars()
        .map(|c| font_for_char_in(c, false, false, Some(roboto)).metrics(c, 16.0).advance_width)
        .sum();
    assert!((own - f64::from(expected)).abs() < 1e-6);
    assert!(face.has_glyph('H'));
    // No face asked for: exactly what it always was.
    assert_eq!(measure_text_width_in("Hello world", 16.0, false, false, None), bundled);
}

#[test]
fn a_character_the_face_lacks_comes_from_the_bundled_fonts() {
    let roboto = web::register(WOFF2).unwrap();
    let mixed = measure_text_width_in("a中", 16.0, false, false, Some(roboto));
    let a = measure_text_width_in("a", 16.0, false, false, Some(roboto));
    let zhong = measure_text_width("中", 16.0, false, false);
    assert!((mixed - (a + zhong)).abs() < 1e-6, "{mixed} {a} {zhong}");
    assert!(std::ptr::eq(
        font_for_char_in('中', false, false, Some(roboto)),
        font_for_char('中', false, false)
    ));
}

#[test]
fn line_metrics_follow_the_face() {
    let roboto = web::register(WOFF2).unwrap();
    let (a, d) = ascent_descent_in(16.0, false, false, Some(roboto));
    let (ba, bd) = ascent_descent(16.0, false, false);
    assert!(a > 5.0 && d > 1.0);
    assert!((a - ba).abs() > 0.01 || (d - bd).abs() > 0.01, "{a},{d} vs {ba},{bd}");
    assert_eq!(ascent_descent_in(16.0, false, false, None), (ba, bd));
}

#[test]
fn a_set_of_faces_draws_each_character_with_the_first_member_whose_range_covers_it() {
    let upper_face = web::register(WOFF2).unwrap();
    let lower_face = web::register(WOFF2_BOLD).unwrap();
    let set = web::register_set(&[
        (upper_face, vec![(0x41, 0x5A)]),
        (lower_face, vec![(0x61, 0x7A)]),
    ])
    .unwrap();
    let upper = web::get(upper_face).unwrap();
    let lower = web::get(lower_face).unwrap();
    assert!(std::ptr::eq(font_for_char_in('A', false, false, Some(set)), upper));
    assert!(std::ptr::eq(font_for_char_in('b', false, false, Some(set)), lower));
    // Outside every range, a digit both faces have a glyph for: the bundled font draws it.
    assert!(std::ptr::eq(
        font_for_char_in('7', false, false, Some(set)),
        font_for_char('7', false, false)
    ));
    // Widths follow the same choice, character by character.
    let expected = font_for_char_in('A', false, false, Some(set)).metrics('A', 16.0).advance_width
        + font_for_char_in('b', false, false, Some(set)).metrics('b', 16.0).advance_width;
    assert!((measure_text_width_in("Ab", 16.0, false, false, Some(set)) - f64::from(expected)).abs() < 1e-6);
}

#[test]
fn overlapping_ranges_go_to_the_member_listed_first_and_an_empty_range_covers_everything() {
    let a = web::register(WOFF2).unwrap();
    let b = web::register(WOFF2_BOLD).unwrap();
    let first_wins = web::register_set(&[(b, vec![(0x41, 0x7A)]), (a, vec![])]).unwrap();
    assert!(std::ptr::eq(font_for_char_in('k', false, false, Some(first_wins)), web::get(b).unwrap()));
    // A character in no range of the first member falls to the member with no range at all.
    assert!(std::ptr::eq(font_for_char_in('7', false, false, Some(first_wins)), web::get(a).unwrap()));
}

#[test]
fn a_member_in_range_that_lacks_the_glyph_leaves_the_character_to_the_next_member_then_the_bundled_fonts() {
    let roboto = web::register(WOFF2).unwrap();
    let dejavu = web::register(TTF).unwrap();
    // `中` is in both ranges but neither face has it: the bundled CJK fallback draws it.
    let set = web::register_set(&[(roboto, vec![(0x4E00, 0x9FFF)]), (dejavu, vec![(0x4E00, 0x9FFF)])]).unwrap();
    assert!(std::ptr::eq(
        font_for_char_in('中', false, false, Some(set)),
        font_for_char('中', false, false)
    ));
}

#[test]
fn a_set_of_one_member_with_no_range_is_that_face_and_equal_sets_are_one_set() {
    let a = web::register(WOFF2).unwrap();
    assert_eq!(web::register_set(&[(a, vec![])]), Some(a));
    let b = web::register(WOFF2_BOLD).unwrap();
    let one = web::register_set(&[(a, vec![(1, 2)]), (b, vec![])]).unwrap();
    let two = web::register_set(&[(a, vec![(1, 2)]), (b, vec![])]).unwrap();
    assert_eq!(one, two);
    assert_ne!(one, web::register_set(&[(b, vec![]), (a, vec![(1, 2)])]).unwrap(), "order matters");
    assert!(web::register_set(&[]).is_none());
}

#[test]
fn line_metrics_of_a_set_come_from_the_member_that_covers_ordinary_latin_text() {
    let latin = web::register(WOFF2).unwrap();
    let other = web::register(TTF).unwrap();
    let set = web::register_set(&[(other, vec![(0x400, 0x4FF)]), (latin, vec![(0x0, 0xFF)])]).unwrap();
    assert_eq!(
        ascent_descent_in(16.0, false, false, Some(set)),
        ascent_descent_in(16.0, false, false, Some(latin))
    );
}
