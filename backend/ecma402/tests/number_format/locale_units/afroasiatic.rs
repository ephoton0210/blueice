// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Afroasiatic raw CLDR NumberFormat unit-pattern regressions.

use super::*;

#[test]
fn sources_amharic_cldr_script_plural_and_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("am").unwrap()],
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
        "2 ጊባ"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Petabyte, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 ፔታ ባይት"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Petabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 ፔታ ባይቶች"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°ሴ"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Narrow)
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
        "2 ጊባ/ሰከ"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°/ሰከ"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-acre").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ጊባ በኤክር"
    );
}
