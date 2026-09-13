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
    let language = locale.id.language.as_str();
    let preference_region = locale_preference_region(locale);
    let calendars = unicode_keyword(locale, "ca")
        .map(|calendar| vec![calendar])
        .unwrap_or_else(|| calendars_for_region(&preference_region));
    let collations = unicode_keyword(locale, "co")
        .filter(|value| value != "standard" && value != "search")
        .map(|collation| vec![collation])
        .unwrap_or_else(|| collations_for_language(language));
    let hour_cycles = unicode_keyword(locale, "hc")
        .map(|hour_cycle| vec![hour_cycle])
        .unwrap_or_else(|| hour_cycles_for_locale(language, &preference_region));
    let numbering_systems = vec![unicode_keyword(locale, "nu").unwrap_or_else(|| {
        if language == "ar" {
            "arab".into()
        } else {
            "latn".into()
        }
    })];
    let script = locale.id.script.map(|script| script.to_string());
    let text_direction = if script.as_deref().is_some_and(|script| {
        matches!(
            script,
            "Arab" | "Hebr" | "Syrc" | "Thaa" | "Nkoo" | "Adlm" | "Rohg"
        )
    }) || matches!(
        language,
        "ar" | "arc"
            | "ckb"
            | "dv"
            | "fa"
            | "he"
            | "ks"
            | "ku"
            | "nqo"
            | "ps"
            | "sd"
            | "syr"
            | "ug"
            | "ur"
            | "yi"
    ) {
        TextDirection::RightToLeft
    } else {
        TextDirection::LeftToRight
    };
    let time_zones = locale.id.region.map(|region| {
        match region.as_str() {
            "US" => vec![
                "America/Adak",
                "America/Anchorage",
                "America/Boise",
                "America/Chicago",
                "America/Denver",
                "America/Detroit",
                "America/Indiana/Indianapolis",
                "America/Los_Angeles",
                "America/New_York",
                "Pacific/Honolulu",
            ],
            "GB" => vec!["Europe/London"],
            "JP" => vec!["Asia/Tokyo"],
            "TW" => vec!["Asia/Taipei"],
            _ => vec!["Etc/UTC"],
        }
        .into_iter()
        .map(str::to_owned)
        .collect()
    });
    let mut week_info = week_info_for_region(&preference_region);
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
fn locale_preference_region(locale: &IcuLocale) -> String {
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

fn calendars_for_region(region: &str) -> Vec<String> {
    match region {
        "TH" => vec!["buddhist", "gregory"],
        "JP" => vec!["gregory", "japanese"],
        "IN" => vec!["gregory", "indian"],
        "IR" | "AF" => vec![
            "persian",
            "gregory",
            "islamic",
            "islamic-civil",
            "islamic-tbla",
        ],
        "ET" => vec!["gregory", "ethiopic"],
        "BD" | "MY" | "PK" => vec!["gregory", "islamic", "islamic-civil", "islamic-tbla"],
        "KR" => vec!["gregory", "dangi"],
        _ => vec!["gregory"],
    }
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn collations_for_language(language: &str) -> Vec<String> {
    if language == "und" || ("qfz"..="qtz").contains(&language) {
        vec!["emoji".into(), "eor".into()]
    } else {
        vec!["emoji".into()]
    }
}

fn hour_cycles_for_locale(language: &str, region: &str) -> Vec<String> {
    // CLDR time-data records may be keyed by both language and region. They
    // therefore take precedence over the region-only fallback below.
    let hour_cycle = match (language, region) {
        ("en", "US" | "CA" | "001") | ("ar", "001") => "h12",
        _ => match region {
            "US" | "IN" | "ET" | "BD" | "GR" | "PH" | "KR" | "MY" | "PK" => "h12",
            _ => "h23",
        },
    };
    vec![hour_cycle.into()]
}

fn week_info_for_region(region: &str) -> WeekInfo {
    let (first_day, weekend) = match region {
        "US" | "CA" | "JP" | "TH" => (7, vec![6, 7]),
        "IN" => (7, vec![7]),
        "IR" => (6, vec![5]),
        "AF" => (6, vec![4, 5]),
        _ => (1, vec![6, 7]),
    };
    WeekInfo { first_day, weekend }
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
