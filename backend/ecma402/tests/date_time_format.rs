// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ecma402::{
    basic_format_matcher, bundled_tzdb_version, canonicalize, DateTimeFormat, DateTimeFormatError,
    DateTimeFormatInput, DateTimeFormatMatcher, DateTimeFormatOptions, DateTimeFormatRecord,
    DateTimeRangePart, DateTimeRangePartSource, DateTimeStyle, DateTimeWidth,
    SUPPORTED_NUMBERING_SYSTEMS,
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
fn typed_inputs_share_single_and_range_temporal_semantics() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let temporal_options = DateTimeFormatOptions {
        time_zone: Some("UTC".into()),
        year: Some(DateTimeWidth::Numeric),
        month: Some(DateTimeWidth::Numeric),
        day: Some(DateTimeWidth::Numeric),
        hour: Some(DateTimeWidth::Numeric),
        minute: Some(DateTimeWidth::Numeric),
        second: Some(DateTimeWidth::Numeric),
        ..Default::default()
    };
    let start = DateTimeFormatInput::TemporalInstant {
        epoch_milliseconds: 1_577_923_200_000.0,
        options: temporal_options.clone(),
    };
    let end = DateTimeFormatInput::TemporalInstant {
        epoch_milliseconds: 1_577_926_800_000.0,
        options: temporal_options,
    };
    let single = format.format_input_to_parts(start.clone()).unwrap();
    assert!(single.iter().any(|part| part.kind == "hour"));
    let range = format.format_range_inputs_to_parts(start, end).unwrap();
    assert!(range.iter().any(|part| part.kind == "hour"));
    assert!(range
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::StartRange));
    assert_eq!(
        format.format_range_inputs_to_parts(
            DateTimeFormatInput::EpochMilliseconds(0.0),
            DateTimeFormatInput::TemporalPlain {
                local_epoch_milliseconds: 0,
                options: DateTimeFormatOptions::default(),
            },
        ),
        Err(DateTimeFormatError::IncompatibleRangeInputs)
    );
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
fn resolves_every_advertised_numbering_system() {
    for numbering_system in SUPPORTED_NUMBERING_SYSTEMS {
        let format = DateTimeFormat::try_new(
            &[canonicalize("en").unwrap()],
            DateTimeFormatOptions {
                numbering_system: Some((*numbering_system).into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(format.numbering_system(), *numbering_system);
    }
}

#[test]
fn resolves_deprecated_islamic_calendars_to_the_advertised_fallback() {
    for calendar in ["islamic", "islamic-rgsa", "islamic-civil"] {
        let formatter = DateTimeFormat::try_new(
            &[canonicalize("en-US").unwrap()],
            DateTimeFormatOptions {
                calendar: Some(calendar.into()),
                time_zone: Some("UTC".into()),
                year: Some(DateTimeWidth::Numeric),
                month: Some(DateTimeWidth::Numeric),
                day: Some(DateTimeWidth::Numeric),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("could not construct {calendar}: {error}"));
        assert_eq!(formatter.calendar(), "islamic-civil");
        assert!(!formatter.format(0.0).unwrap().is_empty(), "{calendar}");
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
fn basic_format_matcher_uses_the_ecma402_penalties_and_stable_ties() {
    let options = DateTimeFormatOptions {
        year: Some(DateTimeWidth::Numeric),
        month: Some(DateTimeWidth::Long),
        fractional_second_digits: Some(3),
        time_zone_name: Some("short".into()),
        ..Default::default()
    };
    let formats = [
        // Missing a requested field is far worse than an added field.
        DateTimeFormatRecord {
            month: Some(DateTimeWidth::Long),
            ..Default::default()
        },
        // The exact fractional precision and the short-to-offset conversion
        // beat a shorter month plus a coarser fractional precision.
        DateTimeFormatRecord {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Short),
            fractional_second_digits: Some(3),
            time_zone_name: Some("shortOffset".into()),
            ..Default::default()
        },
        DateTimeFormatRecord {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Long),
            fractional_second_digits: Some(2),
            time_zone_name: Some("long".into()),
            ..Default::default()
        },
    ];
    assert_eq!(basic_format_matcher(&options, &formats), Some(1));

    let tied = [
        DateTimeFormatRecord {
            year: Some(DateTimeWidth::Numeric),
            ..Default::default()
        },
        DateTimeFormatRecord {
            year: Some(DateTimeWidth::Numeric),
            ..Default::default()
        },
    ];
    assert_eq!(
        basic_format_matcher(
            &DateTimeFormatOptions {
                year: Some(DateTimeWidth::Numeric),
                ..Default::default()
            },
            &tied,
        ),
        Some(0)
    );
}

#[test]
fn every_pinned_datetime_locale_constructs_and_formats_from_provider_data() {
    let provider = blueice_ecma402::locale_data_provider();
    let locales = provider.number_format_resolved_locales();
    let mut formatted = 0usize;

    for locale_name in locales {
        let locale = canonicalize(&locale_name).expect("pinned CLDR locale must canonicalize");
        assert!(
            provider.supports_service_locale(
                blueice_ecma402::IntlService::DateTimeFormat,
                locale.locale()
            ),
            "DateTimeFormat must advertise its complete provider locale {locale_name}"
        );
        let formatter = DateTimeFormat::try_new(
            std::slice::from_ref(&locale),
            DateTimeFormatOptions {
                format_matcher: DateTimeFormatMatcher::Basic,
                time_zone: Some("UTC".into()),
                year: Some(DateTimeWidth::Numeric),
                month: Some(DateTimeWidth::Short),
                day: Some(DateTimeWidth::Numeric),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| {
            panic!("DateTimeFormat could not construct {locale_name}: {error}")
        });
        assert!(
            !formatter.format(0.0).unwrap().is_empty(),
            "DateTimeFormat produced no localized text for {locale_name}"
        );
        formatted += 1;
    }

    assert_eq!(formatted, 766);
}

#[test]
fn every_pinned_datetime_locale_retains_missing_basic_components_via_append_items() {
    let provider = blueice_ecma402::locale_data_provider();
    let options = DateTimeFormatOptions {
        format_matcher: DateTimeFormatMatcher::Basic,
        time_zone: Some("UTC".into()),
        weekday: Some(DateTimeWidth::Long),
        era: Some(DateTimeWidth::Short),
        year: Some(DateTimeWidth::Numeric),
        month: Some(DateTimeWidth::Long),
        day: Some(DateTimeWidth::TwoDigit),
        hour: Some(DateTimeWidth::TwoDigit),
        minute: Some(DateTimeWidth::TwoDigit),
        second: Some(DateTimeWidth::TwoDigit),
        fractional_second_digits: Some(3),
        time_zone_name: Some("short".into()),
        ..Default::default()
    };
    for locale_name in provider.number_format_resolved_locales() {
        let locale = canonicalize(&locale_name).expect("pinned CLDR locale must canonicalize");
        let formatter = DateTimeFormat::try_new(std::slice::from_ref(&locale), options.clone())
            .unwrap_or_else(|error| {
                panic!("DateTimeFormat could not construct {locale_name}: {error}")
            });
        // ICU4X's selected raw skeleton may be missing one or more of these
        // fields. The pinned appendItems pattern must retain each requested
        // component in the actual public typed output, not merely in the
        // formatter's stored option record.
        assert!(formatter.options().weekday.is_some(), "{locale_name}");
        assert!(formatter.options().era.is_some(), "{locale_name}");
        assert!(formatter.options().year.is_some(), "{locale_name}");
        assert!(formatter.options().month.is_some(), "{locale_name}");
        assert!(formatter.options().day.is_some(), "{locale_name}");
        assert!(formatter.options().hour.is_some(), "{locale_name}");
        assert!(formatter.options().minute.is_some(), "{locale_name}");
        assert!(formatter.options().second.is_some(), "{locale_name}");
        assert!(
            formatter.options().fractional_second_digits.is_some(),
            "{locale_name}"
        );
        assert!(
            formatter.options().time_zone_name.is_some(),
            "{locale_name}"
        );
        let parts = formatter.format_to_parts(0.0).unwrap_or_else(|error| {
            panic!("DateTimeFormat could not format {locale_name}: {error}")
        });
        for kind in [
            "weekday",
            "era",
            "year",
            "month",
            "day",
            "hour",
            "minute",
            "second",
            "fractionalSecond",
            "timeZoneName",
        ] {
            assert!(
                parts.iter().any(|part| part.kind == kind),
                "DateTimeFormat dropped {kind} while appending {locale_name}"
            );
        }
    }
}

#[test]
fn basic_matcher_renders_appended_components_with_cldr_literals() {
    let formatter = DateTimeFormat::try_new(
        &[canonicalize("de-CH").unwrap()],
        DateTimeFormatOptions {
            format_matcher: DateTimeFormatMatcher::Basic,
            time_zone: Some("UTC".into()),
            weekday: Some(DateTimeWidth::Long),
            era: Some(DateTimeWidth::Short),
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Long),
            day: Some(DateTimeWidth::TwoDigit),
            hour: Some(DateTimeWidth::TwoDigit),
            minute: Some(DateTimeWidth::TwoDigit),
            second: Some(DateTimeWidth::TwoDigit),
            fractional_second_digits: Some(3),
            time_zone_name: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let parts = formatter.format_to_parts(0.0).unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|part| part.value.as_str())
            .collect::<String>(),
        "Donnerstag, 01. Januar 1970 n. Chr. (Stunde: 00:00:00.000 UTC)"
    );
    assert!(parts
        .iter()
        .any(|part| part.kind == "literal" && part.value == " (Stunde: "));
    for kind in [
        "weekday",
        "era",
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "fractionalSecond",
        "timeZoneName",
    ] {
        assert!(parts.iter().any(|part| part.kind == kind), "missing {kind}");
    }

    // Ranges deliberately continue through ICU4X's complete dynamic
    // interval skeleton rather than trying to render an incomplete raw
    // availableFormats record. That path must retain every Basic request and
    // avoid the raw renderer's literal `Y` failure as well.
    let range = formatter.format_range_to_parts(0.0, 86_400_000.0).unwrap();
    assert!(
        !range.iter().any(|part| part.value.contains('Y')),
        "{range:?}"
    );
    for kind in [
        "weekday",
        "era",
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "fractionalSecond",
        "timeZoneName",
    ] {
        assert!(
            range.iter().any(|part| part.kind == kind),
            "range missing {kind}: {range:?}"
        );
    }
    assert!(formatter.bytes() > std::mem::size_of::<DateTimeFormat>());
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
    assert!(format.format_range_to_parts(300.0, 0.0).is_ok());
    assert!(format
        .format_range_to_parts(0.0, 0.9)
        .unwrap()
        .iter()
        .all(|part| part.source == DateTimeRangePartSource::Shared));
}

#[test]
fn cldr_ranges_cover_time_zone_and_non_gregorian_skeletons() {
    let time_only = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            minute: Some(DateTimeWidth::TwoDigit),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let time_parts = time_only.format_range_to_parts(0.0, 7_200_000.0).unwrap();
    assert!(time_parts.iter().any(|part| part.kind == "hour"));
    assert!(time_parts
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::StartRange));
    assert!(time_parts
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::EndRange));

    let zoned = DateTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            minute: Some(DateTimeWidth::TwoDigit),
            time_zone: Some("America/New_York".into()),
            time_zone_name: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let zone_parts = zoned
        .format_range_to_parts(1_710_052_200_000.0, 1_710_055_800_000.0)
        .unwrap();
    assert!(zone_parts.iter().any(|part| {
        part.kind == "timeZoneName"
            && part.value == "EST"
            && part.source == DateTimeRangePartSource::StartRange
    }));
    assert!(zone_parts.iter().any(|part| {
        part.kind == "timeZoneName"
            && part.value == "EDT"
            && part.source == DateTimeRangePartSource::EndRange
    }));

    let buddhist = DateTimeFormat::try_new(
        &[canonicalize("th-TH-u-ca-buddhist").unwrap()],
        DateTimeFormatOptions {
            year: Some(DateTimeWidth::Numeric),
            month: Some(DateTimeWidth::Short),
            day: Some(DateTimeWidth::Numeric),
            time_zone: Some("UTC".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let buddhist_parts = buddhist.format_range_to_parts(0.0, 86_400_000.0).unwrap();
    assert!(buddhist_parts.iter().any(|part| part.kind == "year"));
    assert!(buddhist_parts
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::StartRange));
    assert!(buddhist_parts
        .iter()
        .any(|part| part.source == DateTimeRangePartSource::EndRange));
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

/// `hourCycle: "h24"`/`"h11"` at midnight (epoch 0). `h23`/`h12` are the
/// control cases: real `Intl.DateTimeFormat` renders midnight as `"00"` for
/// `h23` (0-23, zero-padded), `"12"` for `h12` (1-12), `"24"` for `h24`
/// (1-24 — the *same* range as `h23` except midnight reads 24, not 0), and
/// un-padded `"0"` for `h11` (0-11, the same range as `h12` shifted down by
/// one). Pinned by Test262's `intl402/Temporal/{Instant,PlainTime}/prototype/
/// toLocaleString/hourcycle.js`.
#[test]
fn h24_and_h11_hour_cycles_render_midnight_correctly() {
    fn format_with_hour_cycle(hour_cycle: &str) -> String {
        DateTimeFormat::try_new(
            &[canonicalize("en").unwrap()],
            DateTimeFormatOptions {
                hour: Some(DateTimeWidth::Numeric),
                minute: Some(DateTimeWidth::Numeric),
                second: Some(DateTimeWidth::Numeric),
                hour_cycle: Some(hour_cycle.into()),
                time_zone: Some("UTC".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .format(0.0)
        .unwrap()
    }

    let h23 = format_with_hour_cycle("h23");
    assert!(h23.contains("00:00:00"), "h23 midnight: {h23:?}");

    let h12 = format_with_hour_cycle("h12");
    assert!(h12.contains("12:00:00"), "h12 midnight: {h12:?}");

    let h24 = format_with_hour_cycle("h24");
    assert!(h24.contains("24:00:00"), "h24 midnight: {h24:?}");

    let h11 = format_with_hour_cycle("h11");
    assert!(h11.contains("0:00:00"), "h11 midnight: {h11:?}");
    assert!(!h11.contains("12:00:00"), "h11 midnight: {h11:?}");
}

/// With no component options, `formatMatcher: "basic"` must match the
/// defaulted numeric year/month/day (ECMA-402 applies those defaults before
/// the matcher runs) rather than an empty request, which scores best against
/// the single-field `d` record and used to render only the day.
#[test]
fn basic_matcher_matches_the_defaulted_numeric_date() {
    let format = |locale: &str, format_matcher, era| {
        DateTimeFormat::try_new(
            &[canonicalize(locale).unwrap()],
            DateTimeFormatOptions {
                format_matcher,
                era,
                time_zone: Some("UTC".into()),
                ..Default::default()
            },
        )
        .unwrap()
    };
    for locale in ["de", "fr", "en-US", "ja", "ru"] {
        let basic = format(locale, DateTimeFormatMatcher::Basic, None);
        let best_fit = format(locale, DateTimeFormatMatcher::BestFit, None);
        assert_eq!(
            basic.format(86_400_000.0).unwrap(),
            best_fit.format(86_400_000.0).unwrap(),
            "{locale}"
        );
        let kinds = basic
            .format_to_parts(86_400_000.0)
            .unwrap()
            .into_iter()
            .map(|part| part.kind)
            .filter(|kind| kind != "literal")
            .collect::<Vec<_>>();
        assert_eq!(kinds.len(), 3, "{locale}: {kinds:?}");
        for kind in ["year", "month", "day"] {
            assert!(kinds.iter().any(|actual| actual == kind), "{locale}");
        }
        // `era` is additive: the defaulted date is still matched, with the era.
        let with_era = format(
            locale,
            DateTimeFormatMatcher::Basic,
            Some(DateTimeWidth::Short),
        )
        .format_to_parts(86_400_000.0)
        .unwrap();
        for kind in ["year", "month", "day", "era"] {
            assert!(with_era.iter().any(|part| part.kind == kind), "{locale}");
        }
    }
    // The options a Basic formatter reports are the caller's, so Temporal
    // values can still tell the defaults apart from explicit components.
    assert_eq!(
        format("de", DateTimeFormatMatcher::Basic, None)
            .options()
            .day,
        None
    );
}
