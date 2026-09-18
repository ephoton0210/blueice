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
mod time_zone;
mod time_zone_id;

/// The calendar fields exposed by Temporal are derived from its ISO internal
/// date. Keeping ISO fields in `TemporalValue` preserves the invariant used
/// by DateTimeFormat's plain-value bridge while ICU4X performs the actual
/// non-ISO conversion at each observable calendar boundary.
/// The ten `Temporal.Duration` field names paired with their index in
/// [`blueice_ecma402::DurationRecord`]'s own `years`..`nanoseconds` order.
///
/// The *listed* order is alphabetical, because that is the order
/// `ToTemporalDurationRecord`/`ToPartialDuration` observably read a property
/// bag in — checked by Test262's
/// `built-ins/Temporal/Duration/prototype/{add,with}/order-of-operations.js`,
/// not assumed.
const DURATION_FIELDS_IN_READ_ORDER: [(&str, usize); 10] = [
    ("days", 3),
    ("hours", 4),
    ("microseconds", 8),
    ("milliseconds", 7),
    ("minutes", 5),
    ("months", 1),
    ("nanoseconds", 9),
    ("seconds", 6),
    ("weeks", 2),
    ("years", 0),
];

/// A `largestUnit`/`smallestUnit`/`unit` option's three observable states.
/// `Unset` and `Auto` resolve to the same unit wherever both are allowed, but
/// they are not interchangeable: `Temporal.Duration.prototype.round` requires
/// *some* unit option to be present, and `largestUnit: "auto"` satisfies that
/// while omitting it does not (Test262's
/// `built-ins/Temporal/Duration/prototype/round/succeeds-with-largest-unit-auto.js`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnitOption {
    Unset,
    Auto,
    Unit(rounding::TemporalUnit),
}

impl UnitOption {
    fn unit(self) -> Option<rounding::TemporalUnit> {
        match self {
            Self::Unit(unit) => Some(unit),
            Self::Unset | Self::Auto => None,
        }
    }
}

struct TemporalCalendarFields {
    year: i32,
    month: u8,
    month_code: String,
    day: u8,
    era: Option<String>,
    era_year: Option<i32>,
    months_in_year: u8,
}

/// How many sub-second digits an ISO serialization prints, as
/// `ToSecondsStringPrecision` resolves it: `Minute` omits the seconds field
/// entirely, `Auto` prints the shortest exact fraction (or none), and
/// `Digits(n)` prints exactly `n` digits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SecondsPrecision {
    Minute,
    Auto,
    Digits(u8),
}

/// Canonicalizes a calendar identifier, from wherever it was written: a
/// property bag, a constructor argument, or an ISO string's `[u-ca=...]`
/// annotation. All three spellings are case-insensitive and share the same
/// alias table, so they all resolve here rather than only on the paths that
/// happened to be written first.
fn canonical_calendar_id(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    let canonical = match lower.as_str() {
        // ECMA-402-visible aliases must carry the canonical calendar
        // identifier through Temporal as well as Intl.Locale.
        "islamicc" => "islamic-civil",
        "ethiopic-amete-alem" => "ethioaa",
        value => value,
    };
    calendar::calendar_kind(canonical)
        .is_some()
        .then(|| canonical.to_string())
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
                        // Every `Temporal.Duration` parameter is optional, so
                        // its `length` is zero (Test262's Duration/length.js).
                        TemporalKind::Duration => 0,
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
                        // `sign` and `blank` are accessors, not methods —
                        // Test262's Duration/prototype/{sign,blank}/prop-desc.js
                        // check for a getter function and no setter.
                        ("sign", native::TemporalGetter::DurationSign),
                        ("blank", native::TemporalGetter::DurationBlank),
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
                        ("timeZoneId", native::TemporalGetter::TimeZoneId),
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
                if kind == TemporalKind::Duration {
                    for (name, arity, method) in [
                        ("with", 1, NativeFunction::TemporalDurationWith),
                        ("negated", 0, NativeFunction::TemporalDurationNegated),
                        ("abs", 0, NativeFunction::TemporalDurationAbs),
                        ("add", 1, NativeFunction::TemporalDurationAdd),
                        ("subtract", 1, NativeFunction::TemporalDurationSubtract),
                        ("round", 1, NativeFunction::TemporalDurationRound),
                        ("total", 1, NativeFunction::TemporalDurationTotal),
                        ("toString", 0, NativeFunction::TemporalDurationToString),
                        ("toJSON", 0, NativeFunction::TemporalDurationToJson),
                        (
                            "toLocaleString",
                            0,
                            NativeFunction::TemporalDurationToLocaleString,
                        ),
                        ("valueOf", 0, NativeFunction::TemporalDurationValueOf),
                    ] {
                        self.install_native(prototype, function_prototype, name, arity, method)?;
                    }
                    self.install_native(
                        constructor,
                        function_prototype,
                        "compare",
                        2,
                        NativeFunction::TemporalDurationCompare,
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
                        (
                            "toLocaleString",
                            0,
                            NativeFunction::TemporalInstantToLocaleString,
                        ),
                        ("valueOf", 0, NativeFunction::TemporalInstantValueOf),
                        (
                            "toZonedDateTimeISO",
                            1,
                            NativeFunction::TemporalInstantToZonedDateTimeIso,
                        ),
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
            // `Temporal.Now` is a plain namespace object, not a constructor:
            // no `TemporalKind` variant, no prototype, no `[[Construct]]` on
            // any of its methods (`is_constructor`'s whitelist excludes them).
            let now = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(now));
            self.define_data(
                now,
                JsSymbol::well_known("toStringTag"),
                Value::String("Temporal.Now".into()),
                false,
                false,
                true,
            )?;
            for (name, method) in [
                ("instant", NativeFunction::TemporalNowInstant),
                ("plainDateISO", NativeFunction::TemporalNowPlainDateIso),
                (
                    "plainDateTimeISO",
                    NativeFunction::TemporalNowPlainDateTimeIso,
                ),
                ("plainTimeISO", NativeFunction::TemporalNowPlainTimeIso),
                ("timeZoneId", NativeFunction::TemporalNowTimeZoneId),
                (
                    "zonedDateTimeISO",
                    NativeFunction::TemporalNowZonedDateTimeIso,
                ),
            ] {
                // Every method's own time-zone parameter is optional, so each
                // reports `length` 0.
                self.install_native(now, function_prototype, name, 0, method)?;
            }
            self.define_data(namespace, "Now", Value::Object(now), true, false, true)?;
            self.stack.pop();
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

    /// `ToBigInt`: a Number is a `TypeError` (not a truncation), a Boolean is
    /// `0n`/`1n`, and a String that is not an integer literal is a
    /// `SyntaxError` — the exact set Test262's `Instant/basic.js` and
    /// `Instant/argument.js` pin for the constructor's argument.
    fn temporal_to_big_int(&mut self, value: &Value) -> Result<BigInt, RuntimeError> {
        match self.coerce_primitive(value, "number")? {
            Value::BigInt(value) => Ok(value),
            Value::Bool(flag) => Ok(BigInt::from(u8::from(flag))),
            Value::String(text) => {
                let text = text
                    .to_utf8()
                    .map_err(|_| RuntimeError::SyntaxError("invalid BigInt string".into()))?;
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return Ok(BigInt::from(0));
                }
                BigInt::parse_bytes(trimmed.as_bytes(), 10)
                    .ok_or_else(|| RuntimeError::SyntaxError("invalid BigInt string".into()))
            }
            _ => Err(RuntimeError::TypeError(
                "Temporal.Instant requires epoch nanoseconds as a BigInt".into(),
            )),
        }
    }

    fn temporal_calendar(&mut self, value: &Value) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok("iso8601".into());
        }
        let value = self.coerce_string(value)?;
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar".into()))?;
        canonical_calendar_id(&value)
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
                value.epoch_nanoseconds = self.temporal_to_big_int(native::argument(args, 0))?;
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
                    .temporal_time_zone(native::argument(args, 1))?
                    .identifier();
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
        // Each Temporal type reads a different production of the same
        // grammar: a year-month string may omit the day, a month-day string
        // the year, and a time string the date entirely.
        let invalid =
            || RuntimeError::RangeError(format!("invalid Temporal.{} string", kind.name()));
        let parsed = match kind {
            TemporalKind::PlainYearMonth => iso::parse_year_month(source),
            TemporalKind::PlainMonthDay => iso::parse_month_day(source),
            TemporalKind::PlainTime => iso::parse_time(source),
            _ => iso::parse_date_time(source),
        }
        .ok_or_else(invalid)?;
        // A type with no calendar slot ignores the annotation outright — even
        // an unrecognized or critical one
        // (`Instant/from/argument-string-calendar-annotation.js`,
        // `PlainTime/from/argument-string-calendar-annotation.js`).
        let calendar_slot = !matches!(kind, TemporalKind::Instant | TemporalKind::PlainTime);
        let calendar = match parsed.calendar.as_deref().filter(|_| calendar_slot) {
            Some(calendar) => canonical_calendar_id(calendar).ok_or_else(|| {
                RuntimeError::RangeError(format!("unsupported Temporal calendar: {calendar}"))
            })?,
            None => "iso8601".to_string(),
        };
        // The UTC designator asserts an exact instant, which a wall-clock
        // type has no way to represent, so it is a syntax error there rather
        // than something to ignore.
        if parsed.utc_designator
            && !matches!(kind, TemporalKind::Instant | TemporalKind::ZonedDateTime)
        {
            return Err(invalid());
        }
        let (year, month, day) = (parsed.year, parsed.month, parsed.day);
        let time = parsed.time.unwrap_or((0, 0, 0, 0, 0, 0));
        if kind == TemporalKind::PlainTime {
            return Ok(Self::plain_time_value(time));
        }
        let (hour, minute, second, millisecond, microsecond, nanosecond) = time;
        // A year-month or month-day string that never spelled the missing
        // half cannot be resolved in a calendar whose months do not line up
        // with ISO's, so those combinations are out of range rather than
        // silently reinterpreted.
        let non_iso = calendar != "iso8601";
        match kind {
            TemporalKind::PlainYearMonth => {
                if non_iso && !parsed.day_present {
                    return Err(RuntimeError::RangeError(
                        "a Temporal.PlainYearMonth string without a day requires the ISO calendar"
                            .into(),
                    ));
                }
                if !iso::is_year_month_within_limits(year, month) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainYearMonth string is outside the supported range".into(),
                    ));
                }
            }
            TemporalKind::PlainMonthDay => {
                if non_iso && !parsed.year_present {
                    return Err(RuntimeError::RangeError(
                        "a Temporal.PlainMonthDay string without a year requires the ISO calendar"
                            .into(),
                    ));
                }
                // The reference year carries no range of its own, but a
                // non-ISO calendar still has to convert the spelled date.
                if non_iso && !epoch::is_date_within_limits((year, month, day)) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay string is outside the supported range".into(),
                    ));
                }
            }
            // A `PlainDate`'s range is judged at noon, so it reaches one day
            // further at each end than a `PlainDateTime`'s at midnight.
            TemporalKind::PlainDate if !epoch::is_date_within_limits((year, month, day)) => {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDate string is outside the supported range".into(),
                ));
            }
            TemporalKind::PlainDateTime
                if !epoch::is_date_time_within_limits((year, month, day), time) =>
            {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDateTime string is outside the supported range".into(),
                ));
            }
            _ => {}
        }
        let epoch_nanoseconds = if kind == TemporalKind::Instant {
            // An instant string must pin its offset: a wall-clock reading
            // alone does not identify one.
            if parsed.time.is_none()
                || (!parsed.utc_designator && parsed.offset_nanoseconds.is_none())
            {
                return Err(invalid());
            }
            let epoch_nanoseconds = epoch::nanoseconds_since_epoch((year, month, day), time, 0)
                - parsed.offset_nanoseconds.unwrap_or(0);
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
            time_zone: parsed.time_zone.unwrap_or_else(|| "UTC".into()),
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
        if kind == TemporalKind::Duration {
            // `Temporal.Duration.from` is exactly `ToTemporalDuration`, which
            // already accepts a Duration, an ISO string and a property bag.
            let record = self.temporal_duration_from_value(value)?;
            return self.alloc_temporal_value(Self::temporal_duration_value(record), false);
        }
        if kind == TemporalKind::Instant {
            // `Temporal.Instant.from` *is* `ToTemporalInstant`, including its
            // ZonedDateTime fast path and its TypeError for non-strings.
            let epoch_nanoseconds = self.temporal_to_instant_epoch(value)?;
            return self.instant_from_epoch_nanoseconds(epoch_nanoseconds);
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
            native::TemporalGetter::DurationSign | native::TemporalGetter::DurationBlank => {
                if value.kind != TemporalKind::Duration {
                    return Err(RuntimeError::TypeError(
                        "Temporal.Duration getter requires a duration receiver".into(),
                    ));
                }
                let sign = value
                    .duration
                    .as_deref()
                    .expect("Temporal.Duration values retain a duration record")
                    .sign();
                Ok(if getter == native::TemporalGetter::DurationSign {
                    Value::Number(sign.into())
                } else {
                    Value::Bool(sign == 0)
                })
            }
            native::TemporalGetter::CalendarId => Ok(Value::String(value.calendar.into())),
            native::TemporalGetter::EpochMilliseconds
                if matches!(
                    value.kind,
                    TemporalKind::ZonedDateTime | TemporalKind::Instant
                ) =>
            {
                // `floor(epochNanoseconds / 10^6)`, not a truncation toward
                // zero: a pre-epoch instant's milliseconds round *down*
                // (Test262's `epochMilliseconds/basic.js`).
                let million = BigInt::from(1_000_000);
                let mut milliseconds = &value.epoch_nanoseconds / &million;
                if &value.epoch_nanoseconds % &million != BigInt::from(0)
                    && value.epoch_nanoseconds < BigInt::from(0)
                {
                    milliseconds -= 1;
                }
                milliseconds
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
            native::TemporalGetter::TimeZoneId if value.kind == TemporalKind::ZonedDateTime => {
                Ok(Value::String(value.time_zone.into()))
            }
            native::TemporalGetter::TimeZoneId => Err(RuntimeError::TypeError(
                "Temporal timeZoneId requires a ZonedDateTime receiver".into(),
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
        options: &Value,
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
        // `Temporal.PlainDate.prototype.toZonedDateTime` takes one `item`
        // argument (a bare identifier or a `{ timeZone, plainTime }` bag) and
        // no options object; `Temporal.PlainDateTime.prototype` takes a bare
        // identifier plus an options object carrying `disambiguation`.
        let (zone, time, disambiguation) = if value.kind == TemporalKind::PlainDateTime {
            let zone = self.temporal_time_zone(time_zone)?;
            let disambiguation = self.temporal_disambiguation(options)?;
            (zone, None, disambiguation)
        } else {
            let (zone, time) = self.temporal_plain_date_zone_and_time(time_zone)?;
            (zone, time, time_zone::Disambiguation::Compatible)
        };
        value.epoch_nanoseconds = if value.kind == TemporalKind::PlainDate && time.is_none() {
            // A `PlainDate` with no time of day becomes the zone's start of
            // day, which is not always local midnight.
            zone.start_of_day((value.year, value.month, value.day))
        } else {
            if let Some((hour, minute, second, millisecond, microsecond, nanosecond)) = time {
                value.hour = hour;
                value.minute = minute;
                value.second = second;
                value.millisecond = millisecond;
                value.microsecond = microsecond;
                value.nanosecond = nanosecond;
            }
            zone.epoch_nanoseconds_for(
                (value.year, value.month, value.day),
                (
                    value.hour,
                    value.minute,
                    value.second,
                    value.millisecond,
                    value.microsecond,
                    value.nanosecond,
                ),
                disambiguation,
            )
            .map_err(temporal_resolution_error)?
        };
        if !epoch::is_in_instant_range(&value.epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range".into(),
            ));
        }
        // The stored ISO fields are the *resolved* local wall-clock ones, not
        // the requested ones: a skipped local time resolves to the shifted
        // time, and a start-of-day resolution to the zone's real first
        // wall-clock time of the day.
        temporal_set_local_fields(&mut value, &zone);
        value.kind = TemporalKind::ZonedDateTime;
        value.time_zone = zone.identifier();
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

    /// `GetOptionsObject`: `undefined` becomes a fresh empty object; an
    /// Object is used as-is; any other value is a `TypeError` — it is
    /// deliberately *not* boxed through `ToObject`, so
    /// `instant.toString("some string")` throws rather than reading options
    /// off a String wrapper. Test262's
    /// `Instant/prototype/toString/options-wrong-type.js` and
    /// `PlainTime/prototype/until/options-wrong-type.js` both pass
    /// `"hello"`/`1`/`1n` and require the throw.
    fn temporal_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let result = match value {
            Value::Undefined => Value::Object(self.with_roots(|heap| heap.alloc_object(None))?),
            Value::Object(_) => value.clone(),
            _ => {
                return Err(RuntimeError::TypeError(
                    "Temporal options must be an object or undefined".into(),
                ));
            }
        };
        self.stack.push(result.clone());
        Ok(result)
    }

    /// The `roundTo` parameter of `Temporal.Instant.prototype.round`: a
    /// String is shorthand for `{ smallestUnit: <string> }`, carried on a
    /// null-prototype object so `Object.prototype` accessors for the other
    /// option names are never consulted (Test262's
    /// `string-shorthand-no-object-prototype-pollution.js`).
    fn temporal_round_to(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        if *value == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.Instant.round requires a roundTo argument".into(),
            ));
        }
        if !matches!(value, Value::String(_)) {
            return self.temporal_options(value);
        }
        let object = self.with_roots(|heap| heap.alloc_object(None))?;
        let result = Value::Object(object);
        self.stack.push(result.clone());
        self.define_data(object, "smallestUnit", value.clone(), true, true, true)?;
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

    /// `GetTemporalUnitValuedOption`: reads a unit-valued option, accepting
    /// **every** unit name (including the calendar units `Temporal.Instant`
    /// itself never allows) plus an optional extra literal such as `"auto"`.
    /// Rejecting a syntactically valid but operation-inappropriate unit is a
    /// separate, later step — the ordering Test262's
    /// `options-read-before-algorithmic-validation.js` fixtures observe.
    fn temporal_unit_option(
        &mut self,
        options: &Value,
        name: &str,
        auto: bool,
    ) -> Result<Option<rounding::Unit>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if auto && string == "auto" {
            return Ok(None);
        }
        rounding::parse_unit(&string)
            .map(Some)
            .ok_or_else(|| RuntimeError::RangeError(format!("invalid {name} option")))
    }

    /// Narrows an already-read unit option to a time unit, optionally also
    /// rejecting `"hour"` (which `toString` disallows while `round` allows).
    fn temporal_time_unit(
        unit: Option<rounding::Unit>,
        name: &str,
        allow_hour: bool,
    ) -> Result<Option<rounding::TimeUnit>, RuntimeError> {
        match unit {
            None => Ok(None),
            Some(rounding::Unit::Time(rounding::TimeUnit::Hour)) if !allow_hour => {
                Err(RuntimeError::RangeError(format!("invalid {name} option")))
            }
            Some(rounding::Unit::Time(unit)) => Ok(Some(unit)),
            Some(rounding::Unit::Date(_)) => {
                Err(RuntimeError::RangeError(format!("invalid {name} option")))
            }
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
        // `ToTemporalPartialDurationRecord` reads the ten fields in
        // *alphabetical* order, not in largest-to-smallest order — observable
        // via getters/`valueOf`: Test262's
        // `Instant/prototype/add/order-of-operations.js` and
        // `PlainTime/prototype/add/order-of-operations.js` both assert each
        // getter/`valueOf` fires in exactly this sequence.
        for (name, index) in DURATION_FIELDS_IN_READ_ORDER {
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

    /// Parses a `TemporalInstantString` into epoch nanoseconds.
    fn instant_epoch_from_string(source: &str) -> Result<BigInt, RuntimeError> {
        let parts = iso::parse_instant(source)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant string".into()))?;
        // The offset is exact to the nanosecond, so it is applied here rather
        // than through `nanoseconds_since_epoch`'s whole-second parameter.
        let epoch_nanoseconds = epoch::nanoseconds_since_epoch(parts.date, parts.time, 0)
            - BigInt::from(parts.offset_nanoseconds);
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.Instant string is outside the supported range".into(),
            ));
        }
        Ok(epoch_nanoseconds)
    }

    /// `ToTemporalInstant`: an `Instant` or `ZonedDateTime` argument's epoch
    /// nanoseconds are used directly (no observable property reads); any
    /// other object is taken through `ToPrimitive` with a string hint, and a
    /// result that is not a String is a `TypeError` rather than being
    /// stringified — so `instant.equals(1)` throws `TypeError`, not
    /// `RangeError` (Test262's `argument-wrong-type.js`).
    fn temporal_to_instant_epoch(&mut self, value: &Value) -> Result<BigInt, RuntimeError> {
        let primitive = if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if matches!(
                    temporal.kind,
                    TemporalKind::Instant | TemporalKind::ZonedDateTime
                ) {
                    return Ok(temporal.epoch_nanoseconds);
                }
            }
            self.coerce_primitive(value, "string")?
        } else {
            value.clone()
        };
        let Value::String(text) = primitive else {
            return Err(RuntimeError::TypeError(
                "Temporal.Instant requires an Instant or an ISO 8601 string".into(),
            ));
        };
        let source = text
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal.Instant string".into()))?;
        Self::instant_epoch_from_string(&source)
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
        let options = self.temporal_round_to(options)?;
        // Every option is read and coerced in alphabetical order, before any
        // of them is validated against the others.
        let increment = self.temporal_rounding_increment(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::HalfExpand)?;
        let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", true)?
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal.Instant.round requires smallestUnit".into())
            })?;
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
            .round_as_if_positive(smallest_unit, increment, mode)
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
        // `GetDifferenceSettings` reads largestUnit, roundingIncrement,
        // roundingMode and smallestUnit in that (alphabetical) order, and
        // validates none of them until all four have been read and coerced.
        let largest_unit = self.temporal_unit_option(&options, "largestUnit", true)?;
        let increment = self.temporal_rounding_increment(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
        let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
        let largest_unit = Self::temporal_time_unit(largest_unit, "largestUnit", true)?;
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", true)?
            .unwrap_or(rounding::TimeUnit::Nanosecond);
        // An absent (or explicit `"auto"`) largestUnit is the larger of
        // `Instant`'s own default, `"second"`, and smallestUnit — so
        // `{ smallestUnit: "hours" }` alone balances into hours rather than
        // being rejected as a smaller largestUnit.
        let largest_unit = largest_unit.unwrap_or(smallest_unit.max(rounding::TimeUnit::Second));
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        // `ValidateTemporalRoundingIncrement` with `inclusive = false`: the
        // increment must be strictly smaller than, and divide evenly into,
        // the count of this unit in the next larger one.
        let dividend = smallest_unit.increment_dividend();
        if increment >= dividend || dividend % increment != 0 {
            return Err(RuntimeError::RangeError(
                "roundingIncrement does not divide evenly into the next larger unit".into(),
            ));
        }
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
        // `CreateTemporalDuration` (via `temporal_duration_record`) rounds
        // every field to the nearest float64 before the range check, since
        // every `Temporal.Duration` field is a Number — an exact difference
        // that overflows what a double can represent precisely must be
        // observably rounded, not stored exactly
        // (`prototype/{since,until}/float64-representable-integer.js`).
        let record = Self::temporal_duration_record([
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
        ])?;
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

    fn format_instant_string(
        epoch_nanoseconds: &BigInt,
        precision: SecondsPrecision,
        offset_nanoseconds: Option<i128>,
    ) -> String {
        let local = match offset_nanoseconds {
            Some(offset) => epoch_nanoseconds + BigInt::from(offset),
            None => epoch_nanoseconds.clone(),
        };
        let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
            epoch::instant_fields(&local);
        let mut result = if (0..=9999).contains(&year) {
            format!("{year:04}")
        } else {
            format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
        };
        result.push_str(&format!("-{month:02}-{day:02}T{hour:02}:{minute:02}"));
        if precision != SecondsPrecision::Minute {
            result.push_str(&format!(":{second:02}"));
            let nanos_total = u32::from(millisecond) * 1_000_000
                + u32::from(microsecond) * 1_000
                + u32::from(nanosecond);
            match precision {
                SecondsPrecision::Minute | SecondsPrecision::Digits(0) => {}
                SecondsPrecision::Digits(digits) => {
                    let text = format!("{nanos_total:09}");
                    result.push('.');
                    result.push_str(&text[..digits as usize]);
                }
                SecondsPrecision::Auto if nanos_total != 0 => {
                    let text = format!("{nanos_total:09}");
                    result.push('.');
                    result.push_str(text.trim_end_matches('0'));
                }
                SecondsPrecision::Auto => {}
            }
        }
        match offset_nanoseconds {
            // `FormatDateTimeUTCOffsetRounded`: minutes, never seconds —
            // rounded (half away from zero) to the nearest minute, not
            // truncated. A fixed/UTC offset is always an exact multiple of a
            // minute, so this was unreachable before named zones (which can
            // carry a genuine sub-minute historical offset, e.g. Monrovia's
            // pre-1972 -00:44:30) started flowing through here.
            Some(offset) => {
                let minutes = (offset.abs() + 30_000_000_000) / 60_000_000_000;
                result.push_str(&format!(
                    "{}{:02}:{:02}",
                    if offset < 0 { '-' } else { '+' },
                    minutes / 60,
                    minutes % 60
                ));
            }
            None => result.push('Z'),
        }
        result
    }

    /// `GetTemporalFractionalSecondDigitsOption`, a `GetStringOrNumberOption`
    /// whose only permitted string is `"auto"`: a Number is floored and then
    /// range-checked (so `9.7` is 9 but `-0.6` is out of range), and anything
    /// that is not a Number is stringified and must equal `"auto"`.
    fn temporal_fractional_second_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalSecondDigits".into())?;
        match value {
            Value::Undefined => Ok(None),
            Value::Number(digits) => {
                let digits = digits.floor();
                if !digits.is_finite() || !(0.0..=9.0).contains(&digits) {
                    return Err(RuntimeError::RangeError(
                        "invalid fractionalSecondDigits".into(),
                    ));
                }
                Ok(Some(digits as u8))
            }
            value => {
                let text = self.coerce_string(&value)?;
                let text = text.to_utf8().map_err(|_| {
                    RuntimeError::RangeError("invalid fractionalSecondDigits".into())
                })?;
                if text == "auto" {
                    Ok(None)
                } else {
                    Err(RuntimeError::RangeError(
                        "invalid fractionalSecondDigits".into(),
                    ))
                }
            }
        }
    }

    /// `ToTemporalTimeZoneIdentifier` for the `timeZone` option, resolved to
    /// the offset that zone was actually observing at `epoch_nanoseconds` —
    /// the receiver `Instant`'s own epoch, per `GetOffsetNanosecondsFor`.
    /// `Ok(None)` means the option was absent.
    fn temporal_to_string_time_zone(
        &mut self,
        options: &Value,
        epoch_nanoseconds: &BigInt,
    ) -> Result<Option<i128>, RuntimeError> {
        let value = self.get_property(options, &"timeZone".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return iso::resolve_time_zone_offset(&temporal.time_zone, epoch_nanoseconds)
                        .map(Some)
                        .map_err(|()| RuntimeError::RangeError("invalid time zone".into()));
                }
            }
        }
        let Value::String(text) = &value else {
            return Err(RuntimeError::TypeError(
                "a Temporal time zone must be a string".into(),
            ));
        };
        let source = text
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid time zone".into()))?;
        iso::resolve_time_zone_offset(&source, epoch_nanoseconds)
            .map(Some)
            .map_err(|()| RuntimeError::RangeError("invalid time zone".into()))
    }

    pub(super) fn temporal_instant_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let epoch = self.temporal_instant_epoch(receiver)?;
        let options = self.temporal_options(options)?;
        // Read (and coerce) every option in alphabetical order first; only
        // then reject a unit this operation does not accept.
        let explicit_digits = self.temporal_fractional_second_digits(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
        let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
        let offset = self.temporal_to_string_time_zone(&options, &epoch)?;
        // `hour` is a valid unit name but not a valid `toString` precision.
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", false)?;
        // `ToSecondsStringPrecision`: smallestUnit wins outright, and the
        // digit count implies both the rounding unit and its increment.
        let (precision, unit, increment) = match smallest_unit {
            Some(rounding::TimeUnit::Minute) => {
                (SecondsPrecision::Minute, rounding::TimeUnit::Minute, 1)
            }
            Some(rounding::TimeUnit::Second) => {
                (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1)
            }
            Some(rounding::TimeUnit::Millisecond) => (
                SecondsPrecision::Digits(3),
                rounding::TimeUnit::Millisecond,
                1,
            ),
            Some(rounding::TimeUnit::Microsecond) => (
                SecondsPrecision::Digits(6),
                rounding::TimeUnit::Microsecond,
                1,
            ),
            Some(rounding::TimeUnit::Nanosecond) | Some(rounding::TimeUnit::Hour) => (
                SecondsPrecision::Digits(9),
                rounding::TimeUnit::Nanosecond,
                1,
            ),
            None => match explicit_digits {
                None => (
                    SecondsPrecision::Auto,
                    rounding::TimeUnit::Nanosecond,
                    1_i128,
                ),
                Some(0) => (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1),
                Some(digits @ 1..=3) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Millisecond,
                    10_i128.pow(u32::from(3 - digits)),
                ),
                Some(digits @ 4..=6) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Microsecond,
                    10_i128.pow(u32::from(6 - digits)),
                ),
                Some(digits) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Nanosecond,
                    10_i128.pow(u32::from(9 - digits)),
                ),
            },
        };
        let epoch_i128: i128 = epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let rounded = duration_math::TimeDuration::from_nanoseconds(epoch_i128)
            .round_as_if_positive(unit, increment, mode)
            .total_nanoseconds();
        Ok(Value::String(
            Self::format_instant_string(&BigInt::from(rounded), precision, offset).into(),
        ))
    }

    /// `Temporal.Instant.prototype.toLocaleString`, which is
    /// `CreateDateTimeFormat(locales, options, ANY, ALL)` followed by
    /// `FormatDateTime` — i.e. exactly what
    /// `new Intl.DateTimeFormat(locales, options).format(instant)` produces,
    /// so it is built from the same `Intl.DateTimeFormat` bridge rather than
    /// aliasing `toString`'s ISO serialization.
    pub(super) fn temporal_instant_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        // Brand check before any observable option read.
        self.temporal_instant_epoch(receiver)?;
        let stack_base = self.stack.len();
        let result = (|| {
            let formatter = self.create_date_time_format(
                &Value::Undefined,
                &[
                    native::argument(args, 0).clone(),
                    native::argument(args, 1).clone(),
                ],
                false,
            )?;
            self.stack.push(formatter.clone());
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
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
        let maximum = unit.increment_dividend();
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
                            iso::parse_offset_identifier_nanoseconds(&temporal.time_zone)
                                .ok_or_else(|| {
                                    RuntimeError::RangeError(
                                    "Temporal.PlainTime conversion supports UTC and fixed offsets"
                                        .into(),
                                )
                                })?
                        };
                        let local = &temporal.epoch_nanoseconds + BigInt::from(offset);
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

    // ---- Stage 1 Track C: Temporal.Now ----------------------------------

    /// `SystemUTCEpochNanoseconds`, read from the one wall clock this engine
    /// already has: `Date.now()`'s own `SystemTime` call. Reusing it means
    /// `Temporal.Now.instant()` and `Date.now()` can never disagree, which is
    /// exactly what Test262's `Now/instant/return-value-value.js` checks by
    /// bracketing the call between two `Date.now()` reads.
    ///
    /// Millisecond granularity therefore, not nanosecond. The spec leaves the
    /// clock's resolution implementation-defined and explicitly permits
    /// coarsening it; real engines clamp for the same reason.
    fn temporal_now_epoch_nanoseconds() -> BigInt {
        BigInt::from(Self::current_time() as i64) * 1_000_000_u32
    }

    /// `ToTemporalTimeZoneIdentifier`. A `Temporal.ZonedDateTime` contributes
    /// its own zone; every other object is a `TypeError` — note that no
    /// `ToString` coercion happens at all here, so an object with a
    /// `toString` method is rejected rather than consulted.
    fn temporal_time_zone_identifier(&mut self, value: &Value) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(time_zone_id::SYSTEM.into());
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return Ok(temporal.time_zone);
                }
            }
        }
        let Value::String(text) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal time zone must be a string or a Temporal.ZonedDateTime".into(),
            ));
        };
        let text = text
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
        time_zone_id::resolve(&text)
            .map_err(|()| RuntimeError::RangeError(format!("invalid Temporal time zone: {text}")))
    }

    /// `SystemDateTime`: the current instant's wall-clock fields in the zone
    /// `time_zone` names.
    fn temporal_now_local_fields(
        &mut self,
        time_zone: &Value,
    ) -> Result<(epoch::CivilDate, epoch::CivilTime), RuntimeError> {
        let identifier = self.temporal_time_zone_identifier(time_zone)?;
        let now = Self::temporal_now_epoch_nanoseconds();
        let offset = time_zone_id::offset_seconds(&identifier, &now).ok_or_else(|| {
            RuntimeError::RangeError(format!(
                "Temporal.Now cannot resolve a UTC offset for the time zone {identifier}"
            ))
        })?;
        let local = now + BigInt::from(offset) * 1_000_000_000_u32;
        Ok(epoch::instant_fields(&local))
    }

    pub(super) fn temporal_now_instant(&mut self) -> Result<Value, RuntimeError> {
        self.instant_from_epoch_nanoseconds(Self::temporal_now_epoch_nanoseconds())
    }

    pub(super) fn temporal_now_time_zone_id(&mut self) -> Result<Value, RuntimeError> {
        Ok(Value::String(time_zone_id::SYSTEM.into()))
    }

    /// `Temporal.Now.plainDateISO`/`plainDateTimeISO`/`plainTimeISO`: the same
    /// wall clock, projected onto whichever of the three ISO-calendar plain
    /// types `kind` names. The fields each type does not carry keep the
    /// constructors' own 1970-01-01T00:00 placeholders.
    pub(super) fn temporal_now_plain(
        &mut self,
        kind: TemporalKind,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
            self.temporal_now_local_fields(time_zone)?;
        let dated = kind != TemporalKind::PlainTime;
        let timed = kind != TemporalKind::PlainDate;
        self.alloc_temporal_value(
            TemporalValue {
                kind,
                duration: None,
                year: if dated { year } else { 1970 },
                month: if dated { month } else { 1 },
                day: if dated { day } else { 1 },
                hour: if timed { hour } else { 0 },
                minute: if timed { minute } else { 0 },
                second: if timed { second } else { 0 },
                millisecond: if timed { millisecond } else { 0 },
                microsecond: if timed { microsecond } else { 0 },
                nanosecond: if timed { nanosecond } else { 0 },
                epoch_nanoseconds: 0.into(),
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            },
            false,
        )
    }

    /// `Temporal.Now.zonedDateTimeISO`: unlike the plain variants this needs
    /// only a *valid* zone identifier, never its offset — the epoch value and
    /// the identifier are both exact, so a named IANA zone works here even
    /// while Track E's transition history is still missing. The ISO
    /// wall-clock fields stay at the same 1970-01-01 placeholder
    /// `instant_from_epoch_nanoseconds` leaves on an `Instant`; nothing
    /// observable reads them for a `ZonedDateTime` yet, and Stage 2 will
    /// derive them from the epoch and the zone rather than store them.
    pub(super) fn temporal_now_zoned_date_time(
        &mut self,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let identifier = self.temporal_time_zone_identifier(time_zone)?;
        let epoch_nanoseconds = Self::temporal_now_epoch_nanoseconds();
        self.alloc_temporal_value(
            TemporalValue {
                kind: TemporalKind::ZonedDateTime,
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
                time_zone: identifier,
            },
            false,
        )
    }

    // ---- Stage 1 Track E: time-zone identifiers and offsets -------------

    /// `ToTemporalTimeZoneIdentifier`: a `ZonedDateTime` contributes its own
    /// stored zone; every other object — and every non-string primitive — is
    /// a `TypeError`, because Temporal deliberately does not run `ToString`
    /// on a time-zone argument. An unparseable string is a `RangeError`.
    fn temporal_time_zone(&mut self, value: &Value) -> Result<time_zone::TimeZone, RuntimeError> {
        let invalid = |source: &str| {
            RuntimeError::RangeError(format!("invalid Temporal time zone: {source}"))
        };
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return time_zone::parse_identifier(&temporal.time_zone)
                        .ok_or_else(|| invalid(&temporal.time_zone));
                }
            }
        }
        let Value::String(source) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal time zone must be a string or a Temporal.ZonedDateTime".into(),
            ));
        };
        let source = source
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
        time_zone::parse_identifier(&source).ok_or_else(|| invalid(&source))
    }

    /// `Temporal.PlainDate.prototype.toZonedDateTime`'s single `item`
    /// argument: either a bare time-zone identifier, or a property bag whose
    /// `timeZone` names the zone and whose optional `plainTime` supplies the
    /// time of day (absent meaning the zone's start of day).
    fn temporal_plain_date_zone_and_time(
        &mut self,
        item: &Value,
    ) -> Result<(time_zone::TimeZone, Option<epoch::CivilTime>), RuntimeError> {
        if item.object_id().is_none() {
            return Ok((self.temporal_time_zone(item)?, None));
        }
        let requested = self.get_property(item, &"timeZone".into())?;
        if requested == Value::Undefined {
            // No `timeZone` property: the item itself has to be the zone,
            // which only a `ZonedDateTime` can satisfy — a plain object is a
            // `TypeError`, exactly as `ToTemporalTimeZoneIdentifier` says.
            return Ok((self.temporal_time_zone(item)?, None));
        }
        let zone = self.temporal_time_zone(&requested)?;
        let plain_time = self.get_property(item, &"plainTime".into())?;
        Ok((zone, self.temporal_time_of_day(&plain_time)?))
    }

    /// A narrowed `ToTemporalTime`: `undefined` means "start of day", and an
    /// existing `Temporal.PlainTime`/`PlainDateTime` contributes its own time
    /// fields.
    ///
    /// Converting a *string* to a `Temporal.PlainTime` is deliberately not
    /// implemented here — `Temporal.PlainTime` is Phase 26 Stage 1 Track D's
    /// own scope, and this engine has no time-only string parser yet
    /// (`temporal_value_from_string` requires a date). Rather than accept a
    /// time string and silently mis-parse it, this fails closed with the
    /// `RangeError` the spec raises for an invalid one.
    fn temporal_time_of_day(
        &mut self,
        value: &Value,
    ) -> Result<Option<epoch::CivilTime>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(None);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if matches!(
                    temporal.kind,
                    TemporalKind::PlainTime | TemporalKind::PlainDateTime
                ) {
                    return Ok(Some((
                        temporal.hour,
                        temporal.minute,
                        temporal.second,
                        temporal.millisecond,
                        temporal.microsecond,
                        temporal.nanosecond,
                    )));
                }
            }
        }
        if matches!(value, Value::String(_)) || value.object_id().is_some() {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainTime conversion from this value is not supported yet".into(),
            ));
        }
        Err(RuntimeError::TypeError(
            "Temporal.PlainTime cannot be created from this value".into(),
        ))
    }

    /// `ToTemporalDisambiguation`: a `"compatible"`-defaulted string option.
    fn temporal_disambiguation(
        &mut self,
        options: &Value,
    ) -> Result<time_zone::Disambiguation, RuntimeError> {
        let options = self.temporal_options(options)?;
        let Some(name) = self.temporal_string_option(&options, "disambiguation", &[])? else {
            return Ok(time_zone::Disambiguation::Compatible);
        };
        time_zone::parse_disambiguation(&name)
            .ok_or_else(|| RuntimeError::RangeError("invalid disambiguation option".into()))
    }

    pub(super) fn temporal_instant_to_zoned_date_time_iso(
        &mut self,
        receiver: &Value,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let epoch_nanoseconds = self.temporal_instant_epoch(receiver)?;
        let zone = self.temporal_time_zone(time_zone)?;
        let mut value = TemporalValue {
            kind: TemporalKind::ZonedDateTime,
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
            time_zone: zone.identifier(),
        };
        temporal_set_local_fields(&mut value, &zone);
        self.alloc_temporal_value(value, false)
    }

    // ---- Stage 1 Track B: Temporal.Duration arithmetic ------------------
    //
    // Every method below implements the *calendar-agnostic* case completely:
    // a duration whose `years`/`months`/`weeks` are all zero, addressed with
    // units of `day` or smaller. `days` participate fully, at Temporal's own
    // fixed 86,400 seconds per day. A request that genuinely needs calendar
    // arithmetic — a non-zero `years`/`months`/`weeks`, a `year`/`month`/
    // `week` unit, or a `relativeTo` anchor this engine cannot resolve — is
    // rejected with a `RangeError` rather than answered approximately; see
    // Phase 26's plan for the Stage 2 boundary.

    /// Reads a validated `Temporal.Duration` receiver's own record.
    fn temporal_duration_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Duration method requires a Duration receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Duration method requires a Duration receiver".into())
        })?;
        if value.kind != TemporalKind::Duration {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration method requires a Duration receiver".into(),
            ));
        }
        Ok(*value
            .duration
            .as_deref()
            .expect("Temporal.Duration values retain a duration record"))
    }

    fn temporal_duration_value(record: blueice_ecma402::DurationRecord) -> TemporalValue {
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
        }
    }

    /// `CreateTemporalDuration`: stores the ten fields, then validates.
    ///
    /// Every field is a Number on a `Temporal.Duration`, so an exact
    /// nanosecond-accurate balancing result is observably rounded to the
    /// nearest double *before* the range check — and a value that passed the
    /// check exactly can fail it once rounded. Test262 checks this directly
    /// (`prototype/round/{float64-representable-integer,
    /// out-of-range-when-converting-from-normalized-duration}.js`,
    /// `prototype/add/{float64-representable-integer,result-out-of-range-3,
    /// argument-duration-precision-exact-numerical-values}.js`), so the
    /// round-trip is part of the algorithm rather than a lossy shortcut.
    fn temporal_duration_record(
        fields: [i128; 10],
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        let fields = fields.map(|value| value as f64 as i128);
        blueice_ecma402::DurationRecord::try_new(
            fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], fields[6], fields[7],
            fields[8], fields[9],
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    fn temporal_duration_create(&mut self, fields: [i128; 10]) -> Result<Value, RuntimeError> {
        let record = Self::temporal_duration_record(fields)?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    /// `DefaultTemporalLargestUnit`: the largest unit the record actually uses.
    fn temporal_duration_largest_unit(
        record: &blueice_ecma402::DurationRecord,
    ) -> rounding::TemporalUnit {
        for (value, unit) in [
            (record.years, rounding::TemporalUnit::Year),
            (record.months, rounding::TemporalUnit::Month),
            (record.weeks, rounding::TemporalUnit::Week),
            (record.days, rounding::TemporalUnit::Day),
            (record.hours, rounding::TemporalUnit::Hour),
            (record.minutes, rounding::TemporalUnit::Minute),
            (record.seconds, rounding::TemporalUnit::Second),
            (record.milliseconds, rounding::TemporalUnit::Millisecond),
            (record.microseconds, rounding::TemporalUnit::Microsecond),
        ] {
            if value != 0 {
                return unit;
            }
        }
        rounding::TemporalUnit::Nanosecond
    }

    /// `GetOptionsObject`: `undefined` becomes a fresh empty object; any other
    /// non-object throws. Deliberately not `ToObject` — a primitive must be
    /// rejected, not boxed.
    fn temporal_duration_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        match value {
            Value::Undefined => {
                let object = self.with_roots(|heap| heap.alloc_object(None))?;
                let result = Value::Object(object);
                self.stack.push(result.clone());
                Ok(result)
            }
            Value::Object(_) => Ok(value.clone()),
            _ => Err(RuntimeError::TypeError(
                "Temporal options must be an object".into(),
            )),
        }
    }

    /// The required first argument of `round`/`total`: either a bare unit
    /// string (which the specification turns into a null-prototype object
    /// carrying only that one option, so no other option may be looked up) or
    /// an options object. `undefined` throws a `TypeError`.
    fn temporal_duration_round_to(
        &mut self,
        value: &Value,
        method: &str,
    ) -> Result<(Option<String>, Value), RuntimeError> {
        if *value == Value::Undefined {
            return Err(RuntimeError::TypeError(format!(
                "Temporal.Duration.prototype.{method} requires an argument"
            )));
        }
        if let Value::String(text) = value {
            let text = text
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError(format!("invalid {method} unit")))?;
            return Ok((Some(text), Value::Undefined));
        }
        let options = self.temporal_duration_options(value)?;
        Ok((None, options))
    }

    /// `GetTemporalUnitValuedOption`. `auto` is recognised only where
    /// `allow_auto` says so.
    fn temporal_duration_unit_option(
        &mut self,
        options: &Value,
        name: &str,
        allow_auto: bool,
    ) -> Result<UnitOption, RuntimeError> {
        let mut allowed = rounding::TEMPORAL_UNIT_NAMES.to_vec();
        if allow_auto {
            allowed.push("auto");
        }
        match self.temporal_string_option(options, name, &allowed)? {
            None => Ok(UnitOption::Unset),
            Some(text) if text == "auto" => Ok(UnitOption::Auto),
            Some(text) => Ok(UnitOption::Unit(
                rounding::parse_temporal_unit(&text)
                    .expect("temporal_string_option already validated the unit name"),
            )),
        }
    }

    fn temporal_duration_unit_name(
        text: &str,
        name: &str,
    ) -> Result<rounding::TemporalUnit, RuntimeError> {
        rounding::parse_temporal_unit(text)
            .ok_or_else(|| RuntimeError::RangeError(format!("invalid {name} option")))
    }

    /// `GetTemporalRelativeToOption`, as far as Stage 1 can honour it: returns
    /// whether an anchor was supplied at all.
    ///
    /// Accepted, and then deliberately ignored: a `Temporal.PlainDate`/
    /// `PlainDateTime` object, a date/date-time string with no time-zone
    /// annotation, and a `Temporal.ZonedDateTime` whose time zone is `UTC` or
    /// a fixed UTC offset. None of those can change a calendar-agnostic
    /// answer — relative to a plain date, or inside a zone with no offset
    /// transitions, Temporal fixes a day at exactly 86,400 seconds, which is
    /// what the calendar-agnostic path already assumes.
    ///
    /// Rejected: a named-IANA-zone `Temporal.ZonedDateTime`, where a day can
    /// genuinely be 23 or 25 hours long (that needs Track E's `TimeZone`
    /// transition data), plus a property bag and a zoned string, which both
    /// need Stage 2's calendar-aware `PlainDate` field resolution.
    fn temporal_duration_relative_to(&mut self, value: &Value) -> Result<bool, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(false);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                return match temporal.kind {
                    TemporalKind::PlainDate | TemporalKind::PlainDateTime => Ok(true),
                    TemporalKind::ZonedDateTime => {
                        let fixed_offset = temporal.time_zone.starts_with(['+', '-'])
                            && iso::parse_offset_seconds(&temporal.time_zone).is_some();
                        if temporal.time_zone == "UTC" || fixed_offset {
                            Ok(true)
                        } else {
                            Err(RuntimeError::RangeError(
                                "a named-time-zone Temporal.ZonedDateTime relativeTo is not \
                                 supported yet"
                                    .into(),
                            ))
                        }
                    }
                    _ => Err(RuntimeError::TypeError(
                        "relativeTo must be a PlainDate, PlainDateTime or ZonedDateTime".into(),
                    )),
                };
            }
            return Err(RuntimeError::TypeError(
                "a relativeTo property bag is not supported yet".into(),
            ));
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "relativeTo must be an object or a string".into(),
            ));
        }
        let source = self
            .coerce_string(value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid relativeTo string".into()))?;
        // A leading bracket annotation that is not `u-ca=` is a time-zone
        // identifier, i.e. a ZonedDateTime anchor.
        let zoned = source.find('[').is_some_and(|index| {
            !source[index + 1..]
                .trim_start_matches('!')
                .starts_with("u-ca=")
        });
        if zoned || source.ends_with('Z') || source.ends_with('z') {
            return Err(RuntimeError::RangeError(
                "a zoned relativeTo string is not supported yet".into(),
            ));
        }
        self.temporal_value_from_string(TemporalKind::PlainDateTime, &source)?;
        Ok(true)
    }

    /// The calendar-agnostic gate shared by `add`/`subtract`/`round`/`total`/
    /// `compare`. Where the specification requires a `relativeTo` this engine
    /// cannot honour, the answer is a `RangeError`, never an approximation.
    fn temporal_duration_require_no_calendar_units(
        record: &blueice_ecma402::DurationRecord,
        units: &[rounding::TemporalUnit],
    ) -> Result<(), RuntimeError> {
        if Self::temporal_duration_largest_unit(record).is_calendar()
            || units.iter().any(|unit| unit.is_calendar())
        {
            return Err(RuntimeError::RangeError(
                "a Temporal.Duration with years, months or weeks needs a relativeTo anchor".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn temporal_duration_with(
        &mut self,
        receiver: &Value,
        like: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        if !matches!(like, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration.prototype.with requires a Duration-like object".into(),
            ));
        }
        let mut fields = [
            record.years,
            record.months,
            record.weeks,
            record.days,
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        ];
        let mut present = false;
        for (name, index) in DURATION_FIELDS_IN_READ_ORDER {
            let value = self.get_property(like, &name.into())?;
            if value == Value::Undefined {
                continue;
            }
            present = true;
            fields[index] = self.temporal_duration_integer(&value, name)?;
        }
        if !present {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration.prototype.with requires at least one duration field".into(),
            ));
        }
        self.temporal_duration_create(fields)
    }

    pub(super) fn temporal_duration_negated(
        &mut self,
        receiver: &Value,
        absolute: bool,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let map = |value: i128| if absolute { value.abs() } else { -value };
        // Negating or taking the magnitude of every field at once preserves
        // both the common-sign and the range invariants, so this cannot fail.
        self.alloc_temporal_value(
            Self::temporal_duration_value(blueice_ecma402::DurationRecord {
                years: map(record.years),
                months: map(record.months),
                weeks: map(record.weeks),
                days: map(record.days),
                hours: map(record.hours),
                minutes: map(record.minutes),
                seconds: map(record.seconds),
                milliseconds: map(record.milliseconds),
                microseconds: map(record.microseconds),
                nanoseconds: map(record.nanoseconds),
            }),
            false,
        )
    }

    pub(super) fn temporal_duration_add(
        &mut self,
        receiver: &Value,
        other: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_duration_receiver(receiver)?;
        let mut two = self.temporal_duration_from_value(other)?;
        if negate {
            two = blueice_ecma402::DurationRecord {
                years: -two.years,
                months: -two.months,
                weeks: -two.weeks,
                days: -two.days,
                hours: -two.hours,
                minutes: -two.minutes,
                seconds: -two.seconds,
                milliseconds: -two.milliseconds,
                microseconds: -two.microseconds,
                nanoseconds: -two.nanoseconds,
            };
        }
        // `AddDurations` balances the sum up to the larger of the two
        // operands' own largest units — a calendar one has no fixed length,
        // so it is rejected outright rather than balanced.
        let largest = Self::temporal_duration_largest_unit(&one)
            .max(Self::temporal_duration_largest_unit(&two));
        Self::temporal_duration_require_no_calendar_units(&one, &[largest])?;
        Self::temporal_duration_require_no_calendar_units(&two, &[])?;
        let total = duration_math::TimeDuration::from_record_with_24_hour_days(&one)
            .total_nanoseconds()
            + duration_math::TimeDuration::from_record_with_24_hour_days(&two).total_nanoseconds();
        let balanced =
            duration_math::TimeDuration::from_nanoseconds(total).balance_with_days(largest);
        self.temporal_duration_create([
            0,
            0,
            0,
            balanced[0],
            balanced[1],
            balanced[2],
            balanced[3],
            balanced[4],
            balanced[5],
            balanced[6],
        ])
    }

    pub(super) fn temporal_duration_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let (shorthand, options) = self.temporal_duration_round_to(round_to, "round")?;
            // The specification reads every option, in alphabetical order,
            // before any algorithmic validation happens.
            let (requested_largest, anchored, increment, mode, requested_smallest) =
                match &shorthand {
                    Some(text) => (
                        UnitOption::Unset,
                        false,
                        1,
                        blueice_ecma402::NumberRoundingMode::HalfExpand,
                        UnitOption::Unit(Self::temporal_duration_unit_name(text, "smallestUnit")?),
                    ),
                    None => {
                        let largest =
                            self.temporal_duration_unit_option(&options, "largestUnit", true)?;
                        let relative_to = self.get_property(&options, &"relativeTo".into())?;
                        let anchored = self.temporal_duration_relative_to(&relative_to)?;
                        let increment = self.temporal_rounding_increment(&options)?;
                        let mode = self.temporal_rounding_mode(
                            &options,
                            blueice_ecma402::NumberRoundingMode::HalfExpand,
                        )?;
                        let smallest =
                            self.temporal_duration_unit_option(&options, "smallestUnit", false)?;
                        (largest, anchored, increment, mode, smallest)
                    }
                };
            if requested_largest == UnitOption::Unset && requested_smallest == UnitOption::Unset {
                return Err(RuntimeError::RangeError(
                    "Temporal.Duration.prototype.round requires largestUnit or smallestUnit".into(),
                ));
            }
            let smallest = requested_smallest
                .unit()
                .unwrap_or(rounding::TemporalUnit::Nanosecond);
            // A smallestUnit larger than the duration's own largest unit
            // raises the default largestUnit with it, so e.g. rounding
            // 86,399 seconds to days yields one day rather than zero.
            let default_largest = Self::temporal_duration_largest_unit(&record).max(smallest);
            let largest = requested_largest.unit().unwrap_or(default_largest);
            if smallest > largest {
                return Err(RuntimeError::RangeError(
                    "smallestUnit must not be larger than largestUnit".into(),
                ));
            }
            if let Some(maximum) = smallest.maximum_rounding_increment() {
                if increment >= maximum || maximum % increment != 0 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement does not divide evenly into the next larger unit".into(),
                    ));
                }
            }
            if increment > 1 && smallest != largest && smallest >= rounding::TemporalUnit::Day {
                return Err(RuntimeError::RangeError(
                    "a date-unit roundingIncrement above 1 cannot also balance to a larger unit"
                        .into(),
                ));
            }
            // A blank duration rounds to a blank duration in every unit: zero
            // is an exact multiple of any increment, and balancing zero
            // yields zero. Given an anchor, that is the whole answer even for
            // a calendar unit, with no calendar arithmetic involved.
            if anchored && record.sign() == 0 {
                return self.temporal_duration_create([0; 10]);
            }
            Self::temporal_duration_require_no_calendar_units(&record, &[largest, smallest])?;
            let step = smallest
                .nanoseconds()
                .expect("the calendar units were rejected above")
                * increment;
            let balanced = duration_math::TimeDuration::from_record_with_24_hour_days(&record)
                .rounded_to_step(step, mode)
                .balance_with_days(largest);
            self.temporal_duration_create([
                0,
                0,
                0,
                balanced[0],
                balanced[1],
                balanced[2],
                balanced[3],
                balanced[4],
                balanced[5],
                balanced[6],
            ])
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn temporal_duration_total(
        &mut self,
        receiver: &Value,
        total_of: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let (shorthand, options) = self.temporal_duration_round_to(total_of, "total")?;
            let (unit, anchored) = match &shorthand {
                Some(text) => (Self::temporal_duration_unit_name(text, "unit")?, false),
                None => {
                    let relative_to = self.get_property(&options, &"relativeTo".into())?;
                    let anchored = self.temporal_duration_relative_to(&relative_to)?;
                    let unit = self
                        .temporal_duration_unit_option(&options, "unit", false)?
                        .unit()
                        .ok_or_else(|| {
                            RuntimeError::RangeError(
                                "Temporal.Duration.prototype.total requires unit".into(),
                            )
                        })?;
                    (unit, anchored)
                }
            };
            // A blank duration totals zero in every unit; see `round` above.
            if anchored && record.sign() == 0 {
                return Ok(Value::Number(0.0));
            }
            Self::temporal_duration_require_no_calendar_units(&record, &[unit])?;
            Ok(Value::Number(
                duration_math::TimeDuration::from_record_with_24_hour_days(&record).total_in(unit),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn temporal_duration_compare(
        &mut self,
        one: &Value,
        two: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_duration_from_value(one)?;
        let two = self.temporal_duration_from_value(two)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_duration_options(options)?;
            let relative_to = self.get_property(&options, &"relativeTo".into())?;
            self.temporal_duration_relative_to(&relative_to)?;
            // Field-identical durations compare equal before any unit is
            // considered, so even a calendar-unit duration compares to itself.
            if one == two {
                return Ok(Value::Number(0.0));
            }
            Self::temporal_duration_require_no_calendar_units(&one, &[])?;
            Self::temporal_duration_require_no_calendar_units(&two, &[])?;
            let one = duration_math::TimeDuration::from_record_with_24_hour_days(&one)
                .total_nanoseconds();
            let two = duration_math::TimeDuration::from_record_with_24_hour_days(&two)
                .total_nanoseconds();
            Ok(Value::Number(match one.cmp(&two) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            }))
        })();
        self.stack.truncate(base);
        result
    }

    /// `GetTemporalFractionalSecondDigitsOption`: `auto` (the default, `None`
    /// here) or a digit count in `0..=9`. A non-Number value must stringify to
    /// exactly `"auto"`; a Number is floored rather than required to be
    /// integral.
    fn temporal_duration_fractional_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalSecondDigits".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        if !matches!(value, Value::Number(_)) {
            let text = self
                .coerce_string(&value)?
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError("invalid fractionalSecondDigits".into()))?;
            if text == "auto" {
                return Ok(None);
            }
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        let digits = self.coerce_number(&value)?;
        if !digits.is_finite() {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        let count = digits.floor();
        if !(0.0..=9.0).contains(&count) {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        Ok(Some(count as u8))
    }

    /// `TemporalDurationToString`. `precision` is the number of fractional
    /// second digits to emit, or `None` for `auto` (emit only as many as the
    /// value needs, and none at all for a whole number of seconds).
    fn format_duration_string(
        record: &blueice_ecma402::DurationRecord,
        precision: Option<u8>,
    ) -> String {
        let mut date = String::new();
        for (value, suffix) in [
            (record.years, 'Y'),
            (record.months, 'M'),
            (record.weeks, 'W'),
            (record.days, 'D'),
        ] {
            if value != 0 {
                date.push_str(&value.unsigned_abs().to_string());
                date.push(suffix);
            }
        }
        let mut time = String::new();
        for (value, suffix) in [(record.hours, 'H'), (record.minutes, 'M')] {
            if value != 0 {
                time.push_str(&value.unsigned_abs().to_string());
                time.push(suffix);
            }
        }
        // Seconds and every sub-second field are one exact quantity: 1,500
        // milliseconds serializes as `1.5S`, and 9,007,199,254,740,991
        // milliseconds must not lose precision on the way there.
        let subsecond_total = record.seconds * 1_000_000_000
            + record.milliseconds * 1_000_000
            + record.microseconds * 1_000
            + record.nanoseconds;
        let seconds = subsecond_total / 1_000_000_000;
        let fraction = (subsecond_total % 1_000_000_000).unsigned_abs();
        let only_seconds = date.is_empty() && time.is_empty();
        if seconds != 0 || fraction != 0 || only_seconds || precision.is_some() {
            time.push_str(&seconds.unsigned_abs().to_string());
            let digits = format!("{fraction:09}");
            match precision {
                None if fraction != 0 => {
                    time.push('.');
                    time.push_str(digits.trim_end_matches('0'));
                }
                Some(count) if count > 0 => {
                    time.push('.');
                    time.push_str(&digits[..usize::from(count)]);
                }
                _ => {}
            }
            time.push('S');
        }
        let mut result = String::new();
        if record.sign() < 0 {
            result.push('-');
        }
        result.push('P');
        result.push_str(&date);
        if !time.is_empty() {
            result.push('T');
            result.push_str(&time);
        }
        result
    }

    pub(super) fn temporal_duration_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_duration_options(options)?;
            let digits = self.temporal_duration_fractional_digits(&options)?;
            let mode =
                self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
            let smallest = self.temporal_duration_unit_option(&options, "smallestUnit", false)?;
            // `ToSecondsStringPrecisionRecord`: a smallestUnit pins both the
            // emitted digit count and the rounding unit; a digit count alone
            // pins the digits and derives a unit plus increment from them.
            let (precision, unit, increment) = match smallest.unit() {
                Some(rounding::TemporalUnit::Second) => {
                    (Some(0), rounding::TemporalUnit::Second, 1)
                }
                Some(rounding::TemporalUnit::Millisecond) => {
                    (Some(3), rounding::TemporalUnit::Millisecond, 1)
                }
                Some(rounding::TemporalUnit::Microsecond) => {
                    (Some(6), rounding::TemporalUnit::Microsecond, 1)
                }
                Some(rounding::TemporalUnit::Nanosecond) => {
                    (Some(9), rounding::TemporalUnit::Nanosecond, 1)
                }
                Some(_) => {
                    return Err(RuntimeError::RangeError(
                        "Temporal.Duration.prototype.toString accepts a smallestUnit of second or \
                         smaller"
                            .into(),
                    ));
                }
                None => match digits {
                    None => (None, rounding::TemporalUnit::Nanosecond, 1),
                    Some(0) => (Some(0), rounding::TemporalUnit::Second, 1),
                    Some(count @ 1..=3) => (
                        Some(count),
                        rounding::TemporalUnit::Millisecond,
                        10_i128.pow(u32::from(3 - count)),
                    ),
                    Some(count @ 4..=6) => (
                        Some(count),
                        rounding::TemporalUnit::Microsecond,
                        10_i128.pow(u32::from(6 - count)),
                    ),
                    Some(count) => (
                        Some(count),
                        rounding::TemporalUnit::Nanosecond,
                        10_i128.pow(u32::from(9 - count)),
                    ),
                },
            };
            if unit == rounding::TemporalUnit::Nanosecond && increment == 1 {
                // Nothing to round: serialize the record exactly as stored,
                // which is what keeps a maximal seconds-plus-nanoseconds pair
                // in range instead of balancing it out of range.
                return Ok(Value::String(
                    Self::format_duration_string(&record, precision).into(),
                ));
            }
            // Rounding the time part can carry into `days`, but never past
            // them: `largestUnit` here is the duration's own largest unit (at
            // least `second`), and the date fields are carried through
            // untouched.
            let largest =
                Self::temporal_duration_largest_unit(&record).max(rounding::TemporalUnit::Second);
            let step = unit
                .nanoseconds()
                .expect("second and smaller units have an exact length")
                * increment;
            let balanced = duration_math::TimeDuration::from_fields(
                record.hours,
                record.minutes,
                record.seconds,
                record.milliseconds,
                record.microseconds,
                record.nanoseconds,
            )
            .rounded_to_step(step, mode)
            .balance_with_days(largest.min(rounding::TemporalUnit::Day));
            let rounded = Self::temporal_duration_record([
                record.years,
                record.months,
                record.weeks,
                record.days + balanced[0],
                balanced[1],
                balanced[2],
                balanced[3],
                balanced[4],
                balanced[5],
                balanced[6],
            ])?;
            Ok(Value::String(
                Self::format_duration_string(&rounded, precision).into(),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    /// ECMA-402's `Temporal.Duration.prototype.toLocaleString`: build an
    /// `Intl.DurationFormat` from the same `(locales, options)` arguments and
    /// format this duration with it, rather than returning the ISO string
    /// ECMA-262's own non-402 definition would.
    pub(super) fn temporal_duration_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let formatter = self.duration_format_for_locale_string(args)?;
            self.stack.push(formatter.clone());
            let duration =
                self.alloc_temporal_value(Self::temporal_duration_value(record), false)?;
            self.stack.push(duration.clone());
            self.duration_format_format(&formatter, &duration)
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn temporal_duration_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.Duration cannot be converted to a primitive value".into(),
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

/// Rewrites a value's ISO fields to the local wall-clock fields its
/// `epoch_nanoseconds` really has in `zone`. The ISO fields a `ZonedDateTime`
/// carries are local, so they need the offset the zone was really observing at
/// that instant — Track E's whole reason for existing.
fn temporal_set_local_fields(value: &mut TemporalValue, zone: &time_zone::TimeZone) {
    let offset = zone.offset_nanoseconds_for(&value.epoch_nanoseconds);
    let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
        epoch::instant_fields(&(&value.epoch_nanoseconds + BigInt::from(offset)));
    value.year = year;
    value.month = month;
    value.day = day;
    value.hour = hour;
    value.minute = minute;
    value.second = second;
    value.millisecond = millisecond;
    value.microsecond = microsecond;
    value.nanosecond = nanosecond;
}

/// Maps a host-neutral zone-resolution failure onto the `RangeError` the spec
/// raises for it.
fn temporal_resolution_error(_: time_zone::AmbiguousLocalTime) -> RuntimeError {
    RuntimeError::RangeError(
        "the local time is ambiguous or does not exist in this time zone".into(),
    )
}
