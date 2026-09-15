// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! South Slavic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_croatian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("hr")
}

#[derive(Clone, Copy)]
enum SerbianScript {
    Cyrillic,
    Latin,
}

fn serbian_script(locale: &str) -> Option<SerbianScript> {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut subtags = base.split('-');
    (subtags.next()? == "sr").then_some(())?;
    if subtags.any(|subtag| subtag.eq_ignore_ascii_case("Latn")) {
        Some(SerbianScript::Latin)
    } else {
        Some(SerbianScript::Cyrillic)
    }
}

fn untyped_simple_unit_index(unit: crate::NumberFormatUnit) -> Option<usize> {
    use crate::NumberFormatUnit as Unit;

    Some(match unit {
        Unit::Bit => 0,
        Unit::Byte => 1,
        Unit::Celsius => 2,
        Unit::Degree => 3,
        Unit::Fahrenheit => 4,
        Unit::Gigabit => 5,
        Unit::Gigabyte => 6,
        Unit::Kilobit => 7,
        Unit::Kilobyte => 8,
        Unit::Megabit => 9,
        Unit::Megabyte => 10,
        Unit::Percent => 11,
        Unit::Petabyte => 12,
        Unit::Terabit => 13,
        Unit::Terabyte => 14,
        _ => return None,
    })
}

/// Returns pinned Croatian CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_croatian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if !is_croatian(locale) {
        return None;
    }
    let plural_index = match plural {
        PluralCategory::One => 0,
        PluralCategory::Few => 1,
        PluralCategory::Zero
        | PluralCategory::Two
        | PluralCategory::Many
        | PluralCategory::Other => 2,
    };
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => ["{0} bit", "{0} bita", "{0} bitova"][plural_index],
            Unit::Byte => ["{0} bajt", "{0} bajta", "{0} bajtova"][plural_index],
            Unit::Celsius => [
                "{0} Celzijev stupanj",
                "{0} Celzijeva stupnja",
                "{0} Celzijevih stupnjeva",
            ][plural_index],
            Unit::Degree => ["{0} stupanj", "{0} stupnja", "{0} stupnjeva"][plural_index],
            Unit::Fahrenheit => [
                "{0} Fahrenheitov stupanj",
                "{0} Fahrenheitova stupnja",
                "{0} Fahrenheitovih stupnjeva",
            ][plural_index],
            Unit::Gigabit => ["{0} gigabit", "{0} gigabita", "{0} gigabita"][plural_index],
            Unit::Gigabyte => ["{0} gigabajt", "{0} gigabajta", "{0} gigabajta"][plural_index],
            Unit::Kilobit => ["{0} kilobit", "{0} kilobita", "{0} kilobita"][plural_index],
            Unit::Kilobyte => ["{0} kilobajt", "{0} kilobajta", "{0} kilobajta"][plural_index],
            Unit::Megabit => ["{0} megabit", "{0} megabita", "{0} megabita"][plural_index],
            Unit::Megabyte => ["{0} megabajt", "{0} megabajta", "{0} megabajta"][plural_index],
            Unit::Percent => ["{0} posto", "{0} posto", "{0} posto"][plural_index],
            Unit::Petabyte => ["{0} petabajt", "{0} petabajta", "{0} petabajta"][plural_index],
            Unit::Terabit => ["{0} terabit", "{0} terabita", "{0} terabita"][plural_index],
            Unit::Terabyte => ["{0} terabajt", "{0} terabajta", "{0} terabajta"][plural_index],
            _ => return None,
        },
        Display::Short | Display::Narrow => match unit {
            Unit::Bit => ["{0} bit", "{0} bita", "{0} bitova"][plural_index],
            Unit::Byte => ["{0} bajt", "{0} bajta", "{0} bajtova"][plural_index],
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
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Croatian denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_croatian_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_croatian(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0}/cm",
        "{0} dnevno",
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
        "{0} mjesečno",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0} tjedno",
        "{0} godišnje",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm", "{0}/d.", "{0}/ft", "{0}/gal", "{0}/g", "{0}/h", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/mj.", "{0}/oz", "{0}/lb", "{0}/s", "{0}/tj.", "{0}/g.",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm", "{0}/d.", "{0}/ft", "{0}/gal", "{0}/g", "{0}/h", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/mj.", "{0}/oz", "{0}/lb", "{0}/s", "{0}/tj.", "{0}/g.",
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

/// Composes Croatian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_croatian_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_croatian(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator_raw = cldr_croatian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_raw = cldr_croatian_additional_unit_pattern(
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

/// Returns pinned Serbian Cyrillic or Latin CLDR records for every ECMA-402
/// simple-unit category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_serbian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    let script = serbian_script(locale)?;
    let plural_index = match plural {
        PluralCategory::One => 0,
        PluralCategory::Few => 1,
        PluralCategory::Zero
        | PluralCategory::Two
        | PluralCategory::Many
        | PluralCategory::Other => 2,
    };
    const CYRILLIC_LONG: [[&str; 3]; 15] = [
        ["{0} бит", "{0} бита", "{0} битова"],
        ["{0} бајт", "{0} бајта", "{0} бајтова"],
        [
            "{0} степен Целзијуса",
            "{0} степена Целзијуса",
            "{0} степени Целзијуса",
        ],
        ["{0} степен", "{0} степена", "{0} степени"],
        [
            "{0} степен Фаренхајта",
            "{0} степена Фаренхајта",
            "{0} степени Фаренхајта",
        ],
        ["{0} гигабит", "{0} гигабита", "{0} гигабитова"],
        ["{0} гигабајт", "{0} гигабајта", "{0} гигабајтова"],
        ["{0} килобит", "{0} килобита", "{0} килобитова"],
        ["{0} килобајт", "{0} килобајта", "{0} килобајтова"],
        ["{0} мегабит", "{0} мегабита", "{0} мегабитова"],
        ["{0} мегабајт", "{0} мегабајта", "{0} мегабајтова"],
        ["{0} проценат", "{0} процената", "{0} процената"],
        ["{0} петабајт", "{0} петабајта", "{0} петабајтова"],
        ["{0} терабит", "{0} терабита", "{0} терабитова"],
        ["{0} терабајт", "{0} терабајта", "{0} терабајта"],
    ];
    const LATIN_LONG: [[&str; 3]; 15] = [
        ["{0} bit", "{0} bita", "{0} bitova"],
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
        ["{0} gigabit", "{0} gigabita", "{0} gigabitova"],
        ["{0} gigabajt", "{0} gigabajta", "{0} gigabajtova"],
        ["{0} kilobit", "{0} kilobita", "{0} kilobitova"],
        ["{0} kilobajt", "{0} kilobajta", "{0} kilobajtova"],
        ["{0} megabit", "{0} megabita", "{0} megabitova"],
        ["{0} megabajt", "{0} megabajta", "{0} megabajtova"],
        ["{0} procenat", "{0} procenata", "{0} procenata"],
        ["{0} petabajt", "{0} petabajta", "{0} petabajtova"],
        ["{0} terabit", "{0} terabita", "{0} terabitova"],
        ["{0} terabajt", "{0} terabajta", "{0} terabajta"],
    ];
    let raw = match display {
        Display::Long => match script {
            SerbianScript::Cyrillic => {
                CYRILLIC_LONG[untyped_simple_unit_index(unit)?][plural_index]
            }
            SerbianScript::Latin => LATIN_LONG[untyped_simple_unit_index(unit)?][plural_index],
        },
        Display::Short | Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
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

/// Returns Serbian Cyrillic or Latin denominator-specific pinned CLDR
/// `perUnitPattern` records.
pub(crate) fn cldr_serbian_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    let script = serbian_script(locale)?;
    const CYRILLIC_LONG: [&str; 18] = [
        "{0}/cm",
        "{0}/дневно",
        "{0}/ft",
        "{0} по галону",
        "{0} по граму",
        "{0}/сат",
        "{0}/in",
        "{0} по килограму",
        "{0}/km",
        "{0} по литри",
        "{0}/m",
        "{0} у минуту",
        "{0} месечно",
        "{0} по унци",
        "{0} по фунти",
        "{0}/у секунди",
        "{0} недељно",
        "{0} годишње",
    ];
    const CYRILLIC_SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/д",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/ч",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/мин",
        "{0}/м",
        "{0}/oz",
        "{0}/lb",
        "{0}/с",
        "{0}/н",
        "{0}/год",
    ];
    const LATIN_LONG: [&str; 18] = [
        "{0}/cm",
        "{0}/dnevno",
        "{0}/ft",
        "{0} po galonu",
        "{0} po gramu",
        "{0}/sat",
        "{0}/in",
        "{0} po kilogramu",
        "{0}/km",
        "{0} po litri",
        "{0}/m",
        "{0} u minutu",
        "{0} mesečno",
        "{0} po unci",
        "{0} po funti",
        "{0}/u sekundi",
        "{0} nedeljno",
        "{0} godišnje",
    ];
    const LATIN_SHORT: [&str; 18] = [
        "{0}/cm", "{0}/d", "{0}/ft", "{0}/gal", "{0}/g", "{0}/č", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/m", "{0}/oz", "{0}/lb", "{0}/s", "{0}/n", "{0}/god",
    ];
    let patterns = match (script, display) {
        (SerbianScript::Cyrillic, Display::Long) => &CYRILLIC_LONG,
        (SerbianScript::Cyrillic, Display::Short | Display::Narrow) => &CYRILLIC_SHORT,
        (SerbianScript::Latin, Display::Long) => &LATIN_LONG,
        (SerbianScript::Latin, Display::Short | Display::Narrow) => &LATIN_SHORT,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Serbian generic compounds when an operand has a raw CLDR record.
pub(crate) fn cldr_serbian_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    serbian_script(locale)?;
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator_raw = cldr_serbian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_raw = cldr_serbian_additional_unit_pattern(
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

/// Returns pinned Slovenian CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_slovenian_additional_unit_pattern(
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
        != Some("sl")
    {
        return None;
    }
    let plural_index = match plural {
        PluralCategory::One => 0,
        PluralCategory::Two => 1,
        PluralCategory::Few => 2,
        PluralCategory::Zero | PluralCategory::Many | PluralCategory::Other => 3,
    };
    const LONG: [[&str; 4]; 15] = [
        ["{0} bit", "{0} bita", "{0} biti", "{0} bitov"],
        ["{0} bajt", "{0} bajta", "{0} bajti", "{0} bajtov"],
        [
            "{0} stopinja Celzija",
            "{0} stopinji Celzija",
            "{0} stopinje Celzija",
            "{0} stopinj Celzija",
        ],
        [
            "{0} stopinja",
            "{0} stopinji",
            "{0} stopinje",
            "{0} stopinj",
        ],
        [
            "{0} stopinja Farenheita",
            "{0} stopinji Farenheita",
            "{0} stopinje Farenheita",
            "{0} stopinj Farenheita",
        ],
        [
            "{0} gigabit",
            "{0} gigabita",
            "{0} gigabiti",
            "{0} gigabitov",
        ],
        [
            "{0} gigabajt",
            "{0} gigabajta",
            "{0} gigabajti",
            "{0} gigabajtov",
        ],
        [
            "{0} kilobit",
            "{0} kilobita",
            "{0} kilobiti",
            "{0} kilobitov",
        ],
        [
            "{0} kilobajt",
            "{0} kilobajta",
            "{0} kilobajti",
            "{0} kilobajtov",
        ],
        [
            "{0} megabit",
            "{0} megabita",
            "{0} megabiti",
            "{0} megabitov",
        ],
        [
            "{0} megabajt",
            "{0} megabajta",
            "{0} megabajti",
            "{0} megabajtov",
        ],
        [
            "{0} odstotek",
            "{0} odstotka",
            "{0} odstotki",
            "{0} odstotkov",
        ],
        [
            "{0} petabajt",
            "{0} petabajta",
            "{0} petabajti",
            "{0} petabajtov",
        ],
        [
            "{0} terabit",
            "{0} terabita",
            "{0} terabiti",
            "{0} terabitov",
        ],
        [
            "{0} terabajt",
            "{0} terabajta",
            "{0} terabajti",
            "{0} terabajtov",
        ],
    ];
    let raw = match display {
        Display::Long => LONG[untyped_simple_unit_index(unit)?][plural_index],
        Display::Short => match unit {
            Unit::Bit => ["{0} bit", "{0} bita", "{0} biti", "{0} bitov"][plural_index],
            Unit::Byte => ["{0} bajt", "{0} bajta", "{0} bajti", "{0} bajtov"][plural_index],
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0} °",
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
            Unit::Bit => ["{0} bit", "{0} bita", "{0} biti", "{0} bitov"][plural_index],
            Unit::Byte => ["{0} bajt", "{0} bajta", "{0} bajti", "{0} B"][plural_index],
            Unit::Celsius => "{0} °",
            Unit::Degree => "{0} °",
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
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Slovenian denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_slovenian_per_unit_pattern(
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
        != Some("sl")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} na centimeter",
        "{0} na dan",
        "{0} na čevelj",
        "{0} na galono",
        "{0} na gram",
        "{0} na uro",
        "{0} na palec",
        "{0} na kilogram",
        "{0} na kilometer",
        "{0} na liter",
        "{0} na meter",
        "{0} na minuto",
        "{0} na mesec",
        "{0} na unčo",
        "{0} na funt",
        "{0} na sekundo",
        "{0} na teden",
        "{0} na leto",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0} na dan",
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
        "{0}/m",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/t",
        "{0}/l",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm", "{0}/dan.", "{0}/ft", "{0}/gal", "{0}/g", "{0}/h", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/m", "{0}/oz", "{0}/lb", "{0}/s", "{0}/t", "{0}/l",
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

/// Composes Slovenian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_slovenian_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    if base.split('-').next() != Some("sl") {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator_raw = cldr_slovenian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_raw = cldr_slovenian_additional_unit_pattern(
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
