// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ICU data and algorithms; observable ECMAScript conversions live in vm/intl.
use crate::{native, JsString, RuntimeError, Value};
use icu_collator::{CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale;

pub(crate) struct Collator {
    pub algorithm: CollatorBorrowed<'static>,
    pub locale: String,
    pub usage: String,
    pub sensitivity: String,
    pub ignore_punctuation: bool,
    pub collation: String,
}

impl Collator {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.locale.len() + self.usage.len() + self.sensitivity.len() + self.collation.len()
    }
    pub fn compare(&self, left: &JsString, right: &JsString) -> Value {
        Value::Number(match self.algorithm.compare_utf16(left.as_code_units(), right.as_code_units()) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        })
    }
}

pub(crate) fn canonicalize(string: &JsString) -> Result<Locale, RuntimeError> {
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
        if (part.len() >= 5 || part.len() == 4 && part.as_bytes()[0].is_ascii_digit()) && !variants.insert(part) {
            return Err(invalid());
        }
    }
    let mut locale = Locale::try_from_str(&tag).map_err(|_| invalid())?;
    icu_locale::LocaleCanonicalizer::new_extended().canonicalize(&mut locale);
    Ok(locale)
}

// The implementation's supported collation languages. Region/script subtags
// select ICU's CLDR fallback data. Unknown languages negotiate to en-US.
pub(crate) fn supported(locale: &Locale) -> bool {
    const LANGUAGES: &str = "af am ar as az be bg bn bo br bs ca ceb chr cs cy da de dsb dz ee el en eo es et fa ff fi fil fo fr fy ga gl gu ha haw he hi hr hsb hu hy id ig is it ja ka kk kl km kn ko kok ku ky la lb lkt ln lo lt lv mk ml mn mr ms mt my nb ne nl nn no om or pa pl ps pt ro ru sa se si sk sl so sq sr sv sw ta te th tk to tr ug uk ur uz vi wae wo xh yi yo zh zu";
    LANGUAGES.split(' ').any(|language| locale.id.language.as_str() == language)
}

pub(crate) fn keyword(locale: &Locale, name: &str) -> Option<String> {
    let key: icu_locale_core::extensions::unicode::Key = name.parse().unwrap();
    locale.extensions.unicode.keywords.get(&key).map(|value| value.to_string())
}

pub(crate) fn preferences(locale: &Locale) -> CollatorPreferences {
    locale.into()
}

pub(crate) fn supports_collation(locale: &Locale, collation: &str) -> bool {
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

pub(crate) fn case_map(string: &JsString, locale: &Locale, upper: bool, limit: usize) -> Result<JsString, RuntimeError> {
    let mapper = icu_casemap::CaseMapper::new();
    let mut result = JsString::default();
    let mut run = String::new();
    let flush = |run: &mut String, result: &mut JsString| -> Result<(), RuntimeError> {
        let mapped = if upper { mapper.uppercase_to_string(run, &locale.id) } else { mapper.lowercase_to_string(run, &locale.id) };
        native::append(result, &JsString::from(mapped.as_ref()), limit)?;
        run.clear();
        Ok(())
    };
    for scalar in char::decode_utf16(string.as_code_units().iter().copied()) {
        match scalar {
            Ok(c) => run.push(c),
            Err(error) => {
                flush(&mut run, &mut result)?;
                native::append(&mut result, &JsString::from_code_units(vec![error.unpaired_surrogate()]), limit)?;
            }
        }
    }
    flush(&mut run, &mut result)?;
    Ok(result)
}
