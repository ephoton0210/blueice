// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Intl.DateTimeFormat` data and IANA-zone formatting.
//!
//! The ECMAScript host is responsible for observable option conversion and
//! for selecting a host default zone. This module keeps the resolved service
//! data independent from a Realm and delegates localized calendar rendering
//! to ICU4X. A pinned Jiff TZDB bundle supplies IANA transition rules, so
//! formatting does not depend on the machine's installed zoneinfo files.

use crate::{canonicalize, unicode_keyword, CanonicalLocale, LocaleMatcher};
use icu_datetime::{
    fieldsets::{
        builder::{DateFields, FieldSetBuilder},
        enums::{CompositeDateTimeFieldSet, DateFieldSet},
        YMD,
    },
    input::{DateTime, ZonedDateTime},
    options::{Alignment, Length, SubsecondDigits, TimePrecision, YearStyle},
    range::{DateRangeFormatter, DATE_RANGE_PART_SOURCE_CATEGORY},
    DateTimeFormatter,
};
use jiff::{tz::TimeZoneDatabase, Timestamp};
use std::fmt;
use std::sync::OnceLock;
use writeable::{Part, PartsWrite, Writeable};

const TIME_CLIP_LIMIT: f64 = 8_640_000_000_000_000.0;

fn time_clip_milliseconds(epoch_milliseconds: f64) -> Result<i64, DateTimeFormatError> {
    if !epoch_milliseconds.is_finite() || epoch_milliseconds.abs() > TIME_CLIP_LIMIT {
        return Err(DateTimeFormatError::InvalidTime);
    }
    Ok(epoch_milliseconds.trunc() as i64)
}

/// A date/time field width selected by `Intl.DateTimeFormat`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DateTimeWidth {
    Numeric,
    TwoDigit,
    Short,
    Long,
    Narrow,
}

/// The component-pattern selection policy requested by `formatMatcher`.
///
/// `best fit` is implementation-defined by ECMA-402 and delegates to ICU4X's
/// CLDR semantic skeleton matcher. `basic` is deliberately retained in the
/// resolved service input instead of being discarded at the VM boundary, so a
/// future available-format scorer can use the caller's requested policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DateTimeFormatMatcher {
    Basic,
    #[default]
    BestFit,
}

/// The host-neutral options accepted by the DateTimeFormat service.
#[derive(Clone, Debug)]
pub struct DateTimeFormatOptions {
    /// Host policy switch for ICU4X's CLDR interval formatter. This is
    /// deliberately not an ECMAScript option; JavaScript callers continue to
    /// use only standard `Intl.DateTimeFormat` options. New formatters use
    /// the CLDR formatter by default; `false` remains available only for a
    /// host that needs the legacy compatibility path while upgrading ICU4X.
    pub use_icu4x_range_formatter: bool,
    pub locale_matcher: LocaleMatcher,
    pub format_matcher: DateTimeFormatMatcher,
    pub calendar: Option<String>,
    pub numbering_system: Option<String>,
    pub hour_cycle: Option<String>,
    pub hour12: Option<bool>,
    pub time_zone: Option<String>,
    pub weekday: Option<DateTimeWidth>,
    pub era: Option<DateTimeWidth>,
    pub year: Option<DateTimeWidth>,
    pub month: Option<DateTimeWidth>,
    pub day: Option<DateTimeWidth>,
    pub day_period: Option<DateTimeWidth>,
    pub hour: Option<DateTimeWidth>,
    pub minute: Option<DateTimeWidth>,
    pub second: Option<DateTimeWidth>,
    pub fractional_second_digits: Option<u8>,
    pub time_zone_name: Option<String>,
    pub date_style: Option<DateTimeStyle>,
    pub time_style: Option<DateTimeStyle>,
}

impl Default for DateTimeFormatOptions {
    fn default() -> Self {
        Self {
            use_icu4x_range_formatter: true,
            locale_matcher: LocaleMatcher::default(),
            format_matcher: DateTimeFormatMatcher::default(),
            calendar: None,
            numbering_system: None,
            hour_cycle: None,
            hour12: None,
            time_zone: None,
            weekday: None,
            era: None,
            year: None,
            month: None,
            day: None,
            day_period: None,
            hour: None,
            minute: None,
            second: None,
            fractional_second_digits: None,
            time_zone_name: None,
            date_style: None,
            time_style: None,
        }
    }
}

/// The `dateStyle` and `timeStyle` values prescribed by ECMA-402.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DateTimeStyle {
    Full,
    Long,
    Medium,
    Short,
}

/// One `Intl.DateTimeFormat.prototype.formatToParts` record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateTimePart {
    pub kind: String,
    pub value: String,
}

/// Which end of a formatted range supplied a part.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DateTimeRangePartSource {
    Shared,
    StartRange,
    EndRange,
}

/// One `Intl.DateTimeFormat.prototype.formatRangeToParts` record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateTimeRangePart {
    pub kind: String,
    pub value: String,
    pub source: DateTimeRangePartSource,
}

/// The version of the IANA Time Zone Database compiled into BlueIce.
///
/// The value comes from the pinned `jiff-tzdb` crate rather than the host's
/// zoneinfo installation, making an engine build reproducible across hosts.
pub fn bundled_tzdb_version() -> &'static str {
    jiff_tzdb::VERSION.unwrap_or("unknown")
}

/// A failure from DateTimeFormat construction or formatting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DateTimeFormatError {
    InvalidTime,
    UnsupportedTimeZone,
    Formatter,
}

impl fmt::Display for DateTimeFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTime => formatter.write_str("invalid time value"),
            Self::UnsupportedTimeZone => formatter.write_str("unsupported time zone"),
            Self::Formatter => formatter.write_str("could not initialize date-time formatter"),
        }
    }
}

impl std::error::Error for DateTimeFormatError {}

/// A locale-aware ECMA-402 DateTimeFormat service.
#[derive(Clone, Debug)]
pub struct DateTimeFormat {
    locale: CanonicalLocale,
    format_locale: CanonicalLocale,
    options: DateTimeFormatOptions,
    calendar: String,
    numbering_system: String,
    hour_cycle: String,
    time_zone: String,
    // Offset identifiers are valid ECMA-402 time zones but are intentionally
    // not IANA names. Keep the parsed offset alongside its normalized
    // identifier instead of trying to synthesize an `Etc/GMT` name: those
    // names only cover whole-hour offsets and reverse their sign.
    fixed_offset_seconds: Option<i32>,
}

/// The locale data resolved by `CreateDateTimeFormat`.
///
/// ECMA-402 exposes only accepted `ca`, `nu`, and `hc` extension values in
/// `resolvedOptions().locale`, while ICU needs the effective value for all
/// three keys to select patterns and number symbols. Keeping those two locale
/// forms together prevents an unsupported requested keyword from leaking into
/// the observable locale or from influencing formatting by accident.
struct ResolvedDateTimeLocale {
    locale: CanonicalLocale,
    format_locale: CanonicalLocale,
    calendar: String,
    numbering_system: String,
    hour_cycle: String,
}

const SUPPORTED_CALENDARS: &[&str] = &[
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

// These are the algorithmic and CLDR decimal systems carried by the pinned
// ICU4X data. A syntactically valid but absent `nu` value is deliberately not
// an error: ResolveLocale must fall back to the locale default.
const SUPPORTED_NUMBERING_SYSTEMS: &[&str] = &[
    "adlm", "ahom", "arab", "arabext", "bali", "beng", "bhks", "brah", "cakm", "cham", "deva",
    "diak", "fullwide", "gong", "gonm", "gujr", "guru", "hanidec", "hmng", "hmnp", "java", "kali",
    "khmr", "knda", "lana", "lanatham", "laoo", "latn", "lepc", "limb", "mathbold", "mathdbl",
    "mathmono", "mathsanb", "mathsans", "mlym", "modi", "mong", "mroo", "mtei", "mymr", "mymrepka",
    "mymrpao", "mymrshan", "mymrtlng", "nagm", "newa", "nkoo", "olck", "orya", "osma", "outlined",
    "rohg", "saur", "segment", "shrd", "sind", "sinh", "sora", "sund", "takr", "talu", "tamldec",
    "telu", "thai", "tibt", "tirh", "tnsa", "vaii", "wara", "wcho",
];

fn unicode_type(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

fn canonical_calendar(value: &str) -> Result<Option<&'static str>, DateTimeFormatError> {
    if !unicode_type(value) {
        return Err(DateTimeFormatError::Formatter);
    }
    let value = value.to_ascii_lowercase();
    let value = match value.as_str() {
        "ethiopic-amete-alem" => "ethioaa",
        "islamicc" => "islamic-civil",
        // ECMA-402 requires these aliases to select an available calendar.
        // ICU4X resolves them through regional data; BlueIce's deterministic
        // locale bundle uses the civil tabular calendar as the fallback.
        "islamic" | "islamic-rgsa" => "islamic-civil",
        _ => value.as_str(),
    };
    if !SUPPORTED_CALENDARS.contains(&value) {
        return Ok(None);
    }
    Ok(Some(match value {
        "buddhist" => "buddhist",
        "chinese" => "chinese",
        "coptic" => "coptic",
        "dangi" => "dangi",
        "ethioaa" => "ethioaa",
        "ethiopic" => "ethiopic",
        "gregory" => "gregory",
        "hebrew" => "hebrew",
        "indian" => "indian",
        "islamic-civil" => "islamic-civil",
        "islamic-tbla" => "islamic-tbla",
        "islamic-umalqura" => "islamic-umalqura",
        "iso8601" => "iso8601",
        "japanese" => "japanese",
        "persian" => "persian",
        "roc" => "roc",
        _ => unreachable!("SUPPORTED_CALENDARS is exhaustive"),
    }))
}

fn canonical_numbering_system(value: &str) -> Result<Option<String>, DateTimeFormatError> {
    if !unicode_type(value) {
        return Err(DateTimeFormatError::Formatter);
    }
    let value = value.to_ascii_lowercase();
    Ok(SUPPORTED_NUMBERING_SYSTEMS
        .contains(&value.as_str())
        .then_some(value))
}

fn default_calendar(locale: &CanonicalLocale) -> &'static str {
    match locale.locale().id.language.as_str() {
        "fa" => "persian",
        "th" => "buddhist",
        _ => "gregory",
    }
}

fn default_numbering_system(locale: &CanonicalLocale) -> &'static str {
    match locale.locale().id.language.as_str() {
        "ar" => "arab",
        "fa" => "arabext",
        "bn" => "beng",
        "my" => "mymr",
        _ => "latn",
    }
}

fn default_hour_cycle(locale: &CanonicalLocale) -> &'static str {
    match locale.locale().id.language.as_str() {
        "ar" | "en" | "ko" => "h12",
        _ => "h23",
    }
}

fn canonical_hour_cycle(value: &str) -> Option<&'static str> {
    match value {
        "h11" => Some("h11"),
        "h12" => Some("h12"),
        "h23" => Some("h23"),
        "h24" => Some("h24"),
        _ => None,
    }
}

fn locale_with_date_time_keywords(
    initial: &CanonicalLocale,
    calendar: Option<&str>,
    numbering_system: Option<&str>,
    hour_cycle: Option<&str>,
) -> Result<CanonicalLocale, DateTimeFormatError> {
    let mut locale = initial.locale().clone();
    // ResolveLocale includes only the service's relevant Unicode keys. This
    // also keeps unrelated keys such as `cu` and `tz` from changing the
    // DateTimeFormat service state.
    locale.extensions.unicode.clear();
    for (key, value) in [
        ("ca", calendar),
        ("nu", numbering_system),
        ("hc", hour_cycle),
    ] {
        if let Some(value) = value {
            locale.extensions.unicode.keywords.set(
                key.parse().expect("a DateTimeFormat Unicode key is valid"),
                value.parse().map_err(|_| DateTimeFormatError::Formatter)?,
            );
        }
    }
    canonicalize(&locale.to_string()).map_err(|_| DateTimeFormatError::Formatter)
}

fn resolve_date_time_locale(
    selected: &CanonicalLocale,
    options: &DateTimeFormatOptions,
) -> Result<ResolvedDateTimeLocale, DateTimeFormatError> {
    let extension_calendar = unicode_keyword(selected.locale(), "ca")
        .as_deref()
        .map(canonical_calendar)
        .transpose()?
        .flatten();
    let option_calendar = options
        .calendar
        .as_deref()
        .map(canonical_calendar)
        .transpose()?
        .flatten();
    let calendar = option_calendar
        .or(extension_calendar)
        .unwrap_or_else(|| default_calendar(selected));
    let calendar_extension = extension_calendar
        .filter(|extension| option_calendar.is_none() || option_calendar == Some(*extension));

    let extension_numbering = unicode_keyword(selected.locale(), "nu")
        .as_deref()
        .map(canonical_numbering_system)
        .transpose()?
        .flatten();
    let option_numbering = options
        .numbering_system
        .as_deref()
        .map(canonical_numbering_system)
        .transpose()?
        .flatten();
    let numbering_system = option_numbering
        .as_deref()
        .or(extension_numbering.as_deref())
        .unwrap_or_else(|| default_numbering_system(selected))
        .to_owned();
    let numbering_extension = extension_numbering.filter(|extension| {
        option_numbering.is_none() || option_numbering.as_deref() == Some(extension)
    });

    let extension_hour_cycle = unicode_keyword(selected.locale(), "hc")
        .as_deref()
        .and_then(canonical_hour_cycle);
    let option_hour_cycle = options.hour_cycle.as_deref().and_then(canonical_hour_cycle);
    let hour_cycle = match options.hour12 {
        Some(true) if selected.locale().id.language.as_str() == "ja" => "h11",
        Some(true) => "h12",
        Some(false) => "h23",
        None => option_hour_cycle
            .or(extension_hour_cycle)
            .unwrap_or_else(|| default_hour_cycle(selected)),
    };
    let hour_cycle_extension = (options.hour12.is_none())
        .then_some(extension_hour_cycle)
        .flatten()
        .filter(|extension| option_hour_cycle.is_none() || option_hour_cycle == Some(*extension));

    // ICU4X deliberately interprets `iso8601` as the locale's default
    // calendar. ECMA-402 instead exposes `iso8601` while formatting the ISO
    // (Gregorian) calendar, so pass the latter to ICU while retaining the
    // observable resolved calendar.
    let formatting_calendar = if calendar == "iso8601" {
        "gregory"
    } else {
        calendar
    };
    // ICU4X's current semantic skeleton accepts h11/h12/h23. Use its
    // 24-hour skeleton for h24 while retaining the ECMA-402-visible h24
    // choice in the resolved service state.
    let formatting_hour_cycle = if hour_cycle == "h24" {
        "h23"
    } else {
        hour_cycle
    };
    let format_locale = locale_with_date_time_keywords(
        selected,
        Some(formatting_calendar),
        Some(&numbering_system),
        Some(formatting_hour_cycle),
    )?;
    let locale = locale_with_date_time_keywords(
        selected,
        calendar_extension,
        numbering_extension.as_deref(),
        hour_cycle_extension,
    )?;
    Ok(ResolvedDateTimeLocale {
        locale,
        format_locale,
        calendar: calendar.into(),
        numbering_system,
        hour_cycle: hour_cycle.into(),
    })
}

impl DateTimeFormat {
    /// Builds a DateTimeFormat from already-canonical locale requests.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: DateTimeFormatOptions,
    ) -> Result<Self, DateTimeFormatError> {
        let selected_locale = crate::resolve_collation_locale(requested, options.locale_matcher);
        let requested_time_zone = options.time_zone.clone().unwrap_or_else(|| "UTC".into());
        let (time_zone, fixed_offset_seconds) = match parse_time_zone_offset(&requested_time_zone) {
            Some((identifier, seconds)) => (identifier, Some(seconds)),
            None => {
                // ECMA-402 canonicalizes ASCII casing, but (unlike older
                // editions) deliberately retains the accepted Zone-or-Link
                // identifier rather than replacing an alias with its target.
                // jiff-tzdb carries the complete, pinned IANA name table and
                // gives us that case-normalized identifier directly.
                let Some((identifier, _)) = jiff_tzdb::get(&requested_time_zone) else {
                    return Err(DateTimeFormatError::UnsupportedTimeZone);
                };
                time_zone_database()
                    .get(identifier)
                    .map_err(|_| DateTimeFormatError::UnsupportedTimeZone)?;
                (identifier.into(), None)
            }
        };
        let resolved_locale = resolve_date_time_locale(&selected_locale, &options)?;
        Ok(Self {
            locale: resolved_locale.locale,
            format_locale: resolved_locale.format_locale,
            options,
            calendar: resolved_locale.calendar,
            numbering_system: resolved_locale.numbering_system,
            hour_cycle: resolved_locale.hour_cycle,
            time_zone,
            fixed_offset_seconds,
        })
    }

    /// The selected locale.
    pub fn locale(&self) -> &str {
        self.locale.as_str()
    }

    pub fn calendar(&self) -> &str {
        &self.calendar
    }

    pub fn numbering_system(&self) -> &str {
        &self.numbering_system
    }

    pub fn hour_cycle(&self) -> &str {
        &self.hour_cycle
    }

    pub fn time_zone(&self) -> &str {
        &self.time_zone
    }

    pub fn options(&self) -> &DateTimeFormatOptions {
        &self.options
    }

    /// Returns the component-pattern policy selected from `formatMatcher`.
    /// It is intentionally not exposed from JavaScript `resolvedOptions()`,
    /// which has no `formatMatcher` property.
    pub fn format_matcher(&self) -> DateTimeFormatMatcher {
        self.options.format_matcher
    }

    /// Formats an ECMAScript time value, expressed in milliseconds since the
    /// Unix epoch. ECMA-402 receives TimeClip'd Date values, but callers may
    /// also pass a number directly, so reject non-finite and out-of-range
    /// inputs here as required by `FormatDateTime`.
    pub fn format(&self, epoch_milliseconds: f64) -> Result<String, DateTimeFormatError> {
        Ok(self
            .format_to_parts(epoch_milliseconds)?
            .into_iter()
            .map(|part| part.value)
            .collect())
    }

    /// Formats a range with ICU4X's CLDR interval pattern data.
    pub fn format_range(&self, start: f64, end: f64) -> Result<String, DateTimeFormatError> {
        Ok(self
            .format_range_to_parts(start, end)?
            .into_iter()
            .map(|part| part.value)
            .collect())
    }

    /// Formats a range while retaining the ECMA-402 `source` for every part.
    pub fn format_range_to_parts(
        &self,
        start: f64,
        end: f64,
    ) -> Result<Vec<DateTimeRangePart>, DateTimeFormatError> {
        // ICU4X's DateRangeFormatter accepts date/time input but no external
        // IANA-zone display data. Its time-only and fractional-second
        // interval skeletons also emit malformed end spans. Non-Gregorian
        // calendars need a separate range data path. Keep explicitly limited
        // compatibility paths for those inputs; every supported range is
        // formed by the CLDR interval pattern.
        if !self.options.use_icu4x_range_formatter
            || self.effective_time_zone_name().is_some()
            || self.options.fractional_second_digits.is_some()
            || (self.date_fields().is_none() && self.time_precision().is_some())
            || self.calendar != "gregory"
        {
            return self.format_range_with_zone_name(start, end);
        }
        let start_milliseconds = time_clip_milliseconds(start)?;
        let end_milliseconds = time_clip_milliseconds(end)?;
        let start_parts = self.format_to_parts_from_milliseconds(start_milliseconds, false)?;
        let end_parts = self.format_to_parts_from_milliseconds(end_milliseconds, false)?;
        // ECMA-402 returns one normal pattern when the requested fields have
        // the same displayed values. This is an equality decision for the
        // entire formatted result, before range-pattern serialization; it
        // does not rewrite ownership of individual CLDR range spans.
        if start_parts == end_parts {
            return Ok(start_parts.iter().map(shared_range_part).collect());
        }
        let (start_datetime, _, _) = self.datetime_from_milliseconds(start_milliseconds)?;
        let (end_datetime, _, _) = self.datetime_from_milliseconds(end_milliseconds)?;
        self.format_datetime_range_to_parts(start_datetime, end_datetime)
    }

    fn format_datetime_range_to_parts(
        &self,
        start: DateTime<icu_calendar::Iso>,
        end: DateTime<icu_calendar::Iso>,
    ) -> Result<Vec<DateTimeRangePart>, DateTimeFormatError> {
        let formatter = DateRangeFormatter::try_new(
            self.format_locale.locale().clone().into(),
            self.field_set(),
        )
        .map_err(|_| DateTimeFormatError::Formatter)?;
        let formatted = formatter.format(&start, &end);
        let mut writer = PartWriter::default();
        formatted
            .write_to_parts_with_source(&mut writer)
            .map_err(|_| DateTimeFormatError::Formatter)?;
        let mut parts = self.filter_unrequested_range_parts(split_range_fractional_seconds(
            writer.into_range_parts(),
            self.options.fractional_second_digits,
        ));
        let start = self.format_to_parts_from_datetime(&start)?;
        let end = self.format_to_parts_from_datetime(&end)?;
        normalize_range_part_sources(&mut parts, &start, &end);
        Ok(parts)
    }

    /// Formats a time value and retains every ICU date-time field boundary.
    pub fn format_to_parts(
        &self,
        epoch_milliseconds: f64,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let milliseconds = time_clip_milliseconds(epoch_milliseconds)?;
        self.format_to_parts_from_milliseconds(milliseconds, true)
    }

    /// Formats a Temporal plain value's ISO local calendar fields.
    ///
    /// `ToDateTimeFormattable` carries plain values through a UTC date-time
    /// only to preserve their fields; it does not convert them to an instant.
    /// Consequently this intentionally bypasses TimeClip, ignores the
    /// formatter's requested time zone, and never emits a time-zone name.
    /// The embedding VM has already removed components that do not overlap
    /// with the Temporal value's data model.
    pub fn format_temporal_to_parts(
        &self,
        milliseconds: i64,
        options: DateTimeFormatOptions,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let formatter = Self::try_new(std::slice::from_ref(&self.locale), options)?;
        formatter.format_to_parts_from_milliseconds(milliseconds, false)
    }

    /// Formats an instant with the supplied, already-resolved service options.
    /// This keeps `Temporal.Instant` on the ordinary time-zone-aware path
    /// while allowing `ToDateTimeFormattable` to select its type-specific
    /// default components.
    pub fn format_to_parts_with_options(
        &self,
        epoch_milliseconds: f64,
        options: DateTimeFormatOptions,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let formatter = Self::try_new(std::slice::from_ref(&self.locale), options)?;
        formatter.format_to_parts(epoch_milliseconds)
    }

    /// Formats Temporal plain-object calendar fields. Unlike legacy Date and
    /// number input, Temporal values are not subject to TimeClip. The caller
    /// supplies an already-pruned option record and uses UTC solely as a
    /// calendar carrier: Temporal plain values intentionally ignore the
    /// formatter's time zone and never render its zone name.
    pub fn format_temporal_range_to_parts(
        &self,
        start_milliseconds: i64,
        end_milliseconds: i64,
        options: DateTimeFormatOptions,
        repeat_endpoints: bool,
    ) -> Result<Vec<DateTimeRangePart>, DateTimeFormatError> {
        let formatter = Self::try_new(std::slice::from_ref(&self.locale), options)?;
        let start = formatter.format_to_parts_from_milliseconds(start_milliseconds, false)?;
        let end = formatter.format_to_parts_from_milliseconds(end_milliseconds, false)?;
        if !formatter.options.use_icu4x_range_formatter {
            if repeat_endpoints && start != end {
                return Ok(join_range_parts(
                    &start,
                    &end,
                    formatter.range_format().separator,
                ));
            }
            return Ok(formatter.format_range_from_parts(start, end));
        }
        if start == end {
            return Ok(start.iter().map(shared_range_part).collect());
        }
        let (start_datetime, _, _) = formatter.datetime_from_milliseconds(start_milliseconds)?;
        let (end_datetime, _, _) = formatter.datetime_from_milliseconds(end_milliseconds)?;
        formatter.format_datetime_range_to_parts(start_datetime, end_datetime)
    }

    /// Converts an epoch value into ICU4X's calendar input. ICU4X's input
    /// supports the entire `i64` millisecond domain, while Jiff's `Timestamp`
    /// intentionally stops at ISO year +/-9999. ECMA-402 must nevertheless
    /// accept TimeClip's +/-275,760-year endpoints for UTC and fixed zones.
    fn datetime_from_milliseconds(
        &self,
        milliseconds: i64,
    ) -> Result<(DateTime<icu_calendar::Iso>, i32, String), DateTimeFormatError> {
        if let Some(offset_seconds) = self.fixed_offset_seconds {
            let zoned = ZonedDateTime::from_epoch_milliseconds_and_utc_offset(
                milliseconds,
                icu_datetime::input::UtcOffset::try_from_seconds(offset_seconds)
                    .map_err(|_| DateTimeFormatError::Formatter)?,
            );
            return Ok((
                DateTime {
                    date: zoned.date,
                    time: zoned.time,
                },
                offset_seconds,
                gmt_offset(offset_seconds, false),
            ));
        }
        let time_zone = time_zone_database()
            .get(&self.time_zone)
            .map_err(|_| DateTimeFormatError::UnsupportedTimeZone)?;
        let (offset_seconds, abbreviation) = match Timestamp::from_millisecond(milliseconds) {
            Ok(timestamp) => {
                let info = time_zone.to_offset_info(timestamp);
                (info.offset().seconds(), info.abbreviation().to_string())
            }
            // Jiff's fixed-zone result remains correct outside its civil
            // Timestamp range. For named zones, map the instant into the
            // equivalent position of the Gregorian 400-year cycle. That keeps
            // the ISO date, time, and weekday while allowing Jiff's recurring
            // IANA rule to provide an offset instead of rejecting a valid
            // TimeClip endpoint merely because it is outside Jiff's civil
            // representation.
            Err(_) => match time_zone.to_fixed_offset() {
                Ok(offset) => (offset.seconds(), offset.to_string()),
                Err(_) => {
                    const GREGORIAN_400_YEAR_MILLISECONDS: i64 = 146_097 * 86_400_000;
                    let timestamp = Timestamp::from_millisecond(
                        milliseconds.rem_euclid(GREGORIAN_400_YEAR_MILLISECONDS),
                    )
                    .expect("a 400-year Gregorian cycle fits Jiff's Timestamp range");
                    let info = time_zone.to_offset_info(timestamp);
                    (info.offset().seconds(), info.abbreviation().to_string())
                }
            },
        };
        let zoned = ZonedDateTime::from_epoch_milliseconds_and_utc_offset(
            milliseconds,
            icu_datetime::input::UtcOffset::try_from_seconds(offset_seconds)
                .map_err(|_| DateTimeFormatError::Formatter)?,
        );
        Ok((
            DateTime {
                date: zoned.date,
                time: zoned.time,
            },
            offset_seconds,
            abbreviation,
        ))
    }

    fn format_to_parts_from_milliseconds(
        &self,
        milliseconds: i64,
        include_time_zone_name: bool,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let (datetime, offset_seconds, abbreviation) =
            self.datetime_from_milliseconds(milliseconds)?;
        let formatter = DateTimeFormatter::try_new(
            self.format_locale.locale().clone().into(),
            self.field_set(),
        )
        .map_err(|_| DateTimeFormatError::Formatter)?;
        let formatted = formatter.format(&datetime);
        let mut writer = PartWriter::default();
        formatted
            .write_to_parts(&mut writer)
            .map_err(|_| DateTimeFormatError::Formatter)?;
        let mut parts = self.filter_unrequested_parts(split_fractional_seconds(
            writer.into_parts(),
            self.options.fractional_second_digits,
        ));
        self.apply_numbering_system_punctuation(&mut parts);
        self.apply_style_part_widths(&mut parts);
        self.apply_flexible_day_period(&mut parts, datetime.time.hour.number());
        self.apply_calendar_part_completeness(&mut parts);
        if include_time_zone_name {
            if let Some(style) = self.effective_time_zone_name() {
                parts.push(DateTimePart {
                    kind: "literal".into(),
                    value: " ".into(),
                });
                parts.push(DateTimePart {
                    kind: "timeZoneName".into(),
                    value: time_zone_display_name(
                        style,
                        &self.time_zone,
                        offset_seconds,
                        &abbreviation,
                    ),
                });
            }
        }
        Ok(parts)
    }

    fn format_to_parts_from_datetime(
        &self,
        datetime: &DateTime<icu_calendar::Iso>,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let formatter = DateTimeFormatter::try_new(
            self.format_locale.locale().clone().into(),
            self.field_set(),
        )
        .map_err(|_| DateTimeFormatError::Formatter)?;
        let formatted = formatter.format(datetime);
        let mut writer = PartWriter::default();
        formatted
            .write_to_parts(&mut writer)
            .map_err(|_| DateTimeFormatError::Formatter)?;
        let mut parts = self.filter_unrequested_parts(split_fractional_seconds(
            writer.into_parts(),
            self.options.fractional_second_digits,
        ));
        self.apply_numbering_system_punctuation(&mut parts);
        self.apply_style_part_widths(&mut parts);
        self.apply_flexible_day_period(&mut parts, datetime.time.hour.number());
        self.apply_calendar_part_completeness(&mut parts);
        Ok(parts)
    }

    fn field_set(&self) -> CompositeDateTimeFieldSet {
        let mut builder = FieldSetBuilder::new();
        builder.length = Some(self.length());
        builder.date_fields = self.date_fields();
        builder.time_precision = self.time_precision();
        builder.alignment = self.uses_two_digit_fields().then_some(Alignment::Column);
        builder.year_style = self.year_style();
        // Every builder input is derived from a valid ECMA-402 option set. A
        // final YMD fallback is retained to make formatter construction total
        // even if ICU4X adds a new validation rule.
        builder
            .build_composite_datetime()
            .unwrap_or(CompositeDateTimeFieldSet::Date(DateFieldSet::YMD(
                YMD::medium(),
            )))
    }

    fn date_fields(&self) -> Option<DateFields> {
        if let Some(style) = self.options.date_style {
            return Some(if style == DateTimeStyle::Full {
                DateFields::YMDE
            } else {
                DateFields::YMD
            });
        }
        let year = self.options.year.is_some();
        let month = self.options.month.is_some();
        let day = self.options.day.is_some();
        let weekday = self.options.weekday.is_some();
        match (year, month, day, weekday) {
            (false, false, false, false) => {
                self.uses_default_date_pattern().then_some(DateFields::YMD)
            }
            (false, false, false, true) => Some(DateFields::E),
            (false, false, true, false) => Some(DateFields::D),
            (false, false, true, true) => Some(DateFields::DE),
            (false, true, false, false) => Some(DateFields::M),
            (false, true, false, true) => Some(DateFields::MDE),
            (false, true, true, false) => Some(DateFields::MD),
            (false, true, true, true) => Some(DateFields::MDE),
            (true, false, false, false) => Some(DateFields::Y),
            (true, false, false, true) => Some(DateFields::YMDE),
            (true, false, true, false) => Some(DateFields::YMD),
            (true, false, true, true) => Some(DateFields::YMDE),
            (true, true, false, false) => Some(DateFields::YM),
            (true, true, false, true) => Some(DateFields::YMDE),
            (true, true, true, false) => Some(DateFields::YMD),
            (true, true, true, true) => Some(DateFields::YMDE),
        }
    }

    fn time_precision(&self) -> Option<TimePrecision> {
        if let Some(digits) = self.options.fractional_second_digits {
            return Some(TimePrecision::Subsecond(
                SubsecondDigits::try_from_int(digits).unwrap_or(SubsecondDigits::S3),
            ));
        }
        if self.options.second.is_some()
            || matches!(
                self.options.time_style,
                Some(DateTimeStyle::Full | DateTimeStyle::Long | DateTimeStyle::Medium)
            )
        {
            Some(TimePrecision::Second)
        } else if self.options.minute.is_some()
            || self.options.time_style == Some(DateTimeStyle::Short)
        {
            Some(TimePrecision::Minute)
        } else if self.options.hour.is_some() || self.options.day_period.is_some() {
            Some(TimePrecision::Hour)
        } else {
            None
        }
    }

    fn year_style(&self) -> Option<YearStyle> {
        if self.options.era.is_some() {
            Some(YearStyle::WithEra)
        } else if matches!(self.options.year, Some(DateTimeWidth::Numeric))
            || self.uses_default_date_pattern()
        {
            Some(YearStyle::Full)
        } else {
            None
        }
    }

    fn uses_two_digit_fields(&self) -> bool {
        [
            self.options.month,
            self.options.day,
            self.options.hour,
            self.options.minute,
            self.options.second,
        ]
        .into_iter()
        .any(|width| width == Some(DateTimeWidth::TwoDigit))
    }

    fn effective_time_zone_name(&self) -> Option<&str> {
        self.options
            .time_zone_name
            .as_deref()
            .or(match self.options.time_style {
                Some(DateTimeStyle::Full) => Some("long"),
                Some(DateTimeStyle::Long) => Some("short"),
                _ => None,
            })
    }

    fn shows_part(&self, kind: &str) -> bool {
        let default_date = self.uses_default_date_pattern();
        match kind {
            "weekday" => {
                self.options.date_style == Some(DateTimeStyle::Full)
                    || self.options.weekday.is_some()
            }
            "era" => self.options.era.is_some(),
            "year" => {
                self.options.date_style.is_some() || self.options.year.is_some() || default_date
            }
            "month" => {
                self.options.date_style.is_some() || self.options.month.is_some() || default_date
            }
            "day" => {
                self.options.date_style.is_some() || self.options.day.is_some() || default_date
            }
            "dayPeriod" => {
                self.options.time_style.is_some()
                    || self.options.day_period.is_some()
                    || self.options.hour.is_some()
            }
            "hour" => self.options.time_style.is_some() || self.options.hour.is_some(),
            "minute" => self.options.time_style.is_some() || self.options.minute.is_some(),
            "second" => self.options.time_style.is_some() || self.options.second.is_some(),
            "fractionalSecond" => self.options.fractional_second_digits.is_some(),
            _ => true,
        }
    }

    fn filter_unrequested_parts(&self, parts: Vec<DateTimePart>) -> Vec<DateTimePart> {
        let selected = parts
            .iter()
            .enumerate()
            .filter_map(|(index, part)| {
                (part.kind != "literal" && self.shows_part(&part.kind)).then_some(index)
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return parts;
        }
        let mut result = Vec::with_capacity(selected.len() * 2);
        for (position, index) in selected.iter().copied().enumerate() {
            if position > 0 {
                let literal = parts[selected[position - 1] + 1..index]
                    .iter()
                    .filter(|part| part.kind == "literal")
                    .map(|part| part.value.as_str())
                    .collect::<String>();
                if !literal.is_empty() {
                    result.push(DateTimePart {
                        kind: "literal".into(),
                        value: literal,
                    });
                }
            }
            result.push(parts[index].clone());
        }
        result
    }

    /// ICU4X's semantic short skeleton leaves historic Gregorian years in
    /// their full form under `YearStyle::Auto`. ECMA-402's `dateStyle:
    /// "short"` requests the locale's two-digit year pattern. Keep the
    /// transformation on the typed year part so all decimal systems retain
    /// their own digits rather than parsing and reformatting a localized
    /// number as ASCII.
    fn apply_style_part_widths(&self, parts: &mut [DateTimePart]) {
        if self.options.date_style != Some(DateTimeStyle::Short) {
            return;
        }
        for part in parts {
            if part.kind != "year" || !part.value.chars().all(char::is_numeric) {
                continue;
            }
            let digits = part.value.chars().collect::<Vec<_>>();
            if digits.len() > 2 {
                part.value = digits[digits.len() - 2..].iter().collect();
            }
        }
    }

    /// ICU4X applies the requested numbering system to date-time digits, but
    /// its dynamic fractional-second field currently retains an ASCII decimal
    /// separator. ECMA-402 obtains that separator from a NumberFormat with
    /// the resolved numbering system, so restore the CLDR decimal symbol on
    /// the typed literal at this boundary. Most decimal systems use `.`;
    /// Arabic systems are the relevant non-ASCII exception in the bundled
    /// data.
    fn apply_numbering_system_punctuation(&self, parts: &mut [DateTimePart]) {
        let decimal_separator = match self.numbering_system.as_str() {
            "arab" | "arabext" => "\u{066b}",
            _ => ".",
        };
        for index in 1..parts.len().saturating_sub(1) {
            if parts[index].kind == "literal"
                && parts[index - 1].kind == "second"
                && parts[index + 1].kind == "fractionalSecond"
            {
                parts[index].value = decimal_separator.into();
            }
        }

        // CLDR's hanidec time pattern contains a narrow no-break space before
        // the English AM/PM marker. ECMA-402's en-US pattern uses a regular
        // space; normalize only this data mismatch, without touching spacing
        // rules for other locales or number systems.
        if self.numbering_system == "hanidec" && self.locale.locale().id.language.as_str() == "en" {
            for part in parts {
                if part.kind == "literal" {
                    part.value = part.value.replace('\u{202f}', " ");
                }
            }
        }
    }

    /// Applies the English CLDR flexible-day-period data requested by the
    /// `dayPeriod` component. ICU4X's public dynamic field-set API currently
    /// exposes an AM/PM time field but not the flexible B-period skeleton, so
    /// keeping this at the typed part boundary avoids treating a localized
    /// rendered string as data. Other locales retain ICU's own day-period
    /// result until their CLDR B-period data is exposed by ICU4X.
    fn apply_flexible_day_period(&self, parts: &mut [DateTimePart], hour: u8) {
        let Some(width) = self.options.day_period else {
            return;
        };
        if self.locale.locale().id.language.as_str() != "en" {
            return;
        }
        let period = match hour {
            0..=11 => "in the morning",
            12 => {
                if width == DateTimeWidth::Narrow {
                    "n"
                } else {
                    "noon"
                }
            }
            13..=17 => "in the afternoon",
            18..=20 => "in the evening",
            _ => "at night",
        };
        for index in 0..parts.len() {
            if parts[index].kind == "dayPeriod" {
                parts[index].value = period.into();
                if index > 0 && parts[index - 1].kind == "literal" {
                    parts[index - 1].value = " ".into();
                }
            }
        }
    }

    /// ICU4X's Chinese-calendar year skeleton currently emits related year
    /// and cyclic year name without the CLDR year suffix. ECMA-402 parts must
    /// describe the full localized formatted string, so restore that literal
    /// at the semantic part boundary instead of exposing an incomplete
    /// `relatedYear`/`yearName` pair to JavaScript.
    fn apply_calendar_part_completeness(&self, parts: &mut Vec<DateTimePart>) {
        if self.calendar != "chinese" || self.locale.locale().id.language.as_str() != "zh" {
            return;
        }
        let has_related_year = parts.iter().any(|part| part.kind == "relatedYear");
        let has_year_name = parts.iter().any(|part| part.kind == "yearName");
        if has_related_year && has_year_name && !parts.iter().any(|part| part.kind == "literal") {
            parts.push(DateTimePart {
                kind: "literal".into(),
                value: "年".into(),
            });
        }
    }

    fn filter_unrequested_range_parts(
        &self,
        parts: Vec<DateTimeRangePart>,
    ) -> Vec<DateTimeRangePart> {
        let selected = parts
            .iter()
            .enumerate()
            .filter_map(|(index, part)| {
                (part.kind != "literal" && self.shows_part(&part.kind)).then_some(index)
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return parts;
        }

        let mut result = Vec::with_capacity(selected.len() * 2);
        for (position, index) in selected.iter().copied().enumerate() {
            if position > 0 {
                result.extend(
                    parts[selected[position - 1] + 1..index]
                        .iter()
                        .filter(|part| part.kind == "literal")
                        .cloned(),
                );
            }
            result.push(parts[index].clone());
        }
        result
    }

    fn format_range_with_zone_name(
        &self,
        start: f64,
        end: f64,
    ) -> Result<Vec<DateTimeRangePart>, DateTimeFormatError> {
        let start = self.format_to_parts(start)?;
        let end = self.format_to_parts(end)?;
        Ok(self.format_range_from_parts(start, end))
    }

    fn format_range_from_parts(
        &self,
        start: Vec<DateTimePart>,
        end: Vec<DateTimePart>,
    ) -> Vec<DateTimeRangePart> {
        if start == end {
            return start
                .into_iter()
                .map(|part| DateTimeRangePart {
                    kind: part.kind,
                    value: part.value,
                    source: DateTimeRangePartSource::Shared,
                })
                .collect();
        }

        let range_format = self.range_format();
        if !range_format.collapse
            || has_different_year(&start, &end)
            // ECMA-402 range patterns for a fractional-second difference
            // render both complete time patterns. Collapsing their common
            // minute or second prefix loses both text and `source` ownership.
            || self.options.fractional_second_digits.is_some()
            // The available English time-only interval patterns repeat both
            // endpoints when a displayed time field differs. Prefix-based
            // collapsing would incorrectly mark the repeated prefix shared.
            || (self.date_fields().is_none() && self.time_precision().is_some())
            // The default en-US numeric-date pattern has a default range
            // pattern that repeats complete endpoints. Do not infer sharing
            // merely from equal textual prefixes.
            || self.uses_default_date_pattern()
        {
            return join_range_parts(&start, &end, range_format.separator);
        }

        let prefix = shared_prefix_len(&start, &end);
        let suffix = shared_suffix_len(&start[prefix..], &end[prefix..]);
        if prefix == 0 && suffix == 0 {
            return join_range_parts(&start, &end, range_format.separator);
        }

        let start_end = start.len() - suffix;
        let end_end = end.len() - suffix;
        let mut parts = Vec::with_capacity(start.len() + end.len() + 1);
        parts.extend(start[..prefix].iter().map(shared_range_part));
        parts.extend(start[prefix..start_end].iter().map(start_range_part));
        parts.push(DateTimeRangePart {
            kind: "literal".into(),
            value: range_format.separator.into(),
            source: DateTimeRangePartSource::Shared,
        });
        parts.extend(end[prefix..end_end].iter().map(end_range_part));
        parts.extend(start[start_end..].iter().map(shared_range_part));
        parts
    }

    fn range_format(&self) -> RangeFormat {
        let language = self.locale.as_str().split('-').next().unwrap_or("und");
        match language {
            "ja" => RangeFormat::repeat("～"),
            "zh" if self.locale.as_str().starts_with("zh-TW")
                || self.locale.as_str().starts_with("zh-HK")
                || self.locale.as_str().starts_with("zh-MO") =>
            {
                RangeFormat::repeat("至")
            }
            "zh" => RangeFormat::repeat(" – "),
            "ko" => RangeFormat::collapse("~"),
            "en" => RangeFormat::collapse("\u{2009}–\u{2009}"),
            "de" | "fr" | "es" | "it" | "pt" | "ar" => RangeFormat::collapse("–"),
            _ => RangeFormat::collapse(" – "),
        }
    }

    fn uses_default_date_pattern(&self) -> bool {
        self.options.date_style.is_none()
            && self.options.time_style.is_none()
            && self.options.weekday.is_none()
            && self.options.year.is_none()
            && self.options.month.is_none()
            && self.options.day.is_none()
            && self.options.day_period.is_none()
            && self.options.hour.is_none()
            && self.options.minute.is_none()
            && self.options.second.is_none()
            && self.options.fractional_second_digits.is_none()
    }

    fn length(&self) -> Length {
        if self.uses_default_date_pattern() {
            return Length::Short;
        }
        match self.options.date_style.or(self.options.time_style) {
            Some(DateTimeStyle::Full | DateTimeStyle::Long) => Length::Long,
            Some(DateTimeStyle::Short) => Length::Short,
            Some(DateTimeStyle::Medium) | None => {
                if matches!(
                    self.options.month,
                    Some(DateTimeWidth::Long | DateTimeWidth::Narrow)
                ) {
                    Length::Long
                } else if matches!(
                    self.options.month,
                    Some(DateTimeWidth::Numeric | DateTimeWidth::TwoDigit)
                ) {
                    Length::Short
                } else {
                    Length::Medium
                }
            }
        }
    }

    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.locale.as_str().len()
            + self.format_locale.as_str().len()
            + self.calendar.len()
            + self.numbering_system.len()
            + self.hour_cycle.len()
            + self.time_zone.len()
            + self.options.time_zone_name.as_ref().map_or(0, String::len)
    }
}

fn time_zone_database() -> &'static TimeZoneDatabase {
    static DATABASE: OnceLock<TimeZoneDatabase> = OnceLock::new();
    DATABASE.get_or_init(TimeZoneDatabase::bundled)
}

fn time_zone_display_name(style: &str, zone: &str, seconds: i32, abbreviation: &str) -> String {
    if parse_time_zone_offset(zone).is_some() {
        return match style {
            "short" | "shortGeneric" | "shortOffset" => gmt_offset(seconds, false),
            "long" | "longGeneric" | "longOffset" => gmt_offset(seconds, true),
            _ => gmt_offset(seconds, false),
        };
    }
    match style {
        "shortOffset" => gmt_offset(seconds, false),
        "longOffset" => gmt_offset(seconds, true),
        "short" => abbreviation.into(),
        "long" => {
            if zone == "UTC" {
                "Coordinated Universal Time".into()
            } else {
                // A Zone/Link identifier is not a localized display name.
                // Until ICU4X's zone-name input is wired into this formatter,
                // use a long offset fallback. It also keeps
                // equivalent links (for example Calcutta and Kolkata) from
                // producing different visible text.
                gmt_offset(seconds, true)
            }
        }
        "shortGeneric" => zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
        "longGeneric" => zone.replace('_', " "),
        _ => abbreviation.into(),
    }
}

/// Parses the restricted ISO offset grammar used by `IsTimeZoneOffsetString`.
///
/// Offsets admit only an ASCII sign followed by `HH`, `HHMM`, or `HH:MM`.
/// They are bounded at 23:59, and negative zero is normalized to positive
/// zero for `resolvedOptions().timeZone`.
fn parse_time_zone_offset(identifier: &str) -> Option<(String, i32)> {
    let bytes = identifier.as_bytes();
    let (&sign, rest) = bytes.split_first()?;
    let sign = match sign {
        b'+' => 1_i32,
        b'-' => -1_i32,
        _ => return None,
    };
    let (hour, minute) = match rest {
        [hour_tens, hour_ones] => (ascii_decimal(*hour_tens, *hour_ones)?, 0),
        [hour_tens, hour_ones, minute_tens, minute_ones] => (
            ascii_decimal(*hour_tens, *hour_ones)?,
            ascii_decimal(*minute_tens, *minute_ones)?,
        ),
        [hour_tens, hour_ones, b':', minute_tens, minute_ones] => (
            ascii_decimal(*hour_tens, *hour_ones)?,
            ascii_decimal(*minute_tens, *minute_ones)?,
        ),
        _ => return None,
    };
    if hour > 23 || minute > 59 {
        return None;
    }
    let seconds = sign * (i32::from(hour) * 3_600 + i32::from(minute) * 60);
    let normalized_seconds = if seconds == 0 { 0 } else { seconds };
    let normalized_sign = if normalized_seconds < 0 { '-' } else { '+' };
    let absolute = normalized_seconds.unsigned_abs();
    Some((
        format!(
            "{normalized_sign}{:02}:{:02}",
            absolute / 3_600,
            (absolute % 3_600) / 60
        ),
        normalized_seconds,
    ))
}

fn ascii_decimal(tens: u8, ones: u8) -> Option<u8> {
    tens.is_ascii_digit()
        .then_some((tens - b'0') * 10)
        .zip(ones.is_ascii_digit().then_some(ones - b'0'))
        .map(|(tens, ones)| tens + ones)
}

fn gmt_offset(seconds: i32, long: bool) -> String {
    if seconds == 0 {
        return "GMT".into();
    }
    let sign = if seconds < 0 { '-' } else { '+' };
    let absolute = seconds.unsigned_abs();
    let hours = absolute / 3_600;
    let minutes = (absolute % 3_600) / 60;
    if long || minutes != 0 {
        format!("GMT{sign}{hours:02}:{minutes:02}")
    } else {
        format!("GMT{sign}{hours}")
    }
}

#[derive(Clone, Copy)]
struct RangeFormat {
    separator: &'static str,
    collapse: bool,
}

impl RangeFormat {
    const fn collapse(separator: &'static str) -> Self {
        Self {
            separator,
            collapse: true,
        }
    }

    const fn repeat(separator: &'static str) -> Self {
        Self {
            separator,
            collapse: false,
        }
    }
}

fn has_different_year(start: &[DateTimePart], end: &[DateTimePart]) -> bool {
    let start_year = start
        .iter()
        .find(|part| part.kind == "year")
        .map(|part| part.value.as_str());
    let end_year = end
        .iter()
        .find(|part| part.kind == "year")
        .map(|part| part.value.as_str());
    start_year.is_some() && start_year != end_year
}

fn shared_prefix_len(start: &[DateTimePart], end: &[DateTimePart]) -> usize {
    start
        .iter()
        .zip(end)
        .take_while(|(start, end)| start == end)
        .count()
}

fn shared_suffix_len(start: &[DateTimePart], end: &[DateTimePart]) -> usize {
    start
        .iter()
        .rev()
        .zip(end.iter().rev())
        .take_while(|(start, end)| start == end)
        .count()
}

fn shared_range_part(part: &DateTimePart) -> DateTimeRangePart {
    DateTimeRangePart {
        kind: part.kind.clone(),
        value: part.value.clone(),
        source: DateTimeRangePartSource::Shared,
    }
}

fn start_range_part(part: &DateTimePart) -> DateTimeRangePart {
    DateTimeRangePart {
        kind: part.kind.clone(),
        value: part.value.clone(),
        source: DateTimeRangePartSource::StartRange,
    }
}

fn end_range_part(part: &DateTimePart) -> DateTimeRangePart {
    DateTimeRangePart {
        kind: part.kind.clone(),
        value: part.value.clone(),
        source: DateTimeRangePartSource::EndRange,
    }
}

fn join_range_parts(
    start: &[DateTimePart],
    end: &[DateTimePart],
    separator: &'static str,
) -> Vec<DateTimeRangePart> {
    let mut parts = Vec::with_capacity(start.len() + end.len() + 1);
    parts.extend(start.iter().map(start_range_part));
    parts.push(DateTimeRangePart {
        kind: "literal".into(),
        value: separator.into(),
        source: DateTimeRangePartSource::Shared,
    });
    parts.extend(end.iter().map(end_range_part));
    parts
}

/// Converts ICU4X's interval-side annotations to ECMA-402's serialized-part
/// ownership. ICU marks the side that emitted a segment of its interval
/// pattern, while ECMA-402 marks a field `shared` when one serialized field
/// represents equal start and end values. A field repeated in the output
/// remains endpoint-owned, so a default en-US range keeps both years distinct.
///
/// This runs only on direct CLDR output. Compatibility ranges keep their own
/// deliberately bounded ownership model.
fn normalize_range_part_sources(
    parts: &mut [DateTimeRangePart],
    start: &[DateTimePart],
    end: &[DateTimePart],
) {
    for index in 0..parts.len() {
        let part = &parts[index];
        if part.kind == "literal" {
            continue;
        }
        let emitted_once = parts
            .iter()
            .filter(|candidate| candidate.kind == part.kind && candidate.value == part.value)
            .count()
            == 1;
        let matches_both_endpoints = start
            .iter()
            .any(|candidate| candidate.kind == part.kind && candidate.value == part.value)
            && end
                .iter()
                .any(|candidate| candidate.kind == part.kind && candidate.value == part.value);
        if emitted_once && matches_both_endpoints {
            parts[index].source = DateTimeRangePartSource::Shared;
        }
    }

    for index in 0..parts.len() {
        if parts[index].kind != "literal" {
            continue;
        }
        let previous = parts[..index]
            .iter()
            .rev()
            .find(|part| part.kind != "literal")
            .map(|part| part.source);
        let next = parts[index + 1..]
            .iter()
            .find(|part| part.kind != "literal")
            .map(|part| part.source);
        if previous == Some(DateTimeRangePartSource::Shared)
            || next == Some(DateTimeRangePartSource::Shared)
            || (previous == Some(DateTimeRangePartSource::StartRange)
                && next == Some(DateTimeRangePartSource::EndRange))
        {
            parts[index].source = DateTimeRangePartSource::Shared;
        }
    }
}

/// ICU4X currently emits a fractional second as part of the `second` field.
/// ECMA-402 exposes the decimal marker and fractional digits as independent
/// `formatToParts` fields, so split that ICU field before range ownership is
/// assigned or unrequested fields are filtered.
fn fractional_second_segments(value: &str, digits: Option<u8>) -> Option<(&str, &str, &str)> {
    let digits = usize::from(digits?);
    let positions = value
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if positions.len() <= digits {
        return None;
    }
    let fractional_start = positions[positions.len() - digits];
    let separator_start = positions[positions.len() - digits - 1];
    let second = &value[..separator_start];
    let separator = &value[separator_start..fractional_start];
    let fractional = &value[fractional_start..];
    (!second.is_empty()
        && !separator.is_empty()
        && separator.chars().all(|character| !character.is_numeric())
        && fractional.chars().all(char::is_numeric))
    .then_some((second, separator, fractional))
}

fn split_fractional_seconds(parts: Vec<DateTimePart>, digits: Option<u8>) -> Vec<DateTimePart> {
    let mut result = Vec::with_capacity(parts.len() + usize::from(digits.unwrap_or(0)) * 2);
    for part in parts {
        if part.kind == "second" {
            if let Some((second, separator, fractional)) =
                fractional_second_segments(&part.value, digits)
            {
                result.extend([
                    DateTimePart {
                        kind: "second".into(),
                        value: second.into(),
                    },
                    DateTimePart {
                        kind: "literal".into(),
                        value: separator.into(),
                    },
                    DateTimePart {
                        kind: "fractionalSecond".into(),
                        value: fractional.into(),
                    },
                ]);
                continue;
            }
        }
        result.push(part);
    }
    result
}

fn split_range_fractional_seconds(
    parts: Vec<DateTimeRangePart>,
    digits: Option<u8>,
) -> Vec<DateTimeRangePart> {
    let mut result = Vec::with_capacity(parts.len() + usize::from(digits.unwrap_or(0)) * 2);
    for part in parts {
        if part.kind == "second" {
            if let Some((second, separator, fractional)) =
                fractional_second_segments(&part.value, digits)
            {
                result.extend([
                    DateTimeRangePart {
                        kind: "second".into(),
                        value: second.into(),
                        source: part.source,
                    },
                    DateTimeRangePart {
                        kind: "literal".into(),
                        value: separator.into(),
                        source: part.source,
                    },
                    DateTimeRangePart {
                        kind: "fractionalSecond".into(),
                        value: fractional.into(),
                        source: part.source,
                    },
                ]);
                continue;
            }
        }
        result.push(part);
    }
    result
}

#[derive(Default)]
struct PartWriter {
    string: String,
    parts: Vec<(usize, usize, Part)>,
}

impl fmt::Write for PartWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.string.write_str(value)
    }
}

impl PartsWrite for PartWriter {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut write: impl FnMut(&mut Self::SubPartsWrite) -> fmt::Result,
    ) -> fmt::Result {
        let start = self.string.len();
        write(self)?;
        let end = self.string.len();
        if start < end {
            self.parts.push((start, end, part));
        }
        Ok(())
    }
}

impl PartWriter {
    fn into_parts(mut self) -> Vec<DateTimePart> {
        self.parts.sort_unstable_by_key(|(start, end, part)| {
            (
                *start,
                *end,
                if part.category == "datetime" { 0 } else { 1 },
            )
        });
        let mut result = Vec::new();
        let mut cursor = 0;
        for (start, end, part) in self.parts {
            if part.category != "datetime" || start < cursor {
                continue;
            }
            if cursor < start {
                result.push(DateTimePart {
                    kind: "literal".into(),
                    value: self.string[cursor..start].into(),
                });
            }
            result.push(DateTimePart {
                kind: part.value.into(),
                value: self.string[start..end].into(),
            });
            cursor = end;
        }
        if cursor < self.string.len() {
            result.push(DateTimePart {
                kind: "literal".into(),
                value: self.string[cursor..].into(),
            });
        }
        result
    }

    fn into_range_parts(mut self) -> Vec<DateTimeRangePart> {
        self.parts.sort_unstable_by_key(|(start, end, part)| {
            (
                *start,
                *end,
                if part.category == DATE_RANGE_PART_SOURCE_CATEGORY {
                    0
                } else {
                    1
                },
            )
        });
        let fields = self
            .parts
            .iter()
            .filter(|(_, _, part)| part.category == "datetime")
            .copied()
            .collect::<Vec<_>>();
        let mut sources = self
            .parts
            .iter()
            .filter_map(|(start, end, part)| {
                (part.category == DATE_RANGE_PART_SOURCE_CATEGORY)
                    .then(|| range_part_source(part.value).map(|source| (*start, *end, source)))
                    .flatten()
            })
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|(start, end, _)| (*start, *end));

        let mut result = Vec::new();
        let mut cursor = 0;
        for (start, end, source) in sources {
            if end <= cursor {
                continue;
            }
            if cursor < start {
                push_range_literal(
                    &mut result,
                    &self.string[cursor..start],
                    DateTimeRangePartSource::Shared,
                );
            }
            let mut within_source = start.max(cursor);
            for (field_start, field_end, field) in &fields {
                if *field_start < within_source || *field_end > end {
                    continue;
                }
                push_range_literal(
                    &mut result,
                    &self.string[within_source..*field_start],
                    source,
                );
                result.push(DateTimeRangePart {
                    kind: field.value.into(),
                    value: self.string[*field_start..*field_end].into(),
                    source,
                });
                within_source = *field_end;
            }
            push_range_literal(&mut result, &self.string[within_source..end], source);
            cursor = end;
        }
        push_range_literal(
            &mut result,
            &self.string[cursor..],
            DateTimeRangePartSource::Shared,
        );
        result
    }
}

fn range_part_source(value: &str) -> Option<DateTimeRangePartSource> {
    match value {
        "shared" => Some(DateTimeRangePartSource::Shared),
        "startRange" => Some(DateTimeRangePartSource::StartRange),
        "endRange" => Some(DateTimeRangePartSource::EndRange),
        _ => None,
    }
}

fn push_range_literal(
    parts: &mut Vec<DateTimeRangePart>,
    value: &str,
    source: DateTimeRangePartSource,
) {
    if !value.is_empty() {
        // Nested ICU annotations can split one contiguous pattern literal.
        // Joining equal-source literal bytes preserves both the text and its
        // authoritative owner; unlike the old post-pass, it never changes a
        // source according to neighboring fields or separator contents.
        if let Some(previous) = parts.last_mut() {
            if previous.kind == "literal" && previous.source == source {
                previous.value.push_str(value);
                return;
            }
        }
        parts.push(DateTimeRangePart {
            kind: "literal".into(),
            value: value.into(),
            source,
        });
    }
}
