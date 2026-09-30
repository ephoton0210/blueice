// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Intl.Locale` information services.

use super::*;

/// The writing direction reported by locale information.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextDirection {
    /// Text is laid out from left to right.
    LeftToRight,
    /// Text is laid out from right to left.
    RightToLeft,
}

/// Week data selected for a locale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeekInfo {
    /// ISO-like weekday number: Monday is 1 and Sunday is 7.
    pub first_day: u8,
    /// The locale's weekend weekday numbers.
    pub weekend: Vec<u8>,
}

/// Host-neutral data returned by the `Intl.Locale` information methods.
///
/// This centralizes the data-selection rules BlueJS advertises so another host
/// can make the same choices without a Realm. The service deliberately keeps
/// its compact embedded dataset separate from ECMAScript object semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocaleInformation {
    /// Calendars, with a requested `ca` extension taking precedence.
    pub calendars: Vec<String>,
    /// Collations, with a requested `co` extension taking precedence.
    pub collations: Vec<String>,
    /// Hour cycles, with a requested `hc` extension taking precedence.
    pub hour_cycles: Vec<String>,
    /// Numbering systems, with a requested `nu` extension taking precedence.
    pub numbering_systems: Vec<String>,
    /// The writing direction inferred from language and script.
    pub text_direction: TextDirection,
    /// Region-specific time zones, or `None` when the locale has no region.
    pub time_zones: Option<Vec<String>>,
    /// The locale's first weekday and weekend.
    pub week_info: WeekInfo,
}

/// Returns the locale-information dataset for a canonical tag.
///
/// Calendar, hour-cycle, and week-data lookup use the ECMA-402
/// `RegionPreference` order: a valid `rg` override, an explicit region,
/// `sd`, likely subtags, then the world region (`001`).
pub fn locale_information(locale: &CanonicalLocale) -> LocaleInformation {
    let locale = locale.locale();
    let provider = locale_data_provider();
    let language = locale.id.language.as_str();
    let preference_region = locale_preference_region(locale);
    let calendars = unicode_keyword(locale, "ca")
        .map(|calendar| vec![calendar])
        .unwrap_or_else(|| {
            provider
                .calendars_for_region(&preference_region)
                .iter()
                .map(ToString::to_string)
                .collect()
        });
    let collations = unicode_keyword(locale, "co")
        .filter(|value| value != "standard" && value != "search")
        .map(|collation| vec![collation])
        .unwrap_or_else(|| {
            provider
                .collations_for_language(language)
                .iter()
                .map(ToString::to_string)
                .collect()
        });
    let hour_cycles = unicode_keyword(locale, "hc")
        .map(|hour_cycle| vec![hour_cycle])
        .unwrap_or_else(|| {
            vec![provider
                .hour_cycle_for_locale(language, &preference_region)
                .into()]
        });
    let numbering_systems = vec![unicode_keyword(locale, "nu")
        .unwrap_or_else(|| provider.default_numbering_system(locale).into())];
    let script = locale.id.script.map(|script| script.to_string());
    let text_direction = if provider.is_right_to_left(language, script.as_deref()) {
        TextDirection::RightToLeft
    } else {
        TextDirection::LeftToRight
    };
    let time_zones = locale.id.region.map(|region| {
        provider
            .time_zones_for_region(region.as_str())
            .iter()
            .map(ToString::to_string)
            .collect()
    });
    let (first_day, weekend) = provider.week_data_for_region(&preference_region);
    let mut week_info = WeekInfo {
        first_day,
        weekend: weekend.to_vec(),
    };
    if let Some(first_day) = unicode_keyword(locale, "fw").as_deref().and_then(weekday) {
        week_info.first_day = first_day;
    }
    LocaleInformation {
        calendars,
        collations,
        hour_cycles,
        numbering_systems,
        text_direction,
        time_zones,
        week_info,
    }
}

/// Selects a region according to ECMA-402's `RegionPreference` record.
pub(crate) fn locale_preference_region(locale: &IcuLocale) -> String {
    let region = locale
        .id
        .region
        .map(|region| region.to_string())
        .or_else(|| unicode_keyword_region(locale, "sd"))
        .or_else(|| maximized_region(locale))
        .unwrap_or_else(|| "001".into());

    // The compact data set has world fallbacks for all canonical regions, so a
    // syntactically valid `rg` value always has data available to override it.
    unicode_keyword_region(locale, "rg").unwrap_or(region)
}

/// Extracts the country/region prefix encoded by `rg` or `sd`.
fn unicode_keyword_region(locale: &IcuLocale, key: &str) -> Option<String> {
    let value = unicode_keyword(locale, key)?;
    let bytes = value.as_bytes();
    let length = if bytes.get(..2).is_some_and(|prefix| prefix.is_ascii())
        && bytes[..2].iter().all(u8::is_ascii_alphabetic)
    {
        2
    } else if bytes.get(..3).is_some_and(|prefix| prefix.is_ascii())
        && bytes[..3].iter().all(u8::is_ascii_digit)
    {
        3
    } else {
        return None;
    };
    if key == "rg" && value.get(length..) != Some("zzzz") {
        return None;
    }
    Some(value[..length].to_ascii_uppercase())
}

/// Gets the region supplied by CLDR likely subtags, when one is available.
fn maximized_region(locale: &IcuLocale) -> Option<String> {
    let mut maximal = locale.clone();
    icu_locale::LocaleExpander::new_extended().maximize(&mut maximal.id);
    maximal.id.region.map(|region| region.to_string())
}

fn weekday(value: &str) -> Option<u8> {
    match value {
        "mon" => Some(1),
        "tue" => Some(2),
        "wed" => Some(3),
        "thu" => Some(4),
        "fri" => Some(5),
        "sat" => Some(6),
        "sun" => Some(7),
        _ => None,
    }
}
