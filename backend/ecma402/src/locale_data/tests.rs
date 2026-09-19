// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn pinned_cldr_display_names_cover_every_resolved_cldr_locale() {
    let provider = locale_data_provider();
    let locales = provider.number_format_resolved_locales();
    let mut cells = 0usize;

    for locale in &locales {
        assert!(
            display_names::has_locale(locale),
            "{locale} has no pinned DisplayNames locale group"
        );
        for style in [
            crate::DisplayNamesStyle::Long,
            crate::DisplayNamesStyle::Short,
            crate::DisplayNamesStyle::Narrow,
        ] {
            for field in [
                "era",
                "year",
                "quarter",
                "month",
                "weekOfYear",
                "weekday",
                "day",
                "dayPeriod",
                "hour",
                "minute",
                "second",
                "timeZoneName",
            ] {
                assert!(
                    display_names::display_name(
                        locale,
                        crate::DisplayNamesType::DateTimeField,
                        style,
                        None,
                        crate::DisplayNamesFallback::None,
                        field,
                    )
                    .is_some(),
                    "{locale} has no localized {style:?} {field} display name"
                );
                cells += 1;
            }
        }
    }

    assert_eq!(cells, locales.len() * 3 * 12);
}

#[test]
fn advertised_number_format_simple_unit_provider_matrix_is_total() {
    // Exercise the provider path itself, rather than only the formatter's
    // ICU4X typed-marker fast path. This keeps the raw CLDR supplements
    // and the bounded English compatibility result total for every
    // accepted API plural category.
    const DISPLAYS: [crate::NumberUnitDisplay; 3] = [
        crate::NumberUnitDisplay::Long,
        crate::NumberUnitDisplay::Short,
        crate::NumberUnitDisplay::Narrow,
    ];
    const PLURALS: [crate::PluralCategory; 6] = [
        crate::PluralCategory::Zero,
        crate::PluralCategory::One,
        crate::PluralCategory::Two,
        crate::PluralCategory::Few,
        crate::PluralCategory::Many,
        crate::PluralCategory::Other,
    ];

    let provider = locale_data_provider();
    let mut cells = 0usize;
    for &locale in provider.number_format_locales() {
        for &unit in crate::NumberFormatUnit::ALL {
            for display in DISPLAYS {
                for plural in PLURALS {
                    let (pattern, _) =
                        provider.number_unit_pattern_with_provenance(locale, unit, display, plural);
                    assert!(
                        !pattern.prefix.contains('\u{fdd0}')
                            && !pattern.suffix.contains('\u{fdd0}'),
                        "{locale} {} {display:?} {plural:?} leaked the private number placeholder",
                        unit.as_str()
                    );
                    cells += 1;
                }
            }
        }
    }
    assert_eq!(
        cells,
        provider.number_format_locales().len()
            * crate::NumberFormatUnit::ALL.len()
            * DISPLAYS.len()
            * PLURALS.len()
    );
}

#[test]
fn provider_fallback_tables_are_total_for_every_sanctioned_identifier() {
    use crate::{NumberFormatUnit, NumberUnitDisplay, PluralCategory};

    let provider = locale_data_provider();
    for &numbering_system in SUPPORTED_NUMBERING_SYSTEMS {
        let digits = provider
            .decimal_digits(numbering_system)
            .expect("every advertised numbering system has ten pinned digits");
        assert_eq!(digits.len(), 10, "{numbering_system}");
    }
    assert!(provider.decimal_digits("not-a-numbering-system").is_none());
    assert!(decimal_digit_array("012345678").is_none());
    assert!(decimal_digit_array("01234567890").is_none());

    let displays = [
        NumberUnitDisplay::Long,
        NumberUnitDisplay::Short,
        NumberUnitDisplay::Narrow,
    ];
    for &unit in NumberFormatUnit::ALL {
        for display in displays {
            for singular in [true, false] {
                let pattern = english_number_unit_pattern(unit, display, singular);
                assert!(
                    !pattern.suffix.is_empty(),
                    "{} {display:?} {singular}",
                    unit.as_str()
                );
            }
        }
    }
    let compound = NumberFormatUnit::parse("kilometer-per-hour").unwrap();
    assert_eq!(
        english_number_unit_pattern(compound, NumberUnitDisplay::Long, false).suffix,
        "compound"
    );

    let trailing_languages = [
        "ar", "be", "bg", "ca", "cs", "da", "de", "el", "es", "et", "fi", "fr", "he", "hr", "hu",
        "is", "it", "lt", "lv", "nl", "no", "pl", "pt", "ro", "ru", "sk", "sl", "sr", "sv", "tr",
        "uk",
    ];
    for language in trailing_languages {
        assert!(provider.fallback_percent_is_trailing(language));
    }
    assert!(!provider.fallback_percent_is_trailing("en-US"));
    assert!(provider.fallback_percent_has_space("fr-CA"));
    assert!(!provider.fallback_percent_has_space("ar"));
    assert!(!provider.fallback_percent_has_space("en"));

    assert!(provider
        .number_range_plural_category("en", PluralCategory::One, PluralCategory::Other)
        .is_some());
    assert!(provider
        .number_range_plural_category("not_a_locale", PluralCategory::One, PluralCategory::Other)
        .is_none());
}

#[test]
fn compound_and_pattern_splitters_preserve_complete_typed_data() {
    use crate::{NumberUnitDisplay, PluralCategory};

    let provider = locale_data_provider();
    for locale in [
        "de", "es", "fr", "it", "pt", "ru", "ar", "hi", "ja", "ko", "zh",
    ] {
        for display in [
            NumberUnitDisplay::Long,
            NumberUnitDisplay::Short,
            NumberUnitDisplay::Narrow,
        ] {
            for plural in [
                PluralCategory::One,
                PluralCategory::Few,
                PluralCategory::Other,
            ] {
                let pattern = provider
                    .number_compound_unit_pattern(locale, "kilometer", "hour", display, plural)
                    .expect("listed compatibility locale must provide its direct pattern");
                assert!(
                    !pattern.suffix.is_empty(),
                    "{locale} {display:?} {plural:?}"
                );
            }
        }
    }
    assert!(provider
        .number_compound_unit_pattern(
            "en",
            "kilometer",
            "hour",
            NumberUnitDisplay::Long,
            PluralCategory::Other,
        )
        .is_none());
    assert!(provider
        .number_compound_unit_pattern(
            "fr",
            "meter",
            "second",
            NumberUnitDisplay::Long,
            PluralCategory::Other,
        )
        .is_none());

    let currency_before =
        split_number_currency_pattern("-\u{fdd1}\u{a0}\u{fdd0}", "USD".into(), true).unwrap();
    assert!(matches!(
        currency_before.before_number.as_slice(),
        [NumberCurrencyPatternPiece::Sign, NumberCurrencyPatternPiece::Currency(code), NumberCurrencyPatternPiece::Literal(space)]
            if code == "USD" && space == "\u{a0}"
    ));
    assert!(currency_before.after_number.is_empty());
    let currency_after =
        split_number_currency_pattern("\u{fdd0}\u{a0}\u{fdd1}-", "EUR".into(), false).unwrap();
    assert!(matches!(
        currency_after.after_number.as_slice(),
        [NumberCurrencyPatternPiece::Literal(space), NumberCurrencyPatternPiece::Currency(code), NumberCurrencyPatternPiece::Sign]
            if code == "EUR" && space == "\u{a0}"
    ));
    assert!(split_number_currency_pattern("\u{fdd0}", "USD".into(), false).is_none());
    assert_eq!(
        currency_pattern_with_placeholders("¤#,##0.00"),
        Some("\u{fdd1}\u{fdd0}".into())
    );
    assert!(currency_pattern_with_placeholders("currency-only ¤").is_none());
    assert!(currency_pattern_with_placeholders("¤0 ¤").is_none());

    let percent = split_number_percent_pattern("\u{fdd1}\u{fdd0}\u{a0}%").unwrap();
    assert!(matches!(
        percent.before_number.as_slice(),
        [NumberPercentPatternPiece::Sign]
    ));
    assert!(matches!(
        percent.after_number.as_slice(),
        [NumberPercentPatternPiece::Literal(space), NumberPercentPatternPiece::PercentSign(sign)]
            if space == "\u{a0}" && sign == "%"
    ));
    assert!(split_number_percent_pattern("no number placeholder").is_none());
    assert!(split_number_percent_pattern("\u{fdd0}\u{fdd0}%").is_none());
    assert!(split_number_percent_pattern("\u{fdd0}\u{066a}").is_some());
}

#[test]
fn bidi_controls_and_static_helpers_cover_their_boundaries() {
    let split = NumberScientificSymbols::from_cldr("E".into(), "\u{200e}-\u{200f}".into());
    assert_eq!(split.exponent_minus_prefix, "\u{200e}");
    assert_eq!(split.exponent_minus_sign, "-");
    assert_eq!(split.exponent_minus_suffix, "\u{200f}");
    let all_controls = NumberScientificSymbols::from_cldr("×10".into(), "\u{200e}".into());
    assert_eq!(all_controls.exponent_minus_prefix, "");
    assert_eq!(all_controls.exponent_minus_sign, "\u{200e}");
    assert_eq!(all_controls.exponent_minus_suffix, "");
    for control in [
        '\u{61c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202e}', '\u{2066}', '\u{2069}',
    ] {
        assert!(is_bidi_control(control));
    }
    assert!(!is_bidi_control('-'));

    assert!(contains(&["a", "b"], "b"));
    assert!(!contains(&["a", "b"], "c"));
    assert!(str_eq("same", "same"));
    assert!(!str_eq("same", "different"));
    assert_eq!(canonical_time_zone("Etc/GMT0"), "UTC");
    assert_eq!(canonical_time_zone("America/Toronto"), "America/Toronto");
    assert_eq!(
        NumberFormatCoverageCount {
            data_backed: 1,
            total: 0,
        }
        .percentage(),
        0
    );
    assert_eq!(
        NumberFormatCoverageCount {
            data_backed: 3,
            total: 4,
        }
        .percentage(),
        75
    );
}
