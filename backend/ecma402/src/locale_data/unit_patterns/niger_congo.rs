// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Niger-Congo raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

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

// Returns Swahili denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Swahili generic compounds containing an ICU4X-untyped unit.
