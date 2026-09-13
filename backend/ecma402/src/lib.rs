// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The data-facing, host-neutral core of ECMA-402 internationalization.
//!
//! This crate owns locale identifier canonicalization and ICU-backed locale
//! data selection. ECMAScript object/Realm semantics, coercion, and UTF-16
//! handling intentionally remain in the embedding runtime.

use icu_collator::CollatorPreferences;
use icu_locale_core::Locale as IcuLocale;

/// An error from structurally validating and canonicalizing an ECMA-402
/// Unicode locale identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocaleError {
    /// The input is not a valid Unicode locale identifier for ECMA-402.
    InvalidLanguageTag,
}

impl std::fmt::Display for LocaleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageTag => formatter.write_str("invalid Unicode locale identifier"),
        }
    }
}

impl std::error::Error for LocaleError {}

/// A structurally valid ECMA-402 locale identifier together with the ICU
/// locale used to access locale data.
///
/// ICU4X deliberately does not represent BCP 47 primary-language subtags of
/// five to eight letters. Keeping the canonical name separately preserves
/// their ECMA-402-observable spelling without inventing an ICU language code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalLocale {
    locale: IcuLocale,
    canonical: String,
}

impl CanonicalLocale {
    fn new(locale: IcuLocale) -> Self {
        let canonical = locale.to_string();
        Self { locale, canonical }
    }

    fn with_canonical(locale: IcuLocale, canonical: impl Into<String>) -> Self {
        Self {
            locale,
            canonical: canonical.into(),
        }
    }

    /// Returns the ICU locale used for data lookup.
    pub fn locale(&self) -> &IcuLocale {
        &self.locale
    }

    /// Returns the ECMA-402 canonical identifier.
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// Splits the locale's data lookup representation from its canonical name.
    pub fn into_parts(self) -> (IcuLocale, String) {
        (self.locale, self.canonical)
    }
}

impl std::fmt::Display for CanonicalLocale {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.canonical.fmt(formatter)
    }
}

/// Canonicalizes an ECMA-402 Unicode locale identifier.
pub fn canonicalize(tag: &str) -> Result<CanonicalLocale, LocaleError> {
    let invalid = || LocaleError::InvalidLanguageTag;
    // ICU accepts underscores as separators and sorts/deduplicates variants.
    // ECMAScript requires hyphens and rejects repeated language variants.
    if tag.contains('_') {
        return Err(invalid());
    }
    let lower = tag.to_ascii_lowercase();
    let mut parts = lower.split('-');
    let language = parts.next().unwrap_or_default();
    if !matches!(language.len(), 2..=3 | 5..=8) {
        return Err(invalid());
    }
    let mut variants = std::collections::HashSet::new();
    for part in parts.take_while(|part| part.len() != 1) {
        if (part.len() >= 5 || part.len() == 4 && part.as_bytes()[0].is_ascii_digit())
            && !variants.insert(part)
        {
            return Err(invalid());
        }
    }

    // `posix` is structurally valid but not modelled by ICU4X. Preserve it for
    // ECMA-402 while querying ICU through its `und-posix` representation.
    let posix_language = tag.eq_ignore_ascii_case("posix");
    let mut parser_tag = if posix_language {
        "und-posix".to_owned()
    } else {
        tag.to_owned()
    };
    // ICU canonicalizes transformed-extension language tags and tfield order,
    // but its bundled aliases leave this UTS 35 tvalue untouched.
    let mut transformed = false;
    let mut subtags: Vec<_> = parser_tag.split('-').map(str::to_owned).collect();
    for index in 0..subtags.len() {
        if subtags[index].len() == 1 {
            transformed = subtags[index].eq_ignore_ascii_case("t");
            continue;
        }
        if transformed
            && subtags[index].eq_ignore_ascii_case("m0")
            && subtags
                .get(index + 1)
                .is_some_and(|value| value.eq_ignore_ascii_case("names"))
        {
            subtags[index + 1] = "prprname".into();
        }
    }
    parser_tag = subtags.join("-");

    let mut locale = IcuLocale::try_from_str(&parser_tag).map_err(|_| invalid())?;
    icu_locale::LocaleCanonicalizer::new_extended().canonicalize(&mut locale);
    canonicalize_unicode_keyword_aliases(&mut locale);

    Ok(if posix_language {
        CanonicalLocale::with_canonical(locale, "posix")
    } else {
        CanonicalLocale::new(locale)
    })
}

fn canonicalize_unicode_keyword_aliases(locale: &mut IcuLocale) {
    // ICU canonicalizes language identifiers but deliberately leaves several
    // Unicode keyword aliases to the consumer. ECMA-402 exposes their UTS 35
    // canonical spelling through Intl.Locale and Intl.getCanonicalLocales.
    let calendar: icu_locale_core::extensions::unicode::Key = "ca".parse().unwrap();
    if locale
        .extensions
        .unicode
        .keywords
        .get(&calendar)
        .is_some_and(|value| value.to_string() == "islamicc")
    {
        locale
            .extensions
            .unicode
            .keywords
            .set(calendar, "islamic-civil".parse().unwrap());
    }
    if locale
        .extensions
        .unicode
        .keywords
        .get(&calendar)
        .is_some_and(|value| value.to_string() == "ethiopic-amete-alem")
    {
        locale
            .extensions
            .unicode
            .keywords
            .set(calendar, "ethioaa".parse().unwrap());
    }

    // ICU intentionally preserves several CLDR aliases. ECMA-402 exposes
    // their canonical UTS 35 spellings, including boolean-key removal.
    for key_name in ["kb", "kc", "kh", "kk", "kn", "ks", "ms", "tz"] {
        let key = key_name.parse().unwrap();
        let Some(value) = locale
            .extensions
            .unicode
            .keywords
            .get(&key)
            .map(ToString::to_string)
        else {
            continue;
        };
        let replacement = match (key_name, value.as_str()) {
            ("kb" | "kc" | "kh" | "kk" | "kn", "yes") => None,
            ("ks", "primary") => Some("level1"),
            ("ks", "tertiary") => Some("level3"),
            ("ms", "imperial") => Some("uksystem"),
            ("tz", "cnckg") => Some("cnsha"),
            ("tz", "eire") => Some("iedub"),
            ("tz", "est") => Some("papty"),
            ("tz", "gmt0") => Some("gmt"),
            ("tz", "uct" | "zulu") => Some("utc"),
            _ => continue,
        };
        match replacement {
            Some(value) => {
                locale
                    .extensions
                    .unicode
                    .keywords
                    .set(key, value.parse().unwrap());
            }
            None => {
                // A canonical boolean `true` type is represented by a key
                // without a type (`-u-kn`, not an omitted `kn` key).
                locale
                    .extensions
                    .unicode
                    .keywords
                    .set(key, icu_locale_core::extensions::unicode::Value::default());
            }
        }
    }
}

/// Whether the bundled collation data supports the locale's language.
pub fn supports_collation_locale(locale: &IcuLocale) -> bool {
    const LANGUAGES: &str = "af am ar as az be bg bn bo br bs ca ceb chr cs cy da de dsb dz ee el en eo es et fa ff fi fil fo fr fy ga gl gu ha haw he hi hr hsb hu hy id ig is it ja ka kk kl km kn ko kok ku ky la lb lkt ln lo lt lv mk ml mn mr ms mt my nb ne nl nn no om or pa pl ps pt ro ru sa se si sk sl so sq sr sv sw ta te th tk to tr ug uk ur uz vi wae wo xh yi yo zh zu";
    LANGUAGES
        .split(' ')
        .any(|language| locale.id.language.as_str() == language)
}

/// Returns a Unicode extension keyword's canonical ICU value, if present.
pub fn unicode_keyword(locale: &IcuLocale, name: &str) -> Option<String> {
    let key: icu_locale_core::extensions::unicode::Key = name.parse().ok()?;
    locale
        .extensions
        .unicode
        .keywords
        .get(&key)
        .map(ToString::to_string)
}

/// Produces the ICU collation preferences represented by Unicode extensions.
pub fn collator_preferences(locale: &IcuLocale) -> CollatorPreferences {
    locale.into()
}

/// Whether a collation is available for an already-selected locale.
pub fn supports_collation(locale: &IcuLocale, collation: &str) -> bool {
    let language = locale.id.language.to_string();
    matches!(collation, "emoji" | "eor")
        || matches!(
            (language.as_str(), collation),
            ("de", "phonebk")
                | ("es" | "fi" | "sv" | "bn" | "kn", "trad")
                | ("zh", "pinyin" | "stroke" | "unihan" | "zhuyin")
                | ("ja" | "ko", "unihan")
                | ("ko", "searchjl")
                | ("si", "dict")
                | ("ar", "compat")
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_ecma402_aliases_without_a_vm() {
        for (input, expected) in [
            (
                "EN-latn-us-1901-u-ca-islamicc-kn-true",
                "en-Latn-US-1901-u-ca-islamic-civil-kn",
            ),
            ("en-u-ca-ethiopic-amete-alem", "en-u-ca-ethioaa"),
            ("und-u-ks-primary", "und-u-ks-level1"),
            ("und-u-ms-imperial", "und-u-ms-uksystem"),
            ("und-u-tz-eire", "und-u-tz-iedub"),
            (
                "und-Latn-t-und-hani-m0-names",
                "und-Latn-t-und-hani-m0-prprname",
            ),
            ("posix", "posix"),
        ] {
            assert_eq!(canonicalize(input).unwrap().as_str(), expected, "{input}");
        }
    }

    #[test]
    fn rejects_ecma402_invalid_locale_forms() {
        for input in ["en_US", "en-u-ca-gregory-u-nu-latn", "de-1901-1901"] {
            assert_eq!(canonicalize(input), Err(LocaleError::InvalidLanguageTag));
        }
    }

    #[test]
    fn supplies_collation_metadata_from_the_canonical_locale() {
        let de = canonicalize("de-u-co-phonebk").unwrap();
        assert!(supports_collation_locale(de.locale()));
        assert_eq!(
            unicode_keyword(de.locale(), "co").as_deref(),
            Some("phonebk")
        );
        assert!(supports_collation(de.locale(), "phonebk"));
        assert!(!supports_collation(de.locale(), "zhuyin"));
        let unsupported = canonicalize("zz").unwrap();
        assert!(!supports_collation_locale(unsupported.locale()));
    }
}
