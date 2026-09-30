// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned CLDR Gregorian `availableFormats` records for BasicFormatMatcher.
//!
//! The table is generated from Unicode CLDR JSON 48.2.1, commit
//! `26a79cb42bfcc90def764102aa2af126d9ef3108`, by
//! `../../tools/generate_cldr_date_time_formats.mjs`. It preserves each
//! locale's source ordering because ECMA-402 uses list order to break equal
//! BasicFormatMatcher scores. Derived data is under Unicode License V3; see
//! `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::Read, sync::OnceLock};

const CLDR_RESOLVED_LOCALE_COUNT: usize = 766;
const CLDR_AVAILABLE_FORMAT_COUNT: usize = 38_792;
const CLDR_APPEND_ITEM_COUNT: usize = 8_426;

struct PinnedDateTimeFormats {
    rows: String,
    locale_ranges: HashMap<String, (usize, usize)>,
}

struct PinnedDateTimeAppendItems {
    rows: String,
    locale_ranges: HashMap<String, (usize, usize)>,
}

static PINNED_DATE_TIME_FORMATS: OnceLock<PinnedDateTimeFormats> = OnceLock::new();
static PINNED_DATE_TIME_APPEND_ITEMS: OnceLock<PinnedDateTimeAppendItems> = OnceLock::new();

fn pinned_date_time_formats() -> &'static PinnedDateTimeFormats {
    PINNED_DATE_TIME_FORMATS.get_or_init(|| {
        let encoded = include_str!("date_time_formats_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR availableFormats data must be valid base64");
        let mut rows = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut rows)
            .expect("embedded CLDR availableFormats data must be valid gzip");

        let mut locale_ranges = HashMap::new();
        let mut offset = 0usize;
        let mut active_locale = None::<&str>;
        let mut active_start = 0usize;
        let mut row_count = 0usize;
        for line in rows.split_inclusive('\n') {
            let mut fields = line.splitn(3, '\t');
            let locale = fields
                .next()
                .expect("embedded CLDR availableFormats row has a locale");
            let _skeleton = fields
                .next()
                .expect("embedded CLDR availableFormats row has a skeleton");
            let encoded_pattern = fields
                .next()
                .expect("embedded CLDR availableFormats row has a pattern")
                .trim_end();
            assert!(
                STANDARD.decode(encoded_pattern).is_ok(),
                "embedded CLDR availableFormats row has valid base64 pattern text"
            );
            if active_locale != Some(locale) {
                if let Some(previous) = active_locale {
                    assert!(
                        locale_ranges
                            .insert(previous.to_ascii_lowercase(), (active_start, offset))
                            .is_none(),
                        "embedded CLDR availableFormats data has duplicate locale groups"
                    );
                }
                active_locale = Some(locale);
                active_start = offset;
            }
            row_count += 1;
            offset += line.len();
        }
        if let Some(previous) = active_locale {
            assert!(
                locale_ranges
                    .insert(previous.to_ascii_lowercase(), (active_start, offset))
                    .is_none(),
                "embedded CLDR availableFormats data has duplicate final locale group"
            );
        }
        assert_eq!(
            locale_ranges.len(),
            CLDR_RESOLVED_LOCALE_COUNT,
            "embedded CLDR availableFormats data must retain every resolved locale"
        );
        assert_eq!(
            row_count, CLDR_AVAILABLE_FORMAT_COUNT,
            "embedded CLDR availableFormats record count must remain pinned"
        );
        PinnedDateTimeFormats {
            rows,
            locale_ranges,
        }
    })
}

fn pinned_date_time_append_items() -> &'static PinnedDateTimeAppendItems {
    PINNED_DATE_TIME_APPEND_ITEMS.get_or_init(|| {
        let encoded = include_str!("date_time_append_items_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR appendItems data must be valid base64");
        let mut rows = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut rows)
            .expect("embedded CLDR appendItems data must be valid gzip");

        let mut locale_ranges = HashMap::new();
        let mut offset = 0usize;
        let mut active_locale = None::<&str>;
        let mut active_start = 0usize;
        let mut row_count = 0usize;
        for line in rows.split_inclusive('\n') {
            let mut fields = line.splitn(4, '\t');
            let locale = fields
                .next()
                .expect("embedded CLDR appendItems row has a locale");
            let _field = fields
                .next()
                .expect("embedded CLDR appendItems row has a field");
            let encoded_pattern = fields
                .next()
                .expect("embedded CLDR appendItems row has a pattern")
                .trim_end();
            let encoded_field_name = fields
                .next()
                .expect("embedded CLDR appendItems row has a field name")
                .trim_end();
            assert!(
                STANDARD.decode(encoded_pattern).is_ok(),
                "embedded CLDR appendItems row has valid base64 pattern text"
            );
            assert!(
                STANDARD.decode(encoded_field_name).is_ok(),
                "embedded CLDR appendItems row has valid base64 field-name text"
            );
            if active_locale != Some(locale) {
                if let Some(previous) = active_locale {
                    assert!(
                        locale_ranges
                            .insert(previous.to_ascii_lowercase(), (active_start, offset))
                            .is_none(),
                        "embedded CLDR appendItems data has duplicate locale groups"
                    );
                }
                active_locale = Some(locale);
                active_start = offset;
            }
            row_count += 1;
            offset += line.len();
        }
        if let Some(previous) = active_locale {
            assert!(
                locale_ranges
                    .insert(previous.to_ascii_lowercase(), (active_start, offset))
                    .is_none(),
                "embedded CLDR appendItems data has duplicate final locale group"
            );
        }
        assert_eq!(
            locale_ranges.len(),
            CLDR_RESOLVED_LOCALE_COUNT,
            "embedded CLDR appendItems data must retain every resolved locale"
        );
        assert_eq!(
            row_count, CLDR_APPEND_ITEM_COUNT,
            "embedded CLDR appendItems record count must remain pinned"
        );
        PinnedDateTimeAppendItems {
            rows,
            locale_ranges,
        }
    })
}

fn locale_candidates(locale: &str) -> Vec<String> {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut candidates = Vec::new();
    let mut candidate = base;
    loop {
        candidates.push(candidate.to_owned());
        let Some((parent, _)) = candidate.rsplit_once('-') else {
            break;
        };
        candidate = parent;
    }
    candidates
}

fn record_from_skeleton(skeleton: &str) -> Option<crate::DateTimeFormatRecord> {
    use crate::{DateTimeFormatRecord, DateTimeWidth};

    let mut record = DateTimeFormatRecord::default();
    let mut quoted = false;
    let mut index = 0usize;
    let bytes = skeleton.as_bytes();
    while index < bytes.len() {
        let letter = bytes[index] as char;
        if letter == '\'' {
            if bytes.get(index + 1) == Some(&b'\'') {
                index += 2;
            } else {
                quoted = !quoted;
                index += 1;
            }
            continue;
        }
        if quoted || !letter.is_ascii_alphabetic() {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while bytes.get(end) == Some(&(letter as u8)) {
            end += 1;
        }
        let count = end - index;
        let numeric = || {
            if count == 2 {
                DateTimeWidth::TwoDigit
            } else {
                DateTimeWidth::Numeric
            }
        };
        let textual = || match count {
            0..=3 => DateTimeWidth::Short,
            4 => DateTimeWidth::Long,
            _ => DateTimeWidth::Narrow,
        };
        match letter {
            'G' => record.era = Some(textual()),
            'y' => record.year = Some(numeric()),
            'M' | 'L' => {
                record.month = Some(match count {
                    1 => DateTimeWidth::Numeric,
                    2 => DateTimeWidth::TwoDigit,
                    3 => DateTimeWidth::Short,
                    4 => DateTimeWidth::Long,
                    _ => DateTimeWidth::Narrow,
                })
            }
            'd' => record.day = Some(numeric()),
            'E' | 'e' | 'c' => record.weekday = Some(textual()),
            'a' | 'b' | 'B' => record.day_period = Some(textual()),
            'h' | 'H' | 'K' | 'k' => record.hour = Some(numeric()),
            'm' => record.minute = Some(numeric()),
            's' => record.second = Some(numeric()),
            'S' => record.fractional_second_digits = Some(u8::try_from(count).ok()?),
            'z' => record.time_zone_name = Some(if count <= 3 { "short" } else { "long" }.into()),
            'v' => {
                record.time_zone_name = Some(
                    if count <= 1 {
                        "shortGeneric"
                    } else {
                        "longGeneric"
                    }
                    .into(),
                )
            }
            'O' | 'X' | 'x' | 'Z' => {
                record.time_zone_name = Some(
                    if count <= 3 {
                        "shortOffset"
                    } else {
                        "longOffset"
                    }
                    .into(),
                )
            }
            // The generator rejects any unsupported skeleton letter. Keep
            // this defensive branch so a malformed pinned row cannot turn
            // into a misleading partial DateTime format record.
            _ => return None,
        }
        index = end;
    }
    Some(record)
}

/// Returns the locale's complete ordered CLDR record list for ECMA-402
/// BasicFormatMatcher. Locale fallback chooses the first available parent
/// record list; it never combines format records from multiple locales.
pub(crate) fn basic_format_records(locale: &str) -> Vec<crate::DateTimeFormatRecord> {
    let data = pinned_date_time_formats();
    for candidate in locale_candidates(locale) {
        let Some((start, end)) = data.locale_ranges.get(&candidate.to_ascii_lowercase()) else {
            continue;
        };
        return data.rows[*start..*end]
            .lines()
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                let _locale = fields.next()?;
                let skeleton = fields.next()?;
                let _encoded_pattern = fields.next()?;
                record_from_skeleton(skeleton)
            })
            .collect();
    }
    Vec::new()
}

/// Returns the field widths the locale's own `yMd` `availableFormats` entry
/// actually renders with, read from that entry's literal CLDR pattern text
/// rather than its (locale-invariant) skeleton id.
///
/// `basic_format_records` deliberately derives ECMA-402's *requested*
/// component widths from each row's skeleton id (`"yMd"`, `"yMMMd"`, ...),
/// which is the right source for BasicFormatMatcher's own scoring: two
/// locales sharing the `"yMd"` id are both offering a "numeric year, month,
/// day" format, by definition. But a locale is free to typeset that same
/// numeric skeleton with a customarily zero-padded field regardless of the
/// id's own letter count -- CLDR's `yMd` pattern is literally `"d.M.y"` for
/// `de` (unpadded) but `"dd/MM/y"` for `fr` (day and month zero-padded).
/// ICU4X's own semantic `Length`-based numeric date field set draws from
/// CLDR's differently-authored length-styled `dateFormats` table instead,
/// which does not reliably agree with `yMd` either way. Reading the pattern
/// text itself, with the same letter-repetition-counts-as-width rule
/// `record_from_skeleton` already applies to skeleton ids, is what actually
/// answers "does this locale zero-pad its default numeric date" -- with no
/// per-locale special-casing needed.
pub(crate) fn numeric_date_pattern_widths(locale: &str) -> Option<crate::DateTimeFormatRecord> {
    let data = pinned_date_time_formats();
    for candidate in locale_candidates(locale) {
        let Some((start, end)) = data.locale_ranges.get(&candidate.to_ascii_lowercase()) else {
            continue;
        };
        return data.rows[*start..*end].lines().find_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let _locale = fields.next()?;
            let skeleton = fields.next()?;
            let encoded_pattern = fields.next()?.trim_end();
            if skeleton != "yMd" {
                return None;
            }
            let pattern = STANDARD
                .decode(encoded_pattern)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())?;
            record_from_skeleton(&pattern)
        });
    }
    None
}

/// One decoded pinned CLDR append-item record.
///
/// The `{2}` placeholder used by CLDR's append patterns is the localized
/// field display name, not a literal brace sequence. Keeping it beside the
/// pattern lets DateTimeFormat synthesize typed parts without English labels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppendItemPattern {
    pub(crate) pattern: String,
    pub(crate) field_name: String,
}

/// Returns the pinned CLDR append-item pattern and field name for one field.
///
/// `availableFormats` is allowed to select a closest skeleton that lacks a
/// requested field.  The matching `appendItems` record is the locale's
/// authority for retaining that field in the synthesized pattern.  Keep the
/// decoded pattern available to the DateTimeFormat resolver rather than
/// treating the provider table as an inventory-only completeness check.
pub(crate) fn append_item_pattern(locale: &str, field: &str) -> Option<AppendItemPattern> {
    let data = pinned_date_time_append_items();
    for candidate in locale_candidates(locale) {
        let Some((start, end)) = data.locale_ranges.get(&candidate.to_ascii_lowercase()) else {
            continue;
        };
        for line in data.rows[*start..*end].lines() {
            let mut fields = line.splitn(4, '\t');
            let _locale = fields.next()?;
            let row_field = fields.next()?;
            let encoded_pattern = fields.next()?;
            let encoded_field_name = fields.next()?;
            if row_field == field {
                return Some(AppendItemPattern {
                    pattern: STANDARD
                        .decode(encoded_pattern)
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())?,
                    field_name: STANDARD
                        .decode(encoded_field_name.trim_end())
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())?,
                });
            }
        }
    }
    None
}

/// Whether the pinned CLDR date-time provider has a complete locale record.
pub(super) fn has_locale(locale: &str) -> bool {
    locale_candidates(locale).into_iter().any(|candidate| {
        let candidate = candidate.to_ascii_lowercase();
        let Some((start, end)) = pinned_date_time_append_items()
            .locale_ranges
            .get(&candidate)
        else {
            return false;
        };
        let required = [
            "Day",
            "Day-Of-Week",
            "Era",
            "Hour",
            "Minute",
            "Month",
            "Quarter",
            "Second",
            "Timezone",
            "Week",
            "Year",
        ];
        let has_every_append_item = required.iter().all(|required| {
            pinned_date_time_append_items().rows[*start..*end]
                .lines()
                .any(|line| {
                    line.split_once('\t')
                        .and_then(|(_, row)| row.split_once('\t'))
                        .is_some_and(|(field, _)| field == *required)
                })
        });
        has_every_append_item
            && pinned_date_time_formats()
                .locale_ranges
                .contains_key(&candidate)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_every_cldr_locale_and_ordered_gregorian_format_record() {
        let data = pinned_date_time_formats();
        let append_items = pinned_date_time_append_items();
        assert_eq!(data.locale_ranges.len(), CLDR_RESOLVED_LOCALE_COUNT);
        assert_eq!(append_items.locale_ranges.len(), CLDR_RESOLVED_LOCALE_COUNT);
        assert_eq!(
            data.rows.lines().count(),
            CLDR_AVAILABLE_FORMAT_COUNT,
            "all generated rows must remain visible"
        );
        let english = basic_format_records("en");
        assert_eq!(
            english.first().and_then(|record| record.hour),
            Some(crate::DateTimeWidth::Numeric)
        );
        assert!(english.iter().any(|record| {
            record.year == Some(crate::DateTimeWidth::Numeric)
                && record.month == Some(crate::DateTimeWidth::Short)
                && record.day == Some(crate::DateTimeWidth::Numeric)
        }));
        assert_eq!(
            append_item_pattern("de-CH", "Year"),
            Some(AppendItemPattern {
                pattern: "{1} {0}".into(),
                field_name: "Jahr".into(),
            })
        );
        assert_eq!(
            append_item_pattern("fr-CA-u-ca-gregory", "Timezone"),
            Some(AppendItemPattern {
                pattern: "{0} {1}".into(),
                field_name: "fuseau horaire".into(),
            })
        );
        assert!(has_locale("fr-CA"));
        assert!(has_locale("zh-Hant-u-ca-gregory"));
    }
}
