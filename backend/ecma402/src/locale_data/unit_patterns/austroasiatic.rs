// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Austroasiatic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_khmer(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("km")
}

/// Returns pinned Khmer CLDR records for simple categories that ICU4X's typed
/// unit markers do not provide with Khmer data.
pub(crate) fn cldr_khmer_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_khmer(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} អា",
            Unit::Bit => "{0}\u{a0}ប៊ីត",
            Unit::Byte => "{0} បៃ",
            Unit::Celsius => "{0} អង្សាសេ",
            Unit::Degree => "{0} ដឺក្រេ",
            Unit::Fahrenheit => "{0}\u{a0}អង្សា\u{200b}ហ្វារិនហៃ",
            Unit::Gigabit => "{0}\u{a0}ជីកាប៊ីត",
            Unit::Gigabyte => "{0} ជីកាបៃ",
            Unit::Kilobit => "{0} គីឡូប៊ីត",
            Unit::Kilobyte => "{0}\u{a0}គីឡូបៃ",
            Unit::Megabit => "{0} មេកាប៊ីត",
            Unit::Megabyte => "{0}\u{a0}មេកាបៃ",
            Unit::Percent => "{0} ភាគរយ",
            Unit::Petabyte => "{0} ប៉េតាបៃ",
            Unit::Terabit => "{0} តេរ៉ាប៊ីត",
            Unit::Terabyte => "{0} តេរ៉ាបៃ",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
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
            Unit::Acre => "{0} អា",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
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
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Khmer denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_khmer_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_khmer(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} ក្នុងមួយសង់ទីម៉ែត្រ",
        "{0} ក្នុងមួយថ្ងៃ",
        "{0} ក្នុងមួយហ្វីត",
        "{0} ក្នុងមួយហ្គាឡុង",
        "{0} ក្នុងមួយក្រាម",
        "{0} ក្នុង\u{200b}មួយ\u{200b}ម៉ោង",
        "{0} ក្នុងមួយអ៊ីញ",
        "{0} ក្នុងមួយគីឡូក្រាម",
        "{0} ក្នុងមួយគីឡូម៉ែត្រ",
        "{0} ក្នុងមួយលីត្រ",
        "{0} ក្នុងមួយម៉ែត្រ",
        "{0} ក្នុងមួយនាទី",
        "{0} ក្នុងមួយខែ",
        "{0} ក្នុងមួយអោន",
        "{0} ក្នុងមួយផោន",
        "{0} ក្នុង\u{200b}មួយ\u{200b}វិនាទី",
        "{0} ក្នុងមួយសប្តាហ៍",
        "{0} ក្នុងមួយឆ្នាំ",
    ];
    const SHORT: [&str; 18] = [
        "{0}/សម",
        "{0}/ថ្ងៃ",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/ម៉ោង",
        "{0}/in",
        "{0}/kg",
        "{0}/គម",
        "{0}/l",
        "{0}/ម",
        "{0}/នាទី",
        "{0}/ខែ",
        "{0}/oz",
        "{0}/lb",
        "{0}/វិនាទី",
        "{0}/សប្តាហ៍",
        "{0}/ឆ្នាំ",
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

/// Composes Khmer generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_khmer_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_khmer(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_khmer_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_khmer_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    )
    .or_else(|| {
        experimental_number_unit_pattern(
            locale,
            denominator_unit,
            denominator_display,
            crate::PluralCategory::One,
        )
    })
    .unwrap_or_else(|| {
        crate::locale_data_provider().number_unit_pattern(
            locale,
            denominator_unit,
            denominator_display,
            crate::PluralCategory::One,
        )
    });
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        match display {
            Display::Long => "{0}\u{200b} ក្នុង\u{200b}មួយ\u{200b} {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
