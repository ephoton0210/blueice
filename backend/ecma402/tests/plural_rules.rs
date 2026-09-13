// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral `Intl.PluralRules` category coverage.

use blueice_ecma402::{
    canonicalize, resolve_plural_rules_locale, supported_plural_rules_locales, PluralCategory,
    PluralRuleType, PluralRules, PluralRulesError, PluralRulesOptions, ResolvedPluralRulesOptions,
};

#[test]
fn selects_cardinal_categories_while_preserving_visible_fraction_digits() {
    let polish = canonicalize("pl").unwrap();
    let rules = PluralRules::try_new(&[polish], Default::default()).unwrap();

    assert_eq!(rules.select_decimal("1").unwrap(), PluralCategory::One);
    assert_eq!(rules.select_decimal("2").unwrap(), PluralCategory::Few);
    assert_eq!(rules.select_decimal("5").unwrap(), PluralCategory::Many);
    assert_eq!(rules.select_decimal("1.5").unwrap(), PluralCategory::Other);
    assert_eq!(
        rules.resolved_options(),
        &ResolvedPluralRulesOptions {
            locale: "pl".into(),
            rule_type: PluralRuleType::Cardinal,
        }
    );

    let english = canonicalize("en").unwrap();
    let english_rules = PluralRules::try_new(&[english], Default::default()).unwrap();
    assert_eq!(
        english_rules.select_decimal("1").unwrap(),
        PluralCategory::One
    );
    assert_eq!(
        english_rules.select_decimal("1.0").unwrap(),
        PluralCategory::Other
    );
    assert_eq!(english_rules.select_f64(1.0).unwrap(), PluralCategory::One);
    assert!(english_rules.select_f64(f64::NAN).is_err());
}

#[test]
fn selects_ordinal_categories_and_preserves_the_negotiation_trace() {
    let unavailable = canonicalize("zz").unwrap();
    let english = canonicalize("en-GB").unwrap();
    let rules = PluralRules::try_new(
        &[unavailable, english],
        PluralRulesOptions {
            rule_type: PluralRuleType::Ordinal,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(rules.select_decimal("1").unwrap(), PluralCategory::One);
    assert_eq!(rules.select_decimal("2").unwrap(), PluralCategory::Two);
    assert_eq!(rules.select_decimal("3").unwrap(), PluralCategory::Few);
    assert_eq!(rules.select_decimal("11").unwrap(), PluralCategory::Other);
    assert_eq!(rules.negotiation().selected().as_str(), "en-GB");
    assert!(!rules.negotiation().used_default());
    assert_eq!(
        rules
            .negotiation()
            .candidates()
            .iter()
            .map(|candidate| candidate.is_supported())
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn falls_back_to_the_stable_default_and_rejects_invalid_decimals() {
    let unavailable = canonicalize("zz").unwrap();
    let rules = PluralRules::try_new(&[unavailable], Default::default()).unwrap();

    assert_eq!(rules.negotiation().selected().as_str(), "en-US");
    assert!(rules.negotiation().used_default());
    assert_eq!(rules.select_decimal("1").unwrap(), PluralCategory::One);
    assert!(rules.select_decimal("not a decimal").is_err());
}

#[test]
fn carries_the_compact_exponent_and_supplemental_manx_rules_into_selection() {
    let french = canonicalize("fr").unwrap();
    let french_rules = PluralRules::try_new(&[french], Default::default()).unwrap();
    assert_eq!(
        french_rules.select_f64(1_500_000.0).unwrap(),
        PluralCategory::Other
    );
    assert_eq!(
        french_rules.select_compact_f64(1_500_000.0, false).unwrap(),
        PluralCategory::Many
    );
    assert_eq!(
        french_rules.select_compact_f64(1_500_000.0, true).unwrap(),
        PluralCategory::Many
    );

    let manx = canonicalize("gv").unwrap();
    let manx_rules = PluralRules::try_new(&[manx], Default::default()).unwrap();
    assert_eq!(manx_rules.select_f64(1.0).unwrap(), PluralCategory::One);
    assert_eq!(manx_rules.select_f64(2.0).unwrap(), PluralCategory::Two);
    assert_eq!(manx_rules.select_f64(20.0).unwrap(), PluralCategory::Few);
    assert_eq!(manx_rules.select_f64(3.0).unwrap(), PluralCategory::Other);
    assert_eq!(
        manx_rules.select_decimal("1.0").unwrap(),
        PluralCategory::Many
    );
}

#[test]
fn exposes_plural_locale_filters_errors_and_remaining_supplemental_paths() {
    let requested = [canonicalize("zz").unwrap(), canonicalize("ar").unwrap()];
    assert_eq!(
        resolve_plural_rules_locale(&requested, Default::default()).as_str(),
        "ar"
    );
    assert_eq!(
        supported_plural_rules_locales(&requested, Default::default())
            .iter()
            .map(|locale| locale.as_str())
            .collect::<Vec<_>>(),
        ["ar"]
    );

    let arabic = PluralRules::try_new(&[canonicalize("ar").unwrap()], Default::default()).unwrap();
    assert_eq!(arabic.select_f64(0.0).unwrap(), PluralCategory::Zero);
    assert!(arabic.bytes() > std::mem::size_of::<PluralRules>());

    let manx = PluralRules::try_new(&[canonicalize("gv").unwrap()], Default::default()).unwrap();
    assert_eq!(manx.select_decimal("-20").unwrap(), PluralCategory::Few);
    assert_eq!(
        manx.select_compact_f64(20.0, false).unwrap(),
        PluralCategory::Few
    );
    assert_eq!(
        manx.select_compact_f64(f64::INFINITY, true),
        Err(PluralRulesError::NonFiniteNumber)
    );
    for (error, message) in [
        (
            PluralRulesError::DataUnavailable,
            "plural-rule data is unavailable",
        ),
        (
            PluralRulesError::InvalidDecimal,
            "invalid finite decimal input",
        ),
        (PluralRulesError::NonFiniteNumber, "number must be finite"),
    ] {
        assert_eq!(error.to_string(), message);
    }
}
