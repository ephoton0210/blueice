// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Denominator-specific CLDR `perUnitPattern` records for expanding families.

use super::indic::uses_hindi_devanagari_unit_data;

/// Resolves denominator-specific records. Newer locale tables delegate to
/// their linguistic modules; this module retains only the shared legacy
/// families until each is migrated.
pub(crate) fn expanded_per_unit_pattern(
    locale: &str,
    denominator: crate::NumberFormatUnit,
    display: crate::NumberUnitDisplay,
) -> Option<&'static str> {
    use crate::{NumberFormatUnit as Unit, NumberUnitDisplay as Display};

    if let Some(pattern) =
        super::afroasiatic::cldr_amharic_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::austroasiatic::cldr_khmer_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::austronesian::cldr_indonesian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::austronesian::cldr_malay_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::austronesian::cldr_jawi_malay_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::austronesian::cldr_filipino_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::austronesian::cldr_javanese_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::malayalam::cldr_malayalam_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::iranian::cldr_persian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::gujarati::cldr_gujarati_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) = super::indic::cldr_bengali_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) = super::indic::cldr_tamil_per_unit_pattern(locale, denominator, display) {
        return Some(pattern);
    }
    if let Some(pattern) = super::telugu::cldr_telugu_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::indo_aryan::cldr_urdu_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::indo_aryan::cldr_latin_hindi_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::kabuverdianu::cldr_kabuverdianu_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::east_slavic::cldr_belarusian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::armenian::cldr_armenian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::niger_congo::cldr_swahili_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::baltic::cldr_lithuanian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::baltic::cldr_latvian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::bosnian::cldr_bosnian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::cantonese::cldr_cantonese_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::south_slavic::cldr_croatian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::south_slavic::cldr_serbian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::south_slavic::cldr_macedonian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::south_slavic::cldr_slovenian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::sino_tibetan::cldr_burmese_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::polynesian::cldr_tongan_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::caucasian::cldr_georgian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::north_germanic::cldr_swedish_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::north_germanic::cldr_danish_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::north_germanic::cldr_norwegian_bokmal_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::north_germanic::cldr_norwegian_nynorsk_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::uralic::cldr_hungarian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::uralic::cldr_finnish_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::uralic::cldr_estonian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::romance::cldr_romanian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::slavic::cldr_bulgarian_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }
    if let Some(pattern) = super::tai::cldr_thai_per_unit_pattern(locale, denominator, display) {
        return Some(pattern);
    }
    if let Some(pattern) =
        super::vietic::cldr_vietnamese_per_unit_pattern(locale, denominator, display)
    {
        return Some(pattern);
    }

    const DENOMINATORS: [Unit; 18] = [
        Unit::Centimeter,
        Unit::Day,
        Unit::Foot,
        Unit::Gallon,
        Unit::Gram,
        Unit::Hour,
        Unit::Inch,
        Unit::Kilogram,
        Unit::Kilometer,
        Unit::Liter,
        Unit::Meter,
        Unit::Minute,
        Unit::Month,
        Unit::Ounce,
        Unit::Pound,
        Unit::Second,
        Unit::Week,
        Unit::Year,
    ];
    const NL_LONG: [&str; 18] = [
        "{0} per centimeter",
        "{0} per dag",
        "{0} per voet",
        "{0} per gallon",
        "{0} per gram",
        "{0} per uur",
        "{0} per inch",
        "{0} per kilogram",
        "{0} per kilometer",
        "{0} per liter",
        "{0} per meter",
        "{0} per minuut",
        "{0} per maand",
        "{0} per ounce",
        "{0} per pound",
        "{0} per seconde",
        "{0} per week",
        "{0} per jaar",
    ];
    const NL_SHORT: [&str; 18] = [
        "{0}/cm", "{0}/dag", "{0}/ft", "{0}/gal", "{0}/g", "{0}/uur", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/mnd", "{0}/oz", "{0}/lb", "{0}/sec", "{0}/wk", "{0}/jr",
    ];
    const NL_NARROW: [&str; 18] = [
        "{0}/cm", "{0}/d", "{0}/ft", "{0}/gal", "{0}/g", "{0}/u", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/m", "{0}/m", "{0}/oz", "{0}/lb", "{0}/s", "{0}/w", "{0}/jr",
    ];
    const TR_LONG: [&str; 18] = [
        "{0}/santimetre",
        "{0}/gün",
        "{0}/fit",
        "{0}/galon",
        "{0}/gram",
        "{0}/saat",
        "{0}/inç",
        "{0}/kg",
        "{0}/kilometre",
        "{0}/litre",
        "{0}/metre",
        "{0}/dakika",
        "{0}/ay",
        "{0}/oz",
        "{0}/libre",
        "{0}/saniye",
        "{0}/hafta",
        "{0}/yıl",
    ];
    const TR_SHORT: [&str; 18] = [
        "{0}/cm", "{0}/gün", "{0}/ft", "{0}/gal", "{0}/g", "{0}/sa", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/dk.", "{0}/ay", "{0}/oz", "{0}/lb", "{0}/sn", "{0}/hf.", "{0}/y",
    ];
    const TR_NARROW: [&str; 18] = [
        "{0}/cm", "{0}/g", "{0}/ft", "{0}/gal", "{0}/g", "{0}/sa", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/dk.", "{0}/ay", "{0}/oz", "{0}/lb", "{0}/sn", "{0}/hf.", "{0}/y",
    ];
    const HI_LONG: [&str; 18] = [
        "{0}/सेंटीमीटर",
        "{0} प्रति दिन",
        "{0}/फ़ुट",
        "{0}/गैलन",
        "{0}/ग्राम",
        "{0} प्रति घंटा",
        "{0}/इंच",
        "{0} प्रति किलोग्राम",
        "{0}/किलोमीटर",
        "{0}/लीटर",
        "{0}/मीटर",
        "{0} प्रति मिनट",
        "{0} प्रति महीना",
        "{0}/औंस",
        "{0}/पौंड",
        "{0} प्रति सेकंड",
        "{0} प्रति सप्ताह",
        "{0} प्रति वर्ष",
    ];
    const HI_SHORT: [&str; 18] = [
        "{0}/सें॰मी॰",
        "{0}/दिन",
        "{0}/फ़ीट",
        "{0}/गैलन",
        "{0}/ग्रा॰",
        "{0}/घं॰",
        "{0}/इंच",
        "{0}/कि॰ग्रा॰",
        "{0}/कि॰मी॰",
        "{0}/ली॰",
        "{0}/मी",
        "{0}/मिनट",
        "{0}/माह",
        "{0}/औंस",
        "{0}/पौंड",
        "{0}/से॰",
        "{0}/सप्ताह",
        "{0}/वर्ष",
    ];
    const HI_NARROW: [&str; 18] = [
        "{0}/सेंमी",
        "{0}/दि",
        "{0}/फ़ीट",
        "{0}/गै",
        "{0}/ग्रा",
        "{0}/घं",
        "{0}/इंच",
        "{0}/किग्रा",
        "{0}/किमी",
        "{0}/ली",
        "{0}/मी",
        "{0}/मि",
        "{0}/माह",
        "{0}/औंस",
        "{0}/पौंड",
        "{0}/से",
        "{0}/स",
        "{0}/व",
    ];
    const EL_LONG: [&str; 18] = [
        "{0} ανά εκατοστό",
        "{0} ανά ημέρα",
        "{0} ανά πόδι",
        "{0} ανά γαλόνι",
        "{0} ανά γραμμάριο",
        "{0} ανά ώρα",
        "{0} ανά ίντσα",
        "{0} ανά χιλιόγραμμο",
        "{0} ανά χιλιόμετρο",
        "{0} ανά λίτρο",
        "{0} ανά μέτρο",
        "{0} ανά λεπτό",
        "{0} ανά μήνα",
        "{0} ανά ουγγιά",
        "{0} ανά λίβρα",
        "{0} ανά δευτερόλεπτο",
        "{0} ανά εβδομάδα",
        "{0} ανά έτος",
    ];
    const EL_SHORT: [&str; 18] = [
        "{0}/εκ.",
        "{0}/ημ.",
        "{0}/πδ",
        "{0}/γαλ.",
        "{0}/γρ.",
        "{0}/ώ.",
        "{0}/ίν.",
        "{0}/κιλό",
        "{0}/χλμ.",
        "{0}/λ.",
        "{0}/μ.",
        "{0}/λ.",
        "{0}/μ.",
        "{0}/oz",
        "{0}/λβ",
        "{0}/δευτ.",
        "{0}/εβδ.",
        "{0}/έτ.",
    ];
    const EL_NARROW: [&str; 18] = [
        "{0}/εκ.",
        "{0}/η",
        "{0}/πδ",
        "{0}/γαλ.",
        "{0}/γρ.",
        "{0}/ώ",
        "{0}/ίν.",
        "{0}/kg",
        "{0}/χλμ.",
        "{0}/λ.",
        "{0}/μ.",
        "{0}/λ",
        "{0}/μ",
        "{0}/oz",
        "{0}/λβ",
        "{0}/δ",
        "{0}/ε",
        "{0}/έ",
    ];
    const PL_LONG: [&str; 18] = [
        "{0} na centymetr",
        "{0} na dobę",
        "{0} na stopę",
        "{0} na galon amerykański",
        "{0} na gram",
        "{0} na godzinę",
        "{0} na cal",
        "{0} na kilogram",
        "{0} na kilometr",
        "{0} na litr",
        "{0} na metr",
        "{0} na minutę",
        "{0} na miesiąc",
        "{0} na uncję",
        "{0} na funt",
        "{0} na sekundę",
        "{0} na tydzień",
        "{0} na rok",
    ];
    const PL_SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/dobę",
        "{0}/ft",
        "{0}/gal am.",
        "{0}/g",
        "{0}/godz.",
        "{0}/cal",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/mies.",
        "{0}/oz",
        "{0}/funt",
        "{0}/s",
        "{0}/tydz.",
        "{0}/rok",
    ];
    const PL_NARROW: [&str; 18] = [
        "{0}/cm",
        "{0}/d.",
        "{0}/ft",
        "{0}/gal am.",
        "{0}/g",
        "{0}/h",
        "{0}/cal",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/m-c",
        "{0}/oz",
        "{0}/funt",
        "{0}/s",
        "{0}/t.",
        "{0}/rok",
    ];
    const HE_LONG: [&str; 18] = [
        "{0} לסנטימטר",
        "{0}/יום",
        "{0} לרגל",
        "{0}/גלון",
        "{0}/גרם",
        "{0} לשעה",
        "{0} לאינץ׳",
        "{0}/קילוגרם",
        "{0} לקילומטר",
        "{0}/ליטר",
        "{0} למטר",
        "{0}/דקה",
        "{0} לחודש",
        "{0}/אונקיה",
        "{0}/פאונד",
        "{0} לשניה",
        "{0}/שבוע",
        "{0} לשנה",
    ];
    const HE_SHORT: [&str; 18] = [
        "{0}/ס״מ",
        "{0}/יום",
        "{0} \u{200e}/ft",
        "{0}/גל׳",
        "{0}/גר׳",
        "{0}/שעה",
        "{0} \u{200e}/in",
        "{0}/ק״ג",
        "{0}/ק״מ",
        "{0}/ל׳",
        "{0}/מ׳",
        "{0}/ד׳",
        "{0}/חודש",
        "{0}/oz",
        "{0}/lb",
        "{0}/שנ׳",
        "{0}/שבוע",
        "{0}/שנה",
    ];
    const HE_NARROW: [&str; 18] = [
        "{0}/ס״מ",
        "{0}/יום",
        "{0} \u{200e}/ft",
        "{0}/גל׳",
        "{0}/גר׳",
        "{0}/שע׳",
        "{0} \u{200e}/in",
        "{0}/ק״ג",
        "{0}/ק״מ",
        "{0}/ל׳",
        "{0}/מ׳",
        "{0}/ד׳",
        "{0}/חודש",
        "{0}/oz",
        "{0}/lb",
        "{0}/שנ׳",
        "{0}/שב׳",
        "{0}/שנה",
    ];
    const UK_LONG: [&str; 18] = [
        "{0} на сантиметр",
        "{0} на день",
        "{0} на фут",
        "{0} на галон",
        "{0} на грам",
        "{0} на годину",
        "{0} на дюйм",
        "{0} на кілограм",
        "{0} на кілометр",
        "{0} на літр",
        "{0} на метр",
        "{0} на хвилину",
        "{0} на місяць",
        "{0} на унцію",
        "{0} на фунт",
        "{0} на секунду",
        "{0} на тиждень",
        "{0} на рік",
    ];
    const UK_SHORT: [&str; 18] = [
        "{0}/см",
        "{0}/дн",
        "{0}/фт",
        "{0}/гал",
        "{0}/г",
        "{0}/год",
        "{0}/дюйм",
        "{0}/кг",
        "{0}/км",
        "{0}/л",
        "{0}/м",
        "{0}/хв",
        "{0}/міс",
        "{0}/унц",
        "{0}/фунт",
        "{0}/с",
        "{0}/тиж",
        "{0}/р.",
    ];
    const UK_NARROW: [&str; 18] = [
        "{0}/см",
        "{0}/д",
        "{0}/фт",
        "{0}/гал",
        "{0}/г",
        "{0}/г",
        "{0}/″",
        "{0}/кг",
        "{0}/км",
        "{0}/л",
        "{0}/м",
        "{0}/х",
        "{0}/м",
        "{0}/ун",
        "{0}/фнт",
        "{0}/с",
        "{0}/т",
        "{0}/р",
    ];
    const CS_LONG: [&str; 18] = [
        "{0} na centimetr",
        "{0} za den",
        "{0} na stopu",
        "{0} na galon",
        "{0} na gram",
        "{0} za hodinu",
        "{0} na palec",
        "{0} na kilogram",
        "{0} na kilometr",
        "{0} na litr",
        "{0} na metr",
        "{0} za minutu",
        "{0} za měsíc",
        "{0} na unci",
        "{0} na libru",
        "{0} za sekundu",
        "{0} za týden",
        "{0} za rok",
    ];
    const CS_SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/den",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/h",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/měs.",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/týd.",
        "{0}/rok",
    ];
    const CS_NARROW: [&str; 18] = [
        "{0}/cm", "{0}/d.", "{0}/ft", "{0}/gal", "{0}/g", "{0}/h", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/m", "{0}/m.", "{0}/oz", "{0}/lb", "{0}/s", "{0}/t.", "{0}/r.",
    ];
    const SK_LONG: [&str; 18] = [
        "{0} na centimeter",
        "{0} za deň",
        "{0} na stopu",
        "{0} na galón",
        "{0} na gram",
        "{0} za hodinu",
        "{0} na palec",
        "{0} na kilogram",
        "{0} na kilometer",
        "{0} na liter",
        "{0} na meter",
        "{0} za minútu",
        "{0} za mesiac",
        "{0} na uncu",
        "{0} na libru",
        "{0} za sekundu",
        "{0} za týždeň",
        "{0} za rok",
    ];
    const SK_SHORT: [&str; 18] = [
        "{0}/cm",
        "{0}/deň",
        "{0}/ft",
        "{0}/gal",
        "{0}/g",
        "{0}/h",
        "{0}/in",
        "{0}/kg",
        "{0}/km",
        "{0}/l",
        "{0}/m",
        "{0}/min",
        "{0}/mes.",
        "{0}/oz",
        "{0}/lb",
        "{0}/s",
        "{0}/týž.",
        "{0}/r.",
    ];
    const SK_NARROW: [&str; 18] = [
        "{0}/cm", "{0}/d.", "{0}/ft", "{0}/gal", "{0}/g", "{0}/h", "{0}/in", "{0}/kg", "{0}/km",
        "{0}/l", "{0}/m", "{0}/min", "{0}/m.", "{0}/oz", "{0}/lb", "{0}/s", "{0}/t.", "{0}/r.",
    ];
    let language = locale
        .split_once("-u-")
        .map_or(locale, |(base, _)| base)
        .split('-')
        .next()?;
    let patterns: &[&str] = match (language, display) {
        ("nl", Display::Long) => &NL_LONG,
        ("nl", Display::Short) => &NL_SHORT,
        ("nl", Display::Narrow) => &NL_NARROW,
        ("tr", Display::Long) => &TR_LONG,
        ("tr", Display::Short) => &TR_SHORT,
        ("tr", Display::Narrow) => &TR_NARROW,
        ("hi", Display::Long) if uses_hindi_devanagari_unit_data(locale) => &HI_LONG,
        ("hi", Display::Short) if uses_hindi_devanagari_unit_data(locale) => &HI_SHORT,
        ("hi", Display::Narrow) if uses_hindi_devanagari_unit_data(locale) => &HI_NARROW,
        ("el", Display::Long) => &EL_LONG,
        ("el", Display::Short) => &EL_SHORT,
        ("el", Display::Narrow) => &EL_NARROW,
        ("pl", Display::Long) => &PL_LONG,
        ("pl", Display::Short) => &PL_SHORT,
        ("pl", Display::Narrow) => &PL_NARROW,
        ("he", Display::Long) => &HE_LONG,
        ("he", Display::Short) => &HE_SHORT,
        ("he", Display::Narrow) => &HE_NARROW,
        ("uk", Display::Long) => &UK_LONG,
        ("uk", Display::Short) => &UK_SHORT,
        ("uk", Display::Narrow) => &UK_NARROW,
        ("cs", Display::Long) => &CS_LONG,
        ("cs", Display::Short) => &CS_SHORT,
        ("cs", Display::Narrow) => &CS_NARROW,
        ("sk", Display::Long) => &SK_LONG,
        ("sk", Display::Short) => &SK_SHORT,
        ("sk", Display::Narrow) => &SK_NARROW,
        _ => return None,
    };
    let index = DENOMINATORS
        .iter()
        .position(|candidate| *candidate == denominator)?;
    patterns.get(index).copied()
}
