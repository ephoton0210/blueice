// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Armenian raw CLDR unit-pattern family.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_armenian(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("hy")
}

/// Returns pinned Armenian records for the simple categories without an
/// ICU4X typed unit marker.
pub(crate) fn cldr_armenian_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_armenian(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} բիթ",
            Unit::Byte => "{0} բայթ",
            Unit::Celsius => "{0} աստիճան ըստ Ցելսիուսի",
            Unit::Degree => "{0} աստիճան",
            Unit::Fahrenheit => "{0} աստիճան ըստ Ֆարենհայթի",
            Unit::Gigabit => "{0} գիգաբիթ",
            Unit::Gigabyte => "{0} գիգաբայթ",
            Unit::Kilobit => "{0} կիլոբիթ",
            Unit::Kilobyte => "{0} կիլոբայթ",
            Unit::Megabit => "{0} մեգաբիթ",
            Unit::Megabyte => "{0} մեգաբայթ",
            Unit::Percent => "{0} տոկոս",
            Unit::Petabyte => "{0} պետաբայթ",
            Unit::Terabit => "{0} տերաբիթ",
            Unit::Terabyte => "{0} տերաբայթ",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} բիթ",
            Unit::Byte => "{0} Բ",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Գբիթ",
            Unit::Gigabyte => "{0} ԳԲ",
            Unit::Kilobit => "{0} կբիթ",
            Unit::Kilobyte => "{0} կԲ",
            Unit::Megabit => "{0} Մբիթ",
            Unit::Megabyte => "{0} ՄԲ",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} ՊԲ",
            Unit::Terabit => "{0} Տբիթ",
            Unit::Terabyte => "{0} ՏԲ",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}բիթ",
            Unit::Byte => "{0}Բ",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0}Գբիթ",
            Unit::Gigabyte => "{0}ԳԲ",
            Unit::Kilobit => "{0}կբիթ",
            Unit::Kilobyte => "{0}կԲ",
            Unit::Megabit => "{0}Մբիթ",
            Unit::Megabyte => "{0}ՄԲ",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}ՊԲ",
            Unit::Terabit => "{0}Տբիթ",
            Unit::Terabyte => "{0}ՏԲ",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Armenian denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_armenian_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_armenian(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} սանտիմետրի վրա",
        "օրական {0}",
        "{0} ֆուտի վրա",
        "{0} գալոնի վրա",
        "{0} գրամի վրա",
        "{0} ժամում",
        "{0} մատնաչափի վրա",
        "{0} կիլոգրամի վրա",
        "{0} կիլոմետրի վրա",
        "{0} լիտրի վրա",
        "{0} մետրի վրա",
        "{0} րոպեում",
        "ամսական {0}",
        "{0} ունկիի վրա",
        "{0} ֆունտի վրա",
        "{0} վայրկյանում",
        "շաբաթական {0}",
        "տարեկան {0}",
    ];
    const SHORT: [&str; 18] = [
        "{0}/սմ",
        "{0}/օր",
        "{0}/ֆտ",
        "{0}/գալ",
        "{0}/գ",
        "{0}/ժ",
        "{0}/մատ",
        "{0}/կգ",
        "{0}/կմ",
        "{0}/լ",
        "{0}/մ",
        "{0}/ր",
        "{0}/ամս",
        "{0}/ու",
        "{0}/ֆունտ",
        "{0}/վրկ",
        "{0}/շաբ",
        "{0}/տ",
    ];
    const NARROW: [&str; 18] = [
        "{0}/սմ",
        "{0}/օ",
        "{0}/ֆտ",
        "{0}/գալ",
        "{0}/գ",
        "{0}/ժ",
        "{0}/մատ",
        "{0}/կգ",
        "{0}/կմ",
        "{0}/լ",
        "{0}/մ",
        "{0}/ր",
        "{0}/ա",
        "{0}/ու",
        "{0}/ֆունտ",
        "{0}/վ",
        "{0}/շ",
        "{0}/տ",
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

/// Composes Armenian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_armenian_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_armenian(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator_raw = cldr_armenian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_raw = cldr_armenian_additional_unit_pattern(
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
        "{0}/{1}",
    )
}
