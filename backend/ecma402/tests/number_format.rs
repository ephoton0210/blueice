// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral decimal `Intl.NumberFormat` coverage.

use blueice_ecma402::{
    canonicalize, locale_data_provider, locale_with_numbering_system, resolve_number_format_locale,
    supported_number_format_locales, unicode_keyword, NumberCompactDisplay, NumberCurrencyDisplay,
    NumberCurrencyOptions, NumberCurrencySign, NumberFormat, NumberFormatError, NumberFormatInput,
    NumberFormatOptions, NumberFormatPartKind, NumberFormatStyle, NumberFormatUnit, NumberGrouping,
    NumberNotation, NumberRangePartSource, NumberRoundingMode, NumberRoundingPriority,
    NumberSignDisplay, NumberUnitDisplay, ResolvedNumberFormatOptions, SUPPORTED_NUMBERING_SYSTEMS,
};

#[path = "number_format/locale_units.rs"]
mod locale_units;

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
fn formats_number_ranges_without_losing_decimal_precision_or_part_sources() {
    let currency = NumberCurrencyOptions {
        code: "USD".into(),
        display: NumberCurrencyDisplay::Symbol,
        sign: NumberCurrencySign::Standard,
    };
    let formatter = NumberFormat::try_new_with_currency(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Currency,
            maximum_fraction_digits: Some(0),
            ..Default::default()
        },
        1,
        None,
        None,
        Default::default(),
        Some(currency),
    )
    .unwrap();
    let parts = formatter
        .format_range_inputs_to_parts(
            NumberFormatInput::Number(3.0),
            NumberFormatInput::Number(5.0),
        )
        .unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|part| (part.kind, part.value.as_str(), part.source))
            .collect::<Vec<_>>(),
        vec![
            (
                NumberFormatPartKind::Currency,
                "$",
                NumberRangePartSource::StartRange
            ),
            (
                NumberFormatPartKind::Integer,
                "3",
                NumberRangePartSource::StartRange
            ),
            (
                NumberFormatPartKind::Literal,
                " – ",
                NumberRangePartSource::Shared
            ),
            (
                NumberFormatPartKind::Currency,
                "$",
                NumberRangePartSource::EndRange
            ),
            (
                NumberFormatPartKind::Integer,
                "5",
                NumberRangePartSource::EndRange
            ),
        ]
    );
    assert_eq!(
        formatter
            .format_range_inputs(
                NumberFormatInput::Number(2.9),
                NumberFormatInput::Number(3.1),
            )
            .unwrap(),
        "~$3"
    );

    let french_decimal = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions::default(),
    )
    .unwrap();
    assert_eq!(
        french_decimal
            .format_range_inputs(
                NumberFormatInput::Number(1.0),
                NumberFormatInput::Number(1.0),
            )
            .unwrap(),
        "≃1"
    );

    let decimal = NumberFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions::default(),
    )
    .unwrap();
    assert_eq!(
        decimal
            .format_range_inputs(
                NumberFormatInput::Decimal("987654321987654321".into()),
                NumberFormatInput::Decimal("987654321987654322".into()),
            )
            .unwrap(),
        "987,654,321,987,654,321–987,654,321,987,654,322"
    );
    let exact_scientific = NumberFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            use_grouping: NumberGrouping::Never,
            maximum_fraction_digits: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        exact_scientific
            .format_input(NumberFormatInput::ScientificDecimal {
                significand: "1.234567890123456789".into(),
                exponent: 0,
            })
            .unwrap(),
        "1.234567890123456789"
    );
    assert_eq!(
        decimal
            .format_range_inputs_to_parts(
                NumberFormatInput::Number(f64::NAN),
                NumberFormatInput::Number(1.0),
            )
            .unwrap_err(),
        NumberFormatError::RangeNaN
    );

    let french_currency_name = NumberFormat::try_new_with_currency(
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
        french_currency_name
            .format_range_inputs(
                NumberFormatInput::Decimal("2".into()),
                NumberFormatInput::Decimal("3".into()),
            )
            .unwrap(),
        "2,00–3,00 dollars des États-Unis"
    );
    assert_eq!(
        french_currency_name
            .format_range_inputs(
                NumberFormatInput::Decimal("1".into()),
                NumberFormatInput::Decimal("2".into()),
            )
            .unwrap(),
        "1,00–2,00 dollars des États-Unis"
    );
    assert_eq!(
        french_currency_name
            .format_range_inputs_to_parts(
                NumberFormatInput::Decimal("1".into()),
                NumberFormatInput::Decimal("2".into()),
            )
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value, part.source))
            .collect::<Vec<_>>(),
        vec![
            (
                NumberFormatPartKind::Integer,
                "1".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Decimal,
                ",".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Fraction,
                "00".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Literal,
                "–".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Integer,
                "2".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Decimal,
                ",".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Fraction,
                "00".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Literal,
                " ".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Currency,
                "dollars des États-Unis".into(),
                NumberRangePartSource::Shared,
            ),
        ]
    );

    let french_unit = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Meter),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        french_unit
            .format_range_inputs(
                NumberFormatInput::Number(1.0),
                NumberFormatInput::Number(2.0),
            )
            .unwrap(),
        "1–2\u{a0}mètres"
    );
}

#[test]
fn emits_localized_scientific_and_engineering_exponent_parts() {
    let engineering = NumberFormat::try_new(
        &[canonicalize("de-DE").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Engineering,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        engineering
            .format_to_parts_f64(0.000_345)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "345".into()),
            (NumberFormatPartKind::ExponentSeparator, "E".into()),
            (NumberFormatPartKind::ExponentMinusSign, "-".into()),
            (NumberFormatPartKind::ExponentInteger, "6".into()),
        ]
    );
    let scientific = NumberFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Scientific,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(scientific.format_f64(543_211.1).unwrap(), "5.432E5");

    let arabic = NumberFormat::try_new(
        &[canonicalize("ar-u-nu-arab").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Scientific,
            maximum_fraction_digits: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(arabic.format_f64(123_000.0).unwrap(), "١٫٢أس٥");
    assert_eq!(arabic.format_f64(-0.001_23).unwrap(), "؜-١٫٢أس؜-٣");
    assert_eq!(
        arabic
            .format_to_parts_f64(-0.001_23)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::MinusSign, "؜-".into()),
            (NumberFormatPartKind::Integer, "١".into()),
            (NumberFormatPartKind::Decimal, "٫".into()),
            (NumberFormatPartKind::Fraction, "٢".into()),
            (NumberFormatPartKind::ExponentSeparator, "أس".into()),
            (NumberFormatPartKind::Literal, "؜".into()),
            (NumberFormatPartKind::ExponentMinusSign, "-".into()),
            (NumberFormatPartKind::ExponentInteger, "٣".into()),
        ]
    );

    let persian = NumberFormat::try_new(
        &[canonicalize("fa").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Scientific,
            maximum_fraction_digits: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(persian.format_f64(123_000.0).unwrap(), "۱٫۲×۱۰^۵");
    assert_eq!(persian.format_f64(-0.001_23).unwrap(), "‎−۱٫۲×۱۰^‎−۳");

    let greek = NumberFormat::try_new(
        &[canonicalize("el").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Scientific,
            maximum_fraction_digits: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(greek.format_f64(-0.001_23).unwrap(), "-1,2e-3");

    let estonian = NumberFormat::try_new(
        &[canonicalize("et").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Scientific,
            maximum_fraction_digits: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(estonian.format_f64(-0.001_23).unwrap(), "−1,2×10^−3");

    let pashto = NumberFormat::try_new(
        &[canonicalize("ps-u-nu-arabext").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Scientific,
            maximum_fraction_digits: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(pashto.format_f64(-0.001_23).unwrap(), "‎-‎۱٫۲×۱۰^‎-‎۳");
    assert_eq!(
        pashto
            .format_to_parts_f64(-0.001_23)
            .unwrap()
            .into_iter()
            .skip(4)
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::ExponentSeparator, "×۱۰^".into()),
            (NumberFormatPartKind::Literal, "‎".into()),
            (NumberFormatPartKind::ExponentMinusSign, "-".into()),
            (NumberFormatPartKind::Literal, "‎".into()),
            (NumberFormatPartKind::ExponentInteger, "۳".into()),
        ]
    );
}

#[test]
fn formats_provider_selected_compact_patterns_as_typed_parts() {
    let english = NumberFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        english
            .format_to_parts_f64(9_876.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "9".into()),
            (NumberFormatPartKind::Decimal, ".".into()),
            (NumberFormatPartKind::Fraction, "9".into()),
            (NumberFormatPartKind::Compact, "K".into()),
        ]
    );
    let french = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(french.format_f64(9_876.0).unwrap(), "9,9\u{a0}k");
    assert_eq!(french.format_f64(1_200.0).unwrap(), "1,2\u{a0}k");
    let french_long = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            compact_display: NumberCompactDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(french_long.format_f64(1_200.0).unwrap(), "1,2 millier");
    assert_eq!(french_long.format_f64(1_000.0).unwrap(), "mille");
    assert_eq!(french_long.format_f64(-1_000.0).unwrap(), "-mille");
    assert_eq!(
        french_long
            .format_to_parts_f64(1_000.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![(NumberFormatPartKind::Compact, "mille".into())]
    );
    assert_eq!(
        french_long
            .format_to_parts_f64(-1_000.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::MinusSign, "-".into()),
            (NumberFormatPartKind::Compact, "mille".into()),
        ]
    );
    assert_eq!(
        french_long
            .format_range_inputs(
                NumberFormatInput::Number(1_000.0),
                NumberFormatInput::Number(2_000.0),
            )
            .unwrap(),
        "mille–2 mille"
    );
    assert_eq!(
        french_long
            .format_range_inputs_to_parts(
                NumberFormatInput::Number(1_000.0),
                NumberFormatInput::Number(2_000.0),
            )
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value, part.source))
            .collect::<Vec<_>>(),
        vec![
            (
                NumberFormatPartKind::Compact,
                "mille".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Literal,
                "–".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Integer,
                "2".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Literal,
                " ".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Compact,
                "mille".into(),
                NumberRangePartSource::EndRange,
            ),
        ]
    );
    let french_long_unit = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Meter),
            unit_display: NumberUnitDisplay::Short,
            notation: NumberNotation::Compact,
            compact_display: NumberCompactDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        french_long_unit.format_f64(1_000.0).unwrap(),
        "mille\u{202f}m"
    );
    assert_eq!(
        french_long_unit
            .format_to_parts_f64(1_000.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Compact, "mille".into()),
            (NumberFormatPartKind::Literal, "\u{202f}".into()),
            (NumberFormatPartKind::Unit, "m".into()),
        ]
    );
    let french_compact_currency = NumberFormat::try_new_with_currency(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Currency,
            notation: NumberNotation::Compact,
            ..Default::default()
        },
        1,
        None,
        None,
        Default::default(),
        Some(NumberCurrencyOptions {
            code: "USD".into(),
            display: NumberCurrencyDisplay::Symbol,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(
        french_compact_currency.format_f64(1_200.0).unwrap(),
        "1,2\u{a0}k\u{a0}$US"
    );
    assert_eq!(
        french_compact_currency
            .format_to_parts_f64(1_200.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "1".into()),
            (NumberFormatPartKind::Decimal, ",".into()),
            (NumberFormatPartKind::Fraction, "2".into()),
            (NumberFormatPartKind::Literal, "\u{a0}".into()),
            (NumberFormatPartKind::Compact, "k".into()),
            (NumberFormatPartKind::Literal, "\u{a0}".into()),
            (NumberFormatPartKind::Currency, "$US".into()),
        ]
    );
    let korean = NumberFormat::try_new(
        &[canonicalize("ko-KR").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(korean.format_f64(98_765_432.0).unwrap(), "9877만");

    let swahili = NumberFormat::try_new(
        &[canonicalize("sw").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            compact_display: NumberCompactDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(swahili.format_f64(1_200.0).unwrap(), "elfu 1.2");
    assert_eq!(
        swahili
            .format_to_parts_f64(1_200.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Compact, "elfu".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Integer, "1".into()),
            (NumberFormatPartKind::Decimal, ".".into()),
            (NumberFormatPartKind::Fraction, "2".into()),
        ]
    );
    let hebrew = NumberFormat::try_new(
        &[canonicalize("he").unwrap()],
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hebrew.format_f64(1_200.0).unwrap(), "1.2K\u{200f}");
    assert_eq!(
        hebrew
            .format_to_parts_f64(1_200.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "1".into()),
            (NumberFormatPartKind::Decimal, ".".into()),
            (NumberFormatPartKind::Fraction, "2".into()),
            (NumberFormatPartKind::Compact, "K".into()),
            (NumberFormatPartKind::Literal, "\u{200f}".into()),
        ]
    );
}

#[test]
fn sources_non_english_range_glue_from_the_shared_provider() {
    let format_range = |locale: &str| {
        NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
            NumberFormatOptions::default(),
        )
        .unwrap()
        .format_range_inputs(
            NumberFormatInput::Number(1.0),
            NumberFormatInput::Number(2.0),
        )
        .unwrap()
    };

    assert_eq!(format_range("es"), "1-2");
    assert_eq!(format_range("ja"), "1～2");
    assert_eq!(format_range("ko"), "1~2");

    let french_yen = NumberFormat::try_new_with_currency(
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
            code: "JPY".into(),
            display: NumberCurrencyDisplay::Symbol,
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(
        french_yen
            .format_range_inputs(
                NumberFormatInput::Number(1.0),
                NumberFormatInput::Number(2.0),
            )
            .unwrap(),
        "1–2\u{a0}JPY"
    );

    let japanese_dollar = NumberFormat::try_new_with_currency(
        &[canonicalize("ja").unwrap()],
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
            sign: NumberCurrencySign::Standard,
        }),
    )
    .unwrap();
    assert_eq!(
        japanese_dollar
            .format_range_inputs(
                NumberFormatInput::Number(1.0),
                NumberFormatInput::Number(2.0),
            )
            .unwrap(),
        "$1.00 ～ $2.00"
    );
}

#[test]
fn sources_extended_pinned_cldr_range_glue_and_outer_spacing() {
    let format_range = |locale: &str, start, end| {
        NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
            NumberFormatOptions::default(),
        )
        .unwrap()
        .format_range_inputs(
            NumberFormatInput::Number(start),
            NumberFormatInput::Number(end),
        )
        .unwrap()
    };

    // CLDR 48.2.1 `miscPatterns-numberSystem-*.range` records selected from
    // the same data revision as the pinned ICU4X bundle. The signed cases
    // verify the full-endpoint connector, not just the collapsed raw record.
    assert_eq!(format_range("ca", 1.0, 2.0), "1-2");
    assert_eq!(format_range("ca", -2.0, -1.0), "-2 - -1");
    assert_eq!(format_range("pt", 1.0, 2.0), "1–2");
    assert_eq!(format_range("pt-PT", 1.0, 2.0), "1 - 2");
    assert_eq!(format_range("ro", 1.0, 2.0), "1 - 2");
    assert_eq!(format_range("bg", 1.0, 2.0), "1 – 2");
    assert_eq!(format_range("to", 1.0, 2.0), "1—2");
    assert_eq!(format_range("to", -2.0, -1.0), "-2 — -1");
    assert_eq!(format_range("et", 1.0, 2.0), "1‒2");
    assert_eq!(format_range("mk", 1.0, 2.0), "1\u{2009}–\u{2009}2");
    assert_eq!(format_range("bs-Cyrl", 1.0, 2.0), "1–2");
    assert_eq!(format_range("oc", 1.0, 2.0), "1–2");
    assert_eq!(format_range("oc-ES", 1.0, 2.0), "1-2");
    assert_eq!(format_range("zh-Latn", 1.0, 2.0), "1–2");
}

#[test]
fn selects_currency_interval_patterns_from_affix_and_sign_scope() {
    let formatter = |locale: &str, display, sign| {
        NumberFormat::try_new_with_currency(
            &[canonicalize(locale).unwrap()],
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
                sign,
            }),
        )
        .unwrap()
    };
    let range = |formatter: &NumberFormat, start, end| {
        formatter
            .format_range_inputs(
                NumberFormatInput::Number(start),
                NumberFormatInput::Number(end),
            )
            .unwrap()
    };

    let english = formatter(
        "en-US",
        NumberCurrencyDisplay::Symbol,
        NumberCurrencySign::Standard,
    );
    assert_eq!(range(&english, 3.0, 5.0), "$3.00 – $5.00");
    assert_eq!(range(&english, -5.0, -3.0), "-$5.00–3.00");
    assert_eq!(range(&english, -3.0, 5.0), "-$3.00 – $5.00");

    let english_accounting = formatter(
        "en-US",
        NumberCurrencyDisplay::Symbol,
        NumberCurrencySign::Accounting,
    );
    assert_eq!(range(&english_accounting, -5.0, -3.0), "($5.00–3.00)");
    assert_eq!(
        english_accounting
            .format_range_inputs_to_parts(
                NumberFormatInput::Number(-5.0),
                NumberFormatInput::Number(-3.0),
            )
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value, part.source))
            .collect::<Vec<_>>(),
        vec![
            (
                NumberFormatPartKind::Literal,
                "(".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Currency,
                "$".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Integer,
                "5".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Decimal,
                ".".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Fraction,
                "00".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Literal,
                "–".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Integer,
                "3".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Decimal,
                ".".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Fraction,
                "00".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Literal,
                ")".into(),
                NumberRangePartSource::Shared,
            ),
        ]
    );

    let french = formatter(
        "fr",
        NumberCurrencyDisplay::Symbol,
        NumberCurrencySign::Standard,
    );
    assert_eq!(range(&french, 3.0, 5.0), "3,00–5,00\u{a0}$US");
    assert_eq!(range(&french, -5.0, -3.0), "-5,00–3,00\u{a0}$US");
    assert_eq!(range(&french, -3.0, 5.0), "-3,00\u{a0}$US – 5,00\u{a0}$US");
    assert_eq!(
        french
            .format_range_inputs_to_parts(
                NumberFormatInput::Number(-3.0),
                NumberFormatInput::Number(5.0),
            )
            .unwrap()
            .into_iter()
            .filter(|part| {
                matches!(
                    part.kind,
                    NumberFormatPartKind::Currency | NumberFormatPartKind::Literal
                )
            })
            .map(|part| (part.value, part.source))
            .collect::<Vec<_>>(),
        vec![
            ("\u{a0}".into(), NumberRangePartSource::StartRange),
            ("$US".into(), NumberRangePartSource::StartRange),
            (" – ".into(), NumberRangePartSource::Shared),
            ("\u{a0}".into(), NumberRangePartSource::EndRange),
            ("$US".into(), NumberRangePartSource::EndRange),
        ]
    );

    let english_code = formatter(
        "en-US",
        NumberCurrencyDisplay::Code,
        NumberCurrencySign::Standard,
    );
    assert_eq!(range(&english_code, 3.0, 5.0), "USD\u{a0}3.00–5.00");

    let thai = formatter(
        "th",
        NumberCurrencyDisplay::Symbol,
        NumberCurrencySign::Standard,
    );
    assert_eq!(range(&thai, 3.0, 5.0), "US$3.00-5.00");
    let thai_narrow = formatter(
        "th",
        NumberCurrencyDisplay::NarrowSymbol,
        NumberCurrencySign::Standard,
    );
    assert_eq!(range(&thai_narrow, 3.0, 5.0), "$3.00 - $5.00");

    let japanese_code = formatter(
        "ja",
        NumberCurrencyDisplay::Code,
        NumberCurrencySign::Standard,
    );
    assert_eq!(range(&japanese_code, 3.0, 5.0), "USD\u{a0}3.00～5.00");
}

#[test]
fn selects_common_range_patterns_by_style_affix_and_sign_scope() {
    let formatter = |locale: &str, options| {
        NumberFormat::try_new(&[canonicalize(locale).unwrap()], options).unwrap()
    };
    let range = |formatter: &NumberFormat, start, end| {
        formatter
            .format_range_inputs(
                NumberFormatInput::Number(start),
                NumberFormatInput::Number(end),
            )
            .unwrap()
    };

    let decimal = formatter("en-US", NumberFormatOptions::default());
    assert_eq!(range(&decimal, 3.0, 5.0), "3–5");
    assert_eq!(range(&decimal, -5.0, -3.0), "-5 – -3");

    let japanese_decimal = formatter("ja", NumberFormatOptions::default());
    assert_eq!(range(&japanese_decimal, -5.0, -3.0), "-5 ～ -3");

    let english_percent = formatter(
        "en-US",
        NumberFormatOptions {
            style: NumberFormatStyle::Percent,
            ..Default::default()
        },
    );
    assert_eq!(range(&english_percent, 3.0, 5.0), "300% – 500%");
    assert_eq!(range(&english_percent, -5.0, -3.0), "-500–300%");
    assert_eq!(range(&english_percent, -3.0, 5.0), "-300% – 500%");

    let french_percent = formatter(
        "fr",
        NumberFormatOptions {
            style: NumberFormatStyle::Percent,
            ..Default::default()
        },
    );
    assert_eq!(range(&french_percent, 3.0, 5.0), "300–500\u{a0}%");
    assert_eq!(
        range(&french_percent, -3.0, 5.0),
        "-300\u{a0}% – 500\u{a0}%"
    );

    let turkish_percent = formatter(
        "tr",
        NumberFormatOptions {
            style: NumberFormatStyle::Percent,
            ..Default::default()
        },
    );
    assert_eq!(range(&turkish_percent, 3.0, 5.0), "%300 – %500");
    assert_eq!(range(&turkish_percent, -5.0, -3.0), "-%500–300");

    let english_unit = formatter(
        "en-US",
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Meter),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    );
    assert_eq!(range(&english_unit, 3.0, 5.0), "3–5 meters");
    assert_eq!(range(&english_unit, -5.0, -3.0), "-5 – -3 meters");
    assert_eq!(range(&english_unit, -3.0, 5.0), "-3 – 5 meters");

    // A `percent` unit shares the high-level percent-range semantics rather
    // than behaving like an arbitrary unit suffix. A bare `%` remains on
    // both unsigned endpoints, whereas a shared sign lets it collapse.
    let english_percent_unit = formatter(
        "en-US",
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Percent),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    );
    assert_eq!(range(&english_percent_unit, 3.0, 5.0), "3% – 5%");
    assert_eq!(range(&english_percent_unit, -5.0, -3.0), "-5–3%");
    assert_eq!(range(&english_percent_unit, -3.0, 5.0), "-3% – 5%");

    // The French localized-space affix makes the percent unit collapsible,
    // preserving the language-specific contrast with English.
    let french_percent_unit = formatter(
        "fr",
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Percent),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    );
    assert_eq!(range(&french_percent_unit, 3.0, 5.0), "3–5 %");

    // Turkish locates the same bare marker before the number, so unsigned
    // endpoints retain it rather than sharing a prefix unit marker.
    let turkish_percent_unit = formatter(
        "tr",
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Percent),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    );
    assert_eq!(range(&turkish_percent_unit, 3.0, 5.0), "%3 – %5");

    // Turkish short Celsius changes the placeholder-adjacent space between
    // `one` and `other`. Treat that literal as part of the semantic unit
    // affix so it is reconstructed once with the range plural category.
    let turkish_celsius = formatter(
        "tr",
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Celsius),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    );
    assert_eq!(range(&turkish_celsius, 1.0, 2.0), "1–2°C");
    assert_eq!(range(&turkish_celsius, -5.0, -2.0), "-5 – -2°C");

    let japanese_celsius = formatter(
        "ja",
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Celsius),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    );
    assert_eq!(range(&japanese_celsius, 3.0, 5.0), "摂氏 3～5 度");
    assert_eq!(range(&japanese_celsius, -5.0, -3.0), "摂氏 -5 ～ -3 度");

    let english_compact = formatter(
        "en-US",
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            ..Default::default()
        },
    );
    assert_eq!(range(&english_compact, 1_200.0, 2_300.0), "1.2K – 2.3K");
    assert_eq!(range(&english_compact, -2_300.0, -1_200.0), "-2.3–1.2K");
    assert_eq!(range(&english_compact, -1_200.0, 2_300.0), "-1.2K – 2.3K");

    let french_compact = formatter(
        "fr",
        NumberFormatOptions {
            notation: NumberNotation::Compact,
            ..Default::default()
        },
    );
    assert_eq!(range(&french_compact, 1_200.0, 2_300.0), "1,2–2,3\u{a0}k");
    assert_eq!(
        range(&french_compact, -1_200.0, 2_300.0),
        "-1,2\u{a0}k – 2,3\u{a0}k"
    );
}

#[test]
fn sources_localized_temperature_and_angle_units_from_the_shared_provider() {
    let format = |locale: &str, unit, display, value| {
        NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: Some(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
        .format_f64(value)
        .unwrap()
    };

    assert_eq!(
        format(
            "fr",
            NumberFormatUnit::Celsius,
            NumberUnitDisplay::Long,
            2.0,
        ),
        "2\u{a0}degrés Celsius"
    );
    assert_eq!(
        format(
            "fr",
            NumberFormatUnit::Fahrenheit,
            NumberUnitDisplay::Short,
            2.0,
        ),
        "2\u{202f}°F"
    );
    assert_eq!(
        format(
            "ja",
            NumberFormatUnit::Celsius,
            NumberUnitDisplay::Long,
            2.0,
        ),
        "摂氏 2 度"
    );
    assert_eq!(
        format("ru", NumberFormatUnit::Degree, NumberUnitDisplay::Long, 5.0,),
        "5 градусов"
    );
    assert_eq!(
        format("ar", NumberFormatUnit::Degree, NumberUnitDisplay::Long, 1.0,),
        "درجة"
    );
    assert_eq!(
        format("fr", NumberFormatUnit::Byte, NumberUnitDisplay::Long, 2.0,),
        "2\u{a0}octets"
    );
    assert_eq!(
        format(
            "fr",
            NumberFormatUnit::Kilobyte,
            NumberUnitDisplay::Short,
            2.0,
        ),
        "2\u{202f}ko"
    );
    assert_eq!(
        format(
            "fr",
            NumberFormatUnit::Percent,
            NumberUnitDisplay::Long,
            2.0,
        ),
        "2 pour cent"
    );
    let french_compound = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: NumberFormatUnit::parse("gigabyte-per-second"),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        french_compound.format_f64(2.0).unwrap(),
        "2\u{a0}gigaoctets par seconde"
    );
    assert_eq!(
        french_compound
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, "\u{a0}".into()),
            (NumberFormatPartKind::Unit, "gigaoctets par seconde".into(),),
        ]
    );
    for (display, expected) in [
        (NumberUnitDisplay::Short, "2\u{202f}Go/s"),
        (NumberUnitDisplay::Narrow, "2Go/s"),
    ] {
        let formatter = NumberFormat::try_new(
            &[canonicalize("fr").unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: NumberFormatUnit::parse("gigabyte-per-second"),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(formatter.format_f64(2.0).unwrap(), expected);
    }
    assert_eq!(
        format(
            "ja",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Long,
            2.0,
        ),
        "2 ギガバイト"
    );
    assert_eq!(
        format("ja", NumberFormatUnit::Byte, NumberUnitDisplay::Short, 2.0,),
        "2 byte"
    );
    assert_eq!(
        format(
            "ja",
            NumberFormatUnit::Percent,
            NumberUnitDisplay::Narrow,
            2.0,
        ),
        "2%"
    );
    let japanese_compound = NumberFormat::try_new(
        &[canonicalize("ja").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: NumberFormatUnit::parse("gigabyte-per-second"),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        japanese_compound.format_f64(2.0).unwrap(),
        "2 ギガバイト/秒"
    );
    assert_eq!(
        japanese_compound
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Unit, "ギガバイト/秒".into()),
        ]
    );
    assert_eq!(
        format(
            "ru",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Long,
            1.0,
        ),
        "1 гигабайт"
    );
    assert_eq!(
        format(
            "ru",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Long,
            2.0,
        ),
        "2 гигабайта"
    );
    assert_eq!(
        format(
            "ru",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Long,
            5.0,
        ),
        "5 гигабайт"
    );
    assert_eq!(
        format(
            "ru",
            NumberFormatUnit::Percent,
            NumberUnitDisplay::Long,
            5.0,
        ),
        "5 процентов"
    );
    let russian_compound = NumberFormat::try_new(
        &[canonicalize("ru").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: NumberFormatUnit::parse("gigabyte-per-second"),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        russian_compound.format_f64(2.0).unwrap(),
        "2 гигабайта в секунду"
    );
    assert_eq!(
        format(
            "ru",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Short,
            2.0,
        ),
        "2 ГБ"
    );
    assert_eq!(
        format(
            "ru",
            NumberFormatUnit::Percent,
            NumberUnitDisplay::Narrow,
            2.0,
        ),
        "2%"
    );
    // ICU4X does not yet generate typed name markers for these categories.
    // The Spanish records therefore prove that the provider's raw CLDR bridge
    // covers the whole remaining sanctioned simple-unit family, rather than
    // silently substituting the English fallback.
    assert_eq!(
        format(
            "es",
            NumberFormatUnit::Celsius,
            NumberUnitDisplay::Long,
            2.0,
        ),
        "2 grados Celsius"
    );
    assert_eq!(
        format(
            "es",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Short,
            2.0,
        ),
        "2 GB"
    );
    assert_eq!(
        format(
            "es",
            NumberFormatUnit::Percent,
            NumberUnitDisplay::Short,
            2.0,
        ),
        "2\u{a0}%"
    );
    assert_eq!(
        format(
            "es",
            NumberFormatUnit::Degree,
            NumberUnitDisplay::Narrow,
            2.0,
        ),
        "2°"
    );

    let spanish_compound = NumberFormat::try_new(
        &[canonicalize("es").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: NumberFormatUnit::parse("gigabyte-per-second"),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        spanish_compound.format_f64(2.0).unwrap(),
        "2 gigabytes por segundo"
    );
    assert_eq!(
        spanish_compound
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Unit, "gigabytes por segundo".into(),),
        ]
    );

    assert_eq!(
        format(
            "ar-u-nu-arab",
            NumberFormatUnit::Gigabyte,
            NumberUnitDisplay::Narrow,
            2.0,
        ),
        "٢ غ.ب"
    );
    assert_eq!(
        format(
            "ar-u-nu-arab",
            NumberFormatUnit::Percent,
            NumberUnitDisplay::Long,
            1.0,
        ),
        "١ بالمائة"
    );
    assert_eq!(
        format(
            "ar-u-nu-arab",
            NumberFormatUnit::Percent,
            NumberUnitDisplay::Long,
            2.0,
        ),
        "٢ ٪"
    );
    let arabic_compound = NumberFormat::try_new(
        &[canonicalize("ar-u-nu-arab").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: NumberFormatUnit::parse("gigabyte-per-second"),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        arabic_compound.format_f64(2.0).unwrap(),
        "٢ غيغابايت في الثانية"
    );
}

#[test]
fn uses_localized_per_unit_grammar_before_the_generic_connector() {
    let format = |locale, unit, display| {
        NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
            NumberFormatOptions {
                style: NumberFormatStyle::Unit,
                unit: NumberFormatUnit::parse(unit),
                unit_display: display,
                ..Default::default()
            },
        )
        .unwrap()
        .format_f64(2.0)
        .unwrap()
    };

    // These records cannot be reconstructed from ICU4X's generic `per`
    // connector: French changes `year` to `an`, Arabic selects a preposition,
    // and Japanese uses a slash in long display.
    assert_eq!(
        format("fr", "meter-per-year", NumberUnitDisplay::Long),
        "2\u{a0}mètres par an"
    );
    assert_eq!(
        format("es", "meter-per-week", NumberUnitDisplay::Short),
        "2 m/sem."
    );
    assert_eq!(
        format("ar-u-nu-arab", "meter-per-second", NumberUnitDisplay::Long),
        "٢ متر في الثانية"
    );
    assert_eq!(
        format("ja", "meter-per-second", NumberUnitDisplay::Long),
        "2 メートル/秒"
    );
    assert_eq!(
        format("ru", "meter-per-year", NumberUnitDisplay::Long),
        "2 метра в год"
    );

    let arabic = NumberFormat::try_new(
        &[canonicalize("ar-u-nu-arab").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: NumberFormatUnit::parse("meter-per-second"),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        arabic
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "٢".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Unit, "متر في الثانية".into()),
        ]
    );

    // CLDR's Arabic long singular `acre` deliberately has no number
    // placeholder. Generic composition must preserve that semantic instead
    // of restoring a synthetic digit while adding the denominator.
    let arabic_hidden_numerator = NumberFormat::try_new(
        &[canonicalize("ar-u-nu-arab").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: NumberFormatUnit::parse("acre-per-second"),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        arabic_hidden_numerator.format_f64(1.0).unwrap(),
        "فدان في الثانية"
    );
    assert_eq!(
        arabic_hidden_numerator
            .format_to_parts_f64(1.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![(NumberFormatPartKind::Unit, "فدان في الثانية".into())]
    );
}

#[test]
fn chooses_fraction_or_significant_digits_from_rounding_priority() {
    let more = NumberFormat::try_new_with_precision(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            minimum_fraction_digits: Some(2),
            rounding_priority: NumberRoundingPriority::MorePrecision,
            ..Default::default()
        },
        1,
        Some(2),
        None,
    )
    .unwrap();
    assert_eq!(more.format_f64(1.0).unwrap(), "1.0");
    assert_eq!(
        more.rounding_priority(),
        NumberRoundingPriority::MorePrecision
    );

    let less = NumberFormat::try_new_with_precision(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            minimum_fraction_digits: Some(2),
            rounding_priority: NumberRoundingPriority::LessPrecision,
            ..Default::default()
        },
        1,
        Some(2),
        None,
    )
    .unwrap();
    assert_eq!(less.format_f64(1.0).unwrap(), "1.00");
    assert_eq!(
        less.rounding_priority(),
        NumberRoundingPriority::LessPrecision
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
fn resolves_every_advertised_numbering_system() {
    let english = canonicalize("en").unwrap();
    let mut unavailable = Vec::new();
    for numbering_system in SUPPORTED_NUMBERING_SYSTEMS {
        let locale = locale_with_numbering_system(&english, numbering_system, false);
        match NumberFormat::try_new(&[locale], NumberFormatOptions::default()) {
            Ok(format) => assert_eq!(
                format.resolved_options().numbering_system,
                *numbering_system
            ),
            Err(_) => unavailable.push(*numbering_system),
        }
    }
    assert!(unavailable.is_empty(), "unavailable: {unavailable:?}");
}

#[test]
fn scientific_notation_covers_every_advertised_locale_and_numbering_system() {
    let provider = locale_data_provider();
    let mut unavailable = Vec::new();
    let mut malformed = Vec::new();
    for locale in provider.number_format_locales() {
        let requested = canonicalize(locale).expect("advertised locale is structurally valid");
        for numbering_system in SUPPORTED_NUMBERING_SYSTEMS {
            let requested = locale_with_numbering_system(&requested, numbering_system, false);
            let formatter = match NumberFormat::try_new(
                &[requested],
                NumberFormatOptions {
                    notation: NumberNotation::Scientific,
                    maximum_fraction_digits: Some(1),
                    ..Default::default()
                },
            ) {
                Ok(formatter) => formatter,
                Err(error) => {
                    unavailable.push(format!("{locale}-u-nu-{numbering_system}: {error}"));
                    continue;
                }
            };
            let parts = match formatter.format_to_parts_f64(-0.001_23) {
                Ok(parts) => parts,
                Err(error) => {
                    unavailable.push(format!("{locale}-u-nu-{numbering_system}: {error}"));
                    continue;
                }
            };
            if ![
                NumberFormatPartKind::ExponentSeparator,
                NumberFormatPartKind::ExponentMinusSign,
                NumberFormatPartKind::ExponentInteger,
            ]
            .into_iter()
            .all(|kind| {
                parts
                    .iter()
                    .any(|part| part.kind == kind && !part.value.is_empty())
            }) {
                malformed.push(format!("{locale}-u-nu-{numbering_system}: {parts:?}"));
            }
        }
    }
    assert!(
        unavailable.is_empty(),
        "scientific locale/numbering-system construction gaps: {}",
        unavailable.join(", ")
    );
    assert!(
        malformed.is_empty(),
        "scientific locale/numbering-system part gaps: {}",
        malformed.join(", ")
    );
}

#[test]
fn constructs_every_advertised_decimal_locale_from_pinned_cldr_data() {
    let provider = locale_data_provider();
    let mut unavailable = Vec::new();
    for locale in provider.number_format_locales() {
        let requested = canonicalize(locale).expect("advertised locale is structurally valid");
        let expected_numbering_system = provider.default_numbering_system(requested.locale());
        match NumberFormat::try_new(&[requested], NumberFormatOptions::default()) {
            Ok(formatter) => {
                assert_eq!(
                    formatter.resolved_options().numbering_system,
                    expected_numbering_system,
                    "default numbering system for {locale}"
                );
                assert!(
                    !formatter.format_decimal("12345.6").unwrap().is_empty(),
                    "pinned decimal formatter for {locale}"
                );
            }
            Err(error) => unavailable.push(format!("{locale}: {error}")),
        }
    }
    assert!(
        unavailable.is_empty(),
        "advertised locales without pinned decimal construction: {}",
        unavailable.join(", ")
    );
}

#[test]
fn constructs_former_direct_icu_decimal_locales_from_pinned_cldr_data() {
    // These locales were directly available from the former ICU4X decimal
    // provider but are not part of BlueIce's advertised NumberFormat list.
    // Retain the established construction surface while removing its root
    // fallback behavior.
    for locale in [
        "ast", "ba", "bgc", "bho", "blo", "brx", "bua", "cv", "eu", "ht", "ia", "ie", "jv", "kea",
        "kgp", "ks", "kxv", "lij", "lmo", "mni", "nds", "nqo", "oc", "pms", "qu", "raj", "rm",
        "rw", "sah", "sat", "sc", "scn", "sd", "su", "szl", "tg", "tn", "tt", "tyv", "und", "vec",
        "vmw", "xnr", "yrl",
    ] {
        let formatter = NumberFormat::try_new(
            &[canonicalize(locale).expect("former ICU locale is structurally valid")],
            NumberFormatOptions::default(),
        )
        .unwrap_or_else(|error| panic!("pinned decimal formatter for {locale}: {error}"));
        assert!(
            !formatter.format_decimal("12345.6").unwrap().is_empty(),
            "pinned decimal output for {locale}"
        );
    }
}

#[test]
fn uses_cldr_defaults_not_the_former_synthetic_numbering_system_map() {
    for (locale, numbering_system, expected) in [
        ("ar", "latn", "1,234,567.5"),
        ("as", "beng", "১২,৩৪,৫৬৭.৫"),
        ("dz", "tibt", "༡༢,༣༤,༥༦༧.༥"),
        ("fa", "arabext", "۱٬۲۳۴٬۵۶۷٫۵"),
        ("mr", "deva", "१२,३४,५६७.५"),
        ("or", "latn", "12,34,567.5"),
    ] {
        let formatter = NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
            NumberFormatOptions::default(),
        )
        .unwrap();
        assert_eq!(
            formatter.resolved_options().numbering_system,
            numbering_system,
            "CLDR default numbering system for {locale}"
        );
        assert_eq!(formatter.format_decimal("1234567.5").unwrap(), expected);
    }
}

#[test]
fn sources_simple_numbering_system_digits_from_the_shared_provider() {
    let english = canonicalize("en").unwrap();
    let ahom = locale_with_numbering_system(&english, "ahom", false);
    let format = NumberFormat::try_new(&[ahom], NumberFormatOptions::default()).unwrap();

    assert!(SUPPORTED_NUMBERING_SYSTEMS.contains(&"ahom"));
    assert_eq!(format.resolved_options().numbering_system, "ahom");
    assert_eq!(format.format_decimal("123").unwrap(), "𑜱𑜲𑜳");
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
fn sources_korean_cldr_digital_temperature_and_generic_per_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("ko").unwrap()],
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
        "2기가바이트"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "섭씨 2도"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2%"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Second, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2초"
    );
    let celsius = formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long);
    assert_eq!(
        celsius
            .format_range_inputs(
                NumberFormatInput::Number(3.0),
                NumberFormatInput::Number(5.0),
            )
            .unwrap(),
        "섭씨 3~5도"
    );

    let per_second = formatter(
        NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
        NumberUnitDisplay::Long,
    );
    assert_eq!(per_second.format_f64(2.0).unwrap(), "초당 2기가바이트");
    assert_eq!(
        per_second
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Unit, "초당".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Unit, "기가바이트".into()),
        ]
    );
    assert_eq!(
        per_second
            .format_range_inputs(
                NumberFormatInput::Number(3.0),
                NumberFormatInput::Number(5.0),
            )
            .unwrap(),
        "초당 3~5기가바이트"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/초"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-celsius").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "섭씨당 2기가바이트"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "초당 섭씨 2도"
    );
}

#[test]
fn sources_chinese_cldr_simple_and_generic_per_units_by_script() {
    let formatter = |locale, unit, display| {
        NumberFormat::try_new(
            &[canonicalize(locale).unwrap()],
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
        formatter("zh", NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2吉字节"
    );
    assert_eq!(
        formatter("zh", NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2摄氏度"
    );
    assert_eq!(
        formatter("zh", NumberFormatUnit::Gigabyte, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2 GB"
    );
    assert_eq!(
        formatter(
            "zh",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2吉字节/秒"
    );

    assert_eq!(
        formatter("zh-TW", NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "攝氏 2 度"
    );
    assert_eq!(
        formatter(
            "zh-TW",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "每秒 2 GB"
    );
    assert_eq!(
        formatter(
            "zh-TW",
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "每秒 攝氏 2 度"
    );
    assert_eq!(
        formatter(
            "zh-TW",
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/秒"
    );
}

#[test]
fn sources_german_cldr_temperature_and_angle_widths() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("de").unwrap()],
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
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 Grad Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Fahrenheit, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2 °F"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1\u{a0}Gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 Gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2\u{a0}GB"
    );
    let per_second = formatter(
        NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
        NumberUnitDisplay::Long,
    );
    assert_eq!(
        per_second.format_f64(2.0).unwrap(),
        "2 Gigabyte pro Sekunde"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2\u{a0}GB/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 Grad Celsius pro Sekunde"
    );
}

#[test]
fn sources_portuguese_cldr_digital_temperature_and_percent_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("pt").unwrap()],
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
        "1 gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabytes"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 graus Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 por cento"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Bit, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2 bits"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabytes por segundo"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabytes por hora"
    );
    let european = NumberFormat::try_new(
        &[canonicalize("pt-PT").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("gigabyte-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(european.format_f64(2.0).unwrap(), "2 gigabytes/s");
    let african = NumberFormat::try_new(
        &[canonicalize("pt-AO").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("gigabyte-per-hour").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(african.format_f64(2.0).unwrap(), "2 gigabytes/h");
}

#[test]
fn sources_italian_cldr_digital_temperature_angle_and_percent_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("it").unwrap()],
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
        "2 gigabyte"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 grado Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gradi Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 percento"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte al secondo"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte all’ora"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/s"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-week").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2GB/sett."
    );
}

#[test]
fn sources_dutch_cldr_digital_temperature_angle_and_generic_per_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("nl").unwrap()],
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
        formatter(NumberFormatUnit::Bit, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 bit"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Bit, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 bits"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 graden Celsius"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2°"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 booggraden"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte per seconde"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/uur"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°/s"
    );
}

#[test]
fn sources_turkish_cldr_prefix_units_and_generic_per_units() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("tr").unwrap()],
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
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "yüzde 2"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "%2"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(1.0)
            .unwrap(),
        "1 °C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Narrow)
            .format_f64(1.0)
            .unwrap(),
        "1°C"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Narrow)
            .format_f64(2.0)
            .unwrap(),
        "2 °C"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabayt/saniye"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/sa"
    );
    let cypriot = NumberFormat::try_new(
        &[canonicalize("tr-CY").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("celsius-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(cypriot.format_f64(2.0).unwrap(), "2°C/sn");
}

#[test]
fn sources_hindi_devanagari_cldr_units_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("hi").unwrap()],
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
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 प्रतिशत"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Short)
            .format_f64(2.0)
            .unwrap(),
        "2°से॰"
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
        "2 गीगाबाइट प्रति सेकंड"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/घं॰"
    );
    let latin = NumberFormat::try_new(
        &[canonicalize("hi-Latn").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Gigabyte),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(latin.format_f64(2.0).unwrap(), "2 gigabytes");
}

#[test]
fn sources_greek_cldr_inflection_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("el").unwrap()],
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
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 βαθμός Κελσίου"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 βαθμοί Κελσίου"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabit, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabit"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Percent, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 τοις εκατό"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabyte ανά δευτερόλεπτο"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/ώ."
    );
    let cypriot = NumberFormat::try_new(
        &[canonicalize("el-CY").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("celsius-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Narrow,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(cypriot.format_f64(2.0).unwrap(), "2°C/δ");
}

#[test]
fn sources_polish_cldr_cardinal_forms_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("pl").unwrap()],
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
        "1 gigabajt"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "2 gigabajty"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(5.0)
            .unwrap(),
        "5 gigabajtów"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Long)
            .format_f64(1.5)
            .unwrap(),
        "1,5 gigabajta"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(5.0)
            .unwrap(),
        "5 stopni Celsjusza"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 gigabajty na sekundę"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-hour").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/godz."
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/s"
    );
}

#[test]
fn sources_hebrew_cldr_bidi_hidden_number_and_generic_per_patterns() {
    let formatter = |unit, display| {
        NumberFormat::try_new(
            &[canonicalize("he").unwrap()],
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
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "מעלה אחת"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Degree, NumberUnitDisplay::Long)
            .format_f64(2.0)
            .unwrap(),
        "שתי מעלות"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Celsius, NumberUnitDisplay::Long)
            .format_f64(1.0)
            .unwrap(),
        "1 מעלת צלזיוס"
    );
    assert_eq!(
        formatter(NumberFormatUnit::Gigabyte, NumberUnitDisplay::Short)
            .format_f64(1.0)
            .unwrap(),
        "GB\u{200f}1"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Long,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 ג׳יגה-בייט לשניה"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("gigabyte-per-second").unwrap(),
            NumberUnitDisplay::Short,
        )
        .format_f64(2.0)
        .unwrap(),
        "2 GB/שנ׳"
    );
    assert_eq!(
        formatter(
            NumberFormatUnit::parse("celsius-per-second").unwrap(),
            NumberUnitDisplay::Narrow,
        )
        .format_f64(2.0)
        .unwrap(),
        "2°C/שנ׳"
    );
}

#[test]
fn parses_compound_units_and_keeps_their_locale_pattern_part_boundaries() {
    let unit = NumberFormatUnit::parse("kilometer-per-hour").unwrap();
    assert!(unit.is_compound());
    assert_eq!(unit.identifier(), "kilometer-per-hour");
    assert_eq!(
        unit.compound_parts(),
        Some((NumberFormatUnit::Kilometer, NumberFormatUnit::Hour))
    );
    assert!(NumberFormatUnit::parse("meter-per-second").is_some());
    assert_eq!(NumberFormatUnit::parse("meter-per-per-second"), None);
    assert_eq!(NumberFormatUnit::parse("per-hour"), None);

    let format = NumberFormat::try_new(
        &[canonicalize("ko-KR").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(unit),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        format
            .format_to_parts_f64(-987.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Unit, "시속".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::MinusSign, "-".into()),
            (NumberFormatPartKind::Integer, "987".into()),
            (NumberFormatPartKind::Unit, "킬로미터".into()),
        ]
    );

    let generic = NumberFormat::try_new(
        &[canonicalize("en-US").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("meter-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(generic.format_f64(2.0).unwrap(), "2 meters per second");

    let french_generic = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("meter-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        french_generic.format_f64(2.0).unwrap(),
        "2\u{a0}mètres par seconde"
    );
    assert_eq!(
        french_generic
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, "\u{a0}".into()),
            (NumberFormatPartKind::Unit, "mètres par seconde".into(),),
        ]
    );

    let full_english_inventory = NumberFormat::try_new(
        &[canonicalize("en").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::parse("fluid-ounce-per-second").unwrap()),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(full_english_inventory.format_f64(2.0).unwrap(), "2 fl oz/s");
}

#[test]
fn loads_localized_simple_unit_patterns_from_the_shared_cldr_provider() {
    let french = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Meter),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(french.format_f64(2.0).unwrap(), "2\u{a0}mètres");
    assert_eq!(
        french
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, "\u{a0}".into()),
            (NumberFormatPartKind::Unit, "mètres".into()),
        ]
    );

    let german = NumberFormat::try_new(
        &[canonicalize("de").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Liter),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(german.format_f64(2.0).unwrap(), "2 l");

    let arabic = NumberFormat::try_new(
        &[canonicalize("ar-u-nu-arab").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Meter),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(arabic.format_f64(1.0).unwrap(), "متر");
    assert_eq!(
        arabic
            .format_to_parts_f64(1.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![(NumberFormatPartKind::Unit, "متر".into())]
    );
    assert_eq!(
        arabic
            .format_range_inputs(
                NumberFormatInput::Number(0.0),
                NumberFormatInput::Number(1.0),
            )
            .unwrap(),
        "٠–١ متر"
    );
    assert_eq!(
        arabic
            .format_range_inputs_to_parts(
                NumberFormatInput::Number(0.0),
                NumberFormatInput::Number(1.0),
            )
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value, part.source))
            .collect::<Vec<_>>(),
        vec![
            (
                NumberFormatPartKind::Integer,
                "٠".into(),
                NumberRangePartSource::StartRange,
            ),
            (
                NumberFormatPartKind::Literal,
                "–".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Integer,
                "١".into(),
                NumberRangePartSource::EndRange,
            ),
            (
                NumberFormatPartKind::Literal,
                " ".into(),
                NumberRangePartSource::Shared,
            ),
            (
                NumberFormatPartKind::Unit,
                "متر".into(),
                NumberRangePartSource::Shared,
            ),
        ]
    );
}

#[test]
fn loads_full_cldr_units_for_previously_untyped_locale_cells() {
    let afrikaans = NumberFormat::try_new(
        &[canonicalize("af-NA-u-nu-latn").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(NumberFormatUnit::Bit),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(afrikaans.format_f64(2.0).unwrap(), "2 bis");
    assert_eq!(
        afrikaans
            .format_to_parts_f64(2.0)
            .unwrap()
            .into_iter()
            .map(|part| (part.kind, part.value))
            .collect::<Vec<_>>(),
        vec![
            (NumberFormatPartKind::Integer, "2".into()),
            (NumberFormatPartKind::Literal, " ".into()),
            (NumberFormatPartKind::Unit, "bis".into()),
        ]
    );
}

#[test]
fn compound_unit_patterns_use_the_rounded_cldr_plural_category() {
    let unit = NumberFormatUnit::parse("kilometer-per-hour").unwrap();
    let russian = NumberFormat::try_new(
        &[canonicalize("ru").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(unit),
            unit_display: NumberUnitDisplay::Long,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(russian.format_f64(1.0).unwrap(), "1 километр в час");
    assert_eq!(russian.format_f64(2.0).unwrap(), "2 километра в час");
    assert_eq!(russian.format_f64(5.0).unwrap(), "5 километров в час");

    let french = NumberFormat::try_new(
        &[canonicalize("fr").unwrap()],
        NumberFormatOptions {
            style: NumberFormatStyle::Unit,
            unit: Some(unit),
            unit_display: NumberUnitDisplay::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(french.format_f64(2.0).unwrap(), "2\u{202f}km/h");
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
