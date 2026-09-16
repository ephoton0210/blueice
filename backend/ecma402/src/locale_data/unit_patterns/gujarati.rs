// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Gujarati raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_gujarati(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("gu")
}

/// Returns pinned Gujarati CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_gujarati_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_gujarati(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} એકર",
            Unit::Bit => "{0} બિટ",
            Unit::Byte => "{0} બાઇટ",
            Unit::Celsius => "{0} ડિગ્રી સેલ્સિયસ",
            Unit::Degree => "{0} અંશ",
            Unit::Fahrenheit => "{0} ડિગ્રી ફેરનહીટ",
            Unit::Gigabit => "{0} ગીગાબિટ",
            Unit::Gigabyte => "{0} ગીગાબાઇટ",
            Unit::Kilobit => "{0} કિલોબિટ",
            Unit::Kilobyte => "{0} કિલોબાઇટ",
            Unit::Megabit => "{0} મેગાબિટ",
            Unit::Megabyte => "{0} મેગાબાઇટ",
            Unit::Percent => "{0} ટકા",
            Unit::Petabyte => "{0} પેટાબાઈટ્સ",
            Unit::Second => "{0} સેકંડ",
            Unit::Terabit => "{0} ટેરાબિટ",
            Unit::Terabyte => "{0} ટેરાબાઇટ",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} એકર",
            Unit::Bit => "{0} બિટ",
            Unit::Byte => "{0} બાઇટ",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} અંશ",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} સેકંડ",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} એકર",
            Unit::Bit => "{0} બિટ",
            Unit::Byte => "{0} બાઇટ",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} અંશ",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} સે",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Gujarati denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Gujarati generic compounds containing an ICU4X-untyped unit.
