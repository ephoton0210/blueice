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

/// Returns Bengali denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_bengali_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("bn")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} প্রতি সেন্টিমিটার",
        "{0}/দিন",
        "{0} প্রতি ফুট",
        "{0} প্রতি গ্যালন",
        "{0} প্রতি গ্রাম",
        "{0} প্রতি ঘণ্টা",
        "{0} প্রতি ইঞ্চি",
        "{0} প্রতি কিলোগ্রাম",
        "{0} প্রতি কিলোমিটার",
        "{0} প্রতি লিটার",
        "{0} প্রতি মিটার",
        "{0} প্রতি মিনিট",
        "{0} প্রতি মাস",
        "{0} প্রতি আউন্স",
        "{0} প্রতি পাউন্ড",
        "{0} প্রতি সেকেন্ড",
        "{0} প্রতি সপ্তাহ",
        "{0} প্রতি বছর",
    ];
    const SHORT: [&str; 18] = [
        "{0} প্রতি সেমি",
        "{0}/দিন",
        "{0} প্রতি ফুট",
        "{0}/gal US",
        "{0} প্রতি গ্রাম",
        "{0} প্রতি ঘন্টা",
        "{0} প্রতি ইঞ্চি",
        "{0} প্রতি কেজি",
        "{0} প্রতি কিমি",
        "{0}/l",
        "{0} প্রতি মি",
        "{0} প্রতি মিনিট",
        "{0} প্রতি মাস",
        "{0} প্রতি আউন্স",
        "{0}/lb",
        "{0} প্রতি সেকেন্ড",
        "{0} প্রতি সপ্তাহ",
        "{0} প্রতি বছর",
    ];
    const NARROW: [&str; 18] = [
        "{0}/সেমি",
        "{0}/দিন",
        "{0} প্রতি ফুট",
        "{0}/gal",
        "{0}/গ্রা:",
        "{0}/ঘ:",
        "{0}/ইঞ্চি",
        "{0}/কেজি",
        "{0}/কিমি",
        "{0}/l",
        "{0}/মি",
        "{0}/মি:",
        "{0}/মাস",
        "{0}/আউন্স",
        "{0}/পাউন্ড",
        "{0}/সেঃ",
        "{0}/সপ্তাহ",
        "{0}/বছর",
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

/// Composes Bengali generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_bengali_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("bn")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_bengali_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_bengali_additional_unit_pattern(
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
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        match display {
            Display::Long => "{0} প্রতি {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}

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

/// Returns Tamil denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_tamil_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ta")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0}/சென்டிமீட்டர்",
        "{0} / நாள்",
        "{0}/அடி",
        "{0}/கேலன்",
        "{0}/கிராம்",
        "{0} / மணிநேரம்",
        "{0}/அங்குலம்",
        "{0}/கிலோகிராம்",
        "{0}/கிலோமீட்டர்",
        "{0}/லிட்டர்",
        "{0}/மீட்டர்",
        "{0} / நிமிடம்",
        "{0} / மாதம்",
        "{0}/அவுன்ஸ்",
        "{0}/பவுண்டு",
        "{0}/விநாடி",
        "{0} / வாரம்",
        "ஒரு வருடத்தில் {0}",
    ];
    const SHORT: [&str; 18] = [
        "{0}/செ.மீ.",
        "{0}/நா",
        "{0}/அடி",
        "{0}/கேல.",
        "{0}/கி.",
        "{0} /ம.நே",
        "{0}/அங்.",
        "{0}/kg",
        "{0}/கி.மீ.",
        "{0}/லி.",
        "{0}/மீ.",
        "{0}/நிமி.",
        "{0}/மா",
        "{0}/அவு.",
        "{0}/lb",
        "{0}/வி.",
        "{0}/வா.",
        "{0}/ஆ.",
    ];
    const NARROW: [&str; 18] = [
        "{0}/செ.மீ.",
        "{0}/நா",
        "{0}/அடி",
        "{0}/கேல.",
        "{0}/கி.",
        "{0} /ம.நே",
        "{0}/அங்.",
        "{0}/kg",
        "{0}/கி.மீ.",
        "{0}/லி.",
        "{0}/மீ.",
        "{0}/நிமி.",
        "{0}/மா",
        "{0}/அவு.",
        "{0}/lb",
        "{0}/வி.",
        "{0}/வா.",
        "{0}/ஆ.",
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

/// Composes Tamil generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_tamil_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("ta")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_tamil_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_tamil_additional_unit_pattern(
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
    let label = localized_generic_compound_unit_label(
        locale,
        denominator_unit,
        display,
        &number_unit_pattern_label(&numerator),
        &number_unit_pattern_label(&denominator),
        "{0}/{1}",
    )?;
    Some(NumberGenericCompoundUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: numerator.suffix_separator,
        suffix: label,
    })
}
