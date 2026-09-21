// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The zero UTC offset in a localized-offset zone name.
//!
//! CLDR gives every locale a `gmtFormat` ("GMT{0}") for a non-zero offset and
//! a separate `gmtZeroFormat` ("GMT") for a zero one. ICU4X's
//! `TimeZoneEssentials` data drops the second (its deserializer reads
//! `offset_zero` and discards it), and its localized-offset formatter has no
//! zero case, so a zero offset came out as the signed "GMT+0". The string is
//! restored here, after formatting, from the one thing the data still carries:
//! the locale's `gmtFormat` pattern.
//!
//! The derivation was checked against `cldr-json` (commit
//! `26a79cb42bfcc90def764102aa2af126d9ef3108`, the one CI pins); at run time
//! the pattern comes from ICU4X's compiled data, whose patterns for a few
//! minor locales fall back to the root one.
//!
//! In 761 of the 766 locales of CLDR's `cldr-dates-full`, `gmtZeroFormat` is
//! `gmtFormat` with the `{0}` removed and the whitespace or bidirectional
//! marks that sat beside it trimmed (`"GMT {0}"` -> `"GMT"`, `"{0} GMT"` ->
//! `"GMT"`, `"GMT{0}\u{200e}"` -> `"GMT"`). The other five (`dz`, `ks`,
//! `ks-Arab`, `tok`, `xnr`) translate the zero string independently; they get
//! the derived one, which is closer to right than a signed zero but is not
//! CLDR's text. A zone name is rewritten only when it is exactly that pattern
//! around a body with no letters in it and the offset in effect is zero, so a
//! real name ("UTC", "Greenwich Mean Time") and every non-zero offset are
//! left alone.

use super::{DateTimeFormat, DateTimePart, DateTimeRangePart, DateTimeRangePartSource};
use icu_datetime::provider::time_zones::TimezoneNamesEssentialsV1;
use icu_provider::prelude::*;
use writeable::Writeable;

/// Stands for the offset while the locale's pattern is rendered, so the text
/// before and after it can be told apart. A private-use character never
/// appears in a CLDR pattern.
const OFFSET_MARKER: &str = "\u{e000}";

fn is_pattern_padding(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{200b}' | '\u{200e}' | '\u{200f}' | '\u{061c}')
}

impl DateTimeFormat {
    /// The text before and after the offset in the locale's localized GMT
    /// format, or `None` when the locale has no time-zone data at all.
    fn gmt_format_affixes(&self) -> Option<(String, String)> {
        let locale = DataLocale::from(self.format_locale.locale());
        let response = DataProvider::<TimezoneNamesEssentialsV1>::load(
            &icu_datetime::provider::Baked,
            DataRequest {
                id: DataIdentifierBorrowed::for_locale(&locale),
                ..Default::default()
            },
        )
        .ok()?;
        let rendered = response
            .payload
            .get()
            .offset_pattern
            .interpolate([OFFSET_MARKER])
            .write_to_string()
            .into_owned();
        let (prefix, suffix) = rendered.split_once(OFFSET_MARKER)?;
        Some((prefix.to_owned(), suffix.to_owned()))
    }

    /// `text` as the locale's zero-offset string, if `text` is the locale's
    /// localized-offset format around some offset; `None` for anything else.
    fn zero_offset_zone_text(&self, text: &str) -> Option<String> {
        let (prefix, suffix) = self.gmt_format_affixes()?;
        let body = text
            .strip_prefix(prefix.as_str())?
            .strip_suffix(suffix.as_str())?;
        if body.is_empty() || body.chars().any(char::is_alphabetic) {
            return None;
        }
        Some(
            format!("{prefix}{suffix}")
                .trim_matches(is_pattern_padding)
                .to_owned(),
        )
    }

    /// Rewrites the `timeZoneName` parts of a value formatted at
    /// `offset_seconds`.
    pub(super) fn apply_zero_offset_zone_name(
        &self,
        parts: &mut [DateTimePart],
        offset_seconds: i32,
    ) {
        if offset_seconds != 0 {
            return;
        }
        for part in parts.iter_mut().filter(|part| part.kind == "timeZoneName") {
            if let Some(zero) = self.zero_offset_zone_text(&part.value) {
                part.value = zero;
            }
        }
    }

    /// The range counterpart: a shared or start-side zone name is at the start
    /// offset, an end-side one at the end offset.
    pub(super) fn apply_zero_offset_range_zone_name(
        &self,
        parts: &mut [DateTimeRangePart],
        start_offset_seconds: i32,
        end_offset_seconds: i32,
    ) {
        for part in parts.iter_mut().filter(|part| part.kind == "timeZoneName") {
            let offset = match part.source {
                DateTimeRangePartSource::EndRange => end_offset_seconds,
                DateTimeRangePartSource::Shared | DateTimeRangePartSource::StartRange => {
                    start_offset_seconds
                }
            };
            if offset != 0 {
                continue;
            }
            if let Some(zero) = self.zero_offset_zone_text(&part.value) {
                part.value = zero;
            }
        }
    }
}
