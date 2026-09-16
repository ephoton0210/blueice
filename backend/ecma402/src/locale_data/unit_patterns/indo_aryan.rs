// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Indo-Aryan raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

fn is_urdu(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("ur")
}

fn is_latin_hindi(locale: &str) -> bool {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut subtags = base.split('-');
    subtags.next() == Some("hi") && subtags.any(|subtag| subtag.eq_ignore_ascii_case("Latn"))
}

fn urdu_cardinal_pattern(
    one: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Zero
        | crate::PluralCategory::Two
        | crate::PluralCategory::Few
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => other,
    }
}

/// Returns pinned Urdu CLDR records for simple categories that ICU4X's typed
/// unit markers do not provide with Urdu data.
///
/// The short and narrow temperature forms retain their CLDR left-to-right
/// mark, which is observable at the NumberFormat output boundary.
pub(crate) fn cldr_urdu_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_urdu(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => urdu_cardinal_pattern("{0} ایکڑ", "{0} ایکڑ", plural),
            Unit::Bit => urdu_cardinal_pattern("{0} بٹ", "{0} بٹس", plural),
            Unit::Byte => urdu_cardinal_pattern("{0} بائٹ", "{0} بائٹس", plural),
            Unit::Celsius => urdu_cardinal_pattern("{0} ڈگری سیلسیس", "{0} ڈگری سیلسیس", plural),
            Unit::Degree => urdu_cardinal_pattern("{0} ڈگری", "{0} ڈگری", plural),
            Unit::Fahrenheit => {
                urdu_cardinal_pattern("{0} ڈگری فارن ہائیٹ", "{0} ڈگری فارن ہائیٹ", plural)
            }
            Unit::Gigabit => urdu_cardinal_pattern("{0} گیگابٹ", "{0} گیگابٹس", plural),
            Unit::Gigabyte => urdu_cardinal_pattern("{0} گیگابائٹ", "{0} گیگابائٹ", plural),
            Unit::Kilobit => urdu_cardinal_pattern("{0} کلوبٹ", "{0} کلوبٹس", plural),
            Unit::Kilobyte => urdu_cardinal_pattern("{0} کلوبائٹ", "{0} کلوبائٹس", plural),
            Unit::Megabit => urdu_cardinal_pattern("{0} میگابٹ", "{0} میگابٹس", plural),
            Unit::Megabyte => urdu_cardinal_pattern("{0} میگابائٹ", "{0} ميگابائٹس", plural),
            Unit::Percent => urdu_cardinal_pattern("{0} فیصد", "{0} فیصد", plural),
            Unit::Petabyte => urdu_cardinal_pattern("{0} پیٹا بائٹ", "{0} پیٹا بائٹس", plural),
            Unit::Terabit => urdu_cardinal_pattern("{0} ٹیرابٹ", "{0} ٹیرابٹس", plural),
            Unit::Terabyte => urdu_cardinal_pattern("{0} ٹیرابائٹ", "{0} ٹیرابائٹ", plural),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => urdu_cardinal_pattern("{0} ایکڑ", "{0} ایکڑ", plural),
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0} byte",
            Unit::Celsius => "{0}\u{200e}°C",
            Unit::Degree => urdu_cardinal_pattern("{0} ڈگری", "{0} ڈگری", plural),
            Unit::Fahrenheit => "{0}\u{200e}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => urdu_cardinal_pattern("{0} پی بی", "{0} پی بی", plural),
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => urdu_cardinal_pattern("{0} ایکڑ", "{0} ایکڑ", plural),
            Unit::Bit => "{0} bit",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0}\u{200e}°",
            Unit::Degree => urdu_cardinal_pattern("{0} ڈگری", "{0} ڈگری", plural),
            Unit::Fahrenheit => "{0}\u{200e}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => urdu_cardinal_pattern("{0} پی بی", "{0} پی بی", plural),
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Urdu denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Urdu generic compounds containing an ICU4X-untyped unit.

fn latin_hindi_cardinal_pattern(
    one: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Zero
        | crate::PluralCategory::Two
        | crate::PluralCategory::Few
        | crate::PluralCategory::Many
        | crate::PluralCategory::Other => other,
    }
}

/// Returns pinned transliterated Hindi CLDR records. This is intentionally
/// distinct from Devanagari Hindi: CLDR publishes `hi-Latn` as a separate
/// Latin-script family rather than a script transform of `hi` patterns.
pub(crate) fn cldr_latin_hindi_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_latin_hindi(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => latin_hindi_cardinal_pattern("{0} acre", "{0} acres", plural),
            Unit::Bit => latin_hindi_cardinal_pattern("{0} bit", "{0} bits", plural),
            Unit::Byte => latin_hindi_cardinal_pattern("{0} byte", "{0} bytes", plural),
            Unit::Celsius => {
                latin_hindi_cardinal_pattern("{0} degree Celsius", "{0} degrees Celsius", plural)
            }
            Unit::Degree => latin_hindi_cardinal_pattern("{0} degree", "{0} degrees", plural),
            Unit::Fahrenheit => latin_hindi_cardinal_pattern(
                "{0} degree Fahrenheit",
                "{0} degrees Fahrenheit",
                plural,
            ),
            Unit::Gigabit => latin_hindi_cardinal_pattern("{0} gigabit", "{0} gigabits", plural),
            Unit::Gigabyte => latin_hindi_cardinal_pattern("{0} gigabyte", "{0} gigabytes", plural),
            Unit::Kilobit => latin_hindi_cardinal_pattern("{0} kilobit", "{0} kilobits", plural),
            Unit::Kilobyte => latin_hindi_cardinal_pattern("{0} kilobyte", "{0} kilobytes", plural),
            Unit::Megabit => latin_hindi_cardinal_pattern("{0} megabit", "{0} megabits", plural),
            Unit::Megabyte => latin_hindi_cardinal_pattern("{0} megabyte", "{0} megabytes", plural),
            Unit::Percent => latin_hindi_cardinal_pattern("{0} percent", "{0} percent", plural),
            Unit::Petabyte => latin_hindi_cardinal_pattern("{0} petabyte", "{0} petabytes", plural),
            Unit::Second => latin_hindi_cardinal_pattern("{0} second", "{0} seconds", plural),
            Unit::Terabit => latin_hindi_cardinal_pattern("{0} terabit", "{0} terabits", plural),
            Unit::Terabyte => latin_hindi_cardinal_pattern("{0} terabyte", "{0} terabytes", plural),
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
            Unit::Second => latin_hindi_cardinal_pattern("{0} sec", "{0} secs", plural),
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0}ac",
            Unit::Bit => "{0}bit",
            Unit::Byte => "{0}B",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}PB",
            Unit::Second => "{0}s",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns transliterated Hindi denominator-specific pinned CLDR
// `perUnitPattern` records.

// Composes transliterated Hindi generic compounds containing a raw CLDR unit.
