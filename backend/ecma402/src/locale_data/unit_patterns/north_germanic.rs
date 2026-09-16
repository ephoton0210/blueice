// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! North Germanic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Returns pinned Swedish CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_swedish_additional_unit_pattern(
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
        != Some("sv")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match (unit, plural) {
            (Unit::Celsius, PluralCategory::One) => "{0} grad Celsius",
            (Unit::Celsius, _) => "{0} grader Celsius",
            (Unit::Degree, PluralCategory::One) => "{0} grad",
            (Unit::Degree, _) => "{0} grader",
            (Unit::Fahrenheit, PluralCategory::One) => "{0} grad Fahrenheit",
            (Unit::Fahrenheit, _) => "{0} grader Fahrenheit",
            (Unit::Bit, _) => "{0} bit",
            (Unit::Byte, _) => "{0} byte",
            (Unit::Gigabit, _) => "{0} gigabit",
            (Unit::Gigabyte, _) => "{0} gigabyte",
            (Unit::Kilobit, _) => "{0} kilobit",
            (Unit::Kilobyte, _) => "{0} kilobyte",
            (Unit::Megabit, _) => "{0} megabit",
            (Unit::Megabyte, _) => "{0} megabyte",
            (Unit::Percent, _) => "{0} procent",
            (Unit::Petabyte, _) => "{0} petabyte",
            (Unit::Terabit, _) => "{0} terabit",
            (Unit::Terabyte, _) => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
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
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}b",
            Unit::Byte => "{0}B",
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

// Returns Swedish denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Swedish generic compounds containing an ICU4X-untyped unit.

/// Returns pinned Danish CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_danish_additional_unit_pattern(
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
        != Some("da")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match (unit, plural) {
            (Unit::Byte, PluralCategory::One) => "{0} byte",
            (Unit::Byte, _) => "{0} bytes",
            (Unit::Celsius, PluralCategory::One) => "{0} grad celsius",
            (Unit::Celsius, _) => "{0} grader celsius",
            (Unit::Degree, PluralCategory::One) => "{0} grad",
            (Unit::Degree, _) => "{0} grader",
            (Unit::Fahrenheit, PluralCategory::One) => "{0} grad fahrenheit",
            (Unit::Fahrenheit, _) => "{0} grader fahrenheit",
            (Unit::Gigabyte, PluralCategory::One) => "{0} gigabyte",
            (Unit::Gigabyte, _) => "{0} gigabytes",
            (Unit::Kilobyte, PluralCategory::One) => "{0} kilobyte",
            (Unit::Kilobyte, _) => "{0} kilobytes",
            (Unit::Megabyte, PluralCategory::One) => "{0} megabyte",
            (Unit::Megabyte, _) => "{0} megabytes",
            (Unit::Petabyte, PluralCategory::One) => "{0} petabyte",
            (Unit::Petabyte, _) => "{0} petabytes",
            (Unit::Terabyte, PluralCategory::One) => "{0} terabyte",
            (Unit::Terabyte, _) => "{0} terabytes",
            (Unit::Bit, _) => "{0} bit",
            (Unit::Gigabit, _) => "{0} gigabit",
            (Unit::Kilobit, _) => "{0} kilobit",
            (Unit::Megabit, _) => "{0} megabit",
            (Unit::Percent, _) => "{0} procent",
            (Unit::Terabit, _) => "{0} terabit",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gbit",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kbit",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mbit",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0} pct.",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tbit",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gbit",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Danish denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Danish generic compounds containing an ICU4X-untyped unit.

/// Returns pinned Norwegian Bokmål CLDR records for every simple category
/// unavailable through ICU4X's typed unit markers. Raw `no` records are
/// identical to `nb` for this bounded family; Nynorsk remains distinct.
pub(crate) fn cldr_norwegian_bokmal_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    let language = locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next();
    if !matches!(language, Some("nb" | "no")) {
        return None;
    }
    let raw = match display {
        Display::Long => match (unit, plural) {
            (Unit::Celsius, PluralCategory::One) => "{0} grad celsius",
            (Unit::Celsius, _) => "{0} grader celsius",
            (Unit::Degree, PluralCategory::One) => "{0} grad",
            (Unit::Degree, _) => "{0} grader",
            (Unit::Fahrenheit, PluralCategory::One) => "{0} grad fahrenheit",
            (Unit::Fahrenheit, _) => "{0} grader fahrenheit",
            (Unit::Bit, _) => "{0} bit",
            (Unit::Byte, _) => "{0} byte",
            (Unit::Gigabit, _) => "{0} gigabit",
            (Unit::Gigabyte, _) => "{0} gigabyte",
            (Unit::Kilobit, _) => "{0} kilobit",
            (Unit::Kilobyte, _) => "{0} kilobyte",
            (Unit::Megabit, _) => "{0} megabit",
            (Unit::Megabyte, _) => "{0} megabyte",
            (Unit::Percent, _) => "{0} prosent",
            (Unit::Petabyte, _) => "{0} petabyte",
            (Unit::Terabit, _) => "{0} terabit",
            (Unit::Terabyte, _) => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} bit",
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
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}bit",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0} °C",
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

// Returns Norwegian Bokmål denominator-specific pinned CLDR
// `perUnitPattern` records.

// Composes Norwegian Bokmål generic compounds containing an ICU4X-untyped
// unit.

/// Returns pinned Norwegian Nynorsk CLDR records for every simple category
/// unavailable through ICU4X's typed unit markers.
pub(crate) fn cldr_norwegian_nynorsk_additional_unit_pattern(
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
        != Some("nn")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match (unit, plural) {
            (Unit::Celsius, PluralCategory::One) => "{0} grad celsius",
            (Unit::Celsius, _) => "{0} grader celsius",
            (Unit::Degree, PluralCategory::One) => "{0} grad",
            (Unit::Degree, _) => "{0} grader",
            (Unit::Fahrenheit, PluralCategory::One) => "{0} grad fahrenheit",
            (Unit::Fahrenheit, _) => "{0} grader fahrenheit",
            (Unit::Bit, _) => "{0} bit",
            (Unit::Byte, _) => "{0} byte",
            (Unit::Gigabit, _) => "{0} gigabit",
            (Unit::Gigabyte, _) => "{0} gigabyte",
            (Unit::Kilobit, _) => "{0} kilobit",
            (Unit::Kilobyte, _) => "{0} kilobyte",
            (Unit::Megabit, _) => "{0} megabit",
            (Unit::Megabyte, _) => "{0} megabyte",
            (Unit::Percent, _) => "{0} prosent",
            (Unit::Petabyte, _) => "{0} petabyte",
            (Unit::Terabit, _) => "{0} terabit",
            (Unit::Terabyte, _) => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}bit",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0}°",
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

// Returns Norwegian Nynorsk denominator-specific pinned CLDR
// `perUnitPattern` records.

// Composes Norwegian Nynorsk generic compounds containing an ICU4X-untyped
// unit.
