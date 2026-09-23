// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral ECMA-402 diagnostic parameter parsing and report construction.

use rmcp::schemars;
use serde::Deserialize;

/// A read-only request for the host-neutral Collator service's complete
/// resolution trace. `left`/`right` are ordinary Unicode strings; the
/// `*_utf16` forms accept exact code units when investigating lone surrogates
/// or other input a JSON string cannot represent.
#[derive(Deserialize, schemars::JsonSchema)]
pub(super) struct DebugCollatorParams {
    /// Requested BCP 47 locales, in priority order. Omit or pass an empty list
    /// to inspect the stable service default.
    pub(super) locales: Option<Vec<String>>,
    /// `lookup` or `best fit`; defaults to `lookup` because the bundled data
    /// currently advertises the same policy for both.
    pub(super) locale_matcher: Option<String>,
    /// `sort` or `search`; defaults to `sort`.
    pub(super) usage: Option<String>,
    /// Optional UTS 35 collation type, such as `phonebk` or `emoji`.
    pub(super) collation: Option<String>,
    /// Optional override for numeric ordering.
    pub(super) numeric: Option<bool>,
    /// `upper`, `lower`, or `false`.
    pub(super) case_first: Option<String>,
    /// `base`, `accent`, `case`, or `variant`; defaults to `variant`.
    pub(super) sensitivity: Option<String>,
    /// Optional override for punctuation handling.
    pub(super) ignore_punctuation: Option<bool>,
    /// The first ordinary Unicode string to compare. Must be supplied together
    /// with `right` if neither `*_utf16` form is used.
    pub(super) left: Option<String>,
    /// The second ordinary Unicode string to compare.
    pub(super) right: Option<String>,
    /// Exact first UTF-16 input. Mutually exclusive with `left`.
    pub(super) left_utf16: Option<Vec<u16>>,
    /// Exact second UTF-16 input. Mutually exclusive with `right`.
    pub(super) right_utf16: Option<Vec<u16>>,
}

/// A read-only request for the host-neutral decimal `Intl.NumberFormat`
/// service. The decimal string form avoids JSON-number rounding before the
/// service applies its own ECMA-402 fraction-digit rules.
#[derive(Deserialize, schemars::JsonSchema)]
pub(super) struct DebugNumberFormatParams {
    /// Requested BCP 47 locales, in priority order. Omit or pass an empty list
    /// to inspect the stable service default.
    pub(super) locales: Option<Vec<String>>,
    /// `lookup` or `best fit`; defaults to `lookup`.
    pub(super) locale_matcher: Option<String>,
    /// `auto`, `never`, `always`, or `min2`; defaults to `auto`.
    pub(super) use_grouping: Option<String>,
    /// Optional minimum fraction digits (0 through 100).
    pub(super) minimum_fraction_digits: Option<u8>,
    /// Optional maximum fraction digits (0 through 100).
    pub(super) maximum_fraction_digits: Option<u8>,
    /// An optional finite base-10 decimal string to format. It uses an
    /// optional ASCII sign, ASCII digits and an optional decimal point.
    pub(super) decimal: Option<String>,
}

/// A read-only request for the host-neutral `Intl.PluralRules` service.
#[derive(Deserialize, schemars::JsonSchema)]
pub(super) struct DebugPluralRulesParams {
    /// Requested BCP 47 locales, in priority order. Omit or pass an empty list
    /// to inspect the stable service default.
    pub(super) locales: Option<Vec<String>>,
    /// `lookup` or `best fit`; defaults to `lookup`.
    pub(super) locale_matcher: Option<String>,
    /// `cardinal` or `ordinal`; defaults to `cardinal`.
    pub(super) rule_type: Option<String>,
    /// An optional finite base-10 decimal string to categorize. Its visible
    /// fractional zeros are preserved, so `1` and `1.0` can differ.
    pub(super) decimal: Option<String>,
}

/// A read-only request for the host-neutral `Intl.ListFormat` service.
#[derive(Deserialize, schemars::JsonSchema)]
pub(super) struct DebugListFormatParams {
    /// Requested BCP 47 locales, in priority order. Omit or pass an empty list
    /// to inspect the stable service default.
    pub(super) locales: Option<Vec<String>>,
    /// `lookup` or `best fit`; defaults to `lookup`.
    pub(super) locale_matcher: Option<String>,
    /// `conjunction`, `disjunction`, or `unit`; defaults to `conjunction`.
    pub(super) list_type: Option<String>,
    /// `wide`, `short`, or `narrow`; defaults to `wide`.
    pub(super) style: Option<String>,
    /// Optional already-coerced string items to format.
    pub(super) items: Option<Vec<String>>,
}

/// A read-only request for the host-neutral `Intl.Segmenter` service.
#[derive(Deserialize, schemars::JsonSchema)]
pub(super) struct DebugSegmenterParams {
    /// Requested BCP 47 locales, in priority order. Omit or pass an empty list
    /// to inspect the stable service default.
    pub(super) locales: Option<Vec<String>>,
    /// `lookup` or `best fit`; defaults to `lookup`.
    pub(super) locale_matcher: Option<String>,
    /// `grapheme`, `word`, or `sentence`; defaults to `grapheme`.
    pub(super) granularity: Option<String>,
    /// Optional already-coerced text to segment. Defaults to an empty string.
    pub(super) text: Option<String>,
}

/// A read-only request for the host-neutral `Intl.Locale` information data.
#[derive(Deserialize, schemars::JsonSchema)]
pub(super) struct DebugLocaleParams {
    /// The BCP 47 locale tag to canonicalize and inspect.
    pub(super) locale: String,
    /// Optional likely-subtag transform: `maximize` or `minimize`. Omit it to
    /// inspect the canonical input without a transform.
    pub(super) transform: Option<String>,
    /// Typed `Intl.Locale` options applied before an optional transform.
    pub(super) options: Option<DebugLocaleOptionsParams>,
}

/// JSON-shaped typed options for `debug_locale`.
#[derive(Deserialize, schemars::JsonSchema)]
pub(super) struct DebugLocaleOptionsParams {
    pub(super) language: Option<String>,
    pub(super) script: Option<String>,
    pub(super) region: Option<String>,
    pub(super) variants: Option<String>,
    pub(super) calendar: Option<String>,
    pub(super) collation: Option<String>,
    pub(super) hour_cycle: Option<String>,
    pub(super) case_first: Option<String>,
    pub(super) numeric: Option<bool>,
    pub(super) numbering_system: Option<String>,
    pub(super) first_day_of_week: Option<String>,
}

impl From<DebugLocaleOptionsParams> for blueice_ecma402::LocaleOptions {
    fn from(options: DebugLocaleOptionsParams) -> Self {
        Self {
            language: options.language,
            script: options.script,
            region: options.region,
            variants: options.variants,
            calendar: options.calendar,
            collation: options.collation,
            hour_cycle: options.hour_cycle,
            case_first: options.case_first,
            numeric: options.numeric,
            numbering_system: options.numbering_system,
            first_day_of_week: options.first_day_of_week,
        }
    }
}

const MAX_DEBUG_LOCALES: usize = 100;
const MAX_DEBUG_UTF16_UNITS: usize = 65_536;
const MAX_DEBUG_DECIMAL_BYTES: usize = 4_096;
pub(super) const MAX_DEBUG_LIST_ITEMS: usize = 1_024;
const MAX_DEBUG_LIST_BYTES: usize = 65_536;
const MAX_DEBUG_SEGMENTER_BYTES: usize = 65_536;
pub(super) const MAX_DEBUG_SEGMENTS: usize = 4_096;

fn debug_locale_matcher(value: Option<&str>) -> Result<blueice_ecma402::LocaleMatcher, String> {
    match value.unwrap_or("lookup") {
        "lookup" => Ok(blueice_ecma402::LocaleMatcher::Lookup),
        "best fit" => Ok(blueice_ecma402::LocaleMatcher::BestFit),
        value => Err(format!(
            "invalid locale_matcher {value:?}; expected lookup or best fit"
        )),
    }
}

fn debug_collator_usage(value: Option<&str>) -> Result<blueice_ecma402::CollatorUsage, String> {
    match value.unwrap_or("sort") {
        "sort" => Ok(blueice_ecma402::CollatorUsage::Sort),
        "search" => Ok(blueice_ecma402::CollatorUsage::Search),
        value => Err(format!("invalid usage {value:?}; expected sort or search")),
    }
}

fn debug_case_first(value: Option<&str>) -> Result<Option<blueice_ecma402::CaseFirst>, String> {
    value
        .map(|value| match value {
            "upper" => Ok(blueice_ecma402::CaseFirst::Upper),
            "lower" => Ok(blueice_ecma402::CaseFirst::Lower),
            "false" => Ok(blueice_ecma402::CaseFirst::False),
            value => Err(format!(
                "invalid case_first {value:?}; expected upper, lower, or false"
            )),
        })
        .transpose()
}

fn debug_sensitivity(value: Option<&str>) -> Result<blueice_ecma402::Sensitivity, String> {
    match value.unwrap_or("variant") {
        "base" => Ok(blueice_ecma402::Sensitivity::Base),
        "accent" => Ok(blueice_ecma402::Sensitivity::Accent),
        "case" => Ok(blueice_ecma402::Sensitivity::Case),
        "variant" => Ok(blueice_ecma402::Sensitivity::Variant),
        value => Err(format!(
            "invalid sensitivity {value:?}; expected base, accent, case, or variant"
        )),
    }
}

fn debug_number_grouping(value: Option<&str>) -> Result<blueice_ecma402::NumberGrouping, String> {
    match value.unwrap_or("auto") {
        "auto" => Ok(blueice_ecma402::NumberGrouping::Auto),
        "never" => Ok(blueice_ecma402::NumberGrouping::Never),
        "always" => Ok(blueice_ecma402::NumberGrouping::Always),
        "min2" => Ok(blueice_ecma402::NumberGrouping::Min2),
        value => Err(format!(
            "invalid use_grouping {value:?}; expected auto, never, always, or min2"
        )),
    }
}

fn debug_plural_rule_type(value: Option<&str>) -> Result<blueice_ecma402::PluralRuleType, String> {
    match value.unwrap_or("cardinal") {
        "cardinal" => Ok(blueice_ecma402::PluralRuleType::Cardinal),
        "ordinal" => Ok(blueice_ecma402::PluralRuleType::Ordinal),
        value => Err(format!(
            "invalid rule_type {value:?}; expected cardinal or ordinal"
        )),
    }
}

fn debug_list_type(value: Option<&str>) -> Result<blueice_ecma402::ListType, String> {
    match value.unwrap_or("conjunction") {
        "conjunction" => Ok(blueice_ecma402::ListType::Conjunction),
        "disjunction" => Ok(blueice_ecma402::ListType::Disjunction),
        "unit" => Ok(blueice_ecma402::ListType::Unit),
        value => Err(format!(
            "invalid list_type {value:?}; expected conjunction, disjunction, or unit"
        )),
    }
}

fn debug_list_style(value: Option<&str>) -> Result<blueice_ecma402::ListStyle, String> {
    match value.unwrap_or("wide") {
        "wide" => Ok(blueice_ecma402::ListStyle::Wide),
        "short" => Ok(blueice_ecma402::ListStyle::Short),
        "narrow" => Ok(blueice_ecma402::ListStyle::Narrow),
        value => Err(format!(
            "invalid style {value:?}; expected wide, short, or narrow"
        )),
    }
}

fn debug_segmenter_granularity(
    value: Option<&str>,
) -> Result<blueice_ecma402::SegmenterGranularity, String> {
    match value.unwrap_or("grapheme") {
        "grapheme" => Ok(blueice_ecma402::SegmenterGranularity::Grapheme),
        "word" => Ok(blueice_ecma402::SegmenterGranularity::Word),
        "sentence" => Ok(blueice_ecma402::SegmenterGranularity::Sentence),
        value => Err(format!(
            "invalid granularity {value:?}; expected grapheme, word, or sentence"
        )),
    }
}

fn debug_collation(value: Option<String>) -> Result<Option<String>, String> {
    if let Some(value) = &value {
        if !value.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }) {
            return Err("invalid collation; expected UTS 35 type subtags".into());
        }
    }
    Ok(value)
}

fn debug_utf16_input(
    text: Option<String>,
    units: Option<Vec<u16>>,
    name: &str,
) -> Result<Option<Vec<u16>>, String> {
    let value = match (text, units) {
        (Some(_), Some(_)) => {
            return Err(format!("{name} and {name}_utf16 cannot both be supplied"));
        }
        (Some(text), None) => text.encode_utf16().collect(),
        (None, Some(units)) => units,
        (None, None) => return Ok(None),
    };
    if value.len() > MAX_DEBUG_UTF16_UNITS {
        return Err(format!(
            "{name} input exceeds the {MAX_DEBUG_UTF16_UNITS}-unit debug limit"
        ));
    }
    Ok(Some(value))
}

fn locale_matcher_name(value: blueice_ecma402::LocaleMatcher) -> &'static str {
    match value {
        blueice_ecma402::LocaleMatcher::Lookup => "lookup",
        blueice_ecma402::LocaleMatcher::BestFit => "best fit",
    }
}

fn usage_name(value: blueice_ecma402::CollatorUsage) -> &'static str {
    match value {
        blueice_ecma402::CollatorUsage::Sort => "sort",
        blueice_ecma402::CollatorUsage::Search => "search",
    }
}

fn case_first_name(value: blueice_ecma402::CaseFirst) -> &'static str {
    match value {
        blueice_ecma402::CaseFirst::Upper => "upper",
        blueice_ecma402::CaseFirst::Lower => "lower",
        blueice_ecma402::CaseFirst::False => "false",
    }
}

fn sensitivity_name(value: blueice_ecma402::Sensitivity) -> &'static str {
    match value {
        blueice_ecma402::Sensitivity::Base => "base",
        blueice_ecma402::Sensitivity::Accent => "accent",
        blueice_ecma402::Sensitivity::Case => "case",
        blueice_ecma402::Sensitivity::Variant => "variant",
    }
}

fn number_grouping_name(value: blueice_ecma402::NumberGrouping) -> &'static str {
    match value {
        blueice_ecma402::NumberGrouping::Auto => "auto",
        blueice_ecma402::NumberGrouping::Never => "never",
        blueice_ecma402::NumberGrouping::Always => "always",
        blueice_ecma402::NumberGrouping::Min2 => "min2",
    }
}

fn plural_rule_type_name(value: blueice_ecma402::PluralRuleType) -> &'static str {
    match value {
        blueice_ecma402::PluralRuleType::Cardinal => "cardinal",
        blueice_ecma402::PluralRuleType::Ordinal => "ordinal",
    }
}

fn plural_category_name(value: blueice_ecma402::PluralCategory) -> &'static str {
    match value {
        blueice_ecma402::PluralCategory::Zero => "zero",
        blueice_ecma402::PluralCategory::One => "one",
        blueice_ecma402::PluralCategory::Two => "two",
        blueice_ecma402::PluralCategory::Few => "few",
        blueice_ecma402::PluralCategory::Many => "many",
        blueice_ecma402::PluralCategory::Other => "other",
    }
}

fn list_type_name(value: blueice_ecma402::ListType) -> &'static str {
    match value {
        blueice_ecma402::ListType::Conjunction => "conjunction",
        blueice_ecma402::ListType::Disjunction => "disjunction",
        blueice_ecma402::ListType::Unit => "unit",
    }
}

fn list_style_name(value: blueice_ecma402::ListStyle) -> &'static str {
    match value {
        blueice_ecma402::ListStyle::Wide => "wide",
        blueice_ecma402::ListStyle::Short => "short",
        blueice_ecma402::ListStyle::Narrow => "narrow",
    }
}

fn segmenter_granularity_name(value: blueice_ecma402::SegmenterGranularity) -> &'static str {
    match value {
        blueice_ecma402::SegmenterGranularity::Grapheme => "grapheme",
        blueice_ecma402::SegmenterGranularity::Word => "word",
        blueice_ecma402::SegmenterGranularity::Sentence => "sentence",
    }
}

/// Builds the structured, read-only output shared by the MCP tool and its
/// tests. It has no connection to `core`: ECMA-402 is a pure native service,
/// and this diagnostic cannot execute script, mutate a page, or acquire
/// browser authority.
pub(super) fn debug_collator_report(
    params: DebugCollatorParams,
) -> Result<serde_json::Value, String> {
    let locale_names = params.locales.unwrap_or_default();
    if locale_names.len() > MAX_DEBUG_LOCALES {
        return Err(format!(
            "locales exceeds the {MAX_DEBUG_LOCALES}-locale debug limit"
        ));
    }
    let locales = locale_names
        .iter()
        .enumerate()
        .map(|(index, locale)| {
            blueice_ecma402::canonicalize(locale)
                .map_err(|_| format!("invalid locale at index {index}: {locale:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let options = blueice_ecma402::CollatorOptions {
        locale_matcher: debug_locale_matcher(params.locale_matcher.as_deref())?,
        usage: debug_collator_usage(params.usage.as_deref())?,
        collation: debug_collation(params.collation)?,
        numeric: params.numeric,
        case_first: debug_case_first(params.case_first.as_deref())?,
        sensitivity: debug_sensitivity(params.sensitivity.as_deref())?,
        ignore_punctuation: params.ignore_punctuation,
    };
    let left = debug_utf16_input(params.left, params.left_utf16, "left")?;
    let right = debug_utf16_input(params.right, params.right_utf16, "right")?;
    let comparison = match (left, right) {
        (None, None) => None,
        (Some(left), Some(right)) => Some((left, right)),
        _ => return Err("left and right inputs must be supplied together".into()),
    };
    let collator = blueice_ecma402::Collator::try_new(&locales, options)
        .map_err(|error| format!("could not construct Collator: {error}"))?;
    let negotiation = collator.negotiation();
    let resolved = collator.resolved_options();
    let comparison = comparison.map(|(left, right)| {
        let ordering = match collator.compare_utf16(&left, &right) {
            std::cmp::Ordering::Less => "less",
            std::cmp::Ordering::Equal => "equal",
            std::cmp::Ordering::Greater => "greater",
        };
        serde_json::json!({
            "left_utf16": left,
            "right_utf16": right,
            "ordering": ordering,
        })
    });
    Ok(serde_json::json!({
        "service": "blueice-ecma402/Collator",
        "negotiation": {
            "matcher": locale_matcher_name(negotiation.matcher()),
            "candidates": negotiation.candidates().iter().map(|candidate| serde_json::json!({
                "requested": candidate.requested().as_str(),
                "supported": candidate.is_supported(),
            })).collect::<Vec<_>>(),
            "selected_locale": negotiation.selected().as_str(),
            "used_default": negotiation.used_default(),
        },
        "resolved_options": {
            "locale": resolved.locale,
            "usage": usage_name(resolved.usage),
            "sensitivity": sensitivity_name(resolved.sensitivity),
            "ignore_punctuation": resolved.ignore_punctuation,
            "collation": resolved.collation,
            "numeric": resolved.numeric,
            "case_first": case_first_name(resolved.case_first),
        },
        "comparison": comparison,
    }))
}

/// Builds the structured, read-only decimal `Intl.NumberFormat` diagnostic.
/// Like [`debug_collator_report`], it invokes a pure host-neutral service and
/// cannot access a Realm, page or browser state.
pub(super) fn debug_number_format_report(
    params: DebugNumberFormatParams,
) -> Result<serde_json::Value, String> {
    let locale_names = params.locales.unwrap_or_default();
    if locale_names.len() > MAX_DEBUG_LOCALES {
        return Err(format!(
            "locales exceeds the {MAX_DEBUG_LOCALES}-locale debug limit"
        ));
    }
    let locales = locale_names
        .iter()
        .enumerate()
        .map(|(index, locale)| {
            blueice_ecma402::canonicalize(locale)
                .map_err(|_| format!("invalid locale at index {index}: {locale:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if params
        .decimal
        .as_ref()
        .is_some_and(|decimal| decimal.len() > MAX_DEBUG_DECIMAL_BYTES)
    {
        return Err(format!(
            "decimal exceeds the {MAX_DEBUG_DECIMAL_BYTES}-byte debug limit"
        ));
    }
    let formatter = blueice_ecma402::NumberFormat::try_new(
        &locales,
        blueice_ecma402::NumberFormatOptions {
            locale_matcher: debug_locale_matcher(params.locale_matcher.as_deref())?,
            use_grouping: debug_number_grouping(params.use_grouping.as_deref())?,
            minimum_fraction_digits: params.minimum_fraction_digits,
            maximum_fraction_digits: params.maximum_fraction_digits,
            ..Default::default()
        },
    )
    .map_err(|error| format!("could not construct NumberFormat: {error}"))?;
    let negotiation = formatter.negotiation();
    let resolved = formatter.resolved_options();
    let formatted = params
        .decimal
        .as_deref()
        .map(|decimal| {
            formatter
                .format_decimal(decimal)
                .map_err(|error| format!("could not format decimal: {error}"))
        })
        .transpose()?;
    Ok(serde_json::json!({
        "service": "blueice-ecma402/NumberFormat(decimal)",
        "negotiation": {
            "matcher": locale_matcher_name(negotiation.matcher()),
            "candidates": negotiation.candidates().iter().map(|candidate| serde_json::json!({
                "requested": candidate.requested().as_str(),
                "supported": candidate.is_supported(),
            })).collect::<Vec<_>>(),
            "selected_locale": negotiation.selected().as_str(),
            "used_default": negotiation.used_default(),
        },
        "resolved_options": {
            "locale": resolved.locale,
            "numbering_system": resolved.numbering_system,
            "use_grouping": number_grouping_name(resolved.use_grouping),
            "minimum_fraction_digits": resolved.minimum_fraction_digits,
            "maximum_fraction_digits": resolved.maximum_fraction_digits,
        },
        "decimal": params.decimal,
        "formatted": formatted,
    }))
}

/// Builds the structured, read-only `Intl.PluralRules` diagnostic. Like the
/// other ECMA-402 diagnostics, it has no connection to a Realm or browser
/// state and invokes only the host-neutral service.
pub(super) fn debug_plural_rules_report(
    params: DebugPluralRulesParams,
) -> Result<serde_json::Value, String> {
    let locale_names = params.locales.unwrap_or_default();
    if locale_names.len() > MAX_DEBUG_LOCALES {
        return Err(format!(
            "locales exceeds the {MAX_DEBUG_LOCALES}-locale debug limit"
        ));
    }
    let locales = locale_names
        .iter()
        .enumerate()
        .map(|(index, locale)| {
            blueice_ecma402::canonicalize(locale)
                .map_err(|_| format!("invalid locale at index {index}: {locale:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if params
        .decimal
        .as_ref()
        .is_some_and(|decimal| decimal.len() > MAX_DEBUG_DECIMAL_BYTES)
    {
        return Err(format!(
            "decimal exceeds the {MAX_DEBUG_DECIMAL_BYTES}-byte debug limit"
        ));
    }
    let rules = blueice_ecma402::PluralRules::try_new(
        &locales,
        blueice_ecma402::PluralRulesOptions {
            locale_matcher: debug_locale_matcher(params.locale_matcher.as_deref())?,
            rule_type: debug_plural_rule_type(params.rule_type.as_deref())?,
        },
    )
    .map_err(|error| format!("could not construct PluralRules: {error}"))?;
    let negotiation = rules.negotiation();
    let resolved = rules.resolved_options();
    let category = params
        .decimal
        .as_deref()
        .map(|decimal| {
            rules
                .select_decimal(decimal)
                .map(plural_category_name)
                .map_err(|error| format!("could not categorize decimal: {error}"))
        })
        .transpose()?;
    Ok(serde_json::json!({
        "service": "blueice-ecma402/PluralRules",
        "negotiation": {
            "matcher": locale_matcher_name(negotiation.matcher()),
            "candidates": negotiation.candidates().iter().map(|candidate| serde_json::json!({
                "requested": candidate.requested().as_str(),
                "supported": candidate.is_supported(),
            })).collect::<Vec<_>>(),
            "selected_locale": negotiation.selected().as_str(),
            "used_default": negotiation.used_default(),
        },
        "resolved_options": {
            "locale": resolved.locale,
            "rule_type": plural_rule_type_name(resolved.rule_type),
        },
        "decimal": params.decimal,
        "category": category,
    }))
}

/// Builds the structured, read-only `Intl.ListFormat` diagnostic. It invokes
/// only the host-neutral service and cannot access a Realm or browser state.
pub(super) fn debug_list_format_report(
    params: DebugListFormatParams,
) -> Result<serde_json::Value, String> {
    let locale_names = params.locales.unwrap_or_default();
    if locale_names.len() > MAX_DEBUG_LOCALES {
        return Err(format!(
            "locales exceeds the {MAX_DEBUG_LOCALES}-locale debug limit"
        ));
    }
    let locales = locale_names
        .iter()
        .enumerate()
        .map(|(index, locale)| {
            blueice_ecma402::canonicalize(locale)
                .map_err(|_| format!("invalid locale at index {index}: {locale:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let items = params.items.unwrap_or_default();
    if items.len() > MAX_DEBUG_LIST_ITEMS {
        return Err(format!(
            "items exceeds the {MAX_DEBUG_LIST_ITEMS}-item debug limit"
        ));
    }
    if items.iter().map(String::len).sum::<usize>() > MAX_DEBUG_LIST_BYTES {
        return Err(format!(
            "items exceed the {MAX_DEBUG_LIST_BYTES}-byte debug limit"
        ));
    }
    let formatter = blueice_ecma402::ListFormat::try_new(
        &locales,
        blueice_ecma402::ListFormatOptions {
            locale_matcher: debug_locale_matcher(params.locale_matcher.as_deref())?,
            list_type: debug_list_type(params.list_type.as_deref())?,
            style: debug_list_style(params.style.as_deref())?,
        },
    )
    .map_err(|error| format!("could not construct ListFormat: {error}"))?;
    let negotiation = formatter.negotiation();
    let resolved = formatter.resolved_options();
    let formatted = formatter.format(items.iter());
    let parts = formatter
        .format_to_parts(items.iter())
        .map_err(|error| format!("could not format ListFormat parts: {error}"))?;
    Ok(serde_json::json!({
        "service": "blueice-ecma402/ListFormat",
        "negotiation": {
            "matcher": locale_matcher_name(negotiation.matcher()),
            "candidates": negotiation.candidates().iter().map(|candidate| serde_json::json!({
                "requested": candidate.requested().as_str(),
                "supported": candidate.is_supported(),
            })).collect::<Vec<_>>(),
            "selected_locale": negotiation.selected().as_str(),
            "used_default": negotiation.used_default(),
        },
        "resolved_options": {
            "locale": resolved.locale,
            "list_type": list_type_name(resolved.list_type),
            "style": list_style_name(resolved.style),
        },
        "items": items,
        "formatted": formatted,
        "parts": parts.into_iter().map(|part| serde_json::json!({
            "type": match part.kind {
                blueice_ecma402::ListPartKind::Element => "element",
                blueice_ecma402::ListPartKind::Literal => "literal",
            },
            "value": part.value,
        })).collect::<Vec<_>>(),
    }))
}

/// Builds the structured, read-only `Intl.Segmenter` diagnostic. It invokes
/// only the host-neutral service and cannot access a Realm or browser state.
pub(super) fn debug_segmenter_report(
    params: DebugSegmenterParams,
) -> Result<serde_json::Value, String> {
    let locale_names = params.locales.unwrap_or_default();
    if locale_names.len() > MAX_DEBUG_LOCALES {
        return Err(format!(
            "locales exceeds the {MAX_DEBUG_LOCALES}-locale debug limit"
        ));
    }
    let locales = locale_names
        .iter()
        .enumerate()
        .map(|(index, locale)| {
            blueice_ecma402::canonicalize(locale)
                .map_err(|_| format!("invalid locale at index {index}: {locale:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let text = params.text.unwrap_or_default();
    if text.len() > MAX_DEBUG_SEGMENTER_BYTES {
        return Err(format!(
            "text exceeds the {MAX_DEBUG_SEGMENTER_BYTES}-byte debug limit"
        ));
    }
    let segmenter = blueice_ecma402::Segmenter::try_new(
        &locales,
        blueice_ecma402::SegmenterOptions {
            locale_matcher: debug_locale_matcher(params.locale_matcher.as_deref())?,
            granularity: debug_segmenter_granularity(params.granularity.as_deref())?,
        },
    )
    .map_err(|error| format!("could not construct Segmenter: {error}"))?;
    let negotiation = segmenter.negotiation();
    let resolved = segmenter.resolved_options();
    let segments = segmenter.segment(&text);
    if segments.len() > MAX_DEBUG_SEGMENTS {
        return Err(format!(
            "segmentation exceeds the {MAX_DEBUG_SEGMENTS}-segment debug limit"
        ));
    }
    Ok(serde_json::json!({
        "service": "blueice-ecma402/Segmenter",
        "negotiation": {
            "matcher": locale_matcher_name(negotiation.matcher()),
            "candidates": negotiation.candidates().iter().map(|candidate| serde_json::json!({
                "requested": candidate.requested().as_str(),
                "supported": candidate.is_supported(),
            })).collect::<Vec<_>>(),
            "selected_locale": negotiation.selected().as_str(),
            "used_default": negotiation.used_default(),
        },
        "resolved_options": {
            "locale": resolved.locale,
            "granularity": segmenter_granularity_name(resolved.granularity),
        },
        "text": text,
        "segments": segments.into_iter().map(|segment| serde_json::json!({
            "segment": segment.segment,
            "index_utf16": segment.index_utf16,
            "is_word_like": segment.is_word_like,
        })).collect::<Vec<_>>(),
    }))
}

/// Builds the structured read-only output for `Intl.Locale` data inspection.
/// Like [`debug_collator_report`], this cannot access a Realm or browser state.
pub(super) fn debug_locale_report(params: DebugLocaleParams) -> Result<serde_json::Value, String> {
    let locale = blueice_ecma402::canonicalize(&params.locale)
        .map_err(|_| format!("invalid locale: {:?}", params.locale))?;
    let option_applied_locale = blueice_ecma402::apply_locale_options(
        &locale,
        &params.options.map(Into::into).unwrap_or_default(),
    )
    .map_err(|error| error.to_string())?;
    let effective_locale = match params.transform.as_deref().unwrap_or("none") {
        "none" => option_applied_locale.clone(),
        "maximize" => blueice_ecma402::maximize_locale(&option_applied_locale),
        "minimize" => blueice_ecma402::minimize_locale(&option_applied_locale),
        value => {
            return Err(format!(
                "invalid transform {value:?}; expected maximize or minimize"
            ));
        }
    };
    let information = blueice_ecma402::locale_information(&effective_locale);
    let text_direction = match information.text_direction {
        blueice_ecma402::TextDirection::LeftToRight => "ltr",
        blueice_ecma402::TextDirection::RightToLeft => "rtl",
    };
    Ok(serde_json::json!({
        "service": "blueice-ecma402/LocaleInformation",
        "canonical_locale": locale.as_str(),
        "option_applied_locale": option_applied_locale.as_str(),
        "effective_locale": effective_locale.as_str(),
        "information": {
            "calendars": information.calendars,
            "collations": information.collations,
            "hour_cycles": information.hour_cycles,
            "numbering_systems": information.numbering_systems,
            "text_direction": text_direction,
            "time_zones": information.time_zones,
            "week_info": {
                "first_day": information.week_info.first_day,
                "weekend": information.week_info.weekend,
            },
        },
    }))
}
