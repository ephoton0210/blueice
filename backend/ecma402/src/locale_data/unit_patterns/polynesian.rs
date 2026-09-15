// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Polynesian raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    number_unit_pattern_label, NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_tongan(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("to")
}

fn tongan_unit_label(
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    Some(match display {
        Display::Long => match unit {
            Unit::Acre => "ʻeka",
            Unit::Bit => "piti",
            Unit::Byte => "paiti",
            Unit::Celsius => "tikili selisiasi",
            Unit::Degree => "tikili seakale",
            Unit::Fahrenheit => "tikili felenihaiti",
            Unit::Gigabit => "kikapiti",
            Unit::Gigabyte => "kikapaiti",
            Unit::Kilobit => "kilopiti",
            Unit::Kilobyte => "kilopaiti",
            Unit::Megabit => "mekapiti",
            Unit::Megabyte => "mekapaiti",
            Unit::Percent => "peseti",
            Unit::Petabyte => "petapaiti",
            Unit::Second => "sekoni",
            Unit::Terabit => "telapiti",
            Unit::Terabyte => "telapaiti",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "ʻek",
            Unit::Bit => "piti",
            Unit::Byte => "paiti",
            Unit::Celsius => "°S",
            Unit::Degree => "tsk",
            Unit::Fahrenheit => "°F",
            Unit::Gigabit => "kikapiti",
            Unit::Gigabyte => "kikapaiti",
            Unit::Kilobit => "kilopiti",
            Unit::Kilobyte => "kilopaiti",
            Unit::Megabit => "mekapiti",
            Unit::Megabyte => "mekapaiti",
            Unit::Percent => "%",
            Unit::Petabyte => "petapaiti",
            Unit::Second => "s",
            Unit::Terabit => "telapiti",
            Unit::Terabyte => "telapaiti",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "ʻek",
            Unit::Bit => "b",
            Unit::Byte => "B",
            Unit::Celsius => "°S",
            Unit::Degree => "°",
            Unit::Fahrenheit => "°F",
            Unit::Gigabit => "Gb",
            Unit::Gigabyte => "GB",
            Unit::Kilobit => "kb",
            Unit::Kilobyte => "kB",
            Unit::Megabit => "Mb",
            Unit::Megabyte => "MB",
            Unit::Percent => "%",
            Unit::Petabyte => "PB",
            Unit::Second => "s",
            Unit::Terabit => "Tb",
            Unit::Terabyte => "TB",
            _ => return None,
        },
    })
}

/// Returns pinned Tongan CLDR simple-unit records, including raw `second`
/// data absent from the pinned typed ICU4X marker inventory.
pub(crate) fn cldr_tongan_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_tongan(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "ʻeka ʻe {0}",
            Unit::Bit => "piti ʻe {0}",
            Unit::Byte => "paiti ʻe {0}",
            Unit::Celsius => "tikili selisiasi ʻe {0}",
            Unit::Degree => "tikili seakale ʻe {0}",
            Unit::Fahrenheit => "tikili felenihaiti ʻe {0}",
            Unit::Gigabit => "kikapiti ʻe {0}",
            Unit::Gigabyte => "kikapaiti ʻe {0}",
            Unit::Kilobit => "kilopiti ʻe {0}",
            Unit::Kilobyte => "kilopaiti ʻe {0}",
            Unit::Megabit => "mekapiti ʻe {0}",
            Unit::Megabyte => "mekapaiti ʻe {0}",
            Unit::Percent => "peseti ʻe {0}",
            Unit::Petabyte => "petapaiti ʻe {0}",
            Unit::Second => "sekoni ʻe {0}",
            Unit::Terabit => "telapiti ʻe {0}",
            Unit::Terabyte => "telapaiti ʻe {0}",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "ʻek ʻe {0}",
            Unit::Bit => "piti ʻe {0}",
            Unit::Byte => "paiti ʻe {0}",
            Unit::Celsius => "°S ʻe {0}",
            Unit::Degree => "tsk ʻe {0}",
            Unit::Fahrenheit => "°F ʻe {0}",
            Unit::Gigabit => "Gb ʻe {0}",
            Unit::Gigabyte => "GB ʻe {0}",
            Unit::Kilobit => "kb ʻe {0}",
            Unit::Kilobyte => "kB ʻe {0}",
            Unit::Megabit => "Mb ʻe {0}",
            Unit::Megabyte => "MB ʻe {0}",
            Unit::Percent => "% ʻe {0}",
            Unit::Petabyte => "PB ʻe {0}",
            Unit::Second => "s ʻe {0}",
            Unit::Terabit => "Tb ʻe {0}",
            Unit::Terabyte => "TB ʻe {0}",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} ʻek",
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°S",
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
            Unit::Second => "{0} s",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Tongan denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_tongan_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_tongan(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} he senitimita",
        "{0} he ʻaho",
        "{0} he fute",
        "{0} he kālani",
        "{0} he kalami",
        "{0} ki he houa",
        "{0} he ʻinisi",
        "{0} he kilokalami",
        "{0} he kilomita",
        "{0} he lita",
        "{0} he mita",
        "{0} he miniti",
        "{0} he māhina",
        "{0} he ʻaunise",
        "{0} he pāuni",
        "{0} ki he sekoni",
        "{0} he uike",
        "{0} he taʻu",
    ];
    const SHORT: [&str; 18] = [
        "{0} /sm", "{0} /ʻa", "{0}/ft", "{0}/kā", "{0}/k", "{0} /h", "{0}/in", "{0}/kk", "{0}/km",
        "{0}/l", "{0}/m", "{0} /m", "{0} /mā", "{0}/ʻau", "{0}/pāu", "{0} /s", "{0} /u", "{0} /t",
    ];
    const NARROW: [&str; 18] = [
        "{0}/sm", "{0}/ʻa", "{0}/ft", "{0}/kā", "{0}/k", "{0} /h", "{0}/in", "{0}/kk", "{0}/km",
        "{0}/l", "{0}/m", "{0}/m", "{0}/m", "{0}/ʻau", "{0}/pāu", "{0}/s", "{0}/u", "{0}/t",
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

/// Composes Tongan generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_tongan_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_tongan(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_tongan_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_tongan_additional_unit_pattern(
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
    let denominator_label = tongan_unit_label(denominator_unit, denominator_display)
        .map(str::to_owned)
        .unwrap_or_else(|| number_unit_pattern_label(&denominator));
    super::compose_generic_compound_unit_pattern_with_label(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator_label,
        match display {
            Display::Long => "{0} ʻi he {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
