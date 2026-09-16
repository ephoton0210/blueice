// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Uralic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Returns pinned Hungarian CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_hungarian_additional_unit_pattern(
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
        != Some("hu")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bájt",
            Unit::Celsius => "{0} Celsius-fok",
            Unit::Degree => "{0} fok",
            Unit::Fahrenheit => "{0} Fahrenheit-fok",
            Unit::Gigabit => "{0} gigabit",
            Unit::Gigabyte => "{0} gigabájt",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobájt",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabájt",
            Unit::Percent => "{0} százalék",
            Unit::Petabyte => "{0} petabájt",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabájt",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bájt",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0} fok",
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
        Display::Narrow => match unit {
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
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Hungarian denominator-specific pinned CLDR `perUnitPattern`
// records, co-located with its Uralic simple-unit records.

// Composes Hungarian generic compounds containing an ICU4X-untyped unit.

/// Returns pinned Finnish CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_finnish_additional_unit_pattern(
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
        != Some("fi")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match (unit, plural) {
            (Unit::Bit, PluralCategory::One) => "{0} bitti",
            (Unit::Bit, _) => "{0} bittiä",
            (Unit::Byte, PluralCategory::One) => "{0} tavu",
            (Unit::Byte, _) => "{0} tavua",
            (Unit::Celsius, PluralCategory::One) => "{0} celsiusaste",
            (Unit::Celsius, _) => "{0} celsiusastetta",
            (Unit::Degree, PluralCategory::One) => "{0} aste",
            (Unit::Degree, _) => "{0} astetta",
            (Unit::Fahrenheit, PluralCategory::One) => "{0} fahrenheitaste",
            (Unit::Fahrenheit, _) => "{0} fahrenheitastetta",
            (Unit::Gigabit, PluralCategory::One) => "{0} gigabitti",
            (Unit::Gigabit, _) => "{0} gigabittiä",
            (Unit::Gigabyte, PluralCategory::One) => "{0} gigatavu",
            (Unit::Gigabyte, _) => "{0} gigatavua",
            (Unit::Kilobit, PluralCategory::One) => "{0} kilobitti",
            (Unit::Kilobit, _) => "{0} kilobittiä",
            (Unit::Kilobyte, PluralCategory::One) => "{0} kilotavu",
            (Unit::Kilobyte, _) => "{0} kilotavua",
            (Unit::Megabit, PluralCategory::One) => "{0} megabitti",
            (Unit::Megabit, _) => "{0} megabittiä",
            (Unit::Megabyte, PluralCategory::One) => "{0} megatavu",
            (Unit::Megabyte, _) => "{0} megatavua",
            (Unit::Percent, PluralCategory::One) => "{0} prosentti",
            (Unit::Percent, _) => "{0} prosenttia",
            (Unit::Petabyte, PluralCategory::One) => "{0} petatavu",
            (Unit::Petabyte, _) => "{0} petatavua",
            (Unit::Terabit, PluralCategory::One) => "{0} terabitti",
            (Unit::Terabit, _) => "{0} terabittiä",
            (Unit::Terabyte, PluralCategory::One) => "{0} teratavu",
            (Unit::Terabyte, _) => "{0} teratavua",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} t",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} Gt",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kt",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} Mt",
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} Pt",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} Tt",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}b",
            Unit::Byte => "{0}t",
            Unit::Celsius => "{0}°",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}Gt",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kt",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}Mt",
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0}Pt",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}Tt",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Finnish denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Finnish generic compounds containing an ICU4X-untyped unit.

fn is_estonian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("et")
}

fn estonian_cardinal_pattern(
    one: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Zero
        | crate::PluralCategory::Two
        | crate::PluralCategory::Few
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => other,
    }
}

/// Returns pinned Estonian CLDR records for simple categories that ICU4X's
/// typed unit markers do not provide with Estonian inflection.
pub(crate) fn cldr_estonian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_estonian(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => estonian_cardinal_pattern("{0} aaker", "{0} aakrit", plural),
            Unit::Bit => estonian_cardinal_pattern("{0} bitt", "{0} bitti", plural),
            Unit::Byte => estonian_cardinal_pattern("{0} bait", "{0} baiti", plural),
            Unit::Celsius => {
                estonian_cardinal_pattern("{0} Celsiuse kraad", "{0} Celsiuse kraadi", plural)
            }
            Unit::Degree => estonian_cardinal_pattern("{0} kraad", "{0} kraadi", plural),
            Unit::Fahrenheit => {
                estonian_cardinal_pattern("{0} Fahrenheiti kraad", "{0} Fahrenheiti kraadi", plural)
            }
            Unit::Gigabit => estonian_cardinal_pattern("{0} gigabitt", "{0} gigabitti", plural),
            Unit::Gigabyte => estonian_cardinal_pattern("{0} gigabait", "{0} gigabaiti", plural),
            Unit::Kilobit => estonian_cardinal_pattern("{0} kilobitt", "{0} kilobitti", plural),
            Unit::Kilobyte => estonian_cardinal_pattern("{0} kilobait", "{0} kilobaiti", plural),
            Unit::Megabit => estonian_cardinal_pattern("{0} megabitt", "{0} megabitti", plural),
            Unit::Megabyte => estonian_cardinal_pattern("{0} megabait", "{0} megabaiti", plural),
            Unit::Percent => estonian_cardinal_pattern("{0} protsent", "{0} protsenti", plural),
            Unit::Petabyte => estonian_cardinal_pattern("{0} petabait", "{0} petabaiti", plural),
            Unit::Terabit => estonian_cardinal_pattern("{0} terabitt", "{0} terabitti", plural),
            Unit::Terabyte => estonian_cardinal_pattern("{0} terabait", "{0} terabaiti", plural),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} b",
            Unit::Byte => estonian_cardinal_pattern("{0} bait", "{0} baiti", plural),
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
        Display::Narrow => match unit {
            Unit::Acre => estonian_cardinal_pattern("{0} aaker", "{0} aakrit", plural),
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

// Returns Estonian denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Estonian generic compounds containing an ICU4X-untyped unit.
