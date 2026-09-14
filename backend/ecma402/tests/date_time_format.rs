// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ecma402::{
    bundled_tzdb_version, canonicalize, DateTimeFormat, DateTimeFormatError, DateTimeFormatMatcher,
    DateTimeFormatOptions, DateTimeRangePart, DateTimeRangePartSource, DateTimeStyle,
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
fn formats_non_latin_numbering_systems_in_time_fields() {
    let time = 1_704_076_506_789.0;
    for (locale, no_fraction, fraction, two_digit, second) in [
        ("en-US", "2:35:06", "2:35:06.789", "02:35:06", "6"),
        ("en-US-u-nu-arab", "٢:٣٥:٠٦", "٢:٣٥:٠٦٫٧٨٩", "٠٢:٣٥:٠٦", "٦"),
        ("en-US-u-nu-deva", "२:३५:०६", "२:३५:०६.७८९", "०२:३५:०६", "६"),
        (
            "en-US-u-nu-hanidec",
            "二:三五:〇六",
            "二:三五:〇六.七八九",
            "〇二:三五:〇六 AM",
            "六",
        ),
    ] {
        let format = |options: DateTimeFormatOptions| {
            DateTimeFormat::try_new(&[canonicalize(locale).unwrap()], options)
                .unwrap()
                .format(time)
                .unwrap()
        };
        let common = DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            minute: Some(DateTimeWidth::Numeric),
            second: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            ..Default::default()
        };
        assert!(format(common.clone()).contains(no_fraction), "{locale}");
        assert!(
            format(DateTimeFormatOptions {
                fractional_second_digits: Some(3),
                ..common.clone()
            })
            .contains(fraction),
            "{locale}"
        );
        assert!(
            format(DateTimeFormatOptions {
                hour: Some(DateTimeWidth::TwoDigit),
                minute: Some(DateTimeWidth::TwoDigit),
                second: Some(DateTimeWidth::TwoDigit),
                ..common.clone()
            })
            .contains(two_digit),
            "{locale}"
        );
        assert!(
            format(DateTimeFormatOptions {
                hour: None,
                minute: None,
                second: Some(DateTimeWidth::Numeric),
                ..common
            })
            .contains(second),
            "{locale}"
        );
    }
}

#[test]
fn retains_the_requested_format_matcher_policy() {
    let basic = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            format_matcher: DateTimeFormatMatcher::Basic,
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(basic.format_matcher(), DateTimeFormatMatcher::Basic);

    let best_fit = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(best_fit.format_matcher(), DateTimeFormatMatcher::BestFit);
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
fn accepts_and_normalizes_offset_time_zones() {
    let format = |time_zone: &str| {
        DateTimeFormat::try_new(
            &[canonicalize("en-US").unwrap()],
            DateTimeFormatOptions {
                time_zone: Some(time_zone.into()),
                time_style: Some(DateTimeStyle::Short),
                ..Default::default()
            },
        )
    };
    assert_eq!(format("+0301").unwrap().time_zone(), "+03:01");
    assert_eq!(format("-00:00").unwrap().time_zone(), "+00:00");
    let parts = format("+03:01")
        .unwrap()
        .format_to_parts(819_170_696_000.0)
        .unwrap();
    assert!(parts
        .iter()
        .any(|part| part.kind == "hour" && part.value == "6"));
    assert!(parts
        .iter()
        .any(|part| part.kind == "minute" && part.value == "25"));
    for invalid in [
        "+3",
        "+24",
        "+23:0",
        "+130",
        "+15:59:00",
        "-1:10",
        "\u{2212}0900",
    ] {
        assert!(matches!(
            format(invalid),
            Err(DateTimeFormatError::UnsupportedTimeZone)
        ));
    }
}

#[test]
fn case_normalizes_iana_names_without_resolving_links() {
    let format = |time_zone: &str| {
        DateTimeFormat::try_new(
            &[canonicalize("en").unwrap()],
            DateTimeFormatOptions {
                time_zone: Some(time_zone.into()),
                ..Default::default()
            },
        )
        .unwrap()
    };
    assert_eq!(format("america/new_york").time_zone(), "America/New_York");
    assert_eq!(format("asia/calcutta").time_zone(), "Asia/Calcutta");
    assert_eq!(format("Asia/Kolkata").time_zone(), "Asia/Kolkata");
    assert_eq!(format("etc/gmt").time_zone(), "Etc/GMT");
    assert_eq!(format("gmt").time_zone(), "GMT");
}

#[test]
fn time_zone_name_keeps_the_default_numeric_date_fields() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            time_zone: Some("UTC".into()),
            time_zone_name: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let parts = format.format_to_parts(0.0).unwrap();
    for field in ["month", "day", "year", "timeZoneName"] {
        assert!(parts.iter().any(|part| part.kind == field), "{field}");
    }
}

#[test]
fn named_zones_format_time_clip_endpoints_outside_jiff_civil_range() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            time_zone: Some("America/New_York".into()),
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Numeric),
            day: Some(DateTimeWidth::Numeric),
            time_zone_name: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!format.format(8_640_000_000_000_000.0).unwrap().is_empty());
    assert!(!format.format(-8_640_000_000_000_000.0).unwrap().is_empty());
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
fn cldr_range_patterns_own_their_source_spans() {
    let english = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Long),
            day: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        english.format_range(0.0, 86_400_000.0).unwrap(),
        "January 1\u{2009}–\u{2009}2, 1970"
    );
    let english_parts = english.format_range_to_parts(0.0, 86_400_000.0).unwrap();
    assert_eq!(
        english_parts
            .iter()
            .map(|part| (&part.kind[..], &part.value[..], part.source))
            .collect::<Vec<_>>(),
        vec![
            ("month", "January", DateTimeRangePartSource::Shared),
            ("literal", " ", DateTimeRangePartSource::Shared),
            ("day", "1", DateTimeRangePartSource::StartRange),
            (
                "literal",
                "\u{2009}–\u{2009}",
                DateTimeRangePartSource::Shared,
            ),
            ("day", "2", DateTimeRangePartSource::EndRange),
            ("literal", ", ", DateTimeRangePartSource::Shared),
            ("year", "1970", DateTimeRangePartSource::Shared),
        ]
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
            use_icu4x_range_formatter: true,
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
