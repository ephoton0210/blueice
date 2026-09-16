// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Semitic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn hebrew_cardinal_pattern(
    one: &'static str,
    two: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Two => two,
        crate::PluralCategory::Zero
        | crate::PluralCategory::Few
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => other,
    }
}

/// Returns pinned Hebrew CLDR records for the simple categories unavailable
/// through ICU4X typed markers, including no-number degree forms and the
/// bidirectional-control literals in abbreviated digital forms.
pub(crate) fn cldr_hebrew_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("he")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} ביט",
            Unit::Byte => "{0} בייט",
            Unit::Celsius => hebrew_cardinal_pattern(
                "{0} מעלת צלזיוס",
                "{0} מעלות צלזיוס",
                "{0} מעלות צלזיוס",
                plural,
            ),
            Unit::Degree => hebrew_cardinal_pattern("מעלה אחת", "שתי מעלות", "{0} מעלות", plural),
            Unit::Fahrenheit => hebrew_cardinal_pattern(
                "{0} מעלת פרנהייט",
                "{0} מעלות פרנהייט",
                "{0} מעלות פרנהייט",
                plural,
            ),
            Unit::Gigabit => "{0} ג׳יגה-ביט",
            Unit::Gigabyte => "{0} ג׳יגה-בייט",
            Unit::Kilobit => "{0} קילוביט",
            Unit::Kilobyte => "{0} קילו-בייט",
            Unit::Megabit => "{0} מגה-ביט",
            Unit::Megabyte => "{0} מגה-בייט",
            Unit::Percent => "{0} אחוז",
            Unit::Petabyte => "{0} פטה-בייט",
            Unit::Terabit => "{0} טרה-ביט",
            Unit::Terabyte => "{0} טרה-בייט",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} ביט",
            Unit::Byte => "{0} בייט",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => hebrew_cardinal_pattern("Gb\u{200f}{0}", "{0} Gb", "{0} Gb", plural),
            Unit::Gigabyte => hebrew_cardinal_pattern("GB\u{200f}{0}", "{0} GB", "{0} GB", plural),
            Unit::Kilobit => hebrew_cardinal_pattern("kb\u{200f}{0}", "{0} kb", "{0} kb", plural),
            Unit::Kilobyte => hebrew_cardinal_pattern("kB\u{200f}{0}", "{0} kB", "{0} kB", plural),
            Unit::Megabit => hebrew_cardinal_pattern("Mb\u{200f}{0}", "{0} Mb", "{0} Mb", plural),
            Unit::Megabyte => hebrew_cardinal_pattern("MB\u{200f}{0}", "{0} MB", "{0} MB", plural),
            Unit::Percent => "{0}%",
            Unit::Petabyte => "PB\u{200f}{0}",
            Unit::Terabit => "Tb\u{200f}{0}",
            Unit::Terabyte => hebrew_cardinal_pattern("TB\u{200f}{0}", "{0} TB", "{0} TB", plural),
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => hebrew_cardinal_pattern("bit\u{200f}{0}", "{0} ביט", "{0} ביט", plural),
            Unit::Byte => "B\u{200f}{0}",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => hebrew_cardinal_pattern("Gb\u{200f}{0}", "{0} Gb", "{0} Gb", plural),
            Unit::Gigabyte => hebrew_cardinal_pattern("GB\u{200f}{0}", "{0} GB", "{0} GB", plural),
            Unit::Kilobit => hebrew_cardinal_pattern("kb\u{200f}{0}", "{0} kb", "{0} kb", plural),
            Unit::Kilobyte => hebrew_cardinal_pattern("kB\u{200f}{0}", "{0} kB", "{0} kB", plural),
            Unit::Megabit => hebrew_cardinal_pattern("Mb\u{200f}{0}", "{0} Mb", "{0} Mb", plural),
            Unit::Megabyte => hebrew_cardinal_pattern("MB\u{200f}{0}", "{0} MB", "{0} MB", plural),
            Unit::Percent => "{0}%",
            Unit::Petabyte => "PB\u{200f}{0}",
            Unit::Terabit => "Tb\u{200f}{0}",
            Unit::Terabyte => "TB\u{200f}{0}",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Composes Hebrew generic compounds containing an ICU4X-untyped unit.
