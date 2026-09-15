// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! West Slavic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn slovak_cardinal_pattern(
    one: &'static str,
    few: &'static str,
    many: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Few => few,
        crate::PluralCategory::Many => many,
        crate::PluralCategory::Zero | crate::PluralCategory::Two | crate::PluralCategory::Other => {
            other
        }
    }
}

/// Returns pinned Slovak CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers. Slovak uses the same category set as
/// Czech, but its long labels and `few`/`many` inflections are distinct.
pub(crate) fn cldr_slovak_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("sk")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => {
                slovak_cardinal_pattern("{0} bit", "{0} bity", "{0} bitu", "{0} bitov", plural)
            }
            Unit::Byte => {
                slovak_cardinal_pattern("{0} bajt", "{0} bajty", "{0} bajtu", "{0} bajtov", plural)
            }
            Unit::Celsius => slovak_cardinal_pattern(
                "{0} stupeň Celzia",
                "{0} stupne Celzia",
                "{0} stupňa Celzia",
                "{0} stupňov Celzia",
                plural,
            ),
            Unit::Degree => slovak_cardinal_pattern(
                "{0} stupeň",
                "{0} stupne",
                "{0} stupňa",
                "{0} stupňov",
                plural,
            ),
            Unit::Fahrenheit => slovak_cardinal_pattern(
                "{0} stupeň Fahrenheita",
                "{0} stupne Fahrenheita",
                "{0} stupňa Fahrenheita",
                "{0} stupňov Fahrenheita",
                plural,
            ),
            Unit::Gigabit => slovak_cardinal_pattern(
                "{0} gigabit",
                "{0} gigabity",
                "{0} gigabitu",
                "{0} gigabitov",
                plural,
            ),
            Unit::Gigabyte => slovak_cardinal_pattern(
                "{0} gigabajt",
                "{0} gigabajty",
                "{0} gigabajtu",
                "{0} gigabajtov",
                plural,
            ),
            Unit::Kilobit => slovak_cardinal_pattern(
                "{0} kilobit",
                "{0} kilobity",
                "{0} kilobitu",
                "{0} kilobitov",
                plural,
            ),
            Unit::Kilobyte => slovak_cardinal_pattern(
                "{0} kilobajt",
                "{0} kilobajty",
                "{0} kilobajtu",
                "{0} kilobajtov",
                plural,
            ),
            Unit::Megabit => slovak_cardinal_pattern(
                "{0} megabit",
                "{0} megabity",
                "{0} megabitu",
                "{0} megabitov",
                plural,
            ),
            Unit::Megabyte => slovak_cardinal_pattern(
                "{0} megabajt",
                "{0} megabajty",
                "{0} megabajtu",
                "{0} megabajtov",
                plural,
            ),
            Unit::Percent => slovak_cardinal_pattern(
                "{0} percento",
                "{0} percentá",
                "{0} percenta",
                "{0} percent",
                plural,
            ),
            Unit::Petabyte => slovak_cardinal_pattern(
                "{0} petabajt",
                "{0} petabajty",
                "{0} petabajtu",
                "{0} petabajtov",
                plural,
            ),
            Unit::Terabit => slovak_cardinal_pattern(
                "{0} terabit",
                "{0} terabity",
                "{0} terabitu",
                "{0} terabitov",
                plural,
            ),
            Unit::Terabyte => slovak_cardinal_pattern(
                "{0} terabajt",
                "{0} terabajty",
                "{0} terabajtu",
                "{0} terabajtov",
                plural,
            ),
            _ => return None,
        },
        Display::Short | Display::Narrow => match unit {
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
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Composes Slovak generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_slovak_generic_compound_unit_pattern(
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
        != Some("sk")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_slovak_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_slovak_additional_unit_pattern(
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
    let denominator = denominator_raw.or_else(|| {
        experimental_number_unit_pattern(
            locale,
            denominator_unit,
            denominator_display,
            crate::PluralCategory::One,
        )
    })?;
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
