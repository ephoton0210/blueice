// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Polynesian raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_tongan(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("to")
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

// Returns Tongan denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Tongan generic compounds containing an ICU4X-untyped unit.
