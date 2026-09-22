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

fn resolve_basic_semantic_format(
    locale: &str,
    options: &mut DateTimeFormatOptions,
) -> Option<BasicAppendPlan> {
    if options.format_matcher != DateTimeFormatMatcher::Basic
        || options.date_style.is_some()
        || options.time_style.is_some()
    {
        return None;
    }
    let formats = locale_data_provider().date_time_format_records(locale);
    let selected = basic_format_matcher(options, &formats)?;
    let requested = DateTimeFormatRecord::from_options(options);
    let selected = &formats[selected];

    // A raw CLDR `availableFormats` record is directly renderable only when
    // it contains every requested component.  ICU4X's dynamic formatter
    // already combines date and time records through the locale's complete
    // date-time glue table. Replacing its requested field set with an
    // incomplete raw record would discard that glue and can expose a pattern
    // field (for example a literal `Y`) instead of a localized year.
    if basic_record_covers(selected, &requested) {
        selected.apply_to(options);
        return None;
    }

    let mut base_options = options.clone();
    selected.apply_to(&mut base_options);
    if let Some(plan) = basic_append_plan(locale, options, &requested, selected, base_options) {
        return Some(plan);
    } else {
        // Never discard requested fields if a malformed provider row cannot
        // be synthesized. The complete pinned provider makes this defensive
        // fallback unreachable in normal builds.
        debug_assert!(missing_basic_components_have_append_items(
            locale, &requested, selected
        ));
    }
    None
}

fn basic_record_covers(selected: &DateTimeFormatRecord, requested: &DateTimeFormatRecord) -> bool {
    [
        (requested.weekday.is_some(), selected.weekday.is_some()),
        (requested.era.is_some(), selected.era.is_some()),
        (requested.year.is_some(), selected.year.is_some()),
        (requested.month.is_some(), selected.month.is_some()),
        (requested.day.is_some(), selected.day.is_some()),
        (
            requested.day_period.is_some(),
            selected.day_period.is_some(),
        ),
        (requested.hour.is_some(), selected.hour.is_some()),
        (requested.minute.is_some(), selected.minute.is_some()),
        (requested.second.is_some(), selected.second.is_some()),
        (
            requested.fractional_second_digits.is_some(),
            selected.fractional_second_digits.is_some(),
        ),
        (
            requested.time_zone_name.is_some(),
            selected.time_zone_name.is_some(),
        ),
    ]
    .into_iter()
    .all(|(requested, selected)| !requested || selected)
}

fn missing_basic_components_have_append_items(
    locale: &str,
    requested: &DateTimeFormatRecord,
    selected: &DateTimeFormatRecord,
) -> bool {
    let has_append_item = |field| {
        locale_data_provider()
            .date_time_append_item(locale, field)
            .is_some_and(|item| item.pattern.contains("{0}") && item.pattern.contains("{1}"))
    };
    [
        (
            requested.weekday.is_some() && selected.weekday.is_none(),
            "Day-Of-Week",
        ),
        (requested.era.is_some() && selected.era.is_none(), "Era"),
        (requested.year.is_some() && selected.year.is_none(), "Year"),
        (
            requested.month.is_some() && selected.month.is_none(),
            "Month",
        ),
        (requested.day.is_some() && selected.day.is_none(), "Day"),
        (
            requested.day_period.is_some() && selected.day_period.is_none(),
            "Hour",
        ),
        (requested.hour.is_some() && selected.hour.is_none(), "Hour"),
        (
            requested.minute.is_some() && selected.minute.is_none(),
            "Minute",
        ),
        (
            requested.second.is_some() && selected.second.is_none(),
            "Second",
        ),
        (
            requested.fractional_second_digits.is_some()
                && selected.fractional_second_digits.is_none(),
            "Second",
        ),
        (
            requested.time_zone_name.is_some() && selected.time_zone_name.is_none(),
            "Timezone",
        ),
    ]
    .into_iter()
    .all(|(missing, field)| !missing || has_append_item(field))
}

fn basic_append_plan(
    locale: &str,
    original_options: &DateTimeFormatOptions,
    requested: &DateTimeFormatRecord,
    selected: &DateTimeFormatRecord,
    base_options: DateTimeFormatOptions,
) -> Option<BasicAppendPlan> {
    let missing_date = DateTimeFormatRecord {
        weekday: requested.weekday.filter(|_| selected.weekday.is_none()),
        era: requested.era.filter(|_| selected.era.is_none()),
        year: requested.year.filter(|_| selected.year.is_none()),
        month: requested.month.filter(|_| selected.month.is_none()),
        day: requested.day.filter(|_| selected.day.is_none()),
        ..Default::default()
    };
    let missing_time = DateTimeFormatRecord {
        day_period: requested
            .day_period
            .filter(|_| selected.day_period.is_none()),
        hour: requested.hour.filter(|_| selected.hour.is_none()),
        minute: requested.minute.filter(|_| selected.minute.is_none()),
        second: requested.second.filter(|_| selected.second.is_none()),
        fractional_second_digits: requested
            .fractional_second_digits
            .filter(|_| selected.fractional_second_digits.is_none()),
        time_zone_name: requested
            .time_zone_name
            .clone()
            .filter(|_| selected.time_zone_name.is_none()),
        ..Default::default()
    };
    let date_item = basic_append_item(locale, original_options, &missing_date, true);
    let time_item = basic_append_item(locale, original_options, &missing_time, false);
    if date_item.is_none() && time_item.is_none() {
        return None;
    }

    let selected_has_date = record_has_date_fields(selected);
    let selected_has_time = record_has_time_fields(selected);
    let mut items = Vec::with_capacity(2);
    if !selected_has_date && selected_has_time {
        items.extend(time_item);
        items.extend(date_item);
    } else {
        items.extend(date_item);
        items.extend(time_item);
    }
    Some(BasicAppendPlan {
        base_options,
        items,
    })
}

fn basic_append_item(
    locale: &str,
    original_options: &DateTimeFormatOptions,
    record: &DateTimeFormatRecord,
    is_date: bool,
) -> Option<BasicAppendItem> {
    let field = if is_date {
        append_date_field(record)
    } else {
        append_time_field(record)
    }?;
    let append = locale_data_provider().date_time_append_item(locale, field)?;
    if append.field_name.is_empty()
        || !append.pattern.contains("{0}")
        || !append.pattern.contains("{1}")
    {
        return None;
    }
    Some(BasicAppendItem {
        pattern: append.pattern,
        field_name: append.field_name,
        component_options: component_options_for_record(original_options, record),
    })
}

fn append_date_field(record: &DateTimeFormatRecord) -> Option<&'static str> {
    if record.era.is_some() {
        Some("Era")
    } else if record.year.is_some() {
        Some("Year")
    } else if record.month.is_some() {
        Some("Month")
    } else if record.day.is_some() {
        Some("Day")
    } else {
        record.weekday.is_some().then_some("Day-Of-Week")
    }
}

fn append_time_field(record: &DateTimeFormatRecord) -> Option<&'static str> {
    if record.day_period.is_some() || record.hour.is_some() {
        Some("Hour")
    } else if record.minute.is_some() {
        Some("Minute")
    } else if record.second.is_some() || record.fractional_second_digits.is_some() {
        Some("Second")
    } else {
        record.time_zone_name.is_some().then_some("Timezone")
    }
}

fn record_has_date_fields(record: &DateTimeFormatRecord) -> bool {
    record.weekday.is_some()
        || record.era.is_some()
        || record.year.is_some()
        || record.month.is_some()
        || record.day.is_some()
}

fn record_has_time_fields(record: &DateTimeFormatRecord) -> bool {
    record.day_period.is_some()
        || record.hour.is_some()
        || record.minute.is_some()
        || record.second.is_some()
        || record.fractional_second_digits.is_some()
        || record.time_zone_name.is_some()
}

fn component_options_for_record(
    original: &DateTimeFormatOptions,
    record: &DateTimeFormatRecord,
) -> DateTimeFormatOptions {
    let mut options = original.clone();
    options.date_style = None;
    options.time_style = None;
    options.weekday = None;
    options.era = None;
    options.year = None;
    options.month = None;
    options.day = None;
    options.day_period = None;
    options.hour = None;
    options.minute = None;
    options.second = None;
    options.fractional_second_digits = None;
    options.time_zone_name = None;
    record.apply_to(&mut options);
    options
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
    basic_append_plan: Option<BasicAppendPlan>,
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

/// A CLDR `appendItems` synthesis selected by BasicFormatMatcher.
///
/// `options` remains the original, JavaScript-observable component request.
/// The plan separately carries the raw matched skeleton's field boundary so
/// an incomplete CLDR record can be combined through its literal pattern
/// without exposing ICU4X's incomplete raw-skeleton renderer.
#[derive(Clone, Debug)]
struct BasicAppendPlan {
    base_options: DateTimeFormatOptions,
    items: Vec<BasicAppendItem>,
}

#[derive(Clone, Debug)]
struct BasicAppendItem {
    pattern: String,
    field_name: String,
    component_options: DateTimeFormatOptions,
}

fn date_time_options_bytes(options: &DateTimeFormatOptions) -> usize {
    [
        options.calendar.as_ref(),
        options.numbering_system.as_ref(),
        options.hour_cycle.as_ref(),
        options.time_zone.as_ref(),
        options.time_zone_name.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(String::len)
    .sum()
}

impl BasicAppendPlan {
    fn bytes(&self) -> usize {
        self.items.capacity() * std::mem::size_of::<BasicAppendItem>()
            + date_time_options_bytes(&self.base_options)
            + self
                .items
                .iter()
                .map(|item| {
                    item.pattern.len()
                        + item.field_name.len()
                        + date_time_options_bytes(&item.component_options)
                })
                .sum::<usize>()
    }
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
        // ECMA-402 accepts the two deprecated Islamic identifiers for
        // DateTimeFormat, but requires their observable calendar to be a
        // supported fallback. The provider's civil calendar is the stable,
        // advertised fallback for both.
        "islamic" | "islamic-rgsa" | "islamicc" => "islamic-civil",
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
        let selected_locale = crate::resolve_locale(
            crate::IntlService::DateTimeFormat,
            requested,
            options.locale_matcher,
        )
        .selected()
        .clone();
        let basic_append_plan =
            resolve_basic_semantic_format(selected_locale.as_str(), &mut options);
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
            basic_append_plan,
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
        self.apply_zero_offset_range_zone_name(&mut parts, start_offset, end_offset);
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

    fn rendering_with_options(&self, options: DateTimeFormatOptions) -> Self {
        let mut formatter = self.clone();
        formatter.options = options;
        formatter.basic_append_plan = None;
        formatter
    }

    fn format_basic_append_to_parts_from_milliseconds(
        &self,
        milliseconds: i64,
        include_time_zone_name: bool,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        let plan = self
            .basic_append_plan
            .clone()
            .expect("Basic append formatter has a synthesis plan");
        let complete = self
            .rendering_with_options(self.options.clone())
            .format_to_parts_from_milliseconds(milliseconds, include_time_zone_name)?;
        let mut parts = self
            .rendering_with_options(plan.base_options)
            .filter_unrequested_parts(complete);
        for item in plan.items {
            let appended = self
                .rendering_with_options(item.component_options)
                .format_to_parts_from_milliseconds(milliseconds, include_time_zone_name)?;
            parts = append_datetime_parts(&item.pattern, &item.field_name, parts, appended)
                .ok_or(DateTimeFormatError::Formatter)?;
        }
        Ok(parts)
    }

    fn format_to_parts_from_milliseconds(
        &self,
        milliseconds: i64,
        include_time_zone_name: bool,
    ) -> Result<Vec<DateTimePart>, DateTimeFormatError> {
        if self.basic_append_plan.is_some() {
            return self.format_basic_append_to_parts_from_milliseconds(
                milliseconds,
                include_time_zone_name,
            );
        }
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
        self.trim_numeric_date_part_padding(&mut parts);
        self.apply_numbering_system_punctuation(&mut parts);
        self.apply_style_part_widths(&mut parts);
        self.apply_flexible_day_period(&mut parts, datetime.time.hour.number());
        self.apply_h24_hour_cycle(&mut parts, datetime.time.hour.number());
        self.apply_calendar_part_completeness(&mut parts);
        self.apply_zero_offset_zone_name(&mut parts, offset_seconds);
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
        self.trim_numeric_date_part_padding(&mut parts);
        self.apply_numbering_system_punctuation(&mut parts);
        self.apply_style_part_widths(&mut parts);
        self.apply_flexible_day_period(&mut parts, datetime.time.hour.number());
        self.apply_h24_hour_cycle(&mut parts, datetime.time.hour.number());
        self.apply_calendar_part_completeness(&mut parts);
        self.apply_zero_offset_zone_name(&mut parts, offset_seconds);
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
            "timeZoneName" => self.effective_time_zone_name().is_some(),
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

        // ICU4X's English time skeleton emits a narrow no-break space before
        // an AM/PM field. ECMA-402's en-US-compatible result uses a regular
        // space, including under a Unicode numbering-system override. Limit
        // this to the literal directly preceding the day period so other
        // locale-specific narrow spaces remain data-owned.
        if self.locale.locale().id.language.as_str() == "en" {
            for index in 1..parts.len() {
                if parts[index].kind == "dayPeriod" && parts[index - 1].kind == "literal" {
                    parts[index - 1].value = parts[index - 1].value.replace('\u{202f}', " ");
                }
            }
        }
    }

    /// `Alignment::Column` is required for a requested two-digit clock
    /// component, but ICU4X applies it to date fields in the same dynamic
    /// field set as well. ECMA-402 treats widths independently: a numeric
    /// month/day must not gain a leading localized zero merely because the
    /// caller requested `minute: "2-digit"`. Remove only that provider-added
    /// zero from explicitly numeric date fields.
    fn trim_numeric_date_part_padding(&self, parts: &mut [DateTimePart]) {
        let zero = locale_data_provider()
            .decimal_digits(&self.numbering_system)
            .map_or('0', |digits| digits[0]);
        for part in parts {
            let numeric_width = match part.kind.as_str() {
                "month" => self.options.month,
                "day" => self.options.day,
                _ => continue,
            };
            if numeric_width != Some(DateTimeWidth::Numeric) {
                continue;
            }
            if let Some(unpadded) = part
                .value
                .strip_prefix(zero)
                .filter(|value| !value.is_empty())
            {
                part.value = unpadded.into();
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

    /// `hourCycle: "h24"` has the same 1-24 range as `h23`'s 0-23 range
    /// except that midnight reads `24` instead of `0`. ICU4X's dynamic
    /// semantic skeleton (`icu_datetime`) does not have an `h24` field-set
    /// preference of its own — `resolve_date_time_locale` deliberately
    /// substitutes its `h23` skeleton for formatting (see
    /// `formatting_hour_cycle`) while keeping `h24` as the ECMA-402-visible
    /// resolved value, so the rendered digits still read `0`/`00` at
    /// midnight and need this one-value correction. Test262's
    /// `intl402/Temporal/{Instant,PlainTime}/prototype/toLocaleString/
    /// hourcycle.js` pins the exact rendered text ("24:00:00"), so this
    /// substitutes the same locale-specific digit glyphs
    /// `trim_numeric_date_part_padding` looks up rather than assuming ASCII.
    fn apply_h24_hour_cycle(&self, parts: &mut [DateTimePart], hour: u8) {
        if self.hour_cycle != "h24" || hour != 0 {
            return;
        }
        let digits = locale_data_provider()
            .decimal_digits(&self.numbering_system)
            .unwrap_or(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9']);
        let twenty_four: String = [digits[2], digits[4]].into_iter().collect();
        for part in parts.iter_mut() {
            if part.kind == "hour" {
                part.value = twenty_four.clone();
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
        self.trim_numeric_date_range_part_padding(parts);

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

        if self.locale.locale().id.language.as_str() == "en" {
            for index in 1..parts.len() {
                if parts[index].kind == "dayPeriod" && parts[index - 1].kind == "literal" {
                    parts[index - 1].value = parts[index - 1].value.replace('\u{202f}', " ");
                }
            }
        }
    }

    fn trim_numeric_date_range_part_padding(&self, parts: &mut [DateTimeRangePart]) {
        let zero = locale_data_provider()
            .decimal_digits(&self.numbering_system)
            .map_or('0', |digits| digits[0]);
        for part in parts {
            let numeric_width = match part.kind.as_str() {
                "month" => self.options.month,
                "day" => self.options.day,
                _ => continue,
            };
            if numeric_width != Some(DateTimeWidth::Numeric) {
                continue;
            }
            if let Some(unpadded) = part
                .value
                .strip_prefix(zero)
                .filter(|value| !value.is_empty())
            {
                part.value = unpadded.into();
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
            + date_time_options_bytes(&self.options)
            + self
                .basic_append_plan
                .as_ref()
                .map_or(0, BasicAppendPlan::bytes)
            + self.calendar.len()
            + self.numbering_system.len()
            + self.hour_cycle.len()
            + self.time_zone.len()
    }
}

/// Expands one CLDR `appendItems` pattern while preserving the existing typed
/// DateTimeFormat parts. `{0}` is the matched base skeleton, `{1}` is the
/// formatter built for missing requested fields, and `{2}` is CLDR's localized
/// display name for that field.
fn append_datetime_parts(
    pattern: &str,
    field_name: &str,
    base: Vec<DateTimePart>,
    appended: Vec<DateTimePart>,
) -> Option<Vec<DateTimePart>> {
    let mut parts = Vec::with_capacity(base.len() + appended.len() + 3);
    let mut literal = String::new();
    let mut index = 0usize;
    let mut quoted = false;
    let mut inserted_base = false;
    let mut inserted_appended = false;

    let push_literal = |value: &str, parts: &mut Vec<DateTimePart>| {
        if value.is_empty() {
            return;
        }
        if let Some(previous) = parts.last_mut().filter(|part| part.kind == "literal") {
            previous.value.push_str(value);
        } else {
            parts.push(DateTimePart {
                kind: "literal".into(),
                value: value.into(),
            });
        }
    };
    let push_parts = |source: &[DateTimePart], parts: &mut Vec<DateTimePart>| {
        for part in source {
            if part.kind == "literal" {
                push_literal(&part.value, parts);
            } else {
                parts.push(part.clone());
            }
        }
    };

    while index < pattern.len() {
        let remaining = &pattern[index..];
        if remaining.starts_with("''") {
            literal.push('\'');
            index += 2;
            continue;
        }
        if remaining.starts_with('\'') {
            quoted = !quoted;
            index += 1;
            continue;
        }
        if !quoted && remaining.starts_with("{0}") {
            push_literal(&literal, &mut parts);
            literal.clear();
            push_parts(&base, &mut parts);
            inserted_base = true;
            index += 3;
            continue;
        }
        if !quoted && remaining.starts_with("{1}") {
            push_literal(&literal, &mut parts);
            literal.clear();
            push_parts(&appended, &mut parts);
            inserted_appended = true;
            index += 3;
            continue;
        }
        if !quoted && remaining.starts_with("{2}") {
            literal.push_str(field_name);
            index += 3;
            continue;
        }
        let character = remaining.chars().next()?;
        literal.push(character);
        index += character.len_utf8();
    }
    (!quoted && inserted_base && inserted_appended).then(|| {
        push_literal(&literal, &mut parts);
        parts
    })
}

mod details;
mod zero_offset;

use details::*;

#[cfg(test)]
#[path = "date_time_format/append_datetime_parts_tests.rs"]
mod append_datetime_parts_tests;
