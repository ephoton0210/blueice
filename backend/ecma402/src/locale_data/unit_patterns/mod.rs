// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned CLDR unit-pattern sources for NumberFormat.
//!
//! ICU4X typed markers cover their own units. The focused simple-unit tables
//! below fill the remaining marker gaps, while one complete CLDR table owns
//! every generic compound. There is deliberately no second generic fallback.

use super::NumberUnitPattern;

mod full_cldr;
mod full_cldr_compound;

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

use afroasiatic::cldr_amharic_additional_unit_pattern;
use armenian::cldr_armenian_additional_unit_pattern;
use austroasiatic::cldr_khmer_additional_unit_pattern;
use austronesian::{
    cldr_filipino_additional_unit_pattern, cldr_indonesian_additional_unit_pattern,
    cldr_javanese_additional_unit_pattern, cldr_jawi_malay_additional_unit_pattern,
    cldr_malay_additional_unit_pattern,
};
use baltic::{cldr_latvian_additional_unit_pattern, cldr_lithuanian_additional_unit_pattern};
use bosnian::cldr_bosnian_additional_unit_pattern;
use cantonese::cldr_cantonese_additional_unit_pattern;
use caucasian::cldr_georgian_additional_unit_pattern;
use east_slavic::cldr_belarusian_additional_unit_pattern;
use greek::cldr_greek_additional_unit_pattern;
use gujarati::cldr_gujarati_additional_unit_pattern;
use indic::{
    cldr_bengali_additional_unit_pattern, cldr_hindi_additional_unit_pattern,
    cldr_tamil_additional_unit_pattern,
};
use indo_aryan::{cldr_latin_hindi_additional_unit_pattern, cldr_urdu_additional_unit_pattern};
use iranian::cldr_persian_additional_unit_pattern;
use kabuverdianu::cldr_kabuverdianu_additional_unit_pattern;
use malayalam::cldr_malayalam_additional_unit_pattern;
use niger_congo::cldr_swahili_additional_unit_pattern;
use north_germanic::{
    cldr_danish_additional_unit_pattern, cldr_norwegian_bokmal_additional_unit_pattern,
    cldr_norwegian_nynorsk_additional_unit_pattern, cldr_swedish_additional_unit_pattern,
};
use polynesian::cldr_tongan_additional_unit_pattern;
use romance::cldr_romanian_additional_unit_pattern;
use semitic::cldr_hebrew_additional_unit_pattern;
use sino_tibetan::cldr_burmese_additional_unit_pattern;
use slavic::{
    cldr_bulgarian_additional_unit_pattern, cldr_czech_additional_unit_pattern,
    cldr_polish_additional_unit_pattern, cldr_ukrainian_additional_unit_pattern,
};
use south_slavic::{
    cldr_croatian_additional_unit_pattern, cldr_macedonian_additional_unit_pattern,
    cldr_serbian_additional_unit_pattern, cldr_slovenian_additional_unit_pattern,
};
use tai::cldr_thai_additional_unit_pattern;
use telugu::cldr_telugu_additional_unit_pattern;
use turkic::cldr_turkish_additional_unit_pattern;
use uralic::{
    cldr_estonian_additional_unit_pattern, cldr_finnish_additional_unit_pattern,
    cldr_hungarian_additional_unit_pattern,
};
use vietic::cldr_vietnamese_additional_unit_pattern;
use west_slavic::cldr_slovak_additional_unit_pattern;

/// Routes every focused raw simple-unit family, then the complete raw CLDR
/// supplement for cells ICU4X does not type.
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

pub(super) use full_cldr_compound::{
    cldr_full_generic_compound_hides_number, cldr_full_generic_compound_unit_pattern,
    has_cldr_full_generic_compound_data,
};
