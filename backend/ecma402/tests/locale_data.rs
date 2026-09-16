// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public contract coverage for the shared, pinned ECMA-402 locale data.

use blueice_ecma402::{
    canonicalize, locale_data_provider, IntlService, LocaleDataCategory,
    ICU4X_LOCALE_DATA_REVISION, SUPPORTED_NUMBERING_SYSTEMS,
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
    let cantonese = canonicalize("yue").unwrap();
    let unsupported = canonicalize("zz").unwrap();

    assert!(provider.supports_decimal_locale(german.locale()));
    assert!(provider.supports_decimal_locale(cantonese.locale()));
    assert!(provider.supports_service_locale(IntlService::NumberFormat, cantonese.locale()));
    assert!(!provider.supports_service_locale(IntlService::DurationFormat, cantonese.locale()));
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
    assert!(provider.supports_service_locale(
        IntlService::DurationFormat,
        canonicalize("fr").unwrap().locale()
    ));
}

#[test]
fn advertised_number_format_locales_do_not_resolve_decimal_symbols_from_root() {
    let provider = locale_data_provider();
    let missing = provider
        .number_format_locales()
        .iter()
        .copied()
        .filter(|locale| {
            let locale = canonicalize(locale).expect("advertised locale is structurally valid");
            !provider.supports_decimal_locale(locale.locale())
        })
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "advertised decimal locales resolving through root data: {}",
        missing.join(", ")
    );
}

#[test]
fn former_direct_icu_decimal_locales_remain_backed_by_pinned_symbols() {
    // This is the direct-language inventory from the previous ICU4X baked
    // DecimalSymbols provider at the pinned revision. Keeping it verifies that
    // replacing that provider with pinned CLDR data does not narrow the
    // pre-existing NumberFormat service surface.
    const FORMER_DIRECT_LOCALES: &[&str] = &[
        "af", "ar", "as", "ast", "az", "ba", "be", "bg", "bgc", "bho", "blo", "bn", "br", "brx",
        "bs", "bua", "ca", "cs", "cv", "da", "de", "dsb", "ee", "el", "en", "eo", "es", "et", "eu",
        "fa", "ff", "fi", "fo", "fr", "fy", "gl", "gu", "he", "hi", "hr", "hsb", "ht", "hu", "hy",
        "ia", "id", "ie", "is", "it", "jv", "ka", "kea", "kgp", "kk", "kok", "ks", "ku", "kxv",
        "ky", "lb", "lij", "lmo", "lo", "lt", "lv", "mk", "ml", "mni", "mr", "ms", "my", "nds",
        "ne", "nl", "no", "nqo", "oc", "or", "pa", "pl", "pms", "ps", "pt", "qu", "raj", "rm",
        "ro", "ru", "rw", "sa", "sah", "sat", "sc", "scn", "sd", "sk", "sl", "sq", "sr", "su",
        "sv", "sw", "szl", "ta", "te", "tg", "tk", "tn", "tr", "tt", "tyv", "uk", "und", "ur",
        "uz", "vec", "vi", "vmw", "wo", "xh", "xnr", "yrl",
    ];

    let provider = locale_data_provider();
    let missing = FORMER_DIRECT_LOCALES
        .iter()
        .copied()
        .filter(|locale| {
            let locale = canonicalize(locale).expect("former ICU locale is structurally valid");
            !provider.supports_decimal_locale(locale.locale())
        })
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "former ICU direct decimal locales missing pinned symbols: {}",
        missing.join(", ")
    );
}

#[test]
fn number_format_provider_coverage_inventory_is_complete_and_localized() {
    let provider = locale_data_provider();
    let mut decimal_locales = 0;
    let mut incomplete_decimal_locales = Vec::new();
    let mut scientific_symbols = (0, 0);
    let mut compact_patterns = (0, 0);
    let mut currency_patterns = (0, 0);
    let mut percent_patterns = (0, 0);
    let mut simple_unit_patterns = (0, 0);
    let mut generic_compound_patterns = (0, 0);
    let mut incomplete_locales = Vec::new();

    for locale in provider.number_format_locales() {
        let coverage = provider
            .number_format_coverage(locale)
            .unwrap_or_else(|| panic!("advertised NumberFormat locale lacks coverage: {locale}"));
        decimal_locales += usize::from(coverage.decimal_symbols);
        if !coverage.decimal_symbols {
            incomplete_decimal_locales.push(*locale);
        }
        scientific_symbols.0 += coverage.scientific_symbols.data_backed;
        scientific_symbols.1 += coverage.scientific_symbols.total;
        compact_patterns.0 += coverage.compact_patterns.data_backed;
        compact_patterns.1 += coverage.compact_patterns.total;
        currency_patterns.0 += coverage.currency_patterns.data_backed;
        currency_patterns.1 += coverage.currency_patterns.total;
        percent_patterns.0 += coverage.percent_patterns.data_backed;
        percent_patterns.1 += coverage.percent_patterns.total;
        simple_unit_patterns.0 += coverage.simple_unit_patterns.data_backed;
        simple_unit_patterns.1 += coverage.simple_unit_patterns.total;
        generic_compound_patterns.0 += coverage.generic_compound_patterns.data_backed;
        generic_compound_patterns.1 += coverage.generic_compound_patterns.total;
        assert_eq!(
            coverage.generic_compound_patterns.total,
            coverage
                .simple_unit_patterns
                .total
                .saturating_mul(blueice_ecma402::NumberFormatUnit::ALL.len()),
            "generic compound matrix must include every numerator/denominator pair for {locale}"
        );
        if coverage.simple_unit_patterns.data_backed != coverage.simple_unit_patterns.total {
            incomplete_locales.push(format!(
                "{locale} {}/{}",
                coverage.simple_unit_patterns.data_backed, coverage.simple_unit_patterns.total
            ));
        }
    }

    assert_eq!(
        decimal_locales,
        provider.number_format_locales().len(),
        "NumberFormat decimal-symbol gaps by locale: {}",
        incomplete_decimal_locales.join(", ")
    );
    assert_eq!(currency_patterns.0, currency_patterns.1);
    assert_eq!(compact_patterns.0, compact_patterns.1);
    assert_eq!(percent_patterns.0, percent_patterns.1);
    assert_eq!(scientific_symbols.0, scientific_symbols.1);
    assert_eq!(
        scientific_symbols.1,
        provider
            .number_format_locales()
            .len()
            .saturating_mul(SUPPORTED_NUMBERING_SYSTEMS.len())
    );
    assert_eq!(simple_unit_patterns.0, simple_unit_patterns.1);
    assert_eq!(generic_compound_patterns.0, generic_compound_patterns.1);
    assert_eq!(
        generic_compound_patterns.1,
        simple_unit_patterns
            .1
            .saturating_mul(blueice_ecma402::NumberFormatUnit::ALL.len())
    );
    eprintln!(
        "NumberFormat provider coverage: decimal {decimal_locales}/{}, scientific symbols {}/{}, compact {}/{}, currency {}/{}, percent {}/{}, simple units {}/{} ({}%), generic compound denominators {}/{} ({}%)",
        provider.number_format_locales().len(),
        scientific_symbols.0,
        scientific_symbols.1,
        compact_patterns.0,
        compact_patterns.1,
        currency_patterns.0,
        currency_patterns.1,
        percent_patterns.0,
        percent_patterns.1,
        simple_unit_patterns.0,
        simple_unit_patterns.1,
        blueice_ecma402::NumberFormatCoverageCount {
            data_backed: simple_unit_patterns.0,
            total: simple_unit_patterns.1,
        }
        .percentage(),
        generic_compound_patterns.0,
        generic_compound_patterns.1,
        blueice_ecma402::NumberFormatCoverageCount {
            data_backed: generic_compound_patterns.0,
            total: generic_compound_patterns.1,
        }
        .percentage(),
    );
    assert!(
        incomplete_locales.is_empty(),
        "NumberFormat simple-unit gaps by locale: {}",
        incomplete_locales.join(", ")
    );
}
