// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Niger-Congo raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_swahili(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("sw")
}

/// Returns pinned Swahili CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
///
/// Swahili places the number after most unit labels; retain that order rather
/// than treating every localized unit as a suffix.
pub(crate) fn cldr_swahili_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_swahili(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "biti {0}",
            Unit::Byte => "baiti {0}",
            Unit::Celsius => "nyuzi {0}",
            Unit::Degree => "digrii {0}",
            Unit::Fahrenheit => "nyuzi za farenheiti {0}",
            Unit::Gigabit => "gigabiti {0}",
            Unit::Gigabyte => "gigabaiti {0}",
            Unit::Kilobit => "kilobiti {0}",
            Unit::Kilobyte => "kilobaiti {0}",
            Unit::Megabit => "megabiti {0}",
            Unit::Megabyte => "megabaiti {0}",
            Unit::Percent => "asilimia {0}",
            Unit::Petabyte => "petabaiti {0}",
            Unit::Terabit => "terabiti {0}",
            Unit::Terabyte => "terabaiti {0}",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "biti {0}",
            Unit::Byte => "baiti {0}",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "digrii {0}",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "gigabiti {0}",
            Unit::Gigabyte => "GB {0}",
            Unit::Kilobit => "kilobiti {0}",
            Unit::Kilobyte => "kilobaiti {0}",
            Unit::Megabit => "megabiti {0}",
            Unit::Megabyte => "MB {0}",
            Unit::Percent => "asilimia {0}",
            Unit::Petabyte => "PB {0}",
            Unit::Terabit => "terabiti {0}",
            Unit::Terabyte => "terabaiti {0}",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "biti {0}",
            Unit::Byte => "baiti {0}",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "gigabiti {0}",
            Unit::Gigabyte => "GB {0}",
            Unit::Kilobit => "kilobiti {0}",
            Unit::Kilobyte => "kilobaiti {0}",
            Unit::Megabit => "megabiti {0}",
            Unit::Megabyte => "MB {0}",
            Unit::Percent => "asilimia {0}",
            Unit::Petabyte => "PB {0}",
            Unit::Terabit => "terabiti {0}",
            Unit::Terabyte => "terabaiti {0}",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Swahili denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_swahili_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_swahili(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} kwa kila sentimita",
        "{0} kwa kila siku",
        "{0} kwa kila futi",
        "{0} kwa kila galoni",
        "{0} kwa kila gramu",
        "{0} kwa kila saa",
        "{0} kwa kila inchi",
        "{0} kwa kila kilogramu",
        "{0} kwa kila kilomita",
        "{0} kwa kila lita",
        "{0} kwa kila mita",
        "{0} kwa kila dakika",
        "{0} kwa kila mwezi",
        "{0} kwa kila aunsi",
        "{0} kwa kila ratili",
        "{0} kwa kila sekunde",
        "{0} kwa kila wiki",
        "{0} kwa mwaka",
    ];
    const SHORT: [&str; 18] = [
        "{0} kwa kila sentimita",
        "{0} kwa kila siku",
        "{0} kwa kila futi",
        "{0}/gal",
        "{0} kwa kila gramu",
        "{0} kwa kila saa",
        "{0} kwa kila inchi",
        "{0}/kg",
        "{0} kwa kila kilomita",
        "{0} kwa kila lita",
        "{0} kwa kila mita",
        "{0} kwa kila dakika",
        "{0} kwa kila mwezi",
        "{0}/oz",
        "{0}/lb",
        "{0} kwa kila sekunde",
        "{0} kwa kila wiki",
        "{0} kwa mwaka",
    ];
    const NARROW: [&str; 18] = [
        "{0} kwa kila sentimita",
        "{0} kwa kila siku",
        "{0} kwa kila futi",
        "{0}/gal",
        "{0} kwa kila gramu",
        "{0} kwa kila saa",
        "{0} kwa kila inchi",
        "{0}/kg",
        "{0} kwa kila kilomita",
        "{0} kwa kila lita",
        "{0} kwa kila mita",
        "{0} kwa kila dakika",
        "{0} kwa kila mwezi",
        "{0}/oz",
        "{0}/lb",
        "{0} kwa kila sek",
        "{0} kwa kila wiki",
        "{0} kwa mwaka",
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

/// Composes Swahili generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_swahili_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_swahili(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator_raw = cldr_swahili_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_raw = cldr_swahili_additional_unit_pattern(
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
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        "{0} kwa kila {1}",
    )
}
