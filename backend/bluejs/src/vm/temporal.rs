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
use icu_calendar::{types::DateFields, AnyCalendar, AnyCalendarKind, Date, Iso};
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

/// `(years, months, weeks, days, hours, minutes, seconds, milliseconds,
/// microseconds, nanoseconds)` — the balanced date/time duration fields
/// [`Vm::temporal_zoned_date_time_difference_fields`] resolves a
/// `since`/`until`/`round`/`total` request down to. Named to keep that
/// function's `Result<_, RuntimeError>` signature under clippy's
/// `type_complexity` threshold.
type DateTimeDurationFields = (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64);

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

/// A resolved `Temporal.Duration.prototype.{round,total}`/static `compare`
/// `relativeTo` anchor. `Plain` covers a `PlainDate`/`PlainDateTime` object,
/// a date-only string, or a property bag with no `timeZone` — arithmetic
/// against one only ever needs the *date* (a `PlainDateTime` anchor's
/// time-of-day is read, and validated, but never carried forward:
/// `Duration/prototype/total/relativeto-plaindatetime.js` confirms a
/// `PlainDateTime` anchor and the `PlainDate` made from just its date fields
/// give identical results). `Zoned` covers a real `Temporal.ZonedDateTime` —
/// object, string, or property bag with a `timeZone` — carrying everything
/// `zoned_date_time::{add_zoned_date_time, difference_zoned_date_time,
/// day_length_nanoseconds}` need: the zone itself (now, since Phase 26 Stage
/// 2, real IANA transition data and not just `UTC`/a fixed offset), the
/// exact epoch instant, and the resolved local date/time.
#[derive(Clone)]
enum DurationAnchor {
    Plain {
        calendar: AnyCalendarKind,
        date: epoch::CivilDate,
    },
    Zoned {
        calendar: AnyCalendarKind,
        zone: time_zone::TimeZone,
        epoch_ns: BigInt,
        local_date: epoch::CivilDate,
        local_time: epoch::CivilTime,
    },
}

impl DurationAnchor {
    fn calendar(&self) -> AnyCalendarKind {
        match self {
            DurationAnchor::Plain { calendar, .. } | DurationAnchor::Zoned { calendar, .. } => {
                *calendar
            }
        }
    }

    /// The anchor's local civil date — the *only* thing a `Plain` anchor's
    /// calendar-unit arithmetic ever needs, and the date a `Zoned` anchor's
    /// own date part is computed relative to (its day/hour-granularity
    /// arithmetic additionally needs the zone and exact epoch instant, which
    /// callers reach separately).
    fn date(&self) -> epoch::CivilDate {
        match self {
            DurationAnchor::Plain { date, .. } => *date,
            DurationAnchor::Zoned { local_date, .. } => *local_date,
        }
    }
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
                        ("daysInYear", native::TemporalGetter::DaysInYear),
                        ("daysInMonth", native::TemporalGetter::DaysInMonth),
                        ("monthsInYear", native::TemporalGetter::MonthsInYear),
                        ("inLeapYear", native::TemporalGetter::InLeapYear),
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
                        (
                            "getTimeZoneTransition",
                            1,
                            NativeFunction::TemporalZonedDateTimeGetTimeZoneTransition,
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
        // Spec step: "If calendar is not a String, throw a TypeError
        // exception" -- no `ToString` coercion at all, unlike most other
        // Temporal string arguments. Pinned by
        // `calendar-wrong-type.js` (identical fixture across
        // `PlainDate`/`PlainDateTime`/`PlainMonthDay`/`PlainYearMonth`/
        // `ZonedDateTime`'s numeric constructors, all sharing this
        // function): `null`/`Boolean`/`Number`/`BigInt`/`Symbol`/a plain
        // object/a `Temporal.Duration` instance must all throw `TypeError`
        // immediately rather than being stringified first.
        let Value::String(value) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal calendar must be a string".into(),
            ));
        };
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
    /// `"T11:30[u-ca=hebrew]"` to `withCalendar`, and an *unannotated* ISO
    /// string like `"2020-01-01"` or `"2020-01"` is equally valid and
    /// always means `"iso8601"`. A bare calendar ID wins outright (step 3,
    /// e.g. `"gregory"`); otherwise the whole string must parse as *some*
    /// recognized Temporal string shape (step 4 — date-time, year-month,
    /// month-day or time, matching `TemporalCalendarString`'s own grammar
    /// alternation), whose calendar annotation (if any) is the result — an
    /// unannotated but otherwise syntactically valid string implies
    /// `"iso8601"`, it is not itself tried as a calendar-ID literal. Before
    /// this fix, a bracket-less date-like string always fell straight to
    /// `canonical_calendar_id(&value)` and always failed, since no real
    /// calendar ID looks like a date — pinned by
    /// `PlainYearMonth/PlainMonthDay/prototype/equals/
    /// argument-propertybag-calendar-iso-string.js`'s eight unannotated/
    /// annotated date, dateTime, year-month and month-day forms.
    ///
    /// A Temporal object with its own `[[Calendar]]` slot returns that
    /// calendar directly (`ToTemporalCalendar` step 1.a's fast path,
    /// pinned by `TemporalHelpers.checkToTemporalCalendarFastPath`, which
    /// makes both a `calendar`/`calendarId` JS-visible property throw if
    /// read); only `PlainDate`/`PlainDateTime`/`PlainMonthDay`/
    /// `PlainYearMonth`/`ZonedDateTime` carry `[[InitializedTemporalXxx]]`
    /// calendar semantics — `Temporal.Duration`/`Instant`/`PlainTime` are
    /// calendar-less and fall through to the plain-string handling below,
    /// where a non-`String` value is a `TypeError`, not coerced via
    /// `ToString` the way a plain calendar-agnostic string argument would
    /// be (`argument-propertybag-calendar-wrong-type.js`'s ten cases:
    /// `null`/`Boolean`/`Number`/`BigInt`/`Symbol`/a plain object/a
    /// non-fast-path `Temporal.Duration` instance — this function is also
    /// exactly what a `Duration.prototype.round`'s `relativeTo` property
    /// bag's own `calendar` field goes through, per
    /// `relativeto-propertybag-calendar-wrong-type.js`).
    fn temporal_calendar_identifier(&mut self, value: &Value) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok("iso8601".into());
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if matches!(
                    temporal.kind,
                    TemporalKind::PlainDate
                        | TemporalKind::PlainDateTime
                        | TemporalKind::PlainMonthDay
                        | TemporalKind::PlainYearMonth
                        | TemporalKind::ZonedDateTime
                ) {
                    return Ok(temporal.calendar);
                }
            }
        }
        let Value::String(value) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal calendar must be a string".into(),
            ));
        };
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar".into()))?;
        if let Some(id) = canonical_calendar_id(&value) {
            return Ok(id);
        }
        let parsed = iso::parse_date_time(&value)
            .or_else(|| iso::parse_year_month(&value))
            .or_else(|| iso::parse_month_day(&value))
            .or_else(|| iso::parse_time(&value));
        if let Some(parsed) = parsed {
            let calendar = parsed.calendar.as_deref().unwrap_or("iso8601");
            return canonical_calendar_id(calendar)
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal calendar".into()));
        }
        Err(RuntimeError::RangeError("invalid Temporal calendar".into()))
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
        // `-9_999..=9_999` was too narrow: a `PlainDate`/`PlainDateTime`/
        // `ZonedDateTime` property bag's `year` field is a plain integer
        // with no bound of its own (`ToIntegerWithTruncation` doesn't clamp
        // it) — the *real* representable-range check happens afterward,
        // once an actual calendar date exists
        // (`epoch::is_date_within_limits`/`is_date_time_within_limits`).
        // `relativeto-date-limits.js`'s extreme-year property bags (the
        // exact `-271821`/`275760` boundary) are what surfaced this —
        // reached via `Temporal.Duration`'s own `relativeTo` reuse of this
        // function, though the same bound applied to every other caller too.
        let requested_year = (!matches!(year, Value::Undefined))
            .then(|| self.temporal_integer(&year, -275_760, 275_760, "year"))
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
        if matches!(day, Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Temporal date fields require day".into(),
            ));
        }
        // `1..=i32::MAX`, not `1..=31` -- `ToPositiveIntegerWithTruncation`
        // has no upper bound at all: a raw property-bag `day` beyond a
        // month's real length must reach the calendar's own overflow-aware
        // `Date::try_from_fields` below (which throws under `"reject"` and
        // clamps under the default `"constrain"`), not be rejected here
        // before overflow ever gets a say. Every `.with()`-style call site
        // in this file already uses this exact same widened bound/cast
        // shape (e.g. `temporal_zoned_date_time_with`'s own `requested_day`).
        fields.day = Some(self.temporal_integer(&day, 1, i32::MAX, "day")? as u8);
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
            // A leap second (`60`) is always constrained to `59`, matching
            // the ISO-string grammar's own `:60` handling (`iso.rs`'s
            // `parse_time_spec`) — Temporal has no internal leap-second
            // representation, so a property bag's `second: 60` must be
            // tolerated the same way rather than rejected outright
            // (`relativeto-leap-second.js`, reached via `Temporal.Duration`'s
            // own `relativeTo` property-bag path, which -- unlike a bare
            // `PlainDate` bag -- now reads this field too).
            value.second = self.temporal_optional_integer(&second, 0, 0, 60, "second")?.min(59) as u8;
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
                // The per-field `-271_821..=275_760` bound above is coarser
                // than the exact representable-range boundary (a
                // day-and-nanosecond boundary, not a year one --
                // `+275760-09-13` is the true maximum, so `+275760-09-14`
                // must still throw even though every individual field is
                // itself in range). Pinned by
                // `PlainMonthDay/refisoyear-out-of-range.js`.
                if !epoch::is_date_within_limits((value.year, value.month, value.day)) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay reference year is outside the supported range"
                            .into(),
                    ));
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
                // As with `PlainMonthDay` above: the per-field year bound is
                // coarser than the true representable-range boundary, which
                // is a *month* boundary within the min/max year, not a
                // whole-year one (`-271821-04` is valid, `-271821-03` is
                // not; `+275760-09` is valid, `+275760-10` is not) --
                // `referenceISODay` never affects this, only `year`/`month`
                // do. Pinned by `PlainYearMonth/limits.js`.
                if !iso::is_year_month_within_limits(value.year, value.month) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainYearMonth is outside the supported range".into(),
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
                                | TemporalKind::ZonedDateTime
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
                                | TemporalKind::PlainYearMonth
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
                                | TemporalKind::PlainYearMonth
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
                                | TemporalKind::PlainYearMonth
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
            .then(|| self.temporal_integer(&era_year_v, i32::MIN, i32::MAX, "era year"))
            .transpose()?;
        // `CalendarFields.cpp`'s `NonISOResolveFields`/`NonISOFieldKeysToIgnore`
        // (ported here the same way `temporal_year_month_with` already does
        // for `PlainYearMonth`): the `iso8601` calendar has no eras at all
        // (per the fix in `temporal_calendar_fields` above) — an
        // `era`/`eraYear` property is still read (for property-bag
        // ordering) but never applied to field resolution for it, matching
        // Test262's `with/time-units-ignored.js` (`{ day: 30, era: "BC" }`
        // on an ISO `PlainDate` simply changes `day`, `era` is inert).
        // `chinese`/`dangi` are different from `iso8601` here: ICU4X has no
        // era concept for them either, but Temporal's own behavior is to
        // *reject* any use of `era`/`eraYear` rather than silently ignore
        // it (`mutually-exclusive-fields-{chinese,dangi}.js`). On any
        // calendar that *does* support eras, `era` and `eraYear` must be
        // supplied together or not at all — providing exactly one is a
        // `TypeError` (`mutually-exclusive-fields-*.js`'s trailing
        // `assert.throws(TypeError, ...)` pair), and this check must run
        // before any `RangeError` from an out-of-range/conflicting
        // month/day field (`calendarresolvefields-error-ordering-*.js`),
        // which is why it happens here, before the month/day fields below
        // are even parsed.
        if existing.calendar == "iso8601" {
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        } else if !calendar::calendar_supports_era(&existing.calendar) {
            if era_s.is_some() || era_year_num.is_some() {
                return Err(RuntimeError::TypeError(
                    "era and eraYear are not valid for this calendar".into(),
                ));
            }
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        } else {
            match (era_s.as_deref(), era_year_num) {
                (Some(era), Some(era_year)) => {
                    fields.era = Some(era.as_bytes());
                    fields.era_year = Some(era_year);
                }
                (Some(_), None) => {
                    return Err(RuntimeError::TypeError(
                        "Temporal.with requires eraYear when era is provided".into(),
                    ));
                }
                (None, Some(_)) => {
                    return Err(RuntimeError::TypeError(
                        "Temporal.with requires era when eraYear is provided".into(),
                    ));
                }
                (None, None) => {
                    fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
                }
            }
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
        // `ToPositiveIntegerWithTruncation`: `day` has no upper bound at the
        // field-reading stage (`CalendarFields.cpp`) — the real range check
        // happens once, below, against the calendar's own `overflow`
        // regulation, matching `plain_month_day.rs`'s identical fix and
        // Test262's `wrapping-at-end-of-month-*.js` (`date.with({ day:
        // daysInMonth + 1 })` constrains rather than field-bound-rejecting).
        let requested_day = (!matches!(day_v, Value::Undefined))
            .then(|| self.temporal_integer(&day_v, 1, i32::MAX, "day"))
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
        // Both rounding steps below (`round_calendar_duration` for
        // day/week/month/year granularity, `TimeDuration::round` for
        // sub-day granularity) round a *real*, direction-aware signed
        // quantity computed in the fixed receiver-to-argument direction —
        // `Ceil`/`Floor` round toward a fixed end of the real number line
        // (`ceil(-x) == -floor(x)`, not `-ceil(x)`), and `HalfCeil`/
        // `HalfFloor` are the half-mode analogue. Negating the *result* for
        // `since` without also reflecting an asymmetric mode here would
        // silently round the wrong way whenever `since` negates a
        // non-exact value — exactly the same bug
        // `temporal_year_month_difference` (`PlainYearMonth`) already had
        // fixed for it (see that function's own comment). Confirmed via
        // `built-ins/Temporal/{PlainDate,PlainDateTime}/prototype/since/
        // roundingmode-{ceil,floor}.js`. `Trunc`/`Expand`/`HalfExpand`/
        // `HalfTrunc`/`HalfEven` are all symmetric under negation and need
        // no reflection.
        let effective_mode = if since {
            use blueice_ecma402::NumberRoundingMode as Mode;
            match mode {
                Mode::Ceil => Mode::Floor,
                Mode::Floor => Mode::Ceil,
                Mode::HalfCeil => Mode::HalfFloor,
                Mode::HalfFloor => Mode::HalfCeil,
                other => other,
            }
        } else {
            mode
        };

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
                effective_mode,
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
                .round(time_unit, increment, effective_mode);
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

    /// `Temporal.PlainDate.prototype.toLocaleString`/
    /// `Temporal.PlainDateTime.prototype.toLocaleString` share this adapter
    /// (the same runtime-`TemporalKind`-dispatch pattern every other shared
    /// `PlainDate`/`PlainDateTime` method here uses), but `CreateDateTimeFormat`'s
    /// `required` parameter differs between the two: `PlainDate`'s own is
    /// DATE, which rejects any `timeStyle` option unconditionally -- even
    /// alongside a `dateStyle` that would otherwise make the value visible
    /// (`intl402/.../PlainDate/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`) -- while `PlainDateTime`'s own is ANY, so
    /// `dateStyle`+`timeStyle` together are legal and must still format
    /// (`intl402/.../PlainDateTime/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`). This is the exact mirror of
    /// `temporal_plain_time_to_locale_string`'s own `required = TIME` check
    /// rejecting `dateStyle` unconditionally, flipped to the date side and
    /// scoped to `PlainDate` only.
    pub(super) fn temporal_date_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
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
            if existing.kind == TemporalKind::PlainDate
                && self.date_time_format_data(&formatter)?.options().time_style.is_some()
            {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainDate.prototype.toLocaleString does not accept a timeStyle option"
                        .into(),
                ));
            }
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
            ..Default::default()
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

    /// `ToRelativeTemporalObject`. Resolves every accepted `relativeTo` shape
    /// (`undefined`, a `PlainDate`/`PlainDateTime`/`ZonedDateTime` object, a
    /// string, or a property bag) to a [`DurationAnchor`]. A `PlainYearMonth`/
    /// `PlainMonthDay` object is a real `TypeError` here, not a missing
    /// feature: `relativeto-wrong-type.js` confirms neither is in
    /// `ToRelativeTemporalObject`'s own accepted-object list, independent of
    /// whether those types themselves are otherwise implemented.
    fn temporal_duration_relative_to(
        &mut self,
        value: &Value,
    ) -> Result<Option<DurationAnchor>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(None);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let calendar = calendar::calendar_kind(&temporal.calendar)
                    .expect("Temporal values retain a validated calendar identifier");
                return match temporal.kind {
                    TemporalKind::PlainDate | TemporalKind::PlainDateTime => {
                        Ok(Some(DurationAnchor::Plain {
                            calendar,
                            date: (temporal.year, temporal.month, temporal.day),
                        }))
                    }
                    TemporalKind::ZonedDateTime => {
                        let zone = time_zone::parse_identifier(&temporal.time_zone)
                            .expect("Temporal values retain a validated time zone identifier");
                        Ok(Some(DurationAnchor::Zoned {
                            calendar,
                            zone,
                            epoch_ns: temporal.epoch_nanoseconds.clone(),
                            local_date: (temporal.year, temporal.month, temporal.day),
                            local_time: (
                                temporal.hour,
                                temporal.minute,
                                temporal.second,
                                temporal.millisecond,
                                temporal.microsecond,
                                temporal.nanosecond,
                            ),
                        }))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "relativeTo must be a PlainDate, PlainDateTime or ZonedDateTime".into(),
                    )),
                };
            }
            return self
                .temporal_duration_relative_to_property_bag(value)
                .map(Some);
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
        self.temporal_duration_relative_to_string(&source).map(Some)
    }

    /// `ToRelativeTemporalObject`'s property-bag path (`{ year, month, day,
    /// ..., timeZone?, offset?, calendar? }`). A bag with a `timeZone` is
    /// resolved exactly as `Temporal.ZonedDateTime.from` would resolve it
    /// (`temporal_to_zoned_date_time` — the same real zone-offset resolution,
    /// disambiguation and range check, including a named IANA zone now that
    /// Stage 2's `zoned_date_time.rs`/`time_zone.rs` support one). A bag with
    /// no `timeZone` is read exactly as `Temporal.PlainDateTime.from` would
    /// read it (`temporal_plain_date_from_fields` with
    /// `TemporalKind::PlainDateTime`, not `PlainDate`) even though only the
    /// resulting date is kept — `GetTemporalRelativeToOption`'s real
    /// algorithm reads and validates `hour`/`minute`/`second`/`millisecond`/
    /// `microsecond`/`nanosecond` too, discarding their values, which is
    /// exactly what routing through the `PlainDateTime` field set gives for
    /// free (`relativeto-infinity-throws-rangeerror.js`'s time-field cases,
    /// `order-of-operations.js`'s field-presence list).
    fn temporal_duration_relative_to_property_bag(
        &mut self,
        bag: &Value,
    ) -> Result<DurationAnchor, RuntimeError> {
        let time_zone_value = self.get_property(bag, &"timeZone".into())?;
        if time_zone_value != Value::Undefined {
            let temporal = self.temporal_to_zoned_date_time(bag, &Value::Undefined)?;
            let calendar = calendar::calendar_kind(&temporal.calendar)
                .expect("temporal_to_zoned_date_time validates the calendar identifier");
            let zone = time_zone::parse_identifier(&temporal.time_zone)
                .expect("temporal_to_zoned_date_time validates the time zone identifier");
            return Ok(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns: temporal.epoch_nanoseconds.clone(),
                local_date: (temporal.year, temporal.month, temporal.day),
                local_time: (
                    temporal.hour,
                    temporal.minute,
                    temporal.second,
                    temporal.millisecond,
                    temporal.microsecond,
                    temporal.nanosecond,
                ),
            });
        }
        let temporal =
            self.temporal_plain_date_from_fields(TemporalKind::PlainDateTime, bag, false)?;
        // `offset` is meaningless without a `timeZone`, but its *presence* is
        // still observable: a non-string, non-undefined value is a
        // `TypeError`, matching `GetTemporalRelativeToOption`'s own read.
        let offset_value = self.get_property(bag, &"offset".into())?;
        if !matches!(offset_value, Value::Undefined) {
            self.coerce_string(&offset_value)?;
        }
        let calendar = calendar::calendar_kind(&temporal.calendar)
            .expect("temporal_plain_date_from_fields validates the calendar identifier");
        Ok(DurationAnchor::Plain {
            calendar,
            date: (temporal.year, temporal.month, temporal.day),
        })
    }

    /// `ToRelativeTemporalObject`'s string path. A bracketed time-zone
    /// annotation (or a bare `Z`/`z` with none) names a real `Zoned` anchor —
    /// resolved via the same `temporal_value_from_zoned_date_time_string`
    /// `Temporal.ZonedDateTime.from` itself uses, including a named IANA zone
    /// now that Stage 2 supports one. A plain date(-time) string is a `Plain`
    /// anchor, resolved only against `PlainDate`'s own (looser) representable
    /// range at this stage — `temporal_value_from_string(PlainDate, ...)`,
    /// not `PlainDateTime` — since `ToRelativeTemporalObject` itself only
    /// ever needs a valid `PlainDate`; the tighter `PlainDateTime`
    /// (isoDateTime) boundary is a separate, later check that only applies
    /// once real date arithmetic is attempted
    /// (`temporal_duration_anchor_datetime_in_range`), which is exactly what
    /// `relativeto-string-limits.js`'s "valid ... but fails after early
    /// return" cases pin: a blank `Duration` never needs to convert the
    /// anchor to an isoDateTime at all, so it accepts a date string that a
    /// nonblank one rejects.
    fn temporal_duration_relative_to_string(
        &mut self,
        source: &str,
    ) -> Result<DurationAnchor, RuntimeError> {
        let parsed = iso::parse_date_time(source)
            .ok_or_else(|| RuntimeError::RangeError("invalid relativeTo string".into()))?;
        // A bare `Z` with no bracketed time-zone annotation names no real
        // zone at all, so it can resolve to neither a `ZonedDateTime` (no
        // identifier) nor a `PlainDateTime` (`Z` asserts an exact instant, a
        // contradiction for a wall-clock type) — a straight `RangeError`,
        // confirmed by `relativeto-string-invalid.js`'s own
        // `"2019-11-01T00:00Z"` case (contrast the accepted
        // `"...Z[-07:00]"`, which does carry a real identifier).
        if parsed.utc_designator && parsed.time_zone.is_none() {
            return Err(RuntimeError::RangeError(
                "a relativeTo string with a UTC designator needs a time-zone annotation".into(),
            ));
        }
        if parsed.time_zone.is_some() {
            let temporal = Self::temporal_value_from_zoned_date_time_string(
                source,
                time_zone::Disambiguation::Compatible,
                "reject",
            )?;
            let calendar = calendar::calendar_kind(&temporal.calendar)
                .expect("temporal_value_from_zoned_date_time_string validates the calendar");
            let zone = time_zone::parse_identifier(&temporal.time_zone)
                .expect("temporal_value_from_zoned_date_time_string validates the time zone");
            return Ok(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns: temporal.epoch_nanoseconds.clone(),
                local_date: (temporal.year, temporal.month, temporal.day),
                local_time: (
                    temporal.hour,
                    temporal.minute,
                    temporal.second,
                    temporal.millisecond,
                    temporal.microsecond,
                    temporal.nanosecond,
                ),
            });
        }
        let temporal = self.temporal_value_from_string(TemporalKind::PlainDate, source)?;
        let calendar = calendar::calendar_kind(&temporal.calendar)
            .expect("temporal_value_from_string validates the calendar identifier");
        Ok(DurationAnchor::Plain {
            calendar,
            date: (temporal.year, temporal.month, temporal.day),
        })
    }

    /// The deferred half of a `Plain` anchor's representable-range check: a
    /// date string/object/property-bag anchor only ever needs to be a valid
    /// `PlainDate` to *resolve* (see `temporal_duration_relative_to_string`'s
    /// own doc comment), but every calendar-aware arithmetic path below
    /// converts it to an isoDateTime (implicit midnight) before use, which is
    /// judged against `PlainDateTime`'s tighter, exact range —
    /// `relativeto-string-limits.js`/`relativeto-date-limits.js` pin exactly
    /// this boundary, including that a *blank* `Duration` (which never
    /// reaches this check, short-circuiting first) accepts a date the tighter
    /// check alone would reject.
    fn temporal_duration_anchor_datetime_in_range(
        date: epoch::CivilDate,
    ) -> Result<(), RuntimeError> {
        if !epoch::is_date_time_within_limits(date, (0, 0, 0, 0, 0, 0)) {
            return Err(RuntimeError::RangeError(
                "relativeTo is outside the representable range for a relativeTo parameter after \
                 conversion to DateTime"
                    .into(),
            ));
        }
        Ok(())
    }

    /// The full duration's target instant: `AddZonedDateTime(relativeTo,
    /// internalDuration, constrain)`, per `Duration.prototype.round`/`total`/
    /// static `compare`'s own Step "27.e"/equivalent — every `Zoned`-anchor
    /// arithmetic path below range-checks *this* exact instant first (not an
    /// approximation), matching `throws-if-target-nanoseconds-outside-valid-
    /// limits.js`/`relativeto-zoneddatetime-large-time-component-out-of-
    /// range.js`.
    /// `UTC` or a fixed numeric offset — a day is always exactly 86,400
    /// seconds under either, unlike a real named IANA zone. See
    /// `temporal_duration_round`'s own Zoned dispatch for why this matters:
    /// the already-shipped `Plain`-anchor algorithm is exact (not just an
    /// approximation) whenever this holds.
    fn temporal_duration_zone_is_fixed(zone: &time_zone::TimeZone) -> bool {
        matches!(zone, time_zone::TimeZone::Offset(_) | time_zone::TimeZone::Iana("UTC"))
    }

    fn temporal_duration_zoned_target(
        zone: &time_zone::TimeZone,
        calendar: AnyCalendarKind,
        anchor_epoch_ns: &BigInt,
        anchor_date: epoch::CivilDate,
        anchor_time: epoch::CivilTime,
        record: &blueice_ecma402::DurationRecord,
    ) -> Result<BigInt, RuntimeError> {
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds();
        let target = zoned_date_time::add_zoned_date_time(
            zone,
            calendar,
            anchor_epoch_ns,
            anchor_date,
            anchor_time,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            record.days as i64,
            time_total,
            false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        if !epoch::is_in_instant_range(&target) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range".into(),
            ));
        }
        Ok(target)
    }

    /// `NudgeToZonedTime` (`Duration.prototype.round`/`total`'s `smallestUnit`
    /// finer than `day` path with a `Zoned` anchor), ported from Gecko's
    /// `reference/gecko/js/src/builtin/temporal/Duration.cpp`. Unlike a
    /// `Plain` anchor's fixed-86,400-second day, this needs the receiver's
    /// *own* day (the calendar-date part of the duration, landed through the
    /// zone) real length before it can decide whether a rounded time
    /// remainder overflows it — the two-stage rounding below (round the raw
    /// time part first, then, only if it overflows the day, round the
    /// *excess* again) is the exact mechanism `case-where-relativeto-
    /// affects-rounding-mode-half-even.js`, `adjust-rounded-duration-
    /// days.js` and `dst-balancing-result.js` pin: a single "round, then
    /// subtract the day length" pass gives a different (wrong) answer
    /// whenever the excess itself needs re-rounding to the increment (e.g.
    /// 13h rounded up to the next 12h increment relative to a 23-hour day is
    /// 1 day *12* hours, not 1 day *1* hour).
    ///
    /// Returns the `(years, months, weeks, days, hours, minutes, seconds,
    /// milliseconds, microseconds, nanoseconds)` result fields directly —
    /// `days` here is `record`'s own `days` field plus at most one more (the
    /// "did this roll into the next/previous day" carry), never re-derived
    /// via a calendar difference, matching Gecko's own
    /// `dateDuration.days = duration.date.days + dayDelta` (a
    /// `calendar_difference_date` re-split would double-count whenever
    /// `record` already carries independent `years`/`months`/`weeks`).
    #[allow(clippy::too_many_arguments)]
    fn temporal_duration_nudge_to_zoned_time(
        zone: &time_zone::TimeZone,
        calendar: AnyCalendarKind,
        anchor_date: epoch::CivilDate,
        anchor_time: epoch::CivilTime,
        anchor_epoch_ns: &BigInt,
        record: &blueice_ecma402::DurationRecord,
        smallest: rounding::TemporalUnit,
        largest: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<[i128; 10], RuntimeError> {
        let sign: i64 = if record.sign() < 0 { -1 } else { 1 };
        let range_error = || {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        };
        // Step 1-2: `start` is the receiver's own date part landed through
        // the calendar (constrain), at the receiver's own local time.
        let start_date = plain_date::calendar_add_date(
            calendar,
            anchor_date,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            record.days as i64,
            false,
        )
        .ok_or_else(range_error)?;
        // Step 3-4: `end` is one calendar day further in the duration's own
        // direction — both endpoints must themselves be representable.
        let end_date =
            plain_date::add_iso_date(start_date, 0, 0, 0, sign, false).ok_or_else(range_error)?;
        if !epoch::is_date_time_within_limits(end_date, anchor_time) {
            return Err(range_error());
        }
        // Step 5-8: the *real* elapsed length of that specific day.
        let start_ns = zone
            .epoch_nanoseconds_for(start_date, anchor_time, time_zone::Disambiguation::Compatible)
            .map_err(|_| range_error())?;
        let end_ns = zone
            .epoch_nanoseconds_for(end_date, anchor_time, time_zone::Disambiguation::Compatible)
            .map_err(|_| range_error())?;
        let day_span = i128::try_from(&end_ns - &start_ns).map_err(|_| range_error())?;
        // Steps 9-10: round the receiver's own exact time part first.
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds();
        let time_unit = Self::temporal_unit_to_time_unit(smallest);
        let rounded_time =
            duration_math::TimeDuration::from_nanoseconds(time_total).round(time_unit, increment, mode);
        // Step 11: does the rounded time part reach past this specific day?
        let beyond_day_span = rounded_time.total_nanoseconds() - day_span;
        let beyond_sign = beyond_day_span.signum();
        let (day_delta, final_time_ns, nudged_ns) = if beyond_sign != -sign as i128 {
            // Step 12: round the *excess* again, to the same increment —
            // never just `beyond_day_span` unrounded.
            let re_rounded = duration_math::TimeDuration::from_nanoseconds(beyond_day_span)
                .round(time_unit, increment, mode);
            (sign, re_rounded.total_nanoseconds(), &end_ns + BigInt::from(re_rounded.total_nanoseconds()))
        } else {
            // Step 13: the rounded time already fits inside this day.
            (0_i64, rounded_time.total_nanoseconds(), &start_ns + BigInt::from(rounded_time.total_nanoseconds()))
        };
        if !epoch::is_in_instant_range(&nudged_ns) {
            return Err(range_error());
        }
        let total_days = record.days + i128::from(day_delta);
        // `largest` finer than `day`: no date field is allowed in the
        // output at all (`Temporal.Duration` never mixes a `days` field with
        // an `hours` `largestUnit`), so the *whole* date part — years,
        // months, weeks, and the (possibly nudged) day count — must convert
        // to its exact elapsed nanoseconds through the real zone (never a
        // flat 24-hour assumption) before combining with the already-nudged
        // time remainder. `dst-balancing-result.js`'s `largestUnit: "hours"`
        // cases (`1 day` reported as `25 hours` across a repeated hour) are
        // exactly this path.
        if largest < rounding::TemporalUnit::Day {
            let date_only_ns = zoned_date_time::add_zoned_date_time(
                zone,
                calendar,
                anchor_epoch_ns,
                anchor_date,
                anchor_time,
                record.years as i64,
                record.months as i64,
                record.weeks as i64,
                total_days as i64,
                0,
                false,
            )
            .ok_or_else(range_error)?;
            if !epoch::is_in_instant_range(&date_only_ns) {
                return Err(range_error());
            }
            let elapsed_ns =
                i128::try_from(&date_only_ns - anchor_epoch_ns).map_err(|_| range_error())?;
            let total_ns = elapsed_ns + final_time_ns;
            let balanced = duration_math::TimeDuration::from_nanoseconds(total_ns)
                .balance_to(Self::temporal_unit_to_time_unit(largest));
            return Ok([
                0,
                0,
                0,
                0,
                i128::from(balanced[0]),
                i128::from(balanced[1]),
                i128::from(balanced[2]),
                i128::from(balanced[3]),
                i128::from(balanced[4]),
                i128::from(balanced[5]),
            ]);
        }
        // `largest` is `day` or coarser: the date part is re-decomposed at
        // `largest`'s own granularity via the real landing date — matching
        // the already-shipped `Plain` anchor's `temporal_duration_round_
        // calendar_exact` shape and `Self::temporal_duration_round_zoned_
        // calendar_unit`'s own identical fix (see that function's doc
        // comment): `record`'s raw `years`/`months`/`weeks`/`days` split
        // does not automatically match how those fields re-express at a
        // coarser `largestUnit` (`rounding-increments.js`'s zoned case).
        let landing = plain_date::calendar_add_date(
            calendar,
            anchor_date,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            total_days as i64,
            false,
        )
        .ok_or_else(range_error)?;
        let (years, months, weeks, days) = plain_date::calendar_difference_date(
            calendar,
            anchor_date,
            landing,
            Self::temporal_unit_to_date_unit(largest),
        );
        let balanced =
            duration_math::TimeDuration::from_nanoseconds(final_time_ns).balance_to(rounding::TimeUnit::Hour);
        Ok([
            i128::from(years),
            i128::from(months),
            i128::from(weeks),
            i128::from(days),
            i128::from(balanced[0]),
            i128::from(balanced[1]),
            i128::from(balanced[2]),
            i128::from(balanced[3]),
            i128::from(balanced[4]),
            i128::from(balanced[5]),
        ])
    }

    /// `ComputeNudgeWindow`'s bracket computation (`Duration.prototype.round`/
    /// `total`'s `smallestUnit` of `day`/`week`/`month`/`year` with a `Zoned`
    /// anchor), ported from the same Gecko source. Unlike
    /// `temporal_duration_round_calendar_exact` (the `Plain`-anchor
    /// equivalent, which brackets by *epoch day count* — exact only because a
    /// `Plain` day is always fixed at 86,400 seconds), this brackets by *real
    /// epoch nanoseconds* through the zone, which is what makes month/year
    /// rounding land on the correct fractional position across a DST
    /// transition (`dst-rounding-result.js`'s "exactly 1.5 months" case).
    ///
    /// Returns `(r1, start_epoch_ns, end_epoch_ns, start_duration,
    /// end_duration)` — `start_duration`/`end_duration` are `[years, months,
    /// weeks, days]`, matching Gecko's own `DateDuration` shape for this
    /// window.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn temporal_duration_zoned_calendar_window(
        zone: &time_zone::TimeZone,
        calendar: AnyCalendarKind,
        anchor_date: epoch::CivilDate,
        anchor_time: epoch::CivilTime,
        anchor_epoch_ns: &BigInt,
        record: &blueice_ecma402::DurationRecord,
        sign: i64,
        increment: i64,
        unit: rounding::TemporalUnit,
        additional_shift: bool,
    ) -> Result<(i64, BigInt, BigInt, [i64; 4], [i64; 4]), RuntimeError> {
        let range_error = || {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        };
        let shift = if additional_shift { increment * sign } else { 0 };
        let (r1, start_duration, end_duration): (i64, [i64; 4], [i64; 4]) = match unit {
            rounding::TemporalUnit::Year => {
                let years = (record.years as i64 / increment) * increment;
                let r1 = years + shift;
                let r2 = r1 + increment * sign;
                (r1, [r1, 0, 0, 0], [r2, 0, 0, 0])
            }
            rounding::TemporalUnit::Month => {
                let months = (record.months as i64 / increment) * increment;
                let r1 = months + shift;
                let r2 = r1 + increment * sign;
                (
                    r1,
                    [record.years as i64, r1, 0, 0],
                    [record.years as i64, r2, 0, 0],
                )
            }
            rounding::TemporalUnit::Week => {
                let years_months_point = plain_date::calendar_add_date(
                    calendar,
                    anchor_date,
                    record.years as i64,
                    record.months as i64,
                    0,
                    0,
                    false,
                )
                .ok_or_else(range_error)?;
                let weeks_end =
                    plain_date::add_iso_date(years_months_point, 0, 0, 0, record.days as i64, false)
                        .ok_or_else(range_error)?;
                let (_, _, until_weeks, _) = plain_date::calendar_difference_date(
                    calendar,
                    years_months_point,
                    weeks_end,
                    plain_date::DateUnit::Week,
                );
                let weeks = ((record.weeks as i64 + until_weeks) / increment) * increment;
                let r1 = weeks + shift;
                let r2 = r1 + increment * sign;
                (
                    r1,
                    [record.years as i64, record.months as i64, r1, 0],
                    [record.years as i64, record.months as i64, r2, 0],
                )
            }
            _ => {
                let days = (record.days as i64 / increment) * increment;
                let r1 = days + shift;
                let r2 = r1 + increment * sign;
                (
                    r1,
                    [record.years as i64, record.months as i64, record.weeks as i64, r1],
                    [record.years as i64, record.months as i64, record.weeks as i64, r2],
                )
            }
        };
        let resolve = |duration: [i64; 4]| -> Result<BigInt, RuntimeError> {
            if duration == [0, 0, 0, 0] {
                return Ok(anchor_epoch_ns.clone());
            }
            let date = plain_date::calendar_add_date(
                calendar, anchor_date, duration[0], duration[1], duration[2], duration[3], false,
            )
            .ok_or_else(range_error)?;
            // A `Zoned` bracket endpoint is judged against `Instant`'s own
            // (epoch-nanosecond) range, not `PlainDateTime`'s wall-clock-date
            // range: the two are different boundaries, and the latter is too
            // narrow here — a "next bracket" date can legitimately exceed
            // `PlainDateTime`'s exact limit while its real *instant* is still
            // comfortably representable (`relativeto-date-limits.js`'s own
            // max-boundary `total()` cases, which never need this bracket's
            // value for a blank duration but must not spuriously throw while
            // computing it anyway).
            let resolved = zone
                .epoch_nanoseconds_for(date, anchor_time, time_zone::Disambiguation::Compatible)
                .map_err(|_| range_error())?;
            if !epoch::is_in_instant_range(&resolved) {
                return Err(range_error());
            }
            Ok(resolved)
        };
        let start_epoch_ns = resolve(start_duration)?;
        let end_epoch_ns = resolve(end_duration)?;
        Ok((r1, start_epoch_ns, end_epoch_ns, start_duration, end_duration))
    }

    /// Shared by every calendar-unit rounding decision (both the `Plain`
    /// anchor's own inline decision in
    /// [`Self::temporal_duration_round_calendar_exact`] and the `Zoned`
    /// anchor's [`Self::temporal_duration_round_zoned_calendar_unit`]):
    /// decides, from an *exact*, already sign-normalized (non-negative)
    /// `numerator`/`denominator` progress ratio, whether the value rounds up
    /// to its bracket's upper endpoint. `r1`/`increment` is the "cardinality"
    /// `HalfEven` needs (whether the lower candidate's own unit count is
    /// even) — kept as a separate, small, duplicated function rather than
    /// refactoring the already-shipped `Plain` decision inline, per this
    /// pass's own scope discipline (touch only what a new `Zoned` path
    /// needs).
    fn temporal_duration_calendar_round_up(
        numerator: i128,
        denominator: i128,
        sign: i64,
        r1: i64,
        increment: i64,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> bool {
        if denominator == 0 || numerator == 0 {
            return false;
        }
        use blueice_ecma402::NumberRoundingMode as Mode;
        match mode {
            Mode::Ceil => sign > 0,
            Mode::Floor => sign < 0,
            Mode::Expand => true,
            Mode::Trunc => false,
            Mode::HalfCeil => {
                if sign > 0 {
                    2 * numerator >= denominator
                } else {
                    2 * numerator > denominator
                }
            }
            Mode::HalfFloor => {
                if sign < 0 {
                    2 * numerator >= denominator
                } else {
                    2 * numerator > denominator
                }
            }
            Mode::HalfExpand => 2 * numerator >= denominator,
            Mode::HalfTrunc => 2 * numerator > denominator,
            Mode::HalfEven => {
                if 2 * numerator == denominator {
                    (r1 / increment) % 2 != 0
                } else {
                    2 * numerator > denominator
                }
            }
        }
    }

    /// `NudgeToCalendarUnit` (`Duration.prototype.round`'s `smallestUnit` of
    /// `day`/`week`/`month`/`year` with a `Zoned` anchor). `dest_epoch_ns` is
    /// the already-computed, already-range-checked target instant (the whole
    /// original duration applied via [`Self::temporal_duration_zoned_target`]).
    /// Returns the resolved `[years, months, weeks, days]`, in that order —
    /// but re-decomposed at `largest`'s own granularity via
    /// `calendar_difference_date`, matching the already-shipped `Plain`
    /// anchor's `temporal_duration_round_calendar_exact` shape: Gecko's own
    /// `ComputeNudgeWindow` bracket duration only ever carries `record`'s own
    /// raw field split (rounding just `unit`'s own field), which is *not*
    /// automatically expressed at a coarser `largestUnit` — `P7D` rounded to
    /// `smallestUnit: "days"`/`largestUnit: "weeks"` needs to land as
    /// `{ weeks: 1 }`, not `{ days: 7 }`
    /// (`exact-multiple-of-larger-unit-zoned.js`), even though the *value*
    /// (7 exact days) requires no rounding at all. Time fields are always
    /// zero for this branch (`NudgeToCalendarUnit`'s own `{resultDuration,
    /// {}}`).
    #[allow(clippy::too_many_arguments)]
    /// `UnbalanceDateDurationRelative`: folds every field of `record`'s date
    /// part *coarser* than `unit` down to `unit`'s own granularity, via the
    /// real calendar landing date from `anchor` — e.g. unbalanced to `"day"`,
    /// `{ years: 1, hours: 24 }` becomes a flat day count (366 or 367,
    /// depending on the leap year crossed), not `{ years: 1, days: 0 }`.
    /// Without this, [`Self::temporal_duration_zoned_calendar_window`]'s own
    /// bracket (built from `record`'s raw, still-coarse fields) computes a
    /// fractional position *within the `years: 1` bracket* rather than the
    /// duration's true total in `unit`s — `relativeto-string.js`,
    /// `relativeto-total-of-each-unit.js` (`total()`'s own day-granularity
    /// checks) and `exact-multiple-of-larger-unit-zoned.js` (`round()`'s
    /// `smallestUnit: "days"`/`largestUnit: "weeks"` needing `{ weeks: 1 }`)
    /// all need this. `unit == "year"` is a no-op (there is nothing coarser
    /// to unbalance from). Only the date fields differ in the result; time
    /// fields are copied through unchanged.
    fn temporal_duration_unbalance_date_part(
        calendar: AnyCalendarKind,
        anchor_date: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
        unit: rounding::TemporalUnit,
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        let mut result = *record;
        if unit == rounding::TemporalUnit::Year {
            return Ok(result);
        }
        let landing = plain_date::calendar_add_date(
            calendar,
            anchor_date,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            record.days as i64,
            false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        let (years, months, weeks, days) = plain_date::calendar_difference_date(
            calendar,
            anchor_date,
            landing,
            Self::temporal_unit_to_date_unit(unit),
        );
        result.years = i128::from(years);
        result.months = i128::from(months);
        result.weeks = i128::from(weeks);
        result.days = i128::from(days);
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn temporal_duration_round_zoned_calendar_unit(
        zone: &time_zone::TimeZone,
        calendar: AnyCalendarKind,
        anchor_date: epoch::CivilDate,
        anchor_time: epoch::CivilTime,
        anchor_epoch_ns: &BigInt,
        dest_epoch_ns: &BigInt,
        record: &blueice_ecma402::DurationRecord,
        unit: rounding::TemporalUnit,
        largest: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<[i64; 4], RuntimeError> {
        let record = Self::temporal_duration_unbalance_date_part(calendar, anchor_date, record, unit)?;
        let record = &record;
        let sign: i64 = if record.sign() < 0 { -1 } else { 1 };
        let increment_i64 = increment as i64;
        let window = Self::temporal_duration_zoned_calendar_window(
            zone, calendar, anchor_date, anchor_time, anchor_epoch_ns, record, sign,
            increment_i64, unit, false,
        )?;
        let (start_point, end_point) = if sign > 0 {
            (&window.1, &window.2)
        } else {
            (&window.2, &window.1)
        };
        let window = if !(start_point <= dest_epoch_ns && dest_epoch_ns <= end_point) {
            Self::temporal_duration_zoned_calendar_window(
                zone, calendar, anchor_date, anchor_time, anchor_epoch_ns, record, sign,
                increment_i64, unit, true,
            )?
        } else {
            window
        };
        let (r1, start_ns, end_ns, start_duration, end_duration) = window;
        let (mut numerator, mut denominator) = (
            i128::try_from(dest_epoch_ns - &start_ns).map_err(|_| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?,
            i128::try_from(&end_ns - &start_ns).map_err(|_| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?,
        );
        if denominator < 0 {
            numerator = -numerator;
            denominator = -denominator;
        }
        let round_up = Self::temporal_duration_calendar_round_up(
            numerator, denominator, sign, r1, increment_i64, mode,
        );
        let chosen = if round_up { end_duration } else { start_duration };
        let landing = plain_date::calendar_add_date(
            calendar, anchor_date, chosen[0], chosen[1], chosen[2], chosen[3], false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        let (years, months, weeks, days) = plain_date::calendar_difference_date(
            calendar,
            anchor_date,
            landing,
            Self::temporal_unit_to_date_unit(largest),
        );
        Ok([years, months, weeks, days])
    }

    /// `TotalRelativeDuration`'s `Zoned`-anchor path: for `unit` finer than
    /// `day` this is a pure exact-instant ratio (no calendar or zone
    /// consulted beyond the already-computed target instant); for `day` or
    /// coarser it reuses the same real-epoch-nanosecond bracket
    /// [`Self::temporal_duration_zoned_calendar_window`] computes for
    /// `round`, with `increment = 1` and the exact ratio read directly
    /// (`total = r1 + progress × sign`) rather than rounded.
    #[allow(clippy::too_many_arguments)]
    fn temporal_duration_total_zoned(
        zone: &time_zone::TimeZone,
        calendar: AnyCalendarKind,
        anchor_date: epoch::CivilDate,
        anchor_time: epoch::CivilTime,
        anchor_epoch_ns: &BigInt,
        dest_epoch_ns: &BigInt,
        record: &blueice_ecma402::DurationRecord,
        unit: rounding::TemporalUnit,
    ) -> Result<f64, RuntimeError> {
        if unit < rounding::TemporalUnit::Day {
            let diff_ns = i128::try_from(dest_epoch_ns - anchor_epoch_ns).map_err(|_| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?;
            return Ok(rounding::exact_ratio_to_f64(
                diff_ns,
                unit.nanoseconds()
                    .expect("every time unit has an exact length"),
            ));
        }
        let record = Self::temporal_duration_unbalance_date_part(calendar, anchor_date, record, unit)?;
        let record = &record;
        let sign: i64 = if record.sign() < 0 { -1 } else { 1 };
        let (r1, start_ns, end_ns, _, _) = Self::temporal_duration_zoned_calendar_window(
            zone, calendar, anchor_date, anchor_time, anchor_epoch_ns, record, sign, 1, unit, false,
        )?;
        let (mut numerator, mut denominator) = (
            i128::try_from(dest_epoch_ns - &start_ns).map_err(|_| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?,
            i128::try_from(&end_ns - &start_ns).map_err(|_| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?,
        );
        if denominator < 0 {
            numerator = -numerator;
            denominator = -denominator;
        }
        let n = i128::from(r1) * denominator + numerator * i128::from(sign);
        Ok(rounding::exact_ratio_to_f64(n, denominator))
    }

    /// Applies a `Temporal.Duration` record's date part (calendar-aware) and
    /// time part (folded into whole days, exactly — a duration's fields keep
    /// a common sign, so truncating division loses nothing the way it would
    /// for two independent wall-clock endpoints) to `anchor`, returning the
    /// landing date plus the exact leftover sub-day nanosecond remainder.
    /// This is the one place every calendar-aware `round`/`total`/`compare`
    /// path below computes "anchor + this duration".
    fn temporal_duration_intermediate(
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
    ) -> Result<(epoch::CivilDate, i128), RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds();
        let time_days = time_total / DAY_NS;
        let ns_of_day = time_total % DAY_NS;
        let total_days = record.days + time_days;
        let intermediate = plain_date::calendar_add_date(
            calendar,
            anchor,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            total_days as i64,
            false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        // `calendar_add_date` only range-checks calendar-day validity (an
        // i32-year/valid-month-day check), not Temporal's own narrower
        // representable range: a huge `days`/`weeks` field can land on a
        // numerically valid but unrepresentable date (e.g.
        // `Math.trunc(2**53/86400)` days from `2000-01-01`) without
        // otherwise erroring —
        // `compare/duration-out-of-range-added-to-relativeto.js`,
        // `round/relativeto-duration-out-of-range-added-to-relative-date.js`.
        if !epoch::is_date_within_limits(intermediate) {
            return Err(RuntimeError::RangeError(
                "Temporal date arithmetic is out of range".into(),
            ));
        }
        Ok((intermediate, ns_of_day))
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
            let (requested_largest, anchor, increment, mode, requested_smallest) =
                match &shorthand {
                    Some(text) => (
                        UnitOption::Unset,
                        None,
                        1,
                        blueice_ecma402::NumberRoundingMode::HalfExpand,
                        UnitOption::Unit(Self::temporal_duration_unit_name(text, "smallestUnit")?),
                    ),
                    None => {
                        let largest =
                            self.temporal_duration_unit_option(&options, "largestUnit", true)?;
                        let relative_to = self.get_property(&options, &"relativeTo".into())?;
                        let anchor = self.temporal_duration_relative_to(&relative_to)?;
                        let increment = self.temporal_rounding_increment(&options)?;
                        let mode = self.temporal_rounding_mode(
                            &options,
                            blueice_ecma402::NumberRoundingMode::HalfExpand,
                        )?;
                        let smallest =
                            self.temporal_duration_unit_option(&options, "smallestUnit", false)?;
                        (largest, anchor, increment, mode, smallest)
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
            // A `Zoned` anchor always needs its own real-day-length-aware
            // path, regardless of whether any field is blank or a calendar
            // unit is involved: even a *blank* duration still needs to
            // resolve the next day's start to know how long "one day" is
            // here, which can itself throw (`next-day-out-of-range.js`), and
            // even a purely time-granularity, non-calendar round (`smallest`
            // finer than day) needs the receiver's own real day length to
            // decide whether a rounded remainder overflows it
            // (`case-where-relativeto-affects-rounding-mode-half-even.js`).
            if let Some(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns,
                local_date,
                local_time,
            }) = &anchor
            {
                if smallest < rounding::TemporalUnit::Day {
                    let fields = Self::temporal_duration_nudge_to_zoned_time(
                        zone, *calendar, *local_date, *local_time, epoch_ns, &record, smallest,
                        largest, increment, mode,
                    )?;
                    return self.temporal_duration_create(fields);
                }
                // Range-check the full duration's own target instant first
                // (matching `AddZonedDateTime`'s own check), regardless of
                // which algorithm computes the actual field split below.
                let _dest = Self::temporal_duration_zoned_target(
                    zone, *calendar, epoch_ns, *local_date, *local_time, &record,
                )?;
                // A `UTC`/fixed-offset zone has no real DST, so a day is
                // always exactly 86,400 seconds — the already-shipped,
                // already-Test262-verified `Plain`-anchor calendar-exact
                // algorithm (`temporal_duration_round_relative`) is exact
                // here and, unlike this pass's own from-scratch `Zoned`
                // `NudgeToCalendarUnit` port, correctly handles a
                // `smallestUnit`/`largestUnit` pair that cross a `week`
                // boundary (`exact-multiple-of-larger-unit-zoned.js`'s own
                // `smallestUnit: "weeks"`/`largestUnit: "months"` case) —
                // porting that interaction exactly is left open, see this
                // phase's own PLAN.md entry.
                if Self::temporal_duration_zone_is_fixed(zone) {
                    return self.temporal_duration_round_relative(
                        *calendar,
                        *local_date,
                        &record,
                        largest,
                        smallest,
                        increment,
                        mode,
                    );
                }
                let [years, months, weeks, days] =
                    Self::temporal_duration_round_zoned_calendar_unit(
                        zone, *calendar, *local_date, *local_time, epoch_ns, &_dest, &record,
                        smallest, largest, increment, mode,
                    )?;
                return self.temporal_duration_create([
                    i128::from(years),
                    i128::from(months),
                    i128::from(weeks),
                    i128::from(days),
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ]);
            }
            // A blank duration rounds to a blank duration in every unit: zero
            // is an exact multiple of any increment, and balancing zero
            // yields zero. Given an anchor, that is the whole answer even for
            // a calendar unit, with no calendar arithmetic involved.
            if anchor.is_some() && record.sign() == 0 {
                return self.temporal_duration_create([0; 10]);
            }
            if let Some(anchor) = &anchor {
                Self::temporal_duration_anchor_datetime_in_range(anchor.date())?;
            }
            let needs_calendar = Self::temporal_duration_largest_unit(&record).is_calendar()
                || largest.is_calendar()
                || smallest.is_calendar();
            if needs_calendar {
                let anchor = anchor.ok_or_else(|| {
                    RuntimeError::RangeError(
                        "a Temporal.Duration with years, months or weeks needs a relativeTo \
                         anchor"
                            .into(),
                    )
                })?;
                return self.temporal_duration_round_relative(
                    anchor.calendar(),
                    anchor.date(),
                    &record,
                    largest,
                    smallest,
                    increment,
                    mode,
                );
            }
            let step = smallest
                .nanoseconds()
                .expect("a non-calendar smallestUnit always has an exact length")
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

    /// The calendar-aware half of `round`: `smallestUnit`/`largestUnit`
    /// involves a `year`/`month`/`week`, or the receiver's own largest
    /// nonzero field does, so the answer needs `anchor + record`'s real
    /// calendar-date landing rather than a fixed-length nanosecond total.
    /// Mirrors `plain_date::round_calendar_duration`'s own algorithm shape
    /// (`temporal_date_difference` uses the identical split, between two
    /// already-known dates instead of an anchor plus a duration to add).
    #[allow(clippy::too_many_arguments)]
    fn temporal_duration_round_relative(
        &mut self,
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
        largest: rounding::TemporalUnit,
        smallest: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<Value, RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds();
        if smallest >= rounding::TemporalUnit::Day {
            // Deliberately *not* `plain_date::round_calendar_duration` here:
            // that function only ever sees a whole-day remainder (it takes
            // two already-known dates), so a duration whose only remaining
            // content below `largestUnit` is a sub-day time part would lose
            // exactly the precision `ceil`/`floor`/`halfEven`/etc. need to
            // decide whether that remainder rounds up — Test262's
            // `roundingmode-ceil.js` is what catches this (a `largestUnit:
            // "years"`/no explicit `smallestUnit` case whose only remaining
            // content is ~40.5 leftover hours must still round the day count
            // up under "ceil"). This reimplements the same bracketing shape
            // `round_calendar_duration`/its private `round_month_or_year`
            // use, but carries the exact nanosecond remainder all the way
            // through the rounding decision instead of pre-folding it into a
            // possibly-truncated day count.
            return self.temporal_duration_round_calendar_exact(
                calendar, anchor, record, time_total, largest, smallest, increment, mode,
            );
        }
        let (intermediate, ns_of_day) =
            Self::temporal_duration_intermediate(calendar, anchor, record)?;
        // `smallest` is a time unit: round the exact sub-day remainder first,
        // then recombine with the whole-day calendar difference — the same
        // shape `temporal_date_difference`'s own sub-day branch uses, with
        // `anchor`/`intermediate` standing in for that function's `from`/
        // `adjusted_to` and `ns_of_day` standing in for its `time_diff` (both
        // already exact and sign-consistent, so no day-adjustment step is
        // needed here the way two independent wall-clock endpoints require).
        let time_unit = match smallest {
            rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
            rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
            rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
            rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
            rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
            _ => rounding::TimeUnit::Nanosecond,
        };
        let rounded =
            duration_math::TimeDuration::from_nanoseconds(ns_of_day).round(time_unit, increment, mode);
        let total = rounded.total_nanoseconds();
        let day_carry = total / DAY_NS;
        let ns_of_day = total % DAY_NS;
        let time_largest = if largest >= rounding::TemporalUnit::Day {
            rounding::TimeUnit::Hour
        } else {
            match largest {
                rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
                rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
                rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
                rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
                rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
                _ => rounding::TimeUnit::Nanosecond,
            }
        };
        let balanced = duration_math::TimeDuration::from_nanoseconds(ns_of_day).balance_to(time_largest);
        let whole_days = plain_date::calendar_difference_date(
            calendar,
            anchor,
            intermediate,
            plain_date::DateUnit::Day,
        )
        .3;
        let total_days = whole_days + day_carry as i64;
        let day_target = plain_date::calendar_add_date(calendar, anchor, 0, 0, 0, total_days, false)
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?;
        let (years, months, weeks, days) = plain_date::calendar_difference_date(
            calendar,
            anchor,
            day_target,
            Self::temporal_unit_to_date_unit(largest),
        );
        self.temporal_duration_create([
            years as i128,
            months as i128,
            weeks as i128,
            days as i128,
            i128::from(balanced[0]),
            i128::from(balanced[1]),
            i128::from(balanced[2]),
            i128::from(balanced[3]),
            i128::from(balanced[4]),
            i128::from(balanced[5]),
        ])
    }

    /// `round`'s calendar-aware `day`/`week`/`month`/`year`-granularity
    /// rounding, keeping the exact sub-day nanosecond remainder alive all
    /// the way through the rounding decision (see the caller's own comment
    /// for why `plain_date::round_calendar_duration` can't be reused
    /// directly here).
    ///
    /// Also fixes a real, separate discrepancy found while deriving this
    /// against `roundingmode-ceil.js`'s own `weeks` case:
    /// `round_calendar_duration`'s `Week` branch only places its rounded
    /// value in the `weeks` output field when `largestUnit` is itself
    /// `"weeks"`, folding it into `days` (as an always-multiple-of-7 value)
    /// otherwise — but Temporal's actual field-population rule is that
    /// `weeks` appears whenever `smallestUnit` is `"weeks"`, regardless of
    /// `largestUnit` (`{ largestUnit: "years", smallestUnit: "weeks" }` on a
    /// multi-year duration still reports a real `weeks` field alongside
    /// `years`/`months`, never a `days` value in the hundreds). This
    /// function's own `smallest == Week` handling corrects that locally
    /// rather than by editing the shared, already-merged
    /// `plain_date::round_calendar_duration` (out of this pass's file
    /// scope — see this phase's own scope notes); `temporal_date_difference`
    /// (`PlainDate`/`PlainDateTime.prototype.since`/`until`) calls the
    /// unmodified original directly and likely has the identical gap for
    /// the same option combination, which is worth its own owner's
    /// attention.
    #[allow(clippy::too_many_arguments)]
    fn temporal_duration_round_calendar_exact(
        &mut self,
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
        time_total: i128,
        largest: rounding::TemporalUnit,
        smallest: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<Value, RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let date_unit_largest = Self::temporal_unit_to_date_unit(largest);
        // The date-only landing point (no time contribution at all yet) —
        // used both to find the exact pre-rounding remainder below
        // `largestUnit` and, for `month`/`year`, as the fractional-position
        // anchor `round_month_or_year` itself would use.
        let date_only = plain_date::calendar_add_date(
            calendar, anchor, record.years as i64, record.months as i64, record.weeks as i64,
            record.days as i64, false,
        )
        .ok_or_else(|| RuntimeError::RangeError("Temporal date arithmetic is out of range".into()))?;
        // See `temporal_duration_intermediate`'s own identical comment:
        // `calendar_add_date` alone doesn't catch a numerically valid but
        // unrepresentable landing date. Checked against the *whole* date part
        // including the time component folded into whole days (not just
        // `date_only`, which omits it), since a huge time component alone
        // (e.g. `Number.MAX_SAFE_INTEGER` seconds, `record.days == 0`) is
        // exactly what
        // `relativeto-plaindate-large-time-component-out-of-range.js` checks
        // for every `smallestUnit` (`year`/`month`/`week`), not only the
        // `Day`/`Week` branch below.
        let time_folded_days = record.days + time_total / DAY_NS;
        let date_with_time = plain_date::calendar_add_date(
            calendar, anchor, record.years as i64, record.months as i64, record.weeks as i64,
            time_folded_days as i64, false,
        )
        .ok_or_else(|| RuntimeError::RangeError("Temporal date arithmetic is out of range".into()))?;
        if !epoch::is_date_within_limits(date_only) || !epoch::is_date_within_limits(date_with_time)
        {
            return Err(RuntimeError::RangeError(
                "Temporal date arithmetic is out of range".into(),
            ));
        }

        if matches!(smallest, rounding::TemporalUnit::Day | rounding::TemporalUnit::Week) {
            let (years0, months0, weeks0, days0) =
                plain_date::calendar_difference_date(calendar, anchor, date_only, date_unit_largest);
            let remainder_ns = i128::from(weeks0 * 7 + days0) * DAY_NS + time_total;
            let step_days: i128 = if smallest == rounding::TemporalUnit::Week { 7 } else { 1 };
            let step_ns = DAY_NS * step_days * increment;
            let rounded_ns = rounding::round_to_increment(remainder_ns, step_ns, mode);
            let rounded_days = (rounded_ns / DAY_NS) as i64;
            // `calendar_difference_date` only ever splits out a years/months
            // component when its own `largest_unit` is `Year`/`Month`; for a
            // `Day`/`Week` `largestUnit` there is no such split (the whole
            // duration collapses to a flat day count from `anchor`), so the
            // pre-offset must match that or the final re-split below would
            // double-count a years/months contribution. Crucially, the
            // pre-offset uses `years0`/`months0` — the *calendar-bracketed*
            // decomposition of the whole `date_only` landing point computed
            // just above — rather than `record.years`/`record.months`
            // directly: the record's own `weeks`/`days` (and any leftover
            // time) can themselves push the whole-months/-years count past
            // what the record's own `years`/`months` fields alone would
            // suggest (e.g. 6 months + 7 weeks + 8 days lands on a real
            // 7th month), and it is *that* landing which must anchor the
            // rounding step, not the record's raw field split.
            let years_months_point = if matches!(
                date_unit_largest,
                plain_date::DateUnit::Year | plain_date::DateUnit::Month
            ) {
                plain_date::calendar_add_date(calendar, anchor, years0, months0, 0, 0, false)
                    .ok_or_else(|| {
                        RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                    })?
            } else {
                anchor
            };
            let day_target =
                plain_date::calendar_add_date(calendar, years_months_point, 0, 0, 0, rounded_days, false)
                    .ok_or_else(|| {
                        RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                    })?;
            let (years, months, weeks, days) =
                plain_date::calendar_difference_date(calendar, anchor, day_target, date_unit_largest);
            // See this function's own doc comment: `weeks` must carry the
            // rounded value whenever `smallest` is `week`, even if
            // `largestUnit` folded it into `days` above.
            let (weeks, days) = if smallest == rounding::TemporalUnit::Week
                && largest != rounding::TemporalUnit::Week
            {
                (days / 7, 0)
            } else {
                (weeks, days)
            };
            return self.temporal_duration_create([
                years as i128,
                months as i128,
                weeks as i128,
                days as i128,
                0,
                0,
                0,
                0,
                0,
                0,
            ]);
        }

        // `smallest` is `month` or `year`: the anchor-relative fractional
        // bracketing every Temporal implementation uses (mirrors
        // `plain_date::round_month_or_year`'s own numerator/denominator
        // exact-integer shape), generalized to weigh the exact leftover
        // nanoseconds rather than only a whole-day position.
        let sign = match plain_date::compare_iso_date(anchor, date_only) {
            std::cmp::Ordering::Less => 1_i64,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal if time_total == 0 => {
                return self.temporal_duration_create([0; 10]);
            }
            std::cmp::Ordering::Equal => {
                if time_total < 0 {
                    -1
                } else {
                    1
                }
            }
        };
        // Decompose at `smallest`'s own granularity (not `largestUnit`'s) to
        // get the true combined count: `calendar_difference_date(...,
        // largest_unit: Year)` only ever returns a *remainder* months field
        // (0..11), never years-and-months combined, whereas `smallest ==
        // "months"` needs the single combined total (mirrors
        // `round_calendar_duration`'s own Month branch computing
        // `total_months` this same way when `largestUnit` is `"years"`).
        let count_unit = Self::temporal_unit_to_date_unit(smallest);
        let (count_years, count_months, _, _) =
            plain_date::calendar_difference_date(calendar, anchor, date_only, count_unit);
        // `calendar_difference_date`'s own `Year`/`Month` branches put the
        // combined count in different tuple slots (`years` when its own
        // `largest_unit` is `Year`, `months` — already years*12+months
        // combined — when it is `Month`).
        let count = if smallest == rounding::TemporalUnit::Year {
            count_years
        } else {
            count_months
        };
        let add_n = |n: i64| -> epoch::CivilDate {
            let (y, m) = if smallest == rounding::TemporalUnit::Year { (n, 0) } else { (0, n) };
            plain_date::calendar_add_date(calendar, anchor, y, m, 0, 0, false)
                .expect("constrain-mode single-unit addition always succeeds")
        };
        let lower = add_n(count);
        let upper = add_n(count + sign);
        let total_span_ns =
            i128::from((plain_date::iso_date_to_epoch_days(upper)
                - plain_date::iso_date_to_epoch_days(lower))
            .unsigned_abs())
                * DAY_NS;
        let progressed_ns = i128::from((plain_date::iso_date_to_epoch_days(date_only)
            - plain_date::iso_date_to_epoch_days(lower))
        .unsigned_abs())
            * DAY_NS
            + time_total.unsigned_abs() as i128;

        let magnitude = i128::from(count.unsigned_abs());
        let increment_i128 = increment.max(1);
        let lower_multiple = (magnitude / increment_i128) * increment_i128;
        let upper_multiple = lower_multiple + increment_i128;
        let extra = magnitude - lower_multiple;
        let numerator = extra * total_span_ns + progressed_ns;
        let denominator = increment_i128 * total_span_ns;
        let round_up = if denominator == 0 || numerator == 0 {
            false
        } else {
            use blueice_ecma402::NumberRoundingMode as Mode;
            match mode {
                Mode::Ceil => sign > 0,
                Mode::Floor => sign < 0,
                Mode::Expand => true,
                Mode::Trunc => false,
                Mode::HalfCeil => {
                    if sign > 0 {
                        2 * numerator >= denominator
                    } else {
                        2 * numerator > denominator
                    }
                }
                Mode::HalfFloor => {
                    if sign < 0 {
                        2 * numerator >= denominator
                    } else {
                        2 * numerator > denominator
                    }
                }
                Mode::HalfExpand => 2 * numerator >= denominator,
                Mode::HalfTrunc => 2 * numerator > denominator,
                Mode::HalfEven => {
                    if 2 * numerator == denominator {
                        (lower_multiple / increment_i128) % 2 != 0
                    } else {
                        2 * numerator > denominator
                    }
                }
            }
        };
        let final_magnitude = if round_up { upper_multiple } else { lower_multiple };
        let rounded_count = sign * (final_magnitude as i64);
        let (years, months) = if smallest == rounding::TemporalUnit::Year {
            (rounded_count, 0)
        } else if largest == rounding::TemporalUnit::Year {
            (rounded_count / 12, rounded_count % 12)
        } else {
            (0, rounded_count)
        };
        self.temporal_duration_create([years as i128, months as i128, 0, 0, 0, 0, 0, 0, 0, 0])
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
            let (unit, anchor) = match &shorthand {
                Some(text) => (Self::temporal_duration_unit_name(text, "unit")?, None),
                None => {
                    let relative_to = self.get_property(&options, &"relativeTo".into())?;
                    let anchor = self.temporal_duration_relative_to(&relative_to)?;
                    let unit = self
                        .temporal_duration_unit_option(&options, "unit", false)?
                        .unit()
                        .ok_or_else(|| {
                            RuntimeError::RangeError(
                                "Temporal.Duration.prototype.total requires unit".into(),
                            )
                        })?;
                    (unit, anchor)
                }
            };
            // See `round` above for why a `Zoned` anchor is dispatched before
            // even the blank-duration shortcut: resolving its real day
            // length/bracket can itself throw.
            if let Some(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns,
                local_date,
                local_time,
            }) = &anchor
            {
                let dest = Self::temporal_duration_zoned_target(
                    zone, *calendar, epoch_ns, *local_date, *local_time, &record,
                )?;
                return Ok(Value::Number(Self::temporal_duration_total_zoned(
                    zone, *calendar, *local_date, *local_time, epoch_ns, &dest, &record, unit,
                )?));
            }
            // A blank duration totals zero in every unit; see `round` above.
            if anchor.is_some() && record.sign() == 0 {
                return Ok(Value::Number(0.0));
            }
            if let Some(anchor) = &anchor {
                Self::temporal_duration_anchor_datetime_in_range(anchor.date())?;
            }
            let needs_calendar =
                Self::temporal_duration_largest_unit(&record).is_calendar() || unit.is_calendar();
            if needs_calendar {
                let anchor = anchor.ok_or_else(|| {
                    RuntimeError::RangeError(
                        "a Temporal.Duration with years, months or weeks needs a relativeTo \
                         anchor"
                            .into(),
                    )
                })?;
                return Ok(Value::Number(self.temporal_duration_total_relative(
                    anchor.calendar(),
                    anchor.date(),
                    &record,
                    unit,
                )?));
            }
            Ok(Value::Number(
                duration_math::TimeDuration::from_record_with_24_hour_days(&record).total_in(unit),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    /// The calendar-aware half of `total`: the exact (fractional) count of
    /// `unit`s from `anchor` to `anchor + record`. For `day`/`week` this is
    /// exact days converted directly (with the `years`/`months`/`weeks` part
    /// resolved through the calendar first); for `month`/`year` it is the
    /// anchor-relative bracketing position `plain_date::round_month_or_year`
    /// also uses, computed here as a continuous fraction instead of rounded
    /// to an increment (that function is `round_calendar_duration`'s own
    /// private helper, so this mirrors its shape locally with the
    /// `pub(crate)` primitives `calendar_add_date`/`calendar_difference_date`
    /// rather than reaching into `plain_date.rs`, which Phase 26's own Track
    /// B scope leaves to its other in-flight owners).
    fn temporal_duration_total_relative(
        &mut self,
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
        unit: rounding::TemporalUnit,
    ) -> Result<f64, RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let (intermediate, ns_of_day) =
            Self::temporal_duration_intermediate(calendar, anchor, record)?;
        let day_fraction_abs = ns_of_day.unsigned_abs() as f64 / DAY_NS as f64;
        let sign = match plain_date::compare_iso_date(anchor, intermediate) {
            std::cmp::Ordering::Less => 1_i64,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal if ns_of_day == 0 => return Ok(0.0),
            // A same-day, sub-day-only remainder: its own sign (not the
            // date's, which didn't move) is the direction of travel.
            std::cmp::Ordering::Equal => {
                if ns_of_day < 0 {
                    -1
                } else {
                    1
                }
            }
        };
        match unit {
            // Both `day` and `week` are calendar-invariant fixed lengths
            // (7 days is 7 days regardless of calendar), so the total is
            // just the exact whole-day span from `anchor` plus the exact
            // sub-day remainder — computed as one integer ratio (never an
            // intermediate float) so it matches the spec's single
            // correctly-rounded final division exactly, bit for bit
            // (`relativeto-total-of-each-unit.js` is what catches a
            // two-step float version drifting by one ULP).
            rounding::TemporalUnit::Day | rounding::TemporalUnit::Week => {
                let total_ns = i128::from(
                    plain_date::iso_date_to_epoch_days(intermediate)
                        - plain_date::iso_date_to_epoch_days(anchor),
                ) * DAY_NS
                    + ns_of_day;
                let denominator = if unit == rounding::TemporalUnit::Week {
                    7 * DAY_NS
                } else {
                    DAY_NS
                };
                Ok(rounding::exact_ratio_to_f64(total_ns, denominator))
            }
            rounding::TemporalUnit::Month | rounding::TemporalUnit::Year => {
                let date_unit = Self::temporal_unit_to_date_unit(unit);
                let (years, months, _, _) =
                    plain_date::calendar_difference_date(calendar, anchor, intermediate, date_unit);
                let count = if unit == rounding::TemporalUnit::Year {
                    years
                } else {
                    months
                };
                let add_n = |n: i64| -> epoch::CivilDate {
                    let (y, m) = if unit == rounding::TemporalUnit::Year {
                        (n, 0)
                    } else {
                        (0, n)
                    };
                    plain_date::calendar_add_date(calendar, anchor, y, m, 0, 0, false)
                        .expect("constrain-mode single-unit addition always succeeds")
                };
                let lower = add_n(count);
                let upper = add_n(count + sign);
                // `add_n`'s own `calendar_add_date` call only range-checks
                // calendar-day validity (an i32-year check), not Temporal's
                // narrower representable range: bracketing one unit *past*
                // an anchor already at the exact max/min boundary lands on a
                // numerically valid but unrepresentable date without
                // otherwise erroring —
                // `throws-if-date-time-invalid-with-plaindate-relative.js`.
                if !epoch::is_date_within_limits(lower) || !epoch::is_date_within_limits(upper) {
                    return Err(RuntimeError::RangeError(
                        "Temporal date arithmetic is out of range".into(),
                    ));
                }
                let total_span = (plain_date::iso_date_to_epoch_days(upper)
                    - plain_date::iso_date_to_epoch_days(lower))
                .unsigned_abs() as f64;
                let progressed = (plain_date::iso_date_to_epoch_days(intermediate)
                    - plain_date::iso_date_to_epoch_days(lower))
                .unsigned_abs() as f64
                    + day_fraction_abs;
                let fraction = if total_span == 0.0 {
                    0.0
                } else {
                    progressed / total_span
                };
                Ok(count as f64 + (sign as f64) * fraction)
            }
            // A time-granularity `unit` still reaches this function whenever
            // the *record itself* has a nonzero year/month/week field (the
            // caller's `needs_calendar` gate is keyed on the record, not
            // `unit` alone) — e.g. `duration.total({ unit: "hours",
            // relativeTo })` on a multi-year `Duration`. The exact total
            // relative to `anchor` is just the whole-day span plus the exact
            // sub-day remainder, in `unit`s.
            _ => {
                let total_ns = i128::from(
                    plain_date::iso_date_to_epoch_days(intermediate)
                        - plain_date::iso_date_to_epoch_days(anchor),
                ) * DAY_NS
                    + ns_of_day;
                Ok(rounding::exact_ratio_to_f64(
                    total_ns,
                    unit.nanoseconds()
                        .expect("every time unit has an exact length"),
                ))
            }
        }
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
            let anchor = self.temporal_duration_relative_to(&relative_to)?;
            // Field-identical durations compare equal before any unit is
            // considered, so even a calendar-unit duration compares to itself.
            if one == two {
                return Ok(Value::Number(0.0));
            }
            // A `Zoned` anchor: `AddZonedDateTime` each operand's *full*
            // duration relative to the same anchor (regardless of whether
            // either operand has a calendar unit — a real day's length can
            // differ even for two purely time-based durations, e.g.
            // `twenty-five-hour-day.js`), then compare the two resulting
            // exact instants directly.
            if let Some(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns,
                local_date,
                local_time,
            }) = &anchor
            {
                let one_target = Self::temporal_duration_zoned_target(
                    zone, *calendar, epoch_ns, *local_date, *local_time, &one,
                )?;
                let two_target = Self::temporal_duration_zoned_target(
                    zone, *calendar, epoch_ns, *local_date, *local_time, &two,
                )?;
                return Ok(Value::Number(match one_target.cmp(&two_target) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                }));
            }
            if let Some(anchor) = &anchor {
                Self::temporal_duration_anchor_datetime_in_range(anchor.date())?;
            }
            let needs_calendar = Self::temporal_duration_largest_unit(&one).is_calendar()
                || Self::temporal_duration_largest_unit(&two).is_calendar();
            if needs_calendar {
                let anchor = anchor.ok_or_else(|| {
                    RuntimeError::RangeError(
                        "a Temporal.Duration with years, months or weeks needs a relativeTo \
                         anchor"
                            .into(),
                    )
                })?;
                let calendar = anchor.calendar();
                let anchor_date = anchor.date();
                // Both operands land relative to the *same* anchor, so their
                // exact `(whole days, sub-day nanoseconds)` pairs compare
                // lexicographically exactly as their true nanosecond totals
                // would (each remainder's magnitude stays under one day).
                let (one_date, one_ns) =
                    Self::temporal_duration_intermediate(calendar, anchor_date, &one)?;
                let (two_date, two_ns) =
                    Self::temporal_duration_intermediate(calendar, anchor_date, &two)?;
                let one_days = plain_date::iso_date_to_epoch_days(one_date)
                    - plain_date::iso_date_to_epoch_days(anchor_date);
                let two_days = plain_date::iso_date_to_epoch_days(two_date)
                    - plain_date::iso_date_to_epoch_days(anchor_date);
                return Ok(Value::Number(
                    match (one_days, one_ns).cmp(&(two_days, two_ns)) {
                        std::cmp::Ordering::Less => -1.0,
                        std::cmp::Ordering::Equal => 0.0,
                        std::cmp::Ordering::Greater => 1.0,
                    },
                ));
            }
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

        // Unbounded at the field-reading stage (`ToIntegerWithTruncation`),
        // matching `temporal_year_month_with`'s own identical fix's doc
        // comment -- `iso::is_year_month_within_limits` below still
        // range-checks the resolved date.
        let requested_year = (!matches!(year, Value::Undefined))
            .then(|| self.temporal_integer(&year, i32::MIN, i32::MAX, "year"))
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
            Some(self.temporal_integer(&era_year, i32::MIN, i32::MAX, "era year")?)
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

        // `monthCode`'s own *syntax* (calendar-agnostic: `M` + two digits +
        // an optional `L`) is validated as soon as it is coerced to a
        // string, ahead of `year`'s own numeric coercion below -- pinned by
        // `from/monthcode-invalid.js`'s two Symbol-`year` cases: a
        // syntactically malformed code (`"L99M"`) must throw `RangeError`
        // *before* `year: Symbol()` is ever converted (`TypeError`), while a
        // well-formed-but-calendar-unsuitable one (`"M99L"`, checked later,
        // once an actual calendar resolution is attempted) must not --
        // `year`'s own `Symbol` conversion throws `TypeError` first in that
        // case, since the malformed-syntax short-circuit above never fires
        // for it.
        let month_code_s = (!matches!(month_code, Value::Undefined))
            .then(|| self.coerce_string(&month_code))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))
            })
            .transpose()?;
        if let Some(code) = month_code_s.as_deref() {
            if !plain_month_day::is_well_formed_month_code(code) {
                return Err(RuntimeError::RangeError("invalid Temporal month code".into()));
            }
        }
        // Unbounded at the field-reading stage (`ToIntegerWithTruncation`),
        // matching `temporal_month_day_with`'s own identical `year` fix --
        // see that call site's doc comment. `epoch::is_date_within_limits`
        // below still range-checks the resolved date.
        let requested_year = (!matches!(year, Value::Undefined))
            .then(|| self.temporal_integer(&year, i32::MIN, i32::MAX, "year"))
            .transpose()?;
        // `ToPositiveIntegerWithTruncation`: only a lower bound of `1`, no
        // upper bound, matching this function's own `day`/`year` fields
        // above -- the calendar's own `overflow` regulation constrains or
        // rejects an out-of-range `month`, not this read.
        // `from/overflow.js`'s `{ month: 999999 }` under `overflow:
        // "constrain"` must succeed as `M12`, not throw here.
        let requested_month = (!matches!(month, Value::Undefined))
            .then(|| self.temporal_integer(&month, 1, i32::MAX, "month"))
            .transpose()?;
        if month_code_s.is_none() && requested_month.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require month or monthCode".into(),
            ));
        }
        // `CalendarExtraFields`: requesting `Year` (which `ToTemporalMonthDay`'s
        // field list always does) also reads `era`/`eraYear` for a calendar
        // that supports eras -- `PlainMonthDay`'s own field list has no
        // `era`/`eraYear` of its own, but this is a side effect of it always
        // requesting `year`, not a separate feature. Read only when the
        // calendar actually supports era (`iso8601`/`chinese`/`dangi` never
        // do), matching `PrepareCalendarFields`'s own conditional field-name
        // expansion -- `intl402/Temporal/PlainMonthDay/prototype/{equals,
        // toPlainDate}/infinity-throws-rangeerror.js`'s `{ era: "ad",
        // eraYear: Infinity }` on a `"gregory"`-calendar receiver needs
        // `eraYear`'s own `Infinity` to reach `temporal_integer`'s existing
        // finiteness check, which never happened when this field went
        // entirely unread.
        let supports_era = calendar::calendar_supports_era(&calendar);
        let (era_s, requested_era_year) = if supports_era {
            let era_v = self.get_property(bag, &"era".into())?;
            let era_year_v = self.get_property(bag, &"eraYear".into())?;
            let era_s = (!matches!(era_v, Value::Undefined))
                .then(|| self.coerce_string(&era_v))
                .transpose()?
                .map(|value| {
                    value
                        .to_utf8()
                        .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
                })
                .transpose()?;
            let requested_era_year = (!matches!(era_year_v, Value::Undefined))
                .then(|| self.temporal_integer(&era_year_v, i32::MIN, i32::MAX, "era year"))
                .transpose()?;
            if era_s.is_some() != requested_era_year.is_some() {
                return Err(RuntimeError::TypeError(
                    "Temporal era and eraYear must be supplied together".into(),
                ));
            }
            (era_s, requested_era_year)
        } else {
            (None, None)
        };
        // Per `MissingFieldsStrategy::Ecma`'s own documented rule, a
        // reference year is only derivable from `monthCode` + `day` -- an
        // ordinal `month`'s identity itself varies by year, so it cannot
        // resolve one. `ToTemporalMonthDay`'s own field set has no `era`,
        // so `year`/`monthCode` are the only two ways to avoid this.
        //
        // This only applies to a **non-ISO** calendar, though: Gecko's own
        // `CalendarResolveFields` gives the `iso8601` calendar a separate,
        // narrower branch that requires only `day` and (`monthCode` or
        // `month`) -- no `year` at all, since the ISO calendar's reference
        // year (1972) is fixed and never genuinely ambiguous the way a
        // lunisolar calendar's leap-month numbering is. Verified against
        // Test262's `PlainMonthDay/prototype/equals/basic.js`, whose
        // `md1.equals({ month: 1, day: 22 })` call (a bare ordinal
        // `month`+`day`, no `monthCode`/`year`) must succeed for the ISO
        // calendar.
        if calendar != "iso8601"
            && requested_year.is_none()
            && month_code_s.is_none()
            && era_s.is_none()
        {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require monthCode or year".into(),
            ));
        }
        if matches!(day, Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require day".into(),
            ));
        }
        // `ToPositiveIntegerWithTruncation`: only a lower bound of `1`, no
        // upper bound (see `temporal_month_day_with`'s identical fix's doc
        // comment) -- the calendar's own `overflow` regulation constrains
        // or rejects an out-of-month-range `day`, not this read.
        let day_num = self.temporal_integer(&day, 1, i32::MAX, "day")?;
        let day_num_u8 = day_num.min(i32::from(u8::MAX)) as u8;

        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar_identifier validates the calendar identifier");
        // `icu_calendar`'s reference-year derivation
        // (`MissingFieldsStrategy::Ecma`) only fires from a `monthCode`+
        // `day` pair, never a bare ordinal `month`+`day` -- correct in
        // general (an ordinal month's identity varies by year for a
        // leap-month calendar), but the ISO calendar's ordinal month and
        // `monthCode` always correspond 1:1 (`M01`..`M12`, no ambiguity),
        // so synthesize the equivalent `monthCode` here rather than
        // requiring the caller to supply a redundant `year`.
        let iso_month_code = (calendar == "iso8601" && month_code_s.is_none())
            .then(|| requested_month.map(|value| format!("M{value:02}")))
            .flatten();
        let month_code_for_fields = month_code_s.as_deref().or(iso_month_code.as_deref());
        // Once a `monthCode` is in hand (given directly, or synthesized for
        // `iso8601` above), drop the redundant `ordinal_month` --
        // `icu_calendar::Date::try_from_fields` treats a simultaneous
        // `month_code` + `ordinal_month` as conflicting fields even when
        // they agree, rather than as redundant-but-consistent input.
        // Clamp to `u8::MAX` before the cast rather than a bare `as u8`,
        // which truncates via silent wraparound (`999999 as u8` is `63`) --
        // any month past `u8::MAX` is unambiguously out of the calendar's
        // real month range regardless of exactly how large it was, so
        // saturating here preserves that for the downstream
        // constrain/reject regulation instead of risking a coincidentally
        // small, spuriously in-range wrapped value.
        let ordinal_month_for_fields = month_code_for_fields
            .is_none()
            .then(|| requested_month.map(|value| value.min(i32::from(u8::MAX)) as u8))
            .flatten();
        // `CalendarISOToDate`'s ISO-specific branch (`Calendar.cpp`): a
        // supplied `year` regulates the resolved `day` (e.g. whether 29
        // February constrains/rejects) but never survives into the result
        // -- the `iso8601` calendar's `PlainMonthDay` always reports the
        // fixed reference year 1972. Handled by a dedicated pure-Rust fast
        // path (`iso_month_day_from_fields`) rather than `icu_calendar`,
        // whose own internal year-range limits are far narrower than the
        // regulation year's legitimate domain here (an arbitrarily large or
        // small `year` is valid input purely for leap-year determination).
        // Pinned by `PlainMonthDay/from/iso-year-used-only-for-overflow.js`.
        let date = if calendar == "iso8601" {
            // A well-formed `monthCode` (already syntax-checked above) must
            // still denote an actual ISO 8601 month (`01`-`12`, never a
            // leap-month `L` suffix -- the ISO calendar has no leap months
            // at all) *regardless of `overflow`*: this is a suitability
            // check on the code's own meaning, not a numeric-field
            // constrain/reject regulation, so `{ monthCode: "M19" }` is a
            // `RangeError` even under the default `overflow: "constrain"`
            // -- `from/monthcode-invalid.js`'s `M00`/`M19`/`M99`/`M13`/
            // `M00L`/`M05L`/`M13L` cases.
            let month_code_ordinal = month_code_s
                .as_deref()
                .map(|code| {
                    plain_month_day::iso_month_code_ordinal(code).ok_or_else(|| {
                        RuntimeError::RangeError(
                            "monthCode is not valid for the ISO 8601 calendar".into(),
                        )
                    })
                })
                .transpose()?;
            // A `month`/`monthCode` pair that disagree is always a
            // `RangeError`, independent of `overflow` -- `from/
            // monthcode-invalid.js`'s `{ month: 12, monthCode: "M11" }`
            // ("monthCode and month conflict").
            if let (Some(month_num), Some(code_num)) = (requested_month, month_code_ordinal) {
                if month_num != i32::from(code_num) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay month and monthCode conflict".into(),
                    ));
                }
            }
            // Same saturating-cast rationale as `ordinal_month_for_fields`
            // above.
            let month = requested_month
                .map(|value| value.min(i32::from(u8::MAX)) as u8)
                .or(month_code_ordinal)
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal month code".into()))?;
            plain_month_day::iso_month_day_from_fields(
                month,
                day_num_u8,
                requested_year.unwrap_or(1972),
                reject,
            )
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()))?
        } else {
            // As in `temporal_plain_date_from_fields`: `era`/`eraYear`
            // (when supplied together) resolve the year entirely on their
            // own via `icu_calendar`'s own era-aware `Date::try_from_fields`
            // -- mutually exclusive with `extended_year` here, matching that
            // function's own established precedent, rather than merged with
            // a separately-supplied `year`.
            let fields = plain_month_day::MonthDayFields {
                extended_year: era_s.is_none().then_some(requested_year).flatten(),
                era: era_s.as_deref().map(str::as_bytes),
                era_year: requested_era_year,
                month_code: month_code_for_fields,
                ordinal_month: ordinal_month_for_fields,
                day: day_num_u8,
            };
            plain_month_day::month_day_from_fields(calendar_kind, &fields, reject).map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar month-day".into())
            })?
        };
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
        // `ToTemporalMonthDay`'s real algorithm parses the string (throwing
        // `RangeError` for a malformed one) strictly before it ever reads
        // the `overflow` option -- pinned by `from/options-wrong-type.js`'s
        // "Invalid string string processed before throwing TypeError" case,
        // an invalid string must report `RangeError` even when `options`
        // itself is a wrong-type value that would otherwise throw
        // `TypeError`.
        let parsed = self.temporal_value_from_string(TemporalKind::PlainMonthDay, &source)?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
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
            ..Default::default()
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
        let era_v = self.get_property(like, &"era".into())?;
        let era_year_v = self.get_property(like, &"eraYear".into())?;
        if matches!(year_v, Value::Undefined)
            && matches!(month_v, Value::Undefined)
            && matches!(month_code_v, Value::Undefined)
            && matches!(era_v, Value::Undefined)
            && matches!(era_year_v, Value::Undefined)
        {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        // `ToIntegerWithTruncation`: unbounded at the field-reading stage
        // for both `year` and `eraYear` (`CalendarFields.cpp`'s
        // `CalendarField::Year`/`EraYear` cases) -- the real representable-
        // range check happens once, below, against the *resolved* date via
        // `iso::is_year_month_within_limits`, not here.
        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| self.temporal_integer(&year_v, i32::MIN, i32::MAX, "year"))
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
        let era_s = (!matches!(era_v, Value::Undefined))
            .then(|| self.coerce_string(&era_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
            })
            .transpose()?;
        let requested_era_year = (!matches!(era_year_v, Value::Undefined))
            .then(|| self.temporal_integer(&era_year_v, i32::MIN, i32::MAX, "eraYear"))
            .transpose()?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let month_code = month_code_s
            .clone()
            .or_else(|| requested_month.is_none().then(|| base.month_code.clone()));

        // `CalendarFields.cpp`'s `NonISOResolveFields`: on a calendar that
        // supports eras, `era` and `eraYear` must be supplied together or
        // not at all -- see
        // `development/browser_core/phase-26-ecma262-temporal/PLAN.md`'s
        // Stage 2 `plain_year_month.rs` entry and this test file's own
        // doc comment for the fixture this pins
        // (`mutually-exclusive-fields-gregory.js`).
        let supports_era = calendar::calendar_supports_era(&existing.calendar);
        if supports_era && era_s.is_some() != requested_era_year.is_some() {
            return Err(RuntimeError::TypeError(
                if era_s.is_some() {
                    "Temporal.with requires eraYear when era is provided".into()
                } else {
                    "Temporal.with requires era when eraYear is provided".into()
                },
            ));
        }
        // `chinese`/`dangi` are unlike `iso8601` here even though both fail
        // `calendar_supports_era`: `iso8601` silently ignores an `era`/
        // `eraYear` property (no Test262 fixture requires otherwise, and
        // `PlainDate`'s own `with/time-units-ignored.js` establishes this is
        // the correct cross-type behavior for `iso8601` specifically), but
        // ICU4X has no era concept for `chinese`/`dangi` at all and
        // Temporal's own behavior for them is to *reject* any use of
        // `era`/`eraYear`, matching
        // `mutually-exclusive-fields-{chinese,dangi}.js`'s
        // `assert.throws(TypeError, () => instance.with({ eraYear, era }))`.
        if !supports_era
            && existing.calendar != "iso8601"
            && (era_s.is_some() || requested_era_year.is_some())
        {
            return Err(RuntimeError::TypeError(
                "era and eraYear are not valid for this calendar".into(),
            ));
        }
        // `NonISOFieldKeysToIgnore`: `era`/`eraYear`/`year` are mutually
        // exclusive as a group on an era-supporting calendar -- providing
        // any one of them drops the receiver's own value for all three,
        // rather than only the field actually given.
        let (era_field, era_year_field, extended_year_field) =
            if supports_era && era_s.is_some() && requested_era_year.is_some() {
                (era_s.as_deref(), requested_era_year, None)
            } else {
                (None, None, Some(requested_year.unwrap_or(base.year)))
            };
        let fields = plain_year_month::YearMonthFields {
            era: era_field,
            era_year: era_year_field,
            extended_year: extended_year_field,
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
        // `ToIntegerWithTruncation` (`CalendarFields.cpp`'s
        // `CalendarField::Year` case): unbounded at the field-reading
        // stage, matching `built-ins/Temporal/PlainMonthDay/prototype/with/
        // iso-year-used-only-for-overflow.js` -- for `PlainMonthDay` a huge
        // out-of-range `year` is legitimate input used only to determine
        // leap-year-ness for `overflow` regulation (e.g. is 29 February
        // valid in *this* `year`), never range-checked itself, and never
        // part of the type's own identity (`format_month_day` ignores the
        // stored year entirely for the `iso8601` short form).
        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| self.temporal_integer(&year_v, i32::MIN, i32::MAX, "year"))
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
        // `ToPositiveIntegerWithTruncation` (`CalendarFields.cpp`'s
        // `CalendarField::Day` case): only a lower bound of `1`, no upper
        // bound at the field-reading stage -- `{ day: 100 }` must reach the
        // calendar's own `overflow: "constrain"`/`"reject"` regulation
        // below, not be rejected outright here. The `u8` field this feeds
        // is saturated rather than truncated so a huge value still clamps
        // sensibly (`month_day_from_fields`'s own `Overflow::Constrain`
        // brings it down to the real month length; `Overflow::Reject` still
        // throws, just from calendar regulation instead of this read).
        let requested_day = (!matches!(day_v, Value::Undefined))
            .then(|| self.temporal_integer(&day_v, 1, i32::MAX, "day"))
            .transpose()?;

        // `PrepareCalendarFields` (the field reads/coercions above, which
        // can themselves throw `RangeError` -- e.g. `{ day: -1 }`) runs
        // strictly before `GetOptionsObject`/`GetTemporalOverflowOption`,
        // not after -- pinned by `with/options-wrong-type.js`'s "Partial
        // date processed before throwing TypeError" case: an invalid field
        // must report `RangeError` even when `options` is itself a
        // wrong-type value that would otherwise throw `TypeError`.
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let month_code = month_code_s
            .clone()
            .or_else(|| requested_month.is_none().then(|| base.month_code.clone()));
        // `ISODateToFields(calendar, isoDate, MONTH-DAY)`: the receiver's
        // own base field set for `with()`'s merge is only `monthCode`/`day`
        // -- unlike `PlainDate`/`PlainYearMonth`, `PlainMonthDay` has no
        // `year`/`month` getters at all, so there is no receiver `year` to
        // fall back on for a non-ISO calendar. Falling back to `base.year`
        // regardless (as this function did before this fix) is only
        // correct for `iso8601`, whose fixed 1972 reference year is never
        // genuinely ambiguous; for any other calendar, supplying a bare
        // ordinal `month` with no explicit `year` must throw per
        // `NonISOResolveFields`'s `requireYear` rule -- pinned by
        // `intl402/Temporal/PlainMonthDay/prototype/with/
        // fields-missing-properties.js`.
        let extended_year_for_fields = if existing.calendar == "iso8601" {
            Some(requested_year.unwrap_or(base.year))
        } else {
            requested_year
        };
        if existing.calendar != "iso8601"
            && requested_month.is_some()
            && extended_year_for_fields.is_none()
        {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires year when only an ordinal month is given for this calendar".into(),
            ));
        }
        // As in `temporal_plain_month_day_from_fields`: once a `monthCode`
        // is in hand, drop the redundant `ordinal_month` --
        // `icu_calendar::Date::try_from_fields` treats a simultaneous
        // `month_code` + `ordinal_month` as conflicting fields even when
        // they agree.
        let ordinal_month_for_fields = month_code
            .is_none()
            .then(|| requested_month.map(|value| value as u8))
            .flatten();
        let day_for_fields = requested_day
            .map(|value| value.min(i32::from(u8::MAX)) as u8)
            .unwrap_or(base.day);
        // As in `temporal_plain_month_day_from_fields`: the `iso8601`
        // calendar's own regulation must bypass `icu_calendar` entirely --
        // its internal year-range limits are far narrower than the
        // regulation year's legitimate domain (`year` here is only ever
        // used to decide leap-year-ness, never part of the result).
        // Pinned by `PlainMonthDay/prototype/with/
        // iso-year-used-only-for-overflow.js`.
        let date = if existing.calendar == "iso8601" {
            // A `month`/`monthCode` pair that disagree is always a
            // `RangeError` -- `with/basic.js`'s `{ month: 12, monthCode:
            // "M11" }` ("with({month, monthCode}) disagree"). Only
            // `month_code_s` (the value actually supplied to `with()`, not
            // `month_code`, which also carries the receiver's own
            // unmodified base value) participates in this check: a
            // `monthCode` the caller didn't touch must never conflict with
            // a newly supplied `month`.
            let month_code_ordinal = month_code_s
                .as_deref()
                .and_then(|code| code.strip_prefix('M')?.parse::<u8>().ok());
            if let (Some(month_num), Some(code_num)) = (requested_month, month_code_ordinal) {
                if month_num != i32::from(code_num) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay month and monthCode conflict".into(),
                    ));
                }
            }
            let month = requested_month
                .map(|value| value as u8)
                .or_else(|| {
                    month_code
                        .as_deref()
                        .and_then(|code| code.strip_prefix('M')?.parse().ok())
                })
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal month code".into()))?;
            plain_month_day::iso_month_day_from_fields(
                month,
                day_for_fields,
                extended_year_for_fields.unwrap_or(1972),
                reject,
            )
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()))?
        } else {
            let fields = plain_month_day::MonthDayFields {
                extended_year: extended_year_for_fields,
                month_code: month_code.as_deref(),
                ordinal_month: ordinal_month_for_fields,
                day: day_for_fields,
                ..Default::default()
            };
            plain_month_day::month_day_from_fields(calendar_kind, &fields, reject).map_err(|_| {
                RuntimeError::RangeError("invalid Temporal calendar month-day".into())
            })?
        };
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
        // Always `from = existing, to = other` and negate the *result* for
        // `since`, never swap which date is `from`/`to` — matching
        // `temporal_date_difference`'s own documented rule (see that
        // function's own comment). `calendar_difference_date_leap_month`
        // anchors its whole computation on `from`'s own `Month` identity, so
        // swapping `from`/`to` instead of negating silently computes a
        // different (and for the three leap-month calendars, wrong)
        // quantity: `f(other, existing) != -f(existing, other)` in general.
        // Found via `intl402/Temporal/PlainYearMonth/prototype/since/
        // leap-months-{chinese,dangi,hebrew}.js`, whose "M04L-M04 is 1y not
        // 1y 1mo" case this swap computed as `1y 1mo` instead of `1y`.
        let (from, to) = (&existing, &other);
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

        // `round_calendar_duration`'s own `roundingMode` is direction-
        // sensitive (`Ceil`/`Floor`/`HalfCeil`/`HalfFloor` round toward a
        // fixed end of the *real* number line, not toward a fixed end of
        // whichever internal `from`/`to` direction happened to be computed),
        // so negating the result below without also reflecting an
        // asymmetric mode would silently round the wrong way whenever
        // `since` negates — `ceil(-x) == -floor(x)`, not `-ceil(x)`. Found
        // via `built-ins/Temporal/PlainYearMonth/prototype/since/
        // roundingmode-{ceil,floor}.js`, which this exact reflection fixes.
        // `Trunc`/`Expand`/`HalfExpand`/`HalfTrunc`/`HalfEven` are all
        // symmetric under negation and need no reflection.
        let effective_mode = if since {
            use blueice_ecma402::NumberRoundingMode as Mode;
            match mode {
                Mode::Ceil => Mode::Floor,
                Mode::Floor => Mode::Ceil,
                Mode::HalfCeil => Mode::HalfFloor,
                Mode::HalfFloor => Mode::HalfCeil,
                other => other,
            }
        } else {
            mode
        };
        let (years, months, _, _) = plain_date::round_calendar_duration(
            calendar_kind,
            from_date,
            to_date,
            Self::temporal_unit_to_date_unit(largest_unit),
            Self::temporal_unit_to_date_unit(smallest_unit),
            increment,
            effective_mode,
        );
        let (years, months) = if since { (-years, -months) } else { (years, months) };
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

    /// `CreateDateTimeFormat`'s `required` parameter here is DATE, the same
    /// as `PlainDate`'s own: a `timeStyle` option is rejected unconditionally,
    /// even alongside `dateStyle`
    /// (`intl402/.../PlainYearMonth/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`) -- the mirror of
    /// `temporal_plain_time_to_locale_string`'s own `required = TIME` check.
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
            if self.date_time_format_data(&formatter)?.options().time_style.is_some() {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainYearMonth.prototype.toLocaleString does not accept a timeStyle option"
                        .into(),
                ));
            }
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
    }

    /// `CreateDateTimeFormat`'s `required` parameter here is DATE, the same
    /// as `PlainDate`'s own -- see `temporal_year_month_to_locale_string`'s
    /// own doc comment
    /// (`intl402/.../PlainMonthDay/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`).
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
            if self.date_time_format_data(&formatter)?.options().time_style.is_some() {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainMonthDay.prototype.toLocaleString does not accept a timeStyle option"
                        .into(),
                ));
            }
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
        // Per the actual spec text (`plainmonthday.html`,
        // `sec-temporal.plainmonthday.prototype.toplaindate`), step 6's
        // `PrepareCalendarFields(calendar, item, « year », « », « »)` has an
        // *empty* required-field list -- despite this function's own
        // now-outdated doc comment above, `year` was never literally
        // required here. `era`/`eraYear` (read as a `CalendarExtraFields`
        // side effect of requesting `year`, exactly as in
        // `temporal_plain_month_day_from_fields`) can resolve the year
        // instead, and Test262's own
        // `toPlainDate/infinity-throws-rangeerror.js` calls
        // `instance.toPlainDate({ era: "ad", eraYear: Infinity })` with no
        // `year` property at all, expecting `eraYear`'s own out-of-range
        // value to be what throws (`RangeError`), not a missing-`year`
        // `TypeError`.
        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| {
                // Unbounded at the field-reading stage
                // (`ToIntegerWithTruncation`), matching every other Temporal
                // year field in this file -- the real representable-range
                // check happens afterward, once an actual date is resolved
                // (see the `iso8601` fast path below).
                // `toPlainDate/limits.js`'s own `-271821`/`275760` boundary
                // years are themselves in Temporal's representable range for
                // *some* month/day (just not every one), so they must reach
                // real date resolution rather than being rejected here.
                self.temporal_integer(&year_v, i32::MIN, i32::MAX, "year")
            })
            .transpose()?;
        let base = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let supports_era = calendar::calendar_supports_era(&existing.calendar);
        let (era_s, requested_era_year) = if supports_era {
            let era_v = self.get_property(item, &"era".into())?;
            let era_year_v = self.get_property(item, &"eraYear".into())?;
            let era_s = (!matches!(era_v, Value::Undefined))
                .then(|| self.coerce_string(&era_v))
                .transpose()?
                .map(|value| {
                    value
                        .to_utf8()
                        .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
                })
                .transpose()?;
            let requested_era_year = (!matches!(era_year_v, Value::Undefined))
                .then(|| self.temporal_integer(&era_year_v, i32::MIN, i32::MAX, "era year"))
                .transpose()?;
            if era_s.is_some() != requested_era_year.is_some() {
                return Err(RuntimeError::TypeError(
                    "Temporal era and eraYear must be supplied together".into(),
                ));
            }
            (era_s, requested_era_year)
        } else {
            (None, None)
        };
        if requested_year.is_none() && era_s.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay.prototype.toPlainDate requires a year property".into(),
            ));
        }
        let value = if existing.calendar == "iso8601" {
            // `icu_calendar::Date::try_from_fields`'s own internal
            // `CONSTRUCTOR_YEAR_RANGE` (`-9999..=9999`) is far narrower than
            // Temporal's real range -- the same "Calendar year-range getter
            // bug" class this codebase's Stage 0 audit already fixed for
            // `temporal_calendar_fields`'s getters, reachable here too since
            // `toPlainDate({ year: -271821 })` is a boundary-valid year for
            // April 19th. `plain_date::regulate_iso_date` is pure Rust
            // arithmetic with no such limit; `overflow` is always
            // `"constrain"` here -- `toPlainDate` takes no options argument
            // at all to request `"reject"`.
            let month = base
                .month_code
                .strip_prefix('M')
                .and_then(|digits| digits.parse::<u8>().ok())
                .expect("a resolved PlainMonthDay's own monthCode is always well-formed");
            // `iso8601` never supports era (`calendar_supports_era`), so
            // `era_s` is always `None` here -- the check above guarantees
            // `requested_year` is `Some` whenever this branch is reached.
            let year = requested_year
                .expect("iso8601 has no era substitute, so year must be present here");
            let date = plain_date::regulate_iso_date(year, month, i64::from(base.day), false)
                .expect("regulate_iso_date only fails under overflow: reject");
            if !epoch::is_date_within_limits(date) {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDate is outside the supported range".into(),
                ));
            }
            Self::temporal_date_value(TemporalKind::PlainDate, existing.calendar, date)
        } else {
            // As in `temporal_plain_month_day_from_fields`: `era`/`eraYear`
            // (supplied together) resolve the year entirely on their own via
            // `icu_calendar`'s own era-aware `Date::try_from_fields`,
            // mutually exclusive with a separately-supplied `year`.
            let mut fields = DateFields::default();
            fields.extended_year = era_s.is_none().then_some(requested_year).flatten();
            fields.era = era_s.as_deref().map(str::as_bytes);
            fields.era_year = requested_era_year;
            fields.month_code = Some(base.month_code.as_bytes());
            fields.day = Some(base.day);
            let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
            icu_options.overflow = Some(icu_calendar::options::Overflow::Constrain);
            let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
                .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
            Self::temporal_value_from_calendar_date(TemporalKind::PlainDate, existing.calendar, date)
        };
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
            // `PrepareCalendarFields` reads and validates `calendar` before
            // any other field -- an invalid `calendar` is a `RangeError`
            // even when `timeZone` is missing entirely
            // (`argument-propertybag-calendar-invalid-iso-string.js`,
            // `argument-propertybag-calendar-year-zero.js`). The result is
            // discarded here (`temporal_plain_date_from_fields` below
            // re-resolves it) -- this call exists purely to get the ordering
            // of *when* a bad calendar throws right; a second, harmless
            // re-read of the same property is an already-documented,
            // separate gap shared with every other field-ordering fixture
            // this file doesn't yet pass (`order-of-operations.js`).
            let calendar_value = self.get_property(value, &"calendar".into())?;
            self.temporal_calendar_identifier(&calendar_value)?;
            let time_zone_value = self.get_property(value, &"timeZone".into())?;
            if time_zone_value == Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Temporal.ZonedDateTime property bag requires timeZone".into(),
                ));
            }
            let zone = self.temporal_time_zone(&time_zone_value)?;
            // `offset`'s own *syntax* is read and validated here, ahead of
            // `year`/`month`/`day`/etc. below -- `offset-string-invalid.js`
            // pins this exact ordering both ways: a syntactically invalid
            // offset (`"--00:00"`) is a `RangeError` even when `year` is a
            // `Symbol` that would otherwise throw `TypeError` first, but a
            // syntactically *valid* offset that merely doesn't match the
            // zone (`"+04:30"` against `"UTC"`) only surfaces *after* `year`
            // has already thrown -- because that later *semantic* mismatch
            // check only runs once every field (including `year`) below has
            // been fully resolved.
            let offset_value = self.get_property(value, &"offset".into())?;
            // A property bag's `offset` field goes through `ToPrimitive`
            // with a string hint (never a blanket `ToString`) and then must
            // *already be* a String -- an object's own `toString`/`valueOf`
            // is genuinely called (`order-of-operations.js`'s "get
            // other.offset.toString" / "call other.offset.toString"), but a
            // non-object, non-string primitive (`Number`/`null`/`Boolean`/
            // `BigInt`) is a `TypeError` without ever being stringified,
            // since `ToPrimitive` on an already-primitive value is the
            // identity (`relativeto-propertybag-invalid-offset-string.js`,
            // reached via `Temporal.Duration`'s own `relativeTo` reuse of
            // this function, still rejects a plain `1000`/`null`/`true`/
            // `1000n`). Matches `temporal_to_instant_epoch`'s own
            // `coerce_primitive`-then-check-`String` pattern.
            let offset_primitive = (!matches!(offset_value, Value::Undefined))
                .then(|| self.coerce_primitive(&offset_value, "string"))
                .transpose()?;
            if let Some(primitive) = &offset_primitive {
                if !matches!(primitive, Value::String(_)) {
                    return Err(RuntimeError::TypeError(
                        "Temporal.ZonedDateTime offset must be a string".into(),
                    ));
                }
            }
            let offset_string = offset_primitive
                .map(|primitive| self.coerce_string(&primitive))
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
            // Now resolve the rest of the calendar-date/time-of-day fields
            // (`year`/`month`/`monthCode`/`day`/`era`/`eraYear`/`hour`../
            // `nanosecond`) -- `year`'s own `TypeError` for a non-convertible
            // value (e.g. a `Symbol`) has to come *after* `offset`'s syntax
            // check above, per this function's own doc comment.
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
                false, // a property-bag `offset` field is always `MatchExactly`.
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
        // `ToTemporalZonedDateTime`'s non-object branch requires a literal
        // `String`, never `ToString`-coerced -- a `Number`/`Boolean`/`null`/
        // `BigInt`/`Symbol` argument is a `TypeError`, not an attempt to
        // stringify it first (`argument-wrong-type.js`: `1`/`19761118`/`1n`
        // are all `TypeError`s even though the latter would otherwise parse
        // as a valid-looking string). Matches `temporal_to_plain_date`'s own
        // identical guard.
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime-like value must be an object or a string".into(),
            ));
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
            // `MatchMinutes` unless the leading offset itself was spelled
            // with sub-minute (seconds/fraction) precision -- see
            // `temporal_interpret_offset`'s own doc comment.
            !parsed.offset_sub_minute_precision,
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
            .then(|| self.temporal_integer(&era_year_v, i32::MIN, i32::MAX, "era year"))
            .transpose()?;
        // The same three-way `iso8601`/`!calendar_supports_era`/era-supporting
        // split `temporal_date_with`/`temporal_year_month_with` already use
        // (see those functions' own doc comments for the full rationale):
        // `iso8601` has no eras at all and silently ignores `era`/`eraYear`;
        // `chinese`/`dangi` have no era concept either, but Temporal's own
        // rule is to *reject* any use of them there rather than ignore it;
        // every other calendar requires `era` and `eraYear` together or not
        // at all.
        if existing.calendar == "iso8601" {
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        } else if !calendar::calendar_supports_era(&existing.calendar) {
            if era_s.is_some() || era_year_num.is_some() {
                return Err(RuntimeError::TypeError(
                    "era and eraYear are not valid for this calendar".into(),
                ));
            }
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        } else {
            match (era_s.as_deref(), era_year_num) {
                (Some(era), Some(era_year)) => {
                    fields.era = Some(era.as_bytes());
                    fields.era_year = Some(era_year);
                }
                (Some(_), None) => {
                    return Err(RuntimeError::TypeError(
                        "Temporal.with requires eraYear when era is provided".into(),
                    ));
                }
                (None, Some(_)) => {
                    return Err(RuntimeError::TypeError(
                        "Temporal.with requires era when eraYear is provided".into(),
                    ));
                }
                (None, None) => {
                    fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
                }
            }
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
        // `ToPositiveIntegerWithTruncation` (`CalendarFields.cpp`'s
        // `CalendarField::Day` case) has no upper bound at all -- the same
        // fix already applied to `temporal_date_with`/`temporal_month_day_with`.
        // `date.with({ day: daysInMonth + 1 })` must reach the calendar's own
        // `overflow` regulation (constrain by default, reject on request)
        // rather than throwing immediately at field-parsing time, per
        // Test262's `wrapping-at-end-of-month-*.js`.
        let requested_day = (!matches!(day_v, Value::Undefined))
            .then(|| self.temporal_integer(&day_v, 1, i32::MAX, "day"))
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

        // Same `ToPrimitive`-then-require-`String` shape as
        // `temporal_to_zoned_date_time`'s own `offset` field (see that call
        // site's own doc comment) -- a non-object, non-string primitive
        // (`0`/`null`/`true`/`1000n`) is a `TypeError` without ever being
        // stringified (`offset-property-invalid-string.js`), never a
        // `RangeError` from a coerced-then-rejected string like `"0"`.
        let offset_primitive = (!matches!(offset_v, Value::Undefined))
            .then(|| self.coerce_primitive(&offset_v, "string"))
            .transpose()?;
        if let Some(primitive) = &offset_primitive {
            if !matches!(primitive, Value::String(_)) {
                return Err(RuntimeError::TypeError(
                    "Temporal.ZonedDateTime offset must be a string".into(),
                ));
            }
        }
        let offset_string = offset_primitive
            .map(|primitive| self.coerce_string(&primitive))
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
            false, // `.with()`'s own `offset` field is always `MatchExactly`.
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
    /// The field-level core of [`Self::temporal_zoned_date_time_difference`],
    /// extracted so `Temporal.Duration.prototype.round`/`total`/static
    /// `compare`'s own `ZonedDateTime`-`relativeTo` paths can reuse the exact
    /// same "since/until with rounding" algorithm Gecko's `Duration_round`
    /// itself delegates to (`DifferenceZonedDateTimeWithRounding`), rather
    /// than re-deriving it: `round`/`total` compute a target instant via
    /// `AddZonedDateTime` and then call this with `(anchor, target)` as the
    /// two endpoints, exactly as `ZonedDateTime.prototype.until`/`since`
    /// call it with two real `ZonedDateTime`s.
    ///
    /// The `smallestUnit` day/week/month/year branch is `RoundRelativeDuration`'s
    /// real, day-length-aware fractional-position algorithm
    /// (`zoned_date_time::nudge_to_calendar_unit`/`bubble_relative_duration`,
    /// ported directly from Gecko's `NudgeToCalendarUnit`/
    /// `BubbleRelativeDuration`) rather than the earlier, simpler
    /// approximation this function used to have (folding any nonzero sub-day
    /// remainder into a whole extra day toward the overall duration's sign,
    /// regardless of `roundingMode` — correct only for `"ceil"`/`"expand"`,
    /// confirmed wrong for every other mode by the pinned Test262 corpus's
    /// own `since`/`until` `roundingmode-*.js` fixtures at `smallestUnit:
    /// "days"`).
    #[allow(clippy::too_many_arguments)]
    fn temporal_zoned_date_time_difference_fields(
        zone: &time_zone::TimeZone,
        calendar_kind: AnyCalendarKind,
        existing_epoch_ns: &BigInt,
        date1: epoch::CivilDate,
        time1: epoch::CivilTime,
        other_epoch_ns: &BigInt,
        date2: epoch::CivilDate,
        time2: epoch::CivilTime,
        largest_unit: rounding::TemporalUnit,
        smallest_unit: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<DateTimeDurationFields, RuntimeError> {
        if largest_unit < rounding::TemporalUnit::Day {
            let diff_ns = i128::try_from(other_epoch_ns - existing_epoch_ns)
                .expect("an Instant-range difference fits in i128");
            let rounded = duration_math::TimeDuration::from_nanoseconds(diff_ns).round(
                Self::temporal_unit_to_time_unit(smallest_unit),
                increment,
                mode,
            );
            let [h, m, s, ms, us, ns] =
                rounded.balance_to(Self::temporal_unit_to_time_unit(largest_unit));
            Ok((0, 0, 0, 0, h, m, s, ms, us, ns))
        } else if existing_epoch_ns == other_epoch_ns {
            // `DifferenceTemporalZonedDateTime` step 8: once the epoch
            // instants are already known equal, short-circuit to a blank
            // duration *before* doing any calendar-day bracketing at all --
            // not just an optimization, a real spec-ordering requirement
            // (Gecko's own `ZonedDateTime.cpp` checks this ahead of calling
            // `DifferenceZonedDateTimeWithRounding`). Confirmed as a real,
            // previously-missing fast path via `built-ins/Temporal/
            // ZonedDateTime/prototype/{since,until}/same-epoch-nanoseconds.js`,
            // which iterates every `smallestUnit`/`largestUnit`/time-zone
            // combination (660 calls) with the receiver and argument always
            // at the *same* instant -- expensive enough, run unconditionally
            // through the full calendar-bracketing path below, to exhaust
            // the Test262 harness's own per-script instruction budget before
            // this fast path existed.
            Ok((0, 0, 0, 0, 0, 0, 0, 0, 0, 0))
        } else {
            let range_error = || {
                RuntimeError::RangeError("Temporal.since/until is out of range".into())
            };
            let date_unit_largest = Self::temporal_unit_to_date_unit(largest_unit);
            let (years, months, weeks, days, remainder_ns) =
                zoned_date_time::difference_zoned_date_time(
                    zone,
                    calendar_kind,
                    existing_epoch_ns,
                    date1,
                    time1,
                    other_epoch_ns,
                    date2,
                    time2,
                    date_unit_largest,
                )
                .ok_or_else(range_error)?;
            if smallest_unit >= rounding::TemporalUnit::Day {
                let overall_sign = match other_epoch_ns - existing_epoch_ns {
                    diff if diff > BigInt::from(0) => 1_i64,
                    diff if diff < BigInt::from(0) => -1_i64,
                    _ => 0_i64,
                };
                let date_unit_smallest = Self::temporal_unit_to_date_unit(smallest_unit);
                let nudge = zoned_date_time::nudge_to_calendar_unit(
                    zone,
                    calendar_kind,
                    date1,
                    time1,
                    other_epoch_ns,
                    (years, months, weeks, days),
                    date_unit_smallest,
                    increment,
                    overall_sign,
                    mode,
                )
                .ok_or_else(range_error)?;
                let (years, months, weeks, days) =
                    if nudge.expanded && date_unit_smallest != plain_date::DateUnit::Week {
                        zoned_date_time::bubble_relative_duration(
                            zone,
                            calendar_kind,
                            date1,
                            time1,
                            &nudge,
                            date_unit_largest,
                            date_unit_smallest,
                            overall_sign,
                        )
                        .ok_or_else(range_error)?
                    } else {
                        (nudge.years, nudge.months, nudge.weeks, nudge.days)
                    };
                Ok((years, months, weeks, days, 0, 0, 0, 0, 0, 0))
            } else {
                let time_unit = Self::temporal_unit_to_time_unit(smallest_unit);
                let rounded = duration_math::TimeDuration::from_nanoseconds(remainder_ns)
                    .round(time_unit, increment, mode);
                let [h, m, s, ms, us, ns] = rounded.balance_to(rounding::TimeUnit::Hour);
                Ok((years, months, weeks, days, h, m, s, ms, us, ns))
            }
        }
    }

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
        // `DifferenceTemporalZonedDateTime` only requires `TimeZoneEquals`
        // (canonical zone identity, not raw spelling -- see
        // `TimeZone::time_zone_equals`'s own doc comment) once `largestUnit`
        // is `"day"` or coarser -- a pure time-unit difference (`largestUnit`
        // finer than `"day"`, the branch
        // `temporal_zoned_date_time_difference_fields` itself takes for
        // `largest_unit < TemporalUnit::Day`) is a plain epoch-instant
        // subtraction that never consults either operand's zone at all, so
        // two `ZonedDateTime`s in genuinely different zones may still be
        // diffed that way (`zoneddatetime-string.js`/
        // `argument-string-time-zone-annotation.js`, both using the default
        // `"hour"` largest unit -- checking zone equality unconditionally
        // regressed exactly these). Calendar-unit bracketing below, by
        // contrast, only ever resolves through the *receiver's* own zone, so
        // mismatched zones there must be rejected
        // (`canonicalize-iana-identifiers-before-comparing.js`: two IANA
        // aliases of the same real zone must not throw, but two genuinely
        // different zones must).
        if largest_unit >= rounding::TemporalUnit::Day
            && !temporal_zoned_date_time_zone(&existing)
                .time_zone_equals(&temporal_zoned_date_time_zone(&other))
        {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same time zone".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        let mode = Self::temporal_validated_rounding_mode(
            mode_raw.as_deref(),
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;
        // Same reflection `Vm::temporal_date_difference` needs, and for the
        // identical reason: `temporal_zoned_date_time_difference_fields`'s
        // own rounding steps (`TimeDuration::round` for the sub-day branch,
        // `zoned_date_time::nudge_to_calendar_unit`'s `nudge_expand_decision`
        // for the calendar-unit branch) both round a *real*, direction-aware
        // signed quantity computed in the fixed receiver-to-argument
        // direction — `Ceil`/`Floor` round toward a fixed end of the real
        // number line, not toward a fixed end of whichever internal
        // direction happened to be computed — so negating the *result* for
        // `since` without also reflecting an asymmetric mode here would
        // silently round the wrong way. Confirmed via
        // `built-ins/Temporal/ZonedDateTime/prototype/since/
        // roundingmode-{ceil,floor,halfCeil,halfFloor}.js`.
        let effective_mode = if since {
            use blueice_ecma402::NumberRoundingMode as Mode;
            match mode {
                Mode::Ceil => Mode::Floor,
                Mode::Floor => Mode::Ceil,
                Mode::HalfCeil => Mode::HalfFloor,
                Mode::HalfFloor => Mode::HalfCeil,
                other => other,
            }
        } else {
            mode
        };

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
        ) = Self::temporal_zoned_date_time_difference_fields(
            &zone,
            calendar_kind,
            &existing.epoch_nanoseconds,
            (existing.year, existing.month, existing.day),
            (
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            ),
            &other.epoch_nanoseconds,
            (other.year, other.month, other.day),
            (
                other.hour,
                other.minute,
                other.second,
                other.millisecond,
                other.microsecond,
                other.nanosecond,
            ),
            largest_unit,
            smallest_unit,
            increment,
            effective_mode,
        )?;

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
        // `TimeZoneEquals`: compares primary-zone identity, not raw stored
        // spelling -- an IANA alias and its target (`Asia/Calcutta` /
        // `Asia/Kolkata`) are the same zone even though each value's own
        // `time_zone` field preserves whichever spelling was written (see
        // `TimeZone::time_zone_equals`'s own doc comment).
        let existing_zone = temporal_zoned_date_time_zone(&existing);
        let other_zone = temporal_zoned_date_time_zone(&other);
        Ok(Value::Bool(
            existing.epoch_nanoseconds == other.epoch_nanoseconds
                && existing_zone.time_zone_equals(&other_zone)
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
            ..Default::default()
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

    /// `Temporal.ZonedDateTime.prototype.getTimeZoneTransition`
    /// (`GetDirectionOption` + `GetNamedTimeZoneNextTransition`/
    /// `GetNamedTimeZonePreviousTransition`, delegating the actual real-data
    /// lookup to [`time_zone::TimeZone::adjacent_transition`]). `direction`
    /// is required (a `TypeError` if the argument itself is `undefined`,
    /// mirroring `Temporal.Instant.prototype.round`'s own `roundTo` shape);
    /// a bare String is shorthand for `{ direction: <string> }`, the same
    /// pattern [`Self::temporal_round_to`] already establishes for
    /// `roundTo`. `null` is the spec's own result for "no such transition",
    /// distinct from every other Temporal getter/method on this type.
    pub(super) fn temporal_zoned_date_time_get_time_zone_transition(
        &mut self,
        receiver: &Value,
        direction_param: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        if *direction_param == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime.prototype.getTimeZoneTransition requires a direction"
                    .into(),
            ));
        }
        let options = if matches!(direction_param, Value::String(_)) {
            let object = self.with_roots(|heap| heap.alloc_object(None))?;
            let result = Value::Object(object);
            self.stack.push(result.clone());
            self.define_data(
                object,
                "direction",
                direction_param.clone(),
                true,
                true,
                true,
            )?;
            result
        } else {
            self.temporal_options(direction_param)?
        };
        let direction_v = self.get_property(&options, &"direction".into())?;
        if direction_v == Value::Undefined {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime.prototype.getTimeZoneTransition requires a direction \
                 option"
                    .into(),
            ));
        }
        let direction_s = self.coerce_string(&direction_v)?;
        let direction_s = direction_s
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid direction option".into()))?;
        let forward = match direction_s.as_str() {
            "next" => true,
            "previous" => false,
            _ => return Err(RuntimeError::RangeError("invalid direction option".into())),
        };
        let zone = temporal_zoned_date_time_zone(&existing);
        let Some(transition_ns) = zone.adjacent_transition(&existing.epoch_nanoseconds, forward)
        else {
            return Ok(Value::Null);
        };
        existing.epoch_nanoseconds = transition_ns;
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
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

/// `RoundNumberToIncrement(offsetNanoseconds, 60e9, "halfExpand")`: rounds a
/// real UTC offset to the nearest whole minute, ties rounding away from
/// zero. Only meaningful for [`temporal_interpret_offset`]'s `match_minutes`
/// (`MatchBehaviour::MatchMinutes`) comparison -- see that function's own
/// doc comment.
fn round_offset_nanoseconds_to_minutes(offset_nanoseconds: i64) -> i64 {
    const MINUTE: i64 = 60_000_000_000;
    let quotient = offset_nanoseconds / MINUTE;
    let remainder = offset_nanoseconds % MINUTE;
    let rounded = if remainder.unsigned_abs() * 2 >= MINUTE.unsigned_abs() {
        quotient + if offset_nanoseconds > 0 { 1 } else { -1 }
    } else {
        quotient
    };
    rounded * MINUTE
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
///
/// `match_minutes` (`MatchBehaviour::MatchMinutes` vs. `MatchExactly`):
/// besides an exact match against a real candidate's own offset, also accept
/// a candidate whose real offset *rounded to the nearest minute* equals the
/// given offset -- legacy back-compat for a `ZonedDateTime` string's
/// minute-precision (no seconds spelled) leading offset against a named
/// zone with genuine historical sub-minute precision (`Africa/Monrovia`'s
/// pre-1972 `-00:44:30`, matched by a written `-00:45`). A property-bag
/// `offset` field and `.with()`'s own `offset` property are always
/// `MatchExactly`, per Gecko's `ZonedDateTime.cpp`
/// (`ToTemporalZonedDateTime`'s object overload, and `with`, both construct
/// `MatchBehaviour::MatchExactly` unconditionally -- only the *string*
/// overload of `ToTemporalZonedDateTime` ever picks `MatchMinutes`, and only
/// when the leading offset itself was not spelled with sub-minute
/// precision).
#[allow(clippy::too_many_arguments)]
fn temporal_interpret_offset(
    zone: &time_zone::TimeZone,
    date: epoch::CivilDate,
    time: epoch::CivilTime,
    offset_nanoseconds: Option<i64>,
    utc_exact: bool,
    disambiguation: time_zone::Disambiguation,
    offset_option: &str,
    match_minutes: bool,
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
    let possible = zone.possible_epoch_nanoseconds(date, time);
    for candidate in &possible {
        let candidate_offset = zone.offset_nanoseconds_for(candidate);
        if candidate_offset == offset_ns
            || (match_minutes && round_offset_nanoseconds_to_minutes(candidate_offset) == offset_ns)
        {
            return Ok(candidate.clone());
        }
    }
    match offset_option {
        "use" => Ok(&local - BigInt::from(offset_ns)),
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
