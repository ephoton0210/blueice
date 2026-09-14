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
        enums::{CompositeDateTimeFieldSet, DateAndTimeFieldSet, DateFieldSet, TimeFieldSet},
        T, YMD,
    },
    input::{DateTime, ZonedDateTime},
    options::{Length, SubsecondDigits},
    DateTimeFormatter,
};
use jiff::{tz::TimeZoneDatabase, Timestamp};
use std::fmt;
use std::sync::OnceLock;
use writeable::{Part, PartsWrite, Writeable};

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
#[derive(Clone, Debug, Default)]
pub struct DateTimeFormatOptions {
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
                    if selected_locale.as_str().starts_with("en-US") {
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

    /// Formats a time value and retains every ICU date-time field boundary.
    pub fn format_to_parts(
        &self,
        epoch_milliseconds: f64,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        if !epoch_milliseconds.is_finite()
            || !(-8_640_000_000_000_000.0..=8_640_000_000_000_000.0).contains(&epoch_milliseconds)
        {
            return Err(DateTimeFormatError::InvalidTime);
        }
        let milliseconds = epoch_milliseconds.trunc() as i64;
        let timestamp = Timestamp::from_millisecond(milliseconds)
            .map_err(|_| DateTimeFormatError::InvalidTime)?;
        let time_zone = time_zone_database()
            .get(&self.time_zone)
            .map_err(|_| DateTimeFormatError::UnsupportedTimeZone)?;
        let time_zone_info = time_zone.to_offset_info(timestamp);
        let zoned = ZonedDateTime::from_epoch_milliseconds_and_utc_offset(
            milliseconds,
            icu_datetime::input::UtcOffset::try_from_seconds(time_zone_info.offset().seconds())
                .map_err(|_| DateTimeFormatError::Formatter)?,
        );
        let datetime = DateTime {
            date: zoned.date,
            time: zoned.time,
        };
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
        let mut parts = writer.into_parts();
        if let Some(style) = &self.options.time_zone_name {
            parts.push(DateTimePart {
                kind: "literal".into(),
                value: " ".into(),
            });
            parts.push(DateTimePart {
                kind: "timeZoneName".into(),
                value: time_zone_display_name(
                    style,
                    &self.time_zone,
                    time_zone_info.offset().seconds(),
                    time_zone_info.abbreviation(),
                ),
            });
        }
        Ok(parts)
    }

    fn field_set(&self) -> CompositeDateTimeFieldSet {
        let date = self.options.date_style.is_some()
            || self.options.weekday.is_some()
            || self.options.era.is_some()
            || self.options.year.is_some()
            || self.options.month.is_some()
            || self.options.day.is_some();
        let time = self.options.time_style.is_some()
            || self.options.hour.is_some()
            || self.options.minute.is_some()
            || self.options.second.is_some()
            || self.options.fractional_second_digits.is_some();
        let length = self.length();
        if date && time {
            let ymd = YMD::for_length(length);
            let ymdt = match self.options.fractional_second_digits {
                Some(digits) => ymd.with_time_hmss(
                    SubsecondDigits::try_from_int(digits).unwrap_or(SubsecondDigits::S3),
                ),
                None if self.options.second.is_some() || self.options.time_style.is_some() => {
                    ymd.with_time_hms()
                }
                None => ymd.with_time_hm(),
            };
            CompositeDateTimeFieldSet::DateTime(DateAndTimeFieldSet::YMDT(ymdt))
        } else if time {
            let field = match self.options.fractional_second_digits {
                Some(digits) => {
                    T::hmss(SubsecondDigits::try_from_int(digits).unwrap_or(SubsecondDigits::S3))
                }
                None if self.options.second.is_some() || self.options.time_style.is_some() => {
                    T::hms()
                }
                None => T::hm(),
            };
            CompositeDateTimeFieldSet::Time(TimeFieldSet::T(field))
        } else {
            // `ToDateTimeOptions` supplies year/month/day for FormatDateTime
            // when callers select neither a date nor time component.
            CompositeDateTimeFieldSet::Date(DateFieldSet::YMD(YMD::for_length(length)))
        }
    }

    fn length(&self) -> Length {
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
}
