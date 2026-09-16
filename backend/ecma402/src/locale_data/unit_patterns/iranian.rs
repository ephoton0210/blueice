// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Iranian raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Returns pinned Persian CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_persian_additional_unit_pattern(
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
        != Some("fa")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} بیت",
            Unit::Byte => "{0} بایت",
            Unit::Celsius => "{0} درجهٔ سلسیوس",
            Unit::Degree => "{0} درجه",
            Unit::Fahrenheit => "{0} درجهٔ فارنهایت",
            Unit::Gigabit => "{0} گیگابیت",
            Unit::Gigabyte => "{0} گیگابایت",
            Unit::Kilobit => "{0} کیلوبیت",
            Unit::Kilobyte => "{0} کیلوبایت",
            Unit::Megabit => "{0} مگابیت",
            Unit::Megabyte => "{0} مگابایت",
            Unit::Percent => "{0} درصد",
            Unit::Petabyte => "{0} پتابایت",
            Unit::Terabit => "{0} ترابیت",
            Unit::Terabyte => "{0} ترابایت",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} بیت",
            Unit::Byte => "{0} بایت",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} درجه",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}٪",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} بیت",
            Unit::Byte => "{0} بایت",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}٪",
            Unit::Petabyte => "{0}PB",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Persian denominator-specific pinned CLDR `perUnitPattern`
// records, co-located with its Iranian simple-unit records.

// Composes Persian generic compounds containing an ICU4X-untyped unit.
