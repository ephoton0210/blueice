// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Romance raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn romanian_cardinal_pattern(
    one: &'static str,
    few: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Few => few,
        crate::PluralCategory::Zero
        | crate::PluralCategory::Two
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => other,
    }
}

/// Returns pinned Romanian CLDR records for simple categories unavailable
/// through ICU4X typed unit markers. Romanian's `other` cardinal form carries
/// the required `de` preposition, so a simple one/other fallback is wrong.
pub(crate) fn cldr_romanian_additional_unit_pattern(
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
        != Some("ro")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => romanian_cardinal_pattern("{0} bit", "{0} biți", "{0} de biți", plural),
            Unit::Byte => romanian_cardinal_pattern("{0} byte", "{0} byți", "{0} de byți", plural),
            Unit::Celsius => romanian_cardinal_pattern(
                "{0} grad Celsius",
                "{0} grade Celsius",
                "{0} de grade Celsius",
                plural,
            ),
            Unit::Degree => {
                romanian_cardinal_pattern("{0} grad", "{0} grade", "{0} de grade", plural)
            }
            Unit::Fahrenheit => romanian_cardinal_pattern(
                "{0} grad Fahrenheit",
                "{0} grade Fahrenheit",
                "{0} de grade Fahrenheit",
                plural,
            ),
            Unit::Gigabit => {
                romanian_cardinal_pattern("{0} gigabit", "{0} gigabiți", "{0} de gigabiți", plural)
            }
            Unit::Gigabyte => {
                romanian_cardinal_pattern("{0} gigabyte", "{0} gigabyți", "{0} de gigabyți", plural)
            }
            Unit::Kilobit => {
                romanian_cardinal_pattern("{0} kilobit", "{0} kilobiți", "{0} de kilobiți", plural)
            }
            Unit::Kilobyte => {
                romanian_cardinal_pattern("{0} kilobyte", "{0} kilobyți", "{0} de kilobyți", plural)
            }
            Unit::Megabit => {
                romanian_cardinal_pattern("{0} megabit", "{0} megabiți", "{0} de megabiți", plural)
            }
            Unit::Megabyte => {
                romanian_cardinal_pattern("{0} megabyte", "{0} megabyți", "{0} de megabyți", plural)
            }
            Unit::Percent => {
                romanian_cardinal_pattern("{0} procent", "{0} procente", "{0} de procente", plural)
            }
            Unit::Petabyte => {
                romanian_cardinal_pattern("{0} petabyte", "{0} petabyți", "{0} de petabyți", plural)
            }
            Unit::Terabit => {
                romanian_cardinal_pattern("{0} terabit", "{0} terabiți", "{0} de terabiți", plural)
            }
            Unit::Terabyte => {
                romanian_cardinal_pattern("{0} terabyte", "{0} terabyți", "{0} de terabyți", plural)
            }
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
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Romania's denominator-specific pinned CLDR `perUnitPattern`
/// records. Keep these tables with the Romance simple-unit family so the
/// shared legacy table does not become the future growth point.
pub(crate) fn cldr_romanian_per_unit_pattern(
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
        != Some("ro")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} pe centimetru",
        "{0} pe zi",
        "{0} pe picior",
        "{0} per galon",
        "{0} per gram",
        "{0} pe oră",
        "{0} pe inch",
        "{0} per kilogram",
        "{0} pe kilometru",
        "{0} pe litru",
        "{0} pe metru",
        "{0} pe minut",
        "{0} pe lună",
        "{0} per uncie",
        "{0} per livră",
        "{0} pe secundă",
        "{0} pe săptămână",
        "{0} pe an",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/zi",
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
        "{0}/lună",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/săpt.",
        "{0}/an",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/zi",
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
        "{0}/lună",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/săpt.",
        "{0}/an",
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

/// Composes Romanian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_romanian_generic_compound_unit_pattern(
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
        != Some("ro")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_romanian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_romanian_additional_unit_pattern(
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
    let generic_per = match display {
        Display::Long => "{0} pe {1}",
        Display::Short | Display::Narrow => "{0}/{1}",
    };
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        generic_per,
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}
