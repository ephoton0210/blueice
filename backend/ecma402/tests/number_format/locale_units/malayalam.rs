// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Malayalam raw CLDR NumberFormat unit-pattern regressions.

use super::*;

#[test]
fn sources_malayalam_cldr_units_per_patterns_and_generic_compounds() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("ml").unwrap()],
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
        "2 ഗിഗാബൈറ്റ്"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°സെ"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Byte, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2B"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ഗിഗാബൈറ്റ് / സെക്കൻഡ്"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/മ."
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-liter").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°സെ/ലി."
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ഗിഗാബൈറ്റ്/ഏക്കർ"
    );
}
