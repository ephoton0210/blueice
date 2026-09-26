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
pub(crate) type DateTimeFormat = blueice_ecma402::DateTimeFormat;
pub(crate) type DisplayNames = blueice_ecma402::DisplayNames;
pub(crate) type DurationFormat = blueice_ecma402::DurationFormat;
pub(crate) type ListFormat = blueice_ecma402::ListFormat;
pub(crate) type NumberFormat = blueice_ecma402::NumberFormat;
pub(crate) type RelativeTimeFormat = blueice_ecma402::RelativeTimeFormat;
pub(crate) type Segmenter = blueice_ecma402::Segmenter;

/// ECMAScript-visible state that augments the ICU-backed plural-rule engine.
///
/// ICU selects categories, while the VM owns `GetOption` coercion and the
/// resolved option data prescribed by ECMA-402. Keeping those concerns here
/// ensures that the host service remains usable by non-JavaScript embedders.
pub(crate) struct PluralRules {
    pub data: blueice_ecma402::PluralRules,
    pub rule_type: blueice_ecma402::PluralRuleType,
    pub notation: String,
    pub compact_display: Option<String>,
    pub minimum_integer_digits: u8,
    pub minimum_fraction_digits: u8,
    pub maximum_fraction_digits: u8,
    pub minimum_significant_digits: Option<u8>,
    pub maximum_significant_digits: Option<u8>,
    pub rounding_increment: u16,
    pub rounding_mode: String,
    pub rounding_priority: String,
    pub trailing_zero_display: String,
}

impl PluralRules {
    pub fn bytes(&self) -> usize {
        self.data.bytes()
            + self.notation.len()
            + self.compact_display.as_ref().map_or(0, String::len)
            + self.rounding_mode.len()
            + self.rounding_priority.len()
            + self.trailing_zero_display.len()
    }
}

/// A materialized `%Segments%` result. ICU4X operates on scalar strings, but
/// the retained source uses original ECMAScript UTF-16 code units so records
/// expose lone surrogates without replacement.
pub(crate) struct Segments {
    pub input: JsString,
    pub records: Vec<SegmentRecord>,
}

#[derive(Clone, Copy)]
pub(crate) struct SegmentRecord {
    pub start: usize,
    pub end: usize,
    pub is_word_like: Option<bool>,
}

impl Segments {
    pub fn from_segmenter(segmenter: &Segmenter, input: JsString) -> Self {
        let projected = char::decode_utf16(input.as_code_units().iter().copied())
            .map(|scalar| scalar.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>();
        let segments = segmenter.segment(&projected);
        let mut records = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            let end = segments
                .get(index + 1)
                .map_or(input.as_code_units().len(), |next| next.index_utf16);
            records.push(SegmentRecord {
                start: segment.index_utf16,
                end,
                is_word_like: segment.is_word_like,
            });
        }
        Self { input, records }
    }

    pub fn record(&self, index: usize) -> Option<(JsString, usize, Option<bool>)> {
        let record = self.records.get(index)?;
        Some((
            JsString::from_code_units(
                self.input.as_code_units()[record.start..record.end].to_vec(),
            ),
            record.start,
            record.is_word_like,
        ))
    }

    pub fn containing(&self, index: usize) -> Option<usize> {
        self.records
            .iter()
            .position(|record| record.start <= index && index < record.end)
    }

    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.input.byte_len()
            + self.records.len() * std::mem::size_of::<SegmentRecord>()
    }
}

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

#[cfg(test)]
mod tests {
    use super::{case_map, SegmentRecord, Segments};
    use crate::{JsString, RuntimeError};

    #[test]
    fn segments_return_none_past_the_last_record() {
        let segments = Segments {
            input: JsString::from("a"),
            records: vec![SegmentRecord {
                start: 0,
                end: 1,
                is_word_like: None,
            }],
        };
        assert!(segments.record(1).is_none());
    }

    #[test]
    fn case_mapping_checks_the_limit_at_every_append_boundary() {
        let locale = "en".parse().unwrap();
        let lone_surrogate = JsString::from_code_units(vec![0xD800]);
        for input in [
            JsString::from("A"),
            lone_surrogate.clone(),
            JsString::from_code_units(vec![u16::from(b'A'), 0xD800]),
        ] {
            assert_eq!(
                case_map(&input, &locale, false, 0),
                Err(RuntimeError::StringLimit { limit: 0 })
            );
        }
    }
}
