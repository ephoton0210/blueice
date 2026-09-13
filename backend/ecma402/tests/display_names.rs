// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral `Intl.DisplayNames` coverage.

use blueice_ecma402::{
    canonicalize, supported_display_names_locales, DisplayNames, DisplayNamesError,
    DisplayNamesFallback, DisplayNamesLanguageDisplay, DisplayNamesOptions, DisplayNamesStyle,
    DisplayNamesType,
};

fn display_names(
    locale: &str,
    display_type: DisplayNamesType,
    style: DisplayNamesStyle,
    fallback: DisplayNamesFallback,
    language_display: DisplayNamesLanguageDisplay,
) -> DisplayNames {
    DisplayNames::try_new(
        &[canonicalize(locale).unwrap()],
        DisplayNamesOptions {
            locale_matcher: Default::default(),
            display_type,
            style,
            fallback,
            language_display,
        },
    )
    .unwrap()
}

#[test]
fn canonicalizes_codes_and_uses_localized_or_code_fallback_data() {
    let display_names = DisplayNames::try_new(
        &[canonicalize("en-US").unwrap()],
        DisplayNamesOptions {
            locale_matcher: Default::default(),
            display_type: DisplayNamesType::Language,
            style: DisplayNamesStyle::Long,
            fallback: DisplayNamesFallback::Code,
            language_display: DisplayNamesLanguageDisplay::Dialect,
        },
    )
    .unwrap();

    assert_eq!(display_names.of("fr").unwrap().as_deref(), Some("French"));
    assert_eq!(
        display_names.of("EN-us").unwrap().as_deref(),
        Some("American English")
    );
    assert_eq!(
        display_names.of("cde-ab-abcde").unwrap().as_deref(),
        Some("cde-AB-abcde")
    );
    assert_eq!(display_names.resolved_options().locale, "en-US");
}

#[test]
fn validates_type_specific_codes_and_honors_none_fallback() {
    let display_names = DisplayNames::try_new(
        &[],
        DisplayNamesOptions {
            locale_matcher: Default::default(),
            display_type: DisplayNamesType::Region,
            style: DisplayNamesStyle::Short,
            fallback: DisplayNamesFallback::None,
            language_display: Default::default(),
        },
    )
    .unwrap();

    assert_eq!(
        display_names.of("US").unwrap().as_deref(),
        Some("United States")
    );
    assert_eq!(display_names.of("ZZ").unwrap(), None);
    assert_eq!(
        display_names.of("U").unwrap_err(),
        DisplayNamesError::InvalidCode
    );

    let calendar = DisplayNames::try_new(
        &[],
        DisplayNamesOptions {
            locale_matcher: Default::default(),
            display_type: DisplayNamesType::Calendar,
            style: Default::default(),
            fallback: DisplayNamesFallback::Code,
            language_display: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(
        calendar.of("GREGORY").unwrap().as_deref(),
        Some("Gregorian Calendar")
    );
    assert_eq!(
        calendar.of("tw").unwrap_err(),
        DisplayNamesError::InvalidCode
    );
}

#[test]
fn resolves_every_bundled_name_type_and_parent_width_fallback() {
    let english = display_names(
        "en",
        DisplayNamesType::Language,
        DisplayNamesStyle::Long,
        DisplayNamesFallback::Code,
        DisplayNamesLanguageDisplay::Dialect,
    );
    for (code, expected) in [
        ("en", "English"),
        ("fr", "French"),
        ("de", "German"),
        ("es", "Spanish"),
        ("ja", "Japanese"),
        ("zh", "Chinese"),
        ("en-US", "American English"),
    ] {
        assert_eq!(english.of(code).unwrap().as_deref(), Some(expected));
    }
    assert_eq!(
        english.resolved_options().language_display,
        Some(DisplayNamesLanguageDisplay::Dialect)
    );
    assert!(english.bytes() > std::mem::size_of::<DisplayNames>());

    let standard = display_names(
        "en",
        DisplayNamesType::Language,
        DisplayNamesStyle::Short,
        DisplayNamesFallback::Code,
        DisplayNamesLanguageDisplay::Standard,
    );
    assert_eq!(standard.of("en-US").unwrap().as_deref(), Some("en-US"));
    assert_eq!(standard.of("fr").unwrap().as_deref(), Some("French"));

    let french = display_names(
        "fr",
        DisplayNamesType::Language,
        DisplayNamesStyle::Narrow,
        DisplayNamesFallback::Code,
        DisplayNamesLanguageDisplay::Dialect,
    );
    assert_eq!(french.of("en").unwrap().as_deref(), Some("anglais"));
    assert_eq!(french.of("fr").unwrap().as_deref(), Some("français"));

    for (display_type, code, expected) in [
        (DisplayNamesType::Region, "US", "United States"),
        (DisplayNamesType::Region, "GB", "United Kingdom"),
        (DisplayNamesType::Region, "FR", "France"),
        (DisplayNamesType::Region, "TW", "Taiwan"),
        (DisplayNamesType::Script, "Latn", "Latin"),
        (DisplayNamesType::Script, "Cyrl", "Cyrillic"),
        (DisplayNamesType::Currency, "USD", "US Dollar"),
        (DisplayNamesType::Currency, "EUR", "Euro"),
        (DisplayNamesType::Currency, "JPY", "Japanese Yen"),
        (DisplayNamesType::Calendar, "gregory", "Gregorian Calendar"),
        (DisplayNamesType::Calendar, "buddhist", "Buddhist Calendar"),
        (DisplayNamesType::DateTimeField, "year", "year"),
        (DisplayNamesType::DateTimeField, "month", "month"),
        (DisplayNamesType::DateTimeField, "day", "day"),
        (DisplayNamesType::DateTimeField, "hour", "hour"),
        (DisplayNamesType::DateTimeField, "minute", "minute"),
        (DisplayNamesType::DateTimeField, "second", "second"),
    ] {
        let names = display_names(
            "en",
            display_type,
            DisplayNamesStyle::Narrow,
            DisplayNamesFallback::Code,
            DisplayNamesLanguageDisplay::Standard,
        );
        assert_eq!(names.of(code).unwrap().as_deref(), Some(expected), "{code}");
        assert_eq!(
            names.resolved_options().language_display,
            (display_type == DisplayNamesType::Language)
                .then_some(DisplayNamesLanguageDisplay::Standard)
        );
    }
}

#[test]
fn validates_grammar_fallback_and_locale_negotiation_at_the_public_boundary() {
    let requested = [
        canonicalize("zz").unwrap(),
        canonicalize("fr").unwrap(),
        canonicalize("en").unwrap(),
    ];
    assert_eq!(
        supported_display_names_locales(&requested, Default::default()),
        vec![canonicalize("fr").unwrap(), canonicalize("en").unwrap()]
    );

    let fallback = display_names(
        "zz",
        DisplayNamesType::Language,
        DisplayNamesStyle::Long,
        DisplayNamesFallback::Code,
        DisplayNamesLanguageDisplay::Standard,
    );
    assert_eq!(fallback.resolved_options().locale, "en-US");
    assert_eq!(
        fallback.of("QAA-lAtN-419-1ABC-ABCDE").unwrap().as_deref(),
        Some("qaa-Latn-419-1abc-abcde")
    );

    let cases = [
        (DisplayNamesType::Language, "e"),
        (DisplayNamesType::Language, "en-a"),
        (DisplayNamesType::Language, "en-abcde-ABCDE"),
        (DisplayNamesType::Region, "U"),
        (DisplayNamesType::Region, "1234"),
        (DisplayNamesType::Script, "Lat"),
        (DisplayNamesType::Currency, "US1"),
        (DisplayNamesType::Calendar, "ab"),
        (DisplayNamesType::DateTimeField, "week"),
    ];
    for (display_type, invalid) in cases {
        let names = display_names(
            "en",
            display_type,
            DisplayNamesStyle::Long,
            DisplayNamesFallback::Code,
            DisplayNamesLanguageDisplay::Dialect,
        );
        assert_eq!(
            names.of(invalid),
            Err(DisplayNamesError::InvalidCode),
            "{invalid}"
        );
    }
    assert_eq!(
        DisplayNamesError::DataUnavailable.to_string(),
        "display-name data is unavailable"
    );
    assert_eq!(
        DisplayNamesError::InvalidCode.to_string(),
        "invalid display-name code"
    );
}
