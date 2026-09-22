// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete pinned CLDR data for `Intl.DisplayNames`.
//!
//! The compressed table is generated from Unicode CLDR JSON 48.2.1
//! (commit 26a79cb42bfcc90def764102aa2af126d9ef3108), reading all
//! `cldr-localenames-full` and `cldr-dates-full` locale records. The derived
//! data is distributed under Unicode License V3; see
//! `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::Read, sync::OnceLock};

const CLDR_RESOLVED_LOCALE_COUNT: usize = 766;

struct PinnedDisplayNameData {
    /// The decompressed rows. Values remain base64 encoded, so lookup decodes
    /// only the one string selected by an `Intl.DisplayNames#of` call.
    rows: String,
    /// Byte ranges in `rows`, keyed by lower-cased CLDR locale identifier.
    locale_ranges: HashMap<String, (usize, usize)>,
}

static PINNED_DISPLAY_NAMES: OnceLock<PinnedDisplayNameData> = OnceLock::new();

fn pinned_display_names() -> &'static PinnedDisplayNameData {
    PINNED_DISPLAY_NAMES.get_or_init(|| {
        let encoded = include_str!("display_names_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR DisplayNames data must be valid base64");
        let mut rows = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut rows)
            .expect("embedded CLDR DisplayNames data must be valid gzip");

        let mut locale_ranges = HashMap::new();
        let mut offset = 0usize;
        let mut active_locale = None::<&str>;
        let mut active_start = 0usize;
        for line in rows.split_inclusive('\n') {
            let locale = line
                .split_once('\t')
                .expect("embedded CLDR DisplayNames row has a locale")
                .0;
            if active_locale != Some(locale) {
                if let Some(previous) = active_locale {
                    assert!(
                        locale_ranges
                            .insert(previous.to_ascii_lowercase(), (active_start, offset))
                            .is_none(),
                        "embedded CLDR DisplayNames data has duplicate locale groups"
                    );
                }
                active_locale = Some(locale);
                active_start = offset;
            }
            offset += line.len();
        }
        if let Some(previous) = active_locale {
            assert!(
                locale_ranges
                    .insert(previous.to_ascii_lowercase(), (active_start, offset))
                    .is_none(),
                "embedded CLDR DisplayNames data has duplicate final locale group"
            );
        }
        assert_eq!(
            locale_ranges.len(),
            CLDR_RESOLVED_LOCALE_COUNT,
            "embedded CLDR DisplayNames data must retain every resolved locale"
        );
        PinnedDisplayNameData {
            rows,
            locale_ranges,
        }
    })
}

fn locale_candidates(locale: &str) -> Vec<String> {
    let base = locale.split_once("-u-").map_or(locale, |(base, _)| base);
    let mut candidates: Vec<String> = Vec::new();
    let mut push_parent_chain = |candidate: &str| {
        let mut candidate = candidate;
        loop {
            if !candidates
                .iter()
                .any(|previous| previous.eq_ignore_ascii_case(candidate))
            {
                candidates.push(candidate.to_owned());
            }
            let Some((parent, _)) = candidate.rsplit_once('-') else {
                break;
            };
            candidate = parent;
        }
    };

    push_parent_chain(base);
    if let Ok(canonical) = crate::canonicalize(base) {
        let maximal = crate::locale_data_provider()
            .maximize_likely_subtags(canonical.locale())
            .to_string();
        push_parent_chain(&maximal);
    }
    candidates
}

fn styles(style: crate::DisplayNamesStyle) -> [&'static str; 2] {
    match style {
        crate::DisplayNamesStyle::Long => ["long", "long"],
        crate::DisplayNamesStyle::Short => ["short", "long"],
        crate::DisplayNamesStyle::Narrow => ["narrow", "long"],
    }
}

fn decode_value(value: &str) -> Option<String> {
    String::from_utf8(STANDARD.decode(value).ok()?).ok()
}

fn value_for_locale(
    locale: &str,
    name_type: &str,
    style: crate::DisplayNamesStyle,
    code: &str,
) -> Option<String> {
    let data = pinned_display_names();
    for locale in locale_candidates(locale) {
        let Some((start, end)) = data.locale_ranges.get(&locale.to_ascii_lowercase()) else {
            continue;
        };
        for requested_style in styles(style) {
            for line in data.rows[*start..*end].lines() {
                let mut fields = line.splitn(5, '\t');
                let _locale = fields.next()?;
                let row_type = fields.next()?;
                let row_style = fields.next()?;
                let row_code = fields.next()?;
                let value = fields.next()?;
                if row_type == name_type
                    && row_style == requested_style
                    && row_code.eq_ignore_ascii_case(code)
                {
                    return decode_value(value);
                }
            }
        }
    }
    None
}

fn fixed_pattern(locale: &str, code: &str, fallback: &'static str) -> String {
    value_for_locale(locale, "pattern", crate::DisplayNamesStyle::Long, code)
        .unwrap_or_else(|| fallback.into())
}

fn combine_pattern(pattern: &str, first: &str, second: &str) -> String {
    pattern.replace("{0}", first).replace("{1}", second)
}

fn parsed_language_id(code: &str) -> (&str, Option<&str>, Option<&str>, Vec<&str>) {
    let mut subtags = code.split('-');
    let language = subtags
        .next()
        .expect("validated language code has a language");
    let mut remaining = subtags.peekable();
    let script = remaining
        .peek()
        .copied()
        .filter(|subtag| subtag.len() == 4 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .and_then(|_| remaining.next());
    let region = remaining
        .peek()
        .copied()
        .filter(|subtag| {
            (subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()))
                || (subtag.len() == 3 && subtag.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .and_then(|_| remaining.next());
    (language, script, region, remaining.collect())
}

fn component_or_code(
    value: Option<String>,
    code: &str,
    fallback: crate::DisplayNamesFallback,
) -> Option<String> {
    value.or_else(|| (fallback == crate::DisplayNamesFallback::Code).then(|| code.into()))
}

fn language_name(
    locale: &str,
    style: crate::DisplayNamesStyle,
    language_display: crate::DisplayNamesLanguageDisplay,
    fallback: crate::DisplayNamesFallback,
    code: &str,
) -> Option<String> {
    if language_display == crate::DisplayNamesLanguageDisplay::Dialect {
        if let Some(name) = value_for_locale(locale, "language", style, code) {
            return Some(name);
        }
    }

    let (language, script, region, variants) = parsed_language_id(code);
    let language = component_or_code(
        value_for_locale(locale, "language", style, language),
        language,
        fallback,
    )?;
    let mut qualifiers = Vec::new();
    if let Some(script) = script {
        qualifiers.push(component_or_code(
            value_for_locale(locale, "script", style, script),
            script,
            fallback,
        )?);
    }
    if let Some(region) = region {
        qualifiers.push(component_or_code(
            value_for_locale(locale, "region", style, region),
            region,
            fallback,
        )?);
    }
    let mut localized_variants = Vec::new();
    let mut missing_variant = false;
    for &variant in &variants {
        match value_for_locale(locale, "variant", style, variant) {
            Some(name) => localized_variants.push(name),
            None => missing_variant = true,
        }
    }
    if missing_variant {
        if fallback == crate::DisplayNamesFallback::None {
            return None;
        }
        // UTS 35 represents fallback variant subtags as one underscore-joined
        // qualifier, rather than a list of independent language qualifiers.
        qualifiers.push(variants.join("_").to_ascii_uppercase());
    } else {
        qualifiers.extend(localized_variants);
    }
    let Some((first, remaining)) = qualifiers.split_first() else {
        return Some(language);
    };
    let separator = fixed_pattern(locale, "localeSeparator", "{0}, {1}");
    let qualifiers = remaining
        .iter()
        .fold(first.to_owned(), |joined, qualifier| {
            combine_pattern(&separator, &joined, qualifier)
        });
    let pattern = fixed_pattern(locale, "localePattern", "{0} ({1})");
    Some(combine_pattern(&pattern, &language, &qualifiers))
}

fn calendar_code(code: &str) -> &str {
    match code {
        "gregory" => "gregorian",
        "ethioaa" => "ethiopic-amete-alem",
        _ => code,
    }
}

/// Resolves a pinned CLDR display name and leaves ECMA-402 fallback policy to
/// the caller where a complete localized name is unavailable.
pub(super) fn display_name(
    locale: &str,
    display_type: crate::DisplayNamesType,
    style: crate::DisplayNamesStyle,
    language_display: Option<crate::DisplayNamesLanguageDisplay>,
    fallback: crate::DisplayNamesFallback,
    code: &str,
) -> Option<String> {
    match display_type {
        crate::DisplayNamesType::Language => language_name(
            locale,
            style,
            language_display.unwrap_or(crate::DisplayNamesLanguageDisplay::Dialect),
            fallback,
            code,
        ),
        crate::DisplayNamesType::Region => value_for_locale(locale, "region", style, code),
        crate::DisplayNamesType::Script => value_for_locale(locale, "script", style, code),
        crate::DisplayNamesType::Calendar => {
            value_for_locale(locale, "calendar", style, calendar_code(code))
        }
        crate::DisplayNamesType::DateTimeField => {
            value_for_locale(locale, "dateTimeField", style, code)
        }
        crate::DisplayNamesType::Currency => None,
    }
}

/// Whether the raw table contains a complete CLDR locale group.
pub(super) fn has_locale(locale: &str) -> bool {
    locale_candidates(locale).into_iter().any(|candidate| {
        pinned_display_names()
            .locale_ranges
            .contains_key(&candidate.to_ascii_lowercase())
    })
}

fn plural_category_key(category: crate::PluralCategory) -> &'static str {
    match category {
        crate::PluralCategory::Zero => "zero",
        crate::PluralCategory::One => "one",
        crate::PluralCategory::Two => "two",
        crate::PluralCategory::Few => "few",
        crate::PluralCategory::Many => "many",
        crate::PluralCategory::Other => "other",
    }
}

fn display_name_style_for_relative_time(
    style: crate::RelativeTimeStyle,
) -> crate::DisplayNamesStyle {
    match style {
        crate::RelativeTimeStyle::Long => crate::DisplayNamesStyle::Long,
        crate::RelativeTimeStyle::Short => crate::DisplayNamesStyle::Short,
        crate::RelativeTimeStyle::Narrow => crate::DisplayNamesStyle::Narrow,
    }
}

/// Resolves one complete CLDR `relativeTimePattern` for a relative-time
/// direction, width, unit, and cardinal category.
pub(super) fn relative_time_pattern(
    locale: &str,
    style: crate::RelativeTimeStyle,
    unit: crate::RelativeTimeUnit,
    past: bool,
    category: crate::PluralCategory,
) -> Option<String> {
    let style = display_name_style_for_relative_time(style);
    let direction = if past { "past" } else { "future" };
    let code = format!(
        "{}|{direction}|{}",
        unit.as_str(),
        plural_category_key(category)
    );
    value_for_locale(locale, "relativeTimePattern", style, &code)
}

/// Resolves a qualitative CLDR relative-time term for `numeric: "auto"`.
pub(super) fn relative_time_term(
    locale: &str,
    style: crate::RelativeTimeStyle,
    unit: crate::RelativeTimeUnit,
    offset: i8,
) -> Option<String> {
    let style = display_name_style_for_relative_time(style);
    value_for_locale(
        locale,
        "relativeTimeTerm",
        style,
        &format!("{}|{offset}", unit.as_str()),
    )
}

/// Whether the raw table can format at least the required `other` pattern.
pub(super) fn has_relative_time_locale(locale: &str) -> bool {
    relative_time_pattern(
        locale,
        crate::RelativeTimeStyle::Long,
        crate::RelativeTimeUnit::Second,
        false,
        crate::PluralCategory::Other,
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_every_resolved_locale_and_non_english_display_names() {
        let data = pinned_display_names();
        assert_eq!(data.locale_ranges.len(), CLDR_RESOLVED_LOCALE_COUNT);
        assert_eq!(
            locale_candidates("en-US"),
            ["en-US", "en", "en-Latn-US", "en-Latn"]
        );
        assert_eq!(
            value_for_locale("en-US", "region", crate::DisplayNamesStyle::Long, "US").as_deref(),
            Some("United States")
        );
        assert_eq!(
            display_name(
                "ak",
                crate::DisplayNamesType::Language,
                crate::DisplayNamesStyle::Long,
                Some(crate::DisplayNamesLanguageDisplay::Dialect),
                crate::DisplayNamesFallback::None,
                "en-US",
            )
            .as_deref(),
            Some("Amɛrika Borɔfo")
        );
        assert_eq!(
            display_name(
                "zh-Hant",
                crate::DisplayNamesType::DateTimeField,
                crate::DisplayNamesStyle::Short,
                None,
                crate::DisplayNamesFallback::None,
                "timeZoneName",
            )
            .as_deref(),
            Some("時區")
        );
        assert_eq!(
            relative_time_pattern(
                "ak",
                crate::RelativeTimeStyle::Long,
                crate::RelativeTimeUnit::Day,
                false,
                crate::PluralCategory::Other,
            )
            .as_deref(),
            Some("nna {0} mu")
        );
        assert_eq!(
            relative_time_pattern(
                "ar",
                crate::RelativeTimeStyle::Long,
                crate::RelativeTimeUnit::Second,
                true,
                crate::PluralCategory::Other,
            )
            .as_deref(),
            Some("قبل {0} ثانية")
        );
    }
}
