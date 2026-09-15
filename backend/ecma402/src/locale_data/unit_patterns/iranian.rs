// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Iranian raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

/// Returns pinned Persian CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_persian_additional_unit_pattern(
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
        != Some("fa")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} بیت",
            Unit::Byte => "{0} بایت",
            Unit::Celsius => "{0} درجهٔ سلسیوس",
            Unit::Degree => "{0} درجه",
            Unit::Fahrenheit => "{0} درجهٔ فارنهایت",
            Unit::Gigabit => "{0} گیگابیت",
            Unit::Gigabyte => "{0} گیگابایت",
            Unit::Kilobit => "{0} کیلوبیت",
            Unit::Kilobyte => "{0} کیلوبایت",
            Unit::Megabit => "{0} مگابیت",
            Unit::Megabyte => "{0} مگابایت",
            Unit::Percent => "{0} درصد",
            Unit::Petabyte => "{0} پتابایت",
            Unit::Terabit => "{0} ترابیت",
            Unit::Terabyte => "{0} ترابایت",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} بیت",
            Unit::Byte => "{0} بایت",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0} درجه",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0}٪",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} بیت",
            Unit::Byte => "{0} بایت",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Gb",
            Unit::Gigabyte => "{0}GB",
            Unit::Kilobit => "{0}kb",
            Unit::Kilobyte => "{0}kB",
            Unit::Megabit => "{0}Mb",
            Unit::Megabyte => "{0}MB",
            Unit::Percent => "{0}٪",
            Unit::Petabyte => "{0}PB",
            Unit::Terabit => "{0}Tb",
            Unit::Terabyte => "{0}TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Persian denominator-specific pinned CLDR `perUnitPattern`
/// records, co-located with its Iranian simple-unit records.
pub(crate) fn cldr_persian_per_unit_pattern(
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
        != Some("fa")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} در سانتی‌متر",
        "{0} در روز",
        "{0} در فوت",
        "{0} در گالن",
        "{0}/g",
        "{0} در ساعت",
        "{0} در اینچ",
        "{0} در کیلوگرم",
        "{0} در کیلومتر",
        "{0} در لیتر",
        "{0} در متر",
        "{0} در دقیقه",
        "{0} در ماه",
        "{0} در اونس",
        "{0} در پوند",
        "{0} در ثانیه",
        "{0} در هفته",
        "{0} در سال",
    ];
    const SHORT: [&str; 18] = [
        "{0}/سانتی‌متر",
        "{0}/روز",
        "{0}/فوت",
        "{0} در گالن",
        "{0}/g",
        "{0} در ساعت",
        "{0}/اینچ",
        "{0}\u{200e}/kg",
        "{0}/کیلومتر",
        "{0}\u{200e}/L",
        "{0}/متر",
        "{0} در دقیقه",
        "{0}/ماه",
        "{0} در اونس",
        "{0} در پوند",
        "{0} در ثانیه",
        "{0}/هفته",
        "{0}/سال",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/روز",
        "{0}/ft",
        "{0} در گالن",
        "{0}/g",
        "{0}/ساعت",
        "{0}/اینچ",
        "{0}\u{200e}/kg",
        "{0}\u{200e}/km",
        "{0}\u{200e}/L",
        "{0}\u{200e}/m",
        "{0}/دقیقه",
        "{0}/ماه",
        "{0}/oz",
        "{0} در پوند",
        "{0}/ثانیه",
        "{0}/هفته",
        "{0}/سال",
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

/// Composes Persian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_persian_generic_compound_unit_pattern(
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
        != Some("fa")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_persian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_persian_additional_unit_pattern(
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
            Display::Long => "{0} در {1}",
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
