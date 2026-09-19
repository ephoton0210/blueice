// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bosnian raw CLDR NumberFormat unit-pattern regressions.

use super::*;

fn formatter(locale: &str, unit: NumberFormatUnit, display: NumberUnitDisplay) -> NumberFormat {
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
}

#[test]
fn sources_bosnian_latin_cldr_plural_per_and_generic_compounds() {
    assert_eq!(
        formatter("bs", NumberFormatUnit::Acre, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 katastarsko jutro"
    );
    assert_eq!(
        formatter("bs-Latn", NumberFormatUnit::Acre, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 katastarska jutra"
    );
    assert_eq!(
        formatter("bs", NumberFormatUnit::Acre, NumberUnitDisplay::Long)
            .format_f64(5.0)
            .unwrap(),
        "5 katastarskih jutara"
    );
    assert_eq!(
        formatter(
            "bs",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabajta po sekundi"
    );
    assert_eq!(
        formatter(
            "bs",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/s"
    );
    assert_eq!(
        formatter(
            "bs-Latn",
            NumberFormatUnit::parse("celsius-per-liter").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°/l"
    );
    assert_eq!(
        formatter(
            "bs",
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabajta/katastarsko jutro"
    );
}

#[test]
fn sources_bosnian_cyrillic_cldr_patterns_separately_from_latin() {
    assert_eq!(
        formatter("bs-Cyrl", NumberFormatUnit::Acre, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 акре"
    );
    assert_eq!(
        formatter(
            "bs-Cyrl",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB у секунди"
    );
    assert_eq!(
        formatter(
            "bs-Cyrl",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB у сек."
    );
    assert_eq!(
        formatter(
            "bs-Cyrl",
            NumberFormatUnit::parse("celsius-per-liter").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/l"
    );
    assert_eq!(
        formatter(
            "bs-Cyrl",
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/акра"
    );
}
