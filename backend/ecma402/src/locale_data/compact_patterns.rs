// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete pinned CLDR compact-decimal patterns.
//!
//! This table is generated from every `cldr-numbers-full` 48.2.1
//! `decimalFormats-numberSystem-*/{short,long}/decimalFormat` record
//! (commit `26a79cb42bfcc90def764102aa2af126d9ef3108`).  It deliberately
//! retains every resolved CLDR locale, plural category, magnitude and
//! numbering-system record rather than querying ICU4X's compact baked slice.
//! The derived data is distributed under Unicode License V3; see
//! `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use fixed_decimal::Decimal;
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::Read, sync::OnceLock};

use super::CompactNumberPattern;

const CLDR_RESOLVED_LOCALE_COUNT: usize = 766;
const CLDR_COMPACT_RECORD_COUNT: usize = 38_040;

#[derive(Debug)]
struct PinnedCompactPattern {
    locale: String,
    numbering_system: String,
    display: String,
    magnitude: i16,
    plural: String,
    raw: String,
}

struct PinnedCompactPatterns {
    records: Vec<PinnedCompactPattern>,
    indices: HashMap<String, Vec<usize>>,
    locales: usize,
}

static PINNED_COMPACT_PATTERNS: OnceLock<PinnedCompactPatterns> = OnceLock::new();

fn pinned_compact_patterns() -> &'static PinnedCompactPatterns {
    PINNED_COMPACT_PATTERNS.get_or_init(|| {
        let encoded = include_str!("compact_patterns_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR compact-pattern data must be valid base64");
        let mut tsv = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut tsv)
            .expect("embedded CLDR compact-pattern data must be valid gzip");
        let records = tsv
            .lines()
            .map(|line| {
                let mut fields = line.splitn(6, '\t');
                PinnedCompactPattern {
                    locale: fields
                        .next()
                        .expect("embedded compact row has locale")
                        .into(),
                    numbering_system: fields
                        .next()
                        .expect("embedded compact row has numbering system")
                        .into(),
                    display: fields
                        .next()
                        .expect("embedded compact row has display")
                        .into(),
                    magnitude: fields
                        .next()
                        .expect("embedded compact row has magnitude")
                        .len()
                        .checked_sub(1)
                        .and_then(|magnitude| i16::try_from(magnitude).ok())
                        .expect("embedded compact magnitude must be a nonzero decimal power"),
                    plural: fields
                        .next()
                        .expect("embedded compact row has plural category")
                        .into(),
                    raw: fields
                        .next()
                        .expect("embedded compact row has CLDR pattern")
                        .into(),
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            records.len(),
            CLDR_COMPACT_RECORD_COUNT,
            "embedded CLDR compact-pattern record count must remain pinned"
        );
        let mut indices = HashMap::<String, Vec<usize>>::new();
        for (index, record) in records.iter().enumerate() {
            indices
                .entry(index_key(
                    &record.locale,
                    &record.numbering_system,
                    &record.display,
                ))
                .or_default()
                .push(index);
        }
        let locales = records
            .iter()
            .map(|record| record.locale.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(
            locales, CLDR_RESOLVED_LOCALE_COUNT,
            "embedded compact table must retain every resolved CLDR locale"
        );
        PinnedCompactPatterns {
            records,
            indices,
            locales,
        }
    })
}

fn index_key(locale: &str, numbering_system: &str, display: &str) -> String {
    format!(
        "{}\u{1f}{numbering_system}\u{1f}{display}",
        locale.to_ascii_lowercase()
    )
}

fn display_name(display: crate::NumberCompactDisplay) -> &'static str {
    match display {
        crate::NumberCompactDisplay::Short => "short",
        crate::NumberCompactDisplay::Long => "long",
    }
}

fn plural_name(plural: crate::PluralCategory) -> &'static str {
    match plural {
        crate::PluralCategory::Zero => "zero",
        crate::PluralCategory::One => "one",
        crate::PluralCategory::Two => "two",
        crate::PluralCategory::Few => "few",
        crate::PluralCategory::Many => "many",
        crate::PluralCategory::Other => "other",
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

/// Resolves a full CLDR compact pattern, including pre-number labels such as
/// Swahili `elfu 1` and exact forms that deliberately omit the number.
pub(super) fn compact_number_pattern(
    locale: &str,
    numbering_system: &str,
    magnitude: i16,
    display: crate::NumberCompactDisplay,
    plural: crate::PluralCategory,
    rounded_value: Option<&Decimal>,
) -> Option<CompactNumberPattern> {
    let patterns = pinned_compact_patterns();
    let display = display_name(display);
    let plural = plural_name(plural);
    let exact_count = rounded_value.and_then(compact_exact_count);
    for candidate in locale_candidates(locale) {
        let exact = patterns
            .indices
            .get(&index_key(&candidate, numbering_system, display))
            .or_else(|| {
                let default = super::decimal_symbols::default_numbering_system(&candidate)?;
                patterns
                    .indices
                    .get(&index_key(&candidate, default, display))
            });
        let Some(indices) = exact else {
            continue;
        };
        let selected_magnitude = indices
            .iter()
            .map(|index| &patterns.records[*index])
            .filter(|record| record.magnitude <= magnitude && has_compact_affix(record))
            .map(|record| record.magnitude)
            .max()?;
        let selected = indices
            .iter()
            .map(|index| &patterns.records[*index])
            .find(|record| {
                has_compact_affix(record)
                    && exact_count.as_ref().is_some_and(|count| {
                        record.magnitude == selected_magnitude && record.plural == *count
                    })
            })
            .or_else(|| {
                indices
                    .iter()
                    .map(|index| &patterns.records[*index])
                    .find(|record| {
                        has_compact_affix(record)
                            && record.magnitude == selected_magnitude
                            && record.plural == plural
                    })
            })
            .or_else(|| {
                indices
                    .iter()
                    .map(|index| &patterns.records[*index])
                    .find(|record| {
                        has_compact_affix(record)
                            && record.magnitude == selected_magnitude
                            && record.plural == "other"
                    })
            })?;
        return compact_pattern_from_cldr(&selected.raw, selected.magnitude);
    }
    None
}

fn compact_exact_count(value: &Decimal) -> Option<String> {
    let value = value.to_string();
    let value = value.trim_start_matches(['+', '-']);
    (!value.contains('.')).then_some(value.into())
}

fn has_compact_affix(record: &PinnedCompactPattern) -> bool {
    compact_pattern_from_cldr(&record.raw, record.magnitude).is_some_and(|pattern| {
        pattern.hides_number || !pattern.prefix.is_empty() || !pattern.suffix.is_empty()
    })
}

fn compact_pattern_from_cldr(raw: &str, magnitude: i16) -> Option<CompactNumberPattern> {
    // NumberFormat has already emitted the typed decimal sign, so choose the
    // positive subpattern and leave a negative compact affix to that sign.
    let raw = raw.split(';').next().unwrap_or(raw);
    let first = raw.find(['0', '#']);
    let Some(first) = first else {
        return (!raw.is_empty()).then(|| CompactNumberPattern {
            divisor: magnitude,
            prefix_leading_literal: String::new(),
            prefix: String::new(),
            prefix_trailing_literal: String::new(),
            prefix_separator: String::new(),
            suffix_separator: String::new(),
            suffix: unescape_cldr_literals(raw),
            suffix_trailing_literal: String::new(),
            hides_number: true,
        });
    };
    let mut end = first;
    for (offset, character) in raw[first..].char_indices() {
        if matches!(character, '0' | '#' | ',' | '.') {
            end = first + offset + character.len_utf8();
        } else {
            break;
        }
    }
    let skeleton = &raw[first..end];
    let digits = skeleton
        .chars()
        .filter(|character| matches!(character, '0' | '#'))
        .count();
    let divisor = magnitude.checked_sub(i16::try_from(digits.checked_sub(1)?).ok()?)?;
    let raw_prefix = unescape_cldr_literals(&raw[..first]);
    let raw_suffix = unescape_cldr_literals(&raw[end..]);
    let prefix_label = raw_prefix.trim_end();
    let suffix_label = raw_suffix.trim_start();
    let (prefix_leading_literal, prefix, prefix_trailing_literal) =
        split_directional_controls(prefix_label);
    let (suffix_leading_literal, suffix, suffix_trailing_literal) =
        split_directional_controls(suffix_label);
    Some(CompactNumberPattern {
        divisor,
        prefix_leading_literal,
        prefix,
        prefix_trailing_literal,
        prefix_separator: raw_prefix[prefix_label.len()..].into(),
        suffix_separator: format!(
            "{}{}",
            raw_suffix[..raw_suffix.len() - suffix_label.len()].to_owned(),
            suffix_leading_literal
        ),
        suffix,
        suffix_trailing_literal,
        hides_number: false,
    })
}

/// Applies the decimal-pattern apostrophe quoting rules to an affix. A single
/// apostrophe opens/closes a quoted literal and two apostrophes emit one.
fn unescape_cldr_literals(value: &str) -> String {
    let mut output = String::new();
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\'' {
            if characters.peek() == Some(&'\'') {
                output.push('\'');
                characters.next();
            }
            continue;
        }
        output.push(character);
    }
    output
}

fn split_directional_controls(value: &str) -> (String, String, String) {
    fn is_directional_control(character: char) -> bool {
        matches!(
            character,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
    }

    let start = value
        .char_indices()
        .find(|(_, character)| !is_directional_control(*character))
        .map_or(value.len(), |(index, _)| index);
    let end = value
        .char_indices()
        .rev()
        .find(|(_, character)| !is_directional_control(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    if start >= end {
        return (value.into(), String::new(), String::new());
    }
    (
        value[..start].into(),
        value[start..end].into(),
        value[end..].into(),
    )
}

pub(super) fn has_full_cldr_compact_data(locale: &str) -> bool {
    let patterns = pinned_compact_patterns();
    let covered = locale_candidates(locale).into_iter().any(|candidate| {
        patterns
            .indices
            .keys()
            .any(|key| key.starts_with(&format!("{}\u{1f}", candidate.to_ascii_lowercase())))
    });
    covered && patterns.locales == CLDR_RESOLVED_LOCALE_COUNT
}

/// Counts every raw compact plural/magnitude/numbering-system record selected
/// for one locale. The count is an inventory measure: a CLDR form such as a
/// numberless exact pattern still counts as data-backed even though it emits
/// no decimal digits.
pub(super) fn compact_coverage(locale: &str) -> super::NumberFormatCoverageCount {
    let patterns = pinned_compact_patterns();
    let total = locale_candidates(locale)
        .into_iter()
        .find_map(|candidate| {
            let prefix = format!("{}\u{1f}", candidate.to_ascii_lowercase());
            let count = patterns
                .indices
                .iter()
                .filter(|(key, _)| key.starts_with(&prefix))
                .map(|(_, indices)| indices.len())
                .sum::<usize>();
            (count > 0).then_some(count)
        })
        .unwrap_or(0);
    super::NumberFormatCoverageCount {
        data_backed: if has_full_cldr_compact_data(locale) {
            total
        } else {
            0
        },
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_suffix_and_prefix_compact_affixes() {
        assert_eq!(
            compact_pattern_from_cldr("0K", 3),
            Some(CompactNumberPattern {
                divisor: 3,
                prefix_leading_literal: String::new(),
                prefix: String::new(),
                prefix_trailing_literal: String::new(),
                prefix_separator: String::new(),
                suffix_separator: String::new(),
                suffix: "K".into(),
                suffix_trailing_literal: String::new(),
                hides_number: false,
            })
        );
        assert_eq!(
            compact_pattern_from_cldr("elfu\u{a0}0", 3),
            Some(CompactNumberPattern {
                divisor: 3,
                prefix_leading_literal: String::new(),
                prefix: "elfu".into(),
                prefix_trailing_literal: String::new(),
                prefix_separator: "\u{a0}".into(),
                suffix_separator: String::new(),
                suffix: String::new(),
                suffix_trailing_literal: String::new(),
                hides_number: false,
            })
        );
    }

    #[test]
    fn retains_directional_controls_as_literals_around_compact_labels() {
        assert_eq!(
            compact_pattern_from_cldr("0K\u{200f}", 3),
            Some(CompactNumberPattern {
                divisor: 3,
                prefix_leading_literal: String::new(),
                prefix: String::new(),
                prefix_trailing_literal: String::new(),
                prefix_separator: String::new(),
                suffix_separator: String::new(),
                suffix: "K".into(),
                suffix_trailing_literal: "\u{200f}".into(),
                hides_number: false,
            })
        );
    }

    #[test]
    fn skips_bare_zero_compact_records_and_unescapes_quoted_literals() {
        assert!(!has_compact_affix(&PinnedCompactPattern {
            locale: "de".into(),
            numbering_system: "latn".into(),
            display: "short".into(),
            magnitude: 4,
            plural: "other".into(),
            raw: "0".into(),
        }));
        assert_eq!(
            compact_pattern_from_cldr("0\u{a0}Mio'.'", 6)
                .expect("quoted compact suffix parses")
                .suffix,
            "Mio."
        );
    }
}
