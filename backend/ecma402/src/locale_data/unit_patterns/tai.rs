// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tai raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

/// Returns pinned Thai CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers. Thai long percent uses a non-breaking
/// space, while narrow records join the numeric placeholder directly.
pub(crate) fn cldr_thai_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("th")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} บิต",
            Unit::Byte => "{0} ไบต์",
            Unit::Celsius => "{0} องศาเซลเซียส",
            Unit::Degree => "{0} องศา",
            Unit::Fahrenheit => "{0} องศาฟาเรนไฮต์",
            Unit::Gigabit => "{0} กิกะบิต",
            Unit::Gigabyte => "{0} กิกะไบต์",
            Unit::Kilobit => "{0} กิโลบิต",
            Unit::Kilobyte => "{0} กิโลไบต์",
            Unit::Megabit => "{0} เมกะบิต",
            Unit::Megabyte => "{0} เมกะไบต์",
            Unit::Percent => "{0}\u{a0}เปอร์เซ็นต์",
            Unit::Petabyte => "{0} เพตะไบต์",
            Unit::Terabit => "{0} เทราบิต",
            Unit::Terabyte => "{0} เทราไบต์",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} บิต",
            Unit::Byte => "{0} ไบต์",
            Unit::Celsius => "{0}°C",
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
        Display::Narrow => match unit {
            Unit::Bit => "{0}บิต",
            Unit::Byte => "{0}ไบต์",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}PB",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Thailand's denominator-specific pinned CLDR `perUnitPattern`
/// records, co-located with the Thai simple-unit records.
pub(crate) fn cldr_thai_per_unit_pattern(
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
        != Some("th")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} ต่อเซนติเมตร",
        "{0} ต่อวัน",
        "{0} ต่อฟุต",
        "{0} ต่อแกลลอน",
        "{0} ต่อกรัม",
        "{0} ต่อชั่วโมง",
        "{0} ต่อนิ้ว",
        "{0} ต่อกิโลกรัม",
        "{0} ต่อกิโลเมตร",
        "{0} ต่อลิตร",
        "{0} ต่อเมตร",
        "{0} ต่อนาที",
        "{0} ต่อเดือน",
        "{0} ต่อออนซ์",
        "{0} ต่อปอนด์",
        "{0} ต่อวินาที",
        "{0} ต่อสัปดาห์",
        "{0} ต่อปี",
    ];
    const SHORT: [&str; 18] = [
        "{0}/ซม.",
        "{0}/วัน",
        "{0}/ฟุต",
        "{0}/แกลลอน",
        "{0}/ก.",
        "{0}/ชม.",
        "{0}/นิ้ว",
        "{0}/กก.",
        "{0}/กม.",
        "{0}/ล.",
        "{0}/ม.",
        "{0}/นาที",
        "{0}/เดือน",
        "{0}/ออนซ์",
        "{0}/ปอนด์",
        "{0}/วิ",
        "{0}/สัปดาห์",
        "{0}/ปี",
    ];
    const NARROW: [&str; 18] = [
        "{0}/ซม.",
        "{0}/วัน",
        "{0}/ฟุต",
        "{0}/แกลลอน",
        "{0}/ก.",
        "{0}/ชม.",
        "{0}/นิ้ว",
        "{0}/กก.",
        "{0}/กม.",
        "{0}/ล.",
        "{0}/ม.",
        "{0}/นาที",
        "{0}/เดือน",
        "{0}/ออนซ์",
        "{0}/ปอนด์",
        "{0}/วิ",
        "{0}/สัปดาห์",
        "{0}/ปี",
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

/// Composes Thai generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_thai_generic_compound_unit_pattern(
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
        != Some("th")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_thai_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_thai_additional_unit_pattern(
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
    let denominator = denominator_raw
        .or_else(|| {
            experimental_number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        })
        // The pinned typed duration marker has no Thai records. Reuse the
        // provider's locale-aware duration path so a raw Thai numerator does
        // not force an otherwise local compound back to English.
        .unwrap_or_else(|| {
            crate::locale_data_provider().number_unit_pattern(
                locale,
                denominator_unit,
                denominator_display,
                crate::PluralCategory::One,
            )
        });
    let generic_per = match display {
        Display::Long => "{0}ต่อ{1}",
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
