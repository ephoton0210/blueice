// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Telugu raw CLDR unit-pattern families.

use super::super::{
    experimental_number_unit_pattern, number_unit_pattern_from_placeholder,
    NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

fn is_telugu(locale: &str) -> bool {
    locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()
        == Some("te")
}

fn telugu_cardinal_pattern(
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

/// Returns pinned Telugu CLDR records for every ECMA-402 simple-unit category
/// that ICU4X's typed markers do not cover.
pub(crate) fn cldr_telugu_additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if !is_telugu(locale) {
        return None;
    }
    let raw = match display {
        Display::Long => match unit {
            Unit::Acre => telugu_cardinal_pattern("{0} ఎకరం", "{0} ఎకరాలు", plural),
            Unit::Bit => telugu_cardinal_pattern("{0} బిట్", "{0} బిట్‌లు", plural),
            Unit::Byte => telugu_cardinal_pattern("{0} బైట్", "{0} బైట్‌లు", plural),
            Unit::Celsius => telugu_cardinal_pattern("{0} డిగ్రీ సెల్సియస్", "{0} డిగ్రీల సెల్సియస్", plural),
            Unit::Degree => telugu_cardinal_pattern("{0} డిగ్రీ", "{0} డిగ్రీలు", plural),
            Unit::Fahrenheit => "{0} డిగ్రీల ఫారెన్‌హీట్",
            Unit::Gigabit => telugu_cardinal_pattern("{0} గిగాబిట్", "{0} గిగాబిట్లు", plural),
            Unit::Gigabyte => telugu_cardinal_pattern("{0} గిగాబైట్", "{0} గిగాబైట్లు", plural),
            Unit::Kilobit => telugu_cardinal_pattern("{0} కిలోబిట్", "{0} కిలోబిట్లు", plural),
            Unit::Kilobyte => telugu_cardinal_pattern("{0} కిలోబైట్", "{0} కిలోబైట్లు", plural),
            Unit::Megabit => telugu_cardinal_pattern("{0} మెగాబిట్", "{0} మెగాబిట్లు", plural),
            Unit::Megabyte => telugu_cardinal_pattern("{0} మెగాబైట్", "{0} మెగాబైట్లు", plural),
            Unit::Percent => "{0} శాతం",
            Unit::Petabyte => telugu_cardinal_pattern("{0} పెటాబైట్", "{0} పెటాబైట్లు", plural),
            Unit::Second => telugu_cardinal_pattern("{0} సెకను", "{0} సెకన్లు", plural),
            Unit::Terabit => telugu_cardinal_pattern("{0} టెరాబిట్", "{0} టెరాబిట్లు", plural),
            Unit::Terabyte => telugu_cardinal_pattern("{0} టెరాబైట్", "{0} టెరాబైట్లు", plural),
            _ => return None,
        },
        Display::Short => match unit {
            Unit::Acre => "{0} ఎక.",
            Unit::Bit => "{0} బి",
            Unit::Byte => "{0} బై",
            Unit::Celsius => "{0}°సెల్సి",
            Unit::Degree => "{0} డి.",
            Unit::Fahrenheit => "{0}°ఫా",
            Unit::Gigabit => telugu_cardinal_pattern("{0} గి.బిట్", "{0} గి.బిట్లు", plural),
            Unit::Gigabyte => "{0} జీబీ",
            Unit::Kilobit => telugu_cardinal_pattern("{0} కి.బిట్", "{0} కి.బిట్లు", plural),
            Unit::Kilobyte => "{0} కేబీ",
            Unit::Megabit => telugu_cardinal_pattern("{0} మె.బిట్", "{0} మె.బిట్లు", plural),
            Unit::Megabyte => "{0} ఎమ్‌బి",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} పీబీ",
            Unit::Second => telugu_cardinal_pattern("{0} సె.", "{0} సెక.", plural),
            Unit::Terabit => telugu_cardinal_pattern("{0} టె.బిట్", "{0} టె.బిట్లు", plural),
            Unit::Terabyte => "{0} టీబీ",
            _ => return None,
        },
        Display::Narrow => match unit {
            Unit::Acre => "{0} ఎక.",
            Unit::Bit => "{0} బి",
            Unit::Byte => "{0} బై",
            Unit::Celsius => "{0}°సెల్సి",
            Unit::Degree => "{0}°",
            Unit::Fahrenheit => "{0}°ఫా",
            Unit::Gigabit => telugu_cardinal_pattern("{0} గి.బిట్", "{0} గి.బిట్లు", plural),
            Unit::Gigabyte => "{0} జీబీ",
            Unit::Kilobit => telugu_cardinal_pattern("{0} కి.బిట్", "{0} కి.బిట్లు", plural),
            Unit::Kilobyte => "{0} కేబీ",
            Unit::Megabit => telugu_cardinal_pattern("{0} మె.బిట్", "{0}మె.బి.", plural),
            Unit::Megabyte => "{0} ఎమ్‌బి",
            Unit::Percent => "{0}%",
            Unit::Petabyte => "{0} పీబీ",
            Unit::Second => "{0}సె",
            Unit::Terabit => telugu_cardinal_pattern("{0} టె.బిట్", "{0}టె.బిట్లు", plural),
            Unit::Terabyte => "{0} టీబీ",
            _ => return None,
        },
    };
    number_unit_pattern_from_placeholder(&raw.replace("{0}", "\u{fdd0}"))
}

/// Returns Telugu denominator-specific pinned CLDR `perUnitPattern` records.
pub(crate) fn cldr_telugu_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::NumberUnitDisplay as Display;

    if !is_telugu(locale) {
        return None;
    }
    const LONG: [&str; 18] = [
        "సెంటీమీటరుకు {0}",
        "రోజుకు {0}",
        "అడుగుకి {0}",
        "గ్యాలనుకు {0}",
        "గ్రాముకు {0}",
        "{0}/గంట",
        "అంగుళానికి {0}",
        "కిలోగ్రాముకు {0}",
        "కిలోమీటరుకు {0}",
        "లీటరుకు {0}",
        "మీటరుకు {0}",
        "నిమిషానికి {0}",
        "నెలకు {0}",
        "ఔన్సుకు {0}",
        "పౌండుకు {0}",
        "సెకనుకు {0}",
        "వారానికి {0}",
        "సంవత్సరానికి {0}",
    ];
    const SHORT: [&str; 18] = [
        "{0}/సెం.మీ.",
        "{0}/రో",
        "{0}/అ.",
        "{0}/గ్యా.",
        "{0}/గ్రా.",
        "{0}/గం",
        "{0}/అం.",
        "{0}/కి.గ్రా.",
        "{0}/కి.మీ.",
        "{0}/లీ.",
        "{0}/మీ.",
        "{0}/నిమి.",
        "{0}/నె.",
        "{0}/ ఔ.",
        "{0}/పౌ.",
        "{0}/సె",
        "{0}/వా.",
        "{0}/సం.",
    ];
    const NARROW: [&str; 18] = SHORT;
    let patterns = match display {
        Display::Long => &LONG,
        Display::Short => &SHORT,
        Display::Narrow => &NARROW,
    };
    patterns
        .get(super::per_unit_denominator_index(denominator)?)
        .copied()
}

/// Composes Telugu generic compounds containing an ICU4X-untyped unit.
pub(crate) fn cldr_telugu_generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    use crate::NumberUnitDisplay as Display;

    if !is_telugu(locale) {
        return None;
    }
    let denominator_unit = denominator;
    let denominator_display = match display {
        Display::Long => Display::Long,
        Display::Short | Display::Narrow => Display::Narrow,
    };
    let numerator_raw = cldr_telugu_additional_unit_pattern(locale, numerator, display, plural);
    let denominator_raw = cldr_telugu_additional_unit_pattern(
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
    super::compose_generic_compound_unit_pattern(
        locale,
        denominator_unit,
        display,
        &numerator,
        &denominator,
        "{0}/{1}",
    )
}
