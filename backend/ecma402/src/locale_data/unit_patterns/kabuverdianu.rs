// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Kabuverdianu raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_kabuverdianu(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("kea")
}

/// Returns pinned Kabuverdianu CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_kabuverdianu_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_kabuverdianu(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} ákri",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0} grau Celsius",
            Unit::Degree => "{0} grau",
            Unit::Fahrenheit => "{0} grau Fahrenheit",
            Unit::Gigabit => "{0} jigabit",
            Unit::Gigabyte => "{0} jigabyte",
            Unit::Kilobit => "{0} kilobit",
            Unit::Kilobyte => "{0} kilobyte",
            Unit::Megabit => "{0} megabit",
            Unit::Megabyte => "{0} megabyte",
            Unit::Percent => "{0} pursentu",
            Unit::Petabyte => "{0} petabyte",
            Unit::Second => "{0} sigundu",
            Unit::Terabit => "{0} terabit",
            Unit::Terabyte => "{0} terabyte",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
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
            Unit::Second => "{0} sig.",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} ac",
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
            Unit::Second => "{0} s",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Kabuverdianu denominator-specific pinned CLDR `perUnitPattern`
/// records.
pub(crate) fn cldr_kabuverdianu_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_kabuverdianu(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} pur sentímetru",
        "{0} pur dia",
        "{0} pur pe",
        "{0} pur galãu",
        "{0} pur grama",
        "{0} pur ora",
        "{0} pur pulegada",
        "{0} pur kilograma",
        "{0} pur kilómetru",
        "{0} pur litru",
        "{0} pur metru",
        "{0} pur minutu",
        "{0} pur mes",
        "{0} pur ónsa",
        "{0} pur libra",
        "{0} pur sigundu",
        "{0} pur simana",
        "{0} pur anu",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm", "{0}/dia", "{0}/pe", "{0}/gal", "{0}/g", "{0}/h", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min.", "{0}/mes", "{0}/oz", "{0}/lb", "{0}/s", "{0}/sim.",
        "{0}/anu",
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

/// Composes Kabuverdianu generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_kabuverdianu_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_kabuverdianu(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator =
        cldr_kabuverdianu_additional_unit_pattern(locale, numerator, display, plural)
            .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_kabuverdianu_additional_unit_pattern(
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
            Display::Long => "{0} pur {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
