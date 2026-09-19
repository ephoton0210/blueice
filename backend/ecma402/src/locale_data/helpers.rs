// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn plural_category_sample(
    rules: &IcuPluralRules,
    expected: crate::PluralCategory,
) -> Option<usize> {
    for sample in 0..=200 {
        if plural_category_from_icu(rules.category_for(sample)) == expected {
            return Some(sample);
        }
    }
    [1_000, 1_000_000]
        .into_iter()
        .find(|sample| plural_category_from_icu(rules.category_for(*sample)) == expected)
}

/// Returns only the cardinal categories that can be selected for a locale.
///
/// Coverage is measured over observable patterns. A category whose plural
/// rule can never return it is not a missing locale-data cell. If ICU lacks a
/// rule for an otherwise advertised NumberFormat locale, retain `other` so
/// the report makes that fallback visible instead of reporting an empty
/// matrix.
pub(super) fn number_format_plural_categories(locale: &str) -> Vec<crate::PluralCategory> {
    const CATEGORIES: &[crate::PluralCategory] = &[
        crate::PluralCategory::Zero,
        crate::PluralCategory::One,
        crate::PluralCategory::Two,
        crate::PluralCategory::Few,
        crate::PluralCategory::Many,
        crate::PluralCategory::Other,
    ];
    let Ok(locale) = crate::canonicalize(locale) else {
        return vec![crate::PluralCategory::Other];
    };
    let Ok(rules) = IcuPluralRules::try_new_cardinal(locale.locale().into()) else {
        return vec![crate::PluralCategory::Other];
    };
    let categories = CATEGORIES
        .iter()
        .copied()
        .filter(|category| plural_category_sample(&rules, *category).is_some())
        .collect::<Vec<_>>();
    if categories.is_empty() {
        vec![crate::PluralCategory::Other]
    } else {
        categories
    }
}

pub(super) fn plural_category_from_icu(category: IcuPluralCategory) -> crate::PluralCategory {
    match category {
        IcuPluralCategory::Zero => crate::PluralCategory::Zero,
        IcuPluralCategory::One => crate::PluralCategory::One,
        IcuPluralCategory::Two => crate::PluralCategory::Two,
        IcuPluralCategory::Few => crate::PluralCategory::Few,
        IcuPluralCategory::Many => crate::PluralCategory::Many,
        IcuPluralCategory::Other => crate::PluralCategory::Other,
    }
}

pub(super) fn plural_category_to_icu(category: crate::PluralCategory) -> IcuPluralCategory {
    match category {
        crate::PluralCategory::Zero => IcuPluralCategory::Zero,
        crate::PluralCategory::One => IcuPluralCategory::One,
        crate::PluralCategory::Two => IcuPluralCategory::Two,
        crate::PluralCategory::Few => IcuPluralCategory::Few,
        crate::PluralCategory::Many => IcuPluralCategory::Many,
        crate::PluralCategory::Other => IcuPluralCategory::Other,
    }
}

pub(super) fn number_unit_pattern_from_placeholder(rendered: &str) -> Option<NumberUnitPattern> {
    let Some((prefix, suffix)) = rendered.split_once('\u{fdd0}') else {
        return (!rendered.is_empty()).then(|| NumberUnitPattern {
            prefix: String::new(),
            prefix_separator: String::new(),
            suffix_separator: String::new(),
            suffix: rendered.into(),
            hides_number: true,
        });
    };
    let prefix_label = prefix.trim_end();
    let suffix_label = suffix.trim_start();
    Some(NumberUnitPattern {
        prefix: prefix_label.into(),
        prefix_separator: prefix[prefix_label.len()..].into(),
        suffix_separator: suffix[..suffix.len() - suffix_label.len()].into(),
        suffix: suffix_label.into(),
        hides_number: false,
    })
}

pub(super) fn number_unit_pattern_label(pattern: &NumberUnitPattern) -> String {
    format!("{}{}", pattern.prefix, pattern.suffix)
}

pub(super) fn english_number_unit_pattern(
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    singular: bool,
) -> NumberUnitPattern {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay};

    let (separator, suffix) = match display {
        NumberUnitDisplay::Long => (
            " ",
            match (unit, singular) {
                (Unit::Acre, true) => "acre",
                (Unit::Acre, false) => "acres",
                (Unit::Bit, true) => "bit",
                (Unit::Bit, false) => "bits",
                (Unit::Byte, true) => "byte",
                (Unit::Byte, false) => "bytes",
                (Unit::Celsius, true) => "degree Celsius",
                (Unit::Celsius, false) => "degrees Celsius",
                (Unit::Centimeter, true) => "centimeter",
                (Unit::Centimeter, false) => "centimeters",
                (Unit::Day, true) => "day",
                (Unit::Day, false) => "days",
                (Unit::Degree, true) => "degree",
                (Unit::Degree, false) => "degrees",
                (Unit::Fahrenheit, true) => "degree Fahrenheit",
                (Unit::Fahrenheit, false) => "degrees Fahrenheit",
                (Unit::FluidOunce, true) => "fluid ounce",
                (Unit::FluidOunce, false) => "fluid ounces",
                (Unit::Foot, true) => "foot",
                (Unit::Foot, false) => "feet",
                (Unit::Gallon, true) => "gallon",
                (Unit::Gallon, false) => "gallons",
                (Unit::Gigabit, true) => "gigabit",
                (Unit::Gigabit, false) => "gigabits",
                (Unit::Gigabyte, true) => "gigabyte",
                (Unit::Gigabyte, false) => "gigabytes",
                (Unit::Gram, true) => "gram",
                (Unit::Gram, false) => "grams",
                (Unit::Hectare, true) => "hectare",
                (Unit::Hectare, false) => "hectares",
                (Unit::Hour, true) => "hour",
                (Unit::Hour, false) => "hours",
                (Unit::Inch, true) => "inch",
                (Unit::Inch, false) => "inches",
                (Unit::Kilobit, true) => "kilobit",
                (Unit::Kilobit, false) => "kilobits",
                (Unit::Kilobyte, true) => "kilobyte",
                (Unit::Kilobyte, false) => "kilobytes",
                (Unit::Kilogram, true) => "kilogram",
                (Unit::Kilogram, false) => "kilograms",
                (Unit::Kilometer, true) => "kilometer",
                (Unit::Kilometer, false) => "kilometers",
                (Unit::Liter, true) => "liter",
                (Unit::Liter, false) => "liters",
                (Unit::Megabit, true) => "megabit",
                (Unit::Megabit, false) => "megabits",
                (Unit::Megabyte, true) => "megabyte",
                (Unit::Megabyte, false) => "megabytes",
                (Unit::Meter, true) => "meter",
                (Unit::Meter, false) => "meters",
                (Unit::Microsecond, true) => "microsecond",
                (Unit::Microsecond, false) => "microseconds",
                (Unit::Mile, true) => "mile",
                (Unit::Mile, false) => "miles",
                (Unit::MileScandinavian, true) => "mile-scandinavian",
                (Unit::MileScandinavian, false) => "miles-scandinavian",
                (Unit::Milliliter, true) => "milliliter",
                (Unit::Milliliter, false) => "milliliters",
                (Unit::Millimeter, true) => "millimeter",
                (Unit::Millimeter, false) => "millimeters",
                (Unit::Millisecond, true) => "millisecond",
                (Unit::Millisecond, false) => "milliseconds",
                (Unit::Minute, true) => "minute",
                (Unit::Minute, false) => "minutes",
                (Unit::Month, true) => "month",
                (Unit::Month, false) => "months",
                (Unit::Nanosecond, true) => "nanosecond",
                (Unit::Nanosecond, false) => "nanoseconds",
                (Unit::Ounce, true) => "ounce",
                (Unit::Ounce, false) => "ounces",
                (Unit::Percent, _) => "percent",
                (Unit::Petabyte, true) => "petabyte",
                (Unit::Petabyte, false) => "petabytes",
                (Unit::Pound, true) => "pound",
                (Unit::Pound, false) => "pounds",
                (Unit::Second, true) => "second",
                (Unit::Second, false) => "seconds",
                (Unit::Stone, true) => "stone",
                (Unit::Stone, false) => "stones",
                (Unit::Terabit, true) => "terabit",
                (Unit::Terabit, false) => "terabits",
                (Unit::Terabyte, true) => "terabyte",
                (Unit::Terabyte, false) => "terabytes",
                (Unit::Week, true) => "week",
                (Unit::Week, false) => "weeks",
                (Unit::Yard, true) => "yard",
                (Unit::Yard, false) => "yards",
                (Unit::Year, true) => "year",
                (Unit::Year, false) => "years",
                (Unit::CompoundPer { .. }, _) => unit.as_str(),
            },
        ),
        NumberUnitDisplay::Short => (
            if unit == Unit::Percent { "" } else { " " },
            match (unit, singular) {
                (Unit::Acre, _) => "ac",
                (Unit::Bit, _) => "bit",
                (Unit::Byte, _) => "byte",
                (Unit::Celsius, _) => "°C",
                (Unit::Centimeter, _) => "cm",
                (Unit::Day, true) => "day",
                (Unit::Day, false) => "days",
                (Unit::Degree, _) => "deg",
                (Unit::Fahrenheit, _) => "°F",
                (Unit::FluidOunce, _) => "fl oz",
                (Unit::Foot, _) => "ft",
                (Unit::Gallon, _) => "gal",
                (Unit::Gigabit, _) => "Gb",
                (Unit::Gigabyte, _) => "GB",
                (Unit::Gram, _) => "g",
                (Unit::Hectare, _) => "ha",
                (Unit::Hour, _) => "hr",
                (Unit::Inch, _) => "in",
                (Unit::Kilobit, _) => "kb",
                (Unit::Kilobyte, _) => "kB",
                (Unit::Kilogram, _) => "kg",
                (Unit::Kilometer, _) => "km",
                (Unit::Liter, _) => "L",
                (Unit::Megabit, _) => "Mb",
                (Unit::Megabyte, _) => "MB",
                (Unit::Meter, _) => "m",
                (Unit::Microsecond, _) => "μs",
                (Unit::Mile, _) => "mi",
                (Unit::MileScandinavian, _) => "smi",
                (Unit::Milliliter, _) => "mL",
                (Unit::Millimeter, _) => "mm",
                (Unit::Millisecond, _) => "ms",
                (Unit::Minute, _) => "min",
                (Unit::Month, true) => "mth",
                (Unit::Month, false) => "mths",
                (Unit::Nanosecond, _) => "ns",
                (Unit::Ounce, _) => "oz",
                (Unit::Percent, _) => "%",
                (Unit::Petabyte, _) => "PB",
                (Unit::Pound, _) => "lb",
                (Unit::Second, _) => "sec",
                (Unit::Stone, _) => "st",
                (Unit::Terabit, _) => "Tb",
                (Unit::Terabyte, _) => "TB",
                (Unit::Week, true) => "wk",
                (Unit::Week, false) => "wks",
                (Unit::Yard, _) => "yd",
                (Unit::Year, true) => "yr",
                (Unit::Year, false) => "yrs",
                (Unit::CompoundPer { .. }, _) => unit.as_str(),
            },
        ),
        NumberUnitDisplay::Narrow => (
            "",
            match unit {
                Unit::Acre => "ac",
                Unit::Bit => "bit",
                Unit::Byte => "B",
                Unit::Celsius => "°C",
                Unit::Centimeter => "cm",
                Unit::Day => "d",
                Unit::Degree => "°",
                Unit::Fahrenheit => "°F",
                Unit::FluidOunce => "fl oz",
                Unit::Foot => "′",
                Unit::Gallon => "gal",
                Unit::Gigabit => "Gb",
                Unit::Gigabyte => "GB",
                Unit::Gram => "g",
                Unit::Hectare => "ha",
                Unit::Hour => "h",
                Unit::Inch => "″",
                Unit::Kilobit => "kb",
                Unit::Kilobyte => "kB",
                Unit::Kilogram => "kg",
                Unit::Kilometer => "km",
                Unit::Liter => "L",
                Unit::Megabit => "Mb",
                Unit::Megabyte => "MB",
                Unit::Meter => "m",
                Unit::Microsecond => "μs",
                Unit::Mile => "mi",
                Unit::MileScandinavian => "smi",
                Unit::Milliliter => "mL",
                Unit::Millimeter => "mm",
                Unit::Millisecond => "ms",
                Unit::Minute => "m",
                Unit::Month => "m",
                Unit::Nanosecond => "ns",
                Unit::Ounce => "oz",
                Unit::Percent => "%",
                Unit::Petabyte => "PB",
                Unit::Pound => "#",
                Unit::Second => "s",
                Unit::Stone => "st",
                Unit::Terabit => "Tb",
                Unit::Terabyte => "TB",
                Unit::Week => "w",
                Unit::Yard => "yd",
                Unit::Year => "y",
                Unit::CompoundPer { .. } => unit.as_str(),
            },
        ),
    };
    NumberUnitPattern {
        prefix: String::new(),
        prefix_separator: String::new(),
        suffix_separator: separator.into(),
        suffix: suffix.into(),
        hides_number: false,
    }
}

pub(super) const fn contains(values: &[&str], value: &str) -> bool {
    let mut index = 0;
    while index < values.len() {
        if str_eq(values[index], value) {
            return true;
        }
        index += 1;
    }
    false
}

pub(super) const fn str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

pub(super) fn canonical_time_zone(identifier: &str) -> &str {
    match identifier {
        "Etc/GMT" | "Etc/GMT0" | "Etc/UTC" | "GMT" | "GMT0" => "UTC",
        _ => identifier,
    }
}
