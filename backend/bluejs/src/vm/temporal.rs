// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Temporal value surface: read-only construction/formatting (Phase 26
//! Stage 0) plus Stage 1's calendar-agnostic arithmetic tracks
//! (`Temporal.Instant`, `Temporal.PlainTime`, `Temporal.Duration`).
//! Calendar-aware arithmetic (`PlainDate`/`PlainDateTime`/`PlainYearMonth`/
//! `PlainMonthDay`/`ZonedDateTime`) remains Stage 2, per Phase 26's plan
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! Host-neutral foundation pieces (ISO 8601 grammar, epoch-nanosecond math,
//! the calendar-identifier table, unit/rounding-mode vocabulary and
//! calendar-agnostic duration math) live in the `iso`/`epoch`/`calendar`/
//! `rounding`/`duration_math` submodules — plain Rust with no `Value`/
//! heap/Realm coupling, directly unit-testable without a VM. This file
//! stays the `impl Vm` adapter layer over them.

use super::*;
use crate::heap::{TemporalKind, TemporalValue};
use icu_calendar::{types::DateFields, AnyCalendar, Date, Iso};
use num_traits::ToPrimitive;
mod calendar;
mod duration_math;
mod epoch;
mod iso;
mod rounding;

/// The calendar fields exposed by Temporal are derived from its ISO internal
/// date. Keeping ISO fields in `TemporalValue` preserves the invariant used
/// by DateTimeFormat's plain-value bridge while ICU4X performs the actual
/// non-ISO conversion at each observable calendar boundary.
struct TemporalCalendarFields {
    year: i32,
    month: u8,
    month_code: String,
    day: u8,
    era: Option<String>,
    era_year: Option<i32>,
    months_in_year: u8,
}

impl Vm {
    pub(super) fn temporal_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("Temporal") {
            return Ok(Value::Object(id));
        }
        let string = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(string)?.unwrap();
        let object_prototype = self.object_prototype;
        let namespace = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(namespace)?;
        let base = self.stack.len();
        let result = (|| {
            self.define_data(
                namespace,
                JsSymbol::well_known("toStringTag"),
                Value::String("Temporal".into()),
                false,
                false,
                true,
            )?;
            for kind in [
                TemporalKind::Duration,
                TemporalKind::Instant,
                TemporalKind::PlainDate,
                TemporalKind::PlainDateTime,
                TemporalKind::PlainMonthDay,
                TemporalKind::PlainTime,
                TemporalKind::PlainYearMonth,
                TemporalKind::ZonedDateTime,
            ] {
                self.install_native(
                    namespace,
                    function_prototype,
                    kind.name(),
                    match kind {
                        TemporalKind::Duration => 10,
                        TemporalKind::PlainDate => 3,
                        TemporalKind::PlainDateTime => 3,
                        TemporalKind::PlainMonthDay => 2,
                        TemporalKind::PlainTime => 0,
                        TemporalKind::PlainYearMonth => 2,
                        TemporalKind::Instant | TemporalKind::ZonedDateTime => 1,
                    },
                    NativeFunction::TemporalConstructor(kind),
                )?;
                let constructor = self.heap.get(namespace, kind.name())?.object_id().unwrap();
                let prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.stack.push(Value::Object(prototype));
                self.define_data(
                    constructor,
                    "prototype",
                    Value::Object(prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    prototype,
                    "constructor",
                    Value::Object(constructor),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String(kind.to_string_tag().into()),
                    false,
                    false,
                    true,
                )?;
                self.install_native(
                    constructor,
                    function_prototype,
                    "from",
                    1,
                    NativeFunction::TemporalFrom(kind),
                )?;
                if matches!(kind, TemporalKind::PlainDate | TemporalKind::PlainDateTime) {
                    self.install_native(
                        prototype,
                        function_prototype,
                        "withCalendar",
                        1,
                        NativeFunction::TemporalWithCalendar,
                    )?;
                    self.install_native(
                        prototype,
                        function_prototype,
                        "toZonedDateTime",
                        1,
                        NativeFunction::TemporalPlainToZonedDateTime,
                    )?;
                }
                let getters: &[(&str, native::TemporalGetter)] = match kind {
                    TemporalKind::Duration => &[
                        ("years", native::TemporalGetter::DurationYears),
                        ("months", native::TemporalGetter::DurationMonths),
                        ("weeks", native::TemporalGetter::DurationWeeks),
                        ("days", native::TemporalGetter::DurationDays),
                        ("hours", native::TemporalGetter::DurationHours),
                        ("minutes", native::TemporalGetter::DurationMinutes),
                        ("seconds", native::TemporalGetter::DurationSeconds),
                        ("milliseconds", native::TemporalGetter::DurationMilliseconds),
                        ("microseconds", native::TemporalGetter::DurationMicroseconds),
                        ("nanoseconds", native::TemporalGetter::DurationNanoseconds),
                    ],
                    TemporalKind::PlainDate | TemporalKind::PlainDateTime => &[
                        ("calendarId", native::TemporalGetter::CalendarId),
                        ("year", native::TemporalGetter::Year),
                        ("month", native::TemporalGetter::Month),
                        ("monthCode", native::TemporalGetter::MonthCode),
                        ("day", native::TemporalGetter::Day),
                        ("era", native::TemporalGetter::Era),
                        ("eraYear", native::TemporalGetter::EraYear),
                        ("monthsInYear", native::TemporalGetter::MonthsInYear),
                    ],
                    TemporalKind::PlainMonthDay => &[
                        ("calendarId", native::TemporalGetter::CalendarId),
                        ("month", native::TemporalGetter::Month),
                        ("monthCode", native::TemporalGetter::MonthCode),
                        ("day", native::TemporalGetter::Day),
                    ],
                    TemporalKind::PlainYearMonth => &[
                        ("calendarId", native::TemporalGetter::CalendarId),
                        ("year", native::TemporalGetter::Year),
                        ("month", native::TemporalGetter::Month),
                        ("monthCode", native::TemporalGetter::MonthCode),
                        ("era", native::TemporalGetter::Era),
                        ("eraYear", native::TemporalGetter::EraYear),
                        ("monthsInYear", native::TemporalGetter::MonthsInYear),
                    ],
                    TemporalKind::ZonedDateTime => &[
                        ("calendarId", native::TemporalGetter::CalendarId),
                        (
                            "epochMilliseconds",
                            native::TemporalGetter::EpochMilliseconds,
                        ),
                        ("epochNanoseconds", native::TemporalGetter::EpochNanoseconds),
                    ],
                    TemporalKind::Instant => &[
                        (
                            "epochMilliseconds",
                            native::TemporalGetter::EpochMilliseconds,
                        ),
                        ("epochNanoseconds", native::TemporalGetter::EpochNanoseconds),
                    ],
                    TemporalKind::PlainTime => &[
                        ("hour", native::TemporalGetter::Hour),
                        ("minute", native::TemporalGetter::Minute),
                        ("second", native::TemporalGetter::Second),
                        ("millisecond", native::TemporalGetter::Millisecond),
                        ("microsecond", native::TemporalGetter::Microsecond),
                        ("nanosecond", native::TemporalGetter::Nanosecond),
                    ],
                };
                for (name, getter) in getters {
                    self.install_getter(
                        prototype,
                        function_prototype,
                        (*name).into(),
                        &format!("get {name}"),
                        NativeFunction::TemporalGetter(*getter),
                    )?;
                }
                if kind == TemporalKind::ZonedDateTime {
                    self.install_native(
                        prototype,
                        function_prototype,
                        "toLocaleString",
                        0,
                        NativeFunction::TemporalZonedDateTimeToLocaleString,
                    )?;
                }
                if kind == TemporalKind::Instant {
                    for (name, arity, method) in [
                        ("add", 1, NativeFunction::TemporalInstantAdd),
                        ("subtract", 1, NativeFunction::TemporalInstantSubtract),
                        ("round", 1, NativeFunction::TemporalInstantRound),
                        ("until", 1, NativeFunction::TemporalInstantUntil),
                        ("since", 1, NativeFunction::TemporalInstantSince),
                        ("equals", 1, NativeFunction::TemporalInstantEquals),
                        ("toString", 0, NativeFunction::TemporalInstantToString),
                        ("toJSON", 0, NativeFunction::TemporalInstantToJson),
                        ("toLocaleString", 0, NativeFunction::TemporalInstantToString),
                        ("valueOf", 0, NativeFunction::TemporalInstantValueOf),
                    ] {
                        self.install_native(prototype, function_prototype, name, arity, method)?;
                    }
                    self.install_native(
                        constructor,
                        function_prototype,
                        "compare",
                        2,
                        NativeFunction::TemporalInstantCompare,
                    )?;
                    self.install_native(
                        constructor,
                        function_prototype,
                        "fromEpochMilliseconds",
                        1,
                        NativeFunction::TemporalFromEpochMilliseconds,
                    )?;
                    self.install_native(
                        constructor,
                        function_prototype,
                        "fromEpochNanoseconds",
                        1,
                        NativeFunction::TemporalFromEpochNanoseconds,
                    )?;
                }
                if kind == TemporalKind::PlainTime {
                    for (name, arity, method) in [
                        ("add", 1, NativeFunction::TemporalPlainTimeAdd),
                        ("subtract", 1, NativeFunction::TemporalPlainTimeSubtract),
                        ("round", 1, NativeFunction::TemporalPlainTimeRound),
                        ("until", 1, NativeFunction::TemporalPlainTimeUntil),
                        ("since", 1, NativeFunction::TemporalPlainTimeSince),
                        ("equals", 1, NativeFunction::TemporalPlainTimeEquals),
                        ("with", 1, NativeFunction::TemporalPlainTimeWith),
                        ("toString", 0, NativeFunction::TemporalPlainTimeToString),
                        ("toJSON", 0, NativeFunction::TemporalPlainTimeToJson),
                        ("toLocaleString", 0, NativeFunction::TemporalPlainTimeToJson),
                        ("valueOf", 0, NativeFunction::TemporalPlainTimeValueOf),
                    ] {
                        self.install_native(prototype, function_prototype, name, arity, method)?;
                    }
                    self.install_native(
                        constructor,
                        function_prototype,
                        "compare",
                        2,
                        NativeFunction::TemporalPlainTimeCompare,
                    )?;
                }
                self.globals
                    .insert(format!("%Temporal.{}%", kind.name()), constructor);
                self.stack.pop();
            }
            self.globals.insert("Temporal".into(), namespace);
            if let Some(&global) = self.globals.get("globalThis") {
                self.define_data(
                    global,
                    "Temporal",
                    Value::Object(namespace),
                    true,
                    false,
                    true,
                )?;
            }
            Ok(Value::Object(namespace))
        })();
        self.stack.truncate(base);
        self.heap.unroot(root)?;
        result
    }

    fn temporal_integer(
        &mut self,
        value: &Value,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        let value = self.coerce_number(value)?;
        if !value.is_finite()
            || value.fract() != 0.0
            || !(f64::from(minimum)..=f64::from(maximum)).contains(&value)
        {
            return Err(RuntimeError::RangeError(format!("invalid Temporal {name}")));
        }
        Ok(value as i32)
    }

    fn temporal_calendar(&mut self, value: &Value) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok("iso8601".into());
        }
        let value = self.coerce_string(value)?;
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar".into()))?;
        if value.is_empty() {
            return Err(RuntimeError::RangeError("invalid Temporal calendar".into()));
        }
        let lower = value.to_ascii_lowercase();
        let value = match lower.as_str() {
            // ECMA-402-visible aliases must carry the canonical calendar
            // identifier through Temporal as well as Intl.Locale.
            "islamicc" => "islamic-civil",
            "ethiopic-amete-alem" => "ethioaa",
            value => value,
        };
        calendar::calendar_kind(value)
            .is_some()
            .then(|| value.into())
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal calendar".into()))
    }

    fn temporal_calendar_fields(
        &self,
        value: &TemporalValue,
    ) -> Result<TemporalCalendarFields, RuntimeError> {
        let calendar = calendar::calendar_kind(&value.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let iso = Date::try_new_iso(value.year, value.month, value.day)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal ISO date".into()))?;
        let date = iso.to_calendar(AnyCalendar::new(calendar));
        let year = date.year();
        let month = date.month();
        let era = year.era();
        Ok(TemporalCalendarFields {
            year: year.extended_year(),
            // Temporal's numeric `month` is the ordinal month in a year.
            // A leap month therefore increments every following ordinal,
            // whereas a MonthCode retains the calendar's base month plus L.
            month: month.ordinal,
            month_code: month.to_input().code().to_string(),
            day: date.day_of_month().0,
            era: era.map(|era| era.era.to_string()),
            era_year: era.map(|era| era.year),
            months_in_year: date.months_in_year(),
        })
    }

    fn temporal_value_from_calendar_date(
        kind: TemporalKind,
        calendar: String,
        date: Date<AnyCalendar>,
    ) -> TemporalValue {
        let iso = date.to_calendar(Iso);
        TemporalValue {
            kind,
            duration: None,
            year: iso.year().extended_year(),
            month: iso.month().number(),
            day: iso.day_of_month().0,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            microsecond: 0,
            nanosecond: 0,
            epoch_nanoseconds: 0.into(),
            calendar,
            time_zone: "UTC".into(),
        }
    }

    fn temporal_plain_date_from_fields(
        &mut self,
        kind: TemporalKind,
        bag: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar(&calendar_value)?;
        let year = self.get_property(bag, &"year".into())?;
        let month = self.get_property(bag, &"month".into())?;
        let month_code = self.get_property(bag, &"monthCode".into())?;
        let day = self.get_property(bag, &"day".into())?;
        let era = self.get_property(bag, &"era".into())?;
        let era_year = self.get_property(bag, &"eraYear".into())?;

        let mut fields = DateFields::default();
        let requested_year = (!matches!(year, Value::Undefined))
            .then(|| self.temporal_integer(&year, -9_999, 9_999, "year"))
            .transpose()?;
        let era = (!matches!(era, Value::Undefined))
            .then(|| self.coerce_string(&era))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
            })
            .transpose()?;
        if let Some(era) = era.as_deref() {
            fields.era = Some(era.as_bytes());
            fields.era_year = Some(self.temporal_integer(&era_year, -9_999, 9_999, "era year")?);
        } else {
            if !matches!(era_year, Value::Undefined) {
                return Err(RuntimeError::RangeError(
                    "Temporal eraYear requires an era".into(),
                ));
            }
            fields.extended_year = Some(requested_year.ok_or_else(|| {
                RuntimeError::TypeError("Temporal date fields require year".into())
            })?);
        }

        let requested_month = (!matches!(month, Value::Undefined))
            .then(|| self.temporal_integer(&month, 1, 99, "month"))
            .transpose()?;
        let month_code = (!matches!(month_code, Value::Undefined))
            .then(|| self.coerce_string(&month_code))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;
        if let Some(month_code) = month_code.as_deref() {
            // Month codes preserve leap-month identity. If a property bag
            // also names `month`, validate it after calendar resolution.
            fields.month_code = Some(month_code.as_bytes());
        } else if let Some(month) = requested_month {
            fields.ordinal_month = Some(month as u8);
        } else {
            return Err(RuntimeError::TypeError(
                "Temporal date fields require month or monthCode".into(),
            ));
        }
        fields.day = Some(self.temporal_integer(&day, 1, 31, "day")? as u8);
        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar validates the calendar identifier");
        let mut options = icu_calendar::options::DateFromFieldsOptions::default();
        options.overflow = Some(icu_calendar::options::Overflow::Constrain);
        let date = Date::try_from_fields(fields, options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        let actual_year = date.year().extended_year();
        let actual_month = date.month().ordinal;
        if requested_year.is_some_and(|year| year != actual_year)
            || requested_month.is_some_and(|month| month as u8 != actual_month)
        {
            return Err(RuntimeError::RangeError(
                "inconsistent Temporal calendar fields".into(),
            ));
        }
        let mut value = Self::temporal_value_from_calendar_date(kind, calendar, date);
        if kind == TemporalKind::PlainDateTime {
            let hour = self.get_property(bag, &"hour".into())?;
            let minute = self.get_property(bag, &"minute".into())?;
            let second = self.get_property(bag, &"second".into())?;
            let millisecond = self.get_property(bag, &"millisecond".into())?;
            let microsecond = self.get_property(bag, &"microsecond".into())?;
            let nanosecond = self.get_property(bag, &"nanosecond".into())?;
            value.hour = self.temporal_optional_integer(&hour, 0, 0, 23, "hour")? as u8;
            value.minute = self.temporal_optional_integer(&minute, 0, 0, 59, "minute")? as u8;
            value.second = self.temporal_optional_integer(&second, 0, 0, 59, "second")? as u8;
            value.millisecond =
                self.temporal_optional_integer(&millisecond, 0, 0, 999, "millisecond")? as u16;
            value.microsecond =
                self.temporal_optional_integer(&microsecond, 0, 0, 999, "microsecond")? as u16;
            value.nanosecond =
                self.temporal_optional_integer(&nanosecond, 0, 0, 999, "nanosecond")? as u16;
        }
        Ok(value)
    }

    fn temporal_optional_integer(
        &mut self,
        value: &Value,
        default: i32,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        if *value == Value::Undefined {
            Ok(default)
        } else {
            self.temporal_integer(value, minimum, maximum, name)
        }
    }

    fn temporal_duration_integer(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<i128, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(0);
        }
        let value = self.coerce_number(value)?;
        if !value.is_finite() || value.fract() != 0.0 || value.abs() >= 2_f64.powi(100) {
            return Err(RuntimeError::RangeError(format!(
                "invalid Temporal.Duration {name}"
            )));
        }
        Ok(value as i128)
    }

    fn temporal_value_from_args(
        &mut self,
        kind: TemporalKind,
        args: &[Value],
    ) -> Result<TemporalValue, RuntimeError> {
        let number = |vm: &mut Self, index, minimum, maximum, name| {
            vm.temporal_integer(native::argument(args, index), minimum, maximum, name)
        };
        let mut value = TemporalValue {
            kind,
            duration: None,
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            microsecond: 0,
            nanosecond: 0,
            epoch_nanoseconds: 0.into(),
            calendar: "iso8601".into(),
            time_zone: "UTC".into(),
        };
        match kind {
            TemporalKind::Duration => {
                let values = [
                    self.temporal_duration_integer(native::argument(args, 0), "years")?,
                    self.temporal_duration_integer(native::argument(args, 1), "months")?,
                    self.temporal_duration_integer(native::argument(args, 2), "weeks")?,
                    self.temporal_duration_integer(native::argument(args, 3), "days")?,
                    self.temporal_duration_integer(native::argument(args, 4), "hours")?,
                    self.temporal_duration_integer(native::argument(args, 5), "minutes")?,
                    self.temporal_duration_integer(native::argument(args, 6), "seconds")?,
                    self.temporal_duration_integer(native::argument(args, 7), "milliseconds")?,
                    self.temporal_duration_integer(native::argument(args, 8), "microseconds")?,
                    self.temporal_duration_integer(native::argument(args, 9), "nanoseconds")?,
                ];
                value.duration = Some(Box::new(
                    blueice_ecma402::DurationRecord::try_new(
                        values[0], values[1], values[2], values[3], values[4], values[5],
                        values[6], values[7], values[8], values[9],
                    )
                    .map_err(|error| RuntimeError::RangeError(error.to_string()))?,
                ));
            }
            TemporalKind::Instant => {
                value.epoch_nanoseconds = match native::argument(args, 0) {
                    Value::BigInt(value) => value.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "Temporal.Instant requires epoch nanoseconds as a BigInt".into(),
                        ));
                    }
                };
                if !epoch::is_in_instant_range(&value.epoch_nanoseconds) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.Instant epoch nanoseconds are outside the supported range".into(),
                    ));
                }
            }
            TemporalKind::ZonedDateTime => {
                value.epoch_nanoseconds = match native::argument(args, 0) {
                    Value::BigInt(value) => value.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "Temporal.ZonedDateTime requires epoch nanoseconds as a BigInt".into(),
                        ));
                    }
                };
                if !epoch::is_in_instant_range(&value.epoch_nanoseconds) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range"
                            .into(),
                    ));
                }
                value.time_zone = self
                    .coerce_string(native::argument(args, 1))?
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
            }
            TemporalKind::PlainDate | TemporalKind::PlainDateTime => {
                value.year = number(self, 0, -271_821, 275_760, "year")?;
                value.month = number(self, 1, 1, 12, "month")? as u8;
                value.day = number(self, 2, 1, 31, "day")? as u8;
                if iso::days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
                    return Err(RuntimeError::RangeError("invalid Temporal day".into()));
                }
                if kind == TemporalKind::PlainDateTime {
                    value.hour = self.temporal_optional_integer(
                        native::argument(args, 3),
                        0,
                        0,
                        23,
                        "hour",
                    )? as u8;
                    value.minute = self.temporal_optional_integer(
                        native::argument(args, 4),
                        0,
                        0,
                        59,
                        "minute",
                    )? as u8;
                    value.second = self.temporal_optional_integer(
                        native::argument(args, 5),
                        0,
                        0,
                        59,
                        "second",
                    )? as u8;
                    value.millisecond = self.temporal_optional_integer(
                        native::argument(args, 6),
                        0,
                        0,
                        999,
                        "millisecond",
                    )? as u16;
                    value.microsecond = self.temporal_optional_integer(
                        native::argument(args, 7),
                        0,
                        0,
                        999,
                        "microsecond",
                    )? as u16;
                    value.nanosecond = self.temporal_optional_integer(
                        native::argument(args, 8),
                        0,
                        0,
                        999,
                        "nanosecond",
                    )? as u16;
                    value.calendar = self.temporal_calendar(native::argument(args, 9))?;
                } else {
                    value.calendar = self.temporal_calendar(native::argument(args, 3))?;
                }
            }
            TemporalKind::PlainMonthDay => {
                value.month = number(self, 0, 1, 12, "month")? as u8;
                value.day = number(self, 1, 1, 31, "day")? as u8;
                value.calendar = self.temporal_calendar(native::argument(args, 2))?;
                value.year = self.temporal_optional_integer(
                    native::argument(args, 3),
                    1972,
                    -271_821,
                    275_760,
                    "reference year",
                )?;
                if iso::days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
                    return Err(RuntimeError::RangeError("invalid Temporal day".into()));
                }
            }
            TemporalKind::PlainTime => {
                // `ToIntegerWithTruncation` then `RejectTime`: a fractional
                // argument truncates rather than throwing (`new
                // Temporal.PlainTime(11.9)` is hour 11), but an out-of-range
                // whole value is still a RangeError.
                let fields = [
                    self.temporal_optional_truncated_integer(native::argument(args, 0), "hour")?,
                    self.temporal_optional_truncated_integer(native::argument(args, 1), "minute")?,
                    self.temporal_optional_truncated_integer(native::argument(args, 2), "second")?,
                    self.temporal_optional_truncated_integer(
                        native::argument(args, 3),
                        "millisecond",
                    )?,
                    self.temporal_optional_truncated_integer(
                        native::argument(args, 4),
                        "microsecond",
                    )?,
                    self.temporal_optional_truncated_integer(
                        native::argument(args, 5),
                        "nanosecond",
                    )?,
                ];
                let fields = Self::temporal_regulate_time(fields, true)?;
                value.hour = fields.0;
                value.minute = fields.1;
                value.second = fields.2;
                value.millisecond = fields.3;
                value.microsecond = fields.4;
                value.nanosecond = fields.5;
            }
            TemporalKind::PlainYearMonth => {
                value.year = number(self, 0, -271_821, 275_760, "year")?;
                value.month = number(self, 1, 1, 12, "month")? as u8;
                value.calendar = self.temporal_calendar(native::argument(args, 2))?;
                value.day = self.temporal_optional_integer(
                    native::argument(args, 3),
                    1,
                    1,
                    31,
                    "reference day",
                )? as u8;
                if iso::days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
                    return Err(RuntimeError::RangeError(
                        "invalid Temporal reference day".into(),
                    ));
                }
            }
        }
        Ok(value)
    }

    pub(super) fn temporal_value_from_string(
        &mut self,
        kind: TemporalKind,
        source: &str,
    ) -> Result<TemporalValue, RuntimeError> {
        if kind == TemporalKind::Duration {
            let duration = iso::parse_duration_record(source).ok_or_else(|| {
                RuntimeError::RangeError("invalid Temporal.Duration string".into())
            })?;
            return Ok(TemporalValue {
                kind,
                duration: Some(Box::new(duration)),
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
                microsecond: 0,
                nanosecond: 0,
                epoch_nanoseconds: 0.into(),
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            });
        }
        if kind == TemporalKind::PlainTime {
            // A PlainTime string has its own grammar: it may carry no date at
            // all, and must reject a date-only string rather than treating it
            // as midnight. See `iso::parse_plain_time`.
            let fields = iso::parse_plain_time(source).ok_or_else(|| {
                RuntimeError::RangeError("invalid Temporal.PlainTime string".into())
            })?;
            return Ok(Self::plain_time_value(fields));
        }
        let (year, month, day) = iso::parse_date(source)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal date string".into()))?;
        let annotations = source.find('[').map_or("", |index| &source[index..]);
        let calendar = match iso::parse_annotations(annotations)
            .map_err(|()| RuntimeError::RangeError("invalid Temporal annotation".into()))?
        {
            Some(calendar) => {
                calendar::calendar_kind(&calendar).ok_or_else(|| {
                    RuntimeError::RangeError(format!("unsupported Temporal calendar: {calendar}"))
                })?;
                calendar
            }
            None => "iso8601".to_string(),
        };
        // Bound the search to the character immediately after the date
        // portion (mirroring `temporal_date`'s own boundary computation):
        // a global `split_once(['T', 't'])` would wrongly match the 'T' in
        // a `[UTC]` time-zone annotation on a date-only string.
        let date_end = source
            .find(['T', 't', '[', 'Z', 'z'])
            .unwrap_or(source.len());
        let time = source[date_end..].strip_prefix(['T', 't']);
        let (hour, minute, second, millisecond, microsecond, nanosecond) = match time {
            Some(time) => iso::parse_time(time)
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal time string".into()))?,
            None => (0, 0, 0, 0, 0, 0),
        };
        let epoch_nanoseconds = if kind == TemporalKind::Instant {
            let time = time.ok_or_else(|| {
                RuntimeError::RangeError("invalid Temporal Instant string".into())
            })?;
            let offset = iso::parse_offset_seconds(time).ok_or_else(|| {
                RuntimeError::RangeError("invalid Temporal Instant string".into())
            })?;
            let epoch_nanoseconds = epoch::nanoseconds_since_epoch(
                (year, month, day),
                (hour, minute, second, millisecond, microsecond, nanosecond),
                offset,
            );
            if !epoch::is_in_instant_range(&epoch_nanoseconds) {
                return Err(RuntimeError::RangeError(
                    "Temporal.Instant string is outside the supported range".into(),
                ));
            }
            epoch_nanoseconds
        } else {
            0.into()
        };
        Ok(TemporalValue {
            kind,
            duration: None,
            year: if kind == TemporalKind::PlainMonthDay {
                1972
            } else {
                year
            },
            month,
            day: if kind == TemporalKind::PlainYearMonth {
                1
            } else {
                day
            },
            hour,
            minute,
            second,
            millisecond,
            microsecond,
            nanosecond,
            epoch_nanoseconds,
            calendar,
            time_zone: "UTC".into(),
        })
    }

    fn alloc_temporal_value(
        &mut self,
        value: TemporalValue,
        use_new_target: bool,
    ) -> Result<Value, RuntimeError> {
        self.temporal_global()?;
        let constructor = self.globals[&format!("%Temporal.{}%", value.kind.name())];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Temporal constructor prototype is an object");
        let prototype = if use_new_target {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.with_roots(|heap| heap.alloc_temporal(value, Some(prototype)))
            .map(Value::Object)
    }

    pub(super) fn temporal_constructor(
        &mut self,
        kind: TemporalKind,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(format!(
                "Temporal.{} must be called with new",
                kind.name()
            )));
        }
        let base = self.stack.len();
        self.stack.extend(args.iter().cloned());
        let result = self
            .temporal_value_from_args(kind, args)
            .and_then(|value| self.alloc_temporal_value(value, true));
        self.stack.truncate(base);
        result
    }

    pub(super) fn temporal_from(
        &mut self,
        kind: TemporalKind,
        value: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        if kind == TemporalKind::PlainTime {
            // `ToTemporalTime` covers every accepted argument shape at once
            // (PlainTime/PlainDateTime/ZonedDateTime, property bag, string)
            // and is the only `from` that reads the `overflow` option.
            let base = self.stack.len();
            let result = self
                .temporal_to_plain_time(value, options)
                .and_then(|fields| {
                    self.alloc_temporal_value(Self::plain_time_value(fields), false)
                });
            self.stack.truncate(base);
            return result;
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == kind {
                    return self.alloc_temporal_value(temporal, false);
                }
            }
            if matches!(kind, TemporalKind::PlainDate | TemporalKind::PlainDateTime) {
                return self
                    .temporal_plain_date_from_fields(kind, value)
                    .and_then(|temporal| self.alloc_temporal_value(temporal, false));
            }
        }
        let source = self.coerce_string(value)?;
        let source = source
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal string".into()))?;
        self.temporal_value_from_string(kind, &source)
            .and_then(|temporal| self.alloc_temporal_value(temporal, false))
    }

    pub(super) fn temporal_with_calendar(
        &mut self,
        receiver: &Value,
        calendar: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.withCalendar requires a Temporal receiver".into())
        })?;
        let mut value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.withCalendar requires a Temporal receiver".into())
        })?;
        value.calendar = self.temporal_calendar(calendar)?;
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_getter(
        &mut self,
        receiver: &Value,
        getter: native::TemporalGetter,
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal getter requires a Temporal receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal getter requires a Temporal receiver".into())
        })?;
        match getter {
            native::TemporalGetter::DurationYears
            | native::TemporalGetter::DurationMonths
            | native::TemporalGetter::DurationWeeks
            | native::TemporalGetter::DurationDays
            | native::TemporalGetter::DurationHours
            | native::TemporalGetter::DurationMinutes
            | native::TemporalGetter::DurationSeconds
            | native::TemporalGetter::DurationMilliseconds
            | native::TemporalGetter::DurationMicroseconds
            | native::TemporalGetter::DurationNanoseconds => {
                if value.kind != TemporalKind::Duration {
                    return Err(RuntimeError::TypeError(
                        "Temporal.Duration getter requires a duration receiver".into(),
                    ));
                }
                let duration = value
                    .duration
                    .as_deref()
                    .expect("Temporal.Duration values retain a duration record");
                let field = match getter {
                    native::TemporalGetter::DurationYears => duration.years,
                    native::TemporalGetter::DurationMonths => duration.months,
                    native::TemporalGetter::DurationWeeks => duration.weeks,
                    native::TemporalGetter::DurationDays => duration.days,
                    native::TemporalGetter::DurationHours => duration.hours,
                    native::TemporalGetter::DurationMinutes => duration.minutes,
                    native::TemporalGetter::DurationSeconds => duration.seconds,
                    native::TemporalGetter::DurationMilliseconds => duration.milliseconds,
                    native::TemporalGetter::DurationMicroseconds => duration.microseconds,
                    native::TemporalGetter::DurationNanoseconds => duration.nanoseconds,
                    _ => unreachable!("all Temporal.Duration getters are listed above"),
                };
                Ok(Value::Number(field as f64))
            }
            native::TemporalGetter::CalendarId => Ok(Value::String(value.calendar.into())),
            native::TemporalGetter::EpochMilliseconds
                if matches!(
                    value.kind,
                    TemporalKind::ZonedDateTime | TemporalKind::Instant
                ) =>
            {
                (&value.epoch_nanoseconds / 1_000_000_u32)
                    .to_f64()
                    .map(Value::Number)
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal instant".into()))
            }
            native::TemporalGetter::EpochMilliseconds => Err(RuntimeError::TypeError(
                "Temporal epochMilliseconds requires an Instant or ZonedDateTime receiver".into(),
            )),
            native::TemporalGetter::EpochNanoseconds
                if matches!(
                    value.kind,
                    TemporalKind::ZonedDateTime | TemporalKind::Instant
                ) =>
            {
                Ok(Value::BigInt(value.epoch_nanoseconds))
            }
            native::TemporalGetter::EpochNanoseconds => Err(RuntimeError::TypeError(
                "Temporal epochNanoseconds requires an Instant or ZonedDateTime receiver".into(),
            )),
            native::TemporalGetter::Hour
            | native::TemporalGetter::Minute
            | native::TemporalGetter::Second
            | native::TemporalGetter::Millisecond
            | native::TemporalGetter::Microsecond
            | native::TemporalGetter::Nanosecond => {
                if value.kind != TemporalKind::PlainTime {
                    return Err(RuntimeError::TypeError(
                        "Temporal.PlainTime getter requires a PlainTime receiver".into(),
                    ));
                }
                Ok(Value::Number(match getter {
                    native::TemporalGetter::Hour => value.hour.into(),
                    native::TemporalGetter::Minute => value.minute.into(),
                    native::TemporalGetter::Second => value.second.into(),
                    native::TemporalGetter::Millisecond => value.millisecond.into(),
                    native::TemporalGetter::Microsecond => value.microsecond.into(),
                    native::TemporalGetter::Nanosecond => value.nanosecond.into(),
                    _ => unreachable!("all Temporal.PlainTime getters are listed above"),
                }))
            }
            getter => {
                if !matches!(
                    value.kind,
                    TemporalKind::PlainDate
                        | TemporalKind::PlainDateTime
                        | TemporalKind::PlainMonthDay
                        | TemporalKind::PlainYearMonth
                ) {
                    return Err(RuntimeError::TypeError(
                        "Temporal calendar field requires a plain date receiver".into(),
                    ));
                }
                let fields = self.temporal_calendar_fields(&value)?;
                match getter {
                    native::TemporalGetter::Year
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::PlainYearMonth
                        ) =>
                    {
                        Ok(Value::Number(fields.year.into()))
                    }
                    native::TemporalGetter::Month if value.kind != TemporalKind::PlainTime => {
                        Ok(Value::Number(fields.month.into()))
                    }
                    native::TemporalGetter::MonthCode if value.kind != TemporalKind::PlainTime => {
                        Ok(Value::String(fields.month_code.into()))
                    }
                    native::TemporalGetter::Day
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::PlainMonthDay
                        ) =>
                    {
                        Ok(Value::Number(fields.day.into()))
                    }
                    native::TemporalGetter::Era
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::PlainYearMonth
                        ) =>
                    {
                        Ok(fields
                            .era
                            .map_or(Value::Undefined, |era| Value::String(era.into())))
                    }
                    native::TemporalGetter::EraYear
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::PlainYearMonth
                        ) =>
                    {
                        Ok(fields
                            .era_year
                            .map_or(Value::Undefined, |year| Value::Number(year.into())))
                    }
                    native::TemporalGetter::MonthsInYear
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::PlainYearMonth
                        ) =>
                    {
                        Ok(Value::Number(fields.months_in_year.into()))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "Temporal calendar field is unavailable on this receiver".into(),
                    )),
                }
            }
        }
    }

    pub(super) fn temporal_plain_to_zoned_date_time(
        &mut self,
        receiver: &Value,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.toZonedDateTime requires a plain receiver".into())
        })?;
        let mut value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.toZonedDateTime requires a plain receiver".into())
        })?;
        if !matches!(
            value.kind,
            TemporalKind::PlainDate | TemporalKind::PlainDateTime
        ) {
            return Err(RuntimeError::TypeError(
                "Temporal.toZonedDateTime requires a plain receiver".into(),
            ));
        }
        let time_zone = self
            .coerce_string(time_zone)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
        // DateTimeFormat's current Temporal bridge supplies a fully pinned
        // IANA implementation, but Temporal's compact object slice has not
        // yet exposed its disambiguation options. UTC has no ambiguity and is
        // the portable conversion required to carry ISO fields into an epoch
        // value; reject other zones rather than silently applying UTC.
        if time_zone != "UTC" {
            return Err(RuntimeError::RangeError(
                "Temporal.toZonedDateTime currently supports UTC".into(),
            ));
        }
        value.epoch_nanoseconds = epoch::nanoseconds_since_epoch(
            (value.year, value.month, value.day),
            (
                value.hour,
                value.minute,
                value.second,
                value.millisecond,
                value.microsecond,
                value.nanosecond,
            ),
            0,
        );
        value.kind = TemporalKind::ZonedDateTime;
        value.time_zone = time_zone;
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_zoned_date_time_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.ZonedDateTime method requires a receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.ZonedDateTime method requires a receiver".into())
        })?;
        if value.kind != TemporalKind::ZonedDateTime {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime method requires a receiver".into(),
            ));
        }
        let milliseconds = (&value.epoch_nanoseconds / 1_000_000_u32)
            .to_f64()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal instant".into()))?;
        let stack_base = self.stack.len();
        let result = (|| {
            let options_prototype = if native::argument(args, 1) == &Value::Undefined {
                None
            } else {
                Some(self.coerce_object(native::argument(args, 1))?)
            };
            let options = self.with_roots(|heap| heap.alloc_object(options_prototype))?;
            self.stack.push(Value::Object(options));
            self.define_data(
                options,
                "timeZone",
                Value::String(value.time_zone.into()),
                true,
                true,
                true,
            )?;
            let formatter = self.create_date_time_format(
                &Value::Undefined,
                &[native::argument(args, 0).clone(), Value::Object(options)],
                false,
            )?;
            self.stack.push(formatter.clone());
            self.date_time_format_format(&formatter, &Value::Number(milliseconds))
        })();
        self.stack.truncate(stack_base);
        result
    }

    // ---- Stage 1 Track C: Temporal.Instant arithmetic -------------------

    /// `GetOptionsObject`: `undefined` becomes an empty object; any other
    /// non-object value is a `TypeError` — it is *not* boxed into a wrapper
    /// object the way an ordinary `ToObject` would (Test262's
    /// `PlainTime/prototype/until/options-wrong-type.js` and its siblings
    /// pass `"hello"`/`1`/`1n` and require the throw).
    fn temporal_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let object = if *value == Value::Undefined {
            self.with_roots(|heap| heap.alloc_object(None))?
        } else if matches!(value, Value::Object(_)) {
            self.coerce_object(value)?
        } else {
            return Err(RuntimeError::TypeError(
                "Temporal options must be an object".into(),
            ));
        };
        let result = Value::Object(object);
        self.stack.push(result.clone());
        Ok(result)
    }

    fn temporal_string_option(
        &mut self,
        options: &Value,
        name: &str,
        allowed: &[&str],
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if !allowed.is_empty() && !allowed.contains(&string.as_str()) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(string))
    }

    /// `ToTemporalRoundingIncrement`: an integer in `1..=1e9`, default `1`.
    fn temporal_rounding_increment(&mut self, options: &Value) -> Result<i128, RuntimeError> {
        let value = self.get_property(options, &"roundingIncrement".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        let integer = value.trunc();
        if !(1.0..=1_000_000_000.0).contains(&integer) {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        Ok(integer as i128)
    }

    fn temporal_rounding_mode(
        &mut self,
        options: &Value,
        default: blueice_ecma402::NumberRoundingMode,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match self.temporal_string_option(
            options,
            "roundingMode",
            &[
                "ceil",
                "floor",
                "expand",
                "trunc",
                "halfCeil",
                "halfFloor",
                "halfExpand",
                "halfTrunc",
                "halfEven",
            ],
        )? {
            None => Ok(default),
            Some(mode) => Ok(rounding::parse_rounding_mode(&mode)
                .expect("temporal_string_option already validated the rounding mode name")),
        }
    }

    fn temporal_time_unit_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<rounding::TimeUnit>, RuntimeError> {
        match self.temporal_string_option(
            options,
            name,
            &[
                "hour",
                "hours",
                "minute",
                "minutes",
                "second",
                "seconds",
                "millisecond",
                "milliseconds",
                "microsecond",
                "microseconds",
                "nanosecond",
                "nanoseconds",
            ],
        )? {
            None => Ok(None),
            Some(unit) => Ok(Some(
                rounding::parse_time_unit(&unit)
                    .expect("temporal_string_option already validated the unit name"),
            )),
        }
    }

    /// `ToTemporalDuration`: a `Temporal.Duration` receiver is used
    /// directly; a string is parsed via the ISO duration grammar; anything
    /// else is read as a property bag of (all optional, integer) fields.
    fn temporal_duration_from_value(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::Duration {
                    return Ok(*temporal
                        .duration
                        .as_deref()
                        .expect("Temporal.Duration values retain a duration record"));
                }
            }
        }
        if matches!(value, Value::String(_)) {
            let source = self
                .coerce_string(value)?
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError("invalid Temporal.Duration string".into()))?;
            return iso::parse_duration_record(&source).ok_or_else(|| {
                RuntimeError::RangeError("invalid Temporal.Duration string".into())
            });
        }
        if !matches!(value, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration-like value must be an object".into(),
            ));
        }
        let mut values = [0_i128; 10];
        let mut has_field = false;
        // Alphabetical read order, which is observable: Test262's
        // `PlainTime/prototype/add/order-of-operations.js` asserts each
        // getter/`valueOf` fires in exactly this sequence.
        for (index, name) in [
            (3, "days"),
            (4, "hours"),
            (8, "microseconds"),
            (7, "milliseconds"),
            (5, "minutes"),
            (1, "months"),
            (9, "nanoseconds"),
            (6, "seconds"),
            (2, "weeks"),
            (0, "years"),
        ] {
            let field = self.get_property(value, &name.into())?;
            if field != Value::Undefined {
                has_field = true;
            }
            values[index] = self.temporal_duration_integer(&field, name)?;
        }
        if !has_field {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration-like value has no fields".into(),
            ));
        }
        blueice_ecma402::DurationRecord::try_new(
            values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
            values[8], values[9],
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    /// Reads a validated `Temporal.Instant` receiver's epoch nanoseconds.
    fn temporal_instant_epoch(&mut self, receiver: &Value) -> Result<BigInt, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Instant method requires an Instant receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Instant method requires an Instant receiver".into())
        })?;
        if value.kind != TemporalKind::Instant {
            return Err(RuntimeError::TypeError(
                "Temporal.Instant method requires an Instant receiver".into(),
            ));
        }
        Ok(value.epoch_nanoseconds)
    }

    /// `ToTemporalInstant`: an `Instant` receiver's epoch nanoseconds are
    /// used directly; anything else is coerced to a string and parsed.
    fn temporal_to_instant_epoch(&mut self, value: &Value) -> Result<BigInt, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::Instant {
                    return Ok(temporal.epoch_nanoseconds);
                }
            }
        }
        let source = self
            .coerce_string(value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal.Instant string".into()))?;
        self.temporal_value_from_string(TemporalKind::Instant, &source)
            .map(|temporal| temporal.epoch_nanoseconds)
    }

    fn instant_from_epoch_nanoseconds(
        &mut self,
        epoch_nanoseconds: BigInt,
    ) -> Result<Value, RuntimeError> {
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.Instant epoch nanoseconds are outside the supported range".into(),
            ));
        }
        self.alloc_temporal_value(
            TemporalValue {
                kind: TemporalKind::Instant,
                duration: None,
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
                microsecond: 0,
                nanosecond: 0,
                epoch_nanoseconds,
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            },
            false,
        )
    }

    pub(super) fn temporal_instant_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let epoch = self.temporal_instant_epoch(receiver)?;
        let duration = self.temporal_duration_from_value(duration_value)?;
        if duration.years != 0 || duration.months != 0 || duration.weeks != 0 || duration.days != 0
        {
            return Err(RuntimeError::RangeError(
                "Temporal.Instant arithmetic does not accept calendar-unit duration fields".into(),
            ));
        }
        let time = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        );
        let time = if negate { time.negated() } else { time };
        let epoch_i128: i128 = epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let result_i128 = epoch_i128 + time.total_nanoseconds();
        self.instant_from_epoch_nanoseconds(BigInt::from(result_i128))
    }

    pub(super) fn temporal_instant_round(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let epoch = self.temporal_instant_epoch(receiver)?;
        let options = self.temporal_options(options)?;
        let smallest_unit = self
            .temporal_time_unit_option(&options, "smallestUnit")?
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal.Instant.round requires smallestUnit".into())
            })?;
        let increment = self.temporal_rounding_increment(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::HalfExpand)?;
        let day_nanoseconds = 86_400_000_000_000_i128;
        let step = smallest_unit.nanoseconds() * increment;
        if day_nanoseconds % step != 0 {
            return Err(RuntimeError::RangeError(
                "roundingIncrement does not divide evenly into a day".into(),
            ));
        }
        let epoch_i128: i128 = epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let rounded = duration_math::TimeDuration::from_nanoseconds(epoch_i128)
            .round(smallest_unit, increment, mode)
            .total_nanoseconds();
        self.instant_from_epoch_nanoseconds(BigInt::from(rounded))
    }

    pub(super) fn temporal_instant_difference(
        &mut self,
        receiver: &Value,
        other: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let self_epoch = self.temporal_instant_epoch(receiver)?;
        let other_epoch = self.temporal_to_instant_epoch(other)?;
        let options = self.temporal_options(options)?;
        let smallest_unit = self
            .temporal_time_unit_option(&options, "smallestUnit")?
            .unwrap_or(rounding::TimeUnit::Nanosecond);
        let largest_unit = self
            .temporal_time_unit_option(&options, "largestUnit")?
            .unwrap_or(rounding::TimeUnit::Second);
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        let increment = self.temporal_rounding_increment(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
        // Unlike `.round()`, `.until()`/`.since()` have no fixture in the
        // pinned Test262 corpus requiring the increment to divide evenly
        // into a day — only the general 1..=1e9 range above applies.
        let self_i128: i128 = self_epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let other_i128: i128 = other_epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let difference_ns = if since {
            self_i128 - other_i128
        } else {
            other_i128 - self_i128
        };
        let rounded = duration_math::TimeDuration::from_nanoseconds(difference_ns).round(
            smallest_unit,
            increment,
            mode,
        );
        let [hours, minutes, seconds, milliseconds, microseconds, nanoseconds] =
            rounded.balance_to(largest_unit);
        let record = blueice_ecma402::DurationRecord::try_new(
            0,
            0,
            0,
            0,
            i128::from(hours),
            i128::from(minutes),
            i128::from(seconds),
            i128::from(milliseconds),
            i128::from(microseconds),
            i128::from(nanoseconds),
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.alloc_temporal_value(
            TemporalValue {
                kind: TemporalKind::Duration,
                duration: Some(Box::new(record)),
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
                microsecond: 0,
                nanosecond: 0,
                epoch_nanoseconds: 0.into(),
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            },
            false,
        )
    }

    pub(super) fn temporal_instant_equals(
        &mut self,
        receiver: &Value,
        other: &Value,
    ) -> Result<Value, RuntimeError> {
        let self_epoch = self.temporal_instant_epoch(receiver)?;
        let other_epoch = self.temporal_to_instant_epoch(other)?;
        Ok(Value::Bool(self_epoch == other_epoch))
    }

    pub(super) fn temporal_instant_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_to_instant_epoch(one)?;
        let two = self.temporal_to_instant_epoch(two)?;
        Ok(Value::Number(match one.cmp(&two) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    fn format_instant_string(epoch_nanoseconds: &BigInt, fractional_digits: Option<u8>) -> String {
        let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
            epoch::instant_fields(epoch_nanoseconds);
        let mut result = if (0..=9999).contains(&year) {
            format!("{year:04}")
        } else {
            format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
        };
        result.push_str(&format!(
            "-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"
        ));
        let nanos_total = u32::from(millisecond) * 1_000_000
            + u32::from(microsecond) * 1_000
            + u32::from(nanosecond);
        match fractional_digits {
            Some(0) => {}
            Some(digits) => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(&text[..digits as usize]);
            }
            None if nanos_total != 0 => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(text.trim_end_matches('0'));
            }
            None => {}
        }
        result.push('Z');
        result
    }

    pub(super) fn temporal_instant_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let epoch = self.temporal_instant_epoch(receiver)?;
        let options = self.temporal_options(options)?;
        let fractional_digits_value =
            self.get_property(&options, &"fractionalSecondDigits".into())?;
        let explicit_digits = match &fractional_digits_value {
            Value::Undefined => None,
            Value::String(text) => {
                let text = text.to_utf8().map_err(|_| {
                    RuntimeError::RangeError("invalid fractionalSecondDigits".into())
                })?;
                if text == "auto" {
                    None
                } else {
                    return Err(RuntimeError::RangeError(
                        "invalid fractionalSecondDigits".into(),
                    ));
                }
            }
            _ => {
                let digits = self.coerce_number(&fractional_digits_value)?;
                if !digits.is_finite() || digits.fract() != 0.0 || !(0.0..=9.0).contains(&digits) {
                    return Err(RuntimeError::RangeError(
                        "invalid fractionalSecondDigits".into(),
                    ));
                }
                Some(digits as u8)
            }
        };
        let smallest_unit = self.temporal_time_unit_option(&options, "smallestUnit")?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
        let effective_unit = smallest_unit.or(explicit_digits.map(|digits| match digits {
            0 => rounding::TimeUnit::Second,
            1..=3 => rounding::TimeUnit::Millisecond,
            4..=6 => rounding::TimeUnit::Microsecond,
            _ => rounding::TimeUnit::Nanosecond,
        }));
        let display_digits = smallest_unit
            .map(|unit| match unit {
                rounding::TimeUnit::Hour
                | rounding::TimeUnit::Minute
                | rounding::TimeUnit::Second => 0,
                rounding::TimeUnit::Millisecond => 3,
                rounding::TimeUnit::Microsecond => 6,
                rounding::TimeUnit::Nanosecond => 9,
            })
            .or(explicit_digits);
        let epoch_i128: i128 = epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let rounded = match effective_unit {
            Some(unit) => duration_math::TimeDuration::from_nanoseconds(epoch_i128)
                .round(unit, 1, mode)
                .total_nanoseconds(),
            None => epoch_i128,
        };
        Ok(Value::String(
            Self::format_instant_string(&BigInt::from(rounded), display_digits).into(),
        ))
    }

    pub(super) fn temporal_instant_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.Instant cannot be converted to a primitive value".into(),
        ))
    }

    pub(super) fn temporal_from_epoch_milliseconds(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let milliseconds = self.coerce_number(value)?;
        if !milliseconds.is_finite() || milliseconds.fract() != 0.0 {
            return Err(RuntimeError::RangeError(
                "invalid Temporal.Instant epoch milliseconds".into(),
            ));
        }
        let nanoseconds = BigInt::from(milliseconds as i64) * 1_000_000_u32;
        self.instant_from_epoch_nanoseconds(nanoseconds)
    }

    pub(super) fn temporal_from_epoch_nanoseconds(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let nanoseconds = match value {
            Value::BigInt(value) => value.clone(),
            _ => {
                return Err(RuntimeError::TypeError(
                    "Temporal.Instant.fromEpochNanoseconds requires a BigInt".into(),
                ));
            }
        };
        self.instant_from_epoch_nanoseconds(nanoseconds)
    }

    // ---- Stage 1 Track D: Temporal.PlainTime arithmetic -----------------

    /// Reads `options` values without validating them, so every property a
    /// method consumes is fetched and coerced *before* any of them is
    /// range-checked. Temporal requires exactly that ordering — Test262's
    /// `PlainTime/prototype/round/options-read-before-algorithmic-validation.js`
    /// reads `smallestUnit` (and throws on the increment) only after
    /// `roundingIncrement`/`roundingMode` have already been read — so the
    /// combined read-and-validate `temporal_string_option` cannot be used
    /// where more than one option participates in a joint check.
    fn temporal_raw_string_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        string
            .to_utf8()
            .map(Some)
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))
    }

    fn temporal_raw_number_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<f64>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        self.coerce_number(&value).map(Some)
    }

    /// `ToTemporalRoundingIncrement` applied to an already-read raw value.
    fn temporal_validated_rounding_increment(increment: Option<f64>) -> Result<i128, RuntimeError> {
        let Some(increment) = increment else {
            return Ok(1);
        };
        if !increment.is_finite() {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        let integer = increment.trunc();
        if !(1.0..=1_000_000_000.0).contains(&integer) {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        Ok(integer as i128)
    }

    fn temporal_validated_rounding_mode(
        mode: Option<&str>,
        default: blueice_ecma402::NumberRoundingMode,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match mode {
            None => Ok(default),
            Some(mode) => rounding::parse_rounding_mode(mode)
                .ok_or_else(|| RuntimeError::RangeError("invalid roundingMode option".into())),
        }
    }

    fn temporal_validated_time_unit(
        unit: Option<&str>,
        name: &str,
    ) -> Result<Option<rounding::TimeUnit>, RuntimeError> {
        match unit {
            None => Ok(None),
            Some(unit) => rounding::parse_time_unit(unit)
                .map(Some)
                .ok_or_else(|| RuntimeError::RangeError(format!("invalid {name} option"))),
        }
    }

    /// `ValidateTemporalRoundingIncrement(increment, maximum, false)` for a
    /// time-only type.
    ///
    /// This is deliberately *not* `Temporal.Instant.round`'s rule. An
    /// `Instant` is unbounded, so its increment only has to divide a whole
    /// day (`maximum` inclusive); a `PlainTime` is already bounded to one
    /// day, so its increment must divide the *unit's own* place value and
    /// stay strictly below it — `{ smallestUnit: "hours", roundingIncrement:
    /// 24 }` and `{ smallestUnit: "nanoseconds", roundingIncrement: 1000 }`
    /// both throw, per Test262's
    /// `PlainTime/prototype/round/roundingincrement-invalid.js`.
    fn temporal_validated_plain_time_increment(
        increment: i128,
        unit: rounding::TimeUnit,
    ) -> Result<(), RuntimeError> {
        let maximum = match unit {
            rounding::TimeUnit::Hour => 24,
            rounding::TimeUnit::Minute | rounding::TimeUnit::Second => 60,
            rounding::TimeUnit::Millisecond
            | rounding::TimeUnit::Microsecond
            | rounding::TimeUnit::Nanosecond => 1_000,
        };
        if increment >= maximum || maximum % increment != 0 {
            return Err(RuntimeError::RangeError(
                "roundingIncrement does not divide evenly into the smallestUnit".into(),
            ));
        }
        Ok(())
    }

    /// `GetTemporalOverflowOption`: `true` means `"reject"`.
    fn temporal_overflow_option(&mut self, options: &Value) -> Result<bool, RuntimeError> {
        Ok(self
            .temporal_string_option(options, "overflow", &["constrain", "reject"])?
            .as_deref()
            == Some("reject"))
    }

    /// `ToIntegerWithTruncation`: a finite number, truncated toward zero.
    /// Temporal's *time* fields use this rather than requiring an already
    /// integral value — `new Temporal.PlainTime(11.9)` is hour 11, per
    /// Test262's `PlainTime/argument-convert.js`.
    fn temporal_truncated_integer(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<i64, RuntimeError> {
        let number = self.coerce_number(value)?;
        if !number.is_finite() {
            return Err(RuntimeError::RangeError(format!("invalid Temporal {name}")));
        }
        Ok(number.trunc() as i64)
    }

    fn temporal_optional_truncated_integer(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<i64, RuntimeError> {
        if *value == Value::Undefined {
            Ok(0)
        } else {
            self.temporal_truncated_integer(value, name)
        }
    }

    /// `RegulateTime`: clamp each field into range (`"constrain"`), or reject
    /// an out-of-range one outright (`"reject"`).
    fn temporal_regulate_time(
        fields: [i64; 6],
        reject: bool,
    ) -> Result<(u8, u8, u8, u16, u16, u16), RuntimeError> {
        const MAXIMUM: [i64; 6] = [23, 59, 59, 999, 999, 999];
        let mut regulated = [0_i64; 6];
        for (index, field) in fields.into_iter().enumerate() {
            if reject && !(0..=MAXIMUM[index]).contains(&field) {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainTime field is out of range".into(),
                ));
            }
            regulated[index] = field.clamp(0, MAXIMUM[index]);
        }
        Ok((
            regulated[0] as u8,
            regulated[1] as u8,
            regulated[2] as u8,
            regulated[3] as u16,
            regulated[4] as u16,
            regulated[5] as u16,
        ))
    }

    /// `ToTemporalTimeRecord`, in the spec's alphabetical read order
    /// (`hour`, `microsecond`, `millisecond`, `minute`, `nanosecond`,
    /// `second` — Test262's `PlainTime/prototype/with/order-of-operations.js`
    /// asserts exactly that sequence). Returns each field's value where it
    /// was present, so `with` can fall back to its receiver and `from` can
    /// fall back to zero. A bag with none of the six throws.
    fn temporal_time_record(&mut self, bag: &Value) -> Result<[Option<i64>; 6], RuntimeError> {
        let mut fields = [None; 6];
        let mut present = false;
        for (index, name) in [
            (0, "hour"),
            (4, "microsecond"),
            (3, "millisecond"),
            (1, "minute"),
            (5, "nanosecond"),
            (2, "second"),
        ] {
            let value = self.get_property(bag, &name.into())?;
            if value != Value::Undefined {
                present = true;
                fields[index] = Some(self.temporal_truncated_integer(&value, name)?);
            }
        }
        if !present {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime-like value has no time fields".into(),
            ));
        }
        Ok(fields)
    }

    fn plain_time_value(fields: (u8, u8, u8, u16, u16, u16)) -> TemporalValue {
        TemporalValue {
            kind: TemporalKind::PlainTime,
            duration: None,
            year: 1970,
            month: 1,
            day: 1,
            hour: fields.0,
            minute: fields.1,
            second: fields.2,
            millisecond: fields.3,
            microsecond: fields.4,
            nanosecond: fields.5,
            epoch_nanoseconds: 0.into(),
            calendar: "iso8601".into(),
            time_zone: "UTC".into(),
        }
    }

    /// Reads a validated `Temporal.PlainTime` receiver's time of day.
    fn temporal_plain_time_fields(
        &mut self,
        receiver: &Value,
    ) -> Result<(u8, u8, u8, u16, u16, u16), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainTime method requires a PlainTime receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainTime method requires a PlainTime receiver".into(),
            )
        })?;
        if value.kind != TemporalKind::PlainTime {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime method requires a PlainTime receiver".into(),
            ));
        }
        Ok((
            value.hour,
            value.minute,
            value.second,
            value.millisecond,
            value.microsecond,
            value.nanosecond,
        ))
    }

    /// `ToTemporalTime`: a `PlainTime`/`PlainDateTime`/`ZonedDateTime` carries
    /// its own time of day; a string goes through the `TemporalTimeString`
    /// grammar; anything else object-shaped is read as a property bag.
    pub(super) fn temporal_to_plain_time(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<(u8, u8, u8, u16, u16, u16), RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let carried = match temporal.kind {
                    TemporalKind::PlainTime | TemporalKind::PlainDateTime => Some((
                        temporal.hour,
                        temporal.minute,
                        temporal.second,
                        temporal.millisecond,
                        temporal.microsecond,
                        temporal.nanosecond,
                    )),
                    // A `ZonedDateTime`'s wall-clock time of day is its instant
                    // shifted by the zone's offset. `UTC` and a fixed numeric
                    // offset are resolvable here; a named IANA zone needs the
                    // transition-rule lookup that is Phase 26 Track E's scope.
                    TemporalKind::ZonedDateTime => {
                        let offset = if temporal.time_zone == "UTC" {
                            0
                        } else {
                            iso::parse_offset_seconds(&temporal.time_zone).ok_or_else(|| {
                                RuntimeError::RangeError(
                                    "Temporal.PlainTime conversion supports UTC and fixed offsets"
                                        .into(),
                                )
                            })?
                        };
                        let local = &temporal.epoch_nanoseconds
                            + BigInt::from(i64::from(offset) * 1_000_000_000);
                        Some(epoch::instant_fields(&local).1)
                    }
                    // Every other Temporal type lacks the singular time
                    // fields, so the property-bag path below throws for it
                    // exactly as the spec requires.
                    _ => None,
                };
                if let Some(carried) = carried {
                    // The options object is still read and its `overflow`
                    // value still validated, even though nothing is regulated.
                    let options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&options)?;
                    return Ok(carried);
                }
            }
            let fields = self.temporal_time_record(value)?;
            let options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&options)?;
            return Self::temporal_regulate_time(fields.map(Option::unwrap_or_default), reject);
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime-like value must be an object or a string".into(),
            ));
        }
        let source = self
            .coerce_string(value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal.PlainTime string".into()))?;
        let fields = iso::parse_plain_time(&source)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.PlainTime string".into()))?;
        let options = self.temporal_options(options)?;
        self.temporal_overflow_option(&options)?;
        Ok(fields)
    }

    fn plain_time_from_nanoseconds(&mut self, nanoseconds: i128) -> Result<Value, RuntimeError> {
        let fields = duration_math::time_fields_from_nanoseconds(nanoseconds);
        self.alloc_temporal_value(Self::plain_time_value(fields), false)
    }

    /// `AddDurationToTime`. Years/months/weeks *and days* are read (and
    /// range-validated) but contribute nothing: a `PlainTime` has no date to
    /// carry them into, so the spec's `ToInternalDurationRecord` leaves them
    /// in the date part that `AddTime` never looks at. Test262's
    /// `PlainTime/prototype/add/argument-higher-units.js` pins this —
    /// `plainTime.add({ days: 1 })` is the *same* time, not 24 hours later,
    /// and unlike `Temporal.Instant.prototype.add` it is not an error either.
    pub(super) fn temporal_plain_time_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let duration = self.temporal_duration_from_value(duration_value)?;
        let time = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        );
        let time = if negate { time.negated() } else { time };
        let total = duration_math::time_fields_to_nanoseconds(
            fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
        ) + time.total_nanoseconds();
        self.plain_time_from_nanoseconds(total)
    }

    /// `Temporal.PlainTime.prototype.round`. Unlike `Instant.round`, `roundTo`
    /// is required, and a bare string is shorthand for `{ smallestUnit }` —
    /// via a *null-prototype* options object, so a polluted
    /// `Object.prototype.roundingMode` is never observed (Test262's
    /// `round/string-shorthand-no-object-prototype-pollution.js`).
    pub(super) fn temporal_plain_time_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        if *round_to == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime.round requires a smallestUnit or options argument".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            let options = if let Value::String(unit) = round_to {
                let options = self.with_roots(|heap| heap.alloc_object(None))?;
                self.stack.push(Value::Object(options));
                self.define_data(
                    options,
                    "smallestUnit",
                    Value::String(unit.clone()),
                    true,
                    true,
                    true,
                )?;
                Value::Object(options)
            } else {
                self.temporal_options(round_to)?
            };
            let increment = self.temporal_raw_number_option(&options, "roundingIncrement")?;
            let mode = self.temporal_raw_string_option(&options, "roundingMode")?;
            let smallest_unit = self.temporal_raw_string_option(&options, "smallestUnit")?;
            let increment = Self::temporal_validated_rounding_increment(increment)?;
            let mode = Self::temporal_validated_rounding_mode(
                mode.as_deref(),
                blueice_ecma402::NumberRoundingMode::HalfExpand,
            )?;
            let smallest_unit =
                Self::temporal_validated_time_unit(smallest_unit.as_deref(), "smallestUnit")?
                    .ok_or_else(|| {
                        RuntimeError::RangeError(
                            "Temporal.PlainTime.round requires smallestUnit".into(),
                        )
                    })?;
            Self::temporal_validated_plain_time_increment(increment, smallest_unit)?;
            let total = duration_math::time_fields_to_nanoseconds(
                fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
            );
            let rounded = duration_math::TimeDuration::from_nanoseconds(total)
                .round(smallest_unit, increment, mode)
                .total_nanoseconds();
            self.plain_time_from_nanoseconds(rounded)
        })();
        self.stack.truncate(base);
        result
    }

    /// `DifferenceTemporalPlainTime`.
    ///
    /// The `since` direction needs no rounding-mode negation: the spec
    /// negates the mode, differences in the opposite order, then negates the
    /// result, and those three cancel to "difference in this order, mode as
    /// given" — the same equivalence Track C confirmed for `Instant` against
    /// `since/roundingmode-ceil.js`, and which `PlainTime`'s own
    /// `since/roundingmode-*.js` fixtures agree with.
    pub(super) fn temporal_plain_time_difference(
        &mut self,
        receiver: &Value,
        other: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let other_fields = self.temporal_to_plain_time(other, &Value::Undefined)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_options(options)?;
            let largest_unit = self.temporal_raw_string_option(&options, "largestUnit")?;
            let increment = self.temporal_raw_number_option(&options, "roundingIncrement")?;
            let mode = self.temporal_raw_string_option(&options, "roundingMode")?;
            let smallest_unit = self.temporal_raw_string_option(&options, "smallestUnit")?;
            let largest_unit = match largest_unit.as_deref() {
                None | Some("auto") => rounding::TimeUnit::Hour,
                unit => Self::temporal_validated_time_unit(unit, "largestUnit")?
                    .expect("a present largestUnit resolves to a unit"),
            };
            let increment = Self::temporal_validated_rounding_increment(increment)?;
            let mode = Self::temporal_validated_rounding_mode(
                mode.as_deref(),
                blueice_ecma402::NumberRoundingMode::Trunc,
            )?;
            let smallest_unit =
                Self::temporal_validated_time_unit(smallest_unit.as_deref(), "smallestUnit")?
                    .unwrap_or(rounding::TimeUnit::Nanosecond);
            if smallest_unit > largest_unit {
                return Err(RuntimeError::RangeError(
                    "smallestUnit must not be larger than largestUnit".into(),
                ));
            }
            Self::temporal_validated_plain_time_increment(increment, smallest_unit)?;
            let self_total = duration_math::time_fields_to_nanoseconds(
                fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
            );
            let other_total = duration_math::time_fields_to_nanoseconds(
                other_fields.0,
                other_fields.1,
                other_fields.2,
                other_fields.3,
                other_fields.4,
                other_fields.5,
            );
            let difference = if since {
                self_total - other_total
            } else {
                other_total - self_total
            };
            let balanced = duration_math::TimeDuration::from_nanoseconds(difference)
                .round(smallest_unit, increment, mode)
                .balance_to(largest_unit);
            self.alloc_time_duration(balanced)
        })();
        self.stack.truncate(base);
        result
    }

    fn alloc_time_duration(&mut self, fields: [i64; 6]) -> Result<Value, RuntimeError> {
        let record = blueice_ecma402::DurationRecord::try_new(
            0,
            0,
            0,
            0,
            i128::from(fields[0]),
            i128::from(fields[1]),
            i128::from(fields[2]),
            i128::from(fields[3]),
            i128::from(fields[4]),
            i128::from(fields[5]),
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let mut value = Self::plain_time_value((0, 0, 0, 0, 0, 0));
        value.kind = TemporalKind::Duration;
        value.duration = Some(Box::new(record));
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_plain_time_equals(
        &mut self,
        receiver: &Value,
        other: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let other = self.temporal_to_plain_time(other, &Value::Undefined)?;
        Ok(Value::Bool(fields == other))
    }

    pub(super) fn temporal_plain_time_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_to_plain_time(one, &Value::Undefined)?;
        let two = self.temporal_to_plain_time(two, &Value::Undefined)?;
        Ok(Value::Number(match one.cmp(&two) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    /// `Temporal.PlainTime.prototype.with`. A property bag only: any Temporal
    /// value (including another `PlainTime`), a `calendar` property or a
    /// `timeZone` property is a `TypeError`, per Test262's
    /// `with/plaintimelike-invalid.js`.
    pub(super) fn temporal_plain_time_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let object = like.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.PlainTime.with requires a property bag".into())
        })?;
        if self.heap.temporal_value(object)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime.with does not accept a Temporal value".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            for name in ["calendar", "timeZone"] {
                if self.get_property(like, &name.into())? != Value::Undefined {
                    return Err(RuntimeError::TypeError(format!(
                        "Temporal.PlainTime.with does not accept a {name} property"
                    )));
                }
            }
            let partial = self.temporal_time_record(like)?;
            let options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&options)?;
            let current = [
                i64::from(fields.0),
                i64::from(fields.1),
                i64::from(fields.2),
                i64::from(fields.3),
                i64::from(fields.4),
                i64::from(fields.5),
            ];
            let merged = std::array::from_fn(|index| partial[index].unwrap_or(current[index]));
            let regulated = Self::temporal_regulate_time(merged, reject)?;
            self.alloc_temporal_value(Self::plain_time_value(regulated), false)
        })();
        self.stack.truncate(base);
        result
    }

    fn format_plain_time_string(
        fields: (u8, u8, u8, u16, u16, u16),
        precision: PlainTimePrecision,
    ) -> String {
        let (hour, minute, second, millisecond, microsecond, nanosecond) = fields;
        let mut result = format!("{hour:02}:{minute:02}");
        let digits = match precision {
            PlainTimePrecision::Minute => return result,
            PlainTimePrecision::Seconds(digits) => digits,
        };
        result.push_str(&format!(":{second:02}"));
        let nanos_total = u32::from(millisecond) * 1_000_000
            + u32::from(microsecond) * 1_000
            + u32::from(nanosecond);
        match digits {
            Some(0) => {}
            Some(digits) => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(&text[..digits as usize]);
            }
            None if nanos_total != 0 => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(text.trim_end_matches('0'));
            }
            None => {}
        }
        result
    }

    pub(super) fn temporal_plain_time_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_options(options)?;
            // Alphabetical read order, per `toString/order-of-operations.js`.
            let digits_value = self.get_property(&options, &"fractionalSecondDigits".into())?;
            let digits = match &digits_value {
                Value::Undefined => None,
                Value::Number(number) => {
                    if !number.is_finite() {
                        return Err(RuntimeError::RangeError(
                            "invalid fractionalSecondDigits option".into(),
                        ));
                    }
                    let count = number.floor();
                    if !(0.0..=9.0).contains(&count) {
                        return Err(RuntimeError::RangeError(
                            "invalid fractionalSecondDigits option".into(),
                        ));
                    }
                    Some(count as u8)
                }
                // `GetTemporalFractionalSecondDigitsOption` only accepts a
                // non-Number if it stringifies to exactly "auto".
                other => {
                    let text = self.coerce_string(other)?.to_utf8().map_err(|_| {
                        RuntimeError::RangeError("invalid fractionalSecondDigits option".into())
                    })?;
                    if text != "auto" {
                        return Err(RuntimeError::RangeError(
                            "invalid fractionalSecondDigits option".into(),
                        ));
                    }
                    None
                }
            };
            let mode = self.temporal_raw_string_option(&options, "roundingMode")?;
            let mode = Self::temporal_validated_rounding_mode(
                mode.as_deref(),
                blueice_ecma402::NumberRoundingMode::Trunc,
            )?;
            // `hour`/`hours` is a valid rounding unit but not a valid
            // serialization unit — `smallestunit-invalid-string.js` lists it
            // among the rejected values.
            let smallest_unit = self.temporal_string_option(
                &options,
                "smallestUnit",
                &[
                    "minute",
                    "minutes",
                    "second",
                    "seconds",
                    "millisecond",
                    "milliseconds",
                    "microsecond",
                    "microseconds",
                    "nanosecond",
                    "nanoseconds",
                ],
            )?;
            let smallest_unit =
                Self::temporal_validated_time_unit(smallest_unit.as_deref(), "smallestUnit")?;
            // `ToSecondsStringPrecisionRecord`: an explicit `smallestUnit`
            // always wins over `fractionalSecondDigits`.
            let (unit, increment, precision) = match smallest_unit {
                Some(rounding::TimeUnit::Minute) => {
                    (rounding::TimeUnit::Minute, 1, PlainTimePrecision::Minute)
                }
                Some(rounding::TimeUnit::Second) => (
                    rounding::TimeUnit::Second,
                    1,
                    PlainTimePrecision::Seconds(Some(0)),
                ),
                Some(rounding::TimeUnit::Millisecond) => (
                    rounding::TimeUnit::Millisecond,
                    1,
                    PlainTimePrecision::Seconds(Some(3)),
                ),
                Some(rounding::TimeUnit::Microsecond) => (
                    rounding::TimeUnit::Microsecond,
                    1,
                    PlainTimePrecision::Seconds(Some(6)),
                ),
                Some(rounding::TimeUnit::Nanosecond) => (
                    rounding::TimeUnit::Nanosecond,
                    1,
                    PlainTimePrecision::Seconds(Some(9)),
                ),
                Some(rounding::TimeUnit::Hour) => unreachable!(
                    "the smallestUnit option list above excludes hour for serialization"
                ),
                None => match digits {
                    None => (
                        rounding::TimeUnit::Nanosecond,
                        1,
                        PlainTimePrecision::Seconds(None),
                    ),
                    Some(0) => (
                        rounding::TimeUnit::Second,
                        1,
                        PlainTimePrecision::Seconds(Some(0)),
                    ),
                    Some(count @ 1..=3) => (
                        rounding::TimeUnit::Millisecond,
                        10_i128.pow(u32::from(3 - count)),
                        PlainTimePrecision::Seconds(Some(count)),
                    ),
                    Some(count @ 4..=6) => (
                        rounding::TimeUnit::Microsecond,
                        10_i128.pow(u32::from(6 - count)),
                        PlainTimePrecision::Seconds(Some(count)),
                    ),
                    Some(count) => (
                        rounding::TimeUnit::Nanosecond,
                        10_i128.pow(u32::from(9 - count)),
                        PlainTimePrecision::Seconds(Some(count)),
                    ),
                },
            };
            let total = duration_math::time_fields_to_nanoseconds(
                fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
            );
            let rounded = duration_math::TimeDuration::from_nanoseconds(total)
                .round(unit, increment, mode)
                .total_nanoseconds();
            Ok(Value::String(
                Self::format_plain_time_string(
                    duration_math::time_fields_from_nanoseconds(rounded),
                    precision,
                )
                .into(),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn temporal_plain_time_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainTime cannot be converted to a primitive value".into(),
        ))
    }
}

/// `ToSecondsStringPrecisionRecord`'s `[[Precision]]`: whole minutes, or a
/// seconds field with either an explicit decimal-place count or `auto` (the
/// shortest form that loses nothing).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlainTimePrecision {
    Minute,
    Seconds(Option<u8>),
}
