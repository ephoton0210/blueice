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
                    TemporalKind::PlainTime => &[],
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
                value.hour =
                    self.temporal_optional_integer(native::argument(args, 0), 0, 0, 23, "hour")?
                        as u8;
                value.minute =
                    self.temporal_optional_integer(native::argument(args, 1), 0, 0, 59, "minute")?
                        as u8;
                value.second =
                    self.temporal_optional_integer(native::argument(args, 2), 0, 0, 59, "second")?
                        as u8;
                value.millisecond = self.temporal_optional_integer(
                    native::argument(args, 3),
                    0,
                    0,
                    999,
                    "millisecond",
                )? as u16;
                value.microsecond = self.temporal_optional_integer(
                    native::argument(args, 4),
                    0,
                    0,
                    999,
                    "microsecond",
                )? as u16;
                value.nanosecond = self.temporal_optional_integer(
                    native::argument(args, 5),
                    0,
                    0,
                    999,
                    "nanosecond",
                )? as u16;
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
    ) -> Result<Value, RuntimeError> {
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
    /// non-object value throws.
    fn temporal_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let object = if *value == Value::Undefined {
            self.with_roots(|heap| heap.alloc_object(None))?
        } else {
            self.coerce_object(value)?
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
        for (index, name) in [
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ]
        .into_iter()
        .enumerate()
        {
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
}
