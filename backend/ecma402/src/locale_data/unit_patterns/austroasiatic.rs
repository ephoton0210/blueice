// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Austroasiatic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_khmer(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("km")
}

/// Returns pinned Khmer CLDR records for simple categories that ICU4X's typed
/// unit markers do not provide with Khmer data.
pub(crate) fn cldr_khmer_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_khmer(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} អា",
            Unit::Bit => "{0}\u{a0}ប៊ីត",
            Unit::Byte => "{0} បៃ",
            Unit::Celsius => "{0} អង្សាសេ",
            Unit::Degree => "{0} ដឺក្រេ",
            Unit::Fahrenheit => "{0}\u{a0}អង្សា\u{200b}ហ្វារិនហៃ",
            Unit::Gigabit => "{0}\u{a0}ជីកាប៊ីត",
            Unit::Gigabyte => "{0} ជីកាបៃ",
            Unit::Kilobit => "{0} គីឡូប៊ីត",
            Unit::Kilobyte => "{0}\u{a0}គីឡូបៃ",
            Unit::Megabit => "{0} មេកាប៊ីត",
            Unit::Megabyte => "{0}\u{a0}មេកាបៃ",
            Unit::Percent => "{0} ភាគរយ",
            Unit::Petabyte => "{0} ប៉េតាបៃ",
            Unit::Terabit => "{0} តេរ៉ាប៊ីត",
            Unit::Terabyte => "{0} តេរ៉ាបៃ",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} អា",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Khmer denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Khmer generic compounds containing an ICU4X-untyped unit.
