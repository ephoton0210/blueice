// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Gujarati raw CLDR NumberFormat unit-pattern regressions.

use super::*;

#[test]
fn sources_gujarati_cldr_units_per_patterns_and_generic_compounds() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("gu").unwrap()],
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
        "2 ગીગાબાઇટ"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Byte, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2 બાઇટ"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ગીગાબાઇટ પ્રતિ સેકંડ"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB પ્રતિ કલાક"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-liter").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/લિ"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ગીગાબાઇટ પ્રતિ એકર"
    );
}
