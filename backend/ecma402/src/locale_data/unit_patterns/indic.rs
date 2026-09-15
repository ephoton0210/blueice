// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Devanagari Indic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

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

/// Composes Devanagari Hindi generic compounds when either operand belongs to
/// an ICU4X-untyped category.
pub(crate) fn cldr_hindi_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !uses_hindi_devanagari_unit_data(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_hindi_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_hindi_additional_unit_pattern(
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
    let denominator = denominator_raw.or_else(|| {
        experimental_number_unit_pattern(
            locale,
            denominator_unit,
            denominator_display,
            crate::PluralCategory::One,
        )
    })?;
    let generic_per = match display {
        Display::Long => "{0} प्रति {1}",
        Display::Short | Display::Narrow => "{0}/{1}",
    };
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        generic_per,
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}
