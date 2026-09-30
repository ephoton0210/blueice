// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete pinned CLDR data for generic sanctioned `-per-` compounds.
//!
//! The compressed table is generated from Unicode CLDR JSON 48.2.1
//! (commit 26a79cb42bfcc90def764102aa2af126d9ef3108), reading every
//! `cldr-units-full/main/{locale}/units.json` file.  Each record contains a
//! width's generic `per.compoundUnitPattern`, plus the `displayName`, optional
//! denominator-specific `perUnitPattern`, and plural-sensitive simple-unit
//! patterns for every ECMA-402 sanctioned simple unit. Keeping all resolved
//! CLDR locale files preserves script and regional forms before the normal
//! locale-parent lookup applies.
//! The derived data is distributed under Unicode License V3; see
//! `LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::Read, sync::OnceLock};

use super::super::{
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

const FIELD_SEPARATOR: char = '\u{1f}';
const GROUP_SEPARATOR: char = '\u{1e}';
const CLDR_RESOLVED_LOCALE_COUNT: usize = 766;
const SANCTIONED_UNIT_COUNT: usize = 45;
const PLURAL_CATEGORY_COUNT: usize = 6;

#[derive(Debug)]
struct FullCldrCompoundPattern {
    locale: String,
    display: String,
    generic_per: String,
    display_names: String,
    per_unit_patterns: String,
    unit_patterns: String,
}

struct FullCldrCompoundPatterns {
    records: Vec<FullCldrCompoundPattern>,
    locale_indices: HashMap<String, [usize; 3]>,
}

static FULL_CLDR_COMPOUND_PATTERNS: OnceLock<FullCldrCompoundPatterns> = OnceLock::new();

fn full_cldr_compound_patterns() -> &'static FullCldrCompoundPatterns {
    FULL_CLDR_COMPOUND_PATTERNS.get_or_init(|| {
        let encoded = include_str!("full_cldr_compound_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR compound-unit data must be valid base64");
        let mut tsv = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut tsv)
            .expect("embedded CLDR compound-unit data must be valid gzip");
        let records = tsv
            .lines()
            .map(|line| {
                let mut fields = line.splitn(6, '\t');
                FullCldrCompoundPattern {
                    locale: fields
                        .next()
                        .expect("embedded CLDR compound-unit row has locale")
                        .into(),
                    display: fields
                        .next()
                        .expect("embedded CLDR compound-unit row has display")
                        .into(),
                    generic_per: fields
                        .next()
                        .expect("embedded CLDR compound-unit row has generic pattern")
                        .into(),
                    display_names: fields
                        .next()
                        .expect("embedded CLDR compound-unit row has display names")
                        .into(),
                    per_unit_patterns: fields
                        .next()
                        .expect("embedded CLDR compound-unit row has per-unit patterns")
                        .into(),
                    unit_patterns: fields
                        .next()
                        .expect("embedded CLDR compound-unit row has simple-unit patterns")
                        .into(),
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            records.len(),
            CLDR_RESOLVED_LOCALE_COUNT * 3,
            "embedded CLDR compound-unit record count must remain pinned"
        );
        assert!(records.iter().all(|pattern| {
            pattern.generic_per.contains("{0}")
                && pattern.generic_per.contains("{1}")
                && pattern.display_names.split(FIELD_SEPARATOR).count() == SANCTIONED_UNIT_COUNT
                && pattern.per_unit_patterns.split(FIELD_SEPARATOR).count() == SANCTIONED_UNIT_COUNT
                && pattern.unit_patterns.split(GROUP_SEPARATOR).count() == SANCTIONED_UNIT_COUNT
                && pattern
                    .display_names
                    .split(FIELD_SEPARATOR)
                    .all(|name| !name.is_empty())
                && pattern
                    .per_unit_patterns
                    .split(FIELD_SEPARATOR)
                    .all(|per| per.is_empty() || per.contains("{0}"))
                && pattern
                    .unit_patterns
                    .split(GROUP_SEPARATOR)
                    .all(valid_unit_pattern_group)
        }));
        let mut locale_indices = HashMap::new();
        for (index, record) in records.iter().enumerate() {
            let display_index = match record.display.as_str() {
                "long" => 0,
                "short" => 1,
                "narrow" => 2,
                _ => unreachable!("compound table only contains CLDR widths"),
            };
            let indices = locale_indices
                .entry(record.locale.to_ascii_lowercase())
                .or_insert([usize::MAX; 3]);
            assert_eq!(
                indices[display_index],
                usize::MAX,
                "embedded CLDR compound-unit table has duplicate locale/width rows"
            );
            indices[display_index] = index;
        }
        assert_eq!(
            locale_indices.len(),
            CLDR_RESOLVED_LOCALE_COUNT,
            "embedded CLDR compound-unit table must retain every resolved locale"
        );
        assert!(
            locale_indices
                .values()
                .all(|indices| indices.iter().all(|index| *index != usize::MAX)),
            "every resolved CLDR locale must carry long, short, and narrow compound-unit data"
        );
        FullCldrCompoundPatterns {
            records,
            locale_indices,
        }
    })
}

fn display_index(display: crate::NumberUnitDisplay) -> usize {
    match display {
        crate::NumberUnitDisplay::Long => 0,
        crate::NumberUnitDisplay::Short => 1,
        crate::NumberUnitDisplay::Narrow => 2,
    }
}

fn locale_record(
    locale: &str,
    display: crate::NumberUnitDisplay,
) -> Option<&'static FullCldrCompoundPattern> {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let display_index = display_index(display);
    let patterns = full_cldr_compound_patterns();
    let exact = |candidate: &str| {
        let index = patterns
            .locale_indices
            .get(&candidate.to_ascii_lowercase())?
            .get(display_index)
            .copied()
            .filter(|index| *index != usize::MAX)?;
        patterns.records.get(index)
    };
    if let Some(pattern) = exact(base) {
        return Some(pattern);
    }

    // CLDR's resolved unit files commonly retain a script parent instead of
    // every likely-subtag region spelling. For example, `zh-TW` inherits
    // `zh-Hant`, not bare `zh`. Try the shared pinned maximizer before the
    // ordinary truncation chain so the script's unit grammar is preserved.
    let canonical = crate::canonicalize(base).ok()?;
    let maximal = crate::locale_data_provider()
        .maximize_likely_subtags(canonical.locale())
        .to_string();
    let mut candidate = maximal.as_str();
    loop {
        if let Some(pattern) = exact(candidate) {
            return Some(pattern);
        }
        let Some(parent) = candidate.rsplit_once('-').map(|(parent, _)| parent) else {
            break;
        };
        candidate = parent;
    }

    let mut candidate = base;
    loop {
        if let Some(pattern) = exact(candidate) {
            return Some(pattern);
        }
        candidate = candidate.rsplit_once('-')?.0;
    }
}

fn sanctioned_unit_index(unit: crate::NumberFormatUnit) -> Option<usize> {
    crate::NumberFormatUnit::ALL
        .iter()
        .position(|candidate| *candidate == unit)
}

fn packed_field(values: &str, index: usize) -> Option<&str> {
    values.split(FIELD_SEPARATOR).nth(index)
}

fn valid_unit_pattern_group(group: &str) -> bool {
    let forms = group.split(FIELD_SEPARATOR).collect::<Vec<_>>();
    // CLDR long singular forms can intentionally omit `{0}`; Arabic `فدان`
    // is one example. The formatter must preserve those patterns rather than
    // treating their absence as malformed data.
    forms.len() == PLURAL_CATEGORY_COUNT && !forms[PLURAL_CATEGORY_COUNT - 1].is_empty()
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

fn unit_pattern(
    record: &FullCldrCompoundPattern,
    unit: usize,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    let group = record.unit_patterns.split(GROUP_SEPARATOR).nth(unit)?;
    let forms = group.split(FIELD_SEPARATOR).collect::<Vec<_>>();
    let raw = forms
        .get(plural_index(plural))
        .filter(|pattern| !pattern.is_empty())
        .or_else(|| {
            forms
                .get(PLURAL_CATEGORY_COUNT - 1)
                .filter(|pattern| !pattern.is_empty())
        })?;
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns one exact pinned CLDR simple-unit pattern.
///
/// The complete compound table carries every simple unit because generic
/// `-per-` composition needs its localized numerator and denominator forms.
/// Exposing that shared record avoids maintaining a second, narrower table
/// for services such as DurationFormat.
pub(crate) fn cldr_full_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    let unit = sanctioned_unit_index(unit)?;
    let record = locale_record(locale, display)?;
    unit_pattern(record, unit, plural)
}

/// Applies the exact CLDR direct or generic `per` pattern to a complete
/// localized numerator pattern.  The input retains the private number
/// placeholder until after composition, which preserves both number-first
/// and number-after-label language forms at the observable part boundary.
pub(crate) fn cldr_full_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    let numerator = sanctioned_unit_index(numerator)?;
    let denominator = sanctioned_unit_index(denominator)?;
    let record = locale_record(locale, display)?;
    let numerator_pattern = unit_pattern(record, numerator, plural)?;
    let numerator = format!(
        "{}{}\u{fdd0}{}{}",
        numerator_pattern.prefix,
        numerator_pattern.prefix_separator,
        numerator_pattern.suffix_separator,
        numerator_pattern.suffix,
    );
    let direct = packed_field(&record.per_unit_patterns, denominator)?;
    let rendered = if direct.is_empty() {
        let denominator_pattern = unit_pattern(record, denominator, crate::PluralCategory::One)?;
        // CLDR generic `{1}` denotes the denominator's singular unit form.
        // For ordinary number-first patterns that is exactly the localized
        // `unitPattern-count-one` label supplied by this table.
        // A pattern with labels on both sides of the number has
        // number-adjacent material that does not belong in the denominator
        // form, so only that shape uses the raw CLDR `displayName` instead.
        // Number-after-label forms (for example Tongan `ʻeka ʻe {0}`) keep
        // their trailing particle through `number_unit_pattern_label`.
        let generated_label = number_unit_pattern_label(&denominator_pattern);
        let denominator =
            if !denominator_pattern.prefix.is_empty() && !denominator_pattern.suffix.is_empty() {
                packed_field(&record.display_names, denominator)?
            } else {
                &generated_label
            };
        record
            .generic_per
            .replace("{0}", &numerator)
            .replace("{1}", denominator)
    } else {
        direct.replace("{0}", &numerator)
    };
    let pattern = number_unit_pattern_from_placeholder(&rendered)?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: pattern.prefix,
        prefix_separator: pattern.prefix_separator,
        suffix_separator: pattern.suffix_separator,
        suffix: pattern.suffix,
    })
}

/// Whether the selected full CLDR numerator pattern deliberately omits the
/// numeric placeholder. The NumberFormat renderer uses this alongside the
/// composed affix to retain CLDR's exact singular forms (for example Arabic
/// long `فدان`).
pub(crate) fn cldr_full_generic_compound_hides_number(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<bool> {
    let numerator = sanctioned_unit_index(numerator)?;
    let record = locale_record(locale, display)?;
    Some(unit_pattern(record, numerator, plural)?.hides_number)
}

/// Whether exact CLDR generic-compound data exists for a locale/denominator
/// display cell. A validated record covers every sanctioned numerator and
/// reachable plural category because it embeds every simple-unit pattern.
pub(crate) fn has_cldr_full_generic_compound_data(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> bool {
    sanctioned_unit_index(denominator).is_some() && locale_record(locale, display).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_every_resolved_cldr_locale_and_sanctioned_denominator() {
        for record in &full_cldr_compound_patterns().records {
            for denominator in crate::NumberFormatUnit::ALL {
                assert!(has_cldr_full_generic_compound_data(
                    &record.locale,
                    *denominator,
                    match record.display.as_str() {
                        "long" => crate::NumberUnitDisplay::Long,
                        "short" => crate::NumberUnitDisplay::Short,
                        "narrow" => crate::NumberUnitDisplay::Narrow,
                        _ => unreachable!("compound table only contains CLDR widths"),
                    }
                ));
            }
        }
    }
}
