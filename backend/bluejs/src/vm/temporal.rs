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
mod plain_date_time_difference;
mod plain_month_day;
mod plain_year_month;
mod receiver;
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
pub(super) enum UnitOption {
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

pub(super) struct TemporalCalendarFields {
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
pub(super) type DateTimeDurationFields = (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64);

/// How many sub-second digits an ISO serialization prints, as
/// `ToSecondsStringPrecision` resolves it: `Minute` omits the seconds field
/// entirely, `Auto` prints the shortest exact fraction (or none), and
/// `Digits(n)` prints exactly `n` digits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SecondsPrecision {
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
pub(super) enum DurationAnchor {
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

/// How `temporal_plain_date_from_fields` (and its year-month / month-day
/// siblings) obtains `overflow` (`GetTemporalOverflowOption`). Every caller
/// has not read its own `options` argument at all yet when it calls in:
/// `ToTemporalDate`'s real algorithm resolves `fields` strictly *before*
/// `resolvedOptions`, so `Options` defers that read to the correct point
/// (right before the calendar actually resolves the fields, after every field
/// has already been read). `ToTemporalZonedDateTime`'s property-bag branch
/// also reads `disambiguation` and `offset` from the same `resolvedOptions`,
/// just ahead of `overflow` -- see [`ZonedBagFields`].
pub(super) enum OverflowInput<'a> {
    Options(&'a Value),
}

/// The `ZonedDateTime`-only values `ToTemporalZonedDateTime`'s property-bag
/// branch collects while `temporal_calendar_date_from_bag` reads the
/// calendar-date fields. `PrepareCalendarFields` reads `offset` and `timeZone`
/// *in alphabetical position among* those fields (`offset` between
/// `nanosecond` and `second`; `timeZone` between `second` and `year`), each
/// converted the moment it is read, and only afterwards does
/// `ToTemporalZonedDateTime` fetch `options` and read `disambiguation`,
/// `offset` and finally `overflow`, in that order -- all observable through a
/// property-bag observer (`ZonedDateTime/from/order-of-operations.js`).
pub(super) struct ZonedBagFields {
    /// The `offset` field, already syntax-checked; `None` when absent.
    pub(super) offset_nanoseconds: Option<i64>,
    /// The `timeZone` field, already resolved; `None` when absent (a
    /// `TypeError` once every field has been read).
    pub(super) time_zone: Option<time_zone::TimeZone>,
    /// `GetTemporalDisambiguationOption`, read from `options` once the
    /// fields are.
    pub(super) disambiguation: time_zone::Disambiguation,
    /// `GetTemporalOffsetOption`, read from `options` once the fields are.
    pub(super) offset_option: String,
}

impl ZonedBagFields {
    pub(super) fn new() -> Self {
        Self {
            offset_nanoseconds: None,
            time_zone: None,
            disambiguation: time_zone::Disambiguation::Compatible,
            offset_option: "reject".into(),
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
                        TemporalKind::Instant => 1,
                        // `ZonedDateTime(epochNanoseconds, timeZone [, calendar])`:
                        // only the trailing `calendar` is optional.
                        TemporalKind::ZonedDateTime => 2,
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
                        NativeFunction::TemporalWithCalendar(kind),
                    )?;
                    self.install_native(
                        prototype,
                        function_prototype,
                        "toZonedDateTime",
                        1,
                        NativeFunction::TemporalPlainToZonedDateTime(kind),
                    )?;
                    for (name, arity, method) in [
                        ("with", 1, NativeFunction::TemporalDateWith(kind)),
                        ("add", 1, NativeFunction::TemporalDateAdd(kind)),
                        ("subtract", 1, NativeFunction::TemporalDateSubtract(kind)),
                        ("until", 1, NativeFunction::TemporalDateUntil(kind)),
                        ("since", 1, NativeFunction::TemporalDateSince(kind)),
                        ("equals", 1, NativeFunction::TemporalDateEquals(kind)),
                        ("toString", 0, NativeFunction::TemporalDateToString(kind)),
                        ("toJSON", 0, NativeFunction::TemporalDateToJson(kind)),
                        (
                            "toLocaleString",
                            0,
                            NativeFunction::TemporalDateToLocaleString(kind),
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
                        NativeFunction::TemporalGetter(kind, *getter),
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
                            NativeFunction::TemporalGetter(kind, getter),
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
                            NativeFunction::TemporalWithCalendar(kind),
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
}

mod conversion;
mod dates;
mod duration_conversion;
mod duration_operations;
mod duration_relative;
mod instant;
mod plain_time;
mod year_month;
mod zoned;

use zoned::{
    format_offset_nanoseconds_exact, temporal_interpret_offset, temporal_resolution_error,
    temporal_set_local_fields, temporal_zoned_date_time_zone,
};
