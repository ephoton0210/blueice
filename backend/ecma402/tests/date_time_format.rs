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
fn defaults_to_a_date_rejects_invalid_times_and_uses_iana_dst_rules() {
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
    let new_york = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            time_zone: Some("America/New_York".into()),
            hour: Some(DateTimeWidth::Numeric),
            minute: Some(DateTimeWidth::TwoDigit),
            time_zone_name: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let winter = new_york.format(1_705_320_000_000.0).unwrap();
    let summer = new_york.format(1_721_044_800_000.0).unwrap();
    assert!(winter.ends_with("EST"), "{winter}");
    assert!(summer.ends_with("EDT"), "{summer}");
    assert_eq!(new_york.time_zone(), "America/New_York");
    assert!(matches!(
        DateTimeFormat::try_new(
            &[canonicalize("en").unwrap()],
            DateTimeFormatOptions {
                time_zone: Some("No/Such_Zone".into()),
                ..Default::default()
            },
        ),
        Err(DateTimeFormatError::UnsupportedTimeZone)
    ));
}
