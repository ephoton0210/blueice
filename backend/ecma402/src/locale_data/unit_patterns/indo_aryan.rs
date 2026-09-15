// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Indo-Aryan raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_urdu(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("ur")
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

/// Returns Urdu denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_urdu_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_urdu(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} فی سینٹی میٹر",
        "{0} فی دن",
        "{0} فی فوٹ",
        "{0} فی گیلن",
        "{0} فی گرام",
        "{0} فی گھنٹہ",
        "{0} فی انچ",
        "{0} فی کلو گرام",
        "{0} فی کلومیٹر",
        "{0} فی لیٹر",
        "{0} فی میٹر",
        "{0} فی منٹ",
        "{0} فی ماہ",
        "{0} فی اونس",
        "{0} فی پونڈ",
        "{0} فی سیکنڈ",
        "{0} فی ہفتہ",
        "فی سال {0}",
    ];
    const SHORT: [&str; 18] = [
        "{0}/cm",
        "{0} فی دن",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0} فی گھنٹہ",
        "{0}/in",
        "{0}/kg",
        "{0} فی کلو میٹر",
        "{0} فی لیٹر",
        "{0}/m",
        "{0} فی منٹ",
        "{0}/m",
        "{0}/oz",
        "{0}/lb",
        "{0} فی سیکنڈ",
        "{0} فی ہفتہ",
        "{0}/y",
    ];
    const NARROW: [&str; 18] = [
        "{0} فی سینٹی میٹر",
        "{0} فی دن",
        "{0} فی فیٹ",
        "{0}/gal",
        "{0} فی گرام",
        "{0} فی گھنٹہ",
        "{0} فی انچ",
        "{0} فی کلو گرام",
        "{0} فی کلو میٹر",
        "{0} فی لیٹر",
        "{0} فی میٹر",
        "{0} فی منٹ",
        "{0}/مہینہ",
        "{0}/oz",
        "{0}/lb",
        "{0} فی سیکنڈ",
        "{0}/ہفتہ",
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

/// Composes Urdu generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_urdu_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_urdu(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator = cldr_urdu_additional_unit_pattern(locale, numerator, display, plural)
        .or_else(|| experimental_number_unit_pattern(locale, numerator, display, plural))?;
    let denominator = cldr_urdu_additional_unit_pattern(
        locale,
        denominator_unit,
        denominator_display,
        crate::PluralCategory::One,
    )
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
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        match display {
            Display::Long => "{0} فی {1}",
            Display::Short | Display::Narrow => "{0}/{1}",
        },
    )
}
