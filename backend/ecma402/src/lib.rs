// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The data-facing, host-neutral core of ECMA-402 internationalization.
//!
//! This crate owns locale identifier canonicalization, ICU-backed locale data
//! selection, and UTF-16 collation. ECMAScript object/Realm semantics and
//! coercion intentionally remain in the embedding runtime.

use fixed_decimal::{CompactDecimal, SignedRoundingMode, UnsignedRoundingMode};
use icu_collator::{
    options::{
        AlternateHandling, CaseLevel, CollatorOptions as IcuCollatorOptions, MaxVariable, Strength,
    },
    preferences::{CollationCaseFirst, CollationNumericOrdering, CollationType},
    CollatorBorrowed, CollatorPreferences,
};
use icu_decimal::{
    input::{Decimal, FloatPrecision},
    options::{DecimalFormatterOptions, GroupingStrategy},
    preferences::NumberingSystem,
    provider::{Baked as DecimalData, DecimalDigitsV1, DecimalSymbolsV1},
    CompactDecimalFormatter, DecimalFormatter, DecimalFormatterPreferences,
};
use icu_list::{
    options::{ListFormatterOptions as IcuListFormatterOptions, ListLength as IcuListLength},
    ListFormatter as IcuListFormatter, ListFormatterPreferences,
};
use icu_locale_core::Locale as IcuLocale;
use icu_plurals::{
    PluralCategory as IcuPluralCategory, PluralRuleType as IcuPluralRuleType,
    PluralRules as IcuPluralRules, PluralRulesOptions as IcuPluralRulesOptions,
};
use icu_provider::{DataIdentifierBorrowed, DataMarker, DataProvider, DataRequest};
use icu_segmenter::{
    options::{SentenceBreakOptions, WordBreakOptions},
    GraphemeClusterSegmenter, GraphemeClusterSegmenterBorrowed, SentenceSegmenter, WordSegmenter,
};
use std::{any::TypeId, cell::RefCell, cmp::Ordering};
use writeable::{Part, PartsWrite, Writeable};

/// An error from structurally validating and canonicalizing an ECMA-402
/// Unicode locale identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocaleError {
    /// The input is not a valid Unicode locale identifier for ECMA-402.
    InvalidLanguageTag,
}

impl std::fmt::Display for LocaleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageTag => formatter.write_str("invalid Unicode locale identifier"),
        }
    }
}

impl std::error::Error for LocaleError {}

/// A structurally valid ECMA-402 locale identifier together with the ICU
/// locale used to access locale data.
///
/// ICU4X deliberately does not represent BCP 47 primary-language subtags of
/// five to eight letters. Keeping the canonical name separately preserves
/// their ECMA-402-observable spelling without inventing an ICU language code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalLocale {
    locale: IcuLocale,
    canonical: String,
}

impl CanonicalLocale {
    fn new(locale: IcuLocale) -> Self {
        let canonical = locale.to_string();
        Self { locale, canonical }
    }

    /// Builds a canonical locale from its ICU data locale and ECMA-402 name.
    ///
    /// This is primarily useful to hosts that apply locale options to an
    /// already-canonicalized tag. Hosts should prefer [`canonicalize`] for
    /// untrusted locale input.
    pub fn from_parts(locale: IcuLocale, canonical: impl Into<String>) -> Self {
        Self {
            locale,
            canonical: canonical.into(),
        }
    }

    /// Returns the ICU locale used for data lookup.
    pub fn locale(&self) -> &IcuLocale {
        &self.locale
    }

    /// Returns the ECMA-402 canonical identifier.
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// Splits the locale's data lookup representation from its canonical name.
    pub fn into_parts(self) -> (IcuLocale, String) {
        (self.locale, self.canonical)
    }
}

impl std::fmt::Display for CanonicalLocale {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.canonical.fmt(formatter)
    }
}

/// Canonicalizes an ECMA-402 Unicode locale identifier.
pub fn canonicalize(tag: &str) -> Result<CanonicalLocale, LocaleError> {
    let invalid = || LocaleError::InvalidLanguageTag;
    // ICU accepts underscores as separators and sorts/deduplicates variants.
    // ECMAScript requires hyphens and rejects repeated language variants.
    if tag.contains('_') {
        return Err(invalid());
    }
    let lower = tag.to_ascii_lowercase();
    let mut parts = lower.split('-');
    let language = parts.next().unwrap_or_default();
    if !matches!(language.len(), 2..=3 | 5..=8) {
        return Err(invalid());
    }
    let mut variants = std::collections::HashSet::new();
    for part in parts.take_while(|part| part.len() != 1) {
        if (part.len() >= 5 || part.len() == 4 && part.as_bytes()[0].is_ascii_digit())
            && !variants.insert(part)
        {
            return Err(invalid());
        }
    }

    // `posix` is structurally valid but not modelled by ICU4X. Preserve it for
    // ECMA-402 while querying ICU through its `und-posix` representation.
    let posix_language = tag.eq_ignore_ascii_case("posix");
    let mut parser_tag = if posix_language {
        "und-posix".to_owned()
    } else {
        tag.to_owned()
    };
    // ICU canonicalizes transformed-extension language tags and tfield order,
    // but its bundled aliases leave this UTS 35 tvalue untouched.
    let mut transformed = false;
    let mut subtags: Vec<_> = parser_tag.split('-').map(str::to_owned).collect();
    for index in 0..subtags.len() {
        if subtags[index].len() == 1 {
            transformed = subtags[index].eq_ignore_ascii_case("t");
            continue;
        }
        if transformed
            && subtags[index].eq_ignore_ascii_case("m0")
            && subtags
                .get(index + 1)
                .is_some_and(|value| value.eq_ignore_ascii_case("names"))
        {
            subtags[index + 1] = "prprname".into();
        }
    }
    parser_tag = subtags.join("-");

    let mut locale = IcuLocale::try_from_str(&parser_tag).map_err(|_| invalid())?;
    icu_locale::LocaleCanonicalizer::new_extended().canonicalize(&mut locale);
    canonicalize_unicode_keyword_aliases(&mut locale);

    Ok(if posix_language {
        CanonicalLocale::from_parts(locale, "posix")
    } else {
        CanonicalLocale::new(locale)
    })
}

/// Typed, host-neutral options for applying `Intl.Locale` changes to a tag.
///
/// An embedding host performs observable input coercion and option-property
/// ordering before it creates this value. This service owns structural
/// validation, subtag application and final canonicalization.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocaleOptions {
    /// Replaces the language subtag.
    pub language: Option<String>,
    /// Replaces the script subtag.
    pub script: Option<String>,
    /// Replaces the region subtag.
    pub region: Option<String>,
    /// Replaces the hyphen-separated variant list.
    pub variants: Option<String>,
    /// Sets Unicode key `ca`.
    pub calendar: Option<String>,
    /// Sets Unicode key `co`.
    pub collation: Option<String>,
    /// Sets Unicode key `hc`.
    pub hour_cycle: Option<String>,
    /// Sets Unicode key `kf`.
    pub case_first: Option<String>,
    /// Sets Unicode key `kn`.
    pub numeric: Option<bool>,
    /// Sets Unicode key `nu`.
    pub numbering_system: Option<String>,
    /// Sets Unicode key `fw`; numeric weekday forms are accepted.
    pub first_day_of_week: Option<String>,
}

/// A structurally invalid [`LocaleOptions`] field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocaleOptionError {
    /// The `language` option was not a language subtag.
    InvalidLanguage,
    /// The `script` option was not a script subtag.
    InvalidScript,
    /// The `region` option was not a region subtag.
    InvalidRegion,
    /// The `variants` option was malformed or duplicated a variant.
    InvalidVariants,
    /// The `calendar` option was not a Unicode type.
    InvalidCalendar,
    /// The `collation` option was not a Unicode type.
    InvalidCollation,
    /// The `hourCycle` option was unsupported.
    InvalidHourCycle,
    /// The `caseFirst` option was unsupported.
    InvalidCaseFirst,
    /// The `numberingSystem` option was not a Unicode type.
    InvalidNumberingSystem,
    /// The `firstDayOfWeek` option was not a Unicode type.
    InvalidFirstDayOfWeek,
}

impl std::fmt::Display for LocaleOptionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let option = match self {
            Self::InvalidLanguage => "language",
            Self::InvalidScript => "script",
            Self::InvalidRegion => "region",
            Self::InvalidVariants => "variants",
            Self::InvalidCalendar => "calendar",
            Self::InvalidCollation => "collation",
            Self::InvalidHourCycle => "hourCycle",
            Self::InvalidCaseFirst => "caseFirst",
            Self::InvalidNumberingSystem => "numberingSystem",
            Self::InvalidFirstDayOfWeek => "firstDayOfWeek",
        };
        write!(formatter, "invalid {option} option")
    }
}

impl std::error::Error for LocaleOptionError {}

/// Applies typed locale options and returns the fully canonicalized result.
pub fn apply_locale_options(
    initial: &CanonicalLocale,
    options: &LocaleOptions,
) -> Result<CanonicalLocale, LocaleOptionError> {
    let initial_name = initial.as_str();
    let mut locale = initial.locale().clone();
    if let Some(language) = &options.language {
        locale.id.language = language
            .parse::<icu_locale_core::subtags::Language>()
            .map_err(|_| LocaleOptionError::InvalidLanguage)?;
    }
    if let Some(script) = &options.script {
        locale.id.script = Some(
            script
                .parse::<icu_locale_core::subtags::Script>()
                .map_err(|_| LocaleOptionError::InvalidScript)?,
        );
    }
    if let Some(region) = &options.region {
        locale.id.region = Some(
            region
                .parse::<icu_locale_core::subtags::Region>()
                .map_err(|_| LocaleOptionError::InvalidRegion)?,
        );
    }
    if let Some(variants) = &options.variants {
        if variants.is_empty() {
            return Err(LocaleOptionError::InvalidVariants);
        }
        let mut values = variants
            .split('-')
            .map(|variant| {
                variant
                    .parse::<icu_locale_core::subtags::Variant>()
                    .map_err(|_| LocaleOptionError::InvalidVariants)
            })
            .collect::<Result<Vec<_>, _>>()?;
        values.sort();
        if values.windows(2).any(|values| values[0] == values[1]) {
            return Err(LocaleOptionError::InvalidVariants);
        }
        locale.id.variants = icu_locale_core::subtags::Variants::from_vec_unchecked(values);
    }
    set_locale_keyword(
        &mut locale,
        "ca",
        options.calendar.as_deref(),
        LocaleOptionError::InvalidCalendar,
    )?;
    set_locale_keyword(
        &mut locale,
        "co",
        options.collation.as_deref(),
        LocaleOptionError::InvalidCollation,
    )?;
    if let Some(hour_cycle) = &options.hour_cycle {
        if !matches!(hour_cycle.as_str(), "h11" | "h12" | "h23" | "h24") {
            return Err(LocaleOptionError::InvalidHourCycle);
        }
        set_locale_keyword(
            &mut locale,
            "hc",
            Some(hour_cycle),
            LocaleOptionError::InvalidHourCycle,
        )?;
    }
    if let Some(case_first) = &options.case_first {
        if !matches!(case_first.as_str(), "upper" | "lower" | "false") {
            return Err(LocaleOptionError::InvalidCaseFirst);
        }
        set_locale_keyword(
            &mut locale,
            "kf",
            Some(case_first),
            LocaleOptionError::InvalidCaseFirst,
        )?;
    }
    if let Some(numeric) = options.numeric {
        let key = "kn".parse().expect("kn is a Unicode locale key");
        let value = if numeric {
            icu_locale_core::extensions::unicode::Value::default()
        } else {
            "false".parse().expect("false is a Unicode locale value")
        };
        locale.extensions.unicode.keywords.set(key, value);
    }
    set_locale_keyword(
        &mut locale,
        "nu",
        options.numbering_system.as_deref(),
        LocaleOptionError::InvalidNumberingSystem,
    )?;
    if let Some(first_day) = &options.first_day_of_week {
        let first_day = match first_day.as_str() {
            "0" | "7" => "sun",
            "1" => "mon",
            "2" => "tue",
            "3" => "wed",
            "4" => "thu",
            "5" => "fri",
            "6" => "sat",
            value => value,
        };
        set_locale_keyword(
            &mut locale,
            "fw",
            Some(first_day),
            LocaleOptionError::InvalidFirstDayOfWeek,
        )?;
    }

    let serialized = locale.to_string();
    if initial_name == "posix" && serialized == "und-posix" {
        Ok(CanonicalLocale::from_parts(locale, initial_name))
    } else {
        Ok(canonicalize(&serialized).expect("applied options produce a structurally valid locale"))
    }
}

fn set_locale_keyword(
    locale: &mut IcuLocale,
    key: &str,
    value: Option<&str>,
    error: LocaleOptionError,
) -> Result<(), LocaleOptionError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_empty()
        || !value.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return Err(error);
    }
    let value = value.parse().map_err(|_| error)?;
    locale.extensions.unicode.keywords.set(
        key.parse().expect("the supplied key is static and valid"),
        value,
    );
    Ok(())
}

fn canonicalize_unicode_keyword_aliases(locale: &mut IcuLocale) {
    // ICU canonicalizes language identifiers but deliberately leaves several
    // Unicode keyword aliases to the consumer. ECMA-402 exposes their UTS 35
    // canonical spelling through Intl.Locale and Intl.getCanonicalLocales.
    let calendar: icu_locale_core::extensions::unicode::Key = "ca".parse().unwrap();
    if locale
        .extensions
        .unicode
        .keywords
        .get(&calendar)
        .is_some_and(|value| value.to_string() == "islamicc")
    {
        locale
            .extensions
            .unicode
            .keywords
            .set(calendar, "islamic-civil".parse().unwrap());
    }
    if locale
        .extensions
        .unicode
        .keywords
        .get(&calendar)
        .is_some_and(|value| value.to_string() == "ethiopic-amete-alem")
    {
        locale
            .extensions
            .unicode
            .keywords
            .set(calendar, "ethioaa".parse().unwrap());
    }

    // ICU intentionally preserves several CLDR aliases. ECMA-402 exposes
    // their canonical UTS 35 spellings, including boolean-key removal.
    for key_name in ["kb", "kc", "kh", "kk", "kn", "ks", "ms", "tz"] {
        let key = key_name.parse().unwrap();
        let Some(value) = locale
            .extensions
            .unicode
            .keywords
            .get(&key)
            .map(ToString::to_string)
        else {
            continue;
        };
        let replacement = match (key_name, value.as_str()) {
            ("kb" | "kc" | "kh" | "kk" | "kn", "yes") => None,
            ("ks", "primary") => Some("level1"),
            ("ks", "tertiary") => Some("level3"),
            ("ms", "imperial") => Some("uksystem"),
            ("tz", "cnckg") => Some("cnsha"),
            ("tz", "eire") => Some("iedub"),
            ("tz", "est") => Some("papty"),
            ("tz", "gmt0") => Some("gmt"),
            ("tz", "uct" | "zulu") => Some("utc"),
            _ => continue,
        };
        match replacement {
            Some(value) => {
                locale
                    .extensions
                    .unicode
                    .keywords
                    .set(key, value.parse().unwrap());
            }
            None => {
                // A canonical boolean `true` type is represented by a key
                // without a type (`-u-kn`, not an omitted `kn` key).
                locale
                    .extensions
                    .unicode
                    .keywords
                    .set(key, icu_locale_core::extensions::unicode::Value::default());
            }
        }
    }
}

/// Whether the bundled ECMA-402 data has coverage for the locale's language.
///
/// ICU4X compacts locale data whose values are identical to a parent locale.
/// The registry keeps those ECMA-402 locales available even when a particular
/// data marker resolves through that parent.
pub fn supports_locale_language(locale: &IcuLocale) -> bool {
    const LANGUAGES: &str = "af am ar as az be bg bn bo br bs ca ceb chr cs cy da de dsb dz ee el en eo es et fa ff fi fil fo fr fy ga gl gu gv ha haw he hi hr hsb hu hy id ig is it ja ka kk kl km kn ko kok ku ky la lb lkt ln lo lt lv mk ml mn mr ms mt my nb ne nl nn no om or pa pl ps pt ro ru sa se si sk sl so sq sr sv sw ta te th tk to tr ug uk ur uz vi wae wo xh yi yo zh zu";
    LANGUAGES
        .split(' ')
        .any(|language| locale.id.language.as_str() == language)
}

/// Whether the bundled collation data supports the locale's language.
pub fn supports_collation_locale(locale: &IcuLocale) -> bool {
    supports_locale_language(locale)
}

/// Returns a Unicode extension keyword's canonical ICU value, if present.
pub fn unicode_keyword(locale: &IcuLocale, name: &str) -> Option<String> {
    let key: icu_locale_core::extensions::unicode::Key = name.parse().ok()?;
    locale
        .extensions
        .unicode
        .keywords
        .get(&key)
        .map(ToString::to_string)
}

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
/// This is intentionally a small deterministic dataset rather than a claim to
/// expose all CLDR records. It centralizes the service data BlueJS currently
/// advertises so another host can make the same choices without a Realm.
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

/// Returns the deterministic locale-information dataset for a canonical tag.
pub fn locale_information(locale: &CanonicalLocale) -> LocaleInformation {
    let locale = locale.locale();
    let language = locale.id.language.as_str();
    let calendars = vec![unicode_keyword(locale, "ca").unwrap_or_else(|| "gregory".into())];
    let collations = vec![unicode_keyword(locale, "co")
        .filter(|value| value != "standard" && value != "search")
        .unwrap_or_else(|| "emoji".into())];
    let hour_cycles = vec![unicode_keyword(locale, "hc").unwrap_or_else(|| {
        if language == "en" {
            "h12".into()
        } else {
            "h23".into()
        }
    })];
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
    let first_day = match unicode_keyword(locale, "fw").as_deref() {
        Some("mon") => 1,
        Some("tue") => 2,
        Some("wed") => 3,
        Some("thu") => 4,
        Some("fri") => 5,
        Some("sat") => 6,
        Some("sun") => 7,
        _ if locale
            .id
            .region
            .as_ref()
            .is_some_and(|region| matches!(region.as_str(), "US" | "CA" | "JP")) =>
        {
            7
        }
        _ => 1,
    };
    LocaleInformation {
        calendars,
        collations,
        hour_cycles,
        numbering_systems,
        text_direction,
        time_zones,
        week_info: WeekInfo {
            first_day,
            weekend: vec![6, 7],
        },
    }
}

/// Applies CLDR likely-subtag maximization to a canonical locale.
///
/// Unicode extensions remain intact. The ECMA-402-valid `posix` primary
/// language is represented specially because ICU4X does not model it as a
/// language identifier, so it is returned unchanged.
pub fn maximize_locale(locale: &CanonicalLocale) -> CanonicalLocale {
    transform_locale(locale, true)
}

/// Applies CLDR likely-subtag minimization to a canonical locale.
///
/// Unicode extensions remain intact. See [`maximize_locale`] for the `posix`
/// representation detail.
pub fn minimize_locale(locale: &CanonicalLocale) -> CanonicalLocale {
    transform_locale(locale, false)
}

fn transform_locale(locale: &CanonicalLocale, maximize: bool) -> CanonicalLocale {
    if locale.as_str() == "posix" {
        return locale.clone();
    }
    let mut transformed = locale.locale().clone();
    let expander = icu_locale::LocaleExpander::new_extended();
    if maximize {
        expander.maximize(&mut transformed.id);
    } else {
        expander.minimize(&mut transformed.id);
    }
    canonicalize(&transformed.to_string()).expect("a transformed canonical locale remains valid")
}

/// Whether a collation is available for an already-selected locale.
pub fn supports_collation(locale: &IcuLocale, collation: &str) -> bool {
    let language = locale.id.language.to_string();
    matches!(collation, "emoji" | "eor")
        || matches!(
            (language.as_str(), collation),
            ("de", "phonebk")
                | ("es" | "fi" | "sv" | "bn" | "kn", "trad")
                | ("zh", "pinyin" | "stroke" | "unihan" | "zhuyin")
                | ("ja" | "ko", "unihan")
                | ("ko", "searchjl")
                | ("si", "dict")
                | ("ar", "compat")
        )
}

/// The locale matching algorithm selected by an ECMA-402 service.
///
/// The bundled collation data advertises language-level support, so best-fit
/// currently has the same deterministic result as lookup. Keeping it explicit
/// lets hosts share the selection boundary when the available data grows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocaleMatcher {
    /// The RFC 4647 lookup-style matching policy.
    #[default]
    Lookup,
    /// The ECMA-402 best-fit matching policy.
    BestFit,
}

/// One canonical locale considered during collation negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollationLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl CollationLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled collation data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of collation locale negotiation.
///
/// This value is deliberately host-neutral and read-only. It lets a debugger
/// explain fallback and data support without exposing ICU4X implementation
/// types or constructing a JavaScript Realm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollationLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<CollationLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl CollationLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[CollationLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for the collation service.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Negotiates a requested locale list against the bundled collation service.
///
/// Region and script subtags remain attached to a supported request so ICU4X
/// can select its CLDR fallback data. Unicode extensions are retained for
/// later service-option resolution. If no request is supported, the stable
/// service default is `en-US`.
pub fn negotiate_collation_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CollationLocaleNegotiation {
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| CollationLocaleCandidate {
            supported: supports_collation_locale(requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    CollationLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Resolves one requested locale against the bundled collation service.
///
/// Region and script subtags remain attached to a supported request so ICU4X
/// can select its CLDR fallback data. Unicode extensions are retained here for
/// later service-option resolution. If no request is supported, the stable
/// service default is `en-US`.
pub fn resolve_collation_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_collation_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns the requested locales supported by the bundled collation service.
///
/// The returned values preserve canonical request spelling, as required by
/// `Intl.Collator.supportedLocalesOf`; negotiation only decides support and
/// does not replace a request with the service default.
pub fn supported_collation_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    negotiate_collation_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
}

/// The `usage` option of an ECMA-402 collator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CollatorUsage {
    /// A sorting collator.
    #[default]
    Sort,
    /// A text-search collator.
    Search,
}

/// The `caseFirst` option of an ECMA-402 collator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseFirst {
    /// Sort uppercase before lowercase where the tailoring supports it.
    Upper,
    /// Sort lowercase before uppercase where the tailoring supports it.
    Lower,
    /// Use the locale default case ordering.
    False,
}

/// The `sensitivity` option of an ECMA-402 collator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Sensitivity {
    /// Compare base characters only.
    Base,
    /// Compare base characters and accents.
    Accent,
    /// Compare base characters and case.
    Case,
    /// Compare all supported collation distinctions.
    #[default]
    Variant,
}

/// Host-neutral input for constructing a collation service.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CollatorOptions {
    /// The desired locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// Whether the collator is used for sorting or searching.
    pub usage: CollatorUsage,
    /// An optional UTS 35 collation type. Unsupported values resolve to the
    /// locale default, per ECMA-402.
    pub collation: Option<String>,
    /// Overrides the locale's `kn` extension when present.
    pub numeric: Option<bool>,
    /// Overrides the locale's `kf` extension when present.
    pub case_first: Option<CaseFirst>,
    /// Controls collation strength and case level.
    pub sensitivity: Sensitivity,
    /// Overrides the locale default punctuation handling when present.
    pub ignore_punctuation: Option<bool>,
}

/// ECMAScript-observable data resolved by a collation service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCollatorOptions {
    /// The negotiated locale with only supported, non-overridden Unicode keys.
    pub locale: String,
    /// The selected usage.
    pub usage: CollatorUsage,
    /// The selected sensitivity.
    pub sensitivity: Sensitivity,
    /// Whether punctuation is ignored.
    pub ignore_punctuation: bool,
    /// The selected collation type, or `default`.
    pub collation: String,
    /// Whether numeric collation is enabled.
    pub numeric: bool,
    /// The selected case ordering.
    pub case_first: CaseFirst,
}

/// A failure while constructing a collator from the bundled ICU4X data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollatorError {
    /// The selected built-in collation data was unavailable.
    DataUnavailable,
}

impl std::fmt::Display for CollatorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("collation data is unavailable"),
        }
    }
}

impl std::error::Error for CollatorError {}

/// A host-neutral UTF-16 collation service backed by ICU4X.
///
/// The service accepts and compares UTF-16 code units directly. Embedders are
/// responsible only for their own input coercion and error presentation.
pub struct Collator {
    algorithm: CollatorBorrowed<'static>,
    negotiation: CollationLocaleNegotiation,
    resolved: ResolvedCollatorOptions,
    german_search: bool,
}

impl Collator {
    /// Constructs a collator after locale negotiation and option resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: CollatorOptions,
    ) -> Result<Self, CollatorError> {
        let negotiation = negotiate_collation_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let selected_locale = selected.locale();
        let mut resolved_locale = IcuLocale::from(selected_locale.id.clone());
        let mut algorithm_locale = resolved_locale.clone();
        let mut selected_collation = "default".to_owned();

        for (key, option) in [
            (
                "co",
                options.collation.as_deref().map(str::to_ascii_lowercase),
            ),
            (
                "kf",
                options.case_first.map(|value| match value {
                    CaseFirst::Upper => "upper".to_owned(),
                    CaseFirst::Lower => "lower".to_owned(),
                    CaseFirst::False => "false".to_owned(),
                }),
            ),
            ("kn", options.numeric.map(|value| value.to_string())),
        ] {
            let extension = unicode_keyword(selected_locale, key).map(|value| {
                if key == "kn" && value.is_empty() {
                    "true".to_owned()
                } else {
                    value
                }
            });
            let valid = |value: &str| match key {
                "co" => {
                    options.usage == CollatorUsage::Sort
                        && supports_collation(selected_locale, value)
                }
                "kf" => matches!(value, "upper" | "lower" | "false"),
                "kn" => matches!(value, "true" | "false"),
                _ => unreachable!("the Collator key list is fixed"),
            };
            let extension = extension.filter(|value| valid(value));
            let choice = option
                .filter(|value| valid(value))
                .or_else(|| extension.clone());
            if let Some(value) = choice {
                if extension.as_ref() == Some(&value) {
                    resolved_locale
                        .extensions
                        .unicode
                        .keywords
                        .set(key.parse().unwrap(), value.parse().unwrap());
                }
                algorithm_locale
                    .extensions
                    .unicode
                    .keywords
                    .set(key.parse().unwrap(), value.parse().unwrap());
                if key == "co" {
                    selected_collation = value;
                }
            }
        }

        let mut preferences: CollatorPreferences = (&algorithm_locale).into();
        if options.usage == CollatorUsage::Search {
            preferences.collation_type = Some(CollationType::Search);
        }
        let mut icu_options = IcuCollatorOptions::default();
        icu_options.strength = Some(match options.sensitivity {
            Sensitivity::Base | Sensitivity::Case => Strength::Primary,
            Sensitivity::Accent => Strength::Secondary,
            Sensitivity::Variant => Strength::Tertiary,
        });
        icu_options.case_level = Some(if options.sensitivity == Sensitivity::Case {
            CaseLevel::On
        } else {
            CaseLevel::Off
        });
        let ignore_punctuation = options
            .ignore_punctuation
            .unwrap_or_else(|| selected_locale.id.language.as_str() == "th");
        icu_options.alternate_handling = Some(if ignore_punctuation {
            AlternateHandling::Shifted
        } else {
            AlternateHandling::NonIgnorable
        });
        icu_options.max_variable = Some(MaxVariable::Punctuation);
        let algorithm = CollatorBorrowed::try_new(preferences, icu_options)
            .map_err(|_| CollatorError::DataUnavailable)?;
        let icu_resolved = algorithm.resolved_options();
        let resolved = ResolvedCollatorOptions {
            locale: resolved_locale.to_string(),
            usage: options.usage,
            sensitivity: options.sensitivity,
            ignore_punctuation,
            collation: selected_collation,
            numeric: icu_resolved.numeric == CollationNumericOrdering::True,
            case_first: match icu_resolved.case_first {
                CollationCaseFirst::Upper => CaseFirst::Upper,
                CollationCaseFirst::Lower => CaseFirst::Lower,
                _ => CaseFirst::False,
            },
        };
        let german_search =
            options.usage == CollatorUsage::Search && selected_locale.id.language.as_str() == "de";
        Ok(Self {
            algorithm,
            negotiation,
            resolved,
            german_search,
        })
    }

    /// Compares two UTF-16 strings according to the resolved collation.
    pub fn compare_utf16(&self, left: &[u16], right: &[u16]) -> Ordering {
        if self.german_search {
            let left = german_search_fold(left);
            let right = german_search_fold(right);
            self.algorithm.compare_utf16(&left, &right)
        } else {
            self.algorithm.compare_utf16(left, right)
        }
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedCollatorOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &CollationLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len() + self.resolved.collation.len()
    }
}

fn german_search_fold(value: &[u16]) -> Vec<u16> {
    let mut units = Vec::with_capacity(value.len());
    for unit in value {
        match unit {
            0x00c4 => units.extend([b'A' as u16, b'E' as u16]),
            0x00d6 => units.extend([b'O' as u16, b'E' as u16]),
            0x00dc => units.extend([b'U' as u16, b'E' as u16]),
            0x00df => units.extend([b's' as u16, b's' as u16]),
            0x00e4 => units.extend([b'a' as u16, b'e' as u16]),
            0x00f6 => units.extend([b'o' as u16, b'e' as u16]),
            0x00fc => units.extend([b'u' as u16, b'e' as u16]),
            unit => units.push(*unit),
        }
    }
    units
}

/// Whether the bundled decimal data has a locale-specific fallback for a
/// locale.
///
/// ICU4X falls all unknown languages back to `und`; that fallback is useful
/// for internal data loading but is not an ECMA-402 available-locale match.
/// A language is therefore supported when the decimal-symbol data resolves to
/// the same primary language (possibly after dropping region or script).
pub fn supports_number_format_locale(locale: &IcuLocale) -> bool {
    if supports_locale_language(locale) {
        return true;
    }
    let requested = icu_provider::DataLocale::from(locale);
    let response = <DecimalData as DataProvider<DecimalSymbolsV1>>::load(
        &DecimalData,
        DataRequest {
            id: DataIdentifierBorrowed::for_locale(&requested),
            metadata: Default::default(),
        },
    );
    match response {
        Ok(response) => response
            .metadata
            .locale
            .is_none_or(|resolved| resolved.language == requested.language),
        Err(_) => false,
    }
}

/// One canonical locale considered during decimal number-format negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberFormatLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl NumberFormatLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled decimal data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of decimal number-format locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberFormatLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<NumberFormatLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl NumberFormatLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[NumberFormatLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for decimal formatting.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Negotiates a requested locale list against bundled decimal-format data.
///
/// Region and script subtags remain attached to a supported request so ICU4X
/// can select CLDR data. Unicode extensions are likewise retained for the
/// formatter's option resolution. If no requested locale is supported, the
/// stable service default is `en-US`.
pub fn negotiate_number_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> NumberFormatLocaleNegotiation {
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| NumberFormatLocaleCandidate {
            supported: supports_number_format_locale(requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    NumberFormatLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Resolves one requested locale against bundled decimal-format data.
pub fn resolve_number_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_number_format_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns the requested locales supported by the bundled decimal service.
pub fn supported_number_format_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    negotiate_number_format_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
}

/// The decimal grouping policy selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberGrouping {
    /// Apply the locale's ordinary grouping policy.
    #[default]
    Auto,
    /// Suppress grouping separators.
    Never,
    /// Apply grouping whenever the locale data permits it.
    Always,
    /// Group only when at least two digits precede the last grouping separator.
    Min2,
}

impl From<NumberGrouping> for GroupingStrategy {
    fn from(value: NumberGrouping) -> Self {
        match value {
            NumberGrouping::Auto => Self::Auto,
            NumberGrouping::Never => Self::Never,
            NumberGrouping::Always => Self::Always,
            NumberGrouping::Min2 => Self::Min2,
        }
    }
}

/// Host-neutral options for the decimal-style finite-number subset of
/// `Intl.NumberFormat`.
///
/// Currency, unit, compact/scientific notation, range formatting and
/// non-finite-number symbols are separate service slices and are deliberately
/// not represented by this initial decimal formatter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NumberFormatOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// When locale-specific grouping separators are rendered.
    pub use_grouping: NumberGrouping,
    /// The minimum number of fractional digits after rounding.
    pub minimum_fraction_digits: Option<u8>,
    /// The maximum number of fractional digits after rounding.
    pub maximum_fraction_digits: Option<u8>,
}

/// ECMAScript-observable data resolved by the decimal number formatter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNumberFormatOptions {
    /// The negotiated locale, including its supported Unicode extensions.
    pub locale: String,
    /// The actual ICU4X decimal numbering system.
    pub numbering_system: String,
    /// The selected grouping policy.
    pub use_grouping: NumberGrouping,
    /// The resolved minimum number of fraction digits.
    pub minimum_fraction_digits: u8,
    /// The resolved maximum number of fraction digits.
    pub maximum_fraction_digits: u8,
}

/// A failure while constructing or using a decimal number formatter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormatError {
    /// The selected decimal data was unavailable.
    DataUnavailable,
    /// A fraction-digit option exceeded ECMA-402's supported range of 0–100.
    FractionDigitsOutOfRange,
    /// The requested minimum fraction digits exceeded the maximum.
    IncompatibleFractionDigits,
    /// A decimal input was not a finite, base-10 decimal string.
    InvalidDecimal,
    /// An IEEE-754 input was `NaN` or infinite.
    NonFiniteNumber,
}

impl std::fmt::Display for NumberFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("decimal data is unavailable"),
            Self::FractionDigitsOutOfRange => {
                formatter.write_str("fraction digits must be in the range 0 through 100")
            }
            Self::IncompatibleFractionDigits => {
                formatter.write_str("minimum fraction digits exceed maximum fraction digits")
            }
            Self::InvalidDecimal => formatter.write_str("invalid finite decimal input"),
            Self::NonFiniteNumber => formatter.write_str("number must be finite"),
        }
    }
}

impl std::error::Error for NumberFormatError {}

/// A host-neutral, locale-sensitive decimal formatter.
///
/// Embedders retain ECMAScript coercion, `NaN`/infinity handling and object
/// semantics. Finite decimal formatting, option resolution and all ICU4X data
/// selection occur here.
pub struct NumberFormat {
    formatter: DecimalFormatter,
    negotiation: NumberFormatLocaleNegotiation,
    resolved: ResolvedNumberFormatOptions,
}

impl NumberFormat {
    /// Constructs a decimal formatter after locale negotiation and option
    /// resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: NumberFormatOptions,
    ) -> Result<Self, NumberFormatError> {
        let (minimum_fraction_digits, maximum_fraction_digits) = resolve_fraction_digits(options)?;
        let negotiation = negotiate_number_format_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let provider = NumberingSystemInspectionProvider::default();
        let mut formatter_options = DecimalFormatterOptions::default();
        formatter_options.grouping_strategy = Some(options.use_grouping.into());
        let mut preferences: DecimalFormatterPreferences = selected.locale().into();
        if let Some(numbering_system) = unicode_keyword(selected.locale(), "nu") {
            let value = numbering_system
                .parse::<icu_locale_core::extensions::unicode::Value>()
                .map_err(|_| NumberFormatError::DataUnavailable)?;
            preferences.numbering_system = Some(
                NumberingSystem::try_from(value).map_err(|_| NumberFormatError::DataUnavailable)?,
            );
        }
        let formatter =
            DecimalFormatter::try_new_unstable(&provider, preferences, formatter_options)
                .map_err(|_| NumberFormatError::DataUnavailable)?;
        let numbering_system = provider
            .numbering_system
            .into_inner()
            .unwrap_or_else(|| "latn".into());
        let resolved = ResolvedNumberFormatOptions {
            locale: selected.as_str().into(),
            numbering_system,
            use_grouping: options.use_grouping,
            minimum_fraction_digits,
            maximum_fraction_digits,
        };
        Ok(Self {
            formatter,
            negotiation,
            resolved,
        })
    }

    /// Formats a finite base-10 decimal string.
    ///
    /// The string uses an optional ASCII sign, ASCII digits and an optional
    /// decimal point. It is a host-neutral boundary that does not expose an
    /// ICU4X decimal type to callers.
    pub fn format_decimal(&self, value: &str) -> Result<String, NumberFormatError> {
        let decimal =
            Decimal::try_from_str(value).map_err(|_| NumberFormatError::InvalidDecimal)?;
        Ok(self.format_decimal_value(decimal))
    }

    /// Formats a finite IEEE-754 number using its shortest round-trippable
    /// decimal representation before applying ECMA-402 fraction-digit rules.
    pub fn format_f64(&self, value: f64) -> Result<String, NumberFormatError> {
        let decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
            .map_err(|_| NumberFormatError::NonFiniteNumber)?;
        Ok(self.format_decimal_value(decimal))
    }

    fn format_decimal_value(&self, mut value: Decimal) -> String {
        value.round_with_mode(
            -(self.resolved.maximum_fraction_digits as i16),
            SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfExpand),
        );
        value.pad_end(-(self.resolved.minimum_fraction_digits as i16));
        self.formatter.format(&value).to_string()
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedNumberFormatOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &NumberFormatLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.resolved.locale.len()
            + self.resolved.numbering_system.len()
    }
}

fn resolve_fraction_digits(options: NumberFormatOptions) -> Result<(u8, u8), NumberFormatError> {
    let minimum = options.minimum_fraction_digits.unwrap_or(0);
    let maximum = options
        .maximum_fraction_digits
        .unwrap_or_else(|| minimum.max(3));
    if minimum > 100 || maximum > 100 {
        return Err(NumberFormatError::FractionDigitsOutOfRange);
    }
    if minimum > maximum {
        return Err(NumberFormatError::IncompatibleFractionDigits);
    }
    Ok((minimum, maximum))
}

/// Captures the exact numbering system selected by ICU4X during construction.
///
/// `DecimalFormatter` deliberately hides its data-provider internals. The
/// provider protocol nevertheless defines the final `DecimalDigitsV1` request
/// as the resolved numbering system, so this transparent wrapper makes the
/// resolved ECMA-402 value observable without exposing ICU4X types to callers.
#[derive(Default)]
struct NumberingSystemInspectionProvider {
    numbering_system: RefCell<Option<String>>,
}

impl<M> DataProvider<M> for NumberingSystemInspectionProvider
where
    M: DataMarker,
    DecimalData: DataProvider<M>,
{
    fn load(
        &self,
        request: DataRequest,
    ) -> Result<icu_provider::DataResponse<M>, icu_provider::DataError> {
        if TypeId::of::<M>() == TypeId::of::<DecimalDigitsV1>() {
            self.numbering_system
                .replace(Some(request.id.marker_attributes.as_str().into()));
        }
        DecimalData.load(request)
    }
}

/// The CLDR plural-rule family selected by `Intl.PluralRules`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PluralRuleType {
    /// Select a category for an ordinary quantity.
    #[default]
    Cardinal,
    /// Select a category for an ordinal position.
    Ordinal,
}

impl From<PluralRuleType> for IcuPluralRuleType {
    fn from(value: PluralRuleType) -> Self {
        match value {
            PluralRuleType::Cardinal => Self::Cardinal,
            PluralRuleType::Ordinal => Self::Ordinal,
        }
    }
}

/// A CLDR plural category returned by `Intl.PluralRules`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluralCategory {
    /// The `zero` category.
    Zero,
    /// The `one` category.
    One,
    /// The `two` category.
    Two,
    /// The `few` category.
    Few,
    /// The `many` category.
    Many,
    /// The `other` catch-all category.
    Other,
}

impl From<IcuPluralCategory> for PluralCategory {
    fn from(value: IcuPluralCategory) -> Self {
        match value {
            IcuPluralCategory::Zero => Self::Zero,
            IcuPluralCategory::One => Self::One,
            IcuPluralCategory::Two => Self::Two,
            IcuPluralCategory::Few => Self::Few,
            IcuPluralCategory::Many => Self::Many,
            IcuPluralCategory::Other => Self::Other,
        }
    }
}

/// One canonical locale considered during plural-rule negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluralRulesLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl PluralRulesLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled plural-rule data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of plural-rule locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluralRulesLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<PluralRulesLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl PluralRulesLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[PluralRulesLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for plural-rule evaluation.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Negotiates a requested locale list against bundled plural-rule data.
///
/// Plural data follows the shared compiled-language registry. Script and
/// region subtags remain attached to the selected locale for future data
/// tailoring; if none match, the stable service default is `en-US`.
pub fn negotiate_plural_rules_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> PluralRulesLocaleNegotiation {
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| PluralRulesLocaleCandidate {
            supported: supports_locale_language(requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    PluralRulesLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Resolves one requested locale against bundled plural-rule data.
pub fn resolve_plural_rules_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_plural_rules_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns the requested locales supported by the bundled plural-rule service.
pub fn supported_plural_rules_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    negotiate_plural_rules_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
}

/// Host-neutral options for constructing an `Intl.PluralRules` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PluralRulesOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// Whether to evaluate cardinal or ordinal rules.
    pub rule_type: PluralRuleType,
}

/// ECMAScript-observable data resolved by a plural-rule service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPluralRulesOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The selected plural-rule family.
    pub rule_type: PluralRuleType,
}

/// A failure while constructing or evaluating plural rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluralRulesError {
    /// The selected plural-rule data was unavailable.
    DataUnavailable,
    /// A decimal input was not a finite, base-10 decimal string.
    InvalidDecimal,
    /// An IEEE-754 input was `NaN` or infinite.
    NonFiniteNumber,
}

impl std::fmt::Display for PluralRulesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("plural-rule data is unavailable"),
            Self::InvalidDecimal => formatter.write_str("invalid finite decimal input"),
            Self::NonFiniteNumber => formatter.write_str("number must be finite"),
        }
    }
}

impl std::error::Error for PluralRulesError {}

/// CLDR rules not carried by the compact ICU plural marker bundled with this
/// build. Keeping the exceptional rule at the host boundary means both
/// `select` and `resolvedOptions().pluralCategories` see the same data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SupplementalPluralRules {
    Manx,
}

impl SupplementalPluralRules {
    fn select(self, value: &str) -> PluralCategory {
        match self {
            // CLDR cardinal rules for `gv`: decimals are `many`; for integer
            // operands, the final digit yields `one`/`two`, and multiples of
            // 20 yield `few`.
            Self::Manx => {
                let value = value.trim_start_matches(['+', '-']);
                if value.contains('.') {
                    return PluralCategory::Many;
                }
                let digits = value.as_bytes();
                let last = digits.iter().rev().find(|byte| byte.is_ascii_digit());
                let penultimate = digits
                    .iter()
                    .rev()
                    .filter(|byte| byte.is_ascii_digit())
                    .nth(1);
                let last = last.map_or(0, |byte| byte - b'0');
                let modulo_hundred = penultimate.map_or(last, |byte| (byte - b'0') * 10 + last);
                if last == 1 {
                    PluralCategory::One
                } else if last == 2 {
                    PluralCategory::Two
                } else if matches!(modulo_hundred, 0 | 20 | 40 | 60 | 80) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            }
        }
    }
}

/// A host-neutral `Intl.PluralRules` service backed by ICU4X.
///
/// A decimal string preserves the visible fraction digits CLDR rules need to
/// distinguish values such as `1` and `1.0`. Embedders retain ECMAScript
/// coercion and later digit-option rounding semantics at their public
/// boundary.
pub struct PluralRules {
    rules: IcuPluralRules,
    supplemental: Option<SupplementalPluralRules>,
    negotiation: PluralRulesLocaleNegotiation,
    resolved: ResolvedPluralRulesOptions,
}

impl PluralRules {
    /// Constructs plural rules after locale negotiation and option resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: PluralRulesOptions,
    ) -> Result<Self, PluralRulesError> {
        let negotiation = negotiate_plural_rules_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let preferences = selected.locale().into();
        let rules = IcuPluralRules::try_new(
            preferences,
            IcuPluralRulesOptions::from(IcuPluralRuleType::from(options.rule_type)),
        )
        .map_err(|_| PluralRulesError::DataUnavailable)?;
        Ok(Self {
            rules,
            supplemental: (selected.locale().id.language.as_str() == "gv")
                .then_some(SupplementalPluralRules::Manx),
            negotiation,
            resolved: ResolvedPluralRulesOptions {
                locale: selected.as_str().into(),
                rule_type: options.rule_type,
            },
        })
    }

    /// Selects a plural category for a finite base-10 decimal string.
    pub fn select_decimal(&self, value: &str) -> Result<PluralCategory, PluralRulesError> {
        let decimal = Decimal::try_from_str(value).map_err(|_| PluralRulesError::InvalidDecimal)?;
        if let Some(supplemental) = self.supplemental {
            return Ok(supplemental.select(value));
        }
        Ok(self.rules.category_for(&decimal).into())
    }

    /// Selects a plural category for a finite IEEE-754 number.
    ///
    /// This representation intentionally has no visible trailing fraction
    /// zeros; callers that need those operands should use [`select_decimal`](Self::select_decimal).
    pub fn select_f64(&self, value: f64) -> Result<PluralCategory, PluralRulesError> {
        let decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
            .map_err(|_| PluralRulesError::NonFiniteNumber)?;
        if let Some(supplemental) = self.supplemental {
            return Ok(supplemental.select(&value.to_string()));
        }
        Ok(self.rules.category_for(&decimal).into())
    }

    /// Selects a category after compact decimal notation has supplied its
    /// locale-dependent exponent operand.
    ///
    /// ECMA-402's `PluralRuleSelect` carries the compact exponent (`c`) into
    /// CLDR plural evaluation. Passing the original decimal would make, for
    /// example, French `1.5e6` select `other` instead of the compact `many`.
    pub fn select_compact_f64(
        &self,
        value: f64,
        long_display: bool,
    ) -> Result<PluralCategory, PluralRulesError> {
        let decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
            .map_err(|_| PluralRulesError::NonFiniteNumber)?;
        if let Some(supplemental) = self.supplemental {
            return Ok(supplemental.select(&value.to_string()));
        }
        if value == 0.0 {
            return Ok(self.rules.category_for(&decimal).into());
        }
        let preferences = self.negotiation.selected.locale().into();
        let formatter = if long_display {
            CompactDecimalFormatter::try_new_long(preferences, Default::default())
        } else {
            CompactDecimalFormatter::try_new_short(preferences, Default::default())
        }
        .map_err(|_| PluralRulesError::DataUnavailable)?;
        let exponent = formatter.compact_exponent_for_magnitude(decimal.nonzero_magnitude_start());
        let compact = CompactDecimal::from_significand_and_exponent(decimal, exponent);
        Ok(self.rules.category_for(&compact).into())
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedPluralRulesOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &PluralRulesLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}

/// The kind of relation joined by an `Intl.ListFormat` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ListType {
    /// Join alternatives with a localized equivalent of "and".
    #[default]
    Conjunction,
    /// Join alternatives with a localized equivalent of "or".
    Disjunction,
    /// Join units without a conjunction.
    Unit,
}

/// The CLDR list-pattern width selected by `Intl.ListFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ListStyle {
    /// The normal full-width list pattern.
    #[default]
    Wide,
    /// A compact list pattern.
    Short,
    /// The narrowest list pattern.
    Narrow,
}

impl From<ListStyle> for IcuListLength {
    fn from(value: ListStyle) -> Self {
        match value {
            ListStyle::Wide => Self::Wide,
            ListStyle::Short => Self::Short,
            ListStyle::Narrow => Self::Narrow,
        }
    }
}

/// One canonical locale considered during list-format negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListFormatLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl ListFormatLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled list-pattern data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of list-format locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListFormatLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<ListFormatLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl ListFormatLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[ListFormatLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for list formatting.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Negotiates a requested locale list against bundled list-pattern data.
pub fn negotiate_list_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> ListFormatLocaleNegotiation {
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| ListFormatLocaleCandidate {
            supported: supports_locale_language(requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    ListFormatLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Resolves one requested locale against bundled list-pattern data.
pub fn resolve_list_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_list_format_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns the requested locales supported by the bundled list service.
pub fn supported_list_format_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    negotiate_list_format_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
}

/// Host-neutral options for constructing an `Intl.ListFormat` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ListFormatOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// The relation joined by the list patterns.
    pub list_type: ListType,
    /// The requested CLDR list-pattern width.
    pub style: ListStyle,
}

/// ECMAScript-observable data resolved by a list-format service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedListFormatOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The selected list type.
    pub list_type: ListType,
    /// The selected list style.
    pub style: ListStyle,
}

/// The `type` field of one `Intl.ListFormat.prototype.formatToParts` result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListPartKind {
    /// An input list element.
    Element,
    /// A locale-provided list literal such as a comma or conjunction.
    Literal,
}

/// One host-neutral `Intl.ListFormat.prototype.formatToParts` result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListPart {
    /// Whether this is an input element or a locale-provided literal.
    pub kind: ListPartKind,
    /// The corresponding UTF-8 string segment.
    pub value: String,
}

/// A failure while constructing a list formatter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListFormatError {
    /// The selected list-pattern data was unavailable.
    DataUnavailable,
    /// ICU4X could not write the formatted list to the host-neutral collector.
    FormattingFailed,
}

impl std::fmt::Display for ListFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("list-pattern data is unavailable"),
            Self::FormattingFailed => formatter.write_str("could not collect list-format parts"),
        }
    }
}

impl std::error::Error for ListFormatError {}

/// A host-neutral `Intl.ListFormat` service backed by ICU4X.
///
/// Input item coercion remains an embedding concern. The service formats the
/// resulting strings using the negotiated CLDR list patterns and exposes their
/// literal/element boundaries without needing a JavaScript Realm.
pub struct ListFormat {
    formatter: IcuListFormatter,
    negotiation: ListFormatLocaleNegotiation,
    resolved: ResolvedListFormatOptions,
}

impl ListFormat {
    /// Constructs a list formatter after locale negotiation and option
    /// resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: ListFormatOptions,
    ) -> Result<Self, ListFormatError> {
        let negotiation = negotiate_list_format_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let preferences: ListFormatterPreferences = selected.locale().into();
        let formatter_options =
            IcuListFormatterOptions::default().with_length(options.style.into());
        let formatter = match options.list_type {
            ListType::Conjunction => IcuListFormatter::try_new_and(preferences, formatter_options),
            ListType::Disjunction => IcuListFormatter::try_new_or(preferences, formatter_options),
            ListType::Unit => IcuListFormatter::try_new_unit(preferences, formatter_options),
        }
        .map_err(|_| ListFormatError::DataUnavailable)?;
        Ok(Self {
            formatter,
            negotiation,
            resolved: ResolvedListFormatOptions {
                locale: selected.as_str().into(),
                list_type: options.list_type,
                style: options.style,
            },
        })
    }

    /// Formats a sequence of already-coerced string items.
    pub fn format<I, S>(&self, values: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values = values
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect::<Vec<_>>();
        self.formatter
            .format(values.iter().map(String::as_str))
            .to_string()
    }

    /// Formats a sequence of already-coerced string items into ECMA-402 parts.
    pub fn format_to_parts<I, S>(&self, values: I) -> Result<Vec<ListPart>, ListFormatError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values = values
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect::<Vec<_>>();
        let mut collector = ListPartCollector::default();
        self.formatter
            .format(values.iter().map(String::as_str))
            .write_to_parts(&mut collector)
            .map_err(|_| ListFormatError::FormattingFailed)?;
        Ok(collector.parts)
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedListFormatOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &ListFormatLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}

#[derive(Default)]
struct ListPartCollector {
    parts: Vec<ListPart>,
    stack: Vec<ListPartKind>,
}

impl std::fmt::Write for ListPartCollector {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if value.is_empty() {
            return Ok(());
        }
        let kind = self.stack.last().copied().unwrap_or(ListPartKind::Literal);
        if let Some(part) = self.parts.last_mut().filter(|part| part.kind == kind) {
            part.value.push_str(value);
        } else {
            self.parts.push(ListPart {
                kind,
                value: value.into(),
            });
        }
        Ok(())
    }
}

impl PartsWrite for ListPartCollector {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut write: impl FnMut(&mut Self::SubPartsWrite) -> std::fmt::Result,
    ) -> std::fmt::Result {
        let kind = if part == icu_list::parts::ELEMENT {
            ListPartKind::Element
        } else {
            ListPartKind::Literal
        };
        self.stack.push(kind);
        let result = write(self);
        self.stack.pop();
        result
    }
}

/// The ECMA-402 segmentation granularity to use.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SegmenterGranularity {
    /// Extended grapheme-cluster boundaries.
    #[default]
    Grapheme,
    /// Word boundaries, including non-word-like punctuation and whitespace.
    Word,
    /// Sentence boundaries.
    Sentence,
}

/// One canonical locale considered during segmenter negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmenterLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl SegmenterLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled segmentation data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of segmenter locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmenterLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<SegmenterLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl SegmenterLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[SegmenterLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for the segmenter service.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Returns whether the bundled segmentation data supports this locale.
pub fn supports_segmenter_locale(locale: &IcuLocale) -> bool {
    supports_locale_language(locale)
}

/// Negotiates requested locales against the bundled segmenter service.
pub fn negotiate_segmenter_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> SegmenterLocaleNegotiation {
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| SegmenterLocaleCandidate {
            supported: supports_segmenter_locale(requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    SegmenterLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Resolves one requested locale against the bundled segmenter service.
pub fn resolve_segmenter_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_segmenter_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns requested locales supported by the bundled segmenter service.
pub fn supported_segmenter_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    negotiate_segmenter_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
}

/// Host-neutral options for constructing an `Intl.Segmenter` service.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SegmenterOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// The requested segmentation granularity.
    pub granularity: SegmenterGranularity,
}

/// ECMAScript-observable data resolved by a segmenter service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSegmenterOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The selected segmentation granularity.
    pub granularity: SegmenterGranularity,
}

/// One host-neutral `Intl.Segmenter` segment result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmenterSegment {
    /// The corresponding input substring.
    pub segment: String,
    /// Its index in the original input, counted in UTF-16 code units.
    pub index_utf16: usize,
    /// Whether this is word-like. It is only meaningful for word granularity.
    pub is_word_like: Option<bool>,
}

/// A failure while constructing a segmenter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmenterError {
    /// The selected locale's segmentation data was unavailable.
    DataUnavailable,
}

impl std::fmt::Display for SegmenterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("segmentation data is unavailable"),
        }
    }
}

impl std::error::Error for SegmenterError {}

/// A host-neutral `Intl.Segmenter` service backed by ICU4X.
///
/// Input coercion and iterator-object mechanics remain embedding concerns. The
/// service returns fully materialized segments so a host can map them directly
/// into ECMA-402 `Segments` iterator results without retaining its input.
pub struct Segmenter {
    backend: SegmenterBackend,
    negotiation: SegmenterLocaleNegotiation,
    resolved: ResolvedSegmenterOptions,
}

enum SegmenterBackend {
    Grapheme(GraphemeClusterSegmenterBorrowed<'static>),
    Word(Box<WordSegmenter>),
    Sentence(Box<SentenceSegmenter>),
}

impl Segmenter {
    /// Constructs a segmenter after locale negotiation and option resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: SegmenterOptions,
    ) -> Result<Self, SegmenterError> {
        let negotiation = negotiate_segmenter_locale(requested, options.locale_matcher);
        let selected = negotiation.selected.clone();
        let backend = match options.granularity {
            SegmenterGranularity::Grapheme => {
                SegmenterBackend::Grapheme(GraphemeClusterSegmenter::new())
            }
            SegmenterGranularity::Word => {
                let language = selected.locale().id.clone();
                let mut word_options = WordBreakOptions::default();
                word_options.content_locale = Some(&language);
                SegmenterBackend::Word(Box::new(
                    WordSegmenter::try_new_auto(word_options)
                        .map_err(|_| SegmenterError::DataUnavailable)?,
                ))
            }
            SegmenterGranularity::Sentence => {
                let language = selected.locale().id.clone();
                let mut sentence_options = SentenceBreakOptions::default();
                sentence_options.content_locale = Some(&language);
                SegmenterBackend::Sentence(Box::new(
                    SentenceSegmenter::try_new(sentence_options)
                        .map_err(|_| SegmenterError::DataUnavailable)?,
                ))
            }
        };
        Ok(Self {
            backend,
            negotiation,
            resolved: ResolvedSegmenterOptions {
                locale: selected.as_str().into(),
                granularity: options.granularity,
            },
        })
    }

    /// Segments an already-coerced string at the selected granularity.
    pub fn segment(&self, input: &str) -> Vec<SegmenterSegment> {
        let input_utf16 = input.encode_utf16().collect::<Vec<_>>();
        match &self.backend {
            SegmenterBackend::Grapheme(segmenter) => segmenter_segments(
                &input_utf16,
                segmenter.segment_utf16(&input_utf16).map(|end| (end, None)),
            ),
            SegmenterBackend::Word(segmenter) => segmenter_segments(
                &input_utf16,
                segmenter
                    .as_borrowed()
                    .segment_utf16(&input_utf16)
                    .iter_with_word_type()
                    .map(|(end, word_type)| (end, Some(word_type.is_word_like()))),
            ),
            SegmenterBackend::Sentence(segmenter) => segmenter_segments(
                &input_utf16,
                segmenter
                    .as_borrowed()
                    .segment_utf16(&input_utf16)
                    .map(|end| (end, None)),
            ),
        }
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedSegmenterOptions {
        &self.resolved
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &SegmenterLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}

fn segmenter_segments<I>(input_utf16: &[u16], boundaries: I) -> Vec<SegmenterSegment>
where
    I: IntoIterator<Item = (usize, Option<bool>)>,
{
    let mut start = 0;
    let mut segments = Vec::new();
    for (end, is_word_like) in boundaries {
        if end == start {
            continue;
        }
        segments.push(SegmenterSegment {
            segment: String::from_utf16(&input_utf16[start..end])
                .expect("ICU4X boundaries preserve valid UTF-16"),
            index_utf16: start,
            is_word_like,
        });
        start = end;
    }
    segments
}

/// The `type` option accepted by `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayNamesType {
    /// BCP 47 language identifiers.
    Language,
    /// ISO 3166-style region identifiers.
    Region,
    /// ISO 15924-style script identifiers.
    Script,
    /// ISO 4217 currency identifiers.
    Currency,
    /// Unicode calendar identifiers.
    Calendar,
    /// The fixed ECMA-402 date-time-field identifiers.
    DateTimeField,
}

/// The width requested from `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayNamesStyle {
    /// The ordinary CLDR display name.
    #[default]
    Long,
    /// An abbreviated CLDR display name.
    Short,
    /// A narrow CLDR display name.
    Narrow,
}

/// How missing display-name data is represented.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayNamesFallback {
    /// Return the canonical code when no localized name is available.
    #[default]
    Code,
    /// Return no result when no localized name is available.
    None,
}

/// The language-name spelling policy requested by `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayNamesLanguageDisplay {
    /// Prefer the language's dialect name where the locale data has one.
    #[default]
    Dialect,
    /// Prefer the standard language name.
    Standard,
}

/// Host-neutral options for constructing `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayNamesOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// The kind of code to display.
    pub display_type: DisplayNamesType,
    /// The requested display-name width.
    pub style: DisplayNamesStyle,
    /// The result for a valid code that has no bundled data.
    pub fallback: DisplayNamesFallback,
    /// The policy for language names.
    pub language_display: DisplayNamesLanguageDisplay,
}

/// ECMAScript-observable data resolved by a display-name service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDisplayNamesOptions {
    /// The negotiated locale.
    pub locale: String,
    /// The kind of displayed code.
    pub display_type: DisplayNamesType,
    /// The selected display-name width.
    pub style: DisplayNamesStyle,
    /// The missing-data policy.
    pub fallback: DisplayNamesFallback,
    /// The selected language-name policy, only for language display names.
    pub language_display: Option<DisplayNamesLanguageDisplay>,
}

/// A failure from constructing or using `Intl.DisplayNames`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayNamesError {
    /// The selected locale has no bundled display-name data.
    DataUnavailable,
    /// The code is invalid for the service's selected type.
    InvalidCode,
}

impl std::fmt::Display for DisplayNamesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("display-name data is unavailable"),
            Self::InvalidCode => formatter.write_str("invalid display-name code"),
        }
    }
}

impl std::error::Error for DisplayNamesError {}

/// Returns whether the bundled display-name data supports this locale.
pub fn supports_display_names_locale(locale: &IcuLocale) -> bool {
    supports_locale_language(locale)
}

/// Resolves requested locales for the display-name service.
pub fn resolve_display_names_locale(
    requested: &[CanonicalLocale],
    _matcher: LocaleMatcher,
) -> CanonicalLocale {
    requested
        .iter()
        .find(|locale| supports_display_names_locale(locale.locale()))
        .cloned()
        .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid"))
}

/// Returns requested locales supported by the bundled display-name service.
pub fn supported_display_names_locales(
    requested: &[CanonicalLocale],
    _matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    requested
        .iter()
        .filter(|locale| supports_display_names_locale(locale.locale()))
        .cloned()
        .collect()
}

/// A host-neutral `Intl.DisplayNames` service.
///
/// The standard permits locale-data coverage to be implementation dependent.
/// This service therefore supplies a small deterministic data set and applies
/// the specified `code`/`none` fallback to any canonical code it does not
/// carry. It never treats an invalid code as missing data.
pub struct DisplayNames {
    resolved: ResolvedDisplayNamesOptions,
}

impl DisplayNames {
    /// Constructs a display-name service after locale negotiation.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: DisplayNamesOptions,
    ) -> Result<Self, DisplayNamesError> {
        let locale = resolve_display_names_locale(requested, options.locale_matcher);
        if !supports_display_names_locale(locale.locale()) {
            return Err(DisplayNamesError::DataUnavailable);
        }
        Ok(Self {
            resolved: ResolvedDisplayNamesOptions {
                locale: locale.as_str().to_owned(),
                display_type: options.display_type,
                style: options.style,
                fallback: options.fallback,
                language_display: (options.display_type == DisplayNamesType::Language)
                    .then_some(options.language_display),
            },
        })
    }

    /// Returns the resolved service data.
    pub fn resolved_options(&self) -> &ResolvedDisplayNamesOptions {
        &self.resolved
    }

    /// Canonicalizes `code` for the selected type and returns its display name.
    ///
    /// `None` is the specified result for a valid but uncovered code when the
    /// service was configured with `fallback: "none"`.
    pub fn of(&self, code: &str) -> Result<Option<String>, DisplayNamesError> {
        let code = canonical_display_name_code(self.resolved.display_type, code)?;
        let localized = display_name(
            self.resolved.locale.as_str(),
            self.resolved.display_type,
            self.resolved.style,
            self.resolved.language_display,
            &code,
        );
        Ok(localized.or(match self.resolved.fallback {
            DisplayNamesFallback::Code => Some(code),
            DisplayNamesFallback::None => None,
        }))
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.resolved.locale.len()
    }
}

fn canonical_display_name_code(
    display_type: DisplayNamesType,
    code: &str,
) -> Result<String, DisplayNamesError> {
    let valid_type = |code: &str| {
        !code.is_empty()
            && code.split('-').all(|part| {
                (3..=8).contains(&part.len())
                    && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
    };
    match display_type {
        DisplayNamesType::Language => is_unicode_language_id(code)
            .then_some(())
            .ok_or(DisplayNamesError::InvalidCode)
            .map(|()| {
                // ICU's data-locale parser deliberately rejects some
                // structurally valid, unregistered language/variant values.
                // ECMA-402 still requires them to be canonical display-name
                // codes, so retain the grammar-derived casing when ICU cannot
                // provide an alias transformation.
                canonicalize(code)
                    .map(|locale| locale.to_string())
                    .unwrap_or_else(|_| canonical_unicode_language_id(code))
            }),
        DisplayNamesType::Region => {
            let valid = (code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
                || (code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()));
            valid
                .then(|| code.to_ascii_uppercase())
                .ok_or(DisplayNamesError::InvalidCode)
        }
        DisplayNamesType::Script => (code.len() == 4
            && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then(|| {
            let mut code = code.to_ascii_lowercase();
            code[..1].make_ascii_uppercase();
            code
        })
        .ok_or(DisplayNamesError::InvalidCode),
        DisplayNamesType::Currency => (code.len() == 3
            && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then(|| code.to_ascii_uppercase())
        .ok_or(DisplayNamesError::InvalidCode),
        DisplayNamesType::Calendar => valid_type(code)
            .then(|| code.to_ascii_lowercase())
            .ok_or(DisplayNamesError::InvalidCode),
        DisplayNamesType::DateTimeField => matches!(
            code,
            "era"
                | "year"
                | "quarter"
                | "month"
                | "weekOfYear"
                | "weekday"
                | "day"
                | "dayPeriod"
                | "hour"
                | "minute"
                | "second"
                | "timeZoneName"
        )
        .then(|| code.to_owned())
        .ok_or(DisplayNamesError::InvalidCode),
    }
}

fn is_unicode_language_id(code: &str) -> bool {
    let mut subtags = code.split('-');
    let Some(language) = subtags.next() else {
        return false;
    };
    if !matches!(language.len(), 2..=3 | 5..=8)
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    let mut remaining = subtags.peekable();
    if remaining.peek().is_some_and(|subtag| {
        subtag.len() == 4 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic())
    }) {
        remaining.next();
    }
    if remaining.peek().is_some_and(|subtag| {
        (subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()))
            || (subtag.len() == 3 && subtag.bytes().all(|byte| byte.is_ascii_digit()))
    }) {
        remaining.next();
    }
    let mut variants = std::collections::HashSet::new();
    remaining.all(|subtag| {
        let valid = (5..=8).contains(&subtag.len())
            && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || subtag.len() == 4
                && subtag.as_bytes()[0].is_ascii_digit()
                && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric());
        valid && variants.insert(subtag.to_ascii_lowercase())
    })
}

fn canonical_unicode_language_id(code: &str) -> String {
    let mut subtags = code.split('-');
    let language = subtags.next().expect("validated language identifier");
    let mut result = vec![language.to_ascii_lowercase()];
    let mut remaining = subtags.peekable();
    if remaining.peek().is_some_and(|subtag| {
        subtag.len() == 4 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic())
    }) {
        let mut script = remaining
            .next()
            .expect("present script")
            .to_ascii_lowercase();
        script[..1].make_ascii_uppercase();
        result.push(script);
    }
    if remaining.peek().is_some_and(|subtag| {
        (subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()))
            || (subtag.len() == 3 && subtag.bytes().all(|byte| byte.is_ascii_digit()))
    }) {
        result.push(
            remaining
                .next()
                .expect("present region")
                .to_ascii_uppercase(),
        );
    }
    result.extend(remaining.map(str::to_ascii_lowercase));
    result.join("-")
}

fn display_name(
    locale: &str,
    display_type: DisplayNamesType,
    style: DisplayNamesStyle,
    language_display: Option<DisplayNamesLanguageDisplay>,
    code: &str,
) -> Option<String> {
    // This deliberately small, host-neutral table is data, not a semantic
    // fallback. Its remaining absence is represented by DisplayNamesFallback.
    let english = locale.starts_with("en");
    let french = locale.starts_with("fr");
    let name = match (display_type, code) {
        (DisplayNamesType::Language, "en") if english => "English",
        (DisplayNamesType::Language, "fr") if english => "French",
        (DisplayNamesType::Language, "de") if english => "German",
        (DisplayNamesType::Language, "es") if english => "Spanish",
        (DisplayNamesType::Language, "ja") if english => "Japanese",
        (DisplayNamesType::Language, "zh") if english => "Chinese",
        (DisplayNamesType::Language, "en-US")
            if english && language_display == Some(DisplayNamesLanguageDisplay::Dialect) =>
        {
            "American English"
        }
        (DisplayNamesType::Language, "en") if french => "anglais",
        (DisplayNamesType::Language, "fr") if french => "français",
        (DisplayNamesType::Region, "US") if english => "United States",
        (DisplayNamesType::Region, "GB") if english => "United Kingdom",
        (DisplayNamesType::Region, "FR") if english => "France",
        (DisplayNamesType::Region, "TW") if english => "Taiwan",
        (DisplayNamesType::Script, "Latn") if english => "Latin",
        (DisplayNamesType::Script, "Cyrl") if english => "Cyrillic",
        (DisplayNamesType::Currency, "USD") if english => "US Dollar",
        (DisplayNamesType::Currency, "EUR") if english => "Euro",
        (DisplayNamesType::Currency, "JPY") if english => "Japanese Yen",
        (DisplayNamesType::Calendar, "gregory") if english => "Gregorian Calendar",
        (DisplayNamesType::Calendar, "buddhist") if english => "Buddhist Calendar",
        (DisplayNamesType::DateTimeField, "year") if english => "year",
        (DisplayNamesType::DateTimeField, "month") if english => "month",
        (DisplayNamesType::DateTimeField, "day") if english => "day",
        (DisplayNamesType::DateTimeField, "hour") if english => "hour",
        (DisplayNamesType::DateTimeField, "minute") if english => "minute",
        (DisplayNamesType::DateTimeField, "second") if english => "second",
        _ => return None,
    };
    Some(match style {
        DisplayNamesStyle::Long => name.into(),
        // The bundled data has no separate short/narrow form for these names.
        // CLDR permits a parent-width fallback, so retain the long form.
        DisplayNamesStyle::Short | DisplayNamesStyle::Narrow => name.into(),
    })
}

/// A singular ECMA-402 relative-time unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeTimeUnit {
    /// Seconds.
    Second,
    /// Minutes.
    Minute,
    /// Hours.
    Hour,
    /// Days.
    Day,
    /// Weeks.
    Week,
    /// Months.
    Month,
    /// Quarters.
    Quarter,
    /// Years.
    Year,
}

impl RelativeTimeUnit {
    /// Parses a singular or plural ECMA-402 relative-time unit.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "second" | "seconds" => Some(Self::Second),
            "minute" | "minutes" => Some(Self::Minute),
            "hour" | "hours" => Some(Self::Hour),
            "day" | "days" => Some(Self::Day),
            "week" | "weeks" => Some(Self::Week),
            "month" | "months" => Some(Self::Month),
            "quarter" | "quarters" => Some(Self::Quarter),
            "year" | "years" => Some(Self::Year),
            _ => None,
        }
    }

    /// Returns the singular ECMA-402 unit spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::Year => "year",
        }
    }
}

/// The `style` option accepted by `Intl.RelativeTimeFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RelativeTimeStyle {
    /// The ordinary CLDR relative-time pattern.
    #[default]
    Long,
    /// The abbreviated CLDR relative-time pattern.
    Short,
    /// The narrow CLDR relative-time pattern.
    Narrow,
}

/// The `numeric` option accepted by `Intl.RelativeTimeFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RelativeTimeNumeric {
    /// Always render the numeric relative-time pattern.
    #[default]
    Always,
    /// Use a locale's qualitative relative-time terms where available.
    Auto,
}

/// Host-neutral options for `Intl.RelativeTimeFormat`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelativeTimeFormatOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// A valid numbering-system identifier requested by the embedding host.
    /// Unsupported values resolve to the locale default.
    pub numbering_system: Option<String>,
    /// The relative-time pattern width.
    pub style: RelativeTimeStyle,
    /// Whether qualitative terms may replace a numeric pattern.
    pub numeric: RelativeTimeNumeric,
}

/// ECMAScript-observable data resolved by a relative-time service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedRelativeTimeFormatOptions {
    /// The negotiated locale after any accepted numbering-system override.
    pub locale: String,
    /// The selected numbering system.
    pub numbering_system: String,
    /// The selected relative-time pattern width.
    pub style: RelativeTimeStyle,
    /// The selected qualitative/numeric policy.
    pub numeric: RelativeTimeNumeric,
}

/// A part of a relative-time formatted result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelativeTimePart {
    /// The ECMA-402 `formatToParts` type.
    pub kind: RelativeTimePartKind,
    /// The text of this part.
    pub value: String,
}

/// The kind of a [`RelativeTimePart`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeTimePartKind {
    /// Locale pattern text outside the number.
    Literal,
    /// A contiguous integer digit run.
    Integer,
    /// A grouping separator.
    Group,
    /// A decimal separator.
    Decimal,
    /// A contiguous fraction digit run.
    Fraction,
}

/// A relative-time construction or formatting failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeTimeFormatError {
    /// The selected locale's numeric data was unavailable.
    DataUnavailable,
    /// The numeric input was `NaN` or infinite.
    NonFiniteNumber,
}

impl std::fmt::Display for RelativeTimeFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("relative-time data is unavailable"),
            Self::NonFiniteNumber => formatter.write_str("relative time must be finite"),
        }
    }
}

impl std::error::Error for RelativeTimeFormatError {}

/// Returns whether the bundled relative-time patterns support this locale.
///
/// The service intentionally advertises only languages with bundled patterns;
/// NumberFormat's broader data coverage must not be mistaken for relative-time
/// data coverage.
pub fn supports_relative_time_format_locale(locale: &IcuLocale) -> bool {
    supports_locale_language(locale)
}

/// Returns requested locales supported by the bundled relative-time service.
pub fn supported_relative_time_format_locales(
    requested: &[CanonicalLocale],
    _matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    requested
        .iter()
        .filter(|locale| supports_relative_time_format_locale(locale.locale()))
        .cloned()
        .collect()
}

fn resolve_relative_time_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    supported_relative_time_format_locales(requested, matcher)
        .into_iter()
        .next()
        .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid"))
}

/// A host-neutral `Intl.RelativeTimeFormat` service.
///
/// It owns locale-data lookup and partitioning. The embedding runtime remains
/// responsible for ECMAScript `ToNumber`/`ToString` coercion and result-object
/// construction.
pub struct RelativeTimeFormat {
    number_format: NumberFormat,
    resolved: ResolvedRelativeTimeFormatOptions,
}

impl RelativeTimeFormat {
    /// Constructs a relative-time formatter from typed options.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: RelativeTimeFormatOptions,
    ) -> Result<Self, RelativeTimeFormatError> {
        let mut locale = resolve_relative_time_format_locale(requested, options.locale_matcher);
        let requested_numbering = options
            .numbering_system
            .as_deref()
            .filter(|value| matches!(*value, "latn" | "arab" | "deva" | "hanidec"));
        let extension_numbering = unicode_keyword(locale.locale(), "nu")
            .filter(|value| matches!(value.as_str(), "latn" | "arab" | "deva" | "hanidec"));
        let numbering_system = requested_numbering
            .or(extension_numbering.as_deref())
            .unwrap_or("latn");
        let retain_extension = extension_numbering.as_deref() == Some(numbering_system);
        locale = locale_with_numbering_system(&locale, numbering_system, retain_extension);
        let number_format = NumberFormat::try_new(
            &[locale.clone()],
            NumberFormatOptions {
                locale_matcher: options.locale_matcher,
                ..Default::default()
            },
        )
        .map_err(|_| RelativeTimeFormatError::DataUnavailable)?;
        let resolved = ResolvedRelativeTimeFormatOptions {
            locale: locale.as_str().to_owned(),
            numbering_system: number_format.resolved_options().numbering_system.clone(),
            style: options.style,
            numeric: options.numeric,
        };
        Ok(Self {
            number_format,
            resolved,
        })
    }

    /// Formats a finite relative-time quantity into ECMA-402-style parts.
    pub fn format_to_parts(
        &self,
        value: f64,
        unit: RelativeTimeUnit,
    ) -> Result<Vec<RelativeTimePart>, RelativeTimeFormatError> {
        if !value.is_finite() {
            return Err(RelativeTimeFormatError::NonFiniteNumber);
        }
        if let Some(term) = self.qualitative_term(value, unit) {
            return Ok(vec![RelativeTimePart {
                kind: RelativeTimePartKind::Literal,
                value: term.into(),
            }]);
        }
        let past = value.is_sign_negative();
        let number = self
            .number_format
            .format_f64(value.abs())
            .map_err(|_| RelativeTimeFormatError::NonFiniteNumber)?;
        let mut parts = Vec::new();
        if !past {
            parts.push(RelativeTimePart {
                kind: RelativeTimePartKind::Literal,
                value: if self.resolved.locale.starts_with("pl") {
                    "za ".into()
                } else {
                    "in ".into()
                },
            });
        }
        parts.extend(relative_time_number_parts(
            &number,
            self.resolved.locale.starts_with("pl"),
        ));
        let label = self.unit_label(value.abs(), unit);
        parts.push(RelativeTimePart {
            kind: RelativeTimePartKind::Literal,
            value: if past {
                if self.resolved.locale.starts_with("pl") {
                    format!(" {label} temu")
                } else {
                    format!(" {label} ago")
                }
            } else {
                format!(" {label}")
            },
        });
        Ok(parts)
    }

    /// Formats a finite relative-time quantity into a string.
    pub fn format(
        &self,
        value: f64,
        unit: RelativeTimeUnit,
    ) -> Result<String, RelativeTimeFormatError> {
        Ok(self
            .format_to_parts(value, unit)?
            .into_iter()
            .map(|part| part.value)
            .collect())
    }

    /// Returns the resolved service options.
    pub fn resolved_options(&self) -> &ResolvedRelativeTimeFormatOptions {
        &self.resolved
    }

    /// Returns heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.number_format.bytes()
            + self.resolved.locale.len()
            + self.resolved.numbering_system.len()
    }

    fn qualitative_term(&self, value: f64, unit: RelativeTimeUnit) -> Option<&'static str> {
        if self.resolved.numeric != RelativeTimeNumeric::Auto
            || !self.resolved.locale.starts_with("en")
        {
            return None;
        }
        match (value.is_sign_negative(), value.abs() as i64, unit) {
            (_, 0, RelativeTimeUnit::Second) if value == 0.0 => Some("now"),
            (_, 0, RelativeTimeUnit::Minute) if value == 0.0 => Some("this minute"),
            (_, 0, RelativeTimeUnit::Hour) if value == 0.0 => Some("this hour"),
            (_, 0, RelativeTimeUnit::Day) if value == 0.0 => Some("today"),
            (false, 1, RelativeTimeUnit::Day) => Some("tomorrow"),
            (true, 1, RelativeTimeUnit::Day) => Some("yesterday"),
            (_, 0, RelativeTimeUnit::Week) if value == 0.0 => Some("this week"),
            (false, 1, RelativeTimeUnit::Week) => Some("next week"),
            (true, 1, RelativeTimeUnit::Week) => Some("last week"),
            (_, 0, RelativeTimeUnit::Month) if value == 0.0 => Some("this month"),
            (false, 1, RelativeTimeUnit::Month) => Some("next month"),
            (true, 1, RelativeTimeUnit::Month) => Some("last month"),
            (_, 0, RelativeTimeUnit::Quarter) if value == 0.0 => Some("this quarter"),
            (false, 1, RelativeTimeUnit::Quarter) => Some("next quarter"),
            (true, 1, RelativeTimeUnit::Quarter) => Some("last quarter"),
            (_, 0, RelativeTimeUnit::Year) if value == 0.0 => Some("this year"),
            (false, 1, RelativeTimeUnit::Year) => Some("next year"),
            (true, 1, RelativeTimeUnit::Year) => Some("last year"),
            _ => None,
        }
    }

    fn unit_label(&self, value: f64, unit: RelativeTimeUnit) -> &'static str {
        if self.resolved.locale.starts_with("pl") {
            return polish_relative_time_label(self.resolved.style, unit, value);
        }
        english_relative_time_label(self.resolved.style, unit, value)
    }
}

fn locale_with_numbering_system(
    locale: &CanonicalLocale,
    numbering_system: &str,
    retain_extension: bool,
) -> CanonicalLocale {
    let mut data_locale = locale.locale().clone();
    let key: icu_locale_core::extensions::unicode::Key = "nu".parse().expect("valid key");
    data_locale.extensions.unicode.keywords.remove(key);
    data_locale.extensions.unicode.keywords.set(
        key,
        numbering_system
            .parse()
            .expect("validated numbering system"),
    );
    let canonical = if retain_extension {
        data_locale.to_string()
    } else {
        let mut visible_locale = data_locale.clone();
        visible_locale.extensions.unicode.keywords.remove(key);
        visible_locale.to_string()
    };
    CanonicalLocale::from_parts(data_locale, canonical)
}

fn relative_time_number_parts(number: &str, polish: bool) -> Vec<RelativeTimePart> {
    let mut parts = Vec::new();
    let mut kind = RelativeTimePartKind::Integer;
    let mut buffer = String::new();
    let flush = |parts: &mut Vec<RelativeTimePart>, buffer: &mut String, kind| {
        if !buffer.is_empty() {
            parts.push(RelativeTimePart {
                kind,
                value: std::mem::take(buffer),
            });
        }
    };
    for character in number.chars() {
        let separator = match character {
            ',' if polish => Some(RelativeTimePartKind::Decimal),
            ',' | '\u{a0}' | '\u{202f}' | '\u{66c}' => Some(RelativeTimePartKind::Group),
            '.' | '\u{66b}' => Some(RelativeTimePartKind::Decimal),
            _ => None,
        };
        if let Some(separator) = separator {
            flush(&mut parts, &mut buffer, kind);
            parts.push(RelativeTimePart {
                kind: separator,
                value: character.into(),
            });
            kind = if separator == RelativeTimePartKind::Decimal {
                RelativeTimePartKind::Fraction
            } else {
                RelativeTimePartKind::Integer
            };
        } else {
            buffer.push(character);
        }
    }
    flush(&mut parts, &mut buffer, kind);
    parts
}

fn english_relative_time_label(
    style: RelativeTimeStyle,
    unit: RelativeTimeUnit,
    value: f64,
) -> &'static str {
    let one = value == 1.0;
    match (style, unit, one) {
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, true) => "second",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, true) => "minute",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, true) => "hour",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, true) => "day",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, true) => "week",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, true) => "month",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, true) => "quarter",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, true) => "year",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, false) => "seconds",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, false) => "minutes",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, false) => "hours",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, false) => "days",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, false) => "weeks",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, false) => "months",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, false) => "quarters",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, false) => "years",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Second, _) => "sec.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Minute, _) => "min.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Hour, _) => "hr.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, true) => "day",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, false) => "days",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Week, _) => "wk.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Month, _) => "mo.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Quarter, true) => "qtr.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Quarter, false) => "qtrs.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Year, _) => "yr.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Second, _) => "s",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Minute, _) => "m",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Hour, _) => "h",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Day, _) => "d",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Week, _) => "w",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Month, _) => "mo",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Quarter, _) => "q",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Year, _) => "y",
    }
}

fn polish_relative_time_label(
    style: RelativeTimeStyle,
    unit: RelativeTimeUnit,
    value: f64,
) -> &'static str {
    let integer = value.fract() == 0.0;
    let number = value as i64;
    let category = if integer && number == 1 {
        "one"
    } else if integer
        && (2..=4).contains(&(number.rem_euclid(10)))
        && !(12..=14).contains(&(number.rem_euclid(100)))
    {
        "few"
    } else if integer {
        "many"
    } else {
        "other"
    };
    match (style, unit, category) {
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, "one") => "sekundę",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, "few" | "other") => "sekundy",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, "many") => "sekund",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, "one") => "minutę",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, "few" | "other") => "minuty",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, "many") => "minut",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, "one") => "godzinę",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, "few" | "other") => "godziny",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, "many") => "godzin",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, "one") => "dzień",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, "few" | "many") => "dni",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, "other") => "dnia",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, "one") => "tydzień",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, "few") => "tygodnie",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, "many") => "tygodni",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, "other") => "tygodnia",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, "one") => "miesiąc",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, "few") => "miesiące",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, "many") => "miesięcy",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, "other") => "miesiąca",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, "one") => "kwartał",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, "few") => "kwartały",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, "many") => "kwartałów",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, "other") => "kwartału",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, "one") => "rok",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, "few") => "lata",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, "many") => "lat",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, "other") => "roku",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Second, _) => "sek.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Minute, _) => "min",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Hour, _) => "godz.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, "one") => "dzień",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, "other") => "dnia",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, _) => "dni",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Week, "one") => "tydz.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Week, _) => "tyg.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Month, _) => "mies.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Quarter, _) => "kw.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Year, "one") => "rok",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Year, "few") => "lata",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Year, "other") => "roku",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Year, "many") => "lat",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Second, _) => "s",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Minute, _) => "min",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Hour, _) => "g.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Day, "one") => "dzień",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Day, "other") => "dnia",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Day, _) => "dni",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Week, "one") => "tydz.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Week, _) => "tyg.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Month, _) => "mies.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Quarter, _) => "kw.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Year, "one") => "rok",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Year, "few") => "lata",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Year, "other") => "roku",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Year, "many") => "lat",
        _ => unreachable!("Polish plural selection returns a known category"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_ecma402_aliases_without_a_vm() {
        for (input, expected) in [
            (
                "EN-latn-us-1901-u-ca-islamicc-kn-true",
                "en-Latn-US-1901-u-ca-islamic-civil-kn",
            ),
            ("en-u-ca-ethiopic-amete-alem", "en-u-ca-ethioaa"),
            ("und-u-ks-primary", "und-u-ks-level1"),
            ("und-u-ms-imperial", "und-u-ms-uksystem"),
            ("und-u-tz-eire", "und-u-tz-iedub"),
            (
                "und-Latn-t-und-hani-m0-names",
                "und-Latn-t-und-hani-m0-prprname",
            ),
            ("posix", "posix"),
        ] {
            assert_eq!(canonicalize(input).unwrap().as_str(), expected, "{input}");
        }
    }

    #[test]
    fn rejects_ecma402_invalid_locale_forms() {
        for input in ["en_US", "en-u-ca-gregory-u-nu-latn", "de-1901-1901"] {
            assert_eq!(canonicalize(input), Err(LocaleError::InvalidLanguageTag));
        }
    }

    #[test]
    fn supplies_collation_metadata_from_the_canonical_locale() {
        let de = canonicalize("de-u-co-phonebk").unwrap();
        assert!(supports_collation_locale(de.locale()));
        assert_eq!(
            unicode_keyword(de.locale(), "co").as_deref(),
            Some("phonebk")
        );
        assert!(supports_collation(de.locale(), "phonebk"));
        assert!(!supports_collation(de.locale(), "zhuyin"));
        let unsupported = canonicalize("zz").unwrap();
        assert!(!supports_collation_locale(unsupported.locale()));
    }
}
