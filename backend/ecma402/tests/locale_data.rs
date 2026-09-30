// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public contract coverage for the shared, pinned ECMA-402 locale data.

use blueice_ecma402::{
    canonicalize, locale_data_provider, DurationFormat, DurationFormatOptions, DurationRecord,
    IntlService, LocaleDataCategory, NumberFormat, NumberFormatOptions, NumberFormatPartKind,
    NumberFormatStyle, NumberFormatUnit, NumberUnitDisplay, PluralCategory, PluralRules,
    PluralRulesOptions, ICU4X_LOCALE_DATA_REVISION, SUPPORTED_NUMBERING_SYSTEMS,
};

#[test]
fn provider_exposes_one_pinned_registry_for_static_and_tzdb_data() {
    let provider = locale_data_provider();
    assert_eq!(provider.revision(), ICU4X_LOCALE_DATA_REVISION);
    assert_eq!(provider.revisions().icu4x, ICU4X_LOCALE_DATA_REVISION);
    assert_eq!(provider.revisions().tzdb, "2026c");
    let currencies = provider.values(LocaleDataCategory::Currency);
    assert_eq!(currencies.len(), 307);
    assert!(currencies.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(currencies.contains(&"AFA"));
    assert!(currencies.contains(&"XCG"));
    assert!(currencies.contains(&"XXX"));
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
    assert!(provider.supports_service_locale(IntlService::DurationFormat, cantonese.locale()));
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
fn resolved_cldr_number_and_duration_locale_inventory_is_complete() {
    let provider = locale_data_provider();
    let locales = provider.number_format_resolved_locales();
    assert_eq!(locales.len(), 766);
    assert!(locales.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(locales.contains(&"ak".into()));
    assert!(locales.contains(&"fr-CA".into()));
    assert!(locales.contains(&"zh-Hant".into()));
    for locale_name in locales {
        let locale = canonicalize(&locale_name).expect("pinned CLDR locale is structurally valid");
        assert!(
            provider.supports_service_locale(IntlService::NumberFormat, locale.locale()),
            "NumberFormat lacks pinned decimal data for {locale_name}"
        );
        assert!(
            provider.supports_service_locale(IntlService::DurationFormat, locale.locale()),
            "DurationFormat lacks shared pinned unit data for {locale_name}"
        );
    }
}

#[test]
fn every_resolved_duration_locale_constructs_and_formats_with_pinned_data() {
    let provider = locale_data_provider();
    let record = DurationRecord::try_new(1, 2, 0, 0, 0, 0, 0, 0, 0, 0).unwrap();
    for locale_name in provider.number_format_resolved_locales() {
        let locale = canonicalize(&locale_name).expect("pinned CLDR locale is structurally valid");
        let formatter = DurationFormat::try_new(
            std::slice::from_ref(&locale),
            DurationFormatOptions::default(),
        )
        .unwrap_or_else(|error| {
            panic!("DurationFormat data unavailable for {locale_name}: {error}")
        });
        assert_eq!(formatter.resolved_options().locale, locale_name);
        assert!(
            !formatter.format(record).unwrap().is_empty(),
            "DurationFormat produced an empty result for {locale_name}"
        );
    }
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
    let locales = provider.number_format_resolved_locales();
    let mut decimal_locales = 0;
    let mut incomplete_decimal_locales = Vec::new();
    let mut scientific_symbols = (0, 0);
    let mut compact_patterns = (0, 0);
    let mut currency_patterns = (0, 0);
    let mut percent_patterns = (0, 0);
    let mut simple_unit_patterns = (0, 0);
    let mut generic_compound_patterns = (0, 0);
    let mut incomplete_locales = Vec::new();

    for locale in &locales {
        let coverage = provider
            .number_format_coverage(locale)
            .unwrap_or_else(|| panic!("advertised NumberFormat locale lacks coverage: {locale}"));
        decimal_locales += usize::from(coverage.decimal_symbols);
        if !coverage.decimal_symbols {
            incomplete_decimal_locales.push(locale.clone());
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
        locales.len(),
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
            .number_format_resolved_locales()
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
        locales.len(),
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

#[test]
fn every_advertised_simple_unit_cell_formats_as_a_typed_unit() {
    // The provider inventory above proves that every advertised raw cell is
    // declared data-backed. Exercise those cells through the public formatter
    // as well: a data row is only useful when its locale, plural selection,
    // pattern parsing, and typed `formatToParts` boundary all remain usable.
    // Integer candidates cover CLDR's integer cardinal families; the decimal
    // candidate additionally reaches families (such as Manx) with a distinct
    // fractional category.
    let provider = locale_data_provider();
    let mut exercised_cells = 0usize;
    for &locale_name in provider.number_format_locales() {
        let locale = canonicalize(locale_name).expect("advertised locale is structurally valid");
        let plural_rules =
            PluralRules::try_new(std::slice::from_ref(&locale), PluralRulesOptions::default())
                .expect("advertised NumberFormat locale has cardinal rules");
        let mut representatives = Vec::<(PluralCategory, f64)>::new();
        for candidate in (0..=200).map(f64::from).chain([1_000.0, 1_000_000.0, 0.5]) {
            let category = plural_rules
                .select_f64(candidate)
                .expect("finite plural candidate is valid");
            if !representatives
                .iter()
                .any(|(existing, _)| *existing == category)
            {
                representatives.push((category, candidate));
            }
        }

        for unit in NumberFormatUnit::ALL {
            for unit_display in [
                NumberUnitDisplay::Long,
                NumberUnitDisplay::Short,
                NumberUnitDisplay::Narrow,
            ] {
                let formatter = NumberFormat::try_new(
                    std::slice::from_ref(&locale),
                    NumberFormatOptions {
                        style: NumberFormatStyle::Unit,
                        unit: Some(*unit),
                        unit_display,
                        ..Default::default()
                    },
                )
                .unwrap_or_else(|error| {
                    panic!("{locale_name} {} {unit_display:?}: {error}", unit.as_str())
                });
                for &(category, value) in &representatives {
                    let parts = formatter
                        .format_to_parts_f64(value)
                        .unwrap_or_else(|error| {
                            panic!(
                                "{locale_name} {} {unit_display:?} {category:?}: {error}",
                                unit.as_str()
                            )
                        });
                    assert!(
                        parts.iter().any(|part| part.kind == NumberFormatPartKind::Unit),
                        "{locale_name} {} {unit_display:?} {category:?} omitted its typed unit part",
                        unit.as_str()
                    );
                    exercised_cells += 1;
                }
            }
        }
    }
    assert!(
        exercised_cells > 0,
        "provider has advertised simple-unit cells"
    );
}
