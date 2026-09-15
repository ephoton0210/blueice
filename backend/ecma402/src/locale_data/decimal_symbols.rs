// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned CLDR decimal-symbol data for every supported NumberFormat locale.
//!
//! ICU4X's compact baked decimal payload has intentionally sparse locale
//! coverage. This table is generated from Unicode CLDR JSON 48.2.1 (commit
//! 26a79cb42bfcc90def764102aa2af126d9ef3108), reading each advertised
//! `cldr-numbers-full/main/{locale}/numbers.json` symbols and standard
//! decimal-format records. It preserves every advertised locale and every
//! direct language record that the former ICU4X provider exposed. It contains
//! a row for every CLDR
//! `symbols-numberSystem-*` record in that inventory. The derived data is
//! distributed under Unicode License V3; see `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{io::Read, sync::OnceLock};

/// Decimal symbols and grouping metadata ready to construct an ICU4X payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumberDecimalSymbols {
    pub(crate) numbering_system: String,
    pub(crate) decimal_separator: String,
    pub(crate) grouping_separator: String,
    pub(crate) plus_sign: String,
    pub(crate) minus_sign: String,
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
                    let mut fields = line.splitn(9, '\t');
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
                197,
                "embedded CLDR decimal-symbol record count must remain pinned"
            );
            records
        })
        .as_slice()
}

/// Returns the CLDR default numbering system for an advertised language.
pub(super) fn default_numbering_system(locale: &str) -> Option<&'static str> {
    pinned_decimal_symbols()
        .iter()
        .find(|record| record.locale == locale)
        .map(|record| record.default_numbering_system.as_str())
}

/// Returns a locale's CLDR symbols for an explicitly resolved numbering
/// system. If CLDR has no system-specific symbols, ECMA-402 keeps the
/// locale's default symbols while substituting only the selected digits.
pub(super) fn decimal_symbols(
    locale: &str,
    numbering_system: &str,
) -> Option<NumberDecimalSymbols> {
    let records = pinned_decimal_symbols();
    let default_numbering_system = default_numbering_system(locale)?;
    let record = records
        .iter()
        .find(|record| record.locale == locale && record.numbering_system == numbering_system)
        .or_else(|| {
            records.iter().find(|record| {
                record.locale == locale && record.numbering_system == default_numbering_system
            })
        })?;
    let (primary_grouping, secondary_grouping) = grouping_sizes(&record.standard_pattern)?;
    Some(NumberDecimalSymbols {
        numbering_system: record.numbering_system.clone(),
        decimal_separator: record.decimal_separator.clone(),
        grouping_separator: record.grouping_separator.clone(),
        plus_sign: record.plus_sign.clone(),
        minus_sign: record.minus_sign.clone(),
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
}
