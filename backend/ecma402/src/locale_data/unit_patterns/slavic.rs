// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Slavic raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn four_form_cardinal_pattern(
    one: &'static str,
    few: &'static str,
    many: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    match plural {
        crate::PluralCategory::One => one,
        crate::PluralCategory::Few => few,
        crate::PluralCategory::Many => many,
        crate::PluralCategory::Zero | crate::PluralCategory::Two | crate::PluralCategory::Other => {
            other
        }
    }
}

/// Returns pinned Polish CLDR records for the simple categories unavailable
/// through ICU4X typed unit markers. Long forms preserve all four cardinal
/// records rather than collapsing Polish into English one/other forms.
pub(crate) fn cldr_polish_additional_unit_pattern(
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
        != Some("pl")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => {
                four_form_cardinal_pattern("{0} bit", "{0} bity", "{0} bitów", "{0} bita", plural)
            }
            Unit::Byte => four_form_cardinal_pattern(
                "{0} bajt",
                "{0} bajty",
                "{0} bajtów",
                "{0} bajta",
                plural,
            ),
            Unit::Celsius => four_form_cardinal_pattern(
                "{0} stopień Celsjusza",
                "{0} stopnie Celsjusza",
                "{0} stopni Celsjusza",
                "{0} stopnia Celsjusza",
                plural,
            ),
            Unit::Degree => four_form_cardinal_pattern(
                "{0} stopień",
                "{0} stopnie",
                "{0} stopni",
                "{0} stopnia",
                plural,
            ),
            Unit::Fahrenheit => four_form_cardinal_pattern(
                "{0} stopień Fahrenheita",
                "{0} stopnie Fahrenheita",
                "{0} stopni Fahrenheita",
                "{0} stopnia Fahrenheita",
                plural,
            ),
            Unit::Gigabit => four_form_cardinal_pattern(
                "{0} gigabit",
                "{0} gigabity",
                "{0} gigabitów",
                "{0} gigabita",
                plural,
            ),
            Unit::Gigabyte => four_form_cardinal_pattern(
                "{0} gigabajt",
                "{0} gigabajty",
                "{0} gigabajtów",
                "{0} gigabajta",
                plural,
            ),
            Unit::Kilobit => four_form_cardinal_pattern(
                "{0} kilobit",
                "{0} kilobity",
                "{0} kilobitów",
                "{0} kilobita",
                plural,
            ),
            Unit::Kilobyte => four_form_cardinal_pattern(
                "{0} kilobajt",
                "{0} kilobajty",
                "{0} kilobajtów",
                "{0} kilobajta",
                plural,
            ),
            Unit::Megabit => four_form_cardinal_pattern(
                "{0} megabit",
                "{0} megabity",
                "{0} megabitów",
                "{0} megabita",
                plural,
            ),
            Unit::Megabyte => four_form_cardinal_pattern(
                "{0} megabajt",
                "{0} megabajty",
                "{0} megabajtów",
                "{0} megabajta",
                plural,
            ),
            Unit::Percent => "{0} procent",
            Unit::Petabyte => four_form_cardinal_pattern(
                "{0} petabajt",
                "{0} petabajty",
                "{0} petabajtów",
                "{0} petabajta",
                plural,
            ),
            Unit::Terabit => four_form_cardinal_pattern(
                "{0} terabit",
                "{0} terabity",
                "{0} terabitów",
                "{0} terabita",
                plural,
            ),
            Unit::Terabyte => four_form_cardinal_pattern(
                "{0} terabajt",
                "{0} terabajty",
                "{0} terabajtów",
                "{0} terabajta",
                plural,
            ),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0} st. C",
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
        Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°C",
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

/// Composes Polish generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_polish_generic_compound_unit_pattern(
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
        != Some("pl")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_polish_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_polish_additional_unit_pattern(
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
        Display::Long => "{0} na {1}",
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

/// Returns pinned Ukrainian CLDR records for all simple categories not
/// generated by ICU4X's typed unit markers. Ukrainian needs the same four
/// cardinal forms as Polish, but with its own Cyrillic labels, narrow joins,
/// and non-breaking short-width temperature/percent spacing.
pub(crate) fn cldr_ukrainian_additional_unit_pattern(
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
        != Some("uk")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => {
                four_form_cardinal_pattern("{0} біт", "{0} біти", "{0} бітів", "{0} біта", plural)
            }
            Unit::Byte => four_form_cardinal_pattern(
                "{0} байт",
                "{0} байти",
                "{0} байтів",
                "{0} байта",
                plural,
            ),
            Unit::Celsius => four_form_cardinal_pattern(
                "{0} градус Цельсія",
                "{0} градуси Цельсія",
                "{0} градусів Цельсія",
                "{0} градуса Цельсія",
                plural,
            ),
            Unit::Degree => four_form_cardinal_pattern(
                "{0} градус",
                "{0} градуси",
                "{0} градусів",
                "{0} градуса",
                plural,
            ),
            Unit::Fahrenheit => four_form_cardinal_pattern(
                "{0} градус Фаренгейта",
                "{0} градуси Фаренгейта",
                "{0} градусів Фаренгейта",
                "{0} градуса Фаренгейта",
                plural,
            ),
            Unit::Gigabit => four_form_cardinal_pattern(
                "{0} гігабіт",
                "{0} гігабіти",
                "{0} гігабітів",
                "{0} гігабіта",
                plural,
            ),
            Unit::Gigabyte => four_form_cardinal_pattern(
                "{0} гігабайт",
                "{0} гігабайти",
                "{0} гігабайтів",
                "{0} гігабайта",
                plural,
            ),
            Unit::Kilobit => four_form_cardinal_pattern(
                "{0} кілобіт",
                "{0} кілобіти",
                "{0} кілобітів",
                "{0} кілобіта",
                plural,
            ),
            Unit::Kilobyte => four_form_cardinal_pattern(
                "{0} кілобайт",
                "{0} кілобайти",
                "{0} кілобайтів",
                "{0} кілобайта",
                plural,
            ),
            Unit::Megabit => four_form_cardinal_pattern(
                "{0} мегабіт",
                "{0} мегабіти",
                "{0} мегабітів",
                "{0} мегабіта",
                plural,
            ),
            Unit::Megabyte => four_form_cardinal_pattern(
                "{0} мегабайт",
                "{0} мегабайти",
                "{0} мегабайтів",
                "{0} мегабайта",
                plural,
            ),
            Unit::Percent => four_form_cardinal_pattern(
                "{0} відсоток",
                "{0} відсотки",
                "{0} відсотків",
                "{0} відсотка",
                plural,
            ),
            Unit::Petabyte => four_form_cardinal_pattern(
                "{0} петабайт",
                "{0} петабайти",
                "{0} петабайтів",
                "{0} петабайта",
                plural,
            ),
            Unit::Terabit => four_form_cardinal_pattern(
                "{0} терабіт",
                "{0} терабіти",
                "{0} терабітів",
                "{0} терабіта",
                plural,
            ),
            Unit::Terabyte => four_form_cardinal_pattern(
                "{0} терабайт",
                "{0} терабайти",
                "{0} терабайтів",
                "{0} терабайта",
                plural,
            ),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} б",
            Unit::Byte => "{0} Б",
            Unit::Celsius => "{0}\u{a0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}\u{a0}°F",
            Unit::Gigabit => "{0} Гб",
            Unit::Gigabyte => "{0} ГБ",
            Unit::Kilobit => "{0} кб",
            Unit::Kilobyte => "{0} кБ",
            Unit::Megabit => "{0} Мб",
            Unit::Megabyte => "{0} МБ",
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} ПБ",
            Unit::Terabit => "{0} Тб",
            Unit::Terabyte => "{0} ТБ",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0}б",
            Unit::Byte => "{0}Б",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0}Гб",
            Unit::Gigabyte => "{0}ГБ",
            Unit::Kilobit => "{0}кб",
            Unit::Kilobyte => "{0}кБ",
            Unit::Megabit => "{0}Мб",
            Unit::Megabyte => "{0}МБ",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0}ПБ",
            Unit::Terabit => "{0}Тб",
            Unit::Terabyte => "{0}ТБ",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Composes Ukrainian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_ukrainian_generic_compound_unit_pattern(
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
        != Some("uk")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_ukrainian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_ukrainian_additional_unit_pattern(
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
        Display::Long => "{0} на {1}",
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

/// Returns pinned Czech CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers. Czech's `many` record deliberately
/// differs from both its `few` and decimal `other` forms.
pub(crate) fn cldr_czech_additional_unit_pattern(
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
        != Some("cs")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => {
                four_form_cardinal_pattern("{0} bit", "{0} bity", "{0} bitu", "{0} bitů", plural)
            }
            Unit::Byte => four_form_cardinal_pattern(
                "{0} bajt",
                "{0} bajty",
                "{0} bajtu",
                "{0} bajtů",
                plural,
            ),
            Unit::Celsius => four_form_cardinal_pattern(
                "{0} stupeň Celsia",
                "{0} stupně Celsia",
                "{0} stupně Celsia",
                "{0} stupňů Celsia",
                plural,
            ),
            Unit::Degree => four_form_cardinal_pattern(
                "{0} stupeň",
                "{0} stupně",
                "{0} stupně",
                "{0} stupňů",
                plural,
            ),
            Unit::Fahrenheit => four_form_cardinal_pattern(
                "{0} stupeň Fahrenheita",
                "{0} stupně Fahrenheita",
                "{0} stupně Fahrenheita",
                "{0} stupňů Fahrenheita",
                plural,
            ),
            Unit::Gigabit => four_form_cardinal_pattern(
                "{0} gigabit",
                "{0} gigabity",
                "{0} gigabitu",
                "{0} gigabitů",
                plural,
            ),
            Unit::Gigabyte => four_form_cardinal_pattern(
                "{0} gigabajt",
                "{0} gigabajty",
                "{0} gigabajtu",
                "{0} gigabajtů",
                plural,
            ),
            Unit::Kilobit => four_form_cardinal_pattern(
                "{0} kilobit",
                "{0} kilobity",
                "{0} kilobitu",
                "{0} kilobitů",
                plural,
            ),
            Unit::Kilobyte => four_form_cardinal_pattern(
                "{0} kilobajt",
                "{0} kilobajty",
                "{0} kilobajtu",
                "{0} kilobajtů",
                plural,
            ),
            Unit::Megabit => four_form_cardinal_pattern(
                "{0} megabit",
                "{0} megabity",
                "{0} megabitu",
                "{0} megabitů",
                plural,
            ),
            Unit::Megabyte => four_form_cardinal_pattern(
                "{0} megabajt",
                "{0} megabajty",
                "{0} megabajtu",
                "{0} megabajtů",
                plural,
            ),
            Unit::Percent => four_form_cardinal_pattern(
                "{0} procento",
                "{0} procenta",
                "{0} procenta",
                "{0} procent",
                plural,
            ),
            Unit::Petabyte => four_form_cardinal_pattern(
                "{0} petabajt",
                "{0} petabajty",
                "{0} petabajtu",
                "{0} petabajtů",
                plural,
            ),
            Unit::Terabit => four_form_cardinal_pattern(
                "{0} terabit",
                "{0} terabity",
                "{0} terabitu",
                "{0} terabitů",
                plural,
            ),
            Unit::Terabyte => four_form_cardinal_pattern(
                "{0} terabajt",
                "{0} terabajty",
                "{0} terabajtu",
                "{0} terabajtů",
                plural,
            ),
            _ => return None,
        },
        Display::Short | Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0} °C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0} °F",
            Unit::Gigabit => "{0} Gb",
            Unit::Gigabyte => "{0} GB",
            Unit::Kilobit => "{0} kb",
            Unit::Kilobyte => "{0} kB",
            Unit::Megabit => "{0} Mb",
            Unit::Megabyte => "{0} MB",
            Unit::Percent => "{0} %",
            Unit::Petabyte => "{0} PB",
            Unit::Terabit => "{0} Tb",
            Unit::Terabyte => "{0} TB",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Composes Czech generic compounds containing an ICU4X-untyped unit. CLDR's
/// generic Czech `per` connector is slash-based; denominator-specific long
/// forms still take precedence through the shared `perUnitPattern` table.
pub(crate) fn cldr_czech_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    if locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        != Some("cs")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_czech_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        crate::NumberUnitDisplay::Long => crate::NumberUnitDisplay::Long,
        crate::NumberUnitDisplay::Short | crate::NumberUnitDisplay::Narrow => {
            crate::NumberUnitDisplay::Narrow
        }
    };
    let denominator_raw = cldr_czech_additional_unit_pattern(
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

fn bulgarian_cardinal_pattern(
    one: &'static str,
    other: &'static str,
    plural: crate::PluralCategory,
) -> &'static str {
    if plural == crate::PluralCategory::One {
        one
    } else {
        other
    }
}

/// Returns pinned Bulgarian CLDR records for every simple category
/// unavailable through ICU4X's typed unit markers.
pub(crate) fn cldr_bulgarian_additional_unit_pattern(
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
        != Some("bg")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => bulgarian_cardinal_pattern("{0} бит", "{0} бита", plural),
            Unit::Byte => bulgarian_cardinal_pattern("{0} байт", "{0} байта", plural),
            Unit::Celsius => {
                bulgarian_cardinal_pattern("{0} градус Целзий", "{0} градуса Целзий", plural)
            }
            Unit::Degree => bulgarian_cardinal_pattern("{0} градус", "{0} градуса", plural),
            Unit::Fahrenheit => bulgarian_cardinal_pattern(
                "{0} градус по Фаренхайт",
                "{0} градуса по Фаренхайт",
                plural,
            ),
            Unit::Gigabit => bulgarian_cardinal_pattern("{0} гигабит", "{0} гигабита", plural),
            Unit::Gigabyte => bulgarian_cardinal_pattern("{0} гигабайт", "{0} гигабайта", plural),
            Unit::Kilobit => bulgarian_cardinal_pattern("{0} килобит", "{0} килобита", plural),
            Unit::Kilobyte => bulgarian_cardinal_pattern("{0} килобайт", "{0} килобайта", plural),
            Unit::Megabit => bulgarian_cardinal_pattern("{0} мегабит", "{0} мегабита", plural),
            Unit::Megabyte => bulgarian_cardinal_pattern("{0} мегабайт", "{0} мегабайта", plural),
            Unit::Percent => bulgarian_cardinal_pattern("{0} процент", "{0} процента", plural),
            Unit::Petabyte => bulgarian_cardinal_pattern("{0} петабайт", "{0} петабайта", plural),
            Unit::Terabit => bulgarian_cardinal_pattern("{0} терабит", "{0} терабита", plural),
            Unit::Terabyte => bulgarian_cardinal_pattern("{0} терабайт", "{0} терабайта", plural),
            _ => return None,
        },
        Display::Short | Display::Narrow => match unit {
            Unit::Bit => "{0} b",
            Unit::Byte => "{0} B",
            Unit::Celsius => "{0}°C",
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

/// Returns Bulgaria's denominator-specific pinned CLDR `perUnitPattern`
/// records, co-located with the Bulgarian simple-unit records.
pub(crate) fn cldr_bulgarian_per_unit_pattern(
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
        != Some("bg")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} на сантиметър",
        "{0} на ден",
        "{0} на фут",
        "{0} на галон",
        "{0} на грам",
        "{0} за час",
        "{0} на инч",
        "{0} на килограм",
        "{0} на километър",
        "{0} на литър",
        "{0} на метър",
        "{0} на минута",
        "{0} на месец",
        "{0} на унция",
        "{0} на фунт",
        "{0} за секунда",
        "{0} на седмица",
        "{0} на година",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/д",
        "{0}/ft",
        "{0}/gal US",
        "{0}/g",
        "{0}/ч",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/мин",
        "{0}/месец",
        "{0}/oz",
        "{0}/lb",
        "{0}/сек",
        "{0}/седмица",
        "{0}/год.",
    ];
    const NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/д",
        "{0}/ft",
        "{0}/gal US",
        "{0}/g",
        "{0}/ч",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/мин",
        "{0}/мес.",
        "{0}/oz",
        "{0}/lb",
        "{0}/сек",
        "{0}/седм.",
        "{0}/год.",
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

/// Composes Bulgarian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_bulgarian_generic_compound_unit_pattern(
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
        != Some("bg")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_bulgarian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_bulgarian_additional_unit_pattern(
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
        Display::Long => "{0} на {1}",
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
