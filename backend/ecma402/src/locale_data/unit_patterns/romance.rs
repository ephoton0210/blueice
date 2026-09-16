// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Romance raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn romanian_cardinal_pattern(
    one: &'static str,
    few: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Few => few,
        crate::PluralCategory::Zero
        | crate::PluralCategory::Two
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => other,
    }
}

/// Returns pinned Romanian CLDR records for simple categories unavailable
/// through ICU4X typed unit markers. Romanian's `other` cardinal form carries
/// the required `de` preposition, so a simple one/other fallback is wrong.
pub(crate) fn cldr_romanian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ro")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => romanian_cardinal_pattern("{0} bit", "{0} biți", "{0} de biți", plural),
            Unit::Byte => romanian_cardinal_pattern("{0} byte", "{0} byți", "{0} de byți", plural),
            Unit::Celsius => romanian_cardinal_pattern(
                "{0} grad Celsius",
                "{0} grade Celsius",
                "{0} de grade Celsius",
                plural,
            ),
            Unit::Degree => {
                romanian_cardinal_pattern("{0} grad", "{0} grade", "{0} de grade", plural)
            }
            Unit::Fahrenheit => romanian_cardinal_pattern(
                "{0} grad Fahrenheit",
                "{0} grade Fahrenheit",
                "{0} de grade Fahrenheit",
                plural,
            ),
            Unit::Gigabit => {
                romanian_cardinal_pattern("{0} gigabit", "{0} gigabiți", "{0} de gigabiți", plural)
            }
            Unit::Gigabyte => {
                romanian_cardinal_pattern("{0} gigabyte", "{0} gigabyți", "{0} de gigabyți", plural)
            }
            Unit::Kilobit => {
                romanian_cardinal_pattern("{0} kilobit", "{0} kilobiți", "{0} de kilobiți", plural)
            }
            Unit::Kilobyte => {
                romanian_cardinal_pattern("{0} kilobyte", "{0} kilobyți", "{0} de kilobyți", plural)
            }
            Unit::Megabit => {
                romanian_cardinal_pattern("{0} megabit", "{0} megabiți", "{0} de megabiți", plural)
            }
            Unit::Megabyte => {
                romanian_cardinal_pattern("{0} megabyte", "{0} megabyți", "{0} de megabyți", plural)
            }
            Unit::Percent => {
                romanian_cardinal_pattern("{0} procent", "{0} procente", "{0} de procente", plural)
            }
            Unit::Petabyte => {
                romanian_cardinal_pattern("{0} petabyte", "{0} petabyți", "{0} de petabyți", plural)
            }
            Unit::Terabit => {
                romanian_cardinal_pattern("{0} terabit", "{0} terabiți", "{0} de terabiți", plural)
            }
            Unit::Terabyte => {
                romanian_cardinal_pattern("{0} terabyte", "{0} terabyți", "{0} de terabyți", plural)
            }
            _ => return None,
        },
        Display::Short | Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
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

// Returns Romania's denominator-specific pinned CLDR `perUnitPattern`
// records. Keep these tables with the Romance simple-unit family so the
// shared legacy table does not become the future growth point.

// Composes Romanian generic compounds containing an ICU4X-untyped unit.
