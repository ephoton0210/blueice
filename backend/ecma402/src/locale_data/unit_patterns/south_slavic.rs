// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! South Slavic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_croatian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("hr")
}

fn is_macedonian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("mk")
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

// Returns Croatian denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Croatian generic compounds containing an ICU4X-untyped unit.

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

// Returns Serbian Cyrillic or Latin denominator-specific pinned CLDR
// `perUnitPattern` records.

// Composes Serbian generic compounds when an operand has a raw CLDR record.

fn macedonian_cardinal_pattern(
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

/// Returns pinned Macedonian CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_macedonian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_macedonian(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => macedonian_cardinal_pattern("{0} акр", "{0} акри", plural),
            Unit::Bit => macedonian_cardinal_pattern("{0} бит", "{0} бита", plural),
            Unit::Byte => macedonian_cardinal_pattern("{0} бајт", "{0} бајти", plural),
            Unit::Celsius => macedonian_cardinal_pattern(
                "{0} целзиусов степен",
                "{0} целзиусови степени",
                plural,
            ),
            Unit::Degree => macedonian_cardinal_pattern("{0} степен", "{0} степени", plural),
            Unit::Fahrenheit => macedonian_cardinal_pattern(
                "{0} фаренхајтов степен",
                "{0} фаренхајтови степени",
                plural,
            ),
            Unit::Gigabit => macedonian_cardinal_pattern("{0} гигабит", "{0} гигабита", plural),
            Unit::Gigabyte => macedonian_cardinal_pattern("{0} гигабајт", "{0} гигабајти", plural),
            Unit::Kilobit => macedonian_cardinal_pattern("{0} килобит", "{0} килобита", plural),
            Unit::Kilobyte => macedonian_cardinal_pattern("{0} килобајт", "{0} килобајти", plural),
            Unit::Megabit => macedonian_cardinal_pattern("{0} мегабит", "{0} мегабита", plural),
            Unit::Megabyte => macedonian_cardinal_pattern("{0} мегабајт", "{0} мегабајти", plural),
            Unit::Percent => macedonian_cardinal_pattern("{0} процент", "{0} проценти", plural),
            Unit::Petabyte => macedonian_cardinal_pattern("{0} петабајт", "{0} петабајти", plural),
            Unit::Terabit => macedonian_cardinal_pattern("{0} терабит", "{0} терабита", plural),
            Unit::Terabyte => macedonian_cardinal_pattern("{0} терабајт", "{0} терабајти", plural),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => macedonian_cardinal_pattern("{0} бајт", "{0} бајти", plural),
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0} deg",
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
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0} deg",
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

// Returns Macedonian denominator-specific pinned CLDR `perUnitPattern`
// records.

// Composes Macedonian generic compounds containing an ICU4X-untyped unit.

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

// Returns Slovenian denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Slovenian generic compounds containing an ICU4X-untyped unit.
