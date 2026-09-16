// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Raw CLDR unit-pattern families that fill gaps in ICU4X typed markers.
//!
//! Keep each growing locale family in a focused child module. The provider
//! entry point remains in `locale_data`, while these modules own only raw
//! records and their generic `-per-` composition.

use super::{
    localized_generic_compound_unit_label, number_unit_pattern_from_placeholder,
    number_unit_pattern_label, NumberGenericCompoundUnitPattern, NumberUnitPattern,
};

mod full_cldr;
mod full_cldr_compound;

/// Composes a localized generic compound from the complete numerator pattern.
///
/// A CLDR simple-unit pattern can put the number after the unit name. Passing
/// only its label into a `perUnitPattern` would move that number outside the
/// localized connector. Keep a private placeholder through the entire CLDR
/// composition, then split the final pattern at the observable number seam.
pub(super) fn compose_generic_compound_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    numerator_pattern: &NumberUnitPattern,
    denominator_pattern: &NumberUnitPattern,
    generic_per: &str,
) -> Option<NumberGenericCompoundUnitPattern> {
    let denominator_label = number_unit_pattern_label(denominator_pattern);
    compose_generic_compound_unit_pattern_with_label(
        locale,
        denominator,
        display,
        numerator_pattern,
        &denominator_label,
        generic_per,
    )
}

/// Composes a localized generic compound with an explicit CLDR denominator
/// display name.
///
/// Most simple patterns expose a label by removing the number placeholder.
/// Number-after-label patterns may include a grammatical particle adjacent to
/// that placeholder, however, so their CLDR `displayName` is the only correct
/// value for the `{1}` slot of a generic compound pattern.
pub(super) fn compose_generic_compound_unit_pattern_with_label(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
    numerator_pattern: &NumberUnitPattern,
    denominator_label: &str,
    generic_per: &str,
) -> Option<NumberGenericCompoundUnitPattern> {
    let rendered_numerator = format!(
        "{}{}\u{fdd0}{}{}",
        numerator_pattern.prefix,
        numerator_pattern.prefix_separator,
        numerator_pattern.suffix_separator,
        numerator_pattern.suffix,
    );
    let rendered = localized_generic_compound_unit_label(
        locale,
        denominator,
        display,
        &rendered_numerator,
        denominator_label,
        generic_per,
    )?;
    let pattern = number_unit_pattern_from_placeholder(&rendered)?;
    if pattern.hides_number {
        return None;
    }
    Some(NumberGenericCompoundUnitPattern {
        prefix: pattern.prefix,
        prefix_separator: pattern.prefix_separator,
        suffix_separator: pattern.suffix_separator,
        suffix: pattern.suffix,
    })
}

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
        cldr_amharic_additional_unit_pattern,
        cldr_khmer_additional_unit_pattern,
        cldr_indonesian_additional_unit_pattern,
        cldr_malay_additional_unit_pattern,
        cldr_jawi_malay_additional_unit_pattern,
        cldr_filipino_additional_unit_pattern,
        cldr_javanese_additional_unit_pattern,
        cldr_persian_additional_unit_pattern,
        cldr_swedish_additional_unit_pattern,
        cldr_danish_additional_unit_pattern,
        cldr_norwegian_bokmal_additional_unit_pattern,
        cldr_norwegian_nynorsk_additional_unit_pattern,
        cldr_hungarian_additional_unit_pattern,
        cldr_finnish_additional_unit_pattern,
        cldr_estonian_additional_unit_pattern,
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
        cldr_telugu_additional_unit_pattern,
        cldr_hindi_additional_unit_pattern,
        cldr_bengali_additional_unit_pattern,
        cldr_urdu_additional_unit_pattern,
        cldr_latin_hindi_additional_unit_pattern,
        cldr_belarusian_additional_unit_pattern,
        cldr_georgian_additional_unit_pattern,
        cldr_kabuverdianu_additional_unit_pattern,
        cldr_gujarati_additional_unit_pattern,
        cldr_tamil_additional_unit_pattern,
        cldr_swahili_additional_unit_pattern,
        cldr_lithuanian_additional_unit_pattern,
        cldr_latvian_additional_unit_pattern,
        cldr_malayalam_additional_unit_pattern,
        cldr_bosnian_additional_unit_pattern,
        cldr_cantonese_additional_unit_pattern,
        cldr_croatian_additional_unit_pattern,
        cldr_serbian_additional_unit_pattern,
        cldr_macedonian_additional_unit_pattern,
        cldr_slovenian_additional_unit_pattern,
        cldr_burmese_additional_unit_pattern,
        cldr_tongan_additional_unit_pattern,
        cldr_armenian_additional_unit_pattern,
    ] {
        if let Some(pattern) = resolver(locale, unit, display, plural) {
            return Some(pattern);
        }
    }
    full_cldr::cldr_full_untyped_unit_pattern(locale, unit, display, plural)
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
        cldr_amharic_generic_compound_unit_pattern,
        cldr_khmer_generic_compound_unit_pattern,
        cldr_indonesian_generic_compound_unit_pattern,
        cldr_malay_generic_compound_unit_pattern,
        cldr_jawi_malay_generic_compound_unit_pattern,
        cldr_filipino_generic_compound_unit_pattern,
        cldr_javanese_generic_compound_unit_pattern,
        cldr_persian_generic_compound_unit_pattern,
        cldr_swedish_generic_compound_unit_pattern,
        cldr_danish_generic_compound_unit_pattern,
        cldr_norwegian_bokmal_generic_compound_unit_pattern,
        cldr_norwegian_nynorsk_generic_compound_unit_pattern,
        cldr_hungarian_generic_compound_unit_pattern,
        cldr_finnish_generic_compound_unit_pattern,
        cldr_estonian_generic_compound_unit_pattern,
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
        cldr_telugu_generic_compound_unit_pattern,
        cldr_hindi_generic_compound_unit_pattern,
        cldr_bengali_generic_compound_unit_pattern,
        cldr_urdu_generic_compound_unit_pattern,
        cldr_latin_hindi_generic_compound_unit_pattern,
        cldr_belarusian_generic_compound_unit_pattern,
        cldr_georgian_generic_compound_unit_pattern,
        cldr_kabuverdianu_generic_compound_unit_pattern,
        cldr_gujarati_generic_compound_unit_pattern,
        cldr_tamil_generic_compound_unit_pattern,
        cldr_swahili_generic_compound_unit_pattern,
        cldr_lithuanian_generic_compound_unit_pattern,
        cldr_latvian_generic_compound_unit_pattern,
        cldr_malayalam_generic_compound_unit_pattern,
        cldr_bosnian_generic_compound_unit_pattern,
        cldr_cantonese_generic_compound_unit_pattern,
        cldr_croatian_generic_compound_unit_pattern,
        cldr_serbian_generic_compound_unit_pattern,
        cldr_macedonian_generic_compound_unit_pattern,
        cldr_slovenian_generic_compound_unit_pattern,
        cldr_burmese_generic_compound_unit_pattern,
        cldr_tongan_generic_compound_unit_pattern,
        cldr_armenian_generic_compound_unit_pattern,
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

mod afroasiatic;
mod armenian;
mod austroasiatic;
mod austronesian;
mod baltic;
mod bosnian;
mod cantonese;
mod caucasian;
mod east_slavic;
mod greek;
mod gujarati;
mod indic;
mod indo_aryan;
mod iranian;
mod kabuverdianu;
mod malayalam;
mod niger_congo;
mod north_germanic;
mod per;
mod polynesian;
mod romance;
mod semitic;
mod sino_tibetan;
mod slavic;
mod south_slavic;
mod tai;
mod telugu;
mod turkic;
mod uralic;
mod vietic;
mod west_slavic;

pub(super) use afroasiatic::{
    cldr_amharic_additional_unit_pattern, cldr_amharic_generic_compound_unit_pattern,
};
pub(super) use armenian::{
    cldr_armenian_additional_unit_pattern, cldr_armenian_generic_compound_unit_pattern,
};
pub(super) use austroasiatic::{
    cldr_khmer_additional_unit_pattern, cldr_khmer_generic_compound_unit_pattern,
};
pub(super) use austronesian::{
    cldr_filipino_additional_unit_pattern, cldr_filipino_generic_compound_unit_pattern,
    cldr_indonesian_additional_unit_pattern, cldr_indonesian_generic_compound_unit_pattern,
    cldr_javanese_additional_unit_pattern, cldr_javanese_generic_compound_unit_pattern,
    cldr_jawi_malay_additional_unit_pattern, cldr_jawi_malay_generic_compound_unit_pattern,
    cldr_malay_additional_unit_pattern, cldr_malay_generic_compound_unit_pattern,
};
pub(super) use baltic::{
    cldr_latvian_additional_unit_pattern, cldr_latvian_generic_compound_unit_pattern,
    cldr_lithuanian_additional_unit_pattern, cldr_lithuanian_generic_compound_unit_pattern,
};
pub(super) use bosnian::{
    cldr_bosnian_additional_unit_pattern, cldr_bosnian_generic_compound_unit_pattern,
};
pub(super) use cantonese::{
    cldr_cantonese_additional_unit_pattern, cldr_cantonese_generic_compound_unit_pattern,
};
pub(super) use caucasian::{
    cldr_georgian_additional_unit_pattern, cldr_georgian_generic_compound_unit_pattern,
};
pub(super) use east_slavic::{
    cldr_belarusian_additional_unit_pattern, cldr_belarusian_generic_compound_unit_pattern,
};
pub(super) use full_cldr_compound::{
    cldr_full_generic_compound_hides_number, cldr_full_generic_compound_unit_pattern,
    has_cldr_full_generic_compound_data,
};
pub(super) use greek::{
    cldr_greek_additional_unit_pattern, cldr_greek_generic_compound_unit_pattern,
};
pub(super) use gujarati::{
    cldr_gujarati_additional_unit_pattern, cldr_gujarati_generic_compound_unit_pattern,
};
pub(super) use indic::{
    cldr_bengali_additional_unit_pattern, cldr_bengali_generic_compound_unit_pattern,
    cldr_hindi_additional_unit_pattern, cldr_hindi_generic_compound_unit_pattern,
    cldr_tamil_additional_unit_pattern, cldr_tamil_generic_compound_unit_pattern,
};
pub(super) use indo_aryan::{
    cldr_latin_hindi_additional_unit_pattern, cldr_latin_hindi_generic_compound_unit_pattern,
    cldr_urdu_additional_unit_pattern, cldr_urdu_generic_compound_unit_pattern,
};
pub(super) use iranian::{
    cldr_persian_additional_unit_pattern, cldr_persian_generic_compound_unit_pattern,
};
pub(super) use kabuverdianu::{
    cldr_kabuverdianu_additional_unit_pattern, cldr_kabuverdianu_generic_compound_unit_pattern,
};
pub(super) use malayalam::{
    cldr_malayalam_additional_unit_pattern, cldr_malayalam_generic_compound_unit_pattern,
};
pub(super) use niger_congo::{
    cldr_swahili_additional_unit_pattern, cldr_swahili_generic_compound_unit_pattern,
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
pub(super) use polynesian::{
    cldr_tongan_additional_unit_pattern, cldr_tongan_generic_compound_unit_pattern,
};
pub(super) use romance::{
    cldr_romanian_additional_unit_pattern, cldr_romanian_generic_compound_unit_pattern,
};
pub(super) use semitic::{
    cldr_hebrew_additional_unit_pattern, cldr_hebrew_generic_compound_unit_pattern,
};
pub(super) use sino_tibetan::{
    cldr_burmese_additional_unit_pattern, cldr_burmese_generic_compound_unit_pattern,
};
pub(super) use slavic::{
    cldr_bulgarian_additional_unit_pattern, cldr_bulgarian_generic_compound_unit_pattern,
    cldr_czech_additional_unit_pattern, cldr_czech_generic_compound_unit_pattern,
    cldr_polish_additional_unit_pattern, cldr_polish_generic_compound_unit_pattern,
    cldr_ukrainian_additional_unit_pattern, cldr_ukrainian_generic_compound_unit_pattern,
};
pub(super) use south_slavic::{
    cldr_croatian_additional_unit_pattern, cldr_croatian_generic_compound_unit_pattern,
    cldr_macedonian_additional_unit_pattern, cldr_macedonian_generic_compound_unit_pattern,
    cldr_serbian_additional_unit_pattern, cldr_serbian_generic_compound_unit_pattern,
    cldr_slovenian_additional_unit_pattern, cldr_slovenian_generic_compound_unit_pattern,
};
pub(super) use tai::{cldr_thai_additional_unit_pattern, cldr_thai_generic_compound_unit_pattern};
pub(super) use telugu::{
    cldr_telugu_additional_unit_pattern, cldr_telugu_generic_compound_unit_pattern,
};
pub(super) use turkic::{
    cldr_turkish_additional_unit_pattern, cldr_turkish_generic_compound_unit_pattern,
};
pub(super) use uralic::{
    cldr_estonian_additional_unit_pattern, cldr_estonian_generic_compound_unit_pattern,
    cldr_finnish_additional_unit_pattern, cldr_finnish_generic_compound_unit_pattern,
    cldr_hungarian_additional_unit_pattern, cldr_hungarian_generic_compound_unit_pattern,
};
pub(super) use vietic::{
    cldr_vietnamese_additional_unit_pattern, cldr_vietnamese_generic_compound_unit_pattern,
};
pub(super) use west_slavic::{
    cldr_slovak_additional_unit_pattern, cldr_slovak_generic_compound_unit_pattern,
};
