// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ICU data and algorithms; observable ECMAScript conversions live in vm/intl.
use crate::{native, JsString, RuntimeError, Value};
use icu_collator::{CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale as IcuLocale;

/// The [[Locale]] internal slot of an Intl.Locale instance. Keeping the
/// canonical ICU locale outside script-visible properties makes locale lists
/// immune to a user replacement of `toString`.
pub(crate) struct Locale {
    pub locale: IcuLocale,
    pub canonical: String,
}

impl Locale {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.canonical.len()
    }
}

/// A structurally valid ECMA-402 locale identifier together with the ICU
/// locale used to supply data. ICU4X deliberately does not represent BCP 47
/// primary language subtags of five to eight letters, even though ECMA-402
/// accepts them. Keeping the canonical name separate lets those tags remain
/// observable without inventing an ICU language code.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CanonicalLocale {
    pub locale: IcuLocale,
    canonical: String,
}

impl CanonicalLocale {
    fn new(locale: IcuLocale) -> Self {
        let canonical = locale.to_string();
        Self { locale, canonical }
    }

    pub(crate) fn with_canonical(locale: IcuLocale, canonical: impl Into<String>) -> Self {
        Self {
            locale,
            canonical: canonical.into(),
        }
    }

    pub(crate) fn into_parts(self) -> (IcuLocale, String) {
        (self.locale, self.canonical)
    }
}

impl std::fmt::Display for CanonicalLocale {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.canonical.fmt(formatter)
    }
}

impl From<&Locale> for CanonicalLocale {
    fn from(locale: &Locale) -> Self {
        Self::with_canonical(locale.locale.clone(), locale.canonical.clone())
    }
}

pub(crate) struct Collator {
    pub algorithm: CollatorBorrowed<'static>,
    pub locale: String,
    pub usage: String,
    pub sensitivity: String,
    pub ignore_punctuation: bool,
    pub collation: String,
    pub german_search: bool,
}

impl Collator {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.locale.len()
            + self.usage.len()
            + self.sensitivity.len()
            + self.collation.len()
    }
    pub fn compare(&self, left: &JsString, right: &JsString) -> Value {
        let fold_german_search = |value: &JsString| {
            let mut units = Vec::with_capacity(value.as_code_units().len());
            for unit in value.as_code_units() {
                match unit {
                    0x00c4 => units.extend([b'A' as u16, b'E' as u16]),
                    0x00d6 => units.extend([b'O' as u16, b'E' as u16]),
                    0x00dc => units.extend([b'U' as u16, b'E' as u16]),
                    0x00df => units.extend([b's' as u16, b's' as u16]),
                    0x00e4 => units.extend([b'a' as u16, b'e' as u16]),
                    0x00f6 => units.extend([b'o' as u16, b'e' as u16]),
                    0x00fc => units.extend([b'u' as u16, b'e' as u16]),
                    unit => units.push(*unit),
                }
            }
            JsString::from_code_units(units)
        };
        let (left, right) = if self.german_search {
            (fold_german_search(left), fold_german_search(right))
        } else {
            (left.clone(), right.clone())
        };
        Value::Number(
            match self
                .algorithm
                .compare_utf16(left.as_code_units(), right.as_code_units())
            {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            },
        )
    }
}

pub(crate) fn canonicalize(string: &JsString) -> Result<CanonicalLocale, RuntimeError> {
    let invalid = || RuntimeError::RangeError("invalid Unicode locale identifier".into());
    let tag = string.to_utf8().map_err(|_| invalid())?;
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
    // ICU4X intentionally does not model the BCP 47 five-to-eight-letter
    // primary-language form. `posix` is structurally valid and retained by
    // ECMA-402, so use ICU's `und-posix` data representation while preserving
    // the canonical ECMA-402 spelling separately.
    let posix_language = tag.eq_ignore_ascii_case("posix");
    let mut parser_tag = if posix_language {
        "und-posix".to_string()
    } else {
        tag.clone()
    };
    // ICU canonicalizes transformed-extension language tags and tfield order,
    // but its bundled alias data leaves this UTS 35 tvalue untouched.
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
    // ICU canonicalizes language identifiers but deliberately leaves several
    // Unicode keyword aliases to the consumer. ECMA-402 exposes their UTS 35
    // canonical spelling through Locale and getCanonicalLocales.
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
    // their canonical UTS 35 spellings from both Intl.Locale and
    // Intl.getCanonicalLocales, including the special boolean-key removal.
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
    Ok(if posix_language {
        CanonicalLocale::with_canonical(locale, "posix")
    } else {
        CanonicalLocale::new(locale)
    })
}

// The implementation's supported collation languages. Region/script subtags
// select ICU's CLDR fallback data. Unknown languages negotiate to en-US.
pub(crate) fn supported(locale: &IcuLocale) -> bool {
    const LANGUAGES: &str = "af am ar as az be bg bn bo br bs ca ceb chr cs cy da de dsb dz ee el en eo es et fa ff fi fil fo fr fy ga gl gu ha haw he hi hr hsb hu hy id ig is it ja ka kk kl km kn ko kok ku ky la lb lkt ln lo lt lv mk ml mn mr ms mt my nb ne nl nn no om or pa pl ps pt ro ru sa se si sk sl so sq sr sv sw ta te th tk to tr ug uk ur uz vi wae wo xh yi yo zh zu";
    LANGUAGES
        .split(' ')
        .any(|language| locale.id.language.as_str() == language)
}

pub(crate) fn keyword(locale: &IcuLocale, name: &str) -> Option<String> {
    let key: icu_locale_core::extensions::unicode::Key = name.parse().unwrap();
    locale
        .extensions
        .unicode
        .keywords
        .get(&key)
        .map(|value| value.to_string())
}

pub(crate) fn preferences(locale: &IcuLocale) -> CollatorPreferences {
    locale.into()
}

pub(crate) fn supports_collation(locale: &IcuLocale, collation: &str) -> bool {
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

pub(crate) fn case_map(
    string: &JsString,
    locale: &IcuLocale,
    upper: bool,
    limit: usize,
) -> Result<JsString, RuntimeError> {
    let mapper = icu_casemap::CaseMapper::new();
    let mut result = JsString::default();
    let mut run = String::new();
    let flush = |run: &mut String, result: &mut JsString| -> Result<(), RuntimeError> {
        let mapped = if upper {
            mapper.uppercase_to_string(run, &locale.id)
        } else {
            mapper.lowercase_to_string(run, &locale.id)
        };
        native::append(result, &JsString::from(mapped.as_ref()), limit)?;
        run.clear();
        Ok(())
    };
    for scalar in char::decode_utf16(string.as_code_units().iter().copied()) {
        match scalar {
            Ok(c) => run.push(c),
            Err(error) => {
                flush(&mut run, &mut result)?;
                native::append(
                    &mut result,
                    &JsString::from_code_units(vec![error.unpaired_surrogate()]),
                    limit,
                )?;
            }
        }
    }
    flush(&mut run, &mut result)?;
    Ok(result)
}
