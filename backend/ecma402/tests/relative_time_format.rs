// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral `Intl.RelativeTimeFormat` coverage.

use blueice_ecma402::{
    canonicalize, supported_relative_time_format_locales, RelativeTimeFormat,
    RelativeTimeFormatError, RelativeTimeFormatOptions, RelativeTimeNumeric, RelativeTimePartKind,
    RelativeTimeStyle, RelativeTimeUnit,
};

#[test]
fn uses_qualitative_terms_preserves_negative_zero_and_partitions_numbers() {
    let formatter = RelativeTimeFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        RelativeTimeFormatOptions {
            numeric: RelativeTimeNumeric::Auto,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        formatter.format(-0.0, RelativeTimeUnit::Day).unwrap(),
        "today"
    );
    assert_eq!(
        formatter.format(-1.0, RelativeTimeUnit::Day).unwrap(),
        "yesterday"
    );
    assert_eq!(
        formatter.format(2.0, RelativeTimeUnit::Hour).unwrap(),
        "in 2 hours"
    );
    assert_eq!(
        formatter
            .format_to_parts(123_456.78, RelativeTimeUnit::Second)
            .unwrap(),
        vec![
            (RelativeTimePartKind::Literal, "in "),
            (RelativeTimePartKind::Integer, "123"),
            (RelativeTimePartKind::Group, ","),
            (RelativeTimePartKind::Integer, "456"),
            (RelativeTimePartKind::Decimal, "."),
            (RelativeTimePartKind::Fraction, "78"),
            (RelativeTimePartKind::Literal, " seconds"),
        ]
        .into_iter()
        .map(|(kind, value)| blueice_ecma402::RelativeTimePart {
            kind,
            value: value.into(),
        })
        .collect::<Vec<_>>()
    );
}

#[test]
fn selects_polish_patterns_and_numbering_system_overrides() {
    let polish = RelativeTimeFormat::try_new(
        &[canonicalize("pl-PL").unwrap()],
        RelativeTimeFormatOptions {
            style: RelativeTimeStyle::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        polish.format(-2.0, RelativeTimeUnit::Year).unwrap(),
        "2 lata temu"
    );

    let arab = RelativeTimeFormat::try_new(
        &[canonicalize("en-u-nu-latn").unwrap()],
        RelativeTimeFormatOptions {
            numbering_system: Some("arab".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(arab.resolved_options().locale, "en");
    assert_eq!(arab.resolved_options().numbering_system, "arab");
    assert!(arab
        .format(12.0, RelativeTimeUnit::Second)
        .unwrap()
        .contains('١'));

    let arabic_words = RelativeTimeFormat::try_new(
        &[canonicalize("ar").unwrap()],
        RelativeTimeFormatOptions::default(),
    )
    .unwrap();
    assert_eq!(
        arabic_words.format(-2.0, RelativeTimeUnit::Second).unwrap(),
        "قبل ثانيتين"
    );
    assert_eq!(
        arabic_words
            .format_to_parts(-2.0, RelativeTimeUnit::Second)
            .unwrap(),
        vec![blueice_ecma402::RelativeTimePart {
            kind: RelativeTimePartKind::Literal,
            value: "قبل ثانيتين".into(),
        }]
    );
}

#[test]
fn shares_script_parent_and_supplemental_cardinal_rules_with_other_intl_services() {
    let serbian_latin = RelativeTimeFormat::try_new(
        &[canonicalize("sr-Latn").unwrap()],
        RelativeTimeFormatOptions::default(),
    )
    .unwrap();
    let bosnian_cyrillic = RelativeTimeFormat::try_new(
        &[canonicalize("bs-Cyrl").unwrap()],
        RelativeTimeFormatOptions::default(),
    )
    .unwrap();
    // `21` selects the language-parent `one` category rather than the raw
    // provider's mandatory `other` fallback form in both script records.
    assert_eq!(
        serbian_latin.format(21.0, RelativeTimeUnit::Day).unwrap(),
        "za 21 dan"
    );
    assert_eq!(
        bosnian_cyrillic
            .format(21.0, RelativeTimeUnit::Day)
            .unwrap(),
        "за 21 дан"
    );
}

#[test]
fn parses_every_singular_and_plural_unit_spelling() {
    for (singular, plural, unit) in [
        ("second", "seconds", RelativeTimeUnit::Second),
        ("minute", "minutes", RelativeTimeUnit::Minute),
        ("hour", "hours", RelativeTimeUnit::Hour),
        ("day", "days", RelativeTimeUnit::Day),
        ("week", "weeks", RelativeTimeUnit::Week),
        ("month", "months", RelativeTimeUnit::Month),
        ("quarter", "quarters", RelativeTimeUnit::Quarter),
        ("year", "years", RelativeTimeUnit::Year),
    ] {
        assert_eq!(RelativeTimeUnit::parse(singular), Some(unit));
        assert_eq!(RelativeTimeUnit::parse(plural), Some(unit));
        assert_eq!(unit.as_str(), singular);
    }
    for invalid in ["", "secondly", "century", "Seconds"] {
        assert_eq!(RelativeTimeUnit::parse(invalid), None);
    }
}

#[test]
fn formats_every_english_width_and_qualitative_pattern() {
    let long = RelativeTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        RelativeTimeFormatOptions::default(),
    )
    .unwrap();
    for (unit, singular, plural) in [
        (RelativeTimeUnit::Second, "second", "seconds"),
        (RelativeTimeUnit::Minute, "minute", "minutes"),
        (RelativeTimeUnit::Hour, "hour", "hours"),
        (RelativeTimeUnit::Day, "day", "days"),
        (RelativeTimeUnit::Week, "week", "weeks"),
        (RelativeTimeUnit::Month, "month", "months"),
        (RelativeTimeUnit::Quarter, "quarter", "quarters"),
        (RelativeTimeUnit::Year, "year", "years"),
    ] {
        assert_eq!(long.format(1.0, unit).unwrap(), format!("in 1 {singular}"));
        assert_eq!(long.format(-2.0, unit).unwrap(), format!("2 {plural} ago"));
    }

    let short = RelativeTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        RelativeTimeFormatOptions {
            style: RelativeTimeStyle::Short,
            ..Default::default()
        },
    )
    .unwrap();
    for (unit, one, other) in [
        (RelativeTimeUnit::Second, "sec.", "sec."),
        (RelativeTimeUnit::Minute, "min.", "min."),
        (RelativeTimeUnit::Hour, "hr.", "hr."),
        (RelativeTimeUnit::Day, "day", "days"),
        (RelativeTimeUnit::Week, "wk.", "wk."),
        (RelativeTimeUnit::Month, "mo.", "mo."),
        (RelativeTimeUnit::Quarter, "qtr.", "qtrs."),
        (RelativeTimeUnit::Year, "yr.", "yr."),
    ] {
        assert_eq!(short.format(1.0, unit).unwrap(), format!("in 1 {one}"));
        assert_eq!(short.format(-2.0, unit).unwrap(), format!("2 {other} ago"));
    }

    let narrow = RelativeTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        RelativeTimeFormatOptions {
            style: RelativeTimeStyle::Narrow,
            ..Default::default()
        },
    )
    .unwrap();
    for (unit, label) in [
        (RelativeTimeUnit::Second, "s"),
        (RelativeTimeUnit::Minute, "m"),
        (RelativeTimeUnit::Hour, "h"),
        (RelativeTimeUnit::Day, "d"),
        (RelativeTimeUnit::Week, "w"),
        (RelativeTimeUnit::Month, "mo"),
        (RelativeTimeUnit::Quarter, "q"),
        (RelativeTimeUnit::Year, "y"),
    ] {
        assert_eq!(narrow.format(3.0, unit).unwrap(), format!("in 3{label}"));
    }

    let automatic = RelativeTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        RelativeTimeFormatOptions {
            numeric: RelativeTimeNumeric::Auto,
            ..Default::default()
        },
    )
    .unwrap();
    for (value, unit, expected) in [
        (0.0, RelativeTimeUnit::Second, "now"),
        (0.0, RelativeTimeUnit::Minute, "this minute"),
        (0.0, RelativeTimeUnit::Hour, "this hour"),
        (0.0, RelativeTimeUnit::Day, "today"),
        (1.0, RelativeTimeUnit::Day, "tomorrow"),
        (-1.0, RelativeTimeUnit::Day, "yesterday"),
        (0.0, RelativeTimeUnit::Week, "this week"),
        (1.0, RelativeTimeUnit::Week, "next week"),
        (-1.0, RelativeTimeUnit::Week, "last week"),
        (0.0, RelativeTimeUnit::Month, "this month"),
        (1.0, RelativeTimeUnit::Month, "next month"),
        (-1.0, RelativeTimeUnit::Month, "last month"),
        (0.0, RelativeTimeUnit::Quarter, "this quarter"),
        (1.0, RelativeTimeUnit::Quarter, "next quarter"),
        (-1.0, RelativeTimeUnit::Quarter, "last quarter"),
        (0.0, RelativeTimeUnit::Year, "this year"),
        (1.0, RelativeTimeUnit::Year, "next year"),
        (-1.0, RelativeTimeUnit::Year, "last year"),
    ] {
        assert_eq!(automatic.format(value, unit).unwrap(), expected);
    }
}

#[test]
fn covers_relative_time_locale_fallback_number_parts_and_errors() {
    let requests = [canonicalize("zz").unwrap(), canonicalize("pl").unwrap()];
    assert_eq!(
        supported_relative_time_format_locales(&requests, Default::default()),
        vec![canonicalize("pl").unwrap()]
    );
    let fallback = RelativeTimeFormat::try_new(
        &[canonicalize("zz").unwrap()],
        RelativeTimeFormatOptions {
            numbering_system: Some("unsupported".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(fallback.resolved_options().locale, "en-US");
    assert_eq!(fallback.resolved_options().numbering_system, "latn");
    assert!(fallback.bytes() > std::mem::size_of::<RelativeTimeFormat>());
    assert_eq!(
        fallback.format(f64::NAN, RelativeTimeUnit::Second),
        Err(RelativeTimeFormatError::NonFiniteNumber)
    );
    assert_eq!(
        fallback.format(f64::INFINITY, RelativeTimeUnit::Second),
        Err(RelativeTimeFormatError::NonFiniteNumber)
    );
    assert_eq!(
        RelativeTimeFormatError::DataUnavailable.to_string(),
        "relative-time data is unavailable"
    );
    assert_eq!(
        RelativeTimeFormatError::NonFiniteNumber.to_string(),
        "relative time must be finite"
    );

    let retained_extension = RelativeTimeFormat::try_new(
        &[canonicalize("en-u-nu-arab").unwrap()],
        RelativeTimeFormatOptions::default(),
    )
    .unwrap();
    assert_eq!(retained_extension.resolved_options().locale, "en-u-nu-arab");
    assert_eq!(
        retained_extension
            .format_to_parts(123_456.78, RelativeTimeUnit::Second)
            .unwrap()
            .into_iter()
            .map(|part| part.kind)
            .collect::<Vec<_>>(),
        vec![
            RelativeTimePartKind::Literal,
            RelativeTimePartKind::Integer,
            RelativeTimePartKind::Group,
            RelativeTimePartKind::Integer,
            RelativeTimePartKind::Decimal,
            RelativeTimePartKind::Fraction,
            RelativeTimePartKind::Literal,
        ]
    );

    let numeric = RelativeTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        RelativeTimeFormatOptions::default(),
    )
    .unwrap();
    // The sign bit remains observable for relative-time direction selection.
    assert_eq!(
        numeric.format(-0.0, RelativeTimeUnit::Second).unwrap(),
        "0 seconds ago"
    );
    assert_eq!(
        numeric.format(0.0, RelativeTimeUnit::Second).unwrap(),
        "in 0 seconds"
    );
}

#[test]
fn every_pinned_relative_time_locale_constructs_and_formats_every_public_cell() {
    let provider = blueice_ecma402::locale_data_provider();
    let locales = provider.number_format_resolved_locales();
    let styles = [
        RelativeTimeStyle::Long,
        RelativeTimeStyle::Short,
        RelativeTimeStyle::Narrow,
    ];
    let units = [
        RelativeTimeUnit::Second,
        RelativeTimeUnit::Minute,
        RelativeTimeUnit::Hour,
        RelativeTimeUnit::Day,
        RelativeTimeUnit::Week,
        RelativeTimeUnit::Month,
        RelativeTimeUnit::Quarter,
        RelativeTimeUnit::Year,
    ];
    let mut formatted = 0usize;

    for locale_name in locales {
        let requested = canonicalize(&locale_name).expect("pinned CLDR locale must canonicalize");
        assert_eq!(
            supported_relative_time_format_locales(
                std::slice::from_ref(&requested),
                blueice_ecma402::LocaleMatcher::Lookup,
            ),
            vec![requested.clone()],
            "{locale_name} must be advertised by RelativeTimeFormat",
        );
        for style in styles {
            let formatter = RelativeTimeFormat::try_new(
                std::slice::from_ref(&requested),
                RelativeTimeFormatOptions {
                    locale_matcher: blueice_ecma402::LocaleMatcher::Lookup,
                    style,
                    ..Default::default()
                },
            )
            .unwrap_or_else(|error| {
                panic!("RelativeTimeFormat data unavailable for {locale_name} ({style:?}): {error}")
            });
            assert_eq!(formatter.resolved_options().locale, requested.as_str());
            let automatic = RelativeTimeFormat::try_new(
                std::slice::from_ref(&requested),
                RelativeTimeFormatOptions {
                    locale_matcher: blueice_ecma402::LocaleMatcher::Lookup,
                    style,
                    numeric: RelativeTimeNumeric::Auto,
                    ..Default::default()
                },
            )
            .unwrap();
            for unit in units {
                for value in [-2.0, 2.0] {
                    let parts = formatter.format_to_parts(value, unit).unwrap_or_else(|error| {
                        panic!(
                            "RelativeTimeFormat data unavailable for {locale_name} ({style:?}, {unit:?}, {value}): {error}"
                        )
                    });
                    assert!(
                        !parts.is_empty() && parts.iter().any(|part| !part.value.is_empty()),
                        "RelativeTimeFormat produced no text for {locale_name} ({style:?}, {unit:?}, {value})"
                    );
                    assert_eq!(
                        formatter.format(value, unit).unwrap(),
                        parts.into_iter().map(|part| part.value).collect::<String>(),
                        "format and formatToParts disagree for {locale_name} ({style:?}, {unit:?}, {value})"
                    );
                    formatted += 1;
                }
                for value in [-1.0, 0.0, 1.0] {
                    assert!(
                        !automatic.format(value, unit).unwrap().is_empty(),
                        "numeric:auto produced no text for {locale_name} ({style:?}, {unit:?}, {value})"
                    );
                    formatted += 1;
                }
            }
        }
    }

    // 766 locales × 3 styles × 8 units × (two numeric directions + three
    // qualitative offsets). This is the public API coverage matrix, not a
    // merely decompression-level provider check.
    assert_eq!(formatted, 766 * 3 * 8 * 5);
}

#[test]
fn formats_every_polish_unit_style_and_plural_category() {
    let number = |value: f64| {
        if value.fract() == 0.0 {
            format!("{value:.0}")
        } else {
            value.to_string().replace('.', ",")
        }
    };
    let assert_labels = |style, labels: &[(RelativeTimeUnit, f64, &str)]| {
        let formatter = RelativeTimeFormat::try_new(
            &[canonicalize("pl").unwrap()],
            RelativeTimeFormatOptions {
                style,
                ..Default::default()
            },
        )
        .unwrap();
        for &(unit, value, label) in labels {
            assert_eq!(
                formatter.format(value, unit).unwrap(),
                format!("za {} {label}", number(value)),
                "{style:?} {unit:?} {value}"
            );
        }
    };

    assert_labels(
        RelativeTimeStyle::Long,
        &[
            (RelativeTimeUnit::Second, 1.0, "sekundę"),
            (RelativeTimeUnit::Second, 2.0, "sekundy"),
            (RelativeTimeUnit::Second, 5.0, "sekund"),
            (RelativeTimeUnit::Minute, 1.0, "minutę"),
            (RelativeTimeUnit::Minute, 2.0, "minuty"),
            (RelativeTimeUnit::Minute, 5.0, "minut"),
            (RelativeTimeUnit::Hour, 1.0, "godzinę"),
            (RelativeTimeUnit::Hour, 2.0, "godziny"),
            (RelativeTimeUnit::Hour, 5.0, "godzin"),
            (RelativeTimeUnit::Day, 1.0, "dzień"),
            (RelativeTimeUnit::Day, 2.0, "dni"),
            (RelativeTimeUnit::Day, 1.5, "dnia"),
            (RelativeTimeUnit::Week, 1.0, "tydzień"),
            (RelativeTimeUnit::Week, 2.0, "tygodnie"),
            (RelativeTimeUnit::Week, 5.0, "tygodni"),
            (RelativeTimeUnit::Week, 1.5, "tygodnia"),
            (RelativeTimeUnit::Month, 1.0, "miesiąc"),
            (RelativeTimeUnit::Month, 2.0, "miesiące"),
            (RelativeTimeUnit::Month, 5.0, "miesięcy"),
            (RelativeTimeUnit::Month, 1.5, "miesiąca"),
            (RelativeTimeUnit::Quarter, 1.0, "kwartał"),
            (RelativeTimeUnit::Quarter, 2.0, "kwartały"),
            (RelativeTimeUnit::Quarter, 5.0, "kwartałów"),
            (RelativeTimeUnit::Quarter, 1.5, "kwartału"),
            (RelativeTimeUnit::Year, 1.0, "rok"),
            (RelativeTimeUnit::Year, 2.0, "lata"),
            (RelativeTimeUnit::Year, 5.0, "lat"),
            (RelativeTimeUnit::Year, 1.5, "roku"),
        ],
    );
    assert_labels(
        RelativeTimeStyle::Short,
        &[
            (RelativeTimeUnit::Second, 2.0, "sek."),
            (RelativeTimeUnit::Minute, 2.0, "min"),
            (RelativeTimeUnit::Hour, 2.0, "godz."),
            (RelativeTimeUnit::Day, 1.0, "dzień"),
            (RelativeTimeUnit::Day, 2.0, "dni"),
            (RelativeTimeUnit::Day, 1.5, "dnia"),
            (RelativeTimeUnit::Week, 1.0, "tydz."),
            (RelativeTimeUnit::Week, 2.0, "tyg."),
            (RelativeTimeUnit::Month, 2.0, "mies."),
            (RelativeTimeUnit::Quarter, 2.0, "kw."),
            (RelativeTimeUnit::Year, 1.0, "rok"),
            (RelativeTimeUnit::Year, 2.0, "lata"),
            (RelativeTimeUnit::Year, 5.0, "lat"),
            (RelativeTimeUnit::Year, 1.5, "roku"),
        ],
    );
    assert_labels(
        RelativeTimeStyle::Narrow,
        &[
            (RelativeTimeUnit::Second, 2.0, "s"),
            (RelativeTimeUnit::Minute, 2.0, "min"),
            (RelativeTimeUnit::Hour, 2.0, "g."),
            (RelativeTimeUnit::Day, 1.0, "dzień"),
            (RelativeTimeUnit::Day, 2.0, "dni"),
            (RelativeTimeUnit::Day, 1.5, "dnia"),
            (RelativeTimeUnit::Week, 1.0, "tydz."),
            (RelativeTimeUnit::Week, 2.0, "tyg."),
            (RelativeTimeUnit::Month, 2.0, "mies."),
            (RelativeTimeUnit::Quarter, 2.0, "kw."),
            (RelativeTimeUnit::Year, 1.0, "rok"),
            (RelativeTimeUnit::Year, 2.0, "lata"),
            (RelativeTimeUnit::Year, 5.0, "lat"),
            (RelativeTimeUnit::Year, 1.5, "roku"),
        ],
    );
}
