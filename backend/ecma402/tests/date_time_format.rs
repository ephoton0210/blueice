// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ecma402::{
    bundled_tzdb_version, canonicalize, DateTimeFormat, DateTimeFormatError, DateTimeFormatOptions,
    DateTimeRangePart, DateTimeRangePartSource, DateTimeStyle, DateTimeWidth,
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

#[test]
fn pins_and_exposes_the_iana_tzdb_release() {
    assert_eq!(bundled_tzdb_version(), "2026c");
}

#[test]
fn uses_the_requested_field_skeleton_without_filling_in_extra_fields() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            weekday: Some(DateTimeWidth::Long),
            year: Some(DateTimeWidth::Numeric),
            day: Some(DateTimeWidth::TwoDigit),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let parts = format.format_to_parts(0.0).unwrap();
    assert!(parts.iter().any(|part| part.kind == "weekday"));
    assert!(parts.iter().any(|part| part.kind == "year"));
    assert!(parts.iter().any(|part| part.kind == "day"));
    assert!(!parts.iter().any(|part| part.kind == "month"));

    let time = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap()
    .format_to_parts(0.0)
    .unwrap();
    assert!(time.iter().any(|part| part.kind == "hour"));
    assert!(!time.iter().any(|part| part.kind == "minute"));
    assert!(!time.iter().any(|part| part.kind == "second"));
}

#[test]
fn collapses_ranges_in_field_order_and_repeats_cjk_endpoints() {
    let english = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Long),
            day: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            use_experimental_icu4x_range_formatter: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        english.format_range(0.0, 86_400_000.0).unwrap(),
        "January 1\u{2009}–\u{2009}2, 1970"
    );
    let english_parts = english.format_range_to_parts(0.0, 86_400_000.0).unwrap();
    assert!(english_parts
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::Shared && part.kind == "year"));
    assert!(english_parts
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::StartRange && part.kind == "day"));
    assert!(english_parts
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::EndRange && part.kind == "day"));
    let month = english_parts
        .iter()
        .position(|part| part.kind == "month")
        .unwrap();
    assert_eq!(
        english_parts[month + 1].source,
        DateTimeRangePartSource::Shared
    );

    let cross_year = english
        .format_range_to_parts(0.0, 31_536_000_000.0)
        .unwrap();
    let cross_year_months = cross_year
        .iter()
        .filter(|part| part.kind == "month")
        .map(|part| part.source)
        .collect::<Vec<_>>();
    assert_eq!(
        cross_year_months,
        vec![
            DateTimeRangePartSource::StartRange,
            DateTimeRangePartSource::EndRange,
        ]
    );

    let taiwan = DateTimeFormat::try_new(
        &[canonicalize("zh-TW").unwrap()],
        DateTimeFormatOptions {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Numeric),
            day: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            use_experimental_icu4x_range_formatter: true,
            ..Default::default()
        },
    )
    .unwrap();
    let range = taiwan.format_range(0.0, 86_400_000.0).unwrap();
    assert!(range.contains('至'), "{range}");
    let years = taiwan
        .format_range_to_parts(0.0, 86_400_000.0)
        .unwrap()
        .into_iter()
        .filter(|part| part.kind == "year")
        .count();
    assert_eq!(years, 2);

    let requested = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            weekday: Some(DateTimeWidth::Long),
            year: Some(DateTimeWidth::Numeric),
            day: Some(DateTimeWidth::TwoDigit),
            time_zone: Some("UTC".into()),
            use_experimental_icu4x_range_formatter: true,
            ..Default::default()
        },
    )
    .unwrap()
    .format_range_to_parts(0.0, 86_400_000.0)
    .unwrap();
    assert!(!requested.iter().any(|part| part.kind == "month"));

    let zoned = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Short),
            day: Some(DateTimeWidth::Numeric),
            hour: Some(DateTimeWidth::Numeric),
            minute: Some(DateTimeWidth::TwoDigit),
            time_zone: Some("America/New_York".into()),
            time_zone_name: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(zoned
        .format_range(1_705_319_200_000.0, 1_705_322_800_000.0)
        .unwrap()
        .ends_with("EST"));
}

#[test]
fn range_parts_keep_fractional_second_boundaries_and_accept_reverse_order() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            minute: Some(DateTimeWidth::Numeric),
            second: Some(DateTimeWidth::Numeric),
            fractional_second_digits: Some(1),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        format.format_to_parts(0.0).unwrap(),
        vec![
            blueice_ecma402::DateTimePart {
                kind: "minute".into(),
                value: "00".into(),
            },
            blueice_ecma402::DateTimePart {
                kind: "literal".into(),
                value: ":".into(),
            },
            blueice_ecma402::DateTimePart {
                kind: "second".into(),
                value: "00".into(),
            },
            blueice_ecma402::DateTimePart {
                kind: "literal".into(),
                value: ".".into(),
            },
            blueice_ecma402::DateTimePart {
                kind: "fractionalSecond".into(),
                value: "0".into(),
            },
        ]
    );
    assert_eq!(
        format.format_range_to_parts(0.0, 300.0).unwrap(),
        vec![
            DateTimeRangePart {
                kind: "minute".into(),
                value: "00".into(),
                source: DateTimeRangePartSource::StartRange,
            },
            DateTimeRangePart {
                kind: "literal".into(),
                value: ":".into(),
                source: DateTimeRangePartSource::StartRange,
            },
            DateTimeRangePart {
                kind: "second".into(),
                value: "00".into(),
                source: DateTimeRangePartSource::StartRange,
            },
            DateTimeRangePart {
                kind: "literal".into(),
                value: ".".into(),
                source: DateTimeRangePartSource::StartRange,
            },
            DateTimeRangePart {
                kind: "fractionalSecond".into(),
                value: "0".into(),
                source: DateTimeRangePartSource::StartRange,
            },
            DateTimeRangePart {
                kind: "literal".into(),
                value: "\u{2009}–\u{2009}".into(),
                source: DateTimeRangePartSource::Shared,
            },
            DateTimeRangePart {
                kind: "minute".into(),
                value: "00".into(),
                source: DateTimeRangePartSource::EndRange,
            },
            DateTimeRangePart {
                kind: "literal".into(),
                value: ":".into(),
                source: DateTimeRangePartSource::EndRange,
            },
            DateTimeRangePart {
                kind: "second".into(),
                value: "00".into(),
                source: DateTimeRangePartSource::EndRange,
            },
            DateTimeRangePart {
                kind: "literal".into(),
                value: ".".into(),
                source: DateTimeRangePartSource::EndRange,
            },
            DateTimeRangePart {
                kind: "fractionalSecond".into(),
                value: "3".into(),
                source: DateTimeRangePartSource::EndRange,
            },
        ]
    );
    let experimental = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            use_experimental_icu4x_range_formatter: true,
            minute: Some(DateTimeWidth::Numeric),
            second: Some(DateTimeWidth::Numeric),
            fractional_second_digits: Some(1),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        experimental.format_range_to_parts(0.0, 300.0).unwrap(),
        format.format_range_to_parts(0.0, 300.0).unwrap()
    );
    assert!(format.format_range_to_parts(300.0, 0.0).is_ok());
    assert!(format
        .format_range_to_parts(0.0, 0.9)
        .unwrap()
        .iter()
        .all(|part| part.source == DateTimeRangePartSource::Shared));
}

#[test]
fn accepts_ecmascript_time_clip_endpoints_for_utc_ranges() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        DateTimeFormatOptions {
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    const TIME_CLIP_LIMIT: f64 = 8_640_000_000_000_000.0;

    for time in [-TIME_CLIP_LIMIT, TIME_CLIP_LIMIT] {
        assert!(format.format_to_parts(time).is_ok(), "{time}");
    }
    assert!(format
        .format_range_to_parts(-TIME_CLIP_LIMIT, TIME_CLIP_LIMIT)
        .is_ok());
    assert_eq!(
        format.format_to_parts(TIME_CLIP_LIMIT + 1.0),
        Err(DateTimeFormatError::InvalidTime)
    );
}
