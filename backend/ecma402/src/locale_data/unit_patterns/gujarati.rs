// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Gujarati raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_gujarati(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("gu")
}

/// Returns pinned Gujarati CLDR records for every ECMA-402 simple-unit
/// category that ICU4X's typed markers do not cover.
pub(crate) fn cldr_gujarati_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_gujarati(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} એકર",
            Unit::Bit => "{0} બિટ",
            Unit::Byte => "{0} બાઇટ",
            Unit::Celsius => "{0} ડિગ્રી સેલ્સિયસ",
            Unit::Degree => "{0} અંશ",
            Unit::Fahrenheit => "{0} ડિગ્રી ફેરનહીટ",
            Unit::Gigabit => "{0} ગીગાબિટ",
            Unit::Gigabyte => "{0} ગીગાબાઇટ",
            Unit::Kilobit => "{0} કિલોબિટ",
            Unit::Kilobyte => "{0} કિલોબાઇટ",
            Unit::Megabit => "{0} મેગાબિટ",
            Unit::Megabyte => "{0} મેગાબાઇટ",
            Unit::Percent => "{0} ટકા",
            Unit::Petabyte => "{0} પેટાબાઈટ્સ",
            Unit::Second => "{0} સેકંડ",
            Unit::Terabit => "{0} ટેરાબિટ",
            Unit::Terabyte => "{0} ટેરાબાઇટ",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} એકર",
            Unit::Bit => "{0} બિટ",
            Unit::Byte => "{0} બાઇટ",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} અંશ",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} સેકંડ",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} એકર",
            Unit::Bit => "{0} બિટ",
            Unit::Byte => "{0} બાઇટ",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} અંશ",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Second => "{0} સે",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Gujarati denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_gujarati_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_gujarati(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} પ્રતિ સેન્ટિમીટર",
        "{0} પ્રતિ દિવસ",
        "{0} પ્રતિ ફૂટ",
        "{0} પ્રતિ ગૅલન",
        "{0} પ્રતિ ગ્રામ",
        "{0} પ્રતિ કલાક",
        "{0} પ્રતિ ઈંચ",
        "{0} પ્રતિ કિલોગ્રામ",
        "{0} પ્રતિ કિલોમીટર",
        "{0} પ્રતિ લિટર",
        "{0} પ્રતિ મીટર",
        "{0} પ્રતિ મિનિટ",
        "{0} પ્રતિ મહિનો",
        "{0} પ્રતિ ઔંસ",
        "{0} પ્રતિ પાઉન્ડ",
        "{0} પ્રતિ સેકંડ",
        "{0} પ્રતિ અઠવાડિયું",
        "{0} પ્રતિ વર્ષ",
    ];
    const SHORT: [&str; 18] = [
        "{0}/સેમી",
        "{0}/ દિવસ",
        "{0}/ફૂટ",
        "{0}/ગૅલન",
        "{0}/ગ્રામ",
        "{0} પ્રતિ કલાક",
        "{0}/ઈંચ",
        "{0}/કિગ્રા",
        "{0}/કિમી",
        "{0}/લિ",
        "{0}/મી",
        "{0}/મિ.",
        "{0}/માસ",
        "{0}/ઔંસ",
        "{0}/પાઉન્ડ",
        "{0} પ્રતિ સેકંડ",
        "{0} / અઠ.",
        "{0}/વર્ષ",
    ];
    const NARROW: [&str; 18] = [
        "{0}/સેમી",
        "{0}/ દિ",
        "{0}/ફૂટ",
        "{0}/ગૅલન",
        "{0}/ગ્રામ",
        "{0}/ક",
        "{0}/ઈંચ",
        "{0}/કિગ્રા",
        "{0}/કિમી",
        "{0}/લિ",
        "{0}/મી",
        "{0}/મિ",
        "{0}/માસ",
        "{0}/ઔંસ",
        "{0}/પાઉન્ડ",
        "{0}/સે",
        "{0} / અઠ.",
        "{0}/વર્ષ",
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

/// Composes Gujarati generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_gujarati_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_gujarati(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_gujarati_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_gujarati_additional_unit_pattern(
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
            Display::Long => "{0} પ્રતિ {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
