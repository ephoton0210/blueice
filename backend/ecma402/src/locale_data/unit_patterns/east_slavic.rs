// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! East Slavic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_belarusian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("be")
}

fn belarusian_unit_index(unit: crate::NumberFormatUnit) -> Option<usize> {
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

fn belarusian_cardinal_index(plural: crate::PluralCategory) -> usize {
    match plural {
        crate::PluralCategory::One => 0,
        crate::PluralCategory::Few => 1,
        crate::PluralCategory::Zero | crate::PluralCategory::Many => 2,
        crate::PluralCategory::Two | crate::PluralCategory::Other => 3,
    }
}

fn belarusian_long_pattern(
    unit: crate::NumberFormatUnit,
    plural: crate::PluralCategory,
) -> Option<&'static str> {
    const LONG: [[&str; 4]; 17] = [
        ["{0} акр", "{0} акры", "{0} акраў", "{0} акра"],
        ["{0} біт", "{0} біты", "{0} біт", "{0} біта"],
        ["{0} байт", "{0} байты", "{0} байт", "{0} байта"],
        [
            "{0} градус Цэльсія",
            "{0} градусы Цэльсія",
            "{0} градусаў Цэльсія",
            "{0} градуса Цэльсія",
        ],
        ["{0} градус", "{0} градусы", "{0} градусаў", "{0} градуса"],
        [
            "{0} градус Фарэнгейта",
            "{0} градусы Фарэнгейта",
            "{0} градусаў Фарэнгейта",
            "{0} градуса Фарэнгейта",
        ],
        ["{0} гігабіт", "{0} гігабіты", "{0} гігабіт", "{0} гігабіта"],
        [
            "{0} гігабайт",
            "{0} гігабайты",
            "{0} гігабайт",
            "{0} гігабайта",
        ],
        ["{0} кілабіт", "{0} кілабіты", "{0} кілабіт", "{0} кілабіта"],
        [
            "{0} кілабайт",
            "{0} кілабайты",
            "{0} кілабайт",
            "{0} кілабайта",
        ],
        ["{0} мегабіт", "{0} мегабіты", "{0} мегабіт", "{0} мегабіта"],
        [
            "{0} мегабайт",
            "{0} мегабайты",
            "{0} мегабайт",
            "{0} мегабайта",
        ],
        [
            "{0} працэнт",
            "{0} працэнты",
            "{0} працэнтаў",
            "{0} працэнта",
        ],
        [
            "{0} петабайт",
            "{0} петабайты",
            "{0} петабайт",
            "{0} петабайта",
        ],
        ["{0} тэрабіт", "{0} тэрабіты", "{0} тэрабіт", "{0} тэрабіта"],
        [
            "{0} тэрабайт",
            "{0} тэрабайты",
            "{0} тэрабайт",
            "{0} тэрабайта",
        ],
        ["{0} секунда", "{0} секунды", "{0} секунд", "{0} секунды"],
    ];
    LONG.get(belarusian_unit_index(unit)?)
        .map(|patterns| patterns[belarusian_cardinal_index(plural)])
}

/// Returns pinned Belarusian CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_belarusian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_belarusian(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => belarusian_long_pattern(unit, plural)?,
        Display::Short => match unit {
            Unit::Acre | Unit::Bit | Unit::Byte => belarusian_long_pattern(unit, plural)?,
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Гбіт",
            Unit::Gigabyte => "{0} ГБ",
            Unit::Kilobit => "{0} кбіт",
            Unit::Kilobyte => "{0} КБ",
            Unit::Megabit => "{0} Мбіт",
            Unit::Megabyte => "{0} МБ",
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} ПБ",
            Unit::Terabit => "{0} Тбіт",
            Unit::Terabyte => "{0} ТБ",
            Unit::Second => "{0} с",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre | Unit::Bit | Unit::Byte => belarusian_long_pattern(unit, plural)?,
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Гбіт",
            Unit::Gigabyte => "{0} ГБ",
            Unit::Kilobit => "{0} кбіт",
            Unit::Kilobyte => "{0} КБ",
            Unit::Megabit => "{0} Мбіт",
            Unit::Megabyte => "{0} МБ",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} ПБ",
            Unit::Terabit => "{0} Тбіт",
            Unit::Terabyte => "{0} ТБ",
            Unit::Second => "{0} с",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Belarusian denominator-specific pinned CLDR `perUnitPattern`
// records.

// Composes Belarusian generic compounds containing an ICU4X-untyped unit.
