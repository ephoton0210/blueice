// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral decimal `Intl.NumberFormat` coverage.

use blueice_ecma402::{
    canonicalize, resolve_number_format_locale, supported_number_format_locales, unicode_keyword,
    NumberFormat, NumberFormatError, NumberFormatOptions, NumberGrouping,
    ResolvedNumberFormatOptions,
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
            minimum_fraction_digits: 0,
            maximum_fraction_digits: 2,
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
    assert_eq!(
        formatter.format_f64(f64::INFINITY),
        Err(NumberFormatError::NonFiniteNumber)
    );
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
    ] {
        assert_eq!(error.to_string(), message);
    }
}
