// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Uralic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

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

/// Returns Hungarian denominator-specific pinned CLDR `perUnitPattern`
/// records, co-located with its Uralic simple-unit records.
pub(crate) fn cldr_hungarian_per_unit_pattern(
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
        != Some("hu")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0}/centimeter",
        "{0}/nap",
        "{0}/láb",
        "{0}/gallon",
        "{0}/gramm",
        "{0}/óra",
        "{0}/hüvelyk",
        "{0}/kilogramm",
        "{0}/kilométer",
        "{0}/liter",
        "{0}/méter",
        "{0}/perc",
        "{0}/hónap",
        "{0}/uncia",
        "{0}/font",
        "{0}/másodperc",
        "{0}/hét",
        "{0}/év",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm", "{0}/nap", "{0}/láb", "{0}/gal", "{0}/g", "{0}/ó", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/p", "{0}/hó", "{0}/oz", "{0}/lb", "{0}/mp", "{0}/hét", "{0}/év",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm", "{0}/nap", "{0}/láb", "{0}/gal", "{0}/g", "{0}/ó", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/p", "{0}/hó", "{0}/oz", "{0}/lb", "{0}/mp", "{0}/hét", "{0}/év",
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

/// Composes Hungarian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_hungarian_generic_compound_unit_pattern(
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
        != Some("hu")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_hungarian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_hungarian_additional_unit_pattern(
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

/// Returns Finnish denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_finnish_per_unit_pattern(
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
        != Some("fi")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} / senttimetri",
        "{0} / päivä",
        "{0} / jalka",
        "{0} / am. gallona",
        "{0} / gramma",
        "{0} / tunti",
        "{0} / tuuma",
        "{0} / kilogramma",
        "{0} / kilometri",
        "{0} / litra",
        "{0} / metri",
        "{0} / minuutti",
        "{0} / kuukausi",
        "{0} / unssi",
        "{0} / pauna",
        "{0} / sekunti",
        "{0} / viikko",
        "{0} / vuosi",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/pv",
        "{0}/ft",
        "{0}/am. gal",
        "{0}/g",
        "{0}/t",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/kk",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/vk",
        "{0}/v",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/pv",
        "{0}/′",
        "{0}/am.gal",
        "{0}/g",
        "{0}/t",
        "{0}/″",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/kk",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/vk",
        "{0}/v",
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

/// Composes Finnish generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_finnish_generic_compound_unit_pattern(
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
        != Some("fi")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_finnish_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_finnish_additional_unit_pattern(
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
            Display::Long => "{0} / {1}",
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

/// Returns Estonian denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_estonian_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_estonian(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} sentimeetri kohta",
        "{0} ööpäevas",
        "{0} jala kohta",
        "{0} galloni kohta",
        "{0} grammi kohta",
        "{0} tunnis",
        "{0} tolli kohta",
        "{0} kilogrammi kohta",
        "{0} kilomeetri kohta",
        "{0} liitri kohta",
        "{0} meetri kohta",
        "{0} minutis",
        "{0} kuus",
        "{0} untsi kohta",
        "{0} naela kohta",
        "{0} sekundis",
        "{0} nädalas",
        "{0} aastas",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/ööp",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/t",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/k",
        "{0}/oz",
        "{0}/lb",
        "{0}/sek",
        "{0}/näd",
        "{0}/a",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/ööp",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/t",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/k",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/näd",
        "{0}/a",
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

/// Composes Estonian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_estonian_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_estonian(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_estonian_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_estonian_additional_unit_pattern(
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
            Display::Long => "{0} {1} kohta",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
