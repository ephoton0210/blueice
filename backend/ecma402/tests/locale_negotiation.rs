// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral locale negotiation coverage.

use blueice_ecma402::{
    canonicalize, negotiate_collation_locale, resolve_collation_locale, resolve_locale,
    supported_collation_locales, supported_locales, IntlService, LocaleMatcher,
};

#[test]
fn retains_a_supported_canonical_request_and_ignores_unsupported_requests() {
    let requested = [
        canonicalize("zz").unwrap(),
        canonicalize("de-AT-u-co-phonebk").unwrap(),
        canonicalize("en").unwrap(),
    ];
    let trace = negotiate_collation_locale(&requested, LocaleMatcher::Lookup);

    assert_eq!(
        resolve_collation_locale(&requested, LocaleMatcher::Lookup).as_str(),
        "de-AT-u-co-phonebk"
    );
    assert_eq!(trace.matcher(), LocaleMatcher::Lookup);
    assert_eq!(trace.selected().as_str(), "de-AT-u-co-phonebk");
    assert!(!trace.used_default());
    assert_eq!(
        trace
            .candidates()
            .iter()
            .map(|candidate| (candidate.requested().as_str(), candidate.is_supported()))
            .collect::<Vec<_>>(),
        [("zz", false), ("de-AT-u-co-phonebk", true), ("en", true)]
    );
    assert_eq!(
        supported_collation_locales(&requested, LocaleMatcher::Lookup)
            .iter()
            .map(|locale| locale.as_str())
            .collect::<Vec<_>>(),
        ["de-AT-u-co-phonebk", "en"]
    );
}

#[test]
fn falls_back_to_the_stable_default_when_no_request_is_supported() {
    let requested = [canonicalize("zz").unwrap()];
    let trace = negotiate_collation_locale(&requested, LocaleMatcher::Lookup);

    assert_eq!(
        resolve_collation_locale(&requested, LocaleMatcher::Lookup).as_str(),
        "en-US"
    );
    assert_eq!(trace.selected().as_str(), "en-US");
    assert!(trace.used_default());
    assert!(supported_collation_locales(&requested, LocaleMatcher::Lookup).is_empty());
}

#[test]
fn best_fit_uses_the_advertised_lookup_policy() {
    let requested = [canonicalize("zz").unwrap(), canonicalize("sv-SE").unwrap()];

    assert_eq!(
        resolve_collation_locale(&requested, LocaleMatcher::BestFit).as_str(),
        resolve_collation_locale(&requested, LocaleMatcher::Lookup).as_str()
    );
    assert_eq!(
        supported_collation_locales(&requested, LocaleMatcher::BestFit)
            .iter()
            .map(|locale| locale.as_str())
            .collect::<Vec<_>>(),
        ["sv-SE"]
    );
}

#[test]
fn shared_resolver_carries_service_data_request_order_and_extensions() {
    let requested = [
        canonicalize("zz-u-nu-arab").unwrap(),
        canonicalize("es-MX-u-nu-arab").unwrap(),
        canonicalize("en").unwrap(),
    ];

    for service in [
        IntlService::Collator,
        IntlService::DateTimeFormat,
        IntlService::DisplayNames,
        IntlService::DurationFormat,
        IntlService::ListFormat,
        IntlService::NumberFormat,
        IntlService::PluralRules,
        IntlService::RelativeTimeFormat,
        IntlService::Segmenter,
    ] {
        let resolution = resolve_locale(service, &requested, LocaleMatcher::BestFit);
        assert_eq!(resolution.service(), service);
        assert_eq!(resolution.matcher(), LocaleMatcher::BestFit);
        assert_eq!(resolution.selected().as_str(), "es-MX-u-nu-arab");
        assert!(!resolution.used_default());
        assert_eq!(
            resolution
                .candidates()
                .iter()
                .map(|candidate| (candidate.requested().as_str(), candidate.is_supported()))
                .collect::<Vec<_>>(),
            [
                ("zz-u-nu-arab", false),
                ("es-MX-u-nu-arab", true),
                ("en", true),
            ]
        );
        assert_eq!(
            supported_locales(service, &requested, LocaleMatcher::Lookup)
                .iter()
                .map(|locale| locale.as_str())
                .collect::<Vec<_>>(),
            ["es-MX-u-nu-arab", "en"]
        );
    }
}
