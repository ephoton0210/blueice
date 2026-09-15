// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Austronesian raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

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

/// Returns Indonesia's denominator-specific pinned CLDR `perUnitPattern`
/// records, co-located with its Austronesian simple-unit records.
pub(crate) fn cldr_indonesian_per_unit_pattern(
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
        != Some("id")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} per sentimeter",
        "{0} per hari",
        "{0} per kaki",
        "{0} per galon",
        "{0} per gram",
        "{0} per jam",
        "{0} per inci",
        "{0} per kilogram",
        "{0} per kilometer",
        "{0} per liter",
        "{0} per meter",
        "{0} per menit",
        "{0} per bulan",
        "{0} per ounce",
        "{0} per pound",
        "{0} per detik",
        "{0} per minggu",
        "{0} per tahun",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm", "{0}/hr", "{0}/ft", "{0}/gal", "{0}/g", "{0}/j", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/L", "{0}/m", "{0}/mnt", "{0}/bln", "{0}/oz", "{0}/lb", "{0}/dtk", "{0}/mgg",
        "{0}/thn",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm", "{0}/hr", "{0}/ft", "{0}/gal", "{0}/g", "{0}/j", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/L", "{0}/m", "{0}/mnt", "{0}/bln", "{0}/oz", "{0}/lb", "{0}/dtk", "{0}/mgg",
        "{0}/thn",
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

/// Composes Indonesian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_indonesian_generic_compound_unit_pattern(
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
        != Some("id")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_indonesian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_indonesian_additional_unit_pattern(
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
        "{0}/{1}",
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}

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

/// Returns Malaysia's denominator-specific pinned CLDR `perUnitPattern`
/// records, co-located with its Austronesian simple-unit records.
pub(crate) fn cldr_malay_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !uses_latin_malay_unit_data(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} setiap sentimeter",
        "{0} setiap hari",
        "{0} sekaki",
        "{0} segelen",
        "{0} setiap gram",
        "{0} sejam",
        "{0} seinci",
        "{0} setiap kilogram",
        "{0} setiap kilometer",
        "{0} setiap liter",
        "{0} setiap meter",
        "{0} setiap minit",
        "{0} setiap bulan",
        "{0} setiap auns",
        "{0} setiap paun",
        "{0} sesaat",
        "{0} setiap minggu",
        "{0} setiap tahun",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm", "{0}/h", "{0}/ka", "{0}/gal", "{0}/g", "{0}/j", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/bln", "{0}/oz", "{0}/lb", "{0}/s", "{0}/mgu", "{0}/thn",
    ];
    const NARROW: [&str; 18] = SHORT;
    let patterns = match display {
        Display::Long => &LONG,
        Display::Short => &SHORT,
        Display::Narrow => &NARROW,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Latin Malay generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_malay_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !uses_latin_malay_unit_data(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_malay_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_malay_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    )
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
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        match display {
            Display::Long => "{0} per {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}

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

/// Returns Jawi Malay's denominator-specific pinned CLDR `perUnitPattern`
/// records. CLDR 48.2.1 uses this one abbreviated table in every width.
pub(crate) fn cldr_jawi_malay_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    _display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    if !uses_jawi_malay_unit_data(locale) {
        return None;
    }
    const PATTERNS: [&str; 18] = [
        "{0}/cm",
        "{0}/d",
        "{0}/ft",
        "{0}/gal US",
        "{0}/g",
        "{0}/h",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/m",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/w",
        "{0}/y",
    ];
    PATTERNS
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Jawi Malay generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_jawi_malay_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    if !uses_jawi_malay_unit_data(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let numerator = cldr_jawi_malay_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_jawi_malay_additional_unit_pattern(
        locale,
        denominator_unit,
        display,
        crate::PluralCategory::One,
    )
    .or_else(|| {
        experimental_number_unit_pattern(
            locale,
            denominator_unit,
            display,
            crate::PluralCategory::One,
        )
    })
    .unwrap_or_else(|| {
        crate::locale_data_provider().number_unit_pattern(
            locale,
            denominator_unit,
            display,
            crate::PluralCategory::One,
        )
    });
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        "{0}/{1}",
    )
}

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

/// Returns Filipino denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_filipino_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_filipino(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} kada sentimetro",
        "{0} kada araw",
        "{0} kada talampakan",
        "{0} kada galon",
        "{0} kada gramo",
        "{0} kada oras",
        "{0} kada pulgada",
        "{0} kada kilo",
        "{0} kada kilometro",
        "{0} kada litro",
        "{0} kada metro",
        "{0} kada minuto",
        "{0} kada buwan",
        "{0} kada onsa",
        "{0} kada libra",
        "{0} kada segundo",
        "{0} kada linggo",
        "{0} kada taon",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/araw",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0} kada oras",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/buwan",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/linggo",
        "{0}/taon",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/araw",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/oras",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/L",
        "{0}/m",
        "{0}/min",
        "{0}/buwan",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/linggo",
        "{0}/taon",
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

/// Composes Filipino generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_filipino_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_filipino(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_filipino_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_filipino_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    )
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
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        match display {
            Display::Long => "{0} kada {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}

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

/// Returns Javanese denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_javanese_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_javanese(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} saben sentimeter",
        "{0} saben dina",
        "{0} saben kaki",
        "{0} saben galon",
        "{0} saben gram",
        "{0} saben jam",
        "{0} saben inci",
        "{0} saben kilogram",
        "{0} saben kilometer",
        "{0} saben liter",
        "{0} saben meter",
        "{0} saben menit",
        "{0} saben sasi",
        "{0} saben ons",
        "{0} saben pon",
        "{0} saben detik",
        "{0} saben peken",
        "{0} saben taun",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/d",
        "{0}/kaki",
        "{0}/galon",
        "{0}/g",
        "{0}/jam",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/L",
        "{0}/m",
        "{0}/mnt",
        "{0}/sasi",
        "{0}/ons",
        "{0}/pon",
        "{0}/dtk",
        "{0}/peken",
        "{0}/taun",
    ];
    let patterns = match display {
        Display::Long => &LONG,
        Display::Short | Display::Narrow => &SHORT,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Javanese generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_javanese_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_javanese(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_javanese_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_javanese_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::Other,
    )
    .or_else(|| {
        experimental_number_unit_pattern(
            locale,
            denominator_unit,
            denominator_display,
            crate::PluralCategory::Other,
        )
    })
    .unwrap_or_else(|| {
        crate::locale_data_provider().number_unit_pattern(
            locale,
            denominator_unit,
            denominator_display,
            crate::PluralCategory::Other,
        )
    });
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        match display {
            Display::Long => "{0} saben {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
