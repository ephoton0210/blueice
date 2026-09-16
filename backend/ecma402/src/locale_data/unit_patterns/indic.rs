// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Devanagari Indic raw CLDR unit-pattern families.

use super::super::{number_unit_pattern_from_placeholder, NumberUnitPattern};

/// Whether the request selects Devanagari Hindi rather than CLDR's distinct
/// transliterated `hi-Latn` family.
pub(crate) fn uses_hindi_devanagari_unit_data(locale: &str) -> bool {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut subtags = base.split('-');
    subtags.next() == Some("hi") && !subtags.any(|subtag| subtag.eq_ignore_ascii_case("Latn"))
}

/// Returns pinned Devanagari Hindi CLDR records for the untyped ICU4X unit
/// categories.
pub(crate) fn cldr_hindi_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    _plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !uses_hindi_devanagari_unit_data(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} बिट",
            Unit::Byte => "{0} बाइट",
            Unit::Celsius => "{0} डिग्री सेल्सियस",
            Unit::Degree => "{0} अंश",
            Unit::Fahrenheit => "{0} डिग्री फ़ेरनहाइट",
            Unit::Gigabit => "{0} गीगाबिट",
            Unit::Gigabyte => "{0} गीगाबाइट",
            Unit::Kilobit => "{0} किलोबिट",
            Unit::Kilobyte => "{0} किलोबाइट",
            Unit::Megabit => "{0} मेगाबिट",
            Unit::Megabyte => "{0} मेगाबाइट",
            Unit::Percent => "{0} प्रतिशत",
            Unit::Petabyte => "{0} पेटाबाइट",
            Unit::Terabit => "{0} टेराबिट",
            Unit::Terabyte => "{0} टेराबाइट",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} बिट",
            Unit::Byte => "{0} बाइट",
            Unit::Celsius => "{0}°से॰",
            Unit::Degree => "{0} अंश",
            Unit::Fahrenheit => "{0}°फ़ेरन",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} बिट",
            Unit::Byte => "{0} बाइट",
            Unit::Celsius => "{0}°से॰",
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
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Composes Devanagari Hindi generic compounds when either operand belongs to
// an ICU4X-untyped category.

/// Returns pinned Bengali CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_bengali_additional_unit_pattern(
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
        != Some("bn")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} বিট",
            Unit::Byte => "{0} বাইট",
            Unit::Celsius => "{0} ডিগ্রী সেলসিয়াস",
            Unit::Degree => "{0} ডিগ্রী",
            Unit::Fahrenheit => "{0} ডিগ্রী ফারেনহাইট",
            Unit::Gigabit => "{0} গিগাবিট",
            Unit::Gigabyte => "{0} গিগাবাইট",
            Unit::Kilobit => "{0} কিলোবিট",
            Unit::Kilobyte => "{0} কিলোবাইট",
            Unit::Megabit => "{0} মেগাবিট",
            Unit::Megabyte => "{0} মেগাবাইট",
            Unit::Percent => "{0}শতাংশ",
            Unit::Petabyte => "{0} পেটাবাইটস",
            Unit::Terabit => "{0} টেরাবিট",
            Unit::Terabyte => "{0} টেরাবাইট",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} বিট",
            Unit::Byte => "{0} বাইট",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}ডিগ্রী",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} বিট",
            Unit::Byte => "{0} বাইট",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}ডিগ্রী",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Bengali denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Bengali generic compounds containing an ICU4X-untyped unit.

/// Returns pinned Tamil CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_tamil_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display, PluralCategory};

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ta")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match (unit, plural) {
            (Unit::Bit, PluralCategory::One) => "{0} பிட்",
            (Unit::Bit, _) => "{0} பிட்கள்",
            (Unit::Byte, PluralCategory::One) => "{0} பைட்",
            (Unit::Byte, _) => "{0} பைட்கள்",
            (Unit::Celsius, _) => "{0} டிகிரி செல்சியஸ்",
            (Unit::Degree, PluralCategory::One) => "{0} டிகிரி",
            (Unit::Degree, _) => "{0} டிகிரீஸ்",
            (Unit::Fahrenheit, _) => "{0} டிகிரி ஃபாரன்ஹீட்",
            (Unit::Gigabit, PluralCategory::One) => "{0} கிகாபிட்",
            (Unit::Gigabit, _) => "{0} கிகாபிட்கள்",
            (Unit::Gigabyte, PluralCategory::One) => "{0} கிகாபைட்",
            (Unit::Gigabyte, _) => "{0} கிகாபைட்கள்",
            (Unit::Kilobit, PluralCategory::One) => "{0} கிலோபிட்",
            (Unit::Kilobit, _) => "{0} கிலோபிட்கள்",
            (Unit::Kilobyte, PluralCategory::One) => "{0} கிலோபைட்",
            (Unit::Kilobyte, _) => "{0} கிலோபைட்கள்",
            (Unit::Megabit, PluralCategory::One) => "{0} மெகாபிட்",
            (Unit::Megabit, _) => "{0} மெகாபிட்கள்",
            (Unit::Megabyte, PluralCategory::One) => "{0} மெகாபைட்",
            (Unit::Megabyte, _) => "{0} மெகாபைட்கள்",
            (Unit::Percent, _) => "{0} சதவீதம்",
            (Unit::Petabyte, PluralCategory::One) => "{0} பெடாபைட்",
            (Unit::Petabyte, _) => "{0} பெடாபைட்கள்",
            (Unit::Terabit, PluralCategory::One) => "{0} டெராபிட்",
            (Unit::Terabit, _) => "{0} டெராபிட்கள்",
            (Unit::Terabyte, PluralCategory::One) => "{0} டெராபைட்",
            (Unit::Terabyte, _) => "{0} டெராபைட்கள்",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} பிட்",
            Unit::Byte => "{0} பை.",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} டிகி.",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}பிட்",
            Unit::Byte => "{0}பை.",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}PB",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

// Returns Tamil denominator-specific pinned CLDR `perUnitPattern` records.

// Composes Tamil generic compounds containing an ICU4X-untyped unit.
