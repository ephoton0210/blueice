// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The data-facing, host-neutral core of ECMA-402 internationalization.
//!
//! This crate owns locale identifier canonicalization, ICU-backed locale data
//! selection, and UTF-16 collation. ECMAScript object/Realm semantics and
//! coercion intentionally remain in the embedding runtime.

use fixed_decimal::{RoundingIncrement, SignedRoundingMode, UnsignedRoundingMode};
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
    provider::{Baked as DecimalData, DecimalDigitsV1},
    DecimalFormatter, DecimalFormatterPreferences,
};
use icu_list::{
    options::{ListFormatterOptions as IcuListFormatterOptions, ListLength as IcuListLength},
    ListFormatter as IcuListFormatter, ListFormatterPreferences,
};
use icu_locale_core::Locale as IcuLocale;
use icu_provider::{DataMarker, DataProvider, DataRequest};
use icu_segmenter::{
    options::{SentenceBreakOptions, WordBreakOptions},
    GraphemeClusterSegmenter, GraphemeClusterSegmenterBorrowed, SentenceSegmenter, WordSegmenter,
};
use std::{any::TypeId, cell::RefCell};
use writeable::{Part, PartsWrite, Writeable};

mod collator;
mod date_time_format;
mod display_names;
mod duration;
mod list_format;
mod locale_data;
mod locale_information;
mod number_format;
mod plural_rules;
mod relative_time_format;
mod segmenter;
mod supported_values;

pub use collator::*;
pub use date_time_format::*;
pub use display_names::*;
pub use duration::*;
pub use list_format::*;
pub use locale_data::*;
pub use locale_information::*;
pub use number_format::*;
pub use plural_rules::*;
pub use relative_time_format::*;
pub use segmenter::*;
pub use supported_values::*;

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
    let language = parts.next().expect("split always yields the language slot");
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
        locale.extensions.unicode.keywords.set(
            "hc".parse().expect("hc is a Unicode locale key"),
            hour_cycle
                .parse()
                .expect("the validated hour cycle is a Unicode locale value"),
        );
    }
    if let Some(case_first) = &options.case_first {
        if !matches!(case_first.as_str(), "upper" | "lower" | "false") {
            return Err(LocaleOptionError::InvalidCaseFirst);
        }
        locale.extensions.unicode.keywords.set(
            "kf".parse().expect("kf is a Unicode locale key"),
            case_first
                .parse()
                .expect("the validated case-first value is a Unicode locale value"),
        );
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
    locale_data_provider().supports_language(locale)
}

/// Whether the bundled collation data supports the locale's language.
pub fn supports_collation_locale(locale: &IcuLocale) -> bool {
    locale_data_provider().supports_service_locale(IntlService::Collator, locale)
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

/// Applies an effective numbering-system preference to a canonical locale.
///
/// ICU receives the requested `nu` keyword in every case. `retain_extension`
/// controls whether it remains observable in the locale string: an accepted
/// Unicode locale extension is retained, whereas an explicit options value is
/// reflected only by `resolvedOptions().numberingSystem`.
pub fn locale_with_numbering_system(
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

/// Removes a `nu` extension when an explicit but unavailable option must fall
/// back to the locale's default numbering system.
pub fn locale_without_numbering_system(locale: &CanonicalLocale) -> CanonicalLocale {
    let mut data_locale = locale.locale().clone();
    let key: icu_locale_core::extensions::unicode::Key = "nu".parse().expect("valid key");
    data_locale.extensions.unicode.keywords.remove(key);
    CanonicalLocale::from_parts(data_locale.clone(), data_locale.to_string())
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
    let provider = locale_data_provider();
    let transformed = if maximize {
        provider.maximize_likely_subtags(locale.locale())
    } else {
        provider.minimize_likely_subtags(locale.locale())
    };
    canonicalize(&transformed.to_string()).expect("a transformed canonical locale remains valid")
}

/// Whether a collation is available for an already-selected locale.
pub fn supports_collation(locale: &IcuLocale, collation: &str) -> bool {
    locale_data_provider().supports_collation(locale, collation)
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

/// One requested locale considered by the shared ECMA-402 locale resolver.
///
/// `requested` deliberately retains its canonical Unicode extensions. The
/// service-specific `ResolveLocale` key processing that follows this common
/// step decides which of those extensions remain observable in
/// `resolvedOptions().locale`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocaleResolutionCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl LocaleResolutionCandidate {
    /// Returns the canonical locale supplied by the embedding host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether this service's provider data can satisfy the request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// The shared, service-aware result of ECMA-402 locale resolution.
///
/// The locale-data provider owns the available-locale registry. This type
/// owns request ordering, fallback to the stable default, and the common
/// lookup/best-fit boundary so services cannot accidentally grow independent
/// locale matchers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocaleResolution {
    service: IntlService,
    matcher: LocaleMatcher,
    candidates: Vec<LocaleResolutionCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl LocaleResolution {
    /// Returns the service whose data availability was considered.
    pub fn service(&self) -> IntlService {
        self.service
    }

    /// Returns the requested locale matching policy.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every request and its service-data availability result.
    pub fn candidates(&self) -> &[LocaleResolutionCandidate] {
        &self.candidates
    }

    /// Returns the selected canonical locale before service-key processing.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Whether no requested locale matched and the stable default was used.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Performs the common ECMA-402 locale matching step for an Intl service.
///
/// The provider's available-locale data is language-parent-backed: a request
/// such as `es-MX-u-nu-arab` therefore lookup-matches the same `es` data that
/// serves `es`. We retain the canonical request rather than serializing the
/// parent tag here, because ECMA-402 resolves Unicode extension keys only
/// after matching and each service must retain accepted keys in its visible
/// locale. The pinned provider currently has no distinct best-fit-only
/// mappings, so both algorithms share this deterministic available-data
/// result; keeping the matcher here makes a future CLDR distance table one
/// provider change rather than a per-service fork.
pub fn resolve_locale(
    service: IntlService,
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> LocaleResolution {
    let provider = locale_data_provider();
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| LocaleResolutionCandidate {
            supported: provider.supports_service_locale(service, requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    LocaleResolution {
        service,
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
    }
}

/// Returns the requested locale spellings supported by one service.
///
/// Unlike [`resolve_locale`], this never includes the fallback locale.
pub fn supported_locales(
    service: IntlService,
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    resolve_locale(service, requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
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
    let resolution = resolve_locale(IntlService::Collator, requested, matcher);
    CollationLocaleNegotiation {
        matcher: resolution.matcher,
        candidates: resolution
            .candidates
            .into_iter()
            .map(|candidate| CollationLocaleCandidate {
                requested: candidate.requested,
                supported: candidate.supported,
            })
            .collect(),
        selected: resolution.selected,
        used_default: resolution.used_default,
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
    supported_locales(IntlService::Collator, requested, matcher)
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

    #[test]
    fn internal_host_helpers_preserve_their_publicly_observable_records() {
        assert!(display_names::is_unicode_language_id(
            "qaa-Latn-419-1abc-abcde"
        ));
        assert!(!display_names::is_unicode_language_id(""));
        assert!(!display_names::is_unicode_language_id("en-a"));
        assert_eq!(
            display_names::canonical_unicode_language_id("QAA-lAtN-419-1ABC-ABCDE"),
            "qaa-Latn-419-1abc-abcde"
        );

        for (value, expected) in [
            ("+1", PluralCategory::One),
            ("2", PluralCategory::Two),
            ("20", PluralCategory::Few),
            ("13", PluralCategory::Other),
            ("1.0", PluralCategory::Many),
        ] {
            assert_eq!(
                plural_rules::SupplementalPluralRules::Manx.select(value),
                expected
            );
        }

        let input = "ab".encode_utf16().collect::<Vec<_>>();
        assert_eq!(
            segmenter_segments(&input, [(0, None), (1, Some(true)), (2, Some(false))]),
            vec![
                SegmenterSegment {
                    segment: "a".into(),
                    index_utf16: 0,
                    is_word_like: Some(true),
                },
                SegmenterSegment {
                    segment: "b".into(),
                    index_utf16: 1,
                    is_word_like: Some(false),
                },
            ]
        );

        use std::fmt::Write as _;
        let mut collector = ListPartCollector::default();
        collector.write_str("").unwrap();
        collector.write_str("before ").unwrap();
        collector.write_str("again ").unwrap();
        collector
            .with_part(icu_list::parts::ELEMENT, |writer| writer.write_str("A"))
            .unwrap();
        collector
            .with_part(icu_list::parts::LITERAL, |writer| writer.write_str(", "))
            .unwrap();
        collector
            .with_part(icu_list::parts::ELEMENT, |writer| writer.write_str("B"))
            .unwrap();
        assert_eq!(
            collector.parts,
            vec![
                ListPart {
                    kind: ListPartKind::Literal,
                    value: "before again ".into(),
                },
                ListPart {
                    kind: ListPartKind::Element,
                    value: "A".into(),
                },
                ListPart {
                    kind: ListPartKind::Literal,
                    value: ", ".into(),
                },
                ListPart {
                    kind: ListPartKind::Element,
                    value: "B".into(),
                },
            ]
        );
    }
}
