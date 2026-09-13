// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral `Intl.PluralRules` category coverage.

use blueice_ecma402::{
    canonicalize, PluralCategory, PluralRuleType, PluralRules, PluralRulesOptions,
    ResolvedPluralRulesOptions,
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
