// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Caucasian raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, localized_generic_compound_unit_label,
    number_unit_pattern_from_placeholder, number_unit_pattern_label,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

/// Returns pinned Georgian CLDR records for every simple category unavailable
/// through ICU4X's typed unit markers.
pub(crate) fn cldr_georgian_additional_unit_pattern(
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
        != Some("ka")
    {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Bit => "{0} ბიტი",
            Unit::Byte => "{0} ბაიტი",
            Unit::Celsius => "{0} გრადუსი ცელსიუსით",
            Unit::Degree => "{0} გრადუსი",
            Unit::Fahrenheit => "{0} გრადუსი ფარენჰეიტით",
            Unit::Gigabit => "{0} გიგაბიტი",
            Unit::Gigabyte => "{0} გიგაბაიტი",
            Unit::Kilobit => "{0} კილობიტი",
            Unit::Kilobyte => "{0} კილობაიტი",
            Unit::Megabit => "{0} მეგაბიტი",
            Unit::Megabyte => "{0} მეგაბაიტი",
            Unit::Percent => "{0} პროცენტი",
            Unit::Petabyte => "{0} პეტაბაიტი",
            Unit::Terabit => "{0} ტერაბიტი",
            Unit::Terabyte => "{0} ტერაბაიტი",
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Bit => "{0} ბიტი",
            Unit::Byte => "{0} ბაიტი",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} გბიტი",
            Unit::Gigabyte => "{0} გიგაბაიტი",
            Unit::Kilobit => "{0} კბიტი",
            Unit::Kilobyte => "{0} კბაიტი",
            Unit::Megabit => "{0} მბიტი",
            Unit::Megabyte => "{0} მეგაბაიტი",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} პბაიტი",
            Unit::Terabit => "{0} ტბიტი",
            Unit::Terabyte => "{0} ტბაიტი",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Bit => "{0} ბიტი",
            Unit::Byte => "{0} ბაიტი",
            Unit::Celsius => "{0}°C",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°F",
            Unit::Gigabit => "{0} გბიტი",
            Unit::Gigabyte => "{0} გბაიტი",
            Unit::Kilobit => "{0} კბიტი",
            Unit::Kilobyte => "{0} კბაიტი",
            Unit::Megabit => "{0} მბიტი",
            Unit::Megabyte => "{0} მეგაბაიტი",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} პბაიტი",
            Unit::Terabit => "{0} ტბიტი",
            Unit::Terabyte => "{0} ტბაიტი",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Georgian denominator-specific pinned CLDR `perUnitPattern`
/// records.
pub(crate) fn cldr_georgian_per_unit_pattern(
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
        != Some("ka")
    {
        return None;
    }
    const LONG: [&str; 18] = [
        "{0} სანტიმეტრში",
        "{0} დღეში",
        "{0} ფუტში",
        "{0} გალონში",
        "{0} გრამში",
        "{0} საათში",
        "{0} დუიმში",
        "{0} კილოგრამში",
        "{0} კილომეტრში",
        "{0} ლიტრში",
        "{0} მეტრში",
        "{0} წუთში",
        "{0} თვეში",
        "{0} უნციაში",
        "{0} ფუნტში",
        "{0} წამში",
        "{0} კვირაში",
        "{0} წელში",
    ];
    const SHORT: [&str; 18] = [
        "{0}/სმ",
        "{0}/დღე",
        "{0}/ფტ",
        "{0}/გალონი",
        "{0}/გ",
        "{0}/სთ",
        "{0}/დუიმი",
        "{0}/კგ",
        "{0}/კმ",
        "{0}/ლ",
        "{0}/მ",
        "{0}/წთ",
        "{0}/თვე",
        "{0}/უნც",
        "{0}/ფნტ",
        "{0}/წმ",
        "{0}/კვრ",
        "{0}/წ",
    ];
    const NARROW: [&str; 18] = [
        "{0}/სმ",
        "{0}/დღე",
        "{0}/ფტ",
        "{0}/გალონი",
        "{0}/გ",
        "{0}/სთ",
        "{0}/დუიმი",
        "{0}/კგ",
        "{0}/კმ",
        "{0}/ლ",
        "{0}/მ",
        "{0}/წთ",
        "{0}/თ.",
        "{0}/უნც",
        "{0}/ფნტ",
        "{0}/წმ",
        "{0}/კვრ",
        "{0}/წ",
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

/// Composes Georgian generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_georgian_generic_compound_unit_pattern(
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
        != Some("ka")
    {
        return None;
    }
    let denominator_unit = denominator;
    let numerator_raw = cldr_georgian_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let denominator_raw = cldr_georgian_additional_unit_pattern(
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
            Display::Long => "{0} {1}-ში",
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
