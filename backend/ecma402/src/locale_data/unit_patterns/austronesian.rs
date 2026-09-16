// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Austronesian raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Returns pinned Indonesian CLDR records for every simple category
/// unavailable through ICU4X's typed unit markers.
pub(crate) fn cldr_indonesian_additional_unit_pattern(
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
        != Some("id")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0} derajat Celsius",
            Unit::Degree => "{0} derajat",
            Unit::Fahrenheit => "{0} derajat Fahrenheit",
            Unit::Gigabit => "{0} gigabit",
            Unit::Gigabyte => "{0} gigabyte",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobyte",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabyte",
            Unit::Percent => "{0} persen",
            Unit::Petabyte => "{0} petabyte",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
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
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°",
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

// Returns Indonesia's denominator-specific pinned CLDR `perUnitPattern`
// records, co-located with its Austronesian simple-unit records.

// Composes Indonesian generic compounds containing an ICU4X-untyped unit.

fn uses_latin_malay_unit_data(locale: &str) -> bool {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut subtags = base.split('-');
    subtags.next() == Some("ms") && !subtags.any(|subtag| subtag.eq_ignore_ascii_case("Arab"))
}

fn uses_jawi_malay_unit_data(locale: &str) -> bool {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut subtags = base.split('-');
    subtags.next() == Some("ms") && subtags.any(|subtag| subtag.eq_ignore_ascii_case("Arab"))
}

/// Returns pinned Latin-script Malay CLDR records for simple categories that
/// ICU4X's typed unit markers do not provide with Malay data.
///
/// Jawi (`ms-Arab`) has separate CLDR records and must not inherit these
/// Latin-script labels.
pub(crate) fn cldr_malay_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !uses_latin_malay_unit_data(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} ekar",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bait",
            Unit::Celsius => "{0} darjah Celsius",
            Unit::Degree => "{0} darjah",
            Unit::Fahrenheit => "{0} darjah Fahrenheit",
            Unit::Gigabit => "{0} gigabit",
            Unit::Gigabyte => "{0} gigabait",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobait",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabait",
            Unit::Percent => "{0} peratus",
            Unit::Petabyte => "{0} petabait",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabait",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ekar",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bait",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} darjah",
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
            Unit::Acre => "{0} ekar",
            Unit::Bit => "{0}bit",
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

// Returns Malaysia's denominator-specific pinned CLDR `perUnitPattern`
// records, co-located with its Austronesian simple-unit records.

// Composes Latin Malay generic compounds containing an ICU4X-untyped unit.

/// Returns pinned Jawi Malay CLDR records for simple categories whose typed
/// ICU4X markers have no `ms-Arab` data.
///
/// CLDR 48.2.1 intentionally uses the same compact Latin abbreviations in all
/// three widths for this script family; it must not inherit the Latin Malay
/// words such as `gigabait` or `darjah`.
pub(crate) fn cldr_jawi_malay_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    _display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::NumberFormatUnit as Unit;

    if !uses_jawi_malay_unit_data(locale) {
        return None;
    }
    let raw = match unit {
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
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Jawi Malay's denominator-specific pinned CLDR `perUnitPattern`
// records. CLDR 48.2.1 uses this one abbreviated table in every width.

// Composes Jawi Malay generic compounds containing an ICU4X-untyped unit.

fn is_filipino(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("fil")
}

fn filipino_cardinal_pattern(
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

/// Returns pinned Filipino CLDR records for simple categories that ICU4X's
/// typed unit markers do not provide with Filipino morphology.
///
/// Filipino uses `na` for many non-singular forms, so its raw records must not
/// fall back to superficially similar English labels.
pub(crate) fn cldr_filipino_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_filipino(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => filipino_cardinal_pattern("{0} acre", "{0} acres", plural),
            Unit::Bit => filipino_cardinal_pattern("{0} bit", "{0} na bit", plural),
            Unit::Byte => filipino_cardinal_pattern("{0} byte", "{0} na byte", plural),
            Unit::Celsius => {
                filipino_cardinal_pattern("{0} degree Celsius", "{0} degrees Celsius", plural)
            }
            Unit::Degree => filipino_cardinal_pattern("{0} degree", "{0} na degree", plural),
            Unit::Fahrenheit => {
                filipino_cardinal_pattern("{0} degree Fahrenheit", "{0} degrees Fahrenheit", plural)
            }
            Unit::Gigabit => filipino_cardinal_pattern("{0} gigabit", "{0} na gigabit", plural),
            Unit::Gigabyte => filipino_cardinal_pattern("{0} gigabyte", "{0} na gigabyte", plural),
            Unit::Kilobit => filipino_cardinal_pattern("{0} kilobit", "{0} na kilobit", plural),
            Unit::Kilobyte => filipino_cardinal_pattern("{0} kilobyte", "{0} na kilobyte", plural),
            Unit::Megabit => filipino_cardinal_pattern("{0} megabit", "{0} na megabit", plural),
            Unit::Megabyte => filipino_cardinal_pattern("{0} megabyte", "{0} na megabyte", plural),
            Unit::Percent => filipino_cardinal_pattern("{0} porsyento", "{0} na porsyento", plural),
            Unit::Petabyte => filipino_cardinal_pattern("{0} petabyte", "{0} petabytes", plural),
            Unit::Terabit => filipino_cardinal_pattern("{0} terabit", "{0} na terabit", plural),
            Unit::Terabyte => filipino_cardinal_pattern("{0} terabyte", "{0} na terabyte", plural),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°C",
            Unit::Degree => filipino_cardinal_pattern("{0} deg", "{0} na deg", plural),
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
            Unit::Acre => "{0}ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => filipino_cardinal_pattern("{0} deg", "{0} na deg", plural),
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

// Returns Filipino denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Filipino generic compounds containing an ICU4X-untyped unit.

fn is_javanese(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("jv")
}

/// Returns pinned Javanese CLDR records for simple categories unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_javanese_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_javanese(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} are",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bite",
            Unit::Celsius => "{0} derajat celsius",
            Unit::Degree => "{0} derajat",
            Unit::Fahrenheit => "{0} derajat Fahrenhet",
            Unit::Gigabit => "{0} gigabit",
            Unit::Gigabyte => "{0} gigabite",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobite",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabite",
            Unit::Percent => "{0} persen",
            Unit::Petabyte => "{0} petabite",
            Unit::Second => "{0} detik",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabite",
            _ => return None,
        },
        Display::Short | Display::Narrow => match unit {
            Unit::Acre => "{0} are",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bite",
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
            Unit::Second => "{0} dtk",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Javanese denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Javanese generic compounds containing an ICU4X-untyped unit.
