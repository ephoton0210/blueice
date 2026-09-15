// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned full-CLDR coverage for unit categories ICU4X does not type yet.
//!
//! The compressed table is generated from Unicode CLDR JSON 48.2.1
//! (commit 26a79cb42bfcc90def764102aa2af126d9ef3108), reading
//! `cldr-units-full/main/{locale}/units.json` `unitPattern-count-*`
//! entries. It covers every selectable plural/display cell for the 58
//! advertised NumberFormat locales that otherwise fall back to English for
//! digital, temperature, angle, and percent units. The derived data is
//! distributed under Unicode License V3; see `LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{io::Read, sync::OnceLock};

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

#[derive(Debug)]
struct FullCldrUnitPattern {
    locale: String,
    unit: String,
    display: String,
    plural: String,
    raw: String,
}

static FULL_CLDR_UNIT_PATTERNS: OnceLock<Vec<FullCldrUnitPattern>> = OnceLock::new();

fn full_cldr_unit_patterns() -> &'static [FullCldrUnitPattern] {
    FULL_CLDR_UNIT_PATTERNS
        .get_or_init(|| {
            let encoded = include_str!("full_cldr_data.b64")
                .lines()
                .collect::<String>();
            let compressed = STANDARD
                .decode(encoded)
                .expect("embedded CLDR unit-pattern data must be valid base64");
            let mut tsv = String::new();
            GzDecoder::new(compressed.as_slice())
                .read_to_string(&mut tsv)
                .expect("embedded CLDR unit-pattern data must be valid gzip");

            let patterns = tsv
                .lines()
                .map(|line| {
                    let mut fields = line.splitn(5, '\t');
                    FullCldrUnitPattern {
                        locale: fields
                            .next()
                            .expect("embedded CLDR unit-pattern row has locale")
                            .into(),
                        unit: fields
                            .next()
                            .expect("embedded CLDR unit-pattern row has unit")
                            .into(),
                        display: fields
                            .next()
                            .expect("embedded CLDR unit-pattern row has display")
                            .into(),
                        plural: fields
                            .next()
                            .expect("embedded CLDR unit-pattern row has plural")
                            .into(),
                        raw: fields
                            .next()
                            .expect("embedded CLDR unit-pattern row has pattern")
                            .into(),
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                patterns.len(),
                4_085,
                "embedded CLDR unit-pattern record count must remain pinned"
            );
            patterns
        })
        .as_slice()
}

fn primary_language(locale: &str) -> &str {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        .unwrap_or(locale)
}

/// Looks up raw CLDR cells for ECMA-402 simple units without an ICU4X typed
/// marker. Existing focused raw provider families retain precedence.
pub(super) fn cldr_full_untyped_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberUnitDisplay as Display, PluralCategory};

    let display = match display {
        Display::Long => "long",
        Display::Short => "short",
        Display::Narrow => "narrow",
    };
    let plural = match plural {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    };
    let locale = primary_language(locale);
    let unit = unit.as_str();

    let matching = |pattern: &&FullCldrUnitPattern| {
        pattern.locale == locale && pattern.unit == unit && pattern.display == display
    };
    let pattern = full_cldr_unit_patterns()
        .iter()
        .find(|pattern| matching(pattern) && pattern.plural == plural)
        .or_else(|| {
            full_cldr_unit_patterns()
                .iter()
                .find(|pattern| matching(pattern) && pattern.plural == "other")
        })?;
    number_unit_pattern_from_placeholder(&pattern.raw.replace("{0}", "\u{fdd0}"))
}
