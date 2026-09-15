// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Sino-Tibetan raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_burmese(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("my")
}

/// Returns pinned Burmese CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_burmese_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_burmese(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} ဧက",
            Unit::Bit => "{0} ဘစ်",
            Unit::Byte => "{0} ဘိုက်",
            Unit::Celsius => "{0} ဒီဂရီ စင်တီဂရိတ်",
            Unit::Degree => "{0} ဒီဂရီ",
            Unit::Fahrenheit => "{0} ဒီဂရီ ဖာရင်ဟိုက်",
            Unit::Gigabit => "{0} ဂစ်ဂါဘစ်",
            Unit::Gigabyte => "{0} ဂစ်ဂါဘိုက်",
            Unit::Kilobit => "{0} ကီလိုဘစ်",
            Unit::Kilobyte => "{0} ကီလိုဘိုက်",
            Unit::Megabit => "{0} မီဂါဘစ်",
            Unit::Megabyte => "{0} မီဂါဘိုက်",
            Unit::Percent => "{0} ရာခိုင်နှုန်း",
            Unit::Petabyte => "{0} ပက်တာဘိုက်",
            Unit::Second => "{0} စက္ကန့်",
            Unit::Terabit => "{0} တယ်ရာဘစ်",
            Unit::Terabyte => "{0} တယ်ရာဘိုက်",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} deg",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} sec",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} ac",
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0}B",
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
            Unit::Second => "{0} s",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Burmese denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_burmese_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_burmese(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "တစ်စင်တီမီလာလျှင် {0}",
        "တစ်ရက်လျှင် {0}",
        "တစ်ပေလျှင် {0}",
        "တစ်ဂါလံလျှင် {0}",
        "တစ်ဂရမ်လျှင် {0}",
        "တစ်နာရီလျှင် {0}",
        "တစ်လက်မလျှင် {0}",
        "တစ်ကီလိုဂရမ်လျှင် {0}",
        "တစ်ကီလိုမီတာလျှင် {0}",
        "တစ်လီတာလျှင် {0}",
        "တစ်မီတာလျှင် {0}",
        "တစ်မိနစ်လျှင် {0}",
        "တစ်လလျှင် {0}",
        "တစ်အောင်စလျှင် {0}",
        "တစ်ပေါင်လျှင် {0}",
        "တစ်စက္ကန့်လျှင် {0}",
        "တစ်ပတ်လျှင် {0}",
        "တစ်နှစ်လျှင် {0}",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/ ရက်",
        "{0}/ft",
        "{0}/gal US",
        "{0}/g",
        "{0}/ နာရီ",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/ မိနစ်",
        "{0}/ လ",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/ ပတ်",
        "{0}/ နှစ်",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/ ရက်",
        "{0}/ft",
        "{0}/gal US",
        "{0}/g",
        "{0}/ နာရီ",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/လီတာ",
        "{0}/m",
        "{0}/ မိနစ်",
        "{0}/ လ",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/ ပတ်",
        "{0}/ နှစ်",
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

/// Composes Burmese generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_burmese_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_burmese(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_burmese_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_burmese_additional_unit_pattern(
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
            Display::Long => "တစ်{1} လျှင် {0}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
