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
            _ => self.supports_language(locale),
        }
    }

    /// Selects provider data for a requested locale under one ECMA-402
    /// matching policy.
    ///
    /// Lookup preserves a directly available language-parent request. Best
    /// fit first takes that same exact path, then consults the pinned CLDR
    /// likely-subtag data. This gives structurally valid `und` requests a
    /// data-backed language/region match without inventing a service-local
    /// fallback table. The shared resolver decides request-list ordering and
    /// observable Unicode-key retention around this provider decision.
    pub(crate) fn match_service_locale(
        self,
        service: IntlService,
        locale: &IcuLocale,
        matcher: crate::LocaleMatcher,
    ) -> Option<IcuLocale> {
        if self.supports_service_locale(service, locale) {
            return Some(locale.clone());
        }
        if matcher != crate::LocaleMatcher::BestFit {
            return None;
        }
        let maximal = self.maximize_likely_subtags(locale);
        self.supports_service_locale(service, &maximal)
            .then(|| self.minimize_likely_subtags(&maximal))
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

    /// Looks up one localized name in the bounded DisplayNames data bundle.
    ///
    /// A missing entry is data absence, not a semantic fallback; the
    /// DisplayNames service applies its requested `code`/`none` policy after
    /// this lookup.
    pub(crate) fn display_name(
        self,
        locale: &str,
        display_type: crate::DisplayNamesType,
        style: crate::DisplayNamesStyle,
        language_display: Option<crate::DisplayNamesLanguageDisplay>,
        code: &str,
    ) -> Option<String> {
        use crate::{DisplayNamesLanguageDisplay, DisplayNamesStyle, DisplayNamesType};

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
            (DisplayNamesType::Calendar, "chinese") if english => "Chinese Calendar",
            (DisplayNamesType::Calendar, "coptic") if english => "Coptic Calendar",
            (DisplayNamesType::Calendar, "dangi") if english => "Dangi Calendar",
            (DisplayNamesType::Calendar, "ethioaa") if english => "Ethiopic Amete Alem Calendar",
            (DisplayNamesType::Calendar, "ethiopic") if english => "Ethiopic Calendar",
            (DisplayNamesType::Calendar, "hebrew") if english => "Hebrew Calendar",
            (DisplayNamesType::Calendar, "indian") if english => "Indian National Calendar",
            (DisplayNamesType::Calendar, "islamic-civil") if english => "Islamic Civil Calendar",
            (DisplayNamesType::Calendar, "islamic-tbla") if english => "Islamic Tabular Calendar",
            (DisplayNamesType::Calendar, "islamic-umalqura") if english => {
                "Islamic Calendar (Umm al-Qura)"
            }
            (DisplayNamesType::Calendar, "iso8601") if english => "ISO-8601 Calendar",
            (DisplayNamesType::Calendar, "japanese") if english => "Japanese Calendar",
            (DisplayNamesType::Calendar, "persian") if english => "Persian Calendar",
            (DisplayNamesType::Calendar, "roc") if english => "Minguo Calendar",
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
            // The bundle has no distinct short/narrow name. CLDR permits a
            // parent-width fallback, so retain the long form.
            DisplayNamesStyle::Short | DisplayNamesStyle::Narrow => name.into(),
        })
    }

    /// Returns the bundled DurationFormat unit pattern for `locale`.
    ///
    /// The provider carries the available Spanish CLDR width data alongside
    /// the original English bundle. Other supported languages resolve through
    /// the explicit English parent pattern until their own unit data is added.
    pub(crate) fn duration_unit_pattern(
        self,
        locale: &str,
        unit: crate::DurationUnit,
        style: crate::DurationUnitStyle,
        singular: bool,
    ) -> (&'static str, &'static str) {
        use crate::{DurationUnit, DurationUnitStyle};

        if locale.starts_with("es") {
            return spanish_duration_unit_pattern(unit, style, singular);
        }
        match style {
            DurationUnitStyle::Long => (
                " ",
                match (unit, singular) {
                    (DurationUnit::Years, true) => "year",
                    (DurationUnit::Months, true) => "month",
                    (DurationUnit::Weeks, true) => "week",
                    (DurationUnit::Days, true) => "day",
                    (DurationUnit::Hours, true) => "hour",
                    (DurationUnit::Minutes, true) => "minute",
                    (DurationUnit::Seconds, true) => "second",
                    (DurationUnit::Milliseconds, true) => "millisecond",
                    (DurationUnit::Microseconds, true) => "microsecond",
                    (DurationUnit::Nanoseconds, true) => "nanosecond",
                    (DurationUnit::Years, false) => "years",
                    (DurationUnit::Months, false) => "months",
                    (DurationUnit::Weeks, false) => "weeks",
                    (DurationUnit::Days, false) => "days",
                    (DurationUnit::Hours, false) => "hours",
                    (DurationUnit::Minutes, false) => "minutes",
                    (DurationUnit::Seconds, false) => "seconds",
                    (DurationUnit::Milliseconds, false) => "milliseconds",
                    (DurationUnit::Microseconds, false) => "microseconds",
                    (DurationUnit::Nanoseconds, false) => "nanoseconds",
                },
            ),
            DurationUnitStyle::Short => (
                " ",
                match (unit, singular) {
                    (DurationUnit::Years, true) => "yr",
                    (DurationUnit::Years, false) => "yrs",
                    (DurationUnit::Months, true) => "mth",
                    (DurationUnit::Months, false) => "mths",
                    (DurationUnit::Weeks, true) => "wk",
                    (DurationUnit::Weeks, false) => "wks",
                    (DurationUnit::Days, true) => "day",
                    (DurationUnit::Days, false) => "days",
                    (DurationUnit::Hours, _) => "hr",
                    (DurationUnit::Minutes, _) => "min",
                    (DurationUnit::Seconds, _) => "sec",
                    (DurationUnit::Milliseconds, _) => "ms",
                    (DurationUnit::Microseconds, _) => "μs",
                    (DurationUnit::Nanoseconds, _) => "ns",
                },
            ),
            DurationUnitStyle::Narrow => (
                "",
                match unit {
                    DurationUnit::Years => "y",
                    DurationUnit::Months => "m",
                    DurationUnit::Weeks => "w",
                    DurationUnit::Days => "d",
                    DurationUnit::Hours => "h",
                    DurationUnit::Minutes => "m",
                    DurationUnit::Seconds => "s",
                    DurationUnit::Milliseconds => "ms",
                    DurationUnit::Microseconds => "μs",
                    DurationUnit::Nanoseconds => "ns",
                },
            ),
            // Callers route numeric styles to FormatNumericUnits; the neutral
            // empty pattern keeps this data lookup total if one misroutes it.
            DurationUnitStyle::Numeric | DurationUnitStyle::TwoDigit => ("", ""),
        }
    }

    /// Returns whether the bundled relative-time data uses Polish patterns.
    pub(crate) fn relative_time_uses_polish(self, locale: &str) -> bool {
        locale.starts_with("pl")
    }

    /// Returns a qualitative English relative-time term when bundled data has
    /// one for `numeric: "auto"`.
    pub(crate) fn relative_time_qualitative_term(
        self,
        locale: &str,
        numeric: crate::RelativeTimeNumeric,
        value: f64,
        unit: crate::RelativeTimeUnit,
    ) -> Option<&'static str> {
        use crate::{RelativeTimeNumeric, RelativeTimeUnit};

        if numeric != RelativeTimeNumeric::Auto || !locale.starts_with("en") {
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

    /// Returns the bundled relative-time number affix for a finite value.
    pub(crate) fn relative_time_affixes(
        self,
        locale: &str,
        past: bool,
        label: &str,
    ) -> (&'static str, String) {
        let prefix = if past {
            ""
        } else if self.relative_time_uses_polish(locale) {
            "za "
        } else {
            "in "
        };
        let suffix = match (past, self.relative_time_uses_polish(locale)) {
            (true, true) => format!(" {label} temu"),
            (true, false) => format!(" {label} ago"),
            (false, _) => format!(" {label}"),
        };
        (prefix, suffix)
    }

    /// Returns the bundled relative-time unit pattern for one finite value.
    pub(crate) fn relative_time_unit_label(
        self,
        locale: &str,
        style: crate::RelativeTimeStyle,
        unit: crate::RelativeTimeUnit,
        value: f64,
    ) -> &'static str {
        if self.relative_time_uses_polish(locale) {
            return polish_relative_time_label(style, unit, value);
        }
        english_relative_time_label(style, unit, value)
    }
}

fn spanish_duration_unit_pattern(
    unit: crate::DurationUnit,
    style: crate::DurationUnitStyle,
    singular: bool,
) -> (&'static str, &'static str) {
    use crate::{DurationUnit, DurationUnitStyle};

    let label = match style {
        DurationUnitStyle::Long => match (unit, singular) {
            (DurationUnit::Years, true) => "año",
            (DurationUnit::Months, true) => "mes",
            (DurationUnit::Weeks, true) => "semana",
            (DurationUnit::Days, true) => "día",
            (DurationUnit::Hours, true) => "hora",
            (DurationUnit::Minutes, true) => "minuto",
            (DurationUnit::Seconds, true) => "segundo",
            (DurationUnit::Milliseconds, true) => "milisegundo",
            (DurationUnit::Microseconds, true) => "microsegundo",
            (DurationUnit::Nanoseconds, true) => "nanosegundo",
            (DurationUnit::Years, false) => "años",
            (DurationUnit::Months, false) => "meses",
            (DurationUnit::Weeks, false) => "semanas",
            (DurationUnit::Days, false) => "días",
            (DurationUnit::Hours, false) => "horas",
            (DurationUnit::Minutes, false) => "minutos",
            (DurationUnit::Seconds, false) => "segundos",
            (DurationUnit::Milliseconds, false) => "milisegundos",
            (DurationUnit::Microseconds, false) => "microsegundos",
            (DurationUnit::Nanoseconds, false) => "nanosegundos",
        },
        DurationUnitStyle::Short => match unit {
            DurationUnit::Years => "a",
            DurationUnit::Months => "m",
            DurationUnit::Weeks => "sem",
            DurationUnit::Days => "d",
            DurationUnit::Hours => "h",
            DurationUnit::Minutes => "min",
            DurationUnit::Seconds => "s",
            DurationUnit::Milliseconds => "ms",
            DurationUnit::Microseconds => "μs",
            DurationUnit::Nanoseconds => "ns",
        },
        DurationUnitStyle::Narrow => match unit {
            DurationUnit::Years => "a",
            DurationUnit::Months => "m",
            DurationUnit::Weeks => "sem",
            DurationUnit::Days => "d",
            DurationUnit::Hours => "h",
            DurationUnit::Minutes => "min",
            DurationUnit::Seconds => "s",
            DurationUnit::Milliseconds => "ms",
            DurationUnit::Microseconds => "μs",
            DurationUnit::Nanoseconds => "ns",
        },
        DurationUnitStyle::Numeric | DurationUnitStyle::TwoDigit => "",
    };
    let separator = if matches!(style, DurationUnitStyle::Narrow) {
        ""
    } else {
        " "
    };
    (separator, label)
}

fn english_relative_time_label(
    style: crate::RelativeTimeStyle,
    unit: crate::RelativeTimeUnit,
    value: f64,
) -> &'static str {
    use crate::{RelativeTimeStyle, RelativeTimeUnit};

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
    style: crate::RelativeTimeStyle,
    unit: crate::RelativeTimeUnit,
    value: f64,
) -> &'static str {
    use crate::{RelativeTimeStyle, RelativeTimeUnit};

    #[derive(Clone, Copy)]
    enum Category {
        One,
        Few,
        Many,
        Other,
    }

    let integer = value.fract() == 0.0;
    let number = value as i64;
    let category = if integer && number == 1 {
        Category::One
    } else if integer
        && (2..=4).contains(&(number.rem_euclid(10)))
        && !(12..=14).contains(&(number.rem_euclid(100)))
    {
        Category::Few
    } else if integer {
        Category::Many
    } else {
        Category::Other
    };
    let select = |one, few, many, other| match category {
        Category::One => one,
        Category::Few => few,
        Category::Many => many,
        Category::Other => other,
    };
    match style {
        RelativeTimeStyle::Long => match unit {
            RelativeTimeUnit::Second => select("sekundę", "sekundy", "sekund", "sekundy"),
            RelativeTimeUnit::Minute => select("minutę", "minuty", "minut", "minuty"),
            RelativeTimeUnit::Hour => select("godzinę", "godziny", "godzin", "godziny"),
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydzień", "tygodnie", "tygodni", "tygodnia"),
            RelativeTimeUnit::Month => select("miesiąc", "miesiące", "miesięcy", "miesiąca"),
            RelativeTimeUnit::Quarter => select("kwartał", "kwartały", "kwartałów", "kwartału"),
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
        RelativeTimeStyle::Short => match unit {
            RelativeTimeUnit::Second => "sek.",
            RelativeTimeUnit::Minute => "min",
            RelativeTimeUnit::Hour => "godz.",
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydz.", "tyg.", "tyg.", "tyg."),
            RelativeTimeUnit::Month => "mies.",
            RelativeTimeUnit::Quarter => "kw.",
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
        RelativeTimeStyle::Narrow => match unit {
            RelativeTimeUnit::Second => "s",
            RelativeTimeUnit::Minute => "min",
            RelativeTimeUnit::Hour => "g.",
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydz.", "tyg.", "tyg.", "tyg."),
            RelativeTimeUnit::Month => "mies.",
            RelativeTimeUnit::Quarter => "kw.",
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
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
