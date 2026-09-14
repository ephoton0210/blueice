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

use crate::{CanonicalLocale, LocaleMatcher};
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
}

impl DateTimeFormat {
    /// Builds a DateTimeFormat from already-canonical locale requests.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: DateTimeFormatOptions,
    ) -> Result<Self, DateTimeFormatError> {
        let selected_locale = crate::resolve_collation_locale(requested, options.locale_matcher);
        let requested_time_zone = options.time_zone.clone().unwrap_or_else(|| "UTC".into());
        let time_zone = time_zone_database()
            .get(&requested_time_zone)
            .map_err(|_| DateTimeFormatError::UnsupportedTimeZone)?
            .iana_name()
            .unwrap_or(&requested_time_zone)
            .into();
        let calendar = options
            .calendar
            .clone()
            .or_else(|| crate::unicode_keyword(selected_locale.locale(), "ca"))
            .unwrap_or_else(|| "gregory".into());
        let numbering_system = options
            .numbering_system
            .clone()
            .or_else(|| crate::unicode_keyword(selected_locale.locale(), "nu"))
            .unwrap_or_else(|| "latn".into());
        let hour_cycle = if let Some(hour12) = options.hour12 {
            if hour12 { "h12" } else { "h23" }.into()
        } else {
            options
                .hour_cycle
                .clone()
                .or_else(|| crate::unicode_keyword(selected_locale.locale(), "hc"))
                .unwrap_or_else(|| {
                    // A bare `en` resolves through the CLDR likely-subtag
                    // default to an English locale with a 12-hour cycle.
                    if selected_locale.as_str() == "en"
                        || selected_locale.as_str().starts_with("en-US")
                    {
                        "h12"
                    } else {
                        "h23"
                    }
                    .into()
                })
        };
        let locale = crate::apply_locale_options(
            &selected_locale,
            &crate::LocaleOptions {
                calendar: Some(calendar.clone()),
                hour_cycle: Some(hour_cycle.clone()),
                numbering_system: Some(numbering_system.clone()),
                ..Default::default()
            },
        )
        .map_err(|_| DateTimeFormatError::Formatter)?;
        Ok(Self {
            locale: selected_locale,
            format_locale: locale,
            options,
            calendar,
            numbering_system,
            hour_cycle,
            time_zone,
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
        // The nested range-source annotations are the authoritative CLDR
        // interval-pattern ownership. In particular, do not infer `shared`
        // from equal values, a separator character, or neighboring literals:
        // those are serialization details which need not match pattern spans.
        Ok(
            self.filter_unrequested_range_parts(split_range_fractional_seconds(
                writer.into_range_parts(),
                self.options.fractional_second_digits,
            )),
        )
    }

    /// Formats a time value and retains every ICU date-time field boundary.
    pub fn format_to_parts(
        &self,
        epoch_milliseconds: f64,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let milliseconds = time_clip_milliseconds(epoch_milliseconds)?;
        self.format_to_parts_from_milliseconds(milliseconds, true)
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
        if self.options.second.is_some() || self.options.time_style.is_some() {
            Some(TimePrecision::Second)
        } else if self.options.minute.is_some() {
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
    match style {
        "shortOffset" => gmt_offset(seconds, false),
        "longOffset" => gmt_offset(seconds, true),
        "short" => abbreviation.into(),
        "long" => {
            if zone == "UTC" {
                "Coordinated Universal Time".into()
            } else {
                zone.replace('_', " ")
            }
        }
        "shortGeneric" => zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
        "longGeneric" => zone.replace('_', " "),
        _ => abbreviation.into(),
    }
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
