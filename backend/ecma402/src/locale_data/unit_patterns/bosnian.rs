// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bosnian raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

#[derive(Clone, Copy)]
enum BosnianScript {
    Cyrillic,
    Latin,
}

fn bosnian_script(locale: &str) -> Option<BosnianScript> {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut subtags = base.split('-');
    (subtags.next()? == "bs").then_some(())?;
    if subtags.any(|subtag| subtag.eq_ignore_ascii_case("Cyrl")) {
        Some(BosnianScript::Cyrillic)
    } else {
        Some(BosnianScript::Latin)
    }
}

fn bosnian_unit_index(unit: crate::NumberFormatUnit) -> Option<usize> {
    use crate::NumberFormatUnit as Unit;

    Some(match unit {
        Unit::Acre => 0,
        Unit::Bit => 1,
        Unit::Byte => 2,
        Unit::Celsius => 3,
        Unit::Degree => 4,
        Unit::Fahrenheit => 5,
        Unit::Gigabit => 6,
        Unit::Gigabyte => 7,
        Unit::Kilobit => 8,
        Unit::Kilobyte => 9,
        Unit::Megabit => 10,
        Unit::Megabyte => 11,
        Unit::Percent => 12,
        Unit::Petabyte => 13,
        Unit::Terabit => 14,
        Unit::Terabyte => 15,
        Unit::Second => 16,
        _ => return None,
    })
}

fn bosnian_cardinal_index(plural: crate::PluralCategory) -> usize {
    match plural {
        crate::PluralCategory::One => 0,
        crate::PluralCategory::Few => 1,
        crate::PluralCategory::Zero
        | crate::PluralCategory::Two
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => 2,
    }
}

fn bosnian_latin_long_pattern(
    unit: crate::NumberFormatUnit,
    plural: crate::PluralCategory,
) -> Option<&'static str> {
    const LONG: [[&str; 3]; 17] = [
        [
            "{0} katastarsko jutro",
            "{0} katastarska jutra",
            "{0} katastarskih jutara",
        ],
        ["{0} bit", "{0} bita", "{0} bita"],
        ["{0} bajt", "{0} bajta", "{0} bajtova"],
        [
            "{0} stepen Celzijusa",
            "{0} stepena Celzijusa",
            "{0} stepeni Celzijusa",
        ],
        ["{0} stepen", "{0} stepena", "{0} stepeni"],
        [
            "{0} stepen Farenhajta",
            "{0} stepena Farenhajta",
            "{0} stepeni Farenhajta",
        ],
        ["{0} gigabit", "{0} gigabita", "{0} gigabita"],
        ["{0} gigabajt", "{0} gigabajta", "{0} gigabajta"],
        ["{0} kilobit", "{0} kilobita", "{0} kilobita"],
        ["{0} kilobajt", "{0} kilobajta", "{0} kilobajta"],
        ["{0} megabit", "{0} megabita", "{0} megabita"],
        ["{0} megabajt", "{0} megabajta", "{0} megabajta"],
        ["{0} procenat", "{0} procenta", "{0} procenata"],
        ["{0} petabajt", "{0} petabajta", "{0} petabajta"],
        ["{0} terabit", "{0} terabita", "{0} terabita"],
        ["{0} terabajt", "{0} terabajta", "{0} terabajta"],
        ["{0} sekunda", "{0} sekunde", "{0} sekundi"],
    ];
    LONG.get(bosnian_unit_index(unit)?)
        .map(|patterns| patterns[bosnian_cardinal_index(plural)])
}

/// Returns pinned Bosnian Latin and Cyrillic CLDR records for every ECMA-402
/// simple-unit category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_bosnian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    let script = bosnian_script(locale)?;
    let raw = match script {
        BosnianScript::Latin => match display {
            Display::Long => bosnian_latin_long_pattern(unit, plural)?,
            Display::Short => match unit {
                Unit::Acre => "{0} ac",
                Unit::Bit => "{0} bit",
                Unit::Byte => "{0} bajt",
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
                Unit::Second => "{0} sek.",
                Unit::Terabit => "{0} Tb",
                Unit::Terabyte => "{0} TB",
                _ => return None,
            },
            Display::Narrow => match unit {
                Unit::Acre => "{0} ac",
                Unit::Bit => "{0} bit",
                Unit::Byte => "{0} B",
                Unit::Celsius => "{0}°",
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
        },
        BosnianScript::Cyrillic => match (display, unit) {
            (Display::Long, Unit::Acre) => {
                ["{0} акра", "{0} акре", "{0} акри"][bosnian_cardinal_index(plural)]
            }
            (Display::Short | Display::Narrow, Unit::Acre) => "{0} ac",
            (Display::Long, Unit::Second) => {
                ["{0} секунда", "{0} секунде", "{0} секунди"][bosnian_cardinal_index(plural)]
            }
            (Display::Short | Display::Narrow, Unit::Second) => "{0} сек.",
            (_, Unit::Bit) => "{0} bit",
            (_, Unit::Byte) => "{0} byte",
            (_, Unit::Celsius) => "{0}°C",
            (_, Unit::Degree) => "{0}°",
            (_, Unit::Fahrenheit) => "{0}°F",
            (_, Unit::Gigabit) => "{0} Gb",
            (_, Unit::Gigabyte) => "{0} GB",
            (_, Unit::Kilobit) => "{0} kb",
            (_, Unit::Kilobyte) => "{0} kB",
            (_, Unit::Megabit) => "{0} Mb",
            (_, Unit::Megabyte) => "{0} MB",
            (_, Unit::Percent) => "{0}%",
            (_, Unit::Petabyte) => "{0} PB",
            (_, Unit::Terabit) => "{0} Tb",
            (_, Unit::Terabyte) => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Bosnian Latin and Cyrillic pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_bosnian_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    const LATIN_LONG: [&str; 18] = [
        "{0} po centimetru",
        "{0} dnevno",
        "{0} po stopi",
        "{0} po galonu",
        "{0} po gramu",
        "{0} na sat",
        "{0} po inču",
        "{0} po kilogramu",
        "{0} po kilometru",
        "{0} po litru",
        "{0} po metru",
        "{0} po minuti",
        "{0} mjesečno",
        "{0} po unci",
        "{0} po funti",
        "{0} po sekundi",
        "{0} sedmično",
        "{0} godišnje",
    ];
    const LATIN_SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/d.",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/h",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min.",
        "{0} mj.",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/sedm.",
        "{0}/god.",
    ];
    const LATIN_NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/d.",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/h",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0} mj.",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/sedm.",
        "{0}/god.",
    ];
    const CYRILLIC_LONG: [&str; 18] = [
        "{0} по центиметру",
        "{0} дневно",
        "{0}/ft",
        "{0}/gal US",
        "{0}/g",
        "{0} по сату",
        "{0}/in",
        "{0}/kg",
        "{0} по километру",
        "{0}/l",
        "{0} по метру",
        "{0} у минуту",
        "{0} мјесечно",
        "{0}/oz",
        "{0}/lb",
        "{0} у секунди",
        "{0} седмично",
        "{0} годишње",
    ];
    const CYRILLIC_SHORT: [&str; 18] = [
        "{0}/cm",
        "{0} днев.",
        "{0}/ft",
        "{0}/gal US",
        "{0}/g",
        "{0} по сату",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0} у мин.",
        "{0} мјесеч.",
        "{0}/oz",
        "{0}/lb",
        "{0} у сек.",
        "{0} седм.",
        "{0} годиш.",
    ];
    let patterns = match (bosnian_script(locale)?, display) {
        (BosnianScript::Latin, Display::Long) => &LATIN_LONG,
        (BosnianScript::Latin, Display::Short) => &LATIN_SHORT,
        (BosnianScript::Latin, Display::Narrow) => &LATIN_NARROW,
        (BosnianScript::Cyrillic, Display::Long) => &CYRILLIC_LONG,
        (BosnianScript::Cyrillic, Display::Short | Display::Narrow) => &CYRILLIC_SHORT,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Bosnian generic compounds when either operand has a raw record.
pub(crate) fn cldr_bosnian_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    bosnian_script(locale)?;
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator_raw = cldr_bosnian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_raw = cldr_bosnian_additional_unit_pattern(
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
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        "{0}/{1}",
    )
}
