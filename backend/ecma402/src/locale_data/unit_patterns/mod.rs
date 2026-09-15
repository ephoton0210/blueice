// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Raw CLDR unit-pattern families that fill gaps in ICU4X typed markers.
//!
//! Keep each growing locale family in a focused child module. The provider
//! entry point remains in `locale_data`, while these modules own only raw
//! records and their generic `-per-` composition.

use super::{NumberGenericCompoundUnitPattern, NumberUnitPattern};

/// Routes every child-owned raw simple-unit family. New locales register here,
/// keeping the stable provider entry point free of an ever-growing language
/// dispatch chain.
pub(super) fn additional_unit_pattern(
    locale: &str,
    unit: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberUnitPattern> {
    for resolver in [
        cldr_indonesian_additional_unit_pattern,
        cldr_persian_additional_unit_pattern,
        cldr_swedish_additional_unit_pattern,
        cldr_danish_additional_unit_pattern,
        cldr_norwegian_bokmal_additional_unit_pattern,
        cldr_norwegian_nynorsk_additional_unit_pattern,
        cldr_hungarian_additional_unit_pattern,
        cldr_finnish_additional_unit_pattern,
        cldr_greek_additional_unit_pattern,
        cldr_polish_additional_unit_pattern,
        cldr_ukrainian_additional_unit_pattern,
        cldr_czech_additional_unit_pattern,
        cldr_bulgarian_additional_unit_pattern,
        cldr_slovak_additional_unit_pattern,
        cldr_romanian_additional_unit_pattern,
        cldr_thai_additional_unit_pattern,
        cldr_vietnamese_additional_unit_pattern,
        cldr_hebrew_additional_unit_pattern,
        cldr_turkish_additional_unit_pattern,
        cldr_hindi_additional_unit_pattern,
        cldr_bengali_additional_unit_pattern,
    ] {
        if let Some(pattern) = resolver(locale, unit, display, plural) {
            return Some(pattern);
        }
    }
    None
}

/// Routes every child-owned raw generic-compound family. The public provider
/// retains its English fallback after this returns `None`.
pub(super) fn generic_compound_unit_pattern(
    locale: &str,
    numerator: crate::NumberFormatUnit,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    plural: crate::PluralCategory,
) -> Option<NumberGenericCompoundUnitPattern> {
    for resolver in [
        cldr_indonesian_generic_compound_unit_pattern,
        cldr_persian_generic_compound_unit_pattern,
        cldr_swedish_generic_compound_unit_pattern,
        cldr_danish_generic_compound_unit_pattern,
        cldr_norwegian_bokmal_generic_compound_unit_pattern,
        cldr_norwegian_nynorsk_generic_compound_unit_pattern,
        cldr_hungarian_generic_compound_unit_pattern,
        cldr_finnish_generic_compound_unit_pattern,
        cldr_greek_generic_compound_unit_pattern,
        cldr_polish_generic_compound_unit_pattern,
        cldr_ukrainian_generic_compound_unit_pattern,
        cldr_czech_generic_compound_unit_pattern,
        cldr_bulgarian_generic_compound_unit_pattern,
        cldr_slovak_generic_compound_unit_pattern,
        cldr_romanian_generic_compound_unit_pattern,
        cldr_thai_generic_compound_unit_pattern,
        cldr_vietnamese_generic_compound_unit_pattern,
        cldr_hebrew_generic_compound_unit_pattern,
        cldr_turkish_generic_compound_unit_pattern,
        cldr_hindi_generic_compound_unit_pattern,
        cldr_bengali_generic_compound_unit_pattern,
    ] {
        if let Some(pattern) = resolver(locale, numerator, denominator, display, plural) {
            return Some(pattern);
        }
    }
    None
}

/// Maps the ECMA-402 denominators for which the pinned CLDR input exposes
/// `perUnitPattern` data to their compact table index.
pub(super) fn per_unit_denominator_index(unit: crate::NumberFormatUnit) -> Option<usize> {
    use crate::NumberFormatUnit as Unit;

    Some(match unit {
        Unit::Centimeter => 0,
        Unit::Day => 1,
        Unit::Foot => 2,
        Unit::Gallon => 3,
        Unit::Gram => 4,
        Unit::Hour => 5,
        Unit::Inch => 6,
        Unit::Kilogram => 7,
        Unit::Kilometer => 8,
        Unit::Liter => 9,
        Unit::Meter => 10,
        Unit::Minute => 11,
        Unit::Month => 12,
        Unit::Ounce => 13,
        Unit::Pound => 14,
        Unit::Second => 15,
        Unit::Week => 16,
        Unit::Year => 17,
        _ => return None,
    })
}

mod austronesian;
mod greek;
mod indic;
mod iranian;
mod north_germanic;
mod per;
mod romance;
mod semitic;
mod slavic;
mod tai;
mod turkic;
mod uralic;
mod vietic;
mod west_slavic;

pub(super) use austronesian::{
    cldr_indonesian_additional_unit_pattern, cldr_indonesian_generic_compound_unit_pattern,
};
pub(super) use greek::{
    cldr_greek_additional_unit_pattern, cldr_greek_generic_compound_unit_pattern,
};
pub(super) use indic::{
    cldr_bengali_additional_unit_pattern, cldr_bengali_generic_compound_unit_pattern,
    cldr_hindi_additional_unit_pattern, cldr_hindi_generic_compound_unit_pattern,
};
pub(super) use iranian::{
    cldr_persian_additional_unit_pattern, cldr_persian_generic_compound_unit_pattern,
};
pub(super) use north_germanic::{
    cldr_danish_additional_unit_pattern, cldr_danish_generic_compound_unit_pattern,
    cldr_norwegian_bokmal_additional_unit_pattern,
    cldr_norwegian_bokmal_generic_compound_unit_pattern,
    cldr_norwegian_nynorsk_additional_unit_pattern,
    cldr_norwegian_nynorsk_generic_compound_unit_pattern, cldr_swedish_additional_unit_pattern,
    cldr_swedish_generic_compound_unit_pattern,
};
pub(super) use per::expanded_per_unit_pattern;
pub(super) use romance::{
    cldr_romanian_additional_unit_pattern, cldr_romanian_generic_compound_unit_pattern,
};
pub(super) use semitic::{
    cldr_hebrew_additional_unit_pattern, cldr_hebrew_generic_compound_unit_pattern,
};
pub(super) use slavic::{
    cldr_bulgarian_additional_unit_pattern, cldr_bulgarian_generic_compound_unit_pattern,
    cldr_czech_additional_unit_pattern, cldr_czech_generic_compound_unit_pattern,
    cldr_polish_additional_unit_pattern, cldr_polish_generic_compound_unit_pattern,
    cldr_ukrainian_additional_unit_pattern, cldr_ukrainian_generic_compound_unit_pattern,
};
pub(super) use tai::{cldr_thai_additional_unit_pattern, cldr_thai_generic_compound_unit_pattern};
pub(super) use turkic::{
    cldr_turkish_additional_unit_pattern, cldr_turkish_generic_compound_unit_pattern,
};
pub(super) use uralic::{
    cldr_finnish_additional_unit_pattern, cldr_finnish_generic_compound_unit_pattern,
    cldr_hungarian_additional_unit_pattern, cldr_hungarian_generic_compound_unit_pattern,
};
pub(super) use vietic::{
    cldr_vietnamese_additional_unit_pattern, cldr_vietnamese_generic_compound_unit_pattern,
};
pub(super) use west_slavic::{
    cldr_slovak_additional_unit_pattern, cldr_slovak_generic_compound_unit_pattern,
};
