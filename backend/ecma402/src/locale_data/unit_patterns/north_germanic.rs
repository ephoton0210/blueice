// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! North Germanic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

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

/// Returns Swedish denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_swedish_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("sv")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} per centimeter",
        "{0} per dygn",
        "{0} per fot",
        "{0} per gallon",
        "{0} per gram",
        "{0} per timme",
        "{0} per tum",
        "{0} per kilogram",
        "{0} per kilometer",
        "{0} per liter",
        "{0} per meter",
        "{0} per minut",
        "{0} per månad",
        "{0} per uns",
        "{0} per pund",
        "{0} per sekund",
        "{0} per vecka",
        "{0} per år",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/d",
        "{0}/fot",
        "{0}/gal US",
        "{0}/g",
        "{0}/tim",
        "{0}/tum",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/mån",
        "{0}/uns",
        "{0}/pund",
        "{0}/s",
        "{0}/v",
        "{0}/år",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/d",
        "{0}/fot",
        "{0}/gal US",
        "{0}/g",
        "{0}/tim",
        "{0}/tum",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/m",
        "{0}/m",
        "{0}/uns",
        "{0}/pund",
        "{0}/s",
        "{0}/v",
        "{0}/år",
    ];
    let patterns = match display {
        Display::Long => &LONG,
        Display::Short => &SHORT,
        Display::Narrow => &NARROW,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Swedish generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_swedish_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("sv")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_swedish_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_swedish_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    );
    if numerator_raw.is_none() && denominator_raw.is_none() {
        return None;
    }
    let numerator = numerator_raw
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = denominator_raw
        .or_else(|| {
            experimental_number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        })
        .unwrap_or_else(|| {
            crate::locale_data_provider().number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        });
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        match display {
            Display::Long => "{0} per {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}

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

/// Returns Danish denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_danish_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("da")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} pr. centimeter",
        "{0} pr. dag",
        "{0} pr. fod",
        "{0}/gal",
        "{0} pr. gram",
        "{0} pr. time",
        "{0} pr. tomme",
        "{0} pr. kg",
        "{0} pr. kilometer",
        "{0}/l",
        "{0} pr. meter",
        "{0} pr. min.",
        "{0} pr. måned",
        "{0} pr. ounce",
        "{0} pr. pund",
        "{0} pr. sekund",
        "{0} pr. uge",
        "{0} om året",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/dag",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/t.",
        "{0}/tomme",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min.",
        "{0}/md.",
        "{0}/oz",
        "{0}/lb",
        "{0}/sek.",
        "{0}/uge",
        "{0}/år",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/d",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/t",
        "{0}/tomme",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/m",
        "{0}/m",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/u",
        "{0}/år",
    ];
    let patterns = match display {
        Display::Long => &LONG,
        Display::Short => &SHORT,
        Display::Narrow => &NARROW,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Danish generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_danish_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("da")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_danish_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_danish_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    );
    if numerator_raw.is_none() && denominator_raw.is_none() {
        return None;
    }
    let numerator = numerator_raw
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = denominator_raw
        .or_else(|| {
            experimental_number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        })
        .unwrap_or_else(|| {
            crate::locale_data_provider().number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        });
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        match display {
            Display::Long => "{0} pr. {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}

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

/// Returns Norwegian Bokmål denominator-specific pinned CLDR
/// `perUnitPattern` records.
pub(crate) fn cldr_norwegian_bokmal_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    let language = locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next();
    if !matches!(language, Some("nb" | "no")) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} per centimeter",
        "{0} per døgn",
        "{0} per fot",
        "{0} per gallon",
        "{0} per gram",
        "{0} per time",
        "{0} per tomme",
        "{0} per kilogram",
        "{0} per kilometer",
        "{0} per liter",
        "{0} per meter",
        "{0} per minutt",
        "{0} per måned",
        "{0} per unse",
        "{0} per pund",
        "{0} per sekund",
        "{0} per uke",
        "{0} per år",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/d",
        "{0}/fot",
        "{0}/gal",
        "{0}/g",
        "{0}/t",
        "{0}/tomme",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/md.",
        "{0}/unse",
        "{0}/pund",
        "{0}/s",
        "{0}/u",
        "{0}/år",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm", "{0}/d", "{0}/ft", "{0}/gal", "{0}/g", "{0}/t", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/m", "{0}/unse", "{0}/pund", "{0}/s", "{0}/u", "{0}/år",
    ];
    let patterns = match display {
        Display::Long => &LONG,
        Display::Short => &SHORT,
        Display::Narrow => &NARROW,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Norwegian Bokmål generic compounds containing an ICU4X-untyped
/// unit.
pub(crate) fn cldr_norwegian_bokmal_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    let language = locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next();
    if !matches!(language, Some("nb" | "no")) {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw =
        cldr_norwegian_bokmal_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_norwegian_bokmal_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    );
    if numerator_raw.is_none() && denominator_raw.is_none() {
        return None;
    }
    let numerator = numerator_raw
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = denominator_raw
        .or_else(|| {
            experimental_number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        })
        .unwrap_or_else(|| {
            crate::locale_data_provider().number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        });
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        match display {
            Display::Long => "{0} per {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}

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

/// Returns Norwegian Nynorsk denominator-specific pinned CLDR
/// `perUnitPattern` records.
pub(crate) fn cldr_norwegian_nynorsk_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("nn")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} per centimeter",
        "{0} per døgn",
        "{0} per fot",
        "{0} per gallon",
        "{0} per gram",
        "{0} per time",
        "{0} per tomme",
        "{0} per kilogram",
        "{0} per kilometer",
        "{0} per liter",
        "{0} per meter",
        "{0} per minutt",
        "{0} per månad",
        "{0} per unse",
        "{0} per pund",
        "{0} per sekund",
        "{0}\u{a0}per veke",
        "{0} per år",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/d",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/t",
        "{0}/tomme",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/md.",
        "{0}/unse",
        "{0}/pund",
        "{0}/s",
        "{0}/v",
        "{0}/år",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm", "{0}/d", "{0}/ft", "{0}/gal", "{0}/g", "{0}/h", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/m", "{0}/unse", "{0}/pund", "{0}/s", "{0}/v", "{0}/år",
    ];
    let patterns = match display {
        Display::Long => &LONG,
        Display::Short => &SHORT,
        Display::Narrow => &NARROW,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Norwegian Nynorsk generic compounds containing an ICU4X-untyped
/// unit.
pub(crate) fn cldr_norwegian_nynorsk_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("nn")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw =
        cldr_norwegian_nynorsk_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_norwegian_nynorsk_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    );
    if numerator_raw.is_none() && denominator_raw.is_none() {
        return None;
    }
    let numerator = numerator_raw
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = denominator_raw
        .or_else(|| {
            experimental_number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        })
        .unwrap_or_else(|| {
            crate::locale_data_provider().number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        });
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        match display {
            Display::Long => "{0} per {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}
