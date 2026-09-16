// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Kabuverdianu raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_kabuverdianu(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("kea")
}

/// Returns pinned Kabuverdianu CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_kabuverdianu_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_kabuverdianu(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} ákri",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0} grau Celsius",
            Unit::Degree => "{0} grau",
            Unit::Fahrenheit => "{0} grau Fahrenheit",
            Unit::Gigabit => "{0} jigabit",
            Unit::Gigabyte => "{0} jigabyte",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobyte",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabyte",
            Unit::Percent => "{0} pursentu",
            Unit::Petabyte => "{0} petabyte",
            Unit::Second => "{0} sigundu",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
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
            Unit::Second => "{0} sig.",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} ac",
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
            Unit::Second => "{0} s",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Kabuverdianu denominator-specific pinned CLDR `perUnitPattern`
// records.

// Composes Kabuverdianu generic compounds containing an ICU4X-untyped unit.
