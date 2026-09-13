// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ICU data and algorithms; observable ECMAScript conversions live in vm/intl.
use crate::{native, JsString, RuntimeError, Value};
use icu_collator::CollatorBorrowed;
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
    let locale = blueice_ecma402::canonicalize(&tag).map_err(|_| invalid())?;
    let (locale, canonical) = locale.into_parts();
    Ok(CanonicalLocale::with_canonical(locale, canonical))
}

// The implementation's supported collation languages. Region/script subtags
// select ICU's CLDR fallback data. Unknown languages negotiate to en-US.
pub(crate) fn supported(locale: &IcuLocale) -> bool {
    blueice_ecma402::supports_collation_locale(locale)
}

pub(crate) fn keyword(locale: &IcuLocale, name: &str) -> Option<String> {
    blueice_ecma402::unicode_keyword(locale, name)
}

pub(crate) fn preferences(locale: &IcuLocale) -> icu_collator::CollatorPreferences {
    blueice_ecma402::collator_preferences(locale)
}

pub(crate) fn supports_collation(locale: &IcuLocale, collation: &str) -> bool {
    blueice_ecma402::supports_collation(locale, collation)
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
