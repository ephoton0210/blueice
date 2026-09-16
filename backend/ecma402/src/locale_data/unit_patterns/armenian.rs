// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Armenian raw CLDR unit-pattern family.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

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

// Returns Armenian denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Armenian generic compounds containing an ICU4X-untyped unit.
