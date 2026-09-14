// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Version-pinned, host-neutral locale data used by the ECMA-402 services.
//!
//! ICU4X's compiled data supplies the localized algorithms. This provider is
//! the single place where BlueIce declares the subset it exposes through
//! ECMA-402 and the deterministic fallbacks around that data. It deliberately
//! does not negotiate locales: `ResolveLocale` remains a separate layer.

use icu_decimal::provider::{Baked as DecimalData, DecimalSymbolsV1};
use icu_locale_core::Locale as IcuLocale;
use icu_provider::{DataIdentifierBorrowed, DataProvider, DataRequest};

/// Immutable source revision for the ICU4X data bundle consumed by this crate.
///
/// This is a source revision, rather than a claimed upstream CLDR release:
/// the compatibility fork may contain narrowly scoped integration fixes while
/// retaining the upstream baked-data layout. Consumers that persist or
/// compare Intl output can record this value with their corpus revision.
pub const ICU4X_LOCALE_DATA_REVISION: &str = "31dcf42731d45cb191cdbd5bb92b669b5be12b57";

/// The immutable data inputs used by a BlueIce ECMA-402 build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocaleDataRevisions {
    /// The pinned ICU4X source revision containing compiled CLDR data.
    pub icu4x: &'static str,
    /// The IANA TZDB release compiled into the Jiff bundle.
    pub tzdb: &'static str,
}

/// Calendar identifiers backed by `Intl.DateTimeFormat`.
pub const SUPPORTED_CALENDARS: &[&str] = &[
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "gregory",
    "hebrew",
    "indian",
    "islamic-civil",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "persian",
    "roc",
];

/// Decimal numbering systems backed by date, number, and relative-time
/// formatting. The list includes every ECMA-402 simple digit mapping.
pub const SUPPORTED_NUMBERING_SYSTEMS: &[&str] = &[
    "adlm", "ahom", "arab", "arabext", "bali", "beng", "bhks", "brah", "cakm", "cham", "deva",
    "diak", "fullwide", "gara", "gong", "gonm", "gujr", "gukh", "guru", "hanidec", "hmng", "hmnp",
    "java", "kali", "kawi", "khmr", "knda", "krai", "lana", "lanatham", "laoo", "latn", "lepc",
    "limb", "mathbold", "mathdbl", "mathmono", "mathsanb", "mathsans", "mlym", "modi", "mong",
    "mroo", "mtei", "mymr", "mymrepka", "mymrpao", "mymrshan", "mymrtlng", "nagm", "newa", "nkoo",
    "olck", "onao", "orya", "osma", "outlined", "rohg", "saur", "segment", "shrd", "sind", "sinh",
    "sora", "sund", "sunu", "takr", "talu", "tamldec", "telu", "thai", "tibt", "tirh", "tnsa",
    "tols", "vaii", "wara", "wcho",
];

/// Collation types that can resolve through `Intl.Collator` for at least one
/// bundled locale.
pub const SUPPORTED_COLLATIONS: &[&str] = &[
    "compat", "dict", "emoji", "eor", "phonebk", "pinyin", "searchjl", "stroke", "trad", "unihan",
    "zhuyin",
];

/// ISO 4217 codes with localized `Intl.DisplayNames` data.
pub const SUPPORTED_CURRENCIES: &[&str] = &["EUR", "JPY", "USD"];

/// Sanctioned simple units whose NumberFormat patterns are implemented.
pub const SUPPORTED_UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

/// A capability represented by the bundled locale data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocaleDataCategory {
    /// The DateTimeFormat calendar registry.
    Calendar,
    /// The Collator collation-type registry.
    Collation,
    /// The DisplayNames currency registry.
    Currency,
    /// The decimal-digit-system registry.
    NumberingSystem,
    /// The NumberFormat unit registry.
    Unit,
}

/// An Intl service whose data coverage is declared by this provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntlService {
    /// `Intl.Collator` tailoring data.
    Collator,
    /// `Intl.DateTimeFormat` calendars, patterns, and time zones.
    DateTimeFormat,
    /// `Intl.DisplayNames` localized code names.
    DisplayNames,
    /// `Intl.DurationFormat` unit patterns.
    DurationFormat,
    /// `Intl.ListFormat` list patterns.
    ListFormat,
    /// `Intl.NumberFormat` decimal symbols and number patterns.
    NumberFormat,
    /// `Intl.PluralRules` plural categories.
    PluralRules,
    /// `Intl.RelativeTimeFormat` relative-time patterns.
    RelativeTimeFormat,
    /// `Intl.Segmenter` break rules.
    Segmenter,
}

/// Explicit capabilities for one service's data bundle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocaleDataCapabilities {
    /// Static `Intl.supportedValuesOf` registries consumed by the service.
    pub value_categories: &'static [LocaleDataCategory],
    /// Whether the service consumes the pinned IANA TZDB registry.
    pub uses_time_zone_data: bool,
}

/// The centralized locale-data provider.
///
/// The type has no mutable state: the source data is compiled into the pinned
/// ICU4X dependency and the small ECMA-402 exposure registry is static.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocaleDataProvider;

/// Returns the process-wide provider for every host-neutral Intl service.
pub const fn locale_data_provider() -> LocaleDataProvider {
    LocaleDataProvider
}

impl LocaleDataProvider {
    /// Returns the immutable ICU4X source revision for this provider.
    pub const fn revision(self) -> &'static str {
        ICU4X_LOCALE_DATA_REVISION
    }

    /// Returns every versioned locale-data input used by this provider.
    pub fn revisions(self) -> LocaleDataRevisions {
        LocaleDataRevisions {
            icu4x: self.revision(),
            tzdb: jiff_tzdb::VERSION.unwrap_or("unknown"),
        }
    }

    /// Returns the IANA TZDB release used by DateTimeFormat zone data.
    pub fn tzdb_version(self) -> &'static str {
        self.revisions().tzdb
    }

    /// Returns the registry values supplied by a static data category.
    pub const fn values(self, category: LocaleDataCategory) -> &'static [&'static str] {
        match category {
            LocaleDataCategory::Calendar => SUPPORTED_CALENDARS,
            LocaleDataCategory::Collation => SUPPORTED_COLLATIONS,
            LocaleDataCategory::Currency => SUPPORTED_CURRENCIES,
            LocaleDataCategory::NumberingSystem => SUPPORTED_NUMBERING_SYSTEMS,
            LocaleDataCategory::Unit => SUPPORTED_UNITS,
        }
    }

    /// Returns the registry and TZDB capabilities available to an Intl service.
    pub const fn capabilities(self, service: IntlService) -> LocaleDataCapabilities {
        const NONE: &[LocaleDataCategory] = &[];
        const CALENDAR_AND_NUMBERING: &[LocaleDataCategory] = &[
            LocaleDataCategory::Calendar,
            LocaleDataCategory::NumberingSystem,
        ];
        const DISPLAY_NAMES: &[LocaleDataCategory] =
            &[LocaleDataCategory::Calendar, LocaleDataCategory::Currency];
        const NUMBER_FORMAT: &[LocaleDataCategory] = &[
            LocaleDataCategory::Currency,
            LocaleDataCategory::NumberingSystem,
            LocaleDataCategory::Unit,
        ];
        const NUMBERING: &[LocaleDataCategory] = &[LocaleDataCategory::NumberingSystem];
        const COLLATION: &[LocaleDataCategory] = &[LocaleDataCategory::Collation];
        let value_categories = match service {
            IntlService::Collator => COLLATION,
            IntlService::DateTimeFormat => CALENDAR_AND_NUMBERING,
            IntlService::DisplayNames => DISPLAY_NAMES,
            IntlService::DurationFormat | IntlService::RelativeTimeFormat => NUMBERING,
            IntlService::NumberFormat => NUMBER_FORMAT,
            IntlService::ListFormat | IntlService::PluralRules | IntlService::Segmenter => NONE,
        };
        LocaleDataCapabilities {
            value_categories,
            uses_time_zone_data: matches!(service, IntlService::DateTimeFormat),
        }
    }

    /// Whether the compiled CLDR data has language-level fallback coverage.
    ///
    /// ICU4X compacts records whose values equal their parent locale. The
    /// provider therefore treats a supported primary language as data-backed
    /// even when the concrete marker resolves through that parent record.
    pub fn supports_language(self, locale: &IcuLocale) -> bool {
        const LANGUAGES: &str = "af am ar as az be bg bn bo br bs ca ceb chr cs cy da de dsb dz ee el en eo es et fa ff fi fil fo fr fy ga gl gu gv ha haw he hi hr hsb hu hy id ig is it ja ka kk kl km kn ko kok ku ky la lb lkt ln lo lt lv mk ml mn mr ms mt my nb ne nl nn no om or pa pl ps pt ro ru sa se si sk sl so sq sr sv sw ta te th tk to tr ug uk ur uz vi wae wo xh yi yo zh zu";
        LANGUAGES
            .split(' ')
            .any(|language| locale.id.language.as_str() == language)
    }

    /// Whether decimal-symbol data resolves to this locale's language.
    ///
    /// ICU4X's root fallback is useful for loading internal data but is not an
    /// ECMA-402 available-locale match. A marker that resolves through a
    /// parent record remains available when it retains the requested primary
    /// language.
    pub fn supports_decimal_locale(self, locale: &IcuLocale) -> bool {
        if self.supports_language(locale) {
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
        response.is_ok_and(|response| {
            response
                .metadata
                .locale
                .is_none_or(|resolved| resolved.language == requested.language)
        })
    }

    /// Whether a service has data coverage for this locale.
    ///
    /// This is data availability only. Locale-list ordering, lookup, best-fit,
    /// Unicode-extension handling, and option precedence remain the future
    /// shared `ResolveLocale` layer.
    pub fn supports_service_locale(self, service: IntlService, locale: &IcuLocale) -> bool {
        match service {
            IntlService::NumberFormat => self.supports_decimal_locale(locale),
            IntlService::DurationFormat => locale.id.language.as_str() == "en",
            _ => self.supports_language(locale),
        }
    }

    /// Applies the pinned likely-subtag data to an ICU locale.
    ///
    /// ICU4X 2.3 intentionally leaves an `und` primary language unchanged in
    /// this API. Current ECMA-402/UTS 35 requires the language selected by
    /// likely-subtags to become observable, so the provider supplies the
    /// missing CLDR 44+ undefined-language records after ICU's expansion.
    pub(crate) fn maximize_likely_subtags(self, locale: &IcuLocale) -> IcuLocale {
        let mut maximal = locale.clone();
        icu_locale::LocaleExpander::new_extended().maximize(&mut maximal.id);
        self.patch_undefined_likely_subtags(&mut maximal);
        maximal
    }

    /// Removes likely subtags using the same provider data used for expansion.
    pub(crate) fn minimize_likely_subtags(self, locale: &IcuLocale) -> IcuLocale {
        let mut minimal = self.maximize_likely_subtags(locale);
        icu_locale::LocaleExpander::new_extended().minimize(&mut minimal.id);
        self.patch_minimal_likely_subtags(&mut minimal);
        minimal
    }

    fn patch_undefined_likely_subtags(self, locale: &mut IcuLocale) {
        if locale.id.language.as_str() != "und" {
            return;
        }
        let script = locale.id.script.map(|script| script.to_string());
        let region = locale.id.region.map(|region| region.to_string());
        let (language, default_script, default_region) =
            match (script.as_deref(), region.as_deref()) {
                (Some("Thai"), _) => ("th", "Thai", "TH"),
                (Some("Cyrl"), Some("RO")) => ("bg", "Cyrl", "RO"),
                (_, Some("419")) => ("es", "Latn", "419"),
                (_, Some("AT")) => ("de", "Latn", "AT"),
                (_, Some("CW")) => ("pap", "Latn", "CW"),
                (_, Some("150")) => ("en", "Latn", "150"),
                (_, Some("AQ")) => ("en", "Latn", "AQ"),
                (_, Some("US")) => ("en", "Latn", "US"),
                _ => ("en", "Latn", "US"),
            };
        locale.id.language = language.parse().expect("a pinned language tag is valid");
        if locale.id.script.is_none() {
            locale.id.script = Some(
                default_script
                    .parse()
                    .expect("a pinned script tag is valid"),
            );
        }
        if locale.id.region.is_none() {
            locale.id.region = Some(
                default_region
                    .parse()
                    .expect("a pinned region tag is valid"),
            );
        }
    }

    fn patch_minimal_likely_subtags(self, locale: &mut IcuLocale) {
        if locale.id.language.as_str() == "zh"
            && locale
                .id
                .script
                .is_some_and(|script| script.as_str() == "Hant")
            && locale.id.region.is_none()
        {
            locale.id.script = None;
            locale.id.region = Some("TW".parse().expect("a pinned region tag is valid"));
        }
    }

    /// Whether a collation tailoring is data-backed for the selected locale.
    pub fn supports_collation(self, locale: &IcuLocale, collation: &str) -> bool {
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

    /// Whether a numbering-system identifier is present in the provider.
    pub const fn supports_numbering_system(self, value: &str) -> bool {
        contains(SUPPORTED_NUMBERING_SYSTEMS, value)
    }

    /// Returns the complete Zone-and-Link registry from the pinned TZDB
    /// bundle, in ECMA-402 canonical supported-values order.
    ///
    /// The UTC-equivalent legacy spellings are folded to `UTC`; all other
    /// accepted IANA Link identifiers are retained rather than replaced by
    /// their targets.
    pub fn time_zones(self) -> Vec<String> {
        let mut values = jiff_tzdb::available()
            .map(canonical_time_zone)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        values.sort_unstable();
        values.dedup();
        values
    }

    /// Returns DateTimeFormat's default calendar for a supported locale.
    pub fn default_calendar(self, locale: &IcuLocale) -> &'static str {
        match locale.id.language.as_str() {
            "fa" => "persian",
            "th" => "buddhist",
            _ => "gregory",
        }
    }

    /// Returns the locale's default decimal numbering system.
    pub fn default_numbering_system(self, locale: &IcuLocale) -> &'static str {
        match locale.id.language.as_str() {
            "ar" => "arab",
            "fa" => "arabext",
            "bn" => "beng",
            "my" => "mymr",
            _ => "latn",
        }
    }

    /// Returns the locale's default hour cycle.
    pub fn default_hour_cycle(self, locale: &IcuLocale) -> &'static str {
        match locale.id.language.as_str() {
            "ar" | "en" | "ko" => "h12",
            _ => "h23",
        }
    }

    pub(crate) fn calendars_for_region(self, region: &str) -> &'static [&'static str] {
        match region {
            "TH" => &["buddhist", "gregory"],
            "JP" => &["gregory", "japanese"],
            "IN" => &["gregory", "indian"],
            "IR" | "AF" => &[
                "persian",
                "gregory",
                "islamic",
                "islamic-civil",
                "islamic-tbla",
            ],
            "ET" => &["gregory", "ethiopic"],
            "BD" | "MY" | "PK" => &["gregory", "islamic", "islamic-civil", "islamic-tbla"],
            "KR" => &["gregory", "dangi"],
            _ => &["gregory"],
        }
    }

    pub(crate) fn collations_for_language(self, language: &str) -> &'static [&'static str] {
        if language == "und" || ("qfz"..="qtz").contains(&language) {
            &["emoji", "eor"]
        } else {
            &["emoji"]
        }
    }

    pub(crate) fn hour_cycle_for_locale(self, language: &str, region: &str) -> &'static str {
        match (language, region) {
            ("en", "US" | "CA" | "001") | ("ar", "001") => "h12",
            _ => match region {
                "US" | "IN" | "ET" | "BD" | "GR" | "PH" | "KR" | "MY" | "PK" => "h12",
                _ => "h23",
            },
        }
    }

    pub(crate) fn is_right_to_left(self, language: &str, script: Option<&str>) -> bool {
        script.is_some_and(|script| {
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
        )
    }

    pub(crate) fn time_zones_for_region(self, region: &str) -> &'static [&'static str] {
        match region {
            "US" => &[
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
            "GB" => &["Europe/London"],
            "JP" => &["Asia/Tokyo"],
            "TW" => &["Asia/Taipei"],
            _ => &["Etc/UTC"],
        }
    }

    pub(crate) fn week_data_for_region(self, region: &str) -> (u8, &'static [u8]) {
        match region {
            "US" | "CA" | "JP" | "TH" => (7, &[6, 7]),
            "IN" => (7, &[7]),
            "IR" => (6, &[5]),
            "AF" => (6, &[4, 5]),
            _ => (1, &[6, 7]),
        }
    }
}

const fn contains(values: &[&str], value: &str) -> bool {
    let mut index = 0;
    while index < values.len() {
        if str_eq(values[index], value) {
            return true;
        }
        index += 1;
    }
    false
}

const fn str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

fn canonical_time_zone(identifier: &str) -> &str {
    match identifier {
        "Etc/GMT" | "Etc/GMT0" | "Etc/UTC" | "GMT" | "GMT0" => "UTC",
        _ => identifier,
    }
}
