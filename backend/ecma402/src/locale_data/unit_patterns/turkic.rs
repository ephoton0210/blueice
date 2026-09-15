// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Turkic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

/// Returns pinned Turkish CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover. In particular, percent
/// is a leading word or sign (`yüzde 2`, `%2`), unlike the English fallback.
pub(crate) fn cldr_turkish_additional_unit_pattern(
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
        != Some("tr")
    {
        return None;
    }
    let singular = plural == PluralCategory::One;
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bayt",
            Unit::Celsius => "{0} santigrat derece",
            Unit::Degree => "{0} derece",
            Unit::Fahrenheit => "{0} fahrenhayt derece",
            Unit::Gigabit => "{0} gigabit",
            Unit::Gigabyte => "{0} gigabayt",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobayt",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabayt",
            Unit::Percent => "yüzde {0}",
            Unit::Petabyte => "{0} petabayt",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabayt",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bayt",
            Unit::Celsius => {
                if singular {
                    "{0} °C"
                } else {
                    "{0}°C"
                }
            }
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => {
                if singular {
                    "{0} °F"
                } else {
                    "{0}°F"
                }
            }
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "%{0}",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} bayt",
            Unit::Celsius => {
                if singular {
                    "{0}°C"
                } else {
                    "{0} °C"
                }
            }
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "%{0}",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Composes Turkish generic compounds when an ICU4X-untyped unit category is
/// present. Pinned Turkish CLDR `perUnitPattern` records use a slash without
/// whitespace, including long width forms such as `gigabayt/saniye`.
pub(crate) fn cldr_turkish_generic_compound_unit_pattern(
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
        != Some("tr")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_turkish_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_turkish_additional_unit_pattern(
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
