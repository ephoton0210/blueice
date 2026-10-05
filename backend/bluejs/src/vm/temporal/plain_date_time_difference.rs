// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Temporal.PlainDateTime.prototype.until`/`since` and
//! `Temporal.PlainDate.prototype.until`/`since` arithmetic (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `DifferenceTemporalPlainDate` is `DifferenceTemporalPlainDateTime` at
//! midnight, so both receivers share the rounding steps below and differ only
//! in their entry point ([`difference_plain_date`] and
//! [`difference_plain_date_time`]).
//!
//! A `PlainDateTime` difference is not a `PlainDate` difference plus a
//! separately-rounded time-of-day: the time part decides whether the date
//! part needs a one-day borrow, `largestUnit: "hours"` (or any finer unit)
//! folds *every* whole day into the time fields, and rounding to `day`,
//! `week`, `month` or `year` measures the fraction from the exact
//! nanosecond position of the argument, not from its calendar date alone.
//! This module is the direct port of `DifferenceISODateTime`,
//! `DifferencePlainDateTimeWithRounding`, the no-time-zone branches of
//! `RoundRelativeDuration` (`NudgeToCalendarUnit`, `NudgeToDayOrTime`),
//! `BubbleRelativeDuration` and `TemporalDurationFromInternal` from Gecko's
//! `PlainDateTime.cpp`/`Duration.cpp`
//! (`development/browser_core/reference/gecko/js/src/builtin/temporal/`).
//!
//! With no time zone, every point is a wall-clock date-time read as UTC, so
//! exact epoch positions are plain `i128` nanosecond counts.
//!
//! No `Value`/heap/Realm coupling, matching every other module in this
//! directory — directly unit-testable without a VM.

use super::duration_math::{self, NANOSECONDS_PER_DAY};
use super::epoch::{self, CivilDate, CivilTime};
use super::plain_date::{self, DateUnit};
use super::rounding::{self, TemporalUnit};
use icu_calendar::AnyCalendarKind;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/plain_difference_boundaries.rs"]
mod boundary_tests;

/// The ten `Temporal.Duration` fields, in `years..nanoseconds` order. `i128`
/// throughout, because folding a multi-century span into `nanoseconds`
/// (`largestUnit: "nanoseconds"`) overflows `i64` long before the caller's
/// own `IsValidDuration` bound can reject it.
pub(crate) type DifferenceFields = [i128; 10];

type DateFields = (i64, i64, i64, i64);

/// The specification's `InternalDuration`: a calendar `DateDuration` plus one
/// exact, sign-carrying nanosecond `TimeDuration`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct InternalDuration {
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    time: i128,
}

impl InternalDuration {
    fn new((years, months, weeks, days): DateFields, time: i128) -> Self {
        Self {
            years,
            months,
            weeks,
            days,
            time,
        }
    }

    fn date(self) -> DateFields {
        (self.years, self.months, self.weeks, self.days)
    }

    /// `InternalDurationSign`, as `-1` or `1` (a zero duration never reaches
    /// a rounding step).
    fn sign(self) -> i64 {
        let first_date_field = [self.years, self.months, self.weeks, self.days]
            .into_iter()
            .find(|field| *field != 0);
        let negative = first_date_field.map_or(self.time < 0, |field| field < 0);
        if negative {
            -1
        } else {
            1
        }
    }
}

/// `DifferenceTemporalPlainDateTime`'s numeric core (steps 5-7): the
/// duration from `(date1, time1)` to `(date2, time2)` at `largest`
/// granularity, rounded to `increment` `smallest`s per `mode`. Always the
/// fixed receiver-to-argument direction — `since` negates the finished
/// fields (and reflects an asymmetric `mode`) at its call site.
///
/// `None` only when an intermediate calendar date leaves the representable
/// range; the caller maps that to the specification's `RangeError`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn difference_plain_date_time(
    calendar: AnyCalendarKind,
    (date1, time1): (CivilDate, CivilTime),
    (date2, time2): (CivilDate, CivilTime),
    largest: TemporalUnit,
    increment: i128,
    smallest: TemporalUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<DifferenceFields> {
    if date1 == date2 && time1 == time2 {
        return Some([0; 10]);
    }
    let diff = difference_iso_date_time(calendar, date1, time1, date2, time2, largest);
    let rounded = if smallest == TemporalUnit::Nanosecond && increment == 1 {
        diff
    } else {
        let origin = Point {
            calendar,
            date: date1,
            time: time1,
        };
        round_relative_duration(
            origin,
            epoch_nanoseconds(date2, time2),
            diff,
            largest,
            increment,
            smallest,
            mode,
        )?
    };
    Some(duration_from_internal(rounded, largest))
}

/// `DifferenceTemporalPlainDate`'s numeric core (steps 5-12): the same
/// algorithm as [`difference_plain_date_time`] with both times at midnight,
/// so there is no time-of-day borrow and `largest`/`smallest` are never finer
/// than a day. The specification skips rounding only for the default
/// `smallest` of `day` with an increment of 1 (step 11), where the calendar
/// difference is already whole days.
///
/// `None` only when a rounding bracket leaves the representable range; the
/// caller maps that to the specification's `RangeError`.
pub(crate) fn difference_plain_date(
    calendar: AnyCalendarKind,
    date1: CivilDate,
    date2: CivilDate,
    largest: TemporalUnit,
    increment: i128,
    smallest: TemporalUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<DifferenceFields> {
    difference_between_midnights(
        calendar,
        date1,
        date2,
        (largest, increment, smallest, mode),
        TemporalUnit::Day,
    )
}

/// The rounding settings shared by every entry point:
/// `(largest, increment, smallest, mode)`.
type RoundingSettings = (
    TemporalUnit,
    i128,
    TemporalUnit,
    blueice_ecma402::NumberRoundingMode,
);

/// A difference between two dates read as midnights. `unrounded_unit` is the
/// `smallestUnit` for which an increment of 1 needs no rounding step: the
/// calendar difference is already whole in that unit.
fn difference_between_midnights(
    calendar: AnyCalendarKind,
    date1: CivilDate,
    date2: CivilDate,
    (largest, increment, smallest, mode): RoundingSettings,
    unrounded_unit: TemporalUnit,
) -> Option<DifferenceFields> {
    const MIDNIGHT: CivilTime = (0, 0, 0, 0, 0, 0);
    if date1 == date2 {
        return Some([0; 10]);
    }
    let (years, months, weeks, days) =
        plain_date::calendar_difference_date(calendar, date1, date2, date_unit(largest));
    let diff = InternalDuration::new((years, months, weeks, days), 0);
    let rounded = if smallest == unrounded_unit && increment == 1 {
        diff
    } else {
        let origin = Point {
            calendar,
            date: date1,
            time: MIDNIGHT,
        };
        round_relative_duration(
            origin,
            epoch_nanoseconds(date2, MIDNIGHT),
            diff,
            largest,
            increment,
            smallest,
            mode,
        )?
    };
    Some(duration_from_internal(rounded, largest))
}

/// `DifferencePlainDateTimeWithTotal`: the exact, unrounded count of `unit`s
/// from `(date1, time1)` to `(date2, time2)` as a `(numerator, denominator)`
/// fraction with `denominator > 0`, so the caller converts to a Number once
/// (`rounding::exact_ratio_to_f64`).
///
/// A time `unit` is the plain nanosecond difference and `day` adds the whole
/// days at 24 hours each; `week`, `month` and `year` measure the argument's
/// position inside the bracketing calendar-unit window, exactly as
/// `NudgeToCalendarUnit` does for rounding (`TotalRelativeDuration`).
///
/// `None` only when a bracket leaves the representable range; the caller maps
/// that to the specification's `RangeError`.
pub(crate) fn difference_plain_date_time_total(
    calendar: AnyCalendarKind,
    (date1, time1): (CivilDate, CivilTime),
    (date2, time2): (CivilDate, CivilTime),
    unit: TemporalUnit,
) -> Option<(i128, i128)> {
    if date1 == date2 && time1 == time2 {
        return Some((0, 1));
    }
    let diff = difference_iso_date_time(calendar, date1, time1, date2, time2, unit);
    if !unit.is_calendar() {
        // `unit` is a day or a time unit: `diff` has no year, month or week.
        let nanoseconds = diff.time + i128::from(diff.days) * NANOSECONDS_PER_DAY;
        let length = unit
            .nanoseconds()
            .expect("every non-calendar unit has an exact length");
        return Some((nanoseconds, length));
    }
    let origin = Point {
        calendar,
        date: date1,
        time: time1,
    };
    let NudgePosition {
        window,
        numerator,
        denominator,
        ..
    } = nudge_position(origin, epoch_nanoseconds(date2, time2), diff, 1, unit)?;
    // total = r1 + progress * sign, with progress = numerator / denominator.
    let count = i128::from(window.r1);
    // Both endpoints of the bracket are representable dates. Its count is
    // bounded by a 201,000,000-day upper bound, as are its length and the
    // destination offset. Even their product fits well within i128.
    Some((
        count * denominator + numerator * i128::from(diff.sign()),
        denominator,
    ))
}

fn time_nanoseconds(time: CivilTime) -> i128 {
    duration_math::time_fields_to_nanoseconds(time.0, time.1, time.2, time.3, time.4, time.5)
}

/// The exact position of a wall-clock date-time read as UTC.
fn epoch_nanoseconds(date: CivilDate, time: CivilTime) -> i128 {
    i128::from(plain_date::iso_date_to_epoch_days(date)) * NANOSECONDS_PER_DAY
        + time_nanoseconds(time)
}

/// `DifferenceISODateTime`.
fn difference_iso_date_time(
    calendar: AnyCalendarKind,
    date1: CivilDate,
    time1: CivilTime,
    date2: CivilDate,
    time2: CivilTime,
    largest: TemporalUnit,
) -> InternalDuration {
    let mut time = time_nanoseconds(time2) - time_nanoseconds(time1);
    let time_sign = time.signum();
    // The sign of `date2 - date1`; the specification's `CompareISODate`
    // takes the opposite argument order, so its `timeSign = dateSign` check
    // is this function's `timeSign = -dateSign` one.
    let date_sign = match plain_date::compare_iso_date(date1, date2) {
        std::cmp::Ordering::Less => 1_i128,
        std::cmp::Ordering::Greater => -1,
        std::cmp::Ordering::Equal => 0,
    };
    let mut adjusted_date = date2;
    if time_sign != 0 && time_sign == -date_sign {
        // The time-of-day runs against the date direction, so the date part
        // is one day too long: borrow that day back into the time part.
        adjusted_date = plain_date::add_iso_date(date2, 0, 0, 0, -(date_sign as i64), false)
            .expect("borrowing one day between two valid ordered dates is representable");
        time += date_sign * NANOSECONDS_PER_DAY;
    }
    let date_largest = date_unit(largest);
    let (years, months, weeks, mut days) =
        plain_date::calendar_difference_date(calendar, date1, adjusted_date, date_largest);
    if largest < TemporalUnit::Day {
        // A time-unit `largestUnit` has no day field to report into.
        time += i128::from(days) * NANOSECONDS_PER_DAY;
        days = 0;
    }
    InternalDuration {
        years,
        months,
        weeks,
        days,
        time,
    }
}

/// The origin every rounding step measures from.
#[derive(Clone, Copy)]
struct Point {
    calendar: AnyCalendarKind,
    date: CivilDate,
    time: CivilTime,
}

impl Point {
    /// `CalendarDateAdd(calendar, date, duration, constrain)` followed by the
    /// `ISODateWithinLimits` check the specification makes on every result,
    /// then the exact position of that date at the origin's time-of-day.
    fn epoch_after(self, (years, months, weeks, days): DateFields) -> Option<i128> {
        let date = plain_date::calendar_add_date(
            self.calendar,
            self.date,
            years,
            months,
            weeks,
            days,
            false,
        )?;
        epoch::is_date_within_limits(date).then(|| epoch_nanoseconds(date, self.time))
    }
}

/// `RoundRelativeDuration` with no time zone.
fn round_relative_duration(
    origin: Point,
    dest_epoch_ns: i128,
    duration: InternalDuration,
    largest: TemporalUnit,
    increment: i128,
    smallest: TemporalUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<InternalDuration> {
    let (nudged, nudged_epoch_ns, expanded) = if smallest.is_calendar() {
        nudge_to_calendar_unit(origin, dest_epoch_ns, duration, increment, smallest, mode)?
    } else {
        nudge_to_day_or_time(dest_epoch_ns, duration, largest, increment, smallest, mode)
    };
    if expanded && smallest != TemporalUnit::Week {
        return bubble_relative_duration(
            origin,
            duration.sign(),
            nudged,
            nudged_epoch_ns,
            largest,
            smallest.max(TemporalUnit::Day),
        );
    }
    Some(nudged)
}

/// One bracket of `ComputeNudgeWindow`: the two calendar-unit candidates
/// (`r1` toward zero, `r2` one increment further) on either side of the
/// argument, with the exact position of each.
struct NudgeWindow {
    r1: i64,
    start: DateFields,
    end: DateFields,
    start_ns: i128,
    end_ns: i128,
}

/// `ComputeNudgeWindow` for a calendar `unit`. `additional_shift` slides the
/// bracket one increment further out, for an argument that sits beyond the
/// first bracket's far edge (the duration's day remainder overshot the
/// month it was measured in).
fn compute_nudge_window(
    origin: Point,
    origin_ns: i128,
    duration: InternalDuration,
    increment: i128,
    unit: TemporalUnit,
    additional_shift: bool,
) -> Option<NudgeWindow> {
    let sign = duration.sign();
    let increment_i64 = i64::try_from(increment).ok()?;
    if increment_i64 <= 0 {
        return None;
    }
    // A positive i64 multiplied by either direction cannot overflow.
    let step = increment_i64 * sign;
    let truncate = |value: i64| -> i64 {
        rounding::round_to_increment(
            i128::from(value),
            increment,
            blueice_ecma402::NumberRoundingMode::Trunc,
        ) as i64
    };
    let (years, months) = (duration.years, duration.months);
    let base = match unit {
        TemporalUnit::Year => truncate(years),
        TemporalUnit::Month => truncate(months),
        _ => {
            // `unit` is week: how many whole weeks the day remainder holds,
            // measured from the date the years and months land on.
            let weeks_start = plain_date::calendar_add_date(
                origin.calendar,
                origin.date,
                years,
                months,
                0,
                0,
                false,
            )?;
            let weeks_end = plain_date::add_iso_date(weeks_start, 0, 0, 0, duration.days, false)
                .expect(
                "adding only days leaves the validated year and month unchanged before balancing",
            );
            let (_, _, extra_weeks, _) = plain_date::calendar_difference_date(
                origin.calendar,
                weeks_start,
                weeks_end,
                DateUnit::Week,
            );
            truncate(duration.weeks.checked_add(extra_weeks)?)
        }
    };
    let r1 = if additional_shift {
        base.checked_add(step)?
    } else {
        base
    };
    let r2 = r1.checked_add(step)?;
    let bracket = |count: i64| -> DateFields {
        match unit {
            TemporalUnit::Year => (count, 0, 0, 0),
            TemporalUnit::Month => (years, count, 0, 0),
            _ => (years, months, count, 0),
        }
    };
    let (start, end) = (bracket(r1), bracket(r2));
    let start_ns = if start == (0, 0, 0, 0) {
        origin_ns
    } else {
        origin.epoch_after(start)?
    };
    let end_ns = origin.epoch_after(end)?;
    Some(NudgeWindow {
        r1,
        start,
        end,
        start_ns,
        end_ns,
    })
}

/// Where the argument sits inside its [`NudgeWindow`]: the window itself,
/// whether it had to be shifted one increment outward to contain the
/// argument, and the argument's exact offset from the window's start
/// (`numerator`) against the window's length (`denominator`), both
/// non-negative.
struct NudgePosition {
    window: NudgeWindow,
    shifted: bool,
    numerator: i128,
    denominator: i128,
}

/// `NudgeToCalendarUnit` steps 1-14: brackets the argument between two
/// calendar-unit candidates and measures where it falls between them.
fn nudge_position(
    origin: Point,
    dest_epoch_ns: i128,
    duration: InternalDuration,
    increment: i128,
    unit: TemporalUnit,
) -> Option<NudgePosition> {
    let sign = duration.sign();
    let origin_ns = epoch_nanoseconds(origin.date, origin.time);
    let mut window = compute_nudge_window(origin, origin_ns, duration, increment, unit, false)?;
    let mut shifted = false;
    let (near, far) = if sign > 0 {
        (window.start_ns, window.end_ns)
    } else {
        (window.end_ns, window.start_ns)
    };
    if !(near <= dest_epoch_ns && dest_epoch_ns <= far) {
        window = compute_nudge_window(origin, origin_ns, duration, increment, unit, true)?;
        shifted = true;
    }
    let mut numerator = dest_epoch_ns - window.start_ns;
    let mut denominator = window.end_ns - window.start_ns;
    if denominator < 0 {
        numerator = -numerator;
        denominator = -denominator;
    }
    Some(NudgePosition {
        window,
        shifted,
        numerator,
        denominator,
    })
}

/// `NudgeToCalendarUnit`: rounds to the nearer of two calendar-unit
/// brackets, deciding by where the argument's exact position falls between
/// them. Returns the rounded duration (its time part is always zero), the
/// exact position it stands for, and whether it moved to the outer bracket.
fn nudge_to_calendar_unit(
    origin: Point,
    dest_epoch_ns: i128,
    duration: InternalDuration,
    increment: i128,
    unit: TemporalUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<(InternalDuration, i128, bool)> {
    let sign = duration.sign();
    let NudgePosition {
        window,
        shifted,
        numerator,
        denominator,
    } = nudge_position(origin, dest_epoch_ns, duration, increment, unit)?;
    let rounded_up = rounds_up(numerator, denominator, window.r1, increment, sign, mode);
    let (date, position) = if rounded_up {
        (window.end, window.end_ns)
    } else {
        (window.start, window.start_ns)
    };
    Some((
        InternalDuration::new(date, 0),
        position,
        shifted || rounded_up,
    ))
}

/// The round-up decision of `NudgeToCalendarUnit` steps 18-21
/// (`ApplyUnsignedRoundingMode` over the exact `numerator / denominator`
/// position between the two brackets; `sign` orients `ceil`/`floor` and the
/// half-`ceil`/`floor` modes).
pub(crate) fn rounds_up(
    numerator: i128,
    denominator: i128,
    r1: i64,
    increment: i128,
    sign: i64,
    mode: blueice_ecma402::NumberRoundingMode,
) -> bool {
    use blueice_ecma402::NumberRoundingMode as Mode;
    if numerator == denominator {
        return true;
    }
    if numerator == 0 {
        return false;
    }
    let negative = sign < 0;
    let doubled = numerator + numerator;
    match mode {
        Mode::Trunc => false,
        Mode::Expand => true,
        Mode::Ceil => !negative,
        Mode::Floor => negative,
        Mode::HalfTrunc => doubled > denominator,
        Mode::HalfExpand => doubled >= denominator,
        Mode::HalfCeil if negative => doubled > denominator,
        Mode::HalfCeil => doubled >= denominator,
        Mode::HalfFloor if negative => doubled >= denominator,
        Mode::HalfFloor => doubled > denominator,
        Mode::HalfEven => {
            if doubled == denominator {
                (i128::from(r1) / increment) % 2 != 0
            } else {
                doubled > denominator
            }
        }
    }
}

/// `NudgeToDayOrTime`: folds the date duration's whole days into the exact
/// time duration (a day is 24 hours with no time zone), rounds that total to
/// `increment` `smallest`s, and — when `largest` is a date unit — splits the
/// rounded whole days back out. Returns the rounded duration, the exact
/// position it stands for, and whether the rounding gained a whole day.
fn nudge_to_day_or_time(
    dest_epoch_ns: i128,
    duration: InternalDuration,
    largest: TemporalUnit,
    increment: i128,
    smallest: TemporalUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> (InternalDuration, i128, bool) {
    let time_duration = duration.time + i128::from(duration.days) * NANOSECONDS_PER_DAY;
    let unit_length = smallest
        .nanoseconds()
        .expect("day and every smaller unit has an exact length");
    let rounded_time = rounding::round_to_increment(time_duration, unit_length * increment, mode);
    let whole_days = time_duration / NANOSECONDS_PER_DAY;
    let rounded_whole_days = rounded_time / NANOSECONDS_PER_DAY;
    let expanded = (rounded_whole_days - whole_days).signum() == time_duration.signum();
    let (days, remainder) = if largest >= TemporalUnit::Day {
        (
            rounded_whole_days,
            rounded_time - rounded_whole_days * NANOSECONDS_PER_DAY,
        )
    } else {
        (0, rounded_time)
    };
    let days = i64::try_from(days).expect("a rounded day count of a valid difference fits in i64");
    let nudged = InternalDuration::new(
        (duration.years, duration.months, duration.weeks, days),
        remainder,
    );
    (
        nudged,
        dest_epoch_ns + (rounded_time - time_duration),
        expanded,
    )
}

/// `BubbleRelativeDuration`: after a rounding step expanded into a larger
/// bracket, checks whether the expansion now reaches the *next* calendar
/// unit's boundary, walking from `start` up toward (and including)
/// `largest` — rounding 11 months up to 12 becomes 1 year, 0 months under
/// `largestUnit: "years"`. A weeks count is only ever a bubbling target when
/// `largest` is itself weeks.
fn bubble_relative_duration(
    origin: Point,
    sign: i64,
    nudged: InternalDuration,
    nudged_epoch_ns: i128,
    largest: TemporalUnit,
    start: TemporalUnit,
) -> Option<InternalDuration> {
    if start >= largest {
        return Some(nudged);
    }
    let (mut years, mut months, mut weeks, mut days) = nudged.date();
    let mut time = nudged.time;
    let mut unit = start;
    while unit < largest {
        unit = match unit {
            TemporalUnit::Week => TemporalUnit::Month,
            TemporalUnit::Month => TemporalUnit::Year,
            _ => TemporalUnit::Week,
        };
        if unit == TemporalUnit::Week && largest != TemporalUnit::Week {
            continue;
        }
        let end_duration = match unit {
            TemporalUnit::Year => (years + sign, 0, 0, 0),
            TemporalUnit::Month => (years, months + sign, 0, 0),
            _ => (years, months, weeks + sign, 0),
        };
        let end_ns = origin.epoch_after(end_duration)?;
        // `nudged_epoch_ns` can lie outside the representable range; only
        // its ordering against `end_ns` matters here.
        let beyond_end_sign = (nudged_epoch_ns - end_ns).signum();
        if beyond_end_sign == i128::from(-sign) {
            break;
        }
        (years, months, weeks, days) = end_duration;
        time = 0;
    }
    Some(InternalDuration::new((years, months, weeks, days), time))
}

/// `TemporalDurationFromInternal`: splits the exact time duration into
/// day/hour/.../nanosecond fields, with everything above `largest` folded —
/// unbounded — into `largest`'s own field.
fn duration_from_internal(duration: InternalDuration, largest: TemporalUnit) -> DifferenceFields {
    let fold_into = largest.min(TemporalUnit::Day);
    let [days, hours, minutes, seconds, milliseconds, microseconds, nanoseconds] =
        duration_math::TimeDuration::from_nanoseconds(duration.time).balance_with_days(fold_into);
    [
        i128::from(duration.years),
        i128::from(duration.months),
        i128::from(duration.weeks),
        i128::from(duration.days) + days,
        hours,
        minutes,
        seconds,
        milliseconds,
        microseconds,
        nanoseconds,
    ]
}

/// A calendar `TemporalUnit` as the [`DateUnit`] the calendar arithmetic
/// takes; `day` and every exact unit below it map to `Day`.
fn date_unit(unit: TemporalUnit) -> DateUnit {
    match unit {
        TemporalUnit::Year => DateUnit::Year,
        TemporalUnit::Month => DateUnit::Month,
        TemporalUnit::Week => DateUnit::Week,
        _ => DateUnit::Day,
    }
}

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/plain_difference_internal.rs"]
mod tests;
