// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Caucasian raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Returns pinned Georgian CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_georgian_additional_unit_pattern(
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
        != Some("ka")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} ბიტი",
            Unit::Byte => "{0} ბაიტი",
            Unit::Celsius => "{0} გრადუსი ცელსიუსით",
            Unit::Degree => "{0} გრადუსი",
            Unit::Fahrenheit => "{0} გრადუსი ფარენჰეიტით",
            Unit::Gigabit => "{0} გიგაბიტი",
            Unit::Gigabyte => "{0} გიგაბაიტი",
            Unit::Kilobit => "{0} კილობიტი",
            Unit::Kilobyte => "{0} კილობაიტი",
            Unit::Megabit => "{0} მეგაბიტი",
            Unit::Megabyte => "{0} მეგაბაიტი",
            Unit::Percent => "{0} პროცენტი",
            Unit::Petabyte => "{0} პეტაბაიტი",
            Unit::Terabit => "{0} ტერაბიტი",
            Unit::Terabyte => "{0} ტერაბაიტი",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} ბიტი",
            Unit::Byte => "{0} ბაიტი",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} გბიტი",
            Unit::Gigabyte => "{0} გიგაბაიტი",
            Unit::Kilobit => "{0} კბიტი",
            Unit::Kilobyte => "{0} კბაიტი",
            Unit::Megabit => "{0} მბიტი",
            Unit::Megabyte => "{0} მეგაბაიტი",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} პბაიტი",
            Unit::Terabit => "{0} ტბიტი",
            Unit::Terabyte => "{0} ტბაიტი",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} ბიტი",
            Unit::Byte => "{0} ბაიტი",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} გბიტი",
            Unit::Gigabyte => "{0} გბაიტი",
            Unit::Kilobit => "{0} კბიტი",
            Unit::Kilobyte => "{0} კბაიტი",
            Unit::Megabit => "{0} მბიტი",
            Unit::Megabyte => "{0} მეგაბაიტი",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} პბაიტი",
            Unit::Terabit => "{0} ტბიტი",
            Unit::Terabyte => "{0} ტბაიტი",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Georgian denominator-specific pinned CLDR `perUnitPattern`
// records.

// Composes Georgian generic compounds containing an ICU4X-untyped unit.
