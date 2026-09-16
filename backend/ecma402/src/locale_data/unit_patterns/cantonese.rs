// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cantonese raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_cantonese(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("yue")
}

/// Returns pinned Cantonese CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_cantonese_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_cantonese(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} 英畝",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "攝氏 {0} 度",
            Unit::Degree => "{0} 度",
            Unit::Fahrenheit => "華氏 {0} 度",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} 秒",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} 英畝",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} 度",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} 秒",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} 英畝",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} 度",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} 秒",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Cantonese denominator-specific pinned CLDR `perUnitPattern`
// records.

// Composes Cantonese generic compounds containing an ICU4X-untyped unit.
