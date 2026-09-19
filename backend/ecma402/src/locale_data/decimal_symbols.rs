// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned CLDR decimal-symbol data for every resolved CLDR NumberFormat locale.
//!
//! ICU4X's compact baked decimal payload has intentionally sparse locale
//! coverage. This table is generated from Unicode CLDR JSON 48.2.1 (commit
//! 26a79cb42bfcc90def764102aa2af126d9ef3108), reading every resolved
//! `cldr-numbers-full/main/{locale}/numbers.json` symbols, scientific
//! notation, and standard decimal-format records. It contains a row for every
//! CLDR `symbols-numberSystem-*` record in that inventory. The derived data is
//! distributed under Unicode License V3; see `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{collections::BTreeSet, io::Read, sync::OnceLock};

/// Decimal symbols and grouping metadata ready to construct an ICU4X payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumberDecimalSymbols {
    pub(crate) numbering_system: String,
    pub(crate) decimal_separator: String,
    pub(crate) grouping_separator: String,
    pub(crate) plus_sign: String,
    pub(crate) minus_sign: String,
    /// CLDR's `exponential` symbol for the selected symbols record.
    pub(crate) exponent_separator: String,
    /// CLDR's complete `minusSign` for a negative scientific exponent.
    ///
    /// Directional controls remain attached here until the NumberFormat
    /// typed-parts layer splits them into literal prefix/suffix parts.
    pub(crate) exponent_minus_sign: String,
    pub(crate) primary_grouping: u8,
    pub(crate) secondary_grouping: u8,
    pub(crate) minimum_grouping: u8,
}

#[derive(Debug)]
struct PinnedDecimalSymbols {
    locale: String,
    default_numbering_system: String,
    numbering_system: String,
    decimal_separator: String,
    grouping_separator: String,
    plus_sign: String,
    minus_sign: String,
    standard_pattern: String,
    exponent_separator: String,
    exponent_minus_sign: String,
    minimum_grouping: u8,
}

static PINNED_DECIMAL_SYMBOLS: OnceLock<Vec<PinnedDecimalSymbols>> = OnceLock::new();

fn pinned_decimal_symbols() -> &'static [PinnedDecimalSymbols] {
    PINNED_DECIMAL_SYMBOLS
        .get_or_init(|| {
            let encoded = include_str!("decimal_symbols_data.b64")
                .lines()
                .collect::<String>();
            let compressed = STANDARD
                .decode(encoded)
                .expect("embedded CLDR decimal-symbol data must be valid base64");
            let mut tsv = String::new();
            GzDecoder::new(compressed.as_slice())
                .read_to_string(&mut tsv)
                .expect("embedded CLDR decimal-symbol data must be valid gzip");
            let records = tsv
                .lines()
                .map(|line| {
                    let mut fields = line.splitn(11, '\t');
                    PinnedDecimalSymbols {
                        locale: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has locale")
                            .into(),
                        default_numbering_system: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has default numbering system")
                            .into(),
                        numbering_system: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has numbering system")
                            .into(),
                        decimal_separator: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has decimal separator")
                            .into(),
                        grouping_separator: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has grouping separator")
                            .into(),
                        plus_sign: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has plus sign")
                            .into(),
                        minus_sign: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has minus sign")
                            .into(),
                        standard_pattern: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has standard pattern")
                            .into(),
                        exponent_separator: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has exponent separator")
                            .into(),
                        exponent_minus_sign: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has exponent minus sign")
                            .into(),
                        minimum_grouping: fields
                            .next()
                            .expect("embedded CLDR decimal-symbol row has minimum grouping digits")
                            .parse()
                            .expect("embedded CLDR minimum grouping digits must be an integer"),
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                records.len(),
                910,
                "embedded CLDR decimal-symbol record count must remain pinned"
            );
            records
        })
        .as_slice()
}

fn locale_candidates(locale: &str) -> Vec<&str> {
    let locale = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut candidates = vec![locale];
    let mut candidate = locale;
    while let Some((parent, _)) = candidate.rsplit_once('-') {
        candidates.push(parent);
        candidate = parent;
    }
    candidates
}

/// Returns the CLDR default numbering system for a resolved locale.
pub(super) fn default_numbering_system(locale: &str) -> Option<&'static str> {
    locale_candidates(locale).into_iter().find_map(|candidate| {
        pinned_decimal_symbols()
            .iter()
            .find(|record| record.locale.eq_ignore_ascii_case(candidate))
            .map(|record| record.default_numbering_system.as_str())
    })
}

/// Returns every resolved CLDR locale represented in the pinned decimal data.
///
/// Each locale can carry several numbering-system records, so this is a
/// deduplicated locale inventory rather than the underlying row count.
pub(super) fn resolved_locales() -> Vec<String> {
    let locales = pinned_decimal_symbols()
        .iter()
        .map(|record| record.locale.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        locales.len(),
        766,
        "embedded decimal-symbol locale inventory must retain every resolved CLDR locale"
    );
    locales
}

/// Returns a locale's CLDR symbols for an explicitly resolved numbering
/// system. If CLDR has no system-specific symbols, ECMA-402 keeps the
/// locale's default symbols while substituting only the selected digits.
pub(super) fn decimal_symbols(
    locale: &str,
    numbering_system: &str,
) -> Option<NumberDecimalSymbols> {
    let records = pinned_decimal_symbols();
    let record = locale_candidates(locale)
        .into_iter()
        .find_map(|candidate| {
            let default_numbering_system = default_numbering_system(candidate)?;
            records
                .iter()
                .find(|record| {
                    record.locale.eq_ignore_ascii_case(candidate)
                        && record.numbering_system == numbering_system
                })
                .or_else(|| {
                    records.iter().find(|record| {
                        record.locale.eq_ignore_ascii_case(candidate)
                            && record.numbering_system == default_numbering_system
                    })
                })
        })?;
    let (primary_grouping, secondary_grouping) = grouping_sizes(&record.standard_pattern)?;
    Some(NumberDecimalSymbols {
        numbering_system: record.numbering_system.clone(),
        decimal_separator: record.decimal_separator.clone(),
        grouping_separator: record.grouping_separator.clone(),
        plus_sign: record.plus_sign.clone(),
        minus_sign: record.minus_sign.clone(),
        exponent_separator: record.exponent_separator.clone(),
        exponent_minus_sign: record.exponent_minus_sign.clone(),
        primary_grouping,
        secondary_grouping,
        minimum_grouping: record.minimum_grouping,
    })
}

fn grouping_sizes(pattern: &str) -> Option<(u8, u8)> {
    let integer = pattern.split('.').next()?;
    let groups = integer.split(',').collect::<Vec<_>>();
    let primary = groups
        .last()?
        .chars()
        .filter(|character| *character == '#' || *character == '0')
        .count();
    let primary = u8::try_from(primary).ok()?;
    let secondary = (groups.len() > 2)
        .then(|| groups.get(groups.len() - 2))
        .flatten()
        .map(|group| {
            group
                .chars()
                .filter(|character| *character == '#' || *character == '0')
                .count()
        })
        .map(u8::try_from)
        .transpose()
        .ok()?
        .unwrap_or(0);
    (primary > 0).then_some((primary, secondary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_primary_and_secondary_grouping_from_cldr_patterns() {
        assert_eq!(grouping_sizes("#,##0.###"), Some((3, 0)));
        assert_eq!(grouping_sizes("#,##,##0.###"), Some((3, 2)));
    }

    #[test]
    fn falls_back_to_pinned_default_symbols_not_root_symbols() {
        let thai = decimal_symbols("th", "thai").expect("Thai is pinned");
        assert_eq!(thai.numbering_system, "thai");
        let tamil = decimal_symbols("ta", "thai").expect("Tamil is pinned");
        assert_eq!(tamil.numbering_system, "latn");
        assert_eq!(tamil.decimal_separator, ".");
    }

    #[test]
    fn retains_cldr_scientific_symbols_and_full_bidi_minus_shape() {
        let arabic = decimal_symbols("ar", "arab").expect("Arabic is pinned");
        assert_eq!(arabic.exponent_separator, "أس");
        assert_eq!(arabic.exponent_minus_sign, "\u{61c}-");

        let pashto = decimal_symbols("ps", "arabext").expect("Pashto is pinned");
        assert_eq!(pashto.exponent_separator, "×۱۰^");
        assert_eq!(pashto.exponent_minus_sign, "\u{200e}-\u{200e}");

        let estonian = decimal_symbols("et", "latn").expect("Estonian is pinned");
        assert_eq!(estonian.exponent_separator, "×10^");
        assert_eq!(estonian.exponent_minus_sign, "−");
    }

    #[test]
    fn retains_region_specific_decimal_separators() {
        let french_canadian = decimal_symbols("fr-CA", "latn").expect("fr-CA is pinned");
        assert_eq!(french_canadian.grouping_separator, "\u{a0}");
        assert_eq!(french_canadian.decimal_separator, ",");
    }
}
