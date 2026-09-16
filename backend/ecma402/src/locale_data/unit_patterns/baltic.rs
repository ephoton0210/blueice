// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Baltic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_lithuanian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("lt")
}

fn is_latvian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("lv")
}

/// Returns pinned Lithuanian CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_lithuanian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if !is_lithuanian(locale) {
        return None;
    }
    let plural_index = match plural {
        PluralCategory::One => 0,
        PluralCategory::Few => 1,
        PluralCategory::Many => 2,
        PluralCategory::Zero | PluralCategory::Two | PluralCategory::Other => 3,
    };
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => ["{0} bitas", "{0} bitai", "{0} bito", "{0} bitų"][plural_index],
            Unit::Byte => ["{0} baitas", "{0} baitai", "{0} baito", "{0} baitų"][plural_index],
            Unit::Celsius => [
                "{0} Celsijaus laipsnis",
                "{0} Celsijaus laipsniai",
                "{0} Celsijaus laipsnio",
                "{0} Celsijaus laipsnių",
            ][plural_index],
            Unit::Degree => [
                "{0} laipsnis",
                "{0} laipsniai",
                "{0} laipsnio",
                "{0} laipsnių",
            ][plural_index],
            Unit::Fahrenheit => [
                "{0} Farenheito laipsnis",
                "{0} Farenheito laipsniai",
                "{0} Farenheito laipsnio",
                "{0} Farenheito laipsnių",
            ][plural_index],
            Unit::Gigabit => [
                "{0} gigabitas",
                "{0} gigabitai",
                "{0} gigabito",
                "{0} gigabitų",
            ][plural_index],
            Unit::Gigabyte => [
                "{0} gigabaitas",
                "{0} gigabaitai",
                "{0} gigabaito",
                "{0} gigabaitų",
            ][plural_index],
            Unit::Kilobit => [
                "{0} kilobitas",
                "{0} kilobitai",
                "{0} kilobito",
                "{0} kilobitų",
            ][plural_index],
            Unit::Kilobyte => [
                "{0} kilobaitas",
                "{0} kilobaitai",
                "{0} kilobaito",
                "{0} kilobaitų",
            ][plural_index],
            Unit::Megabit => [
                "{0} megabitas",
                "{0} megabitai",
                "{0} megabito",
                "{0} megabitų",
            ][plural_index],
            Unit::Megabyte => [
                "{0} megabaitas",
                "{0} megabaitai",
                "{0} megabaito",
                "{0} megabaitų",
            ][plural_index],
            Unit::Percent => [
                "{0} procentas",
                "{0} procentai",
                "{0} procento",
                "{0} procentas",
            ][plural_index],
            Unit::Petabyte => {
                ["{0} pentabaitas", "{0} PB", "{0} PB", "{0} pentabaitų"][plural_index]
            }
            Unit::Terabit => [
                "{0} terabitas",
                "{0} terabitai",
                "{0} terabito",
                "{0} terabitų",
            ][plural_index],
            Unit::Terabyte => [
                "{0} terabaitas",
                "{0} terabaitai",
                "{0} terabaito",
                "{0} terabaitų",
            ][plural_index],
            _ => return None,
        },
        Display::Short => match unit {
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
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
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

// Returns Lithuanian denominator-specific pinned CLDR `perUnitPattern`
// records.

// Composes Lithuanian generic compounds containing an ICU4X-untyped unit.

/// Returns pinned Latvian CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_latvian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if !is_latvian(locale) {
        return None;
    }
    let plural_index = match plural {
        PluralCategory::Zero => 0,
        PluralCategory::One => 1,
        PluralCategory::Two
        | PluralCategory::Few
        | PluralCategory::Many
        | PluralCategory::Other => 2,
    };
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => ["{0} bitu", "{0} bits", "{0} biti"][plural_index],
            Unit::Byte => ["{0} baitu", "{0} baits", "{0} baiti"][plural_index],
            Unit::Celsius => [
                "{0} Celsija grādu",
                "{0} Celsija grāds",
                "{0} Celsija grādi",
            ][plural_index],
            Unit::Degree => ["{0} grādu", "{0} grāds", "{0} grādi"][plural_index],
            Unit::Fahrenheit => [
                "{0} Fārenheita grādu",
                "{0} Fārenheita grāds",
                "{0} Fārenheita grādi",
            ][plural_index],
            Unit::Gigabit => ["{0} gigabitu", "{0} gigabits", "{0} gigabiti"][plural_index],
            Unit::Gigabyte => ["{0} gigabaitu", "{0} gigabaits", "{0} gigabaiti"][plural_index],
            Unit::Kilobit => ["{0} kilobitu", "{0} kilobits", "{0} kilobiti"][plural_index],
            Unit::Kilobyte => ["{0} kilobaitu", "{0} kilobaits", "{0} kilobaiti"][plural_index],
            Unit::Megabit => ["{0} megabitu", "{0} megabits", "{0} megabiti"][plural_index],
            Unit::Megabyte => ["{0} megabaitu", "{0} megabaits", "{0} megabaits"][plural_index],
            Unit::Percent => ["{0} procentu", "{0} procents", "{0} procenti"][plural_index],
            Unit::Petabyte => ["{0} petabaitu", "{0} petabaits", "{0} petabaiti"][plural_index],
            Unit::Terabit => ["{0} terabitu", "{0} terabits", "{0} terabiti"][plural_index],
            Unit::Terabyte => ["{0} terabaitu", "{0} terabaits", "{0} terabaiti"][plural_index],
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
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0} °C",
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

// Returns Latvian denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Latvian generic compounds containing an ICU4X-untyped unit.
