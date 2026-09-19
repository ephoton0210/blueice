// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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

    let canadian_dollar = NumberFormat::try_new_with_currency(
        &[canonicalize("fr-CA").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Currency,
            ..Default::default()
        },
        1,
        None,
        None,
        Default::default(),
        Some(NumberCurrencyOptions {
            code: "CAD".into(),
            display: NumberCurrencyDisplay::Symbol,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(
        canadian_dollar.format_decimal("12.34").unwrap(),
        "12,34\u{a0}$"
    );

    let forint = NumberFormat::try_new_with_currency(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Currency,
            ..Default::default()
        },
        1,
        None,
        None,
        Default::default(),
        Some(NumberCurrencyOptions {
            code: "HUF".into(),
            display: NumberCurrencyDisplay::NarrowSymbol,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(forint.resolved_options().minimum_fraction_digits, 0);
    assert_eq!(forint.resolved_options().maximum_fraction_digits, 0);
    assert_eq!(forint.format_decimal("12").unwrap(), "Ft\u{a0}12");

    let malagasy_ariary = NumberFormat::try_new_with_currency(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Currency,
            ..Default::default()
        },
        1,
        None,
        None,
        Default::default(),
        Some(NumberCurrencyOptions {
            code: "MGA".into(),
            display: NumberCurrencyDisplay::Symbol,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(malagasy_ariary.format_decimal("12").unwrap(), "MGA\u{a0}12");

    let iso_code = NumberFormat::try_new_with_currency(
        &[canonicalize("en").unwrap()],
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
            display: NumberCurrencyDisplay::Code,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(iso_code.format_decimal("12").unwrap(), "USD\u{a0}12.00");

    let french_name = NumberFormat::try_new_with_currency(
        &[canonicalize("fr").unwrap()],
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
            display: NumberCurrencyDisplay::Name,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(
        french_name.format_decimal("1").unwrap(),
        "1,00 dollar des États-Unis"
    );
    assert_eq!(
        french_name.format_decimal("2").unwrap(),
        "2,00 dollars des États-Unis"
    );
    assert_eq!(
        french_name
            .format_to_parts_decimal("2")
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Decimal, ",".into()),
            (NumberFormatPartKind::Fraction, "00".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (
                NumberFormatPartKind::Currency,
                "dollars des États-Unis".into(),
            ),
        ]
    );

    let accounting_name = NumberFormat::try_new_with_currency(
        &[canonicalize("en").unwrap()],
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
            display: NumberCurrencyDisplay::Name,
            sign: NumberCurrencySign::Accounting,
        }),
    )
    .unwrap();
    assert_eq!(
        accounting_name.format_decimal("-1").unwrap(),
        "-1.00 US dollars"
    );

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

    let canadian_us_dollar = |display| {
        NumberFormat::try_new_with_currency(
            &[canonicalize("fr-CA").unwrap()],
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
                display,
                sign: NumberCurrencySign::Standard,
            }),
        )
        .unwrap()
    };
    assert_eq!(
        canadian_us_dollar(NumberCurrencyDisplay::Symbol)
            .format_decimal("1234.5")
            .unwrap(),
        "1\u{a0}234,50\u{a0}$\u{a0}US"
    );
    assert_eq!(
        canadian_us_dollar(NumberCurrencyDisplay::NarrowSymbol)
            .format_decimal("1234.5")
            .unwrap(),
        "1\u{a0}234,50\u{a0}$"
    );
    assert_eq!(
        canadian_us_dollar(NumberCurrencyDisplay::Code)
            .format_decimal("1234.5")
            .unwrap(),
        "1\u{a0}234,50\u{a0}USD"
    );

    let arabic_code = NumberFormat::try_new_with_currency(
        &[canonicalize("ar-u-nu-arab").unwrap()],
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
            display: NumberCurrencyDisplay::Code,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(
        arabic_code.format_decimal("1234").unwrap(),
        "\u{200f}١٬٢٣٤٫٠٠\u{a0}USD"
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

    let turkish = NumberFormat::try_new(
        &[canonicalize("tr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Percent,
            sign_display: NumberSignDisplay::Always,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(turkish.format_decimal("0.2").unwrap(), "+%20");
    assert_eq!(
        turkish
            .format_to_parts_decimal("-0.2")
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::MinusSign, "-".into()),
            (NumberFormatPartKind::PercentSign, "%".into()),
            (NumberFormatPartKind::Integer, "20".into()),
        ]
    );
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
