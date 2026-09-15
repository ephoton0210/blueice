// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Austronesian raw CLDR NumberFormat unit-pattern regressions.

use super::*;

#[test]
fn sources_indonesian_cldr_units_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("id").unwrap()],
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
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 derajat Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 persen"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Fahrenheit, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte per detik"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/dtk"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/dtk"
    );
}

#[test]
fn sources_malay_cldr_patterns_by_latin_and_jawi_script() {
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
        formatter("ms", NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabait"
    );
    assert_eq!(
        formatter("ms-BN", NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 darjah Celsius"
    );
    assert_eq!(
        formatter("ms", NumberFormatUnit::Degree, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2 darjah"
    );
    assert_eq!(
        formatter("ms", NumberFormatUnit::Gigabyte, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2GB"
    );
    assert_eq!(
        formatter(
            "ms-Arab",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Long
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB"
    );
    assert_eq!(
        formatter(
            "ms-Arab-BN",
            NumberFormatUnit::Celsius,
            NumberUnitDisplay::Long
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C"
    );
    assert_eq!(
        formatter(
            "ms",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabait sesaat"
    );
    assert_eq!(
        formatter(
            "ms",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/s"
    );
    assert_eq!(
        formatter(
            "ms",
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/s"
    );
    assert_eq!(
        formatter(
            "ms",
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabait per ekar"
    );
    assert_eq!(
        formatter(
            "ms-Arab",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/s"
    );
    assert_eq!(
        formatter(
            "ms-Arab",
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/ac"
    );
}

#[test]
fn sources_filipino_cldr_cardinal_and_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("fil").unwrap()],
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
            .format_f64(4.0)
            .unwrap(),
        "4 na gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Short)
            .format_f64(4.0)
            .unwrap(),
        "4 na deg"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2 GB"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(4.0)
        .unwrap(),
        "4 na gigabyte kada segundo"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB kada oras"
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
            NumberUnitDisplay::Long,
        )
        .format_f64(4.0)
        .unwrap(),
        "4 na gigabyte kada acre"
    );
}

#[test]
fn sources_javanese_cldr_patterns_separately_from_indonesian_and_malay() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("jv").unwrap()],
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
            .format_f64(2.0)
            .unwrap(),
        "2 gigabite"
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
        "2 GB"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabite saben detik"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/dtk"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/dtk"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabite saben are"
    );
}
