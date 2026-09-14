// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public contract coverage for the shared, pinned ECMA-402 locale data.

use blueice_ecma402::{
    canonicalize, locale_data_provider, IntlService, LocaleDataCategory, ICU4X_LOCALE_DATA_REVISION,
};

#[test]
fn provider_exposes_one_pinned_registry_for_static_and_tzdb_data() {
    let provider = locale_data_provider();
    assert_eq!(provider.revision(), ICU4X_LOCALE_DATA_REVISION);
    assert_eq!(provider.revisions().icu4x, ICU4X_LOCALE_DATA_REVISION);
    assert_eq!(provider.revisions().tzdb, "2026c");
    assert_eq!(
        provider.values(LocaleDataCategory::Currency),
        ["EUR", "JPY", "USD"]
    );
    assert!(provider
        .values(LocaleDataCategory::Calendar)
        .contains(&"gregory"));
    assert!(provider
        .values(LocaleDataCategory::NumberingSystem)
        .contains(&"latn"));
    assert!(provider
        .values(LocaleDataCategory::Unit)
        .contains(&"kilometer"));

    let zones = provider.time_zones();
    assert!(zones.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(zones.contains(&"UTC".into()));
    assert!(!zones.contains(&"Etc/UTC".into()));
}

#[test]
fn provider_centralizes_language_and_datetime_defaults() {
    let provider = locale_data_provider();
    let persian = canonicalize("fa-IR").unwrap();
    let thai = canonicalize("th-TH").unwrap();
    let unsupported = canonicalize("zz").unwrap();

    assert!(provider.supports_language(persian.locale()));
    assert!(!provider.supports_language(unsupported.locale()));
    assert_eq!(provider.default_calendar(persian.locale()), "persian");
    assert_eq!(provider.default_calendar(thai.locale()), "buddhist");
    assert_eq!(
        provider.default_numbering_system(persian.locale()),
        "arabext"
    );
    assert_eq!(provider.default_hour_cycle(thai.locale()), "h23");
    assert!(provider.supports_numbering_system("latn"));
    assert!(!provider.supports_numbering_system("roman"));
}

#[test]
fn provider_owns_decimal_and_collation_data_availability() {
    let provider = locale_data_provider();
    let german = canonicalize("de-DE").unwrap();
    let chinese = canonicalize("zh-Hant-TW").unwrap();
    let unsupported = canonicalize("zz").unwrap();

    assert!(provider.supports_decimal_locale(german.locale()));
    assert!(!provider.supports_decimal_locale(unsupported.locale()));
    assert!(provider.supports_collation(german.locale(), "phonebk"));
    assert!(!provider.supports_collation(german.locale(), "zhuyin"));
    assert!(provider.supports_collation(chinese.locale(), "zhuyin"));
}

#[test]
fn provider_exposes_per_service_capabilities_without_negotiating_locales() {
    let provider = locale_data_provider();
    let datetime = provider.capabilities(IntlService::DateTimeFormat);
    assert!(datetime.uses_time_zone_data);
    assert_eq!(
        datetime.value_categories,
        [
            LocaleDataCategory::Calendar,
            LocaleDataCategory::NumberingSystem,
        ]
    );
    assert_eq!(
        provider
            .capabilities(IntlService::NumberFormat)
            .value_categories,
        [
            LocaleDataCategory::Currency,
            LocaleDataCategory::NumberingSystem,
            LocaleDataCategory::Unit,
        ]
    );
    assert!(provider.supports_service_locale(
        IntlService::DurationFormat,
        canonicalize("en-GB").unwrap().locale()
    ));
    assert!(!provider.supports_service_locale(
        IntlService::DurationFormat,
        canonicalize("fr").unwrap().locale()
    ));
}
