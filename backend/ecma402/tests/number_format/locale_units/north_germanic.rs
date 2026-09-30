// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! North Germanic raw CLDR NumberFormat unit-pattern regressions.

use super::*;

#[test]
fn sources_swedish_cldr_cardinal_forms_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("sv").unwrap()],
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
            .format_f64(1.0)
            .unwrap(),
        "1 grad Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 grader Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2 %"
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
        "2 gigabyte per sekund"
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

    let finland = NumberFormat::try_new(
        &[canonicalize("sv-FI").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("gigabyte-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(finland.format_f64(2.0).unwrap(), "2 gigabyte per sekund");
}
