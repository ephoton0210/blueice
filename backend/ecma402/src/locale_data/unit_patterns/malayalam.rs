// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Malayalam raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_malayalam(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("ml")
}

/// Returns pinned Malayalam CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_malayalam_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_malayalam(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} ഏക്കർ",
            Unit::Bit => "{0} ബിറ്റ്",
            Unit::Byte => "{0} ബൈറ്റ്",
            Unit::Celsius => "{0} ഡിഗ്രി സെൽഷ്യസ്",
            Unit::Degree => "{0} ഡിഗ്രി",
            Unit::Fahrenheit => "{0} ഡിഗ്രി ഫാരൻഹീറ്റ്",
            Unit::Gigabit => "{0} ജിഗാബിറ്റ്",
            Unit::Gigabyte => "{0} ഗിഗാബൈറ്റ്",
            Unit::Kilobit => "{0} കിലോബിറ്റ്",
            Unit::Kilobyte => "{0} കിലോബൈറ്റ്",
            Unit::Megabit => "{0} മെഗാബിറ്റ്",
            Unit::Megabyte => "{0} മെഗാബൈറ്റ്",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} പെറ്റാബൈറ്റ്",
            Unit::Second => "{0} സെക്കൻഡ്",
            Unit::Terabit => "{0} ടെറാബിറ്റ്",
            Unit::Terabyte => "{0} ടെറാബൈറ്റ്",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ഏക്ക.",
            Unit::Bit => "{0} ബിറ്റ്",
            Unit::Byte => "{0} ബൈറ്റ്",
            Unit::Celsius => "{0}°സെ",
            Unit::Degree => "{0} ഡിഗ്രി",
            Unit::Fahrenheit => "{0}°ഫാ",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} സെ.",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} ഏക്ക",
            Unit::Bit => "{0} ബിറ്റ്",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0}°സെ",
            Unit::Degree => "{0} ഡിഗ്രി",
            Unit::Fahrenheit => "{0}°ഫാ",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}PB",
            Unit::Second => "{0} സെ.",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Malayalam denominator-specific pinned CLDR `perUnitPattern`
/// records.
pub(crate) fn cldr_malayalam_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_malayalam(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} / സെന്റിമീറ്റർ",
        "{0} / ദിവസം",
        "{0} / അടി",
        "{0} / ഗാലൺ",
        "{0}/ഗ്രാം",
        "{0} / മണിക്കൂർ",
        "{0} / ഇഞ്ച്",
        "{0}/കിലോഗ്രാം",
        "{0} / കിലോമീറ്റർ",
        "{0} / ലിറ്റർ",
        "{0} / മീറ്റർ",
        "{0} / മിനിറ്റ്",
        "{0} / മാസം",
        "{0}/ഔൺസ്",
        "{0}/പൗണ്ട്",
        "{0} / സെക്കൻഡ്",
        "{0} / ആഴ്ച",
        "{0} / വർഷം",
    ];
    const SHORT: [&str; 18] = [
        "{0}/സെ.മീ.",
        "{0}/ദി.",
        "{0}/അടി",
        "{0}/ഗാ.",
        "{0}/ഗ്രാം",
        "{0}/മ.",
        "{0}/ഇഞ്ച്",
        "{0}/കി.ഗ്രാം",
        "{0}/കി.മീ.",
        "{0}/ലി.",
        "{0}/മീ.",
        "{0}/മി.",
        "{0}/മാ.",
        "{0}/ഔ.",
        "{0}/പൗ.",
        "{0}/സെ.",
        "{0}/ആ.",
        "{0}/വ.",
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

/// Composes Malayalam generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_malayalam_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_malayalam(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_malayalam_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_malayalam_additional_unit_pattern(
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
        "{0}/{1}",
    )
}
