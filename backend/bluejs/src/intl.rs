// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ICU data and algorithms; observable ECMAScript conversions live in vm/intl.
use crate::{native, JsString, RuntimeError, Value};
use icu_locale_core::Locale as IcuLocale;

pub(crate) use blueice_ecma402::CanonicalLocale;

/// The [[Locale]] internal slot of an Intl.Locale instance. Keeping the
/// canonical ICU locale outside script-visible properties makes locale lists
/// immune to a user replacement of `toString`.
pub(crate) struct Locale {
    pub locale: CanonicalLocale,
}

impl Locale {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.locale.as_str().len()
    }
}

impl From<&Locale> for CanonicalLocale {
    fn from(locale: &Locale) -> Self {
        locale.locale.clone()
    }
}

pub(crate) type Collator = blueice_ecma402::Collator;
pub(crate) type NumberFormat = blueice_ecma402::NumberFormat;

pub(crate) fn canonicalize(string: &JsString) -> Result<CanonicalLocale, RuntimeError> {
    let invalid = || RuntimeError::RangeError("invalid Unicode locale identifier".into());
    let tag = string.to_utf8().map_err(|_| invalid())?;
    blueice_ecma402::canonicalize(&tag).map_err(|_| invalid())
}

pub(crate) fn keyword(locale: &IcuLocale, name: &str) -> Option<String> {
    blueice_ecma402::unicode_keyword(locale, name)
}

pub(crate) fn collate(collator: &Collator, left: &JsString, right: &JsString) -> Value {
    Value::Number(
        match collator.compare_utf16(left.as_code_units(), right.as_code_units()) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        },
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
