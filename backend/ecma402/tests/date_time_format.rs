// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ecma402::{
    canonicalize, DateTimeFormat, DateTimeFormatError, DateTimeFormatOptions, DateTimeStyle,
    DateTimeWidth,
};

#[test]
fn formats_a_utc_epoch_with_localized_parts() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Long),
            day: Some(DateTimeWidth::Numeric),
            hour: Some(DateTimeWidth::Numeric),
            minute: Some(DateTimeWidth::TwoDigit),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let parts = format.format_to_parts(0.0).unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|part| part.value.as_str())
            .collect::<String>(),
        format.format(0.0).unwrap()
    );
    assert!(parts.iter().any(|part| part.kind == "year"));
    assert!(parts.iter().any(|part| part.kind == "month"));
    assert!(parts.iter().any(|part| part.kind == "day"));
    assert_eq!(format.time_zone(), "UTC");
}

#[test]
fn defaults_to_a_date_and_rejects_invalid_times_and_zones() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("de-DE").unwrap()],
        DateTimeFormatOptions {
            date_style: Some(DateTimeStyle::Short),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!format.format(0.0).unwrap().is_empty());
    assert_eq!(
        format.format(f64::NAN),
        Err(DateTimeFormatError::InvalidTime)
    );
    assert!(matches!(
        DateTimeFormat::try_new(
            &[canonicalize("en").unwrap()],
            DateTimeFormatOptions {
                time_zone: Some("America/New_York".into()),
                ..Default::default()
            },
        ),
        Err(DateTimeFormatError::UnsupportedTimeZone)
    ));
}
