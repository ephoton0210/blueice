// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Greek raw CLDR unit-pattern family.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Returns pinned Greek CLDR records for all ECMA-402 simple categories that
/// ICU4X's typed unit markers do not yet include.
pub(crate) fn cldr_greek_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("el")
    {
        return None;
    }
    let singular = plural == PluralCategory::One;
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => {
                if singular {
                    "{0} βαθμός Κελσίου"
                } else {
                    "{0} βαθμοί Κελσίου"
                }
            }
            Unit::Degree => {
                if singular {
                    "{0} μοίρα"
                } else {
                    "{0} μοίρες"
                }
            }
            Unit::Fahrenheit => {
                if singular {
                    "{0} βαθμός Φαρενάιτ"
                } else {
                    "{0} βαθμοί Φαρενάιτ"
                }
            }
            Unit::Gigabit => "{0} gigabit",
            Unit::Gigabyte => "{0} gigabyte",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobyte",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabyte",
            Unit::Percent => "{0} τοις εκατό",
            Unit::Petabyte => "{0} petabyte",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabyte",
            _ => return None,
        },
        Display::Short | Display::Narrow => match unit {
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

// Composes Greek generic compounds containing an ICU4X-untyped unit.
