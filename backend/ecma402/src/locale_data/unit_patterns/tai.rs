// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tai raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Returns pinned Thai CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers. Thai long percent uses a non-breaking
/// space, while narrow records join the numeric placeholder directly.
pub(crate) fn cldr_thai_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("th")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} บิต",
            Unit::Byte => "{0} ไบต์",
            Unit::Celsius => "{0} องศาเซลเซียส",
            Unit::Degree => "{0} องศา",
            Unit::Fahrenheit => "{0} องศาฟาเรนไฮต์",
            Unit::Gigabit => "{0} กิกะบิต",
            Unit::Gigabyte => "{0} กิกะไบต์",
            Unit::Kilobit => "{0} กิโลบิต",
            Unit::Kilobyte => "{0} กิโลไบต์",
            Unit::Megabit => "{0} เมกะบิต",
            Unit::Megabyte => "{0} เมกะไบต์",
            Unit::Percent => "{0}\u{a0}เปอร์เซ็นต์",
            Unit::Petabyte => "{0} เพตะไบต์",
            Unit::Terabit => "{0} เทราบิต",
            Unit::Terabyte => "{0} เทราไบต์",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} บิต",
            Unit::Byte => "{0} ไบต์",
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
            Unit::Bit => "{0}บิต",
            Unit::Byte => "{0}ไบต์",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}PB",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Thailand's denominator-specific pinned CLDR `perUnitPattern`
// records, co-located with the Thai simple-unit records.

// Composes Thai generic compounds containing an ICU4X-untyped unit.
