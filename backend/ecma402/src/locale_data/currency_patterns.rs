// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete pinned CLDR currency names, symbols, patterns, and spacing.
//!
//! The compressed data is generated from every `cldr-numbers-full` 48.2.1
//! resolved locale's `currencies.json` and `numbers.json`
//! `currencyFormats-numberSystem-*` records (commit
//! `26a79cb42bfcc90def764102aa2af126d9ef3108`).  Currency name maps are
//! structurally delta-compressed against identical or nearest preceding CLDR
//! maps; this is a storage encoding only, not a locale fallback policy.
//! The derived data is distributed under Unicode License V3; see
//! `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::Read, sync::OnceLock};

const FIELD_SEPARATOR: char = '\u{1d}';
const GROUP_SEPARATOR: char = '\u{1e}';
const ALIAS_SEPARATOR: char = '\u{1f}';
const PATTERN_SECTION: &str = "--currency-patterns--";
const CLDR_RESOLVED_LOCALE_COUNT: usize = 766;
const CLDR_CURRENCY_NAME_TEMPLATE_COUNT: usize = 444;
const CLDR_CURRENCY_PATTERN_COUNT: usize = 910;
const PLURAL_COUNT: usize = 6;

#[derive(Debug)]
pub(super) struct CurrencyName {
    pub(super) symbol: String,
    pub(super) narrow_symbol: String,
    pub(super) display_name: String,
    pub(super) plural_names: [String; PLURAL_COUNT],
}

#[derive(Debug)]
struct CurrencyNameTemplate {
    base: Option<usize>,
    changes: HashMap<String, Option<CurrencyName>>,
}

#[derive(Debug)]
struct CurrencyFormatPattern {
    locale: String,
    numbering_system: String,
    standard: String,
    standard_alpha: String,
    accounting: String,
    accounting_alpha: String,
    before_currency_spacing: String,
    after_currency_spacing: String,
    unit_patterns: [String; PLURAL_COUNT],
}

struct PinnedCurrencyData {
    names: Vec<CurrencyNameTemplate>,
    locale_indices: HashMap<String, usize>,
    patterns: Vec<CurrencyFormatPattern>,
}

static PINNED_CURRENCY_DATA: OnceLock<PinnedCurrencyData> = OnceLock::new();

fn pinned_currency_data() -> &'static PinnedCurrencyData {
    PINNED_CURRENCY_DATA.get_or_init(|| {
        let encoded = include_str!("currency_patterns_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR currency data must be valid base64");
        let mut tsv = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut tsv)
            .expect("embedded CLDR currency data must be valid gzip");
        let (name_rows, pattern_rows) = tsv
            .split_once(&format!("\n{PATTERN_SECTION}\n"))
            .expect("embedded CLDR currency data must separate names and patterns");
        let mut names = Vec::new();
        let mut locale_indices = HashMap::new();
        for row in name_rows.lines() {
            let mut fields = row.splitn(3, '\t');
            let aliases = fields
                .next()
                .expect("embedded currency-name row has aliases");
            let base = fields.next().expect("embedded currency-name row has base");
            let raw_changes = fields
                .next()
                .expect("embedded currency-name row has changes");
            let index = names.len();
            let base = (!base.is_empty()).then(|| {
                let base = base
                    .parse::<usize>()
                    .expect("embedded currency-name base must be an index");
                assert!(
                    base < index,
                    "embedded currency-name delta base must precede its child"
                );
                base
            });
            let mut changes = HashMap::new();
            if !raw_changes.is_empty() {
                for change in raw_changes.split(GROUP_SEPARATOR) {
                    let mut fields = change.split(FIELD_SEPARATOR);
                    let code = fields
                        .next()
                        .expect("embedded currency-name change has code");
                    let first = fields
                        .next()
                        .expect("embedded currency-name change has symbol");
                    let value = if first == "\0" {
                        assert!(
                            fields.next().is_none(),
                            "deleted currency-name record must have no further fields"
                        );
                        None
                    } else {
                        let mut values = vec![first.into()];
                        values.extend(fields.map(str::to_owned));
                        assert_eq!(
                            values.len(),
                            9,
                            "embedded currency-name record must have nine value fields"
                        );
                        let plural_names = std::array::from_fn(|index| values[index + 3].clone());
                        Some(CurrencyName {
                            symbol: values[0].clone(),
                            narrow_symbol: values[1].clone(),
                            display_name: values[2].clone(),
                            plural_names,
                        })
                    };
                    assert!(
                        changes.insert(code.into(), value).is_none(),
                        "embedded currency-name delta must not duplicate a code"
                    );
                }
            }
            for alias in aliases.split(ALIAS_SEPARATOR) {
                assert!(
                    locale_indices
                        .insert(alias.to_ascii_lowercase(), index)
                        .is_none(),
                    "embedded currency-name table must not duplicate a locale alias"
                );
            }
            names.push(CurrencyNameTemplate { base, changes });
        }
        assert_eq!(
            names.len(),
            CLDR_CURRENCY_NAME_TEMPLATE_COUNT,
            "embedded CLDR currency-name template count must remain pinned"
        );
        assert_eq!(
            locale_indices.len(),
            CLDR_RESOLVED_LOCALE_COUNT,
            "embedded CLDR currency-name aliases must retain every resolved locale"
        );
        let patterns = pattern_rows
            .lines()
            .map(parse_pattern_row)
            .collect::<Vec<_>>();
        assert_eq!(
            patterns.len(),
            CLDR_CURRENCY_PATTERN_COUNT,
            "embedded CLDR currency-pattern record count must remain pinned"
        );
        PinnedCurrencyData {
            names,
            locale_indices,
            patterns,
        }
    })
}

fn parse_pattern_row(row: &str) -> CurrencyFormatPattern {
    let mut fields = row.splitn(14, '\t');
    let locale = fields
        .next()
        .expect("embedded currency-pattern row has locale")
        .into();
    let numbering_system = fields
        .next()
        .expect("embedded currency-pattern row has numbering system")
        .into();
    let standard = fields
        .next()
        .expect("embedded currency-pattern row has standard pattern")
        .into();
    let standard_alpha = fields
        .next()
        .expect("embedded currency-pattern row has standard alpha pattern")
        .into();
    let accounting = fields
        .next()
        .expect("embedded currency-pattern row has accounting pattern")
        .into();
    let accounting_alpha = fields
        .next()
        .expect("embedded currency-pattern row has accounting alpha pattern")
        .into();
    let before_currency_spacing = fields
        .next()
        .expect("embedded currency-pattern row has before-currency spacing")
        .into();
    let after_currency_spacing = fields
        .next()
        .expect("embedded currency-pattern row has after-currency spacing")
        .into();
    let unit_patterns = std::array::from_fn(|_| {
        fields
            .next()
            .expect("embedded currency-pattern row has plural unit pattern")
            .into()
    });
    assert!(
        fields.next().is_none(),
        "embedded currency-pattern row must have exactly fourteen fields"
    );
    CurrencyFormatPattern {
        locale,
        numbering_system,
        standard,
        standard_alpha,
        accounting,
        accounting_alpha,
        before_currency_spacing,
        after_currency_spacing,
        unit_patterns,
    }
}

fn locale_candidates(locale: &str) -> Vec<String> {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut candidates = vec![base.to_owned()];
    if let Ok(canonical) = crate::canonicalize(base) {
        let maximal = crate::locale_data_provider()
            .maximize_likely_subtags(canonical.locale())
            .to_string();
        let mut candidate = maximal.as_str();
        loop {
            if !candidates
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(candidate))
            {
                candidates.push(candidate.into());
            }
            let Some((parent, _)) = candidate.rsplit_once('-') else {
                break;
            };
            candidate = parent;
        }
    }
    let mut candidate = base;
    while let Some((parent, _)) = candidate.rsplit_once('-') {
        if !candidates
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(parent))
        {
            candidates.push(parent.into());
        }
        candidate = parent;
    }
    candidates
}

fn locale_template(data: &PinnedCurrencyData, locale: &str) -> Option<usize> {
    locale_candidates(locale).into_iter().find_map(|candidate| {
        data.locale_indices
            .get(&candidate.to_ascii_lowercase())
            .copied()
    })
}

fn name_record_for_locale<'a>(
    data: &'a PinnedCurrencyData,
    locale: &str,
    code: &str,
) -> Option<&'a CurrencyName> {
    locale_candidates(locale).into_iter().find_map(|candidate| {
        data.locale_indices
            .get(&candidate.to_ascii_lowercase())
            .and_then(|template| name_record(data, *template, code))
    })
}

fn name_record<'a>(
    data: &'a PinnedCurrencyData,
    template: usize,
    code: &str,
) -> Option<&'a CurrencyName> {
    let template = data.names.get(template)?;
    match template.changes.get(code) {
        Some(Some(record)) => Some(record),
        Some(None) => None,
        None => template.base.and_then(|base| name_record(data, base, code)),
    }
}

fn plural_index(plural: crate::PluralCategory) -> usize {
    match plural {
        crate::PluralCategory::Zero => 0,
        crate::PluralCategory::One => 1,
        crate::PluralCategory::Two => 2,
        crate::PluralCategory::Few => 3,
        crate::PluralCategory::Many => 4,
        crate::PluralCategory::Other => 5,
    }
}

fn pattern_record<'a>(
    data: &'a PinnedCurrencyData,
    locale: &str,
    numbering_system: &str,
) -> Option<&'a CurrencyFormatPattern> {
    for candidate in locale_candidates(locale) {
        if let Some(pattern) = data.patterns.iter().find(|pattern| {
            pattern.locale.eq_ignore_ascii_case(&candidate)
                && pattern.numbering_system == numbering_system
        }) {
            return Some(pattern);
        }
        if numbering_system != "latn" {
            if let Some(pattern) = data.patterns.iter().find(|pattern| {
                pattern.locale.eq_ignore_ascii_case(&candidate)
                    && pattern.numbering_system == "latn"
            }) {
                return Some(pattern);
            }
        }
    }
    None
}

fn starts_or_ends_with_letter(value: &str) -> (bool, bool) {
    (
        value.chars().next().is_some_and(char::is_alphabetic),
        value.chars().next_back().is_some_and(char::is_alphabetic),
    )
}

/// Resolves the short/narrow symbol and its alpha-adjacency classification.
/// Missing CLDR symbols deliberately use the ISO code, as ECMA-402 requires.
pub(super) fn currency_symbol(
    locale: &str,
    code: &str,
    display: crate::NumberCurrencyDisplay,
) -> (String, bool, bool) {
    let data = pinned_currency_data();
    let record = name_record_for_locale(data, locale, code);
    let value = match display {
        crate::NumberCurrencyDisplay::Symbol => record
            .map(|record| record.symbol.as_str())
            .filter(|value| !value.is_empty()),
        crate::NumberCurrencyDisplay::NarrowSymbol => record.and_then(|record| {
            (!record.narrow_symbol.is_empty())
                .then_some(record.narrow_symbol.as_str())
                .or_else(|| (!record.symbol.is_empty()).then_some(record.symbol.as_str()))
        }),
        crate::NumberCurrencyDisplay::Code | crate::NumberCurrencyDisplay::Name => None,
    }
    .unwrap_or(code);
    let (starts_with_letter, ends_with_letter) = starts_or_ends_with_letter(value);
    (value.into(), starts_with_letter, ends_with_letter)
}

/// Resolves a plural-sensitive currency display name. A missing localized
/// name is not an English compatibility fallback: the prescribed ISO code is
/// retained instead.
pub(super) fn currency_name(locale: &str, code: &str, plural: crate::PluralCategory) -> String {
    let data = pinned_currency_data();
    let record = name_record_for_locale(data, locale, code);
    record
        .and_then(|record| {
            record
                .plural_names
                .get(plural_index(plural))
                .filter(|name| !name.is_empty())
                .or_else(|| {
                    (!record.plural_names[PLURAL_COUNT - 1].is_empty())
                        .then_some(&record.plural_names[PLURAL_COUNT - 1])
                })
                .or_else(|| (!record.display_name.is_empty()).then_some(&record.display_name))
        })
        .filter(|name| !name.is_empty())
        .cloned()
        .unwrap_or_else(|| code.into())
}

/// Returns the selected standard/accounting pattern and whether its negative
/// subpattern owns the decimal minus sign.
pub(super) fn currency_pattern(
    locale: &str,
    numbering_system: &str,
    accounting: bool,
    starts_with_letter: bool,
    ends_with_letter: bool,
    negative: bool,
) -> Option<(String, bool)> {
    let pattern = pattern_record(pinned_currency_data(), locale, numbering_system)?;
    let ordinary = if accounting {
        &pattern.accounting
    } else {
        &pattern.standard
    };
    let number = ordinary.find(['0', '#'])?;
    let currency = ordinary.find('¤')?;
    let currency_before_number = currency < number;
    // CLDR's alpha-next-to-number form is controlled by the currency
    // character adjacent to the number, not by any letter elsewhere in a
    // multi-character symbol. `US$3` therefore retains its ordinary Thai
    // pattern, whereas `USD 3` selects the before-currency spacing record.
    let alpha_adjacent = if currency_before_number {
        ends_with_letter && !pattern.before_currency_spacing.is_empty()
    } else {
        starts_with_letter && !pattern.after_currency_spacing.is_empty()
    };
    let base = match (accounting, alpha_adjacent) {
        (false, false) => &pattern.standard,
        (false, true) if !pattern.standard_alpha.is_empty() => &pattern.standard_alpha,
        (true, false) => &pattern.accounting,
        (true, true) if !pattern.accounting_alpha.is_empty() => &pattern.accounting_alpha,
        (true, _) => &pattern.accounting,
        (false, _) => &pattern.standard,
    };
    let mut subpatterns = base.split(';');
    let positive = subpatterns.next().filter(|value| !value.is_empty())?;
    let negative_pattern = subpatterns.next().filter(|value| !value.is_empty());
    if negative {
        if let Some(negative_pattern) = negative_pattern {
            return Some((negative_pattern.into(), true));
        }
    }
    Some((positive.into(), false))
}

/// Resolves the CLDR currency-name number/unit pattern for the rounded
/// display plural category.
pub(super) fn currency_name_pattern(
    locale: &str,
    numbering_system: &str,
    plural: crate::PluralCategory,
) -> Option<String> {
    let pattern = pattern_record(pinned_currency_data(), locale, numbering_system)?;
    pattern
        .unit_patterns
        .get(plural_index(plural))
        .filter(|pattern| !pattern.is_empty())
        .or_else(|| {
            pattern
                .unit_patterns
                .get(PLURAL_COUNT - 1)
                .filter(|pattern| !pattern.is_empty())
        })
        .cloned()
}

pub(super) fn has_full_cldr_currency_data(locale: &str) -> bool {
    let data = pinned_currency_data();
    locale_template(data, locale).is_some()
        && locale_candidates(locale).into_iter().any(|candidate| {
            data.patterns
                .iter()
                .any(|pattern| pattern.locale.eq_ignore_ascii_case(&candidate))
        })
}

/// Number of ISO code records represented by the complete pinned input.
pub(super) fn currency_code_count() -> usize {
    let data = pinned_currency_data();
    let codes = data
        .names
        .iter()
        .flat_map(|template| template.changes.keys())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        codes.len(),
        307,
        "embedded CLDR currency-name table must retain every currency code"
    );
    codes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_resolved_locale_has_names_and_currency_patterns() {
        let data = pinned_currency_data();
        assert_eq!(data.locale_indices.len(), CLDR_RESOLVED_LOCALE_COUNT);
        assert_eq!(data.patterns.len(), CLDR_CURRENCY_PATTERN_COUNT);
        assert!(data
            .patterns
            .iter()
            .all(|pattern| !pattern.standard.is_empty() && !pattern.accounting.is_empty()));
    }
}
