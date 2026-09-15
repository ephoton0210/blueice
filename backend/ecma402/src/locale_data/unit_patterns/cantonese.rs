// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cantonese raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_cantonese(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("yue")
}

/// Returns pinned Cantonese CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_cantonese_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_cantonese(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} 英畝",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "攝氏 {0} 度",
            Unit::Degree => "{0} 度",
            Unit::Fahrenheit => "華氏 {0} 度",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} 秒",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} 英畝",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} 度",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} 秒",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} 英畝",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} 度",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} 秒",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Cantonese denominator-specific pinned CLDR `perUnitPattern`
/// records.
pub(crate) fn cldr_cantonese_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_cantonese(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "每厘米 {0}",
        "每天 {0}",
        "每英呎 {0}",
        "每加侖 {0}",
        "每克 {0}",
        "每小時 {0}",
        "每英吋 {0}",
        "每公斤 {0}",
        "每公里 {0}",
        "每公升 {0}",
        "每米 {0}",
        "每分鐘 {0}",
        "每月 {0}",
        "每安士 {0}",
        "每磅 {0}",
        "每秒 {0}",
        "每週 {0}",
        "每年 {0}",
    ];
    const SHORT: [&str; 18] = [
        "每厘米{0}",
        "每天{0}",
        "每英呎{0}",
        "每加侖{0}",
        "每克{0}",
        "每小時{0}",
        "每英吋{0}",
        "每公斤{0}",
        "每公里{0}",
        "每公升{0}",
        "每米{0}",
        "每分鐘{0}",
        "每月{0}",
        "每安士{0}",
        "每磅{0}",
        "每秒{0}",
        "每週{0}",
        "每年{0}",
    ];
    const NARROW: [&str; 18] = [
        "每厘米{0}",
        "每天{0}",
        "每英呎{0}",
        "每加侖{0}",
        "每克 {0}",
        "每小時{0}",
        "每英吋 {0}",
        "每公斤 {0}",
        "每公里{0}",
        "每公升{0}",
        "每米{0}",
        "每分鐘{0}",
        "每月{0}",
        "每安士 {0}",
        "每磅 {0}",
        "每秒{0}",
        "每週{0}",
        "每年{0}",
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

/// Composes Cantonese generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_cantonese_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_cantonese(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_cantonese_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_cantonese_additional_unit_pattern(
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
            Display::Long => "每 {1} {0}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
