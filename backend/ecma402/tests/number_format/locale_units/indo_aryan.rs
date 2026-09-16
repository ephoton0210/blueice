// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Indo-Aryan raw CLDR NumberFormat unit-pattern regressions.

use super::*;

#[test]
fn sources_urdu_cldr_bidi_plural_and_per_patterns() {
    let formatter = |locale: &str, unit, display| {
        NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter("ur", NumberFormatUnit::Gigabit, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 گیگابٹ"
    );
    assert_eq!(
        formatter("ur", NumberFormatUnit::Gigabit, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 گیگابٹس"
    );
    assert_eq!(
        formatter("ur-IN", NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "۲\u{200e}°C"
    );
    assert_eq!(
        formatter("ur", NumberFormatUnit::Byte, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2B"
    );
    assert_eq!(
        formatter(
            "ur",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 گیگابائٹ فی سیکنڈ"
    );
    assert_eq!(
        formatter(
            "ur",
            NumberFormatUnit::parse("gigabyte-per-year").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "فی سال 2 گیگابائٹ"
    );
    assert_eq!(
        formatter(
            "ur",
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2\u{200e}° فی سیکنڈ"
    );
    assert_eq!(
        formatter(
            "ur",
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 گیگابائٹ فی ایکڑ"
    );
}

#[test]
fn sources_transliterated_hindi_without_using_devanagari_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("hi-Latn").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
    };

    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabytes"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2GB"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabytes har second"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/ac"
    );
}
