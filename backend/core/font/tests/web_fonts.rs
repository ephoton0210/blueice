// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fonts a page brings with it: WOFF2, WOFF and plain sfnt files decode into faces that
//! text can be measured and drawn with, next to the bundled ones.

use blueice_font::Variation;
use blueice_font::web::{self, MAX_FONT_BYTES};
use blueice_font::{
    Glyphs,
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
    assert!(font_for_char_in('中', false, false, Some(roboto)).address() == Glyphs::Static(font_for_char('中', false, false)).address());
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
    assert!(font_for_char_in('A', false, false, Some(set)).address() == upper.address());
    assert!(font_for_char_in('b', false, false, Some(set)).address() == lower.address());
    // Outside every range, a digit both faces have a glyph for: the bundled font draws it.
    assert!(font_for_char_in('7', false, false, Some(set)).address() == Glyphs::Static(font_for_char('7', false, false)).address());
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
    assert!(font_for_char_in('k', false, false, Some(first_wins)).address() == web::get(b).unwrap().address());
    // A character in no range of the first member falls to the member with no range at all.
    assert!(font_for_char_in('7', false, false, Some(first_wins)).address() == web::get(a).unwrap().address());
}

#[test]
fn a_member_in_range_that_lacks_the_glyph_leaves_the_character_to_the_next_member_then_the_bundled_fonts() {
    let roboto = web::register(WOFF2).unwrap();
    let dejavu = web::register(TTF).unwrap();
    // `中` is in both ranges but neither face has it: the bundled CJK fallback draws it.
    let set = web::register_set(&[(roboto, vec![(0x4E00, 0x9FFF)]), (dejavu, vec![(0x4E00, 0x9FFF)])]).unwrap();
    assert!(font_for_char_in('中', false, false, Some(set)).address() == Glyphs::Static(font_for_char('中', false, false)).address());
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

const INTER_VAR: &[u8] = include_bytes!("data/inter-latin-var.woff2");

fn varied(weight: u16) -> blueice_font::web::WebFontId {
    let face = web::register(INTER_VAR).unwrap();
    web::register_set_varied(&[(face, vec![], Variation { wght: Some(weight), ..Variation::NONE })]).unwrap()
}

#[test]
fn a_variable_face_is_drawn_at_the_weight_asked_for() {
    let (light, regular, black) = (varied(100), varied(400), varied(900));
    assert_ne!(light, regular);
    let width = |id| measure_text_width_in("Hamburgefonstiv", 32.0, false, false, Some(id));
    assert!(width(light) < width(regular) && width(regular) < width(black), "wider as it gets heavier");
    // Heavier strokes: more ink.
    let ink = |id| {
        let (_, bitmap) = font_for_char_in('H', false, false, Some(id)).rasterize('H', 64.0);
        bitmap.iter().map(|&v| u64::from(v)).sum::<u64>()
    };
    assert!(ink(light) < ink(regular) && ink(regular) < ink(black));
}

#[test]
fn weights_outside_the_axis_are_clamped_and_equal_sets_are_one() {
    assert_eq!(varied(900), varied(5000), "past the axis maximum is the maximum");
    assert_eq!(varied(400), varied(400));
    // A static face has no weights: the same set whatever weight it is asked for.
    let roboto = web::register(WOFF2).unwrap();
    let at = |w| web::register_set_varied(&[(roboto, vec![], Variation { wght: Some(w), ..Variation::NONE })]).unwrap();
    assert_eq!(at(100), at(900));
}

#[test]
fn a_variable_glyphs_metrics_agree_with_the_bitmap() {
    let id = varied(700);
    let face = font_for_char_in('g', false, false, Some(id));
    let (m, bitmap) = face.rasterize('g', 40.0);
    assert_eq!(bitmap.len(), m.width * m.height);
    assert!(m.width > 10 && m.height > 20 && m.advance_width > 10.0);
    assert!(m.ymin < 0, "g has a descender below the baseline");
    let (asc, desc) = ascent_descent_in(40.0, false, false, Some(id));
    assert!(asc > 25.0 && desc > 5.0);
    assert!(face.has_glyph('g') && !face.has_glyph('\u{4e00}'));
}

#[test]
fn bundled_fonts_answer_to_the_names_a_local_source_uses() {
    let regular = blueice_font::bundled_font_bytes("DejaVu Sans").unwrap();
    assert_eq!(blueice_font::bundled_font_bytes("dejavusans"), Some(regular));
    assert_eq!(blueice_font::bundled_font_bytes("DejaVuSans"), Some(regular));
    let bold = blueice_font::bundled_font_bytes("DejaVu Sans Bold").unwrap();
    assert_ne!(bold, regular);
    assert_eq!(blueice_font::bundled_font_bytes("DejaVuSans-Bold"), Some(bold));
    assert!(blueice_font::bundled_font_bytes("Noto Sans TC").is_some());
    assert!(blueice_font::bundled_font_bytes("Arial").is_none());
    assert!(blueice_font::web::register(bold).is_some(), "a bundled file decodes as a page face");
}

const INTER_OPSZ: &[u8] = include_bytes!("data/inter-latin-opsz.woff2");
const OPEN_SANS_WDTH: &[u8] = include_bytes!("data/opensans-latin-wdth.woff2");

fn axis_of(file: &[u8], tag: &[u8; 4]) -> Option<web::Axis> {
    web::axes_of(web::register(file).unwrap()).into_iter().find(|a| &a.tag == tag)
}

fn axis(tag: &[u8; 4]) -> Option<web::Axis> {
    axis_of(INTER_VAR, tag)
}

#[test]
fn a_variable_faces_axes_are_listed() {
    let wght = axis(b"wght").expect("Inter has a weight axis");
    assert!(wght.min <= 100.0 && wght.max >= 900.0 && wght.min <= wght.default && wght.default <= wght.max);
    assert!(web::axes_of(web::register(WOFF2).unwrap()).is_empty(), "a static face has none");
}

#[test]
fn an_optical_size_axis_changes_the_drawing_and_unknown_axes_are_ignored() {
    let opsz = axis_of(INTER_OPSZ, b"opsz").expect("the fixture has an optical size axis");
    let face = web::register(INTER_OPSZ).unwrap();
    let at = |v: Variation| web::register_set_varied(&[(face, vec![], v)]).unwrap();
    let (small, large) = (
        at(Variation { opsz: Some((opsz.min * 10.0) as u16), ..Variation::NONE }),
        at(Variation { opsz: Some((opsz.max * 10.0) as u16), ..Variation::NONE }),
    );
    assert_ne!(small, large);
    let width = |id| measure_text_width_in("Hamburgefonstiv", 32.0, false, false, Some(id));
    assert!((width(small) - width(large)).abs() > 0.5, "a display cut is drawn with other widths");
    // The font has no slant or width axis: asking for them changes nothing.
    let plain = at(Variation::NONE);
    let skewed = at(Variation { slnt: Some(-100), wdth: Some(750), ital: Some(true), ..Variation::NONE });
    assert_eq!(plain, skewed, "axes the face does not have are dropped from the request");
}

const ROBOTO_FLEX: &[u8] = include_bytes!("data/robotoflex-latin-grad.woff2");

#[test]
fn a_custom_axis_is_listed_driven_clamped_and_dropped_where_absent() {
    let soft = axis_of(ROBOTO_FLEX, b"GRAD").expect("the fixture has a grade axis, which is not a registered one");
    assert!(soft.min < soft.max);
    let face = web::register(ROBOTO_FLEX).unwrap();
    let at = |v: Variation| web::register_set_varied(&[(face, vec![], v)]).unwrap();
    let (sharp, round) = (
        at(Variation::NONE.with_axis(*b"GRAD", (soft.min * 1000.0) as i32)),
        at(Variation::NONE.with_axis(*b"GRAD", (soft.max * 1000.0) as i32)),
    );
    assert_ne!(sharp, round);
    let ink = |id| {
        let (_, bitmap) = font_for_char_in('o', false, false, Some(id)).rasterize('o', 96.0);
        bitmap
    };
    assert_ne!(ink(sharp), ink(round), "grade changes the drawing of the letters");
    assert_eq!(at(Variation::NONE.with_axis(*b"GRAD", i32::MAX)), round, "a value past the axis is its maximum");
    assert_eq!(at(Variation::NONE.with_axis(*b"NOPE", 5000)), at(Variation::NONE), "an axis the face lacks is dropped");
    let inter = web::register(INTER_VAR).unwrap();
    assert_eq!(
        web::register_set_varied(&[(inter, vec![], Variation::NONE.with_axis(*b"GRAD", 100_000))]).unwrap(),
        web::register_set_varied(&[(inter, vec![], Variation::NONE)]).unwrap()
    );
}

#[test]
fn a_width_axis_condenses_the_text() {
    let wdth = axis_of(OPEN_SANS_WDTH, b"wdth").expect("the fixture has a width axis");
    assert!(wdth.min < wdth.default);
    let face = web::register(OPEN_SANS_WDTH).unwrap();
    let at = |tenths: u16| {
        web::register_set_varied(&[(face, vec![], Variation { wdth: Some(tenths), ..Variation::NONE })]).unwrap()
    };
    let (narrow, normal) = (at((wdth.min * 10.0) as u16), at((wdth.default * 10.0) as u16));
    let width = |id| measure_text_width_in("Hamburgefonstiv", 32.0, false, false, Some(id));
    assert!(width(narrow) < width(normal) * 0.9, "{} vs {}", width(narrow), width(normal));
    // Below the axis is its minimum.
    assert_eq!(at(10), narrow);
}

#[test]
fn a_request_is_clamped_to_each_axis_and_an_italic_axis_replaces_a_slant() {
    let axes = [
        web::Axis { tag: *b"wght", min: 100.0, default: 400.0, max: 900.0 },
        web::Axis { tag: *b"wdth", min: 75.0, default: 100.0, max: 125.0 },
        web::Axis { tag: *b"slnt", min: -12.0, default: 0.0, max: 0.0 },
        web::Axis { tag: *b"opsz", min: 8.0, default: 14.0, max: 144.0 },
    ];
    let r = Variation { wght: Some(2000), wdth: Some(500), slnt: Some(-300), opsz: Some(40), ital: Some(true), ..Variation::NONE }.resolve(&axes);
    assert_eq!(r, Variation { wght: Some(900), wdth: Some(750), slnt: Some(-120), opsz: Some(80), ital: None, ..Variation::NONE });
    let r = Variation { wght: Some(1), wdth: Some(10), slnt: Some(50), opsz: Some(5000), ital: None, ..Variation::NONE }.resolve(&axes);
    assert_eq!(r, Variation { wght: Some(100), wdth: Some(750), slnt: Some(0), opsz: Some(1440), ital: None, ..Variation::NONE });
    // With an `ital` axis the italic switch is used, and the slant is left alone.
    let with_ital = [web::Axis { tag: *b"ital", min: 0.0, default: 0.0, max: 1.0 }, axes[2]];
    let r = Variation { slnt: Some(-140), ital: Some(true), ..Variation::NONE }.resolve(&with_ital);
    assert_eq!(r, Variation { ital: Some(true), ..Variation::NONE });
    assert_eq!(Variation::NONE.resolve(&axes), Variation::NONE);
}

#[test]
fn a_face_can_tell_whether_it_slants_by_axis() {
    assert!(!web::slants_by_axis(web::register(INTER_VAR).unwrap()));
    assert!(!web::slants_by_axis(web::register(WOFF2).unwrap()));
}
