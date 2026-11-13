// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static PARSED_FONTS: AtomicU32 = AtomicU32::new(0);

pub(super) fn record_parsed_font(bytes: &[u8]) {
    let mut fonts = vec![
        super::FONT_REGULAR,
        super::FONT_BOLD,
        super::FONT_ITALIC,
        super::FONT_BOLD_ITALIC,
        super::FONT_CJK_FALLBACK,
    ];
    fonts.extend(super::SCRIPT_FACES.iter().map(|face| face.bytes));
    let index = fonts
        .iter()
        .position(|font| *font == bytes)
        .expect("every bundled parse has a known face");
    PARSED_FONTS.fetch_or(1 << index, Ordering::SeqCst);
}

fn cold_process(test: &str) -> bool {
    if std::env::var("BLUEICE_FONT_COLD_TEST").as_deref() == Ok(test) {
        return true;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("BLUEICE_FONT_COLD_TEST", test)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cold font process failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

#[test]
fn latin_measurement_does_not_parse_unused_bundled_faces() {
    if !cold_process("cold_start::latin_measurement_does_not_parse_unused_bundled_faces") {
        return;
    }
    assert!(super::measure_text_width("first", 16.0, false, false) > 0.0);
    assert_eq!(PARSED_FONTS.load(Ordering::SeqCst), 1);
    let primary = super::font_for(false, false);
    assert!(std::ptr::eq(
        primary,
        super::font_for_char('a', false, false)
    ));
    assert_eq!(PARSED_FONTS.load(Ordering::SeqCst), 1);
}

#[test]
fn cjk_measurement_parses_only_its_primary_and_fallback_faces() {
    if !cold_process("cold_start::cjk_measurement_parses_only_its_primary_and_fallback_faces") {
        return;
    }
    assert!(super::measure_text_width("a關", 16.0, false, false) > 0.0);
    assert_eq!(PARSED_FONTS.load(Ordering::SeqCst), 0b10001);
    let fallback = super::font_for_char('關', false, false);
    assert!(fallback.has_glyph('關'));
    assert!(std::ptr::eq(
        fallback,
        super::font_for_char('關', false, false)
    ));
    assert_eq!(PARSED_FONTS.load(Ordering::SeqCst), 0b10001);
}

#[test]
fn a_script_measurement_parses_only_dejavu_and_that_scripts_own_face() {
    if !cold_process("cold_start::a_script_measurement_parses_only_dejavu_and_that_scripts_own_face") {
        return;
    }
    // Devanagari is the first script face, after the five Latin and CJK ones
    assert!(super::measure_text_width("\u{928}\u{92E}\u{938}\u{94D}\u{924}\u{947}", 16.0, false, false) > 0.0);
    assert_eq!(PARSED_FONTS.load(Ordering::SeqCst), 1 | (1 << 5), "DejaVu (to find it lacks the letters) and Devanagari");
}
