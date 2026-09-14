// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral decimal `Intl.NumberFormat` coverage.

use blueice_ecma402::{
    canonicalize, resolve_number_format_locale, supported_number_format_locales, unicode_keyword,
    NumberCurrencyDisplay, NumberCurrencyOptions, NumberCurrencySign, NumberFormat,
    NumberFormatError, NumberFormatOptions, NumberFormatPartKind, NumberFormatStyle,
    NumberFormatUnit, NumberGrouping, NumberNotation, NumberRoundingMode, NumberSignDisplay,
    NumberUnitDisplay, ResolvedNumberFormatOptions,
};

#[test]
fn localizes_decimal_digits_separators_and_half_expand_rounding() {
    let german = canonicalize("de-DE").unwrap();
    let formatter = NumberFormat::try_new(
        &[german],
        NumberFormatOptions {
            maximum_fraction_digits: Some(2),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        formatter.format_decimal("1234567.895").unwrap(),
        "1.234.567,9"
    );
    assert_eq!(formatter.format_decimal("1.2345").unwrap(), "1,23");
    assert_eq!(
        formatter.resolved_options(),
        &ResolvedNumberFormatOptions {
            locale: "de-DE".into(),
            numbering_system: "latn".into(),
            use_grouping: NumberGrouping::Auto,
            style: NumberFormatStyle::Decimal,
            notation: NumberNotation::Standard,
            unit: None,
            unit_display: NumberUnitDisplay::Short,
            minimum_integer_digits: 1,
            minimum_fraction_digits: 0,
            maximum_fraction_digits: 2,
            rounding_mode: NumberRoundingMode::HalfExpand,
            sign_display: NumberSignDisplay::Auto,
        }
    );
}

#[test]
fn honors_unicode_numbering_systems_and_fraction_padding() {
    let thai = canonicalize("th-u-nu-thai").unwrap();
    assert_eq!(thai.as_str(), "th-u-nu-thai");
    assert_eq!(
        unicode_keyword(thai.locale(), "nu").as_deref(),
        Some("thai")
    );
    let formatter = NumberFormat::try_new(
        &[thai],
        NumberFormatOptions {
            minimum_fraction_digits: Some(2),
            maximum_fraction_digits: Some(2),
            use_grouping: NumberGrouping::Never,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(formatter.resolved_options().numbering_system, "thai");
    assert_eq!(
        formatter.resolved_options().use_grouping,
        NumberGrouping::Never
    );
    assert_eq!(formatter.format_decimal("1007.5").unwrap(), "๑๐๐๗.๕๐");
}

#[test]
fn rounds_with_all_host_neutral_rounding_increment_representations() {
    let english = canonicalize("en").unwrap();
    let options = NumberFormatOptions {
        minimum_fraction_digits: Some(2),
        maximum_fraction_digits: Some(2),
        ..Default::default()
    };
    let formatter = NumberFormat::try_new_with_rounding_increment(&[english], options, 25).unwrap();
    assert_eq!(formatter.rounding_increment(), 25);
    assert_eq!(formatter.format_decimal("1.1125").unwrap(), "1.00");
    assert_eq!(formatter.format_decimal("1.125").unwrap(), "1.25");

    let whole = NumberFormat::try_new_with_rounding_increment(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            minimum_fraction_digits: Some(0),
            maximum_fraction_digits: Some(0),
            ..Default::default()
        },
        2500,
    )
    .unwrap();
    assert_eq!(whole.format_decimal("1249").unwrap(), "0");
    assert_eq!(whole.format_decimal("1250").unwrap(), "2,500");
    assert!(matches!(
        NumberFormat::try_new_with_rounding_increment(
            &[canonicalize("en").unwrap()],
            Default::default(),
            3,
        ),
        Err(NumberFormatError::InvalidRoundingIncrement)
    ));

    let precision = NumberFormat::try_new_with_precision(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            use_grouping: NumberGrouping::Never,
            rounding_mode: NumberRoundingMode::Ceil,
            ..Default::default()
        },
        1,
        Some(1),
        Some(2),
    )
    .unwrap();
    assert_eq!(precision.significant_digits(), Some((1, 2)));
    assert_eq!(precision.format_decimal("1.101").unwrap(), "1.2");
    assert_eq!(precision.format_decimal("-1.1999").unwrap(), "-1.1");

    let trailing = NumberFormat::try_new_with_digit_options(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            minimum_fraction_digits: Some(2),
            maximum_fraction_digits: Some(2),
            ..Default::default()
        },
        1,
        None,
        None,
        blueice_ecma402::NumberTrailingZeroDisplay::StripIfInteger,
    )
    .unwrap();
    assert_eq!(trailing.format_decimal("1").unwrap(), "1");
    assert_eq!(trailing.format_decimal("1.5").unwrap(), "1.50");
}

#[test]
fn negotiates_against_decimal_data_and_exposes_the_trace() {
    let unavailable = canonicalize("zz").unwrap();
    let bengali = canonicalize("bn").unwrap();
    let formatter = NumberFormat::try_new(&[unavailable, bengali], Default::default()).unwrap();

    assert_eq!(formatter.format_decimal("1000007").unwrap(), "১০,০০,০০৭");
    assert_eq!(formatter.negotiation().selected().as_str(), "bn");
    assert!(!formatter.negotiation().used_default());
    assert_eq!(
        formatter
            .negotiation()
            .candidates()
            .iter()
            .map(|candidate| candidate.is_supported())
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn falls_back_to_the_stable_decimal_default_when_nothing_is_supported() {
    let unavailable = canonicalize("zz").unwrap();
    let formatter = NumberFormat::try_new(&[unavailable], Default::default()).unwrap();

    assert_eq!(formatter.negotiation().selected().as_str(), "en-US");
    assert!(formatter.negotiation().used_default());
    assert_eq!(formatter.format_decimal("1000").unwrap(), "1,000");
}

#[test]
fn applies_min2_grouping_and_finite_ieee754_boundaries() {
    let english = canonicalize("en-US").unwrap();
    let formatter = NumberFormat::try_new(
        &[english],
        NumberFormatOptions {
            use_grouping: NumberGrouping::Min2,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(formatter.format_f64(-0.0).unwrap(), "-0");
    assert_eq!(formatter.format_decimal("1000").unwrap(), "1000");
    assert_eq!(formatter.format_decimal("10000").unwrap(), "10,000");
    assert_eq!(formatter.format_f64(f64::INFINITY).unwrap(), "∞");
}

#[test]
fn rejects_incompatible_or_out_of_range_fraction_options() {
    let english = canonicalize("en").unwrap();
    assert!(matches!(
        NumberFormat::try_new(
            std::slice::from_ref(&english),
            NumberFormatOptions {
                minimum_fraction_digits: Some(3),
                maximum_fraction_digits: Some(2),
                ..Default::default()
            },
        ),
        Err(NumberFormatError::IncompatibleFractionDigits)
    ));
    assert!(matches!(
        NumberFormat::try_new(
            &[english],
            NumberFormatOptions {
                maximum_fraction_digits: Some(101),
                ..Default::default()
            },
        ),
        Err(NumberFormatError::FractionDigitsOutOfRange)
    ));
}

#[test]
fn exposes_every_decimal_error_grouping_policy_and_locale_selection_path() {
    let requested = [canonicalize("zz").unwrap(), canonicalize("en-GB").unwrap()];
    assert_eq!(
        resolve_number_format_locale(&requested, Default::default()).as_str(),
        "en-GB"
    );
    assert_eq!(
        supported_number_format_locales(&requested, Default::default())
            .iter()
            .map(|locale| locale.as_str())
            .collect::<Vec<_>>(),
        ["en-GB"]
    );
    let traced = NumberFormat::try_new(&requested, Default::default()).unwrap();
    assert_eq!(traced.negotiation().matcher(), Default::default());
    assert_eq!(
        traced
            .negotiation()
            .candidates()
            .iter()
            .map(|candidate| candidate.requested().as_str())
            .collect::<Vec<_>>(),
        ["zz", "en-GB"]
    );

    let always = NumberFormat::try_new(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            use_grouping: NumberGrouping::Always,
            minimum_fraction_digits: Some(4),
            maximum_fraction_digits: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(always.format_decimal("1000.5").unwrap(), "1,000.5000");
    assert_eq!(always.resolved_options().maximum_fraction_digits, 4);
    assert!(always.bytes() > std::mem::size_of::<NumberFormat>());
    assert_eq!(
        always.format_decimal("not a decimal"),
        Err(NumberFormatError::InvalidDecimal)
    );
    for (error, message) in [
        (
            NumberFormatError::DataUnavailable,
            "decimal data is unavailable",
        ),
        (
            NumberFormatError::FractionDigitsOutOfRange,
            "fraction digits must be in the range 0 through 100",
        ),
        (
            NumberFormatError::IncompatibleFractionDigits,
            "minimum fraction digits exceed maximum fraction digits",
        ),
        (
            NumberFormatError::InvalidDecimal,
            "invalid finite decimal input",
        ),
        (NumberFormatError::NonFiniteNumber, "number must be finite"),
        (NumberFormatError::MissingUnit, "unit style requires a unit"),
        (
            NumberFormatError::MinimumIntegerDigitsOutOfRange,
            "minimum integer digits must be in the range 1 through 21",
        ),
        (
            NumberFormatError::FormattingFailed,
            "number formatting failed",
        ),
    ] {
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn formats_duration_units_and_parts_through_the_number_service() {
    let english = canonicalize("en").unwrap();
    let short_year = NumberFormat::try_new(
        std::slice::from_ref(&english),
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Year),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(short_year.format_f64(2.0).unwrap(), "2 yrs");
    assert_eq!(
        short_year
            .format_to_parts_decimal("1234.5")
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "1".into()),
            (NumberFormatPartKind::Group, ",".into()),
            (NumberFormatPartKind::Integer, "234".into()),
            (NumberFormatPartKind::Decimal, ".".into()),
            (NumberFormatPartKind::Fraction, "5".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Unit, "yrs".into()),
        ]
    );
    assert_eq!(
        NumberFormatUnit::parse("microsecond"),
        Some(NumberFormatUnit::Microsecond)
    );
    assert_eq!(NumberFormatUnit::parse("invalid"), None);
}

#[test]
fn formats_currency_patterns_digits_and_parts_through_the_number_service() {
    let accounting = NumberFormat::try_new_with_currency(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Currency,
            ..Default::default()
        },
        1,
        None,
        None,
        Default::default(),
        Some(NumberCurrencyOptions {
            code: "USD".into(),
            display: NumberCurrencyDisplay::Symbol,
            sign: NumberCurrencySign::Accounting,
        }),
    )
    .unwrap();
    assert_eq!(accounting.format_decimal("-987").unwrap(), "($987.00)");
    assert_eq!(
        accounting
            .format_to_parts_decimal("-987")
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Literal, "(".into()),
            (NumberFormatPartKind::Currency, "$".into()),
            (NumberFormatPartKind::Integer, "987".into()),
            (NumberFormatPartKind::Decimal, ".".into()),
            (NumberFormatPartKind::Fraction, "00".into()),
            (NumberFormatPartKind::Literal, ")".into()),
        ]
    );
    assert_eq!(
        NumberFormat::try_new_with_currency(
            &[canonicalize("de").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Currency,
                ..Default::default()
            },
            1,
            None,
            None,
            Default::default(),
            Some(NumberCurrencyOptions {
                code: "USD".into(),
                display: NumberCurrencyDisplay::Symbol,
                sign: NumberCurrencySign::Accounting,
            }),
        )
        .unwrap()
        .format_decimal("-987")
        .unwrap(),
        "-987,00\u{a0}$"
    );
    assert!(matches!(
        NumberFormat::try_new(
            &[canonicalize("en").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Currency,
                ..Default::default()
            },
        ),
        Err(NumberFormatError::MissingCurrency)
    ));

    let french = NumberFormat::try_new_with_currency(
        &[canonicalize("fr-FR").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Currency,
            ..Default::default()
        },
        1,
        None,
        None,
        Default::default(),
        Some(NumberCurrencyOptions {
            code: "EUR".into(),
            display: NumberCurrencyDisplay::Symbol,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(
        french.format_decimal("1234.5").unwrap(),
        "1\u{202f}234,50\u{a0}€"
    );
}

#[test]
fn formats_percent_values_and_parts_through_the_number_service() {
    let formatter = NumberFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Percent,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(formatter.format_decimal("0.2").unwrap(), "20%");
    assert_eq!(
        formatter
            .format_to_parts_decimal("-123")
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::MinusSign, "-".into()),
            (NumberFormatPartKind::Integer, "12".into()),
            (NumberFormatPartKind::Group, ",".into()),
            (NumberFormatPartKind::Integer, "300".into()),
            (NumberFormatPartKind::PercentSign, "%".into()),
        ]
    );
    let french = NumberFormat::try_new(
        &[canonicalize("fr-FR").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Percent,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(french.format_decimal("0.2").unwrap(), "20\u{a0}%");
}

#[test]
fn honors_duration_delegation_digit_sign_and_rounding_options() {
    let english = canonicalize("en").unwrap();
    let formatter = NumberFormat::try_new(
        &[english],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Second),
            unit_display: NumberUnitDisplay::Narrow,
            use_grouping: NumberGrouping::Never,
            minimum_integer_digits: 2,
            maximum_fraction_digits: Some(2),
            rounding_mode: NumberRoundingMode::Trunc,
            sign_display: NumberSignDisplay::Never,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(formatter.format_decimal("-1.239").unwrap(), "01.23s");
    let resolved = formatter.resolved_options();
    assert_eq!(resolved.style, NumberFormatStyle::Unit);
    assert_eq!(resolved.unit, Some(NumberFormatUnit::Second));
    assert_eq!(resolved.unit_display, NumberUnitDisplay::Narrow);
    assert_eq!(resolved.minimum_integer_digits, 2);
    assert_eq!(resolved.rounding_mode, NumberRoundingMode::Trunc);
    assert_eq!(resolved.sign_display, NumberSignDisplay::Never);
    assert!(matches!(
        NumberFormat::try_new(
            &[canonicalize("en").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                ..Default::default()
            },
        ),
        Err(NumberFormatError::MissingUnit)
    ));
    assert!(matches!(
        NumberFormat::try_new(
            &[canonicalize("en").unwrap()],
            NumberFormatOptions {
                minimum_integer_digits: 22,
                ..Default::default()
            },
        ),
        Err(NumberFormatError::MinimumIntegerDigitsOutOfRange)
    ));
}

#[test]
fn distinguishes_all_ecma402_sign_display_options_including_negative_zero() {
    let english = canonicalize("en").unwrap();
    for (sign_display, expected) in [
        (NumberSignDisplay::Auto, ["-0", "-2", "2"]),
        (NumberSignDisplay::Never, ["0", "2", "2"]),
        (NumberSignDisplay::Always, ["-0", "-2", "+2"]),
        (NumberSignDisplay::ExceptZero, ["0", "-2", "+2"]),
        (NumberSignDisplay::Negative, ["0", "-2", "2"]),
    ] {
        let formatter = NumberFormat::try_new(
            std::slice::from_ref(&english),
            NumberFormatOptions {
                sign_display,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(formatter.format_f64(-0.0).unwrap(), expected[0]);
        assert_eq!(formatter.format_decimal("-2").unwrap(), expected[1]);
        assert_eq!(formatter.format_f64(2.0).unwrap(), expected[2]);
    }
    let formatter = NumberFormat::try_new(
        &[english],
        NumberFormatOptions {
            sign_display: NumberSignDisplay::Always,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        formatter
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::PlusSign, "+".into()),
            (NumberFormatPartKind::Integer, "2".into()),
        ]
    );
}

#[test]
fn formats_non_finite_numbers_and_their_observable_parts() {
    let english = canonicalize("en-US").unwrap();
    let formatter = NumberFormat::try_new(
        &[english],
        NumberFormatOptions {
            sign_display: NumberSignDisplay::Always,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(formatter.format_f64(f64::NEG_INFINITY).unwrap(), "-∞");
    assert_eq!(formatter.format_f64(f64::INFINITY).unwrap(), "+∞");
    assert_eq!(formatter.format_f64(f64::NAN).unwrap(), "+NaN");
    assert_eq!(
        formatter
            .format_to_parts_f64(f64::INFINITY)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::PlusSign, "+".into()),
            (NumberFormatPartKind::Infinity, "∞".into()),
        ]
    );
    let except_zero = NumberFormat::try_new(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            sign_display: NumberSignDisplay::ExceptZero,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(except_zero.format_f64(f64::NAN).unwrap(), "NaN");
}
