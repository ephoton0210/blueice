// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete pinned CLDR list-pattern data for `Intl.ListFormat`.
//!
//! The compressed table is generated from Unicode CLDR JSON 48.2.1
//! (commit 26a79cb42bfcc90def764102aa2af126d9ef3108), reading every
//! `cldr-misc-full/main/{locale}/listPatterns.json` record. It retains all
//! 766 resolved locales, three ECMA-402 list relations, and wide/short/narrow
//! widths. The derived data is distributed under Unicode License V3; see
//! `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::Read, sync::OnceLock};

const CLDR_RESOLVED_LOCALE_COUNT: usize = 766;
const PATTERN_VARIANT_COUNT: usize = 9;
const RECORD_COUNT: usize = CLDR_RESOLVED_LOCALE_COUNT * PATTERN_VARIANT_COUNT;

/// One complete CLDR `listPattern` quartet.
#[derive(Clone, Debug)]
pub(crate) struct PinnedListPatterns {
    pub(crate) two: String,
    pub(crate) start: String,
    pub(crate) middle: String,
    pub(crate) end: String,
}

impl PinnedListPatterns {
    pub(crate) fn bytes(&self) -> usize {
        self.two.len() + self.start.len() + self.middle.len() + self.end.len()
    }
}

#[derive(Debug)]
struct ListPatternRecord {
    locale: String,
    list_type: String,
    style: String,
    patterns: PinnedListPatterns,
}

struct PinnedListPatternData {
    records: Vec<ListPatternRecord>,
    indices: HashMap<(String, String, String), usize>,
}

static PINNED_LIST_PATTERNS: OnceLock<PinnedListPatternData> = OnceLock::new();

fn pinned_list_patterns() -> &'static PinnedListPatternData {
    PINNED_LIST_PATTERNS.get_or_init(|| {
        let encoded = include_str!("list_patterns_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR list-pattern data must be valid base64");
        let mut tsv = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut tsv)
            .expect("embedded CLDR list-pattern data must be valid gzip");
        let records = tsv
            .lines()
            .map(|line| {
                let mut fields = line.splitn(7, '\t');
                let locale = fields
                    .next()
                    .expect("embedded CLDR list-pattern row has locale")
                    .into();
                let list_type = fields
                    .next()
                    .expect("embedded CLDR list-pattern row has relation")
                    .into();
                let style = fields
                    .next()
                    .expect("embedded CLDR list-pattern row has width")
                    .into();
                let patterns = PinnedListPatterns {
                    two: fields
                        .next()
                        .expect("embedded CLDR list-pattern row has two-item pattern")
                        .into(),
                    start: fields
                        .next()
                        .expect("embedded CLDR list-pattern row has start pattern")
                        .into(),
                    middle: fields
                        .next()
                        .expect("embedded CLDR list-pattern row has middle pattern")
                        .into(),
                    end: fields
                        .next()
                        .expect("embedded CLDR list-pattern row has end pattern")
                        .into(),
                };
                assert!(
                    [
                        patterns.two.as_str(),
                        patterns.start.as_str(),
                        patterns.middle.as_str(),
                        patterns.end.as_str(),
                    ]
                    .into_iter()
                    .all(valid_pattern),
                    "embedded CLDR list pattern must contain one {{0}} and one {{1}} placeholder"
                );
                ListPatternRecord {
                    locale,
                    list_type,
                    style,
                    patterns,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            records.len(),
            RECORD_COUNT,
            "embedded CLDR list-pattern record count must remain pinned"
        );
        let mut indices = HashMap::new();
        for (index, record) in records.iter().enumerate() {
            let key = (
                record.locale.to_ascii_lowercase(),
                record.list_type.clone(),
                record.style.clone(),
            );
            assert!(
                indices.insert(key, index).is_none(),
                "embedded CLDR list-pattern table has duplicate locale/type/style rows"
            );
        }
        assert_eq!(
            indices.len(),
            RECORD_COUNT,
            "embedded CLDR list-pattern table must retain every relation/width cell"
        );
        assert_eq!(
            indices
                .keys()
                .map(|(locale, _, _)| locale)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            CLDR_RESOLVED_LOCALE_COUNT,
            "embedded CLDR list-pattern table must retain every resolved locale"
        );
        PinnedListPatternData { records, indices }
    })
}

fn valid_pattern(pattern: &str) -> bool {
    pattern.matches("{0}").count() == 1 && pattern.matches("{1}").count() == 1
}

fn pattern_keys(
    list_type: crate::ListType,
    style: crate::ListStyle,
) -> (&'static str, &'static str) {
    let list_type = match list_type {
        crate::ListType::Conjunction => "conjunction",
        crate::ListType::Disjunction => "disjunction",
        crate::ListType::Unit => "unit",
    };
    let style = match style {
        crate::ListStyle::Wide => "wide",
        crate::ListStyle::Short => "short",
        crate::ListStyle::Narrow => "narrow",
    };
    (list_type, style)
}

fn exact_record<'a>(
    data: &'a PinnedListPatternData,
    locale: &str,
    list_type: &str,
    style: &str,
) -> Option<&'a ListPatternRecord> {
    let key = (locale.to_ascii_lowercase(), list_type.into(), style.into());
    data.indices
        .get(&key)
        .and_then(|index| data.records.get(*index))
}

fn record_for_locale(
    locale: &str,
    list_type: &str,
    style: &str,
) -> Option<&'static ListPatternRecord> {
    let data = pinned_list_patterns();
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    if let Some(record) = exact_record(data, base, list_type, style) {
        return Some(record);
    }

    let canonical = crate::canonicalize(base).ok()?;
    let maximal = crate::locale_data_provider()
        .maximize_likely_subtags(canonical.locale())
        .to_string();
    let mut candidate = maximal.as_str();
    loop {
        if let Some(record) = exact_record(data, candidate, list_type, style) {
            return Some(record);
        }
        let Some((parent, _)) = candidate.rsplit_once('-') else {
            break;
        };
        candidate = parent;
    }

    let mut candidate = base;
    loop {
        if let Some(record) = exact_record(data, candidate, list_type, style) {
            return Some(record);
        }
        candidate = candidate.rsplit_once('-')?.0;
    }
}

/// Returns exact pinned CLDR list patterns after locale fallback.
pub(crate) fn patterns(
    locale: &str,
    list_type: crate::ListType,
    style: crate::ListStyle,
) -> Option<PinnedListPatterns> {
    let (list_type, style) = pattern_keys(list_type, style);
    record_for_locale(locale, list_type, style).map(|record| record.patterns.clone())
}

/// Whether any complete pinned list-pattern record resolves for `locale`.
pub(super) fn has_locale(locale: &str) -> bool {
    patterns(locale, crate::ListType::Conjunction, crate::ListStyle::Wide).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_every_resolved_locale_and_list_pattern_variant() {
        let data = pinned_list_patterns();
        assert_eq!(data.records.len(), RECORD_COUNT);
        assert_eq!(
            data.records
                .iter()
                .map(|record| record.locale.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            CLDR_RESOLVED_LOCALE_COUNT
        );
        assert_eq!(
            patterns("ak", crate::ListType::Unit, crate::ListStyle::Short)
                .expect("Akan unit patterns are pinned")
                .end,
            "{0}, ne {1}"
        );
    }
}
