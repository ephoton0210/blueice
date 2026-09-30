// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! East Slavic raw CLDR NumberFormat unit-pattern regressions.

use super::*;

#[test]
fn sources_belarusian_cldr_cardinal_per_and_generic_compounds() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("be").unwrap()],
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
        "1 гігабайт"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 гігабайты"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(5.0)
            .unwrap(),
        "5 гігабайт"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.5)
            .unwrap(),
        "1,5 гігабайта"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Narrow)
            .format_f64(0.02)
            .unwrap(),
        "0,02%"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 гігабайты у секунду"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ГБ/с"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 °C/с"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 гігабайты/акр"
    );
}
