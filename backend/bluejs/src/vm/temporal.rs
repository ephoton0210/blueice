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
mod plain_date;
mod plain_month_day;
mod plain_year_month;
mod rounding;
mod time_zone;
mod time_zone_id;
mod zoned_date_time;

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
    days_in_month: u8,
    days_in_year: u16,
    in_leap_year: bool,
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
                    for (name, arity, method) in [
                        ("with", 1, NativeFunction::TemporalDateWith),
                        ("add", 1, NativeFunction::TemporalDateAdd),
                        ("subtract", 1, NativeFunction::TemporalDateSubtract),
                        ("until", 1, NativeFunction::TemporalDateUntil),
                        ("since", 1, NativeFunction::TemporalDateSince),
                        ("equals", 1, NativeFunction::TemporalDateEquals),
                        ("toString", 0, NativeFunction::TemporalDateToString),
                        ("toJSON", 0, NativeFunction::TemporalDateToJson),
                        (
                            "toLocaleString",
                            0,
                            NativeFunction::TemporalDateToLocaleString,
                        ),
                        ("valueOf", 0, NativeFunction::TemporalDateValueOf),
                    ] {
                        self.install_native(prototype, function_prototype, name, arity, method)?;
                    }
                    self.install_native(
                        constructor,
                        function_prototype,
                        "compare",
                        2,
                        NativeFunction::TemporalDateCompare(kind),
                    )?;
                    if kind == TemporalKind::PlainDate {
                        self.install_native(
                            prototype,
                            function_prototype,
                            "toPlainDateTime",
                            0,
                            NativeFunction::TemporalPlainDateToPlainDateTime,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "toPlainYearMonth",
                            0,
                            NativeFunction::TemporalPlainDateToPlainYearMonth,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "toPlainMonthDay",
                            0,
                            NativeFunction::TemporalPlainDateToPlainMonthDay,
                        )?;
                    } else {
                        self.install_native(
                            prototype,
                            function_prototype,
                            "toPlainDate",
                            0,
                            NativeFunction::TemporalPlainDateTimeToPlainDate,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "toPlainTime",
                            0,
                            NativeFunction::TemporalPlainDateTimeToPlainTime,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "withPlainTime",
                            0,
                            NativeFunction::TemporalPlainDateTimeWithPlainTime,
                        )?;
                        self.install_native(
                            prototype,
                            function_prototype,
                            "round",
                            1,
                            NativeFunction::TemporalPlainDateTimeRound,
                        )?;
                    }
                }
                if kind == TemporalKind::PlainYearMonth {
                    for (name, arity, method) in [
                        ("with", 1, NativeFunction::TemporalYearMonthWith),
                        ("add", 1, NativeFunction::TemporalYearMonthAdd),
                        ("subtract", 1, NativeFunction::TemporalYearMonthSubtract),
                        ("until", 1, NativeFunction::TemporalYearMonthUntil),
                        ("since", 1, NativeFunction::TemporalYearMonthSince),
                        ("equals", 1, NativeFunction::TemporalYearMonthEquals),
                        ("toString", 0, NativeFunction::TemporalYearMonthToString),
                        ("toJSON", 0, NativeFunction::TemporalYearMonthToJson),
                        (
                            "toLocaleString",
                            0,
                            NativeFunction::TemporalYearMonthToLocaleString,
                        ),
                        ("valueOf", 0, NativeFunction::TemporalYearMonthValueOf),
                        (
                            "toPlainDate",
                            1,
                            NativeFunction::TemporalYearMonthToPlainDate,
                        ),
                    ] {
                        self.install_native(prototype, function_prototype, name, arity, method)?;
                    }
                    self.install_native(
                        constructor,
                        function_prototype,
                        "compare",
                        2,
                        NativeFunction::TemporalYearMonthCompare,
                    )?;
                }
                if kind == TemporalKind::PlainMonthDay {
                    // Deliberately no `add`/`subtract`/`until`/`since`/
                    // `compare` -- see `NativeFunction::TemporalMonthDay*`'s
                    // own doc comment for why this type has none.
                    for (name, arity, method) in [
                        ("with", 1, NativeFunction::TemporalMonthDayWith),
                        ("equals", 1, NativeFunction::TemporalMonthDayEquals),
                        ("toString", 0, NativeFunction::TemporalMonthDayToString),
                        ("toJSON", 0, NativeFunction::TemporalMonthDayToJson),
                        (
                            "toLocaleString",
                            0,
                            NativeFunction::TemporalMonthDayToLocaleString,
                        ),
                        ("valueOf", 0, NativeFunction::TemporalMonthDayValueOf),
                        (
                            "toPlainDate",
                            1,
                            NativeFunction::TemporalMonthDayToPlainDate,
                        ),
                    ] {
                        self.install_native(prototype, function_prototype, name, arity, method)?;
                    }
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
                        ("dayOfWeek", native::TemporalGetter::DayOfWeek),
                        ("dayOfYear", native::TemporalGetter::DayOfYear),
                        ("weekOfYear", native::TemporalGetter::WeekOfYear),
                        ("yearOfWeek", native::TemporalGetter::YearOfWeek),
                        ("daysInWeek", native::TemporalGetter::DaysInWeek),
                        ("daysInMonth", native::TemporalGetter::DaysInMonth),
                        ("daysInYear", native::TemporalGetter::DaysInYear),
                        ("inLeapYear", native::TemporalGetter::InLeapYear),
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
                        ("year", native::TemporalGetter::Year),
                        ("month", native::TemporalGetter::Month),
                        ("monthCode", native::TemporalGetter::MonthCode),
                        ("day", native::TemporalGetter::Day),
                        ("era", native::TemporalGetter::Era),
                        ("eraYear", native::TemporalGetter::EraYear),
                        ("monthsInYear", native::TemporalGetter::MonthsInYear),
                        ("hour", native::TemporalGetter::Hour),
                        ("minute", native::TemporalGetter::Minute),
                        ("second", native::TemporalGetter::Second),
                        ("millisecond", native::TemporalGetter::Millisecond),
                        ("microsecond", native::TemporalGetter::Microsecond),
                        ("nanosecond", native::TemporalGetter::Nanosecond),
                        ("dayOfWeek", native::TemporalGetter::DayOfWeek),
                        ("dayOfYear", native::TemporalGetter::DayOfYear),
                        ("weekOfYear", native::TemporalGetter::WeekOfYear),
                        ("yearOfWeek", native::TemporalGetter::YearOfWeek),
                        ("daysInWeek", native::TemporalGetter::DaysInWeek),
                        ("daysInMonth", native::TemporalGetter::DaysInMonth),
                        ("daysInYear", native::TemporalGetter::DaysInYear),
                        ("inLeapYear", native::TemporalGetter::InLeapYear),
                        (
                            "offsetNanoseconds",
                            native::TemporalGetter::OffsetNanoseconds,
                        ),
                        ("offset", native::TemporalGetter::Offset),
                        ("hoursInDay", native::TemporalGetter::HoursInDay),
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
                if kind == TemporalKind::PlainDateTime {
                    // A `PlainDateTime` carries a time of day too, on top of
                    // the calendar-date getters shared with `PlainDate`
                    // above.
                    for (name, getter) in [
                        ("hour", native::TemporalGetter::Hour),
                        ("minute", native::TemporalGetter::Minute),
                        ("second", native::TemporalGetter::Second),
                        ("millisecond", native::TemporalGetter::Millisecond),
                        ("microsecond", native::TemporalGetter::Microsecond),
                        ("nanosecond", native::TemporalGetter::Nanosecond),
                    ] {
                        self.install_getter(
                            prototype,
                            function_prototype,
                            name.into(),
                            &format!("get {name}"),
                            NativeFunction::TemporalGetter(getter),
                        )?;
                    }
                }
                if kind == TemporalKind::ZonedDateTime {
                    self.install_native(
                        prototype,
                        function_prototype,
                        "toLocaleString",
                        0,
                        NativeFunction::TemporalZonedDateTimeToLocaleString,
                    )?;
                    for (name, arity, method) in [
                        ("with", 1, NativeFunction::TemporalZonedDateTimeWith),
                        (
                            "withCalendar",
                            1,
                            NativeFunction::TemporalWithCalendar,
                        ),
                        (
                            "withTimeZone",
                            1,
                            NativeFunction::TemporalZonedDateTimeWithTimeZone,
                        ),
                        (
                            "withPlainTime",
                            0,
                            NativeFunction::TemporalZonedDateTimeWithPlainTime,
                        ),
                        ("add", 1, NativeFunction::TemporalZonedDateTimeAdd),
                        ("subtract", 1, NativeFunction::TemporalZonedDateTimeSubtract),
                        ("round", 1, NativeFunction::TemporalZonedDateTimeRound),
                        ("until", 1, NativeFunction::TemporalZonedDateTimeUntil),
                        ("since", 1, NativeFunction::TemporalZonedDateTimeSince),
                        ("equals", 1, NativeFunction::TemporalZonedDateTimeEquals),
                        ("toString", 0, NativeFunction::TemporalZonedDateTimeToString),
                        ("toJSON", 0, NativeFunction::TemporalZonedDateTimeToJson),
                        ("valueOf", 0, NativeFunction::TemporalZonedDateTimeValueOf),
                        (
                            "toInstant",
                            0,
                            NativeFunction::TemporalZonedDateTimeToInstant,
                        ),
                        (
                            "toPlainDate",
                            0,
                            NativeFunction::TemporalZonedDateTimeToPlainDate,
                        ),
                        (
                            "toPlainTime",
                            0,
                            NativeFunction::TemporalZonedDateTimeToPlainTime,
                        ),
                        (
                            "toPlainDateTime",
                            0,
                            NativeFunction::TemporalZonedDateTimeToPlainDateTime,
                        ),
                        (
                            "toPlainYearMonth",
                            0,
                            NativeFunction::TemporalZonedDateTimeToPlainYearMonth,
                        ),
                        (
                            "toPlainMonthDay",
                            0,
                            NativeFunction::TemporalZonedDateTimeToPlainMonthDay,
                        ),
                        (
                            "startOfDay",
                            0,
                            NativeFunction::TemporalZonedDateTimeStartOfDay,
                        ),
                        (
                            "getISOFields",
                            0,
                            NativeFunction::TemporalZonedDateTimeGetIsoFields,
                        ),
                    ] {
                        self.install_native(prototype, function_prototype, name, arity, method)?;
                    }
                    self.install_native(
                        constructor,
                        function_prototype,
                        "compare",
                        2,
                        NativeFunction::TemporalZonedDateTimeCompare,
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
                        (
                            "toLocaleString",
                            0,
                            NativeFunction::TemporalPlainTimeToLocaleString,
                        ),
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

    /// `ToIntegerWithTruncation`, plus a range check: every Temporal numeric
    /// date/time field (year, month, day, era year, hour, ...) uses this
    /// same conversion in every context — constructor argument or
    /// property-bag field — truncating a fractional value toward zero
    /// rather than rejecting it (Test262's `PlainDate/argument-convert.js`,
    /// `PlainDate/prototype/with/order-of-operations.js`'s `year: 1.7`).
    fn temporal_integer(
        &mut self,
        value: &Value,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        let value = self.coerce_number(value)?;
        let value = value.trunc();
        if !value.is_finite() || !(f64::from(minimum)..=f64::from(maximum)).contains(&value) {
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

    /// The raw `Temporal.PlainDate`/`PlainDateTime`/etc. **constructor**'s
    /// own positional `calendar` argument: a bare calendar ID string only.
    /// Test262's `calendar-invalid-iso-string.js` confirms a full
    /// date-with-annotation string (`"1997-12-04[u-ca=iso8601]"`) is a
    /// `RangeError` here specifically, unlike [`Self::temporal_calendar`]'s
    /// wider grammar below.
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

    /// `ToTemporalCalendarIdentifier`, used everywhere *other* than the raw
    /// constructor's own positional `calendar` argument: a property-bag
    /// `calendar` field (`Temporal.PlainDate.from({..., calendar})`) or a
    /// `Temporal.PlainDate.prototype.withCalendar` argument may themselves
    /// be a full date/date-time/offset/time/year-month/month-day string,
    /// not only a bare calendar ID — Test262's `since/calendar-id-match.js`
    /// passes `"2024-05-16[u-ca=iso8601]"` as a property-bag `calendar`
    /// value, `withCalendar/calendar-time-string.js` passes
    /// `"T11:30[u-ca=hebrew]"` to `withCalendar`, and (found via a real
    /// `equals/argument-propertybag-calendar-iso-string.js` failure) an
    /// *unannotated* ISO string like `"2020-01-01"` or `"2020-01"` is
    /// equally valid and always means `"iso8601"` — the calendar defaults
    /// to `iso8601` whenever no `[u-ca=...]` annotation is present, per
    /// `ParseISODateTime`. Tries every ISO string production this crate has
    /// a parser for (date-time, year-month, month-day, time — matching
    /// `TemporalCalendarString`'s own grammar alternation), extracting the
    /// first `u-ca=` annotation from whichever one matches; only when
    /// *none* of them parse does this fall back to a bare calendar ID
    /// lookup (an actual calendar ID like `"gregory"` never matches any of
    /// those productions, so the two paths never compete).
    fn temporal_calendar_identifier(&mut self, value: &Value) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok("iso8601".into());
        }
        let value = self.coerce_string(value)?;
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar".into()))?;
        let parsed_calendar = iso::parse_date_time(&value)
            .or_else(|| iso::parse_year_month(&value))
            .or_else(|| iso::parse_month_day(&value))
            .or_else(|| iso::parse_time(&value))
            .map(|parsed| parsed.calendar);
        if let Some(calendar) = parsed_calendar {
            let calendar = calendar.as_deref().unwrap_or("iso8601");
            return canonical_calendar_id(calendar)
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal calendar".into()));
        }
        canonical_calendar_id(&value)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal calendar".into()))
    }

    fn temporal_calendar_fields(
        &self,
        value: &TemporalValue,
    ) -> Result<TemporalCalendarFields, RuntimeError> {
        // The `"iso8601"` calendar is a fast path that deliberately never
        // reaches `icu_calendar::Date::try_new_iso` below: that constructor
        // enforces `icu_calendar`'s own `CONSTRUCTOR_YEAR_RANGE` (-9999..=9999
        // in the pinned `icu_calendar`), which is far narrower than
        // Temporal's own representable range (roughly ±271,821 years,
        // enforced separately by `epoch::is_date_within_limits` at
        // construction time). Without this fast path, `.year`/`.month`/
        // `.day`/etc. getters on an in-range extreme-year ISO date -- one
        // that *constructed* successfully -- would throw a spurious
        // `RangeError` from this getter dispatch alone. ISO fields are
        // exactly the value's own stored ISO date by definition (no
        // conversion needed), and the ISO calendar has no eras and always
        // twelve months, so this bypasses `icu_calendar` entirely rather
        // than special-casing its error path.
        // See development/browser_core/phase-26-ecma262-temporal/PLAN.md.
        if value.calendar == "iso8601" {
            return Ok(TemporalCalendarFields {
                year: value.year,
                month: value.month,
                month_code: format!("M{:02}", value.month),
                day: value.day,
                era: None,
                era_year: None,
                months_in_year: 12,
                days_in_month: plain_date::iso_days_in_month(value.year, value.month),
                days_in_year: if plain_date::is_iso_leap_year(value.year) {
                    366
                } else {
                    365
                },
                in_leap_year: plain_date::is_iso_leap_year(value.year),
            });
        }
        let calendar = calendar::calendar_kind(&value.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let iso = Date::try_new_iso(value.year, value.month, value.day)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal ISO date".into()))?;
        let date = iso.to_calendar(AnyCalendar::new(calendar));
        let year = date.year();
        let month = date.month();
        // The `iso8601` calendar never reaches this point (see the fast
        // path above), so every calendar here keeps ICU4X's own era, if any.
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
            days_in_month: date.days_in_month(),
            days_in_year: date.days_in_year(),
            in_leap_year: date.is_in_leap_year(),
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
        reject: bool,
    ) -> Result<TemporalValue, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar_identifier(&calendar_value)?;
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
        options.overflow = Some(if reject {
            icu_calendar::options::Overflow::Reject
        } else {
            icu_calendar::options::Overflow::Constrain
        });
        let date = Date::try_from_fields(fields, options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        // `year` is checked against the resolved date to catch a `year`
        // that contradicts an also-supplied `era`/`eraYear` (which is what
        // actually drives field resolution whenever an era is present — see
        // the branch above). `month` is checked only when `monthCode` was
        // *also* supplied — `monthCode` wins field resolution (the branch
        // above), so an inconsistent plain `month` alongside it must still
        // be caught as a conflict (`with/overflow.js`'s `{ month: 5,
        // monthCode: "M06" }`); but when `month` is the *only* month field
        // given, `overflow: "constrain"` (the default) legitimately
        // resolves an out-of-range one to a different value (that same
        // fixture's `{ month: 13 }` constrains to `12`), so it must not be
        // re-validated as an "inconsistency" there.
        let actual_year = date.year().extended_year();
        let actual_month = date.month().ordinal;
        if requested_year.is_some_and(|year| year != actual_year)
            || (month_code.is_some() && requested_month.is_some_and(|month| month as u8 != actual_month))
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
                let zone = self.temporal_time_zone(native::argument(args, 1))?;
                // The constructor's own third positional argument is a bare
                // calendar ID (like `PlainDate`'s), not
                // `ToTemporalCalendarIdentifier`'s wider string grammar --
                // this was previously never read at all, silently ignoring
                // `new Temporal.ZonedDateTime(ns, tz, "gregory")`'s own
                // calendar.
                value.calendar = self.temporal_calendar(native::argument(args, 2))?;
                value.time_zone = zone.identifier();
                // The stored ISO fields are always the *local* wall-clock
                // ones a `ZonedDateTime` presents (`temporal_set_local_fields`'s
                // own doc comment) -- this was previously never called here,
                // silently leaving every numeric-constructor `ZonedDateTime`
                // at its `1970-01-01T00:00:00` field defaults regardless of
                // its real epoch/zone, which broke every getter
                // (`.year`/`.hour`/etc) on a directly-constructed value.
                temporal_set_local_fields(&mut value, &zone);
            }
            TemporalKind::PlainDate | TemporalKind::PlainDateTime => {
                // `temporal_integer` itself is `ToIntegerWithTruncation`, so
                // a fractional year/month/day truncates toward zero rather
                // than being rejected (Test262's `argument-convert.js`).
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
            // The `1972`/`1` reference-field hardcodes below are only
            // correct for the `iso8601` calendar (`ToTemporalMonthDay`'s own
            // literal `referenceISOYear = 1972`, mirrored here for
            // `PlainYearMonth`'s reference day). A non-`iso8601` calendar
            // string keeps its own parsed year/day instead, which
            // `Vm::temporal_to_plain_year_month`/`temporal_to_plain_month_day`
            // then re-resolves through `CalendarYearMonthFromFields`/
            // `CalendarMonthDayFromFields` -- discarding it unconditionally
            // here (as this function previously did) silently dropped a
            // non-ISO calendar string's own explicit year/day.
            year: if kind == TemporalKind::PlainMonthDay && calendar == "iso8601" {
                1972
            } else {
                year
            },
            month,
            day: if kind == TemporalKind::PlainYearMonth && calendar == "iso8601" {
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
        if kind == TemporalKind::PlainYearMonth {
            // `Temporal.PlainYearMonth.from` *is* `ToTemporalYearMonth`,
            // which (unlike `PlainDate`/`PlainDateTime`) is handled entirely
            // by one function rather than split between this generic
            // dispatcher's object/string branches below -- both a
            // property-bag object and a calendar-annotated string need the
            // same `CalendarYearMonthFromFields` re-resolution.
            let resolved = self.temporal_to_plain_year_month(value, options)?;
            return self.alloc_temporal_value(resolved, false);
        }
        if kind == TemporalKind::PlainMonthDay {
            // Same rationale as `PlainYearMonth` above, for
            // `ToTemporalMonthDay`.
            let resolved = self.temporal_to_plain_month_day(value, options)?;
            return self.alloc_temporal_value(resolved, false);
        }
        if kind == TemporalKind::ZonedDateTime {
            // `Temporal.ZonedDateTime.from` *is* `ToTemporalZonedDateTime`,
            // which (like `PlainYearMonth`/`PlainMonthDay` above) needs its
            // own dedicated conversion rather than the generic object/string
            // dispatcher below: a property bag needs a `timeZone` (and
            // optional `offset`) read alongside the calendar-date fields,
            // and a string needs a *mandatory* time-zone annotation resolved
            // through real zone/disambiguation logic -- neither of which the
            // generic dispatcher (built for the calendar-only plain types)
            // has any notion of.
            let resolved = self.temporal_to_zoned_date_time(value, options)?;
            return self.alloc_temporal_value(resolved, false);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == kind {
                    return self.alloc_temporal_value(temporal, false);
                }
            }
            if matches!(kind, TemporalKind::PlainDate | TemporalKind::PlainDateTime) {
                let resolved_options = self.temporal_options(options)?;
                let reject = self.temporal_overflow_option(&resolved_options)?;
                return self
                    .temporal_plain_date_from_fields(kind, value, reject)
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
        value.calendar = self.temporal_calendar_identifier(calendar)?;
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
                if !matches!(
                    value.kind,
                    TemporalKind::PlainTime
                        | TemporalKind::PlainDateTime
                        | TemporalKind::ZonedDateTime
                ) {
                    return Err(RuntimeError::TypeError(
                        "Temporal time-of-day getter requires a PlainTime, PlainDateTime or \
                         ZonedDateTime receiver"
                            .into(),
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
            native::TemporalGetter::DayOfWeek
            | native::TemporalGetter::DayOfYear
            | native::TemporalGetter::WeekOfYear
            | native::TemporalGetter::YearOfWeek
            | native::TemporalGetter::DaysInWeek => {
                if !matches!(
                    value.kind,
                    TemporalKind::PlainDate | TemporalKind::PlainDateTime | TemporalKind::ZonedDateTime
                ) {
                    return Err(RuntimeError::TypeError(
                        "Temporal ISO week-date getter requires a PlainDate, PlainDateTime or \
                         ZonedDateTime receiver"
                            .into(),
                    ));
                }
                // Calendar-invariant: Temporal's day-of-week/week-of-year
                // getters operate on the ISO representation for every
                // calendar, per the current spec revision.
                let date = (value.year, value.month, value.day);
                Ok(match getter {
                    native::TemporalGetter::DayOfWeek => {
                        Value::Number(plain_date::iso_day_of_week(date).into())
                    }
                    native::TemporalGetter::DayOfYear => {
                        Value::Number(plain_date::iso_day_of_year(date).into())
                    }
                    native::TemporalGetter::WeekOfYear => {
                        Value::Number(plain_date::iso_week_of_year(date).0.into())
                    }
                    native::TemporalGetter::YearOfWeek => {
                        Value::Number(plain_date::iso_week_of_year(date).1.into())
                    }
                    native::TemporalGetter::DaysInWeek => Value::Number(7.0),
                    _ => unreachable!("all ISO week-date getters are listed above"),
                })
            }
            native::TemporalGetter::OffsetNanoseconds | native::TemporalGetter::Offset => {
                if value.kind != TemporalKind::ZonedDateTime {
                    return Err(RuntimeError::TypeError(
                        "Temporal offset getter requires a ZonedDateTime receiver".into(),
                    ));
                }
                let zone = temporal_zoned_date_time_zone(&value);
                let offset = zone.offset_nanoseconds_for(&value.epoch_nanoseconds);
                Ok(if getter == native::TemporalGetter::OffsetNanoseconds {
                    Value::Number(offset as f64)
                } else {
                    Value::String(format_offset_nanoseconds_exact(offset).into())
                })
            }
            native::TemporalGetter::HoursInDay => {
                if value.kind != TemporalKind::ZonedDateTime {
                    return Err(RuntimeError::TypeError(
                        "Temporal hoursInDay getter requires a ZonedDateTime receiver".into(),
                    ));
                }
                let zone = temporal_zoned_date_time_zone(&value);
                let date = (value.year, value.month, value.day);
                let length = zoned_date_time::day_length_nanoseconds(&zone, date);
                Ok(Value::Number(length as f64 / 3_600_000_000_000.0))
            }
            getter => {
                if !matches!(
                    value.kind,
                    TemporalKind::PlainDate
                        | TemporalKind::PlainDateTime
                        | TemporalKind::PlainMonthDay
                        | TemporalKind::PlainYearMonth
                        | TemporalKind::ZonedDateTime
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
                                | TemporalKind::ZonedDateTime
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
                                | TemporalKind::ZonedDateTime
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
                                | TemporalKind::ZonedDateTime
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
                                | TemporalKind::ZonedDateTime
                        ) =>
                    {
                        Ok(Value::Number(fields.months_in_year.into()))
                    }
                    native::TemporalGetter::DaysInMonth
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::ZonedDateTime
                        ) =>
                    {
                        Ok(Value::Number(fields.days_in_month.into()))
                    }
                    native::TemporalGetter::DaysInYear
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::ZonedDateTime
                        ) =>
                    {
                        Ok(Value::Number(fields.days_in_year.into()))
                    }
                    native::TemporalGetter::InLeapYear
                        if matches!(
                            value.kind,
                            TemporalKind::PlainDate
                                | TemporalKind::PlainDateTime
                                | TemporalKind::ZonedDateTime
                        ) =>
                    {
                        Ok(Value::Bool(fields.in_leap_year))
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

    /// `Temporal.PlainTime.prototype.toLocaleString`, which is
    /// `CreateDateTimeFormat(locales, options, TIME, TIME)` followed by
    /// `FormatDateTime` — i.e. exactly what
    /// `new Intl.DateTimeFormat(locales, options).format(plainTime)`
    /// produces, so it is built from the same `Intl.DateTimeFormat` bridge
    /// `Instant`/`ZonedDateTime`'s own `toLocaleString` already use, rather
    /// than aliasing `toString`'s ISO serialization. `create_date_time_format`
    /// -> `date_time_format_format` finds the receiver's `TemporalValue` via
    /// `date_time_format_value`/`date_time_format_input` and routes a
    /// `PlainTime` through `DateTimeFormatInput::TemporalPlain` the same way
    /// a direct `Intl.DateTimeFormat.prototype.format` call already does —
    /// `temporal_format_options`'s `TemporalKind::PlainTime` arm clears date
    /// components/time zone name from the per-value resolved options.
    ///
    /// `required = TIME` is a *formatter-construction-time* rule, separate
    /// from that per-value pruning: it rejects a `dateStyle` option
    /// unconditionally, even with `timeStyle`/individual time fields also
    /// present, because `toLocaleString`'s own freshly-constructed formatter
    /// has no other value to format. Test262's `datestyle-and-timestyle.js`
    /// (`{ dateStyle, timeStyle }` together) pins this. This is deliberately
    /// *not* folded into `temporal_format_options`, which
    /// `Intl.DateTimeFormat.prototype.format`/`formatToParts`/range methods
    /// share too — those construct an ordinary (`required = ANY`) formatter
    /// first and may format *any* value with it, so `dateStyle` there is
    /// simply ignored once `timeStyle` (or another time field) also applies
    /// to a `PlainTime` argument, per
    /// `intl402/DateTimeFormat/prototype/format/
    /// temporal-plaintime-formatting-datetime-style.js` — folding this
    /// check in there regressed that fixture during development.
    pub(super) fn temporal_plain_time_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        // Brand check before any observable option read.
        self.temporal_plain_time_fields(receiver)?;
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
            if self.date_time_format_data(&formatter)?.options().date_style.is_some() {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainTime.prototype.toLocaleString does not accept a dateStyle option"
                        .into(),
                ));
            }
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
    }

    pub(super) fn temporal_plain_time_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainTime cannot be converted to a primitive value".into(),
        ))
    }

    // ---- Stage 2: Temporal.PlainDate / Temporal.PlainDateTime -----------

    fn temporal_date_value(kind: TemporalKind, calendar: String, date: epoch::CivilDate) -> TemporalValue {
        TemporalValue {
            kind,
            duration: None,
            year: date.0,
            month: date.1,
            day: date.2,
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

    fn temporal_date_time_value(
        kind: TemporalKind,
        calendar: String,
        date: epoch::CivilDate,
        time: epoch::CivilTime,
    ) -> TemporalValue {
        TemporalValue {
            kind,
            duration: None,
            year: date.0,
            month: date.1,
            day: date.2,
            hour: time.0,
            minute: time.1,
            second: time.2,
            millisecond: time.3,
            microsecond: time.4,
            nanosecond: time.5,
            epoch_nanoseconds: 0.into(),
            calendar,
            time_zone: "UTC".into(),
        }
    }

    /// Brand check shared by every `Temporal.PlainDate`/`PlainDateTime`
    /// prototype method (both kinds share one adapter layer, dispatched at
    /// runtime on the receiver's own `TemporalKind`, the same pattern
    /// `temporal_with_calendar`/`temporal_plain_to_zoned_date_time` already
    /// use).
    fn temporal_date_receiver(&mut self, receiver: &Value) -> Result<TemporalValue, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainDate/PlainDateTime method requires a matching receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainDate/PlainDateTime method requires a matching receiver".into(),
            )
        })?;
        if !matches!(value.kind, TemporalKind::PlainDate | TemporalKind::PlainDateTime) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDate/PlainDateTime method requires a matching receiver".into(),
            ));
        }
        Ok(value)
    }

    fn temporal_unit_to_date_unit(unit: rounding::TemporalUnit) -> plain_date::DateUnit {
        match unit {
            rounding::TemporalUnit::Year => plain_date::DateUnit::Year,
            rounding::TemporalUnit::Month => plain_date::DateUnit::Month,
            rounding::TemporalUnit::Week => plain_date::DateUnit::Week,
            _ => plain_date::DateUnit::Day,
        }
    }

    /// `ToTemporalDate`.
    pub(super) fn temporal_to_plain_date(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let resolved = match temporal.kind {
                    TemporalKind::PlainDate | TemporalKind::PlainDateTime => Some((
                        (temporal.year, temporal.month, temporal.day),
                        temporal.calendar.clone(),
                    )),
                    TemporalKind::ZonedDateTime => {
                        let offset = if temporal.time_zone == "UTC" {
                            0
                        } else {
                            iso::parse_offset_identifier_nanoseconds(&temporal.time_zone)
                                .ok_or_else(|| {
                                    RuntimeError::RangeError(
                                        "Temporal.PlainDate conversion supports UTC and fixed \
                                         offsets"
                                            .into(),
                                    )
                                })?
                        };
                        let local = &temporal.epoch_nanoseconds + BigInt::from(offset);
                        Some((epoch::instant_fields(&local).0, temporal.calendar.clone()))
                    }
                    _ => None,
                };
                if let Some((date, calendar)) = resolved {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(Self::temporal_date_value(TemporalKind::PlainDate, calendar, date));
                }
            }
            let resolved_options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&resolved_options)?;
            return self.temporal_plain_date_from_fields(TemporalKind::PlainDate, value, reject);
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDate-like value must be an object or a string".into(),
            ));
        }
        let source = self
            .coerce_string(value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal.PlainDate string".into()))?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        self.temporal_value_from_string(TemporalKind::PlainDate, &source)
    }

    /// `ToTemporalDateTime`.
    pub(super) fn temporal_to_plain_date_time(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let resolved = match temporal.kind {
                    TemporalKind::PlainDateTime => Some((
                        (temporal.year, temporal.month, temporal.day),
                        (
                            temporal.hour,
                            temporal.minute,
                            temporal.second,
                            temporal.millisecond,
                            temporal.microsecond,
                            temporal.nanosecond,
                        ),
                        temporal.calendar.clone(),
                    )),
                    TemporalKind::PlainDate => Some((
                        (temporal.year, temporal.month, temporal.day),
                        (0, 0, 0, 0, 0, 0),
                        temporal.calendar.clone(),
                    )),
                    TemporalKind::ZonedDateTime => {
                        let offset = if temporal.time_zone == "UTC" {
                            0
                        } else {
                            iso::parse_offset_identifier_nanoseconds(&temporal.time_zone)
                                .ok_or_else(|| {
                                    RuntimeError::RangeError(
                                        "Temporal.PlainDateTime conversion supports UTC and \
                                         fixed offsets"
                                            .into(),
                                    )
                                })?
                        };
                        let local = &temporal.epoch_nanoseconds + BigInt::from(offset);
                        let (date, time) = epoch::instant_fields(&local);
                        Some((date, time, temporal.calendar.clone()))
                    }
                    _ => None,
                };
                if let Some((date, time, calendar)) = resolved {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(Self::temporal_date_time_value(
                        TemporalKind::PlainDateTime,
                        calendar,
                        date,
                        time,
                    ));
                }
            }
            let resolved_options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&resolved_options)?;
            return self.temporal_plain_date_from_fields(TemporalKind::PlainDateTime, value, reject);
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDateTime-like value must be an object or a string".into(),
            ));
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.PlainDateTime string".into())
        })?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        self.temporal_value_from_string(TemporalKind::PlainDateTime, &source)
    }

    fn temporal_to_matching(
        &mut self,
        value: &Value,
        kind: TemporalKind,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if kind == TemporalKind::PlainDate {
            self.temporal_to_plain_date(value, options)
        } else {
            self.temporal_to_plain_date_time(value, options)
        }
    }

    /// `Temporal.PlainDate.prototype.with`/`Temporal.PlainDateTime.prototype.with`.
    /// A property bag only: a `calendar`/`timeZone` property, or a
    /// Temporal-like object, is a `TypeError`; at least one recognized
    /// calendar/time field must be present.
    pub(super) fn temporal_date_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let like_object = like.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.with requires an object".into())
        })?;
        if self.heap.temporal_value(like_object)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal.with does not accept a Temporal-like object".into(),
            ));
        }
        for banned in ["calendar", "timeZone"] {
            if self.get_property(like, &banned.into())? != Value::Undefined {
                return Err(RuntimeError::TypeError(format!(
                    "Temporal.with does not accept a {banned} property"
                )));
            }
        }
        let existing_fields = self.temporal_calendar_fields(&existing)?;
        let year_v = self.get_property(like, &"year".into())?;
        let month_v = self.get_property(like, &"month".into())?;
        let month_code_v = self.get_property(like, &"monthCode".into())?;
        let day_v = self.get_property(like, &"day".into())?;
        let era_v = self.get_property(like, &"era".into())?;
        let era_year_v = self.get_property(like, &"eraYear".into())?;
        let (hour_v, minute_v, second_v, ms_v, us_v, ns_v) =
            if existing.kind == TemporalKind::PlainDateTime {
                (
                    self.get_property(like, &"hour".into())?,
                    self.get_property(like, &"minute".into())?,
                    self.get_property(like, &"second".into())?,
                    self.get_property(like, &"millisecond".into())?,
                    self.get_property(like, &"microsecond".into())?,
                    self.get_property(like, &"nanosecond".into())?,
                )
            } else {
                (
                    Value::Undefined,
                    Value::Undefined,
                    Value::Undefined,
                    Value::Undefined,
                    Value::Undefined,
                    Value::Undefined,
                )
            };
        let any_present = [
            &year_v, &month_v, &month_code_v, &day_v, &era_v, &era_year_v, &hour_v, &minute_v,
            &second_v, &ms_v, &us_v, &ns_v,
        ]
        .into_iter()
        .any(|value| *value != Value::Undefined);
        if !any_present {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let mut fields = DateFields::default();
        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| self.temporal_integer(&year_v, -9_999, 9_999, "year"))
            .transpose()?;
        let era_s = (!matches!(era_v, Value::Undefined))
            .then(|| self.coerce_string(&era_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
            })
            .transpose()?;
        let era_year_num = (!matches!(era_year_v, Value::Undefined))
            .then(|| self.temporal_integer(&era_year_v, -9_999, 9_999, "era year"))
            .transpose()?;
        // The `iso8601` calendar has no eras at all (per the fix in
        // `temporal_calendar_fields` above) — an `era`/`eraYear` property is
        // still read (for property-bag ordering) but never applied to field
        // resolution for it, matching Test262's
        // `with/time-units-ignored.js` (`{ day: 30, era: "BC" }` on an ISO
        // `PlainDate` simply changes `day`, `era` is inert).
        if let Some(era) = era_s.as_deref().filter(|_| existing.calendar != "iso8601") {
            fields.era = Some(era.as_bytes());
            fields.era_year = Some(era_year_num.or(existing_fields.era_year).ok_or_else(|| {
                RuntimeError::TypeError("Temporal eraYear requires an era".into())
            })?);
        } else if era_year_num.is_some() && existing.calendar != "iso8601" {
            return Err(RuntimeError::RangeError(
                "Temporal eraYear requires an era".into(),
            ));
        } else {
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        }

        let requested_month = (!matches!(month_v, Value::Undefined))
            .then(|| self.temporal_integer(&month_v, 1, 99, "month"))
            .transpose()?;
        let month_code_s = (!matches!(month_code_v, Value::Undefined))
            .then(|| self.coerce_string(&month_code_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;
        if let Some(month_code) = month_code_s.as_deref() {
            fields.month_code = Some(month_code.as_bytes());
        } else if let Some(month) = requested_month {
            fields.ordinal_month = Some(month as u8);
        } else {
            fields.month_code = Some(existing_fields.month_code.as_bytes());
        }
        let requested_day = (!matches!(day_v, Value::Undefined))
            .then(|| self.temporal_integer(&day_v, 1, 31, "day"))
            .transpose()?;
        fields.day = Some(requested_day.unwrap_or(i32::from(existing_fields.day)) as u8);

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(if reject {
            icu_calendar::options::Overflow::Reject
        } else {
            icu_calendar::options::Overflow::Constrain
        });
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        // See `temporal_plain_date_from_fields`'s identical check for why
        // `month` is only cross-checked when `monthCode` was also supplied.
        if requested_year.is_some_and(|year| year != date.year().extended_year())
            || (month_code_s.is_some()
                && requested_month.is_some_and(|month| month as u8 != date.month().ordinal))
        {
            return Err(RuntimeError::RangeError(
                "inconsistent Temporal calendar fields".into(),
            ));
        }
        let mut result =
            Self::temporal_value_from_calendar_date(existing.kind, existing.calendar.clone(), date);
        if existing.kind == TemporalKind::PlainDateTime {
            result.hour =
                self.temporal_optional_integer(&hour_v, i32::from(existing.hour), 0, 23, "hour")?
                    as u8;
            result.minute = self.temporal_optional_integer(
                &minute_v,
                i32::from(existing.minute),
                0,
                59,
                "minute",
            )? as u8;
            result.second = self.temporal_optional_integer(
                &second_v,
                i32::from(existing.second),
                0,
                59,
                "second",
            )? as u8;
            result.millisecond = self.temporal_optional_integer(
                &ms_v,
                i32::from(existing.millisecond),
                0,
                999,
                "millisecond",
            )? as u16;
            result.microsecond = self.temporal_optional_integer(
                &us_v,
                i32::from(existing.microsecond),
                0,
                999,
                "microsecond",
            )? as u16;
            result.nanosecond = self.temporal_optional_integer(
                &ns_v,
                i32::from(existing.nanosecond),
                0,
                999,
                "nanosecond",
            )? as u16;
        }
        self.alloc_temporal_value(result, false)
    }

    /// `Temporal.PlainDate.prototype.add`/`subtract`,
    /// `Temporal.PlainDateTime.prototype.add`/`subtract`. Years/months/weeks
    /// carry through the calendar first; every time-of-day unit (including a
    /// bare `days` field) then folds into a flat day/nanosecond offset —
    /// `PlainDate/prototype/add/balance-smaller-units.js` pins the 24-hour
    /// fold for a receiver with no time to preserve, and a `PlainDateTime`
    /// receiver's own time of day genuinely advances (with day carry) rather
    /// than being discarded.
    pub(super) fn temporal_date_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let mut duration = self.temporal_duration_from_value(duration_value)?;
        if negate {
            duration.years = -duration.years;
            duration.months = -duration.months;
            duration.weeks = -duration.weeks;
            duration.days = -duration.days;
            duration.hours = -duration.hours;
            duration.minutes = -duration.minutes;
            duration.seconds = -duration.seconds;
            duration.milliseconds = -duration.milliseconds;
            duration.microseconds = -duration.microseconds;
            duration.nanoseconds = -duration.nanoseconds;
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let start = (existing.year, existing.month, existing.day);
        let time_total = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        )
        .total_nanoseconds();
        const DAY_NS: i128 = 86_400_000_000_000;
        let (total_days, time_fields) = if existing.kind == TemporalKind::PlainDateTime {
            let existing_ns = duration_math::time_fields_to_nanoseconds(
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            );
            let combined = existing_ns + time_total;
            let day_carry = combined.div_euclid(DAY_NS);
            let ns_of_day = combined.rem_euclid(DAY_NS);
            (
                duration.days + day_carry,
                Some(duration_math::time_fields_from_nanoseconds(ns_of_day)),
            )
        } else {
            (duration.days + time_total / DAY_NS, None)
        };
        let result_date = plain_date::calendar_add_date(
            calendar_kind,
            start,
            duration.years as i64,
            duration.months as i64,
            duration.weeks as i64,
            total_days as i64,
            reject,
        )
        .ok_or_else(|| RuntimeError::RangeError("Temporal date arithmetic is out of range".into()))?;
        // `calendar_add_date` only range-checks via `regulate_iso_date`/
        // `balance_iso_date` (an i32-year/valid-month-day check), not
        // Temporal's own narrower representable range
        // (`-271821-04-19`..`+275760-09-13`, exclusive at the exact
        // day-and-nanosecond boundary for `PlainDateTime`) -- confirmed by a
        // real `add/limits.js` failure: subtracting one day from the exact
        // minimum `PlainDate` silently produced a valid-but-unrepresentable
        // `-271821-04-18` instead of throwing. `alloc_temporal_value`
        // performs no range validation of its own, matching the same gap
        // `Temporal.PlainDateTime.prototype.round` had.
        let in_range = match &time_fields {
            Some(time) => epoch::is_date_time_within_limits(result_date, *time),
            None => epoch::is_date_within_limits(result_date),
        };
        if !in_range {
            return Err(RuntimeError::RangeError(
                "Temporal date arithmetic is out of range".into(),
            ));
        }
        let value = match time_fields {
            Some(time) => {
                Self::temporal_date_time_value(existing.kind, existing.calendar.clone(), result_date, time)
            }
            None => Self::temporal_date_value(existing.kind, existing.calendar.clone(), result_date),
        };
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainDate.prototype.until`/`since`,
    /// `Temporal.PlainDateTime.prototype.until`/`since`.
    pub(super) fn temporal_date_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let other = self.temporal_to_matching(other_value, existing.kind, &Value::Undefined)?;
        if existing.calendar != other.calendar {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same calendar".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let largest_raw = self.temporal_raw_string_option(&resolved_options, "largestUnit")?;
        let increment_raw = self.temporal_raw_number_option(&resolved_options, "roundingIncrement")?;
        let mode_raw = self.temporal_raw_string_option(&resolved_options, "roundingMode")?;
        let smallest_raw = self.temporal_raw_string_option(&resolved_options, "smallestUnit")?;

        let unit_floor = if existing.kind == TemporalKind::PlainDateTime {
            rounding::TemporalUnit::Nanosecond
        } else {
            rounding::TemporalUnit::Day
        };
        let default_smallest = unit_floor;
        let smallest_unit = match smallest_raw.as_deref() {
            None => default_smallest,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        if smallest_unit < unit_floor {
            return Err(RuntimeError::RangeError(
                "smallestUnit is out of range for this receiver".into(),
            ));
        }
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => smallest_unit.max(rounding::TemporalUnit::Day),
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if largest_unit < unit_floor {
            return Err(RuntimeError::RangeError(
                "largestUnit is out of range for this receiver".into(),
            ));
        }
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        let mode = Self::temporal_validated_rounding_mode(
            mode_raw.as_deref(),
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        // `DifferenceTemporalPlainDate`/`DifferenceTemporalPlainDateTime`
        // always compute `CalendarDateUntil(calendar, temporalDate, other,
        // largestUnit)` — i.e. always in the fixed receiver-to-argument
        // direction, exactly like `until` — and only negate the *resulting*
        // Duration afterward for `since` (step 10). This must not be
        // implemented by swapping which date is `from`/`to` and skipping the
        // negation: `CalendarDateUntil`'s own algorithm anchors on `from`'s
        // day-of-month while walking years/months, so it is not
        // anti-symmetric (`f(other, existing) != -f(existing, other)` in
        // general — verified against Test262's
        // `PlainDate/prototype/since/basic-gregory.js`, whose "23 years, 11
        // months and 29 days" case a swap-based `from`/`to` computes as 30
        // days instead of 29, because it anchors on the wrong date's day
        // field). `from`/`to` are therefore always `existing`/`other`, and
        // the whole result is negated below when `since` is true.
        let from = (existing.year, existing.month, existing.day);
        let to = (other.year, other.month, other.day);
        let from_time = (
            existing.hour, existing.minute, existing.second, existing.millisecond,
            existing.microsecond, existing.nanosecond,
        );
        let to_time = (
            other.hour, other.minute, other.second, other.millisecond, other.microsecond,
            other.nanosecond,
        );

        const DAY_NS: i128 = 86_400_000_000_000;
        let from_ns = duration_math::time_fields_to_nanoseconds(
            from_time.0, from_time.1, from_time.2, from_time.3, from_time.4, from_time.5,
        );
        let to_ns = duration_math::time_fields_to_nanoseconds(
            to_time.0, to_time.1, to_time.2, to_time.3, to_time.4, to_time.5,
        );
        let mut time_diff = to_ns - from_ns;
        let date_sign = match plain_date::compare_iso_date(from, to) {
            std::cmp::Ordering::Less => 1_i64,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal => 0,
        };
        let mut adjusted_to = to;
        if time_diff != 0 && date_sign != 0 && time_diff.signum() != i128::from(date_sign) {
            adjusted_to = plain_date::add_iso_date(to, 0, 0, 0, -date_sign, false)
                .expect("shifting by one day never overflows a representable date");
            time_diff += i128::from(date_sign) * DAY_NS;
        }

        let (years, months, weeks, days, time_fields) = if smallest_unit >= rounding::TemporalUnit::Day
        {
            let (years, months, weeks, days) = plain_date::round_calendar_duration(
                calendar_kind,
                from,
                adjusted_to,
                Self::temporal_unit_to_date_unit(largest_unit),
                Self::temporal_unit_to_date_unit(smallest_unit),
                increment,
                mode,
            );
            (years, months, weeks, days, None)
        } else {
            let time_unit = match smallest_unit {
                rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
                rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
                rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
                rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
                rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
                _ => rounding::TimeUnit::Nanosecond,
            };
            let rounded = duration_math::TimeDuration::from_nanoseconds(time_diff)
                .round(time_unit, increment, mode);
            // This is a *duration* (signed magnitude), not a wall-clock time
            // of day, so the day/time split must be sign-consistent
            // (truncating toward zero) rather than the `div_euclid`/
            // `rem_euclid` wraparound `temporal_date_add`/`toString`/`round`
            // use elsewhere for an actual date+time point — otherwise a
            // negative difference's `days` field could end up negative while
            // its time fields stayed non-negative, which
            // `DurationRecord::try_new`'s common-sign rule rejects.
            let total = rounded.total_nanoseconds();
            let day_carry = total / DAY_NS;
            let ns_of_day = total % DAY_NS;
            let time_largest = if largest_unit >= rounding::TemporalUnit::Day {
                rounding::TimeUnit::Hour
            } else {
                match largest_unit {
                    rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
                    rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
                    rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
                    rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
                    rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
                    _ => rounding::TimeUnit::Nanosecond,
                }
            };
            let balanced =
                duration_math::TimeDuration::from_nanoseconds(ns_of_day).balance_to(time_largest);
            let (_, _, _, whole_days) = plain_date::calendar_difference_date(
                calendar_kind,
                from,
                adjusted_to,
                plain_date::DateUnit::Day,
            );
            let total_days = whole_days + day_carry as i64;
            let day_target = plain_date::calendar_add_date(calendar_kind, from, 0, 0, 0, total_days, false)
                .expect("a rounded day-count from a representable date stays representable");
            let (y, m, w, d) = plain_date::calendar_difference_date(
                calendar_kind,
                from,
                day_target,
                Self::temporal_unit_to_date_unit(largest_unit),
            );
            (y, m, w, d, Some(balanced))
        };

        let (hours, minutes, seconds, milliseconds, microseconds, nanoseconds) =
            time_fields.map_or((0, 0, 0, 0, 0, 0), |fields: [i64; 6]| {
                (fields[0], fields[1], fields[2], fields[3], fields[4], fields[5])
            });
        // Step 10 of `DifferenceTemporalPlainDate`/`DifferenceTemporalPlainDateTime`:
        // the whole `years`..`nanoseconds` computation above is always in the
        // fixed `existing` (receiver) -> `other` (argument) direction — see
        // the comment on `from`/`to` above — so `since` negates every field
        // of the finished result rather than the inputs to the computation.
        let (years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds, nanoseconds) =
            if since {
                (
                    -years, -months, -weeks, -days, -hours, -minutes, -seconds, -milliseconds,
                    -microseconds, -nanoseconds,
                )
            } else {
                (
                    years, months, weeks, days, hours, minutes, seconds, milliseconds,
                    microseconds, nanoseconds,
                )
            };
        let record = blueice_ecma402::DurationRecord::try_new(
            i128::from(years),
            i128::from(months),
            i128::from(weeks),
            i128::from(days),
            i128::from(hours),
            i128::from(minutes),
            i128::from(seconds),
            i128::from(milliseconds),
            i128::from(microseconds),
            i128::from(nanoseconds),
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    pub(super) fn temporal_date_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let other = self.temporal_to_matching(other_value, existing.kind, &Value::Undefined)?;
        let mut equal = existing.year == other.year
            && existing.month == other.month
            && existing.day == other.day
            && existing.calendar == other.calendar;
        if equal && existing.kind == TemporalKind::PlainDateTime {
            equal = existing.hour == other.hour
                && existing.minute == other.minute
                && existing.second == other.second
                && existing.millisecond == other.millisecond
                && existing.microsecond == other.microsecond
                && existing.nanosecond == other.nanosecond;
        }
        Ok(Value::Bool(equal))
    }

    pub(super) fn temporal_date_compare(
        &mut self,
        kind: TemporalKind,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let a = self.temporal_to_matching(one, kind, &Value::Undefined)?;
        let b = self.temporal_to_matching(two, kind, &Value::Undefined)?;
        let ord = (
            a.year, a.month, a.day, a.hour, a.minute, a.second, a.millisecond, a.microsecond,
            a.nanosecond,
        )
            .cmp(&(
                b.year, b.month, b.day, b.hour, b.minute, b.second, b.millisecond, b.microsecond,
                b.nanosecond,
            ));
        Ok(Value::Number(match ord {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    pub(super) fn temporal_date_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let resolved_options = self.temporal_options(options)?;
        let explicit_digits = self.temporal_fractional_second_digits(&resolved_options)?;
        let mode =
            self.temporal_rounding_mode(&resolved_options, blueice_ecma402::NumberRoundingMode::Trunc)?;
        let smallest_unit = self.temporal_unit_option(&resolved_options, "smallestUnit", false)?;
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", false)?;
        let show_calendar_raw = self.temporal_string_option(
            &resolved_options,
            "calendarName",
            &["auto", "always", "never", "critical"],
        )?;
        let show_calendar = show_calendar_raw
            .as_deref()
            .map(|value| plain_date::parse_show_calendar(value).expect("already validated"))
            .unwrap_or(plain_date::ShowCalendar::Auto);

        if existing.kind == TemporalKind::PlainDate {
            let mut result = plain_date::format_iso_date((existing.year, existing.month, existing.day));
            result.push_str(&plain_date::format_calendar_annotation(&existing.calendar, show_calendar));
            return Ok(Value::String(result.into()));
        }

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
                None => (SecondsPrecision::Auto, rounding::TimeUnit::Nanosecond, 1_i128),
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
        let time_ns = duration_math::time_fields_to_nanoseconds(
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        let rounded = duration_math::TimeDuration::from_nanoseconds(time_ns)
            .round(unit, increment, mode)
            .total_nanoseconds();
        const DAY_NS: i128 = 86_400_000_000_000;
        let day_carry = rounded.div_euclid(DAY_NS);
        let ns_of_day = rounded.rem_euclid(DAY_NS);
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let date = plain_date::calendar_add_date(
            calendar_kind,
            (existing.year, existing.month, existing.day),
            0,
            0,
            0,
            day_carry as i64,
            false,
        )
        .ok_or_else(|| RuntimeError::RangeError("Temporal.PlainDateTime.toString is out of range".into()))?;
        let (hour, minute, second, millisecond, microsecond, nanosecond) =
            duration_math::time_fields_from_nanoseconds(ns_of_day);
        let mut result = plain_date::format_iso_date(date);
        result.push_str(&format!("T{hour:02}:{minute:02}"));
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
        result.push_str(&plain_date::format_calendar_annotation(&existing.calendar, show_calendar));
        Ok(Value::String(result.into()))
    }

    pub(super) fn temporal_date_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.temporal_date_receiver(receiver)?;
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

    pub(super) fn temporal_date_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainDate/PlainDateTime cannot be converted to a primitive value".into(),
        ))
    }

    pub(super) fn temporal_plain_date_to_plain_date_time(
        &mut self,
        receiver: &Value,
        time_like: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let time = if *time_like == Value::Undefined {
            (0, 0, 0, 0, 0, 0)
        } else {
            self.temporal_to_plain_time(time_like, &Value::Undefined)?
        };
        let value = Self::temporal_date_time_value(
            TemporalKind::PlainDateTime,
            existing.calendar,
            (existing.year, existing.month, existing.day),
            time,
        );
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainDate.prototype.toPlainYearMonth`: resolves through
    /// `CalendarYearMonthFromFields` (Phase 26 Stage 2's
    /// `plain_year_month.rs`), correct for every calendar -- this used to
    /// pin the ISO reference day at `1` unconditionally, which is only
    /// correct for the `iso8601` calendar.
    pub(super) fn temporal_plain_date_to_plain_year_month(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let fields = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let ym_fields = plain_year_month::YearMonthFields {
            era: None,
            era_year: None,
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &ym_fields, false)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar year-month".into())
            })?;
        let value = Self::temporal_date_value(TemporalKind::PlainYearMonth, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainDate.prototype.toPlainMonthDay`: resolves through
    /// `CalendarMonthDayFromFields` (Phase 26 Stage 2's
    /// `plain_month_day.rs`), correct for every calendar -- this used to pin
    /// the ISO reference year at `1972` unconditionally, which is only
    /// correct for the `iso8601` calendar.
    pub(super) fn temporal_plain_date_to_plain_month_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let fields = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let md_fields = plain_month_day::MonthDayFields {
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
            day: fields.day,
        };
        let date = plain_month_day::month_day_from_fields(calendar_kind, &md_fields, false)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar month-day".into())
            })?;
        let value = Self::temporal_date_value(TemporalKind::PlainMonthDay, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_plain_date_time_to_plain_date(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let value = Self::temporal_date_value(
            TemporalKind::PlainDate,
            existing.calendar,
            (existing.year, existing.month, existing.day),
        );
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_plain_date_time_to_plain_time(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let value = Self::plain_time_value((
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        ));
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_plain_date_time_with_plain_time(
        &mut self,
        receiver: &Value,
        time_like: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let time = if *time_like == Value::Undefined {
            (0, 0, 0, 0, 0, 0)
        } else {
            self.temporal_to_plain_time(time_like, &Value::Undefined)?
        };
        let value = Self::temporal_date_time_value(
            TemporalKind::PlainDateTime,
            existing.calendar,
            (existing.year, existing.month, existing.day),
            time,
        );
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_plain_date_time_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        if *round_to == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDateTime.round requires a smallestUnit or options argument".into(),
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
            // `PlainDateTime.prototype.round`'s `smallestUnit` spans
            // `"day"`..`"nanosecond"` (`RoundISODateTime`'s own unit range),
            // one wider than a bare `PlainTime`'s `"hour"`..`"nanosecond"` —
            // a real gap this fixed: every `smallestUnit: "day"` call
            // (`round/roundingmode-*.js`, `round/balance.js`,
            // `round/roundingincrement-one-day.js`, `round/limits.js`)
            // threw "invalid smallestUnit option" before this, since only
            // the narrower time-unit vocabulary was ever accepted.
            let smallest_unit_text = smallest_unit.as_deref().ok_or_else(|| {
                RuntimeError::RangeError(
                    "Temporal.PlainDateTime.round requires smallestUnit".into(),
                )
            })?;
            const DAY_NS: i128 = 86_400_000_000_000;
            let time_ns = duration_math::time_fields_to_nanoseconds(
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            );
            let (day_carry, ns_of_day) = if matches!(smallest_unit_text, "day" | "days") {
                // `ValidateTemporalRoundingIncrement(increment, 1, true)`:
                // day granularity has no finer subdivision to increment by
                // within this call (unlike `Temporal.Instant.round`'s own
                // day rule, which allows any divisor of a day) — only `1`
                // is ever valid.
                if increment != 1 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement must be 1 when smallestUnit is \"day\"".into(),
                    ));
                }
                let rounded = rounding::round_to_increment(time_ns, DAY_NS, mode);
                (rounded.div_euclid(DAY_NS), 0_i128)
            } else {
                let smallest_unit = rounding::parse_time_unit(smallest_unit_text).ok_or_else(|| {
                    RuntimeError::RangeError("invalid smallestUnit option".into())
                })?;
                Self::temporal_validated_plain_time_increment(increment, smallest_unit)?;
                let rounded = duration_math::TimeDuration::from_nanoseconds(time_ns)
                    .round(smallest_unit, increment, mode)
                    .total_nanoseconds();
                (rounded.div_euclid(DAY_NS), rounded.rem_euclid(DAY_NS))
            };
            let calendar_kind = calendar::calendar_kind(&existing.calendar)
                .expect("Temporal values retain a validated calendar identifier");
            let date = plain_date::calendar_add_date(
                calendar_kind,
                (existing.year, existing.month, existing.day),
                0,
                0,
                0,
                day_carry as i64,
                false,
            )
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal.PlainDateTime.round is out of range".into())
            })?;
            let time = duration_math::time_fields_from_nanoseconds(ns_of_day);
            // `calendar_add_date` only range-checks the *calendar date*
            // (year/month/day); a rounded result can still fall outside
            // Temporal's exact day-and-nanosecond `PlainDateTime` boundary
            // while landing on an otherwise-representable date -- e.g.
            // flooring `-271821-04-19T00:00:00.000000001` (the actual
            // minimum representable `PlainDateTime`) to any unit rounds
            // its single nanosecond away, landing exactly on
            // `-271821-04-19T00:00:00.000000000`, a representable *date*
            // but not a representable `PlainDateTime` (`PlainDateTime/
            // from/argument-string-limits.js`'s own boundary). Confirmed
            // by a real `round/limits.js` failure — `alloc_temporal_value`
            // performs no range validation of its own, unlike
            // `temporal_value_from_args`'s construction path.
            if !epoch::is_date_time_within_limits(date, time) {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDateTime.round is out of range".into(),
                ));
            }
            let value = Self::temporal_date_time_value(
                TemporalKind::PlainDateTime,
                existing.calendar.clone(),
                date,
                time,
            );
            self.alloc_temporal_value(value, false)
        })();
        self.stack.truncate(base);
        result
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
    /// `ToTemporalTime`, but optional: `undefined` means no `plainTime` was
    /// given at all (`toZonedDateTime`'s date-only fast path), which is
    /// distinct from a `PlainTime` whose fields happen to all be zero.
    ///
    /// This used to be its own hand-rolled subset (Temporal object/
    /// `PlainDateTime` only, a `RangeError` stub for a string or property
    /// bag) — left that way deliberately, per Phase 26's plan, until Stage 1
    /// Track D's real `Temporal.PlainTime` string/property-bag conversion
    /// landed. It has, as [`Self::temporal_to_plain_time`]; delegate to it
    /// instead of re-deriving the same conversion a second time.
    fn temporal_time_of_day(
        &mut self,
        value: &Value,
    ) -> Result<Option<epoch::CivilTime>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(None);
        }
        self.temporal_to_plain_time(value, &Value::Undefined).map(Some)
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

/// Phase 26 Stage 2's second `PlainYearMonth`/`PlainMonthDay` slice. Kept as
/// its own `impl Vm` block (rather than folded into the block above) so this
/// addition stays textually disjoint from the region a sibling worktree is
/// concurrently editing for `PlainDate`/`PlainDateTime` bug fixes -- per this
/// document's own repeatedly-recorded "git diff misalignment" merge pattern.
impl Vm {
    /// Brand check shared by every `Temporal.PlainYearMonth` prototype
    /// method, mirroring `temporal_date_receiver`'s own pattern for
    /// `PlainDate`/`PlainDateTime`.
    fn temporal_year_month_receiver(&mut self, receiver: &Value) -> Result<TemporalValue, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainYearMonth method requires a matching receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainYearMonth method requires a matching receiver".into(),
            )
        })?;
        if value.kind != TemporalKind::PlainYearMonth {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth method requires a matching receiver".into(),
            ));
        }
        Ok(value)
    }

    /// Same as [`Self::temporal_year_month_receiver`], for `PlainMonthDay`.
    fn temporal_month_day_receiver(&mut self, receiver: &Value) -> Result<TemporalValue, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainMonthDay method requires a matching receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainMonthDay method requires a matching receiver".into(),
            )
        })?;
        if value.kind != TemporalKind::PlainMonthDay {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay method requires a matching receiver".into(),
            ));
        }
        Ok(value)
    }

    /// `CalendarYearMonthFromFields`'s property-bag entry point --
    /// `Temporal.PlainYearMonth.from({...})` and the object branch of
    /// `ToTemporalYearMonth`.
    fn temporal_plain_year_month_from_fields(
        &mut self,
        bag: &Value,
        reject: bool,
    ) -> Result<TemporalValue, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar_identifier(&calendar_value)?;
        let year = self.get_property(bag, &"year".into())?;
        let month = self.get_property(bag, &"month".into())?;
        let month_code = self.get_property(bag, &"monthCode".into())?;
        let era = self.get_property(bag, &"era".into())?;
        let era_year = self.get_property(bag, &"eraYear".into())?;

        let requested_year = (!matches!(year, Value::Undefined))
            .then(|| self.temporal_integer(&year, -9_999, 9_999, "year"))
            .transpose()?;
        let era_s = (!matches!(era, Value::Undefined))
            .then(|| self.coerce_string(&era))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
            })
            .transpose()?;
        let era_year_num = if era_s.is_some() {
            Some(self.temporal_integer(&era_year, -9_999, 9_999, "era year")?)
        } else {
            if !matches!(era_year, Value::Undefined) {
                return Err(RuntimeError::RangeError(
                    "Temporal eraYear requires an era".into(),
                ));
            }
            None
        };
        if era_s.is_none() && requested_year.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth fields require year".into(),
            ));
        }

        let requested_month = (!matches!(month, Value::Undefined))
            .then(|| self.temporal_integer(&month, 1, 99, "month"))
            .transpose()?;
        let month_code_s = (!matches!(month_code, Value::Undefined))
            .then(|| self.coerce_string(&month_code))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;
        if month_code_s.is_none() && requested_month.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth fields require month or monthCode".into(),
            ));
        }

        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar_identifier validates the calendar identifier");
        let fields = plain_year_month::YearMonthFields {
            era: era_s.as_deref(),
            era_year: era_year_num,
            extended_year: requested_year,
            month_code: month_code_s.as_deref(),
            ordinal_month: requested_month.map(|value| value as u8),
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &fields, reject)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar year-month".into())
            })?;
        if !iso::is_year_month_within_limits(date.0, date.1) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth is outside the supported range".into(),
            ));
        }
        Ok(Self::temporal_date_value(TemporalKind::PlainYearMonth, calendar, date))
    }

    /// `CalendarMonthDayFromFields`'s property-bag entry point --
    /// `Temporal.PlainMonthDay.from({...})` and the object branch of
    /// `ToTemporalMonthDay`.
    fn temporal_plain_month_day_from_fields(
        &mut self,
        bag: &Value,
        reject: bool,
    ) -> Result<TemporalValue, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar_identifier(&calendar_value)?;
        let year = self.get_property(bag, &"year".into())?;
        let month = self.get_property(bag, &"month".into())?;
        let month_code = self.get_property(bag, &"monthCode".into())?;
        let day = self.get_property(bag, &"day".into())?;

        let requested_year = (!matches!(year, Value::Undefined))
            .then(|| self.temporal_integer(&year, -9_999, 9_999, "year"))
            .transpose()?;
        let requested_month = (!matches!(month, Value::Undefined))
            .then(|| self.temporal_integer(&month, 1, 99, "month"))
            .transpose()?;
        let month_code_s = (!matches!(month_code, Value::Undefined))
            .then(|| self.coerce_string(&month_code))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;
        if month_code_s.is_none() && requested_month.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require month or monthCode".into(),
            ));
        }
        // Per `MissingFieldsStrategy::Ecma`'s own documented rule, a
        // reference year is only derivable from `monthCode` + `day` -- an
        // ordinal `month`'s identity itself varies by year, so it cannot
        // resolve one. `ToTemporalMonthDay`'s own field set has no `era`,
        // so `year`/`monthCode` are the only two ways to avoid this.
        if requested_year.is_none() && month_code_s.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require monthCode or year".into(),
            ));
        }
        if matches!(day, Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require day".into(),
            ));
        }
        let day_num = self.temporal_integer(&day, 1, 31, "day")?;

        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar_identifier validates the calendar identifier");
        let fields = plain_month_day::MonthDayFields {
            extended_year: requested_year,
            month_code: month_code_s.as_deref(),
            ordinal_month: requested_month.map(|value| value as u8),
            day: day_num as u8,
        };
        let date = plain_month_day::month_day_from_fields(calendar_kind, &fields, reject)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar month-day".into())
            })?;
        if !epoch::is_date_within_limits(date) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainMonthDay is outside the supported range".into(),
            ));
        }
        Ok(Self::temporal_date_value(TemporalKind::PlainMonthDay, calendar, date))
    }

    /// `ToTemporalYearMonth`.
    pub(super) fn temporal_to_plain_year_month(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::PlainYearMonth {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(temporal);
                }
            }
            let resolved_options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&resolved_options)?;
            return self.temporal_plain_year_month_from_fields(value, reject);
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth-like value must be an object or a string".into(),
            ));
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.PlainYearMonth string".into())
        })?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        let parsed = self.temporal_value_from_string(TemporalKind::PlainYearMonth, &source)?;
        if parsed.calendar == "iso8601" {
            return Ok(parsed);
        }
        let fields = self.temporal_calendar_fields(&parsed)?;
        let calendar_kind = calendar::calendar_kind(&parsed.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let ym_fields = plain_year_month::YearMonthFields {
            era: None,
            era_year: None,
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &ym_fields, false)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar year-month".into())
            })?;
        Ok(Self::temporal_date_value(TemporalKind::PlainYearMonth, parsed.calendar, date))
    }

    /// `ToTemporalMonthDay`.
    pub(super) fn temporal_to_plain_month_day(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::PlainMonthDay {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(temporal);
                }
            }
            let resolved_options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&resolved_options)?;
            return self.temporal_plain_month_day_from_fields(value, reject);
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay-like value must be an object or a string".into(),
            ));
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.PlainMonthDay string".into())
        })?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        let parsed = self.temporal_value_from_string(TemporalKind::PlainMonthDay, &source)?;
        if parsed.calendar == "iso8601" {
            return Ok(parsed);
        }
        let fields = self.temporal_calendar_fields(&parsed)?;
        let calendar_kind = calendar::calendar_kind(&parsed.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let md_fields = plain_month_day::MonthDayFields {
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
            day: fields.day,
        };
        let date = plain_month_day::month_day_from_fields(calendar_kind, &md_fields, false)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar month-day".into())
            })?;
        Ok(Self::temporal_date_value(TemporalKind::PlainMonthDay, parsed.calendar, date))
    }

    /// `Temporal.PlainYearMonth.prototype.with`. Unlike
    /// `PlainDate`/`PlainDateTime.prototype.with`, only `year`/`month`/
    /// `monthCode` are recognized overrides -- `era`/`eraYear` are always
    /// carried through unchanged from the receiver (Gecko's own
    /// `PlainYearMonth_with` restricts `PreparePartialCalendarFields` to
    /// exactly this trio).
    pub(super) fn temporal_year_month_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let like_object = like
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Temporal.with requires an object".into()))?;
        if self.heap.temporal_value(like_object)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal.with does not accept a Temporal-like object".into(),
            ));
        }
        for banned in ["calendar", "timeZone"] {
            if self.get_property(like, &banned.into())? != Value::Undefined {
                return Err(RuntimeError::TypeError(format!(
                    "Temporal.with does not accept a {banned} property"
                )));
            }
        }
        let base = self.temporal_calendar_fields(&existing)?;
        let year_v = self.get_property(like, &"year".into())?;
        let month_v = self.get_property(like, &"month".into())?;
        let month_code_v = self.get_property(like, &"monthCode".into())?;
        if matches!(year_v, Value::Undefined)
            && matches!(month_v, Value::Undefined)
            && matches!(month_code_v, Value::Undefined)
        {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| self.temporal_integer(&year_v, -9_999, 9_999, "year"))
            .transpose()?;
        let requested_month = (!matches!(month_v, Value::Undefined))
            .then(|| self.temporal_integer(&month_v, 1, 99, "month"))
            .transpose()?;
        let month_code_s = (!matches!(month_code_v, Value::Undefined))
            .then(|| self.coerce_string(&month_code_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let month_code = month_code_s
            .clone()
            .or_else(|| requested_month.is_none().then(|| base.month_code.clone()));
        let fields = plain_year_month::YearMonthFields {
            era: None,
            era_year: None,
            extended_year: Some(requested_year.unwrap_or(base.year)),
            month_code: month_code.as_deref(),
            ordinal_month: requested_month.map(|value| value as u8),
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &fields, reject)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar year-month".into())
            })?;
        if !iso::is_year_month_within_limits(date.0, date.1) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth is outside the supported range".into(),
            ));
        }
        let value = Self::temporal_date_value(TemporalKind::PlainYearMonth, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainMonthDay.prototype.with`.
    pub(super) fn temporal_month_day_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
        let like_object = like
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Temporal.with requires an object".into()))?;
        if self.heap.temporal_value(like_object)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal.with does not accept a Temporal-like object".into(),
            ));
        }
        for banned in ["calendar", "timeZone"] {
            if self.get_property(like, &banned.into())? != Value::Undefined {
                return Err(RuntimeError::TypeError(format!(
                    "Temporal.with does not accept a {banned} property"
                )));
            }
        }
        let base = self.temporal_calendar_fields(&existing)?;
        let year_v = self.get_property(like, &"year".into())?;
        let month_v = self.get_property(like, &"month".into())?;
        let month_code_v = self.get_property(like, &"monthCode".into())?;
        let day_v = self.get_property(like, &"day".into())?;
        if [&year_v, &month_v, &month_code_v, &day_v]
            .into_iter()
            .all(|value| matches!(value, Value::Undefined))
        {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| self.temporal_integer(&year_v, -9_999, 9_999, "year"))
            .transpose()?;
        let requested_month = (!matches!(month_v, Value::Undefined))
            .then(|| self.temporal_integer(&month_v, 1, 99, "month"))
            .transpose()?;
        let month_code_s = (!matches!(month_code_v, Value::Undefined))
            .then(|| self.coerce_string(&month_code_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;
        let requested_day = (!matches!(day_v, Value::Undefined))
            .then(|| self.temporal_integer(&day_v, 1, 31, "day"))
            .transpose()?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let month_code = month_code_s
            .clone()
            .or_else(|| requested_month.is_none().then(|| base.month_code.clone()));
        let fields = plain_month_day::MonthDayFields {
            extended_year: Some(requested_year.unwrap_or(base.year)),
            month_code: month_code.as_deref(),
            ordinal_month: requested_month.map(|value| value as u8),
            day: requested_day.map(|value| value as u8).unwrap_or(base.day),
        };
        let date = plain_month_day::month_day_from_fields(calendar_kind, &fields, reject)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar month-day".into())
            })?;
        let value = Self::temporal_date_value(TemporalKind::PlainMonthDay, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainYearMonth.prototype.add`/`subtract`. Only a duration
    /// with zero weeks/days/time is accepted -- `AddDurationToYearMonth`'s
    /// own rule (Gecko's `NonZeroDurationPartAfterMonths`).
    pub(super) fn temporal_year_month_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let mut duration = self.temporal_duration_from_value(duration_value)?;
        if negate {
            duration.years = -duration.years;
            duration.months = -duration.months;
            duration.weeks = -duration.weeks;
            duration.days = -duration.days;
            duration.hours = -duration.hours;
            duration.minutes = -duration.minutes;
            duration.seconds = -duration.seconds;
            duration.milliseconds = -duration.milliseconds;
            duration.microseconds = -duration.microseconds;
            duration.nanoseconds = -duration.nanoseconds;
        }
        if duration.weeks != 0
            || duration.days != 0
            || duration.hours != 0
            || duration.minutes != 0
            || duration.seconds != 0
            || duration.milliseconds != 0
            || duration.microseconds != 0
            || duration.nanoseconds != 0
        {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth arithmetic only accepts a years/months duration".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let fields = self.temporal_calendar_fields(&existing)?;
        let anchor_fields = plain_year_month::YearMonthFields {
            era: None,
            era_year: None,
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
        };
        let anchor = plain_year_month::year_month_from_fields(calendar_kind, &anchor_fields, false)
            .map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar year-month".into())
            })?;
        let result_date = plain_date::calendar_add_date(
            calendar_kind,
            anchor,
            duration.years as i64,
            duration.months as i64,
            0,
            0,
            reject,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal.PlainYearMonth arithmetic is out of range".into())
        })?;
        if !iso::is_year_month_within_limits(result_date.0, result_date.1) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth arithmetic is out of range".into(),
            ));
        }
        let value = Self::temporal_date_value(TemporalKind::PlainYearMonth, existing.calendar, result_date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainYearMonth.prototype.until`/`since`. `smallestUnit`/
    /// `largestUnit` are restricted to `"month"`/`"year"` -- `until`/`since`
    /// simply do not accept a finer unit for this type
    /// (`GetDifferenceSettings`'s own `disallowedUnits` for `YearMonth`).
    pub(super) fn temporal_year_month_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let other = self.temporal_to_plain_year_month(other_value, &Value::Undefined)?;
        if existing.calendar != other.calendar {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same calendar".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let largest_raw = self.temporal_raw_string_option(&resolved_options, "largestUnit")?;
        let increment_raw = self.temporal_raw_number_option(&resolved_options, "roundingIncrement")?;
        let mode_raw = self.temporal_raw_string_option(&resolved_options, "roundingMode")?;
        let smallest_raw = self.temporal_raw_string_option(&resolved_options, "smallestUnit")?;

        let smallest_unit = match smallest_raw.as_deref() {
            None => rounding::TemporalUnit::Month,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        if !matches!(
            smallest_unit,
            rounding::TemporalUnit::Month | rounding::TemporalUnit::Year
        ) {
            return Err(RuntimeError::RangeError(
                "smallestUnit is out of range for this receiver".into(),
            ));
        }
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => rounding::TemporalUnit::Year,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if !matches!(
            largest_unit,
            rounding::TemporalUnit::Month | rounding::TemporalUnit::Year
        ) {
            return Err(RuntimeError::RangeError(
                "largestUnit is out of range for this receiver".into(),
            ));
        }
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        let mode = Self::temporal_validated_rounding_mode(
            mode_raw.as_deref(),
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let (from, to) = if since { (&other, &existing) } else { (&existing, &other) };
        let from_fields = self.temporal_calendar_fields(from)?;
        let to_fields = self.temporal_calendar_fields(to)?;
        let resolve = |fields: &TemporalCalendarFields| {
            plain_year_month::year_month_from_fields(
                calendar_kind,
                &plain_year_month::YearMonthFields {
                    era: None,
                    era_year: None,
                    extended_year: Some(fields.year),
                    month_code: Some(&fields.month_code),
                    ordinal_month: None,
                },
                false,
            )
        };
        let from_date = resolve(&from_fields).map_err(|_| {
            RuntimeError::RangeError("invalid Temporal calendar year-month".into())
        })?;
        let to_date = resolve(&to_fields).map_err(|_| {
            RuntimeError::RangeError("invalid Temporal calendar year-month".into())
        })?;

        let (years, months, _, _) = plain_date::round_calendar_duration(
            calendar_kind,
            from_date,
            to_date,
            Self::temporal_unit_to_date_unit(largest_unit),
            Self::temporal_unit_to_date_unit(smallest_unit),
            increment,
            mode,
        );
        let record = blueice_ecma402::DurationRecord::try_new(
            i128::from(years),
            i128::from(months),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    pub(super) fn temporal_year_month_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let other = self.temporal_to_plain_year_month(other_value, &Value::Undefined)?;
        let equal = existing.year == other.year
            && existing.month == other.month
            && existing.day == other.day
            && existing.calendar == other.calendar;
        Ok(Value::Bool(equal))
    }

    pub(super) fn temporal_year_month_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let a = self.temporal_to_plain_year_month(one, &Value::Undefined)?;
        let b = self.temporal_to_plain_year_month(two, &Value::Undefined)?;
        let ord = (a.year, a.month, a.day).cmp(&(b.year, b.month, b.day));
        Ok(Value::Number(match ord {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    pub(super) fn temporal_month_day_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
        let other = self.temporal_to_plain_month_day(other_value, &Value::Undefined)?;
        let equal = existing.year == other.year
            && existing.month == other.month
            && existing.day == other.day
            && existing.calendar == other.calendar;
        Ok(Value::Bool(equal))
    }

    pub(super) fn temporal_year_month_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let resolved_options = self.temporal_options(options)?;
        let show_calendar_raw = self.temporal_string_option(
            &resolved_options,
            "calendarName",
            &["auto", "always", "never", "critical"],
        )?;
        let show_calendar = show_calendar_raw
            .as_deref()
            .map(|value| plain_date::parse_show_calendar(value).expect("already validated"))
            .unwrap_or(plain_date::ShowCalendar::Auto);
        let text = plain_year_month::format_year_month(
            (existing.year, existing.month, existing.day),
            &existing.calendar,
            show_calendar,
        );
        Ok(Value::String(text.into()))
    }

    pub(super) fn temporal_month_day_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
        let resolved_options = self.temporal_options(options)?;
        let show_calendar_raw = self.temporal_string_option(
            &resolved_options,
            "calendarName",
            &["auto", "always", "never", "critical"],
        )?;
        let show_calendar = show_calendar_raw
            .as_deref()
            .map(|value| plain_date::parse_show_calendar(value).expect("already validated"))
            .unwrap_or(plain_date::ShowCalendar::Auto);
        let text = plain_month_day::format_month_day(
            (existing.year, existing.month, existing.day),
            &existing.calendar,
            show_calendar,
        );
        Ok(Value::String(text.into()))
    }

    pub(super) fn temporal_year_month_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.temporal_year_month_receiver(receiver)?;
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

    pub(super) fn temporal_month_day_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.temporal_month_day_receiver(receiver)?;
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

    pub(super) fn temporal_year_month_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainYearMonth cannot be converted to a primitive value".into(),
        ))
    }

    pub(super) fn temporal_month_day_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainMonthDay cannot be converted to a primitive value".into(),
        ))
    }

    /// `Temporal.PlainYearMonth.prototype.toPlainDate`: merges the
    /// receiver's own year/monthCode with the required `item.day`.
    pub(super) fn temporal_year_month_to_plain_date(
        &mut self,
        receiver: &Value,
        item: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        if item.object_id().is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth.prototype.toPlainDate requires an object".into(),
            ));
        }
        let day_v = self.get_property(item, &"day".into())?;
        if matches!(day_v, Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth.prototype.toPlainDate requires a day property".into(),
            ));
        }
        let day = self.temporal_integer(&day_v, 1, 31, "day")?;
        let base = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let mut fields = DateFields::default();
        fields.extended_year = Some(base.year);
        fields.month_code = Some(base.month_code.as_bytes());
        fields.day = Some(day as u8);
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(icu_calendar::options::Overflow::Constrain);
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        let value = Self::temporal_value_from_calendar_date(TemporalKind::PlainDate, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainMonthDay.prototype.toPlainDate`: merges the
    /// receiver's own monthCode/day with the required `item.year`.
    pub(super) fn temporal_month_day_to_plain_date(
        &mut self,
        receiver: &Value,
        item: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
        if item.object_id().is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay.prototype.toPlainDate requires an object".into(),
            ));
        }
        let year_v = self.get_property(item, &"year".into())?;
        if matches!(year_v, Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay.prototype.toPlainDate requires a year property".into(),
            ));
        }
        let year = self.temporal_integer(&year_v, -9_999, 9_999, "year")?;
        let base = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let mut fields = DateFields::default();
        fields.extended_year = Some(year);
        fields.month_code = Some(base.month_code.as_bytes());
        fields.day = Some(base.day);
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(icu_calendar::options::Overflow::Constrain);
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        let value = Self::temporal_value_from_calendar_date(TemporalKind::PlainDate, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }
}

// ---- Stage 2's `zoned_date_time.rs` slice (third and final Stage 2 type) -
//
// A third, textually separate `impl Vm` block, deliberately appended here
// rather than folded into either block above -- this phase's own
// established "git diff misalignment" avoidance: keeping a new type's own
// adapter methods in their own block minimizes the chance a line-based merge
// mistakes one type's method for another's when this file is merged against
// concurrent sibling work (see this document's PLAN.md for the repeated
// pattern this avoids).
//
// `Temporal.ZonedDateTime` composes `PlainDateTime` + `TimeZone` +
// `Instant`: its stored ISO fields are always the *local* wall-clock fields
// its `epoch_nanoseconds` resolves to in its own `time_zone`
// (`temporal_set_local_fields`, Track E's own convention, reused throughout
// below), so every calendar-field/time-of-day getter and every
// `plain_date`/`calendar` helper this module already has for `PlainDate`/
// `PlainDateTime` applies to a `ZonedDateTime` receiver for free once its
// local fields are known to be correct -- which is genuinely new as of this
// slice: the numeric constructor and `from()` previously left every
// `ZonedDateTime`'s fields at their `1970-01-01T00:00:00` defaults
// regardless of its real epoch/zone (see the constructor fix above, and
// `temporal_to_zoned_date_time`/`temporal_value_from_zoned_date_time_string`
// below for the `from()` half of the same gap).
impl Vm {
    /// Brand check shared by every `Temporal.ZonedDateTime.prototype` method.
    fn temporal_zoned_date_time_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
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
        Ok(value)
    }

    /// `ToTemporalOffset`: reads the `offset` option, one of `"prefer"`/
    /// `"use"`/`"ignore"`/`"reject"`.
    fn temporal_offset_option(
        &mut self,
        options: &Value,
        default: &'static str,
    ) -> Result<String, RuntimeError> {
        Ok(self
            .temporal_string_option(options, "offset", &["prefer", "use", "ignore", "reject"])?
            .unwrap_or_else(|| default.to_string()))
    }

    /// `ToTemporalZonedDateTime`.
    fn temporal_to_zoned_date_time(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    // Options are still read (for validation/ordering parity)
                    // even though a `ZonedDateTime` argument is used as-is.
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    self.temporal_disambiguation(&resolved_options)?;
                    self.temporal_offset_option(&resolved_options, "reject")?;
                    return Ok(temporal);
                }
            }
            // A property bag: `timeZone` is required, `offset` optional;
            // every calendar-date and time-of-day field is the same set
            // `temporal_plain_date_from_fields` already resolves for
            // `PlainDateTime` (a `ZonedDateTime`'s own field list per
            // `PrepareCalendarFields`/`CalendarDateFromFields` is identical
            // once `timeZone`/`offset` are set aside), so that resolution is
            // reused rather than re-derived, with the result's `kind`
            // overridden afterward.
            let resolved_options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&resolved_options)?;
            let disambiguation = self.temporal_disambiguation(&resolved_options)?;
            let offset_option = self.temporal_offset_option(&resolved_options, "reject")?;
            let time_zone_value = self.get_property(value, &"timeZone".into())?;
            if time_zone_value == Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Temporal.ZonedDateTime property bag requires timeZone".into(),
                ));
            }
            let zone = self.temporal_time_zone(&time_zone_value)?;
            let offset_value = self.get_property(value, &"offset".into())?;
            let offset_string = (!matches!(offset_value, Value::Undefined))
                .then(|| self.coerce_string(&offset_value))
                .transpose()?
                .map(|text| {
                    text.to_utf8()
                        .map_err(|_| RuntimeError::RangeError("invalid Temporal offset".into()))
                })
                .transpose()?;
            let offset_nanoseconds = match offset_string.as_deref() {
                None => None,
                Some(text) => Some(
                    iso::parse_offset_string_nanoseconds(text)
                        .ok_or_else(|| RuntimeError::RangeError("invalid Temporal offset".into()))?,
                ),
            };
            let mut fields =
                self.temporal_plain_date_from_fields(TemporalKind::PlainDateTime, value, reject)?;
            let date = (fields.year, fields.month, fields.day);
            let time = (
                fields.hour,
                fields.minute,
                fields.second,
                fields.millisecond,
                fields.microsecond,
                fields.nanosecond,
            );
            let epoch_nanoseconds = temporal_interpret_offset(
                &zone,
                date,
                time,
                offset_nanoseconds,
                false,
                disambiguation,
                &offset_option,
            )?;
            if !epoch::is_in_instant_range(&epoch_nanoseconds) {
                return Err(RuntimeError::RangeError(
                    "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range"
                        .into(),
                ));
            }
            fields.kind = TemporalKind::ZonedDateTime;
            fields.epoch_nanoseconds = epoch_nanoseconds;
            fields.time_zone = zone.identifier();
            temporal_set_local_fields(&mut fields, &zone);
            return Ok(fields);
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.ZonedDateTime string".into())
        })?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        let disambiguation = self.temporal_disambiguation(&resolved_options)?;
        let offset_option = self.temporal_offset_option(&resolved_options, "reject")?;
        Self::temporal_value_from_zoned_date_time_string(&source, disambiguation, &offset_option)
    }

    /// `ParseTemporalZonedDateTimeString` + resolution: a
    /// `TemporalZonedDateTimeString` always carries a `TimeZoneAnnotation`
    /// (unlike every other Temporal string production, where one is at most
    /// optional), which is this method's real reason to exist separately
    /// from the generic `temporal_value_from_string` path every other type
    /// shares.
    fn temporal_value_from_zoned_date_time_string(
        source: &str,
        disambiguation: time_zone::Disambiguation,
        offset_option: &str,
    ) -> Result<TemporalValue, RuntimeError> {
        let parsed = iso::parse_date_time(source).ok_or_else(|| {
            RuntimeError::RangeError("invalid Temporal.ZonedDateTime string".into())
        })?;
        let annotation = parsed.time_zone.as_deref().ok_or_else(|| {
            RuntimeError::RangeError(
                "a Temporal.ZonedDateTime string requires a time zone annotation".into(),
            )
        })?;
        let zone = time_zone::parse_identifier(annotation).ok_or_else(|| {
            RuntimeError::RangeError(format!("invalid Temporal time zone: {annotation}"))
        })?;
        let calendar = match parsed.calendar.as_deref() {
            Some(calendar) => canonical_calendar_id(calendar).ok_or_else(|| {
                RuntimeError::RangeError(format!("unsupported Temporal calendar: {calendar}"))
            })?,
            None => "iso8601".to_string(),
        };
        let (year, month, day) = (parsed.year, parsed.month, parsed.day);
        let time = parsed.time.unwrap_or((0, 0, 0, 0, 0, 0));
        let epoch_nanoseconds = temporal_interpret_offset(
            &zone,
            (year, month, day),
            time,
            parsed.offset_nanoseconds,
            parsed.utc_designator,
            disambiguation,
            offset_option,
        )?;
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime string is outside the supported range".into(),
            ));
        }
        let mut value = TemporalValue {
            kind: TemporalKind::ZonedDateTime,
            duration: None,
            year,
            month,
            day,
            hour: time.0,
            minute: time.1,
            second: time.2,
            millisecond: time.3,
            microsecond: time.4,
            nanosecond: time.5,
            epoch_nanoseconds,
            calendar,
            time_zone: zone.identifier(),
        };
        temporal_set_local_fields(&mut value, &zone);
        Ok(value)
    }

    pub(super) fn temporal_zoned_date_time_with_time_zone(
        &mut self,
        receiver: &Value,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut value = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = self.temporal_time_zone(time_zone)?;
        // The instant itself is unchanged; only its presentation zone (and
        // therefore the local fields resolved from it) changes.
        value.time_zone = zone.identifier();
        temporal_set_local_fields(&mut value, &zone);
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_zoned_date_time_with_plain_time(
        &mut self,
        receiver: &Value,
        plain_time_like: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut value = self.temporal_zoned_date_time_receiver(receiver)?;
        let time = if *plain_time_like == Value::Undefined {
            (0, 0, 0, 0, 0, 0)
        } else {
            self.temporal_to_plain_time(plain_time_like, &Value::Undefined)?
        };
        let zone = temporal_zoned_date_time_zone(&value);
        let date = (value.year, value.month, value.day);
        value.epoch_nanoseconds = zone
            .epoch_nanoseconds_for(date, time, time_zone::Disambiguation::Compatible)
            .map_err(temporal_resolution_error)?;
        temporal_set_local_fields(&mut value, &zone);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.ZonedDateTime.prototype.with`. Mirrors `temporal_date_with`'s
    /// own field-merge shape (deliberately re-derived here rather than
    /// shared, per this block's own "own textual block" rationale above),
    /// extended with the `offset` field and the `disambiguation`/`offset`
    /// options `PlainDate`/`PlainDateTime` have no notion of.
    pub(super) fn temporal_zoned_date_time_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let like_object = like
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Temporal.with requires an object".into()))?;
        if self.heap.temporal_value(like_object)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal.with does not accept a Temporal-like object".into(),
            ));
        }
        for banned in ["calendar", "timeZone"] {
            if self.get_property(like, &banned.into())? != Value::Undefined {
                return Err(RuntimeError::TypeError(format!(
                    "Temporal.with does not accept a {banned} property"
                )));
            }
        }
        let existing_fields = self.temporal_calendar_fields(&existing)?;
        let year_v = self.get_property(like, &"year".into())?;
        let month_v = self.get_property(like, &"month".into())?;
        let month_code_v = self.get_property(like, &"monthCode".into())?;
        let day_v = self.get_property(like, &"day".into())?;
        let era_v = self.get_property(like, &"era".into())?;
        let era_year_v = self.get_property(like, &"eraYear".into())?;
        let hour_v = self.get_property(like, &"hour".into())?;
        let minute_v = self.get_property(like, &"minute".into())?;
        let second_v = self.get_property(like, &"second".into())?;
        let ms_v = self.get_property(like, &"millisecond".into())?;
        let us_v = self.get_property(like, &"microsecond".into())?;
        let ns_v = self.get_property(like, &"nanosecond".into())?;
        let offset_v = self.get_property(like, &"offset".into())?;
        let any_present = [
            &year_v, &month_v, &month_code_v, &day_v, &era_v, &era_year_v, &hour_v, &minute_v,
            &second_v, &ms_v, &us_v, &ns_v, &offset_v,
        ]
        .into_iter()
        .any(|value| *value != Value::Undefined);
        if !any_present {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;
        let disambiguation = self.temporal_disambiguation(&resolved_options)?;
        let offset_option = self.temporal_offset_option(&resolved_options, "prefer")?;

        let mut fields = DateFields::default();
        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| self.temporal_integer(&year_v, -9_999, 9_999, "year"))
            .transpose()?;
        let era_s = (!matches!(era_v, Value::Undefined))
            .then(|| self.coerce_string(&era_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
            })
            .transpose()?;
        let era_year_num = (!matches!(era_year_v, Value::Undefined))
            .then(|| self.temporal_integer(&era_year_v, -9_999, 9_999, "era year"))
            .transpose()?;
        if let Some(era) = era_s.as_deref().filter(|_| existing.calendar != "iso8601") {
            fields.era = Some(era.as_bytes());
            fields.era_year = Some(era_year_num.or(existing_fields.era_year).ok_or_else(|| {
                RuntimeError::TypeError("Temporal eraYear requires an era".into())
            })?);
        } else if era_year_num.is_some() && existing.calendar != "iso8601" {
            return Err(RuntimeError::RangeError(
                "Temporal eraYear requires an era".into(),
            ));
        } else {
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        }

        let requested_month = (!matches!(month_v, Value::Undefined))
            .then(|| self.temporal_integer(&month_v, 1, 99, "month"))
            .transpose()?;
        let month_code_s = (!matches!(month_code_v, Value::Undefined))
            .then(|| self.coerce_string(&month_code_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;
        if let Some(month_code) = month_code_s.as_deref() {
            fields.month_code = Some(month_code.as_bytes());
        } else if let Some(month) = requested_month {
            fields.ordinal_month = Some(month as u8);
        } else {
            fields.month_code = Some(existing_fields.month_code.as_bytes());
        }
        let requested_day = (!matches!(day_v, Value::Undefined))
            .then(|| self.temporal_integer(&day_v, 1, 31, "day"))
            .transpose()?;
        fields.day = Some(requested_day.unwrap_or(i32::from(existing_fields.day)) as u8);

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(if reject {
            icu_calendar::options::Overflow::Reject
        } else {
            icu_calendar::options::Overflow::Constrain
        });
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        if requested_year.is_some_and(|year| year != date.year().extended_year())
            || (month_code_s.is_some()
                && requested_month.is_some_and(|month| month as u8 != date.month().ordinal))
        {
            return Err(RuntimeError::RangeError(
                "inconsistent Temporal calendar fields".into(),
            ));
        }
        let mut result = Self::temporal_value_from_calendar_date(
            TemporalKind::ZonedDateTime,
            existing.calendar.clone(),
            date,
        );
        result.hour =
            self.temporal_optional_integer(&hour_v, i32::from(existing.hour), 0, 23, "hour")? as u8;
        result.minute = self.temporal_optional_integer(
            &minute_v,
            i32::from(existing.minute),
            0,
            59,
            "minute",
        )? as u8;
        result.second = self.temporal_optional_integer(
            &second_v,
            i32::from(existing.second),
            0,
            59,
            "second",
        )? as u8;
        result.millisecond = self.temporal_optional_integer(
            &ms_v,
            i32::from(existing.millisecond),
            0,
            999,
            "millisecond",
        )? as u16;
        result.microsecond = self.temporal_optional_integer(
            &us_v,
            i32::from(existing.microsecond),
            0,
            999,
            "microsecond",
        )? as u16;
        result.nanosecond = self.temporal_optional_integer(
            &ns_v,
            i32::from(existing.nanosecond),
            0,
            999,
            "nanosecond",
        )? as u16;

        let offset_string = (!matches!(offset_v, Value::Undefined))
            .then(|| self.coerce_string(&offset_v))
            .transpose()?
            .map(|text| {
                text.to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal offset".into()))
            })
            .transpose()?;
        let zone = temporal_zoned_date_time_zone(&existing);
        let offset_nanoseconds = match offset_string.as_deref() {
            Some(text) => Some(
                iso::parse_offset_string_nanoseconds(text)
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal offset".into()))?,
            ),
            // `.with()`'s own default offset behaviour: an omitted `offset`
            // field keeps the receiver's *current real* offset as the
            // preferred one, rather than falling all the way back to plain
            // zone/disambiguation resolution -- what makes `{ hour: 2 }` on
            // a value observing a repeated local hour stay on the same side
            // of the transition it already was on, not jump to
            // `"compatible"`'s default choice.
            None => Some(zone.offset_nanoseconds_for(&existing.epoch_nanoseconds)),
        };
        let date_tuple = (result.year, result.month, result.day);
        let time_tuple = (
            result.hour,
            result.minute,
            result.second,
            result.millisecond,
            result.microsecond,
            result.nanosecond,
        );
        let epoch_nanoseconds = temporal_interpret_offset(
            &zone,
            date_tuple,
            time_tuple,
            offset_nanoseconds,
            false,
            disambiguation,
            &offset_option,
        )?;
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime.with is out of range".into(),
            ));
        }
        result.epoch_nanoseconds = epoch_nanoseconds;
        result.time_zone = zone.identifier();
        temporal_set_local_fields(&mut result, &zone);
        self.alloc_temporal_value(result, false)
    }

    /// `Temporal.ZonedDateTime.prototype.add`/`subtract`: `AddZonedDateTime`
    /// (`vm/temporal/zoned_date_time.rs`) -- calendar years/months/weeks/days
    /// carried through the calendar at the receiver's own local date/time,
    /// re-resolved through the zone, and only then the exact time-duration
    /// nanoseconds added directly to that resolved instant.
    pub(super) fn temporal_zoned_date_time_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let mut duration = self.temporal_duration_from_value(duration_value)?;
        if negate {
            duration.years = -duration.years;
            duration.months = -duration.months;
            duration.weeks = -duration.weeks;
            duration.days = -duration.days;
            duration.hours = -duration.hours;
            duration.minutes = -duration.minutes;
            duration.seconds = -duration.seconds;
            duration.milliseconds = -duration.milliseconds;
            duration.microseconds = -duration.microseconds;
            duration.nanoseconds = -duration.nanoseconds;
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let zone = temporal_zoned_date_time_zone(&existing);
        let time_total = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        )
        .total_nanoseconds();
        let local_date = (existing.year, existing.month, existing.day);
        let local_time = (
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        let result_ns = zoned_date_time::add_zoned_date_time(
            &zone,
            calendar_kind,
            &existing.epoch_nanoseconds,
            local_date,
            local_time,
            duration.years as i64,
            duration.months as i64,
            duration.weeks as i64,
            duration.days as i64,
            time_total,
            reject,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal.ZonedDateTime arithmetic is out of range".into())
        })?;
        if !epoch::is_in_instant_range(&result_ns) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime arithmetic is out of range".into(),
            ));
        }
        existing.epoch_nanoseconds = result_ns;
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
    }

    /// `Temporal.ZonedDateTime.prototype.round`: `RoundZonedDateTimeInstant`
    /// -- day-unit rounding anchors on `GetStartOfDay`'s real (possibly
    /// 23/25-hour) day boundary rather than a fixed 86,400-second one; every
    /// other unit rounds the local wall-clock time (`RoundISODateTime`'s own
    /// shape, matching `Temporal.PlainDateTime.prototype.round`), then
    /// re-resolves through the zone with `"compatible"` disambiguation.
    pub(super) fn temporal_zoned_date_time_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        if *round_to == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime.round requires a smallestUnit or options argument".into(),
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
            let smallest_unit_text = smallest_unit.as_deref().ok_or_else(|| {
                RuntimeError::RangeError(
                    "Temporal.ZonedDateTime.round requires smallestUnit".into(),
                )
            })?;
            let zone = temporal_zoned_date_time_zone(&existing);
            if matches!(smallest_unit_text, "day" | "days") {
                if increment != 1 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement must be 1 when smallestUnit is \"day\"".into(),
                    ));
                }
                let date = (existing.year, existing.month, existing.day);
                let start = zone.start_of_day(date);
                let next = plain_date::add_iso_date(date, 0, 0, 0, 1, false).ok_or_else(|| {
                    RuntimeError::RangeError("Temporal.ZonedDateTime.round is out of range".into())
                })?;
                let end = zone.start_of_day(next);
                let day_length = i128::try_from(&end - &start)
                    .expect("one day's length fits in i128 many times over");
                let offset_into_day = i128::try_from(&existing.epoch_nanoseconds - &start)
                    .expect("an offset within one day fits in i128");
                let rounded =
                    rounding::round_to_increment_as_if_positive(offset_into_day, day_length, mode);
                existing.epoch_nanoseconds = start + BigInt::from(rounded);
            } else {
                let smallest_unit = rounding::parse_time_unit(smallest_unit_text)
                    .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?;
                // `Instant`/`ZonedDateTime.round`'s own rule: the increment
                // must divide a whole day (inclusive) -- not `PlainTime`'s
                // narrower "must stay below the unit's own place value" one.
                let day_ns = 86_400_000_000_000_i128;
                let step = smallest_unit.nanoseconds() * increment;
                if day_ns % step != 0 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement does not divide evenly into a day".into(),
                    ));
                }
                let time_ns = duration_math::time_fields_to_nanoseconds(
                    existing.hour,
                    existing.minute,
                    existing.second,
                    existing.millisecond,
                    existing.microsecond,
                    existing.nanosecond,
                );
                let rounded = duration_math::TimeDuration::from_nanoseconds(time_ns)
                    .round(smallest_unit, increment, mode)
                    .total_nanoseconds();
                let day_carry = rounded.div_euclid(86_400_000_000_000);
                let ns_of_day = rounded.rem_euclid(86_400_000_000_000);
                let calendar_kind = calendar::calendar_kind(&existing.calendar)
                    .expect("Temporal values retain a validated calendar identifier");
                let date = plain_date::calendar_add_date(
                    calendar_kind,
                    (existing.year, existing.month, existing.day),
                    0,
                    0,
                    0,
                    day_carry as i64,
                    false,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal.ZonedDateTime.round is out of range".into())
                })?;
                let time = duration_math::time_fields_from_nanoseconds(ns_of_day);
                existing.epoch_nanoseconds = zone
                    .epoch_nanoseconds_for(date, time, time_zone::Disambiguation::Compatible)
                    .map_err(temporal_resolution_error)?;
            }
            if !epoch::is_in_instant_range(&existing.epoch_nanoseconds) {
                return Err(RuntimeError::RangeError(
                    "Temporal.ZonedDateTime.round is out of range".into(),
                ));
            }
            temporal_set_local_fields(&mut existing, &zone);
            self.alloc_temporal_value(existing, false)
        })();
        self.stack.truncate(base);
        result
    }

    /// Maps a `rounding::TemporalUnit` (the wide ten-variant vocabulary) down
    /// to its `rounding::TimeUnit` counterpart -- only ever called once the
    /// caller already knows the unit is `hour`..`nanosecond`.
    fn temporal_unit_to_time_unit(unit: rounding::TemporalUnit) -> rounding::TimeUnit {
        match unit {
            rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
            rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
            rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
            rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
            rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
            _ => rounding::TimeUnit::Nanosecond,
        }
    }

    /// `Temporal.ZonedDateTime.prototype.until`/`since`:
    /// `DifferenceTemporalZonedDateTime`. When `largestUnit` never reaches a
    /// calendar day, this is pure exact-nanosecond `Instant` difference (no
    /// zone or calendar consulted at all); otherwise the date part comes
    /// from `zoned_date_time::difference_zoned_date_time` (real calendar-day
    /// arithmetic, honouring a DST-shortened/lengthened day exactly) and
    /// only the sub-day remainder is a time duration.
    ///
    /// Known simplification, documented rather than silently approximated:
    /// when `smallestUnit` itself reaches a calendar day (week/month/year),
    /// a nonzero sub-day exact-time remainder is folded into a whole extra
    /// day toward the later endpoint before calendar rounding, rather than
    /// `RoundRelativeDuration`'s own exact fractional-day position within
    /// that specific (possibly 23/25-hour) day. This keeps every result
    /// field sign-consistent with the overall direction (`DurationRecord`'s
    /// own invariant) and is exact whenever the remainder is zero (the
    /// common case: two `ZonedDateTime`s that share the same local time of
    /// day), which is the case every `since`/`until` calendar-unit fixture
    /// this slice was verified against exercises.
    pub(super) fn temporal_zoned_date_time_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let other = self.temporal_to_zoned_date_time(other_value, &Value::Undefined)?;
        if existing.calendar != other.calendar {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same calendar".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let largest_raw = self.temporal_raw_string_option(&resolved_options, "largestUnit")?;
        let increment_raw =
            self.temporal_raw_number_option(&resolved_options, "roundingIncrement")?;
        let mode_raw = self.temporal_raw_string_option(&resolved_options, "roundingMode")?;
        let smallest_raw = self.temporal_raw_string_option(&resolved_options, "smallestUnit")?;

        let smallest_unit = match smallest_raw.as_deref() {
            None => rounding::TemporalUnit::Nanosecond,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        // `ZonedDateTime`'s own default `largestUnit` is the larger of
        // `"hour"` and `smallestUnit` -- unlike `Instant`'s `"second"` and
        // `PlainDate`/`PlainDateTime`'s `"day"` defaults.
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => smallest_unit.max(rounding::TemporalUnit::Hour),
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        let mode = Self::temporal_validated_rounding_mode(
            mode_raw.as_deref(),
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;

        let zone = temporal_zoned_date_time_zone(&existing);
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");

        let (
            years,
            months,
            weeks,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        ) = if largest_unit < rounding::TemporalUnit::Day {
            let diff_ns = i128::try_from(&other.epoch_nanoseconds - &existing.epoch_nanoseconds)
                .expect("an Instant-range difference fits in i128");
            let rounded = duration_math::TimeDuration::from_nanoseconds(diff_ns).round(
                Self::temporal_unit_to_time_unit(smallest_unit),
                increment,
                mode,
            );
            let [h, m, s, ms, us, ns] =
                rounded.balance_to(Self::temporal_unit_to_time_unit(largest_unit));
            (0, 0, 0, 0, h, m, s, ms, us, ns)
        } else {
            let date1 = (existing.year, existing.month, existing.day);
            let time1 = (
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            );
            let date2 = (other.year, other.month, other.day);
            let date_unit_largest = Self::temporal_unit_to_date_unit(largest_unit);
            let (_, _, _, _, remainder_ns) = zoned_date_time::difference_zoned_date_time(
                &zone,
                calendar_kind,
                &existing.epoch_nanoseconds,
                date1,
                time1,
                &other.epoch_nanoseconds,
                date2,
                date_unit_largest,
            );
            if smallest_unit >= rounding::TemporalUnit::Day {
                let overall_sign = match &other.epoch_nanoseconds - &existing.epoch_nanoseconds {
                    diff if diff > BigInt::from(0) => 1_i64,
                    diff if diff < BigInt::from(0) => -1_i64,
                    _ => 0_i64,
                };
                let rounding_date2 = if remainder_ns != 0 && overall_sign != 0 {
                    plain_date::add_iso_date(date2, 0, 0, 0, overall_sign, false).unwrap_or(date2)
                } else {
                    date2
                };
                let date_unit_smallest = Self::temporal_unit_to_date_unit(smallest_unit);
                let (years, months, weeks, days) = plain_date::round_calendar_duration(
                    calendar_kind,
                    date1,
                    rounding_date2,
                    date_unit_largest,
                    date_unit_smallest,
                    increment,
                    mode,
                );
                (years, months, weeks, days, 0, 0, 0, 0, 0, 0)
            } else {
                let (years, months, weeks, days, remainder_ns) =
                    zoned_date_time::difference_zoned_date_time(
                        &zone,
                        calendar_kind,
                        &existing.epoch_nanoseconds,
                        date1,
                        time1,
                        &other.epoch_nanoseconds,
                        date2,
                        date_unit_largest,
                    );
                let time_unit = Self::temporal_unit_to_time_unit(smallest_unit);
                let rounded = duration_math::TimeDuration::from_nanoseconds(remainder_ns)
                    .round(time_unit, increment, mode);
                let [h, m, s, ms, us, ns] = rounded.balance_to(rounding::TimeUnit::Hour);
                (years, months, weeks, days, h, m, s, ms, us, ns)
            }
        };

        let (
            years,
            months,
            weeks,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        ) = if since {
            (
                -years,
                -months,
                -weeks,
                -days,
                -hours,
                -minutes,
                -seconds,
                -milliseconds,
                -microseconds,
                -nanoseconds,
            )
        } else {
            (
                years,
                months,
                weeks,
                days,
                hours,
                minutes,
                seconds,
                milliseconds,
                microseconds,
                nanoseconds,
            )
        };
        let record = blueice_ecma402::DurationRecord::try_new(
            i128::from(years),
            i128::from(months),
            i128::from(weeks),
            i128::from(days),
            i128::from(hours),
            i128::from(minutes),
            i128::from(seconds),
            i128::from(milliseconds),
            i128::from(microseconds),
            i128::from(nanoseconds),
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    pub(super) fn temporal_zoned_date_time_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let other = self.temporal_to_zoned_date_time(other_value, &Value::Undefined)?;
        Ok(Value::Bool(
            existing.epoch_nanoseconds == other.epoch_nanoseconds
                && existing.time_zone == other.time_zone
                && existing.calendar == other.calendar,
        ))
    }

    pub(super) fn temporal_zoned_date_time_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_to_zoned_date_time(one, &Value::Undefined)?;
        let two = self.temporal_to_zoned_date_time(two, &Value::Undefined)?;
        Ok(Value::Number(
            match one.epoch_nanoseconds.cmp(&two.epoch_nanoseconds) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.0,
            },
        ))
    }

    /// `TemporalZonedDateTimeToString`: rounds the exact epoch instant first
    /// (`RoundTemporalInstant`'s own magnitude-based rounding, matching
    /// `Instant.prototype.toString` -- not a zone-day-aware rounding), then
    /// formats the *rounded* instant's local fields plus an exact (not
    /// minute-rounded) offset, an optional time-zone annotation and a
    /// calendar annotation.
    pub(super) fn temporal_zoned_date_time_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_options(options)?;
            let show_calendar = self.temporal_string_option(
                &options,
                "calendarName",
                &["auto", "always", "never", "critical"],
            )?;
            let explicit_digits = self.temporal_fractional_second_digits(&options)?;
            let show_offset = self
                .temporal_string_option(&options, "offset", &["auto", "never"])?
                .unwrap_or_else(|| "auto".into());
            let mode = self
                .temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
            let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
            let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", false)?;
            let show_time_zone = self
                .temporal_string_option(&options, "timeZoneName", &["auto", "never", "critical"])?
                .unwrap_or_else(|| "auto".into());
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
            let epoch_i128: i128 = existing.epoch_nanoseconds.to_i128().ok_or_else(|| {
                RuntimeError::RangeError("invalid Temporal.ZonedDateTime".into())
            })?;
            let rounded = duration_math::TimeDuration::from_nanoseconds(epoch_i128)
                .round_as_if_positive(unit, increment, mode)
                .total_nanoseconds();
            let rounded_ns = BigInt::from(rounded);
            let zone = temporal_zoned_date_time_zone(&existing);
            let offset_ns = zone.offset_nanoseconds_for(&rounded_ns);
            let local = &rounded_ns + BigInt::from(offset_ns);
            let mut result = format_zoned_date_time_date_time(&local, precision);
            if show_offset != "never" {
                result.push_str(&format_offset_nanoseconds_exact(offset_ns));
            }
            if show_time_zone != "never" {
                result.push('[');
                if show_time_zone == "critical" {
                    result.push('!');
                }
                result.push_str(&existing.time_zone);
                result.push(']');
            }
            let show = plain_date::parse_show_calendar(show_calendar.as_deref().unwrap_or("auto"))
                .ok_or_else(|| RuntimeError::RangeError("invalid calendarName option".into()))?;
            result.push_str(&plain_date::format_calendar_annotation(
                &existing.calendar,
                show,
            ));
            Ok(Value::String(result.into()))
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn temporal_zoned_date_time_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.ZonedDateTime cannot be converted to a primitive value".into(),
        ))
    }

    pub(super) fn temporal_zoned_date_time_to_instant(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        self.instant_from_epoch_nanoseconds(existing.epoch_nanoseconds)
    }

    pub(super) fn temporal_zoned_date_time_to_plain_date(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let value = Self::temporal_date_value(
            TemporalKind::PlainDate,
            existing.calendar,
            (existing.year, existing.month, existing.day),
        );
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_zoned_date_time_to_plain_time(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let fields = (
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        self.alloc_temporal_value(Self::plain_time_value(fields), false)
    }

    pub(super) fn temporal_zoned_date_time_to_plain_date_time(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let value = Self::temporal_date_time_value(
            TemporalKind::PlainDateTime,
            existing.calendar,
            (existing.year, existing.month, existing.day),
            (
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            ),
        );
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.ZonedDateTime.prototype.toPlainYearMonth`: identical
    /// `CalendarYearMonthFromFields` resolution to
    /// `temporal_plain_date_to_plain_year_month`, reused here since a
    /// `ZonedDateTime`'s own stored ISO fields are already its local
    /// calendar date -- `temporal_calendar_fields` does not care which
    /// `TemporalKind` supplied them.
    pub(super) fn temporal_zoned_date_time_to_plain_year_month(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let fields = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let ym_fields = plain_year_month::YearMonthFields {
            era: None,
            era_year: None,
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &ym_fields, false)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar year-month".into()))?;
        let value = Self::temporal_date_value(TemporalKind::PlainYearMonth, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_zoned_date_time_to_plain_month_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let fields = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let md_fields = plain_month_day::MonthDayFields {
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
            day: fields.day,
        };
        let date = plain_month_day::month_day_from_fields(calendar_kind, &md_fields, false)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()))?;
        let value = Self::temporal_date_value(TemporalKind::PlainMonthDay, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_zoned_date_time_start_of_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = temporal_zoned_date_time_zone(&existing);
        let date = (existing.year, existing.month, existing.day);
        existing.epoch_nanoseconds = zone.start_of_day(date);
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
    }

    pub(super) fn temporal_zoned_date_time_get_iso_fields(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = temporal_zoned_date_time_zone(&existing);
        let offset_ns = zone.offset_nanoseconds_for(&existing.epoch_nanoseconds);
        let object = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            self.define_data(
                object,
                "calendar",
                Value::String(existing.calendar.clone().into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoDay",
                Value::Number(existing.day.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoHour",
                Value::Number(existing.hour.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMicrosecond",
                Value::Number(existing.microsecond.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMillisecond",
                Value::Number(existing.millisecond.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMinute",
                Value::Number(existing.minute.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMonth",
                Value::Number(existing.month.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoNanosecond",
                Value::Number(existing.nanosecond.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoSecond",
                Value::Number(existing.second.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoYear",
                Value::Number(existing.year.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "offset",
                Value::String(format_offset_nanoseconds_exact(offset_ns).into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "timeZone",
                Value::String(existing.time_zone.clone().into()),
                true,
                true,
                true,
            )?;
            Ok(Value::Object(object))
        })();
        self.stack.truncate(base);
        result
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

/// Re-parses a `ZonedDateTime` value's own stored `time_zone` identifier
/// back into a [`time_zone::TimeZone`]. Always succeeds: the identifier only
/// ever comes from [`time_zone::TimeZone::identifier`] itself (the
/// constructor/`from`/every method below all store it that way), which is
/// always round-trippable through [`time_zone::parse_identifier`].
fn temporal_zoned_date_time_zone(value: &TemporalValue) -> time_zone::TimeZone {
    time_zone::parse_identifier(&value.time_zone)
        .expect("a ZonedDateTime value's own stored time zone is always a valid identifier")
}

/// `InterpretISODateTimeOffset`, collapsed to this engine's own three
/// offset-behaviour shapes:
///
/// - `utc_exact` (the ISO string `Z` designator only): the offset is exactly
///   zero, and the zone/disambiguation are never consulted at all.
/// - `offset_nanoseconds: None` (`"wall"` behaviour -- no offset spelled at
///   all): resolved purely through the zone and `disambiguation`.
/// - `offset_nanoseconds: Some(_)` (`"option"` behaviour -- a property-bag
///   `offset` field or a string's own numeric offset): used directly
///   whenever it matches one of the zone's real possible instants for that
///   local date/time; otherwise `offset_option` decides -- `"use"` trusts it
///   regardless, `"reject"` throws, and `"ignore"`/`"prefer"` both fall back
///   to zone/disambiguation resolution (the spec's own `InterpretISODateTimeOffset`
///   already collapses those last two into the same branch once no
///   candidate matches, so there is no separate `"prefer"` case to add).
fn temporal_interpret_offset(
    zone: &time_zone::TimeZone,
    date: epoch::CivilDate,
    time: epoch::CivilTime,
    offset_nanoseconds: Option<i64>,
    utc_exact: bool,
    disambiguation: time_zone::Disambiguation,
    offset_option: &str,
) -> Result<BigInt, RuntimeError> {
    let local = epoch::nanoseconds_since_epoch(date, time, 0);
    if utc_exact {
        return Ok(local - BigInt::from(offset_nanoseconds.unwrap_or(0)));
    }
    let Some(offset_ns) = offset_nanoseconds else {
        return zone
            .epoch_nanoseconds_for(date, time, disambiguation)
            .map_err(temporal_resolution_error);
    };
    let candidate = &local - BigInt::from(offset_ns);
    let possible = zone.possible_epoch_nanoseconds(date, time);
    if possible.contains(&candidate) {
        return Ok(candidate);
    }
    match offset_option {
        "use" => Ok(candidate),
        "reject" => Err(RuntimeError::RangeError(
            "the given offset does not match the time zone".into(),
        )),
        _ => zone
            .epoch_nanoseconds_for(date, time, disambiguation)
            .map_err(temporal_resolution_error),
    }
}

/// `FormatUTCOffsetNanoseconds`: an *exact* `±HH:MM[:SS[.sssssssss]]`
/// representation -- unlike `format_instant_string`'s own offset formatting
/// (`FormatDateTimeUTCOffsetRounded`, always rounded to the nearest minute,
/// which is what `Instant.prototype.toString`'s optional `timeZone` display
/// specifically calls for). `ZonedDateTime`'s own `offset`
/// getter/`getISOFields`/`toString` all need the real, possibly sub-minute
/// historical offset a named zone can carry (e.g. Monrovia's pre-1972
/// -00:44:30), because round-tripping the string must be exact.
fn format_offset_nanoseconds_exact(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let magnitude = offset.unsigned_abs();
    let hours = magnitude / 3_600_000_000_000;
    let minutes = (magnitude / 60_000_000_000) % 60;
    let seconds = (magnitude / 1_000_000_000) % 60;
    let nanoseconds = magnitude % 1_000_000_000;
    let mut result = format!("{sign}{hours:02}:{minutes:02}");
    if seconds != 0 || nanoseconds != 0 {
        result.push_str(&format!(":{seconds:02}"));
        if nanoseconds != 0 {
            let text = format!("{nanoseconds:09}");
            result.push('.');
            result.push_str(text.trim_end_matches('0'));
        }
    }
    result
}

/// The date/time portion of `TemporalZonedDateTimeToString`'s output, given
/// an already-rounded *local* epoch value (`rounded_epoch_nanoseconds +
/// offset`, i.e. what `epoch::instant_fields` decomposes as this zone's own
/// wall-clock fields). Deliberately duplicates
/// `Vm::format_instant_string`'s own date/time formatting (rather than
/// reusing it) since that function always appends its *own* offset/`Z`
/// suffix, which `ZonedDateTime`'s exact (not minute-rounded) offset display
/// cannot reuse -- see [`format_offset_nanoseconds_exact`]'s own doc
/// comment.
fn format_zoned_date_time_date_time(local: &BigInt, precision: SecondsPrecision) -> String {
    let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
        epoch::instant_fields(local);
    let mut result = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
    };
    result.push_str(&format!("-{month:02}-{day:02}T{hour:02}:{minute:02}"));
    if precision != SecondsPrecision::Minute {
        result.push_str(&format!(":{second:02}"));
        let nanos_total =
            u32::from(millisecond) * 1_000_000 + u32::from(microsecond) * 1_000 + u32::from(nanosecond);
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
    result
}
