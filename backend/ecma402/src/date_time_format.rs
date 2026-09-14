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

use crate::{
    locale_data_provider, supports_numbering_system, CanonicalLocale, LocaleMatcher,
    SUPPORTED_CALENDARS,
};
use icu_datetime::{
    fieldsets::{
        builder::{DateFields, FieldSetBuilder, ZoneStyle},
        enums::{CompositeDateTimeFieldSet, CompositeFieldSet, DateFieldSet},
        YMD,
    },
    input::{DateTime, TimeZone, UtcOffset, ZonedDateTime},
    options::{Alignment, Length, SubsecondDigits, TimePrecision, YearStyle},
    range::{DateRangeFormatter, DATE_RANGE_PART_SOURCE_CATEGORY},
    DateTimeFormatter,
};
use icu_time::{
    zone::{models::AtTime, ZoneNameTimestamp},
    TimeZoneInfo,
};
use jiff::{tz::TimeZoneDatabase, Timestamp};
use std::fmt;
use std::sync::OnceLock;
use writeable::{Part, PartsWrite, Writeable};

const TIME_CLIP_LIMIT: f64 = 8_640_000_000_000_000.0;
const BASIC_REMOVAL_PENALTY: i32 = 120;
const BASIC_ADDITION_PENALTY: i32 = 20;
const BASIC_LONG_LESS_PENALTY: i32 = 8;
const BASIC_LONG_MORE_PENALTY: i32 = 6;
const BASIC_SHORT_LESS_PENALTY: i32 = 6;
const BASIC_SHORT_MORE_PENALTY: i32 = 3;
const BASIC_OFFSET_PENALTY: i32 = 1;

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

/// The component-pattern selection policy requested by formatMatcher.
///
/// best fit is implementation-defined by ECMA-402 and delegates to ICU4X's
/// CLDR semantic skeleton matcher. basic uses the deterministic scoring
/// algorithm in ECMA-402's BasicFormatMatcher.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DateTimeFormatMatcher {
    Basic,
    #[default]
    BestFit,
}

/// The host-neutral options accepted by the DateTimeFormat service.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DateTimeFormatOptions {
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

/// A locale date-time format record considered by basic_format_matcher.
///
/// It models the component fields in ECMA-402's DateTime Format Records,
/// independently of an ICU pattern string. Pattern rendering remains owned by
/// ICU4X after the record has been selected.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DateTimeFormatRecord {
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
}

impl DateTimeFormatRecord {
    fn from_options(options: &DateTimeFormatOptions) -> Self {
        Self {
            weekday: options.weekday,
            era: options.era,
            year: options.year,
            month: options.month,
            day: options.day,
            day_period: options.day_period,
            hour: options.hour,
            minute: options.minute,
            second: options.second,
            fractional_second_digits: options.fractional_second_digits,
            time_zone_name: options.time_zone_name.clone(),
        }
    }

    fn apply_to(&self, options: &mut DateTimeFormatOptions) {
        options.weekday = self.weekday;
        options.era = self.era;
        options.year = self.year;
        options.month = self.month;
        options.day = self.day;
        options.day_period = self.day_period;
        options.hour = self.hour;
        options.minute = self.minute;
        options.second = self.second;
        options.fractional_second_digits = self.fractional_second_digits;
        options.time_zone_name = self.time_zone_name.clone();
    }
}

/// Selects a record using ECMA-402 §11.5.2 BasicFormatMatcher.
///
/// The first record wins equal scores, matching the List iteration and strict
/// greater-than comparison in the specification. Callers must supply the
/// locale's available format records in their preference order.
pub fn basic_format_matcher(
    options: &DateTimeFormatOptions,
    formats: &[DateTimeFormatRecord],
) -> Option<usize> {
    let requested = DateTimeFormatRecord::from_options(options);
    let mut best_score = i32::MIN;
    let mut best = None;
    for (index, format) in formats.iter().enumerate() {
        let mut score = 0;
        for (requested, available) in [
            (requested.weekday, format.weekday),
            (requested.era, format.era),
            (requested.year, format.year),
            (requested.month, format.month),
            (requested.day, format.day),
            (requested.day_period, format.day_period),
            (requested.hour, format.hour),
            (requested.minute, format.minute),
            (requested.second, format.second),
        ] {
            score += basic_field_score(
                requested.map(BasicValue::Width),
                available.map(BasicValue::Width),
                BasicField::Width,
            );
        }
        score += basic_field_score(
            requested
                .fractional_second_digits
                .map(BasicValue::FractionalSecondDigits),
            format
                .fractional_second_digits
                .map(BasicValue::FractionalSecondDigits),
            BasicField::FractionalSecondDigits,
        );
        score += basic_field_score(
            requested
                .time_zone_name
                .as_deref()
                .map(BasicValue::TimeZoneName),
            format
                .time_zone_name
                .as_deref()
                .map(BasicValue::TimeZoneName),
            BasicField::TimeZoneName,
        );
        if score > best_score {
            best_score = score;
            best = Some(index);
        }
    }
    best
}

#[derive(Clone, Copy)]
enum BasicField {
    Width,
    FractionalSecondDigits,
    TimeZoneName,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BasicValue<'a> {
    Width(DateTimeWidth),
    FractionalSecondDigits(u8),
    TimeZoneName(&'a str),
}

fn basic_field_score(
    requested: Option<BasicValue<'_>>,
    available: Option<BasicValue<'_>>,
    field: BasicField,
) -> i32 {
    let (Some(requested), Some(available)) = (requested, available) else {
        return match (requested, available) {
            (None, Some(_)) => -BASIC_ADDITION_PENALTY,
            (Some(_), None) => -BASIC_REMOVAL_PENALTY,
            (None, None) => 0,
            (Some(_), Some(_)) => unreachable!(),
        };
    };
    if requested == available {
        return 0;
    }
    if matches!(field, BasicField::TimeZoneName) {
        let (BasicValue::TimeZoneName(requested), BasicValue::TimeZoneName(available)) =
            (requested, available)
        else {
            return -BASIC_REMOVAL_PENALTY;
        };
        return basic_time_zone_name_score(requested, available);
    }
    let (requested, available) = match (requested, available) {
        (
            BasicValue::FractionalSecondDigits(requested),
            BasicValue::FractionalSecondDigits(available),
        ) => (i32::from(requested), i32::from(available)),
        (BasicValue::Width(requested), BasicValue::Width(available)) => {
            (basic_width_index(requested), basic_width_index(available))
        }
        _ => return -BASIC_REMOVAL_PENALTY,
    };
    match (available - requested).clamp(-2, 2) {
        2 => -BASIC_LONG_MORE_PENALTY,
        1 => -BASIC_SHORT_MORE_PENALTY,
        -1 => -BASIC_SHORT_LESS_PENALTY,
        -2 => -BASIC_LONG_LESS_PENALTY,
        0 => 0,
        _ => unreachable!("the delta is clamped"),
    }
}

fn basic_width_index(width: DateTimeWidth) -> i32 {
    match width {
        DateTimeWidth::TwoDigit => 0,
        DateTimeWidth::Numeric => 1,
        DateTimeWidth::Narrow => 2,
        DateTimeWidth::Short => 3,
        DateTimeWidth::Long => 4,
    }
}

fn basic_time_zone_name_score(requested: &str, available: &str) -> i32 {
    match requested {
        "short" | "shortGeneric" => match available {
            "shortOffset" => -BASIC_OFFSET_PENALTY,
            "longOffset" => -(BASIC_OFFSET_PENALTY + BASIC_SHORT_MORE_PENALTY),
            "long" if requested == "short" => -BASIC_SHORT_MORE_PENALTY,
            "longGeneric" if requested == "shortGeneric" => -BASIC_SHORT_MORE_PENALTY,
            _ if requested == available => 0,
            _ => -BASIC_REMOVAL_PENALTY,
        },
        "shortOffset" if available == "longOffset" => -BASIC_SHORT_MORE_PENALTY,
        "long" | "longGeneric" => match available {
            "longOffset" => -BASIC_OFFSET_PENALTY,
            "shortOffset" => -(BASIC_OFFSET_PENALTY + BASIC_LONG_LESS_PENALTY),
            "short" if requested == "long" => -BASIC_LONG_LESS_PENALTY,
            "shortGeneric" if requested == "longGeneric" => -BASIC_LONG_LESS_PENALTY,
            _ if requested == available => 0,
            _ => -BASIC_REMOVAL_PENALTY,
        },
        "longOffset" if available == "shortOffset" => -BASIC_LONG_LESS_PENALTY,
        _ if requested == available => 0,
        _ => -BASIC_REMOVAL_PENALTY,
    }
}

fn resolve_basic_semantic_format(options: &mut DateTimeFormatOptions) {
    if options.format_matcher != DateTimeFormatMatcher::Basic
        || options.date_style.is_some()
        || options.time_style.is_some()
    {
        return;
    }
    // ICU4X's public dynamic formatter accepts the exact semantic field set
    // requested by this service. Treat that generated pattern as an available
    // format record, then run the standard scorer before construction. This
    // keeps BasicFormatMatcher observable in the service path without
    // inventing a locale-independent CLDR fallback record. A future ICU4X
    // available-format enumeration can provide additional ordered records to
    // this call without changing the scorer.
    let formats = [DateTimeFormatRecord::from_options(options)];
    let selected = basic_format_matcher(options, &formats)
        .expect("the dynamic semantic formatter always supplies one format");
    formats[selected].apply_to(options);
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
    locale_data_provider().tzdb_version()
}

/// A failure from DateTimeFormat construction or formatting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DateTimeFormatError {
    InvalidTime,
    UnsupportedTimeZone,
    Formatter,
    IncompatibleRangeInputs,
}

impl fmt::Display for DateTimeFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTime => formatter.write_str("invalid time value"),
            Self::UnsupportedTimeZone => formatter.write_str("unsupported time zone"),
            Self::Formatter => formatter.write_str("could not initialize date-time formatter"),
            Self::IncompatibleRangeInputs => {
                formatter.write_str("incompatible date-time range inputs")
            }
        }
    }
}

impl std::error::Error for DateTimeFormatError {}

/// A typed value supplied to a DateTimeFormat formatting operation.
///
/// ECMAScript values and Temporal object identity remain the embedding VM's
/// responsibility. This preserves the two semantic inputs relevant to the
/// host-neutral formatter: instants are subject to TimeClip and use ordinary
/// time-zone conversion, while plain values carry local ISO fields without
/// TimeClip or a time-zone name. Temporal-specific options are carried with
/// the value so single-value and range formatting cannot diverge.
#[derive(Clone, Debug, PartialEq)]
pub enum DateTimeFormatInput {
    EpochMilliseconds(f64),
    TemporalInstant {
        epoch_milliseconds: f64,
        options: DateTimeFormatOptions,
    },
    TemporalPlain {
        local_epoch_milliseconds: i64,
        options: DateTimeFormatOptions,
    },
}

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
    Ok(supports_numbering_system(&value).then_some(value))
}

fn default_calendar(locale: &CanonicalLocale) -> &'static str {
    locale_data_provider().default_calendar(locale.locale())
}

fn default_numbering_system(locale: &CanonicalLocale) -> &'static str {
    locale_data_provider().default_numbering_system(locale.locale())
}

fn default_hour_cycle(locale: &CanonicalLocale) -> &'static str {
    locale_data_provider().default_hour_cycle(locale.locale())
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

fn resolve_date_time_locale(
    selected: &CanonicalLocale,
    options: &DateTimeFormatOptions,
) -> Result<ResolvedDateTimeLocale, DateTimeFormatError> {
    if let Some(calendar) = options.calendar.as_deref() {
        canonical_calendar(calendar)?;
    }
    if let Some(numbering_system) = options.numbering_system.as_deref() {
        canonical_numbering_system(numbering_system)?;
    }
    let calendar_key = crate::resolve_locale_key(
        selected,
        "ca",
        options.calendar.as_deref(),
        Some(default_calendar(selected)),
        |value| canonical_calendar(value).ok().flatten().map(str::to_owned),
    );
    let calendar = calendar_key
        .value()
        .expect("DateTimeFormat always has a provider calendar");
    let numbering_key = crate::resolve_locale_key(
        selected,
        "nu",
        options.numbering_system.as_deref(),
        Some(default_numbering_system(selected)),
        |value| canonical_numbering_system(value).ok().flatten(),
    );
    let numbering_system = numbering_key
        .value()
        .expect("DateTimeFormat always has a provider numbering system");
    let hour_cycle_key = crate::resolve_locale_key(
        selected,
        "hc",
        options.hour_cycle.as_deref(),
        Some(default_hour_cycle(selected)),
        |value| canonical_hour_cycle(value).map(str::to_owned),
    );
    let hour_cycle = match options.hour12 {
        Some(true) if selected.locale().id.language.as_str() == "ja" => "h11",
        Some(true) => "h12",
        Some(false) => "h23",
        None => hour_cycle_key
            .value()
            .expect("DateTimeFormat always has a provider hour cycle"),
    };

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
    let formatting_calendar = crate::LocaleKeyResolution::fixed(formatting_calendar);
    let formatting_numbering = crate::LocaleKeyResolution::fixed(numbering_system);
    let formatting_hour_cycle = crate::LocaleKeyResolution::fixed(formatting_hour_cycle);
    // `hour12` is not a Unicode-key option. It overrides the hour-cycle
    // algorithm preference and therefore suppresses a requested `hc` key
    // from the observable resolved locale.
    let visible_hour_cycle = options.hour12.map(|_| {
        crate::LocaleKeyResolution::fixed(
            hour_cycle_key
                .value()
                .expect("DateTimeFormat always has a provider hour cycle"),
        )
    });
    let format_locale = crate::locale_with_resolved_keys(
        selected,
        &[
            ("ca", &formatting_calendar),
            ("nu", &formatting_numbering),
            ("hc", &formatting_hour_cycle),
        ],
    );
    let locale = crate::locale_with_resolved_keys(
        selected,
        &[
            ("ca", &calendar_key),
            ("nu", &numbering_key),
            ("hc", visible_hour_cycle.as_ref().unwrap_or(&hour_cycle_key)),
        ],
    );
    Ok(ResolvedDateTimeLocale {
        locale,
        format_locale,
        calendar: calendar.into(),
        numbering_system: numbering_system.into(),
        hour_cycle: hour_cycle.into(),
    })
}

impl DateTimeFormat {
    /// Builds a DateTimeFormat from already-canonical locale requests.
    pub fn try_new(
        requested: &[CanonicalLocale],
        mut options: DateTimeFormatOptions,
    ) -> Result<Self, DateTimeFormatError> {
        resolve_basic_semantic_format(&mut options);
        let selected_locale = crate::resolve_locale(
            crate::IntlService::DateTimeFormat,
            requested,
            options.locale_matcher,
        )
        .selected()
        .clone();
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
        let start_milliseconds = time_clip_milliseconds(start)?;
        let end_milliseconds = time_clip_milliseconds(end)?;
        let (start_datetime, start_offset) = self.datetime_from_milliseconds(start_milliseconds)?;
        let (end_datetime, end_offset) = self.datetime_from_milliseconds(end_milliseconds)?;
        let start_parts = self.format_range_endpoint_to_parts(&start_datetime, start_offset)?;
        let end_parts = self.format_range_endpoint_to_parts(&end_datetime, end_offset)?;
        // ECMA-402 returns one normal pattern when the requested fields have
        // the same displayed values. This is an equality decision for the
        // entire formatted result, before range-pattern serialization; it
        // does not rewrite ownership of individual CLDR range spans.
        if start_parts == end_parts {
            return Ok(start_parts.iter().map(shared_range_part).collect());
        }
        self.format_datetime_range_to_parts(
            &start_datetime,
            start_offset,
            &end_datetime,
            end_offset,
        )
    }

    /// Formats a typed ECMAScript or Temporal input.
    ///
    /// The typed bridge is deliberately shared by `format`, `formatToParts`,
    /// and their range counterparts. In particular, a Temporal.Instant uses
    /// the same resolved default components in a single value and interval.
    pub fn format_input_to_parts(
        &self,
        input: DateTimeFormatInput,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        match input {
            DateTimeFormatInput::EpochMilliseconds(epoch_milliseconds) => {
                self.format_to_parts(epoch_milliseconds)
            }
            DateTimeFormatInput::TemporalInstant {
                epoch_milliseconds,
                options,
            } => {
                let formatter = self.with_input_options(options)?;
                formatter.format_to_parts(epoch_milliseconds)
            }
            DateTimeFormatInput::TemporalPlain {
                local_epoch_milliseconds,
                options,
            } => {
                let formatter = self.with_input_options(options)?;
                formatter.format_to_parts_from_milliseconds(local_epoch_milliseconds, false)
            }
        }
    }

    /// Formats a pair of typed ECMAScript or Temporal inputs with one
    /// consistent semantic formatter.
    pub fn format_range_inputs_to_parts(
        &self,
        start: DateTimeFormatInput,
        end: DateTimeFormatInput,
    ) -> Result<Vec<DateTimeRangePart>, DateTimeFormatError> {
        match (start, end) {
            (
                DateTimeFormatInput::EpochMilliseconds(start),
                DateTimeFormatInput::EpochMilliseconds(end),
            ) => self.format_range_to_parts(start, end),
            (
                DateTimeFormatInput::TemporalInstant {
                    epoch_milliseconds: start,
                    options,
                },
                DateTimeFormatInput::TemporalInstant {
                    epoch_milliseconds: end,
                    options: end_options,
                },
            ) => {
                if options != end_options {
                    return Err(DateTimeFormatError::IncompatibleRangeInputs);
                }
                let formatter = self.with_input_options(options)?;
                formatter.format_range_to_parts(start, end)
            }
            (
                DateTimeFormatInput::TemporalPlain {
                    local_epoch_milliseconds: start,
                    options,
                },
                DateTimeFormatInput::TemporalPlain {
                    local_epoch_milliseconds: end,
                    options: end_options,
                },
            ) => {
                if options != end_options {
                    return Err(DateTimeFormatError::IncompatibleRangeInputs);
                }
                let formatter = self.with_input_options(options)?;
                formatter.format_plain_range_to_parts(start, end)
            }
            _ => Err(DateTimeFormatError::IncompatibleRangeInputs),
        }
    }

    fn format_datetime_range_to_parts(
        &self,
        start: &DateTime<icu_calendar::Iso>,
        start_offset: i32,
        end: &DateTime<icu_calendar::Iso>,
        end_offset: i32,
    ) -> Result<Vec<DateTimeRangePart>, DateTimeFormatError> {
        let formatter = DateRangeFormatter::try_new(
            self.format_locale.locale().clone().into(),
            self.range_field_set(),
        )
        .map_err(|_| DateTimeFormatError::Formatter)?;
        let start_input = self.zoned_datetime(start, start_offset)?;
        let end_input = self.zoned_datetime(end, end_offset)?;
        let formatted = formatter.format(&start_input, &end_input);
        let mut writer = PartWriter::default();
        formatted
            .write_to_parts_with_source(&mut writer)
            .map_err(|_| DateTimeFormatError::Formatter)?;
        let mut parts = self.filter_unrequested_range_parts(split_range_fractional_seconds(
            writer.into_range_parts(),
            self.options.fractional_second_digits,
        ));
        let start = self.format_range_endpoint_to_parts(start, start_offset)?;
        let end = self.format_range_endpoint_to_parts(end, end_offset)?;
        self.repair_missing_range_seconds(&mut parts, &start, &end);
        self.apply_range_part_formatting(&mut parts);
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

    fn with_input_options(
        &self,
        options: DateTimeFormatOptions,
    ) -> Result<Self, DateTimeFormatError> {
        Self::try_new(std::slice::from_ref(&self.locale), options)
    }

    /// Formats local ISO fields carried by a Temporal plain value. This
    /// intentionally bypasses TimeClip, time-zone conversion, and zone-name
    /// output; the typed input builder has already pruned non-overlapping
    /// components.
    fn format_plain_range_to_parts(
        &self,
        start_milliseconds: i64,
        end_milliseconds: i64,
    ) -> Result<Vec<DateTimeRangePart>, DateTimeFormatError> {
        let (start_datetime, start_offset) = self.datetime_from_milliseconds(start_milliseconds)?;
        let (end_datetime, end_offset) = self.datetime_from_milliseconds(end_milliseconds)?;
        let start = self.format_range_endpoint_to_parts(&start_datetime, start_offset)?;
        let end = self.format_range_endpoint_to_parts(&end_datetime, end_offset)?;
        if start == end {
            return Ok(start.iter().map(shared_range_part).collect());
        }
        self.format_datetime_range_to_parts(
            &start_datetime,
            start_offset,
            &end_datetime,
            end_offset,
        )
    }

    /// Converts an epoch value into ICU4X's calendar input. ICU4X's input
    /// supports the entire `i64` millisecond domain, while Jiff's `Timestamp`
    /// intentionally stops at ISO year +/-9999. ECMA-402 must nevertheless
    /// accept TimeClip's +/-275,760-year endpoints for UTC and fixed zones.
    fn datetime_from_milliseconds(
        &self,
        milliseconds: i64,
    ) -> Result<(DateTime<icu_calendar::Iso>, i32), DateTimeFormatError> {
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
            ));
        }
        let time_zone = time_zone_database()
            .get(&self.time_zone)
            .map_err(|_| DateTimeFormatError::UnsupportedTimeZone)?;
        let offset_seconds = match Timestamp::from_millisecond(milliseconds) {
            Ok(timestamp) => {
                let info = time_zone.to_offset_info(timestamp);
                info.offset().seconds()
            }
            // Jiff's fixed-zone result remains correct outside its civil
            // Timestamp range. For named zones, map the instant into the
            // equivalent position of the Gregorian 400-year cycle. That keeps
            // the ISO date, time, and weekday while allowing Jiff's recurring
            // IANA rule to provide an offset instead of rejecting a valid
            // TimeClip endpoint merely because it is outside Jiff's civil
            // representation.
            Err(_) => match time_zone.to_fixed_offset() {
                Ok(offset) => offset.seconds(),
                Err(_) => {
                    const GREGORIAN_400_YEAR_MILLISECONDS: i64 = 146_097 * 86_400_000;
                    let timestamp = Timestamp::from_millisecond(
                        milliseconds.rem_euclid(GREGORIAN_400_YEAR_MILLISECONDS),
                    )
                    .expect("a 400-year Gregorian cycle fits Jiff's Timestamp range");
                    let info = time_zone.to_offset_info(timestamp);
                    info.offset().seconds()
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
        ))
    }

    fn format_to_parts_from_milliseconds(
        &self,
        milliseconds: i64,
        include_time_zone_name: bool,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let (datetime, offset_seconds) = self.datetime_from_milliseconds(milliseconds)?;
        if include_time_zone_name && self.effective_time_zone_name().is_some() {
            return self.format_range_endpoint_to_parts(&datetime, offset_seconds);
        }
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
        Ok(parts)
    }

    /// Formats one range endpoint using precisely the same dynamic CLDR
    /// skeleton as the range formatter. This keeps the whole-pattern
    /// equality check and source normalization aligned with zone-bearing
    /// ranges, including local CLDR zone names.
    fn format_range_endpoint_to_parts(
        &self,
        datetime: &DateTime<icu_calendar::Iso>,
        offset_seconds: i32,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let formatter = DateTimeFormatter::try_new(
            self.format_locale.locale().clone().into(),
            self.range_field_set(),
        )
        .map_err(|_| DateTimeFormatError::Formatter)?;
        let datetime = self.zoned_datetime(datetime, offset_seconds)?;
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
        Ok(parts)
    }

    fn zoned_datetime(
        &self,
        datetime: &DateTime<icu_calendar::Iso>,
        offset_seconds: i32,
    ) -> Result<ZonedDateTime<icu_calendar::Iso, TimeZoneInfo<AtTime>>, DateTimeFormatError> {
        let time_zone = if self.range_zone_style().is_some() {
            let zone = if self.fixed_offset_seconds.is_some() {
                TimeZone::UNKNOWN
            } else {
                TimeZone::from_iana_id(&self.time_zone)
            };
            zone.with_offset(Some(
                UtcOffset::try_from_seconds(offset_seconds)
                    .map_err(|_| DateTimeFormatError::Formatter)?,
            ))
            .at_date_time(*datetime)
        } else {
            // The zone participates in ICU's range-difference calculation
            // only when its name is part of the requested skeleton. Otherwise
            // a DST transition must not force a complete endpoint fallback.
            TimeZone::UNKNOWN
                .without_offset()
                .with_zone_name_timestamp(ZoneNameTimestamp::from_epoch_seconds(0))
        };
        Ok(ZonedDateTime {
            date: datetime.date,
            time: datetime.time,
            zone: time_zone,
        })
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

    fn range_field_set(&self) -> CompositeFieldSet {
        let mut builder = FieldSetBuilder::new();
        builder.length = Some(self.length());
        builder.date_fields = self.date_fields();
        builder.time_precision = self.time_precision();
        builder.zone_style = self.range_zone_style();
        builder.alignment = self.uses_two_digit_fields().then_some(Alignment::Column);
        builder.year_style = self.year_style();
        // Dynamic range field sets must retain the zone marker when one was
        // requested, rather than post-appending a host-generated name.
        builder
            .build_composite()
            .unwrap_or(CompositeFieldSet::Date(DateFieldSet::YMD(YMD::medium())))
    }

    fn range_zone_style(&self) -> Option<ZoneStyle> {
        match self.effective_time_zone_name()? {
            "short" => Some(ZoneStyle::SpecificShort),
            "long" => Some(ZoneStyle::SpecificLong),
            "shortOffset" => Some(ZoneStyle::LocalizedOffsetShort),
            "longOffset" => Some(ZoneStyle::LocalizedOffsetLong),
            "shortGeneric" => Some(ZoneStyle::GenericShort),
            "longGeneric" => Some(ZoneStyle::GenericLong),
            _ => None,
        }
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
        if matches!(self.calendar.as_str(), "chinese" | "dangi") {
            for part in parts.iter_mut() {
                if part.kind == "year" {
                    part.kind = "relatedYear".into();
                }
            }
        }

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

    /// Applies the post-processing required by the ECMA-402 part surface to
    /// CLDR's range-owned fields without discarding their endpoint sources.
    fn apply_range_part_formatting(&self, parts: &mut Vec<DateTimeRangePart>) {
        self.repair_time_only_range_delimiter(parts);

        if matches!(self.calendar.as_str(), "chinese" | "dangi") {
            for part in parts.iter_mut() {
                if part.kind == "year" {
                    part.kind = "relatedYear".into();
                }
            }
        }

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

        if self.options.date_style == Some(DateTimeStyle::Short) {
            for part in parts.iter_mut() {
                if part.kind != "year" || !part.value.chars().all(char::is_numeric) {
                    continue;
                }
                let digits = part.value.chars().collect::<Vec<_>>();
                if digits.len() > 2 {
                    part.value = digits[digits.len() - 2..].iter().collect();
                }
            }
        }

        if self.numbering_system == "hanidec" && self.locale.locale().id.language.as_str() == "en" {
            for part in parts.iter_mut() {
                if part.kind == "literal" {
                    part.value = part.value.replace('\u{202f}', " ");
                }
            }
        }
    }

    /// ICU4X's generic fallback for a time-only interval can
    /// retain the narrow day-period separator after the unrequested day period
    /// has been removed. The interval and endpoint fields are still CLDR's;
    /// discard only that orphaned punctuation and retain the CLDR range glue.
    fn repair_time_only_range_delimiter(&self, parts: &mut Vec<DateTimeRangePart>) {
        for index in 1..parts.len().saturating_sub(1) {
            if parts[index].kind != "literal"
                || !parts[index].value.contains('–')
                || parts[index - 1].kind != "literal"
                || parts[index - 1].value != "\u{202f}"
                || parts[index + 1].kind != "literal"
                || parts[index + 1].value != ":"
            {
                continue;
            }
            let value = parts[index].value.clone();
            parts.splice(
                index - 1..index + 2,
                [DateTimeRangePart {
                    kind: "literal".into(),
                    value,
                    source: DateTimeRangePartSource::Shared,
                }],
            );
            break;
        }
    }

    /// ICU4X's dynamic interval fallback can omit an explicitly requested
    /// seconds field from both endpoints of a time range, even though its
    /// matching endpoint formatter includes it. Keep the CLDR interval and
    /// its ownership data, and restore only the missing typed field and its
    /// endpoint-local separator.
    fn repair_missing_range_seconds(
        &self,
        parts: &mut Vec<DateTimeRangePart>,
        start: &[DateTimePart],
        end: &[DateTimePart],
    ) {
        if self.options.second.is_none() || parts.iter().any(|part| part.kind == "second") {
            return;
        }

        for (source, endpoint) in [
            (DateTimeRangePartSource::StartRange, start),
            (DateTimeRangePartSource::EndRange, end),
        ] {
            let Some(second_index) = endpoint.iter().position(|part| part.kind == "second") else {
                continue;
            };
            let Some(separator) = second_index
                .checked_sub(1)
                .and_then(|index| endpoint.get(index))
                .filter(|part| part.kind == "literal")
            else {
                continue;
            };
            let Some(minute_index) = parts
                .iter()
                .position(|part| part.source == source && part.kind == "minute")
            else {
                continue;
            };

            // When a day period or zone follows the minute, its leading
            // literal belongs after the restored second. Otherwise the range
            // glue (or the end of the vector) directly follows the minute.
            let insertion = parts[minute_index + 1..]
                .iter()
                .position(|part| {
                    part.source == source
                        && matches!(part.kind.as_str(), "dayPeriod" | "timeZoneName")
                })
                .map(|index| {
                    let field_index = minute_index + 1 + index;
                    (minute_index + 1..field_index)
                        .rev()
                        .find(|&index| {
                            parts[index].source == source && parts[index].kind == "literal"
                        })
                        .unwrap_or(field_index)
                })
                .unwrap_or(minute_index + 1);
            parts.splice(
                insertion..insertion,
                [
                    DateTimeRangePart {
                        kind: "literal".into(),
                        value: separator.value.clone(),
                        source,
                    },
                    DateTimeRangePart {
                        kind: "second".into(),
                        value: endpoint[second_index].value.clone(),
                        source,
                    },
                ],
            );
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

fn shared_range_part(part: &DateTimePart) -> DateTimeRangePart {
    DateTimeRangePart {
        kind: part.kind.clone(),
        value: part.value.clone(),
        source: DateTimeRangePartSource::Shared,
    }
}

/// Converts ICU4X's interval-side annotations to ECMA-402's serialized-part
/// ownership. ICU marks the side that emitted a segment of its interval
/// pattern, while ECMA-402 marks a field `shared` when one serialized field
/// represents equal start and end values. A field repeated in the output
/// remains endpoint-owned, so a default en-US range keeps both years distinct.
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
