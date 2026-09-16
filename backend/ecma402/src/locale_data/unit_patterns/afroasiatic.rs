// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Afroasiatic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_amharic(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("am")
}

fn amharic_petabyte_pattern(plural: crate::PluralCategory) -> &'static str {
    match plural {
        crate::PluralCategory::One => "{0} ፔታ ባይት",
        crate::PluralCategory::Zero
        | crate::PluralCategory::Two
        | crate::PluralCategory::Few
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => "{0} ፔታ ባይቶች",
    }
}

/// Returns pinned Amharic CLDR records for simple categories that ICU4X's
/// typed unit markers do not provide with Amharic data.
pub(crate) fn cldr_amharic_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_amharic(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => "{0} ኤክር",
            Unit::Bit => "{0} ቢት",
            Unit::Byte => "{0} ባይት",
            Unit::Celsius => "{0} ዲግሪ ሴልሺየስ",
            Unit::Degree => "{0} ዲግሪ",
            Unit::Fahrenheit => "{0} ዲግሪ ፋራንሃይት",
            Unit::Gigabit => "{0} ጊጋባይት",
            Unit::Gigabyte => "{0} ጊባ",
            Unit::Kilobit => "{0} ኪሎባይት",
            Unit::Kilobyte => "{0} ኪባ",
            Unit::Megabit => "{0} ሜባ",
            Unit::Megabyte => "{0} ሜጋባይት",
            Unit::Percent => "{0} ፐርሰንት",
            Unit::Petabyte => amharic_petabyte_pattern(plural),
            Unit::Terabit => "{0} ቴባ",
            Unit::Terabyte => "{0} ቴራባይት",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ኤክር",
            Unit::Bit => "{0} ቢት",
            Unit::Byte => "{0} ባይት",
            Unit::Celsius => "{0}°ሴ",
            Unit::Degree => "{0}°ዲግሪ",
            Unit::Fahrenheit => "{0}°ፋ",
            Unit::Gigabit => "{0} ጊጋባይት",
            Unit::Gigabyte => "{0} ጊባ",
            Unit::Kilobit => "{0} ኪሎባይት",
            Unit::Kilobyte => "{0} ኪባ",
            Unit::Megabit => "{0} ሜባ",
            Unit::Megabyte => "{0} ሜጋባይት",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} ፔባ",
            Unit::Terabit => "{0} ቴባ",
            Unit::Terabyte => "{0} ቴራባይት",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} ኤክር",
            Unit::Bit => "{0} ቢት",
            Unit::Byte => "{0} ባይት",
            Unit::Celsius => "{0}°",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°ፋ",
            Unit::Gigabit => "{0} ጊጋባይት",
            Unit::Gigabyte => "{0} ጊባ",
            Unit::Kilobit => "{0} ኪሎባይት",
            Unit::Kilobyte => "{0} ኪባ",
            Unit::Megabit => "{0} ሜባ",
            Unit::Megabyte => "{0} ሜጋባይት",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} ፔባ",
            Unit::Terabit => "{0} ቴባ",
            Unit::Terabyte => "{0} ቴራባይት",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Amharic denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Amharic generic compounds containing an ICU4X-untyped unit.
