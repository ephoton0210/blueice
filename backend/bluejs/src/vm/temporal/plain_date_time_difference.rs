// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Temporal.PlainDateTime.prototype.until`/`since` arithmetic
//! (Phase 26 Stage 3, `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
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
    let diff = difference_iso_date_time(calendar, date1, time1, date2, time2, largest)?;
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
) -> Option<InternalDuration> {
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
        adjusted_date = plain_date::add_iso_date(date2, 0, 0, 0, -(date_sign as i64), false)?;
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
    Some(InternalDuration {
        years,
        months,
        weeks,
        days,
        time,
    })
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
        // No representable date is anywhere near a million years out; the
        // guard also keeps the limit check's own day arithmetic from
        // overflowing for a bracket a huge `roundingIncrement` slides to the
        // edge of `i32` years.
        (date.0.unsigned_abs() <= MAX_YEAR_MAGNITUDE && epoch::is_date_within_limits(date))
            .then(|| epoch_nanoseconds(date, self.time))
    }
}

/// Far beyond Temporal's ±275,760-year range, and still safe for
/// [`epoch::is_date_within_limits`]'s `i64` day arithmetic.
const MAX_YEAR_MAGNITUDE: u32 = 1_000_000;

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
    let step = i64::try_from(increment).ok()?.checked_mul(sign)?;
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
            let weeks_end = plain_date::add_iso_date(weeks_start, 0, 0, 0, duration.days, false)?;
            let (_, _, extra_weeks, _) = plain_date::calendar_difference_date(
                origin.calendar,
                weeks_start,
                weeks_end,
                DateUnit::Week,
            );
            truncate(duration.weeks + extra_weeks)
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
    let origin_ns = epoch_nanoseconds(origin.date, origin.time);
    let mut window = compute_nudge_window(origin, origin_ns, duration, increment, unit, false)?;
    let mut expanded = false;
    let (near, far) = if sign > 0 {
        (window.start_ns, window.end_ns)
    } else {
        (window.end_ns, window.start_ns)
    };
    if !(near <= dest_epoch_ns && dest_epoch_ns <= far) {
        window = compute_nudge_window(origin, origin_ns, duration, increment, unit, true)?;
        expanded = true;
    }
    let mut numerator = dest_epoch_ns - window.start_ns;
    let mut denominator = window.end_ns - window.start_ns;
    if denominator < 0 {
        numerator = -numerator;
        denominator = -denominator;
    }
    let rounded_up = rounds_up(numerator, denominator, window.r1, increment, sign, mode);
    let (date, position) = if rounded_up {
        (window.end, window.end_ns)
    } else {
        (window.start, window.start_ns)
    };
    Some((
        InternalDuration::new(date, 0),
        position,
        expanded || rounded_up,
    ))
}

/// The round-up decision of `NudgeToCalendarUnit` steps 18-21
/// (`ApplyUnsignedRoundingMode` over the exact `numerator / denominator`
/// position between the two brackets; `sign` orients `ceil`/`floor` and the
/// half-`ceil`/`floor` modes).
fn rounds_up(
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

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ecma402::NumberRoundingMode as Mode;

    const MIDNIGHT: CivilTime = (0, 0, 0, 0, 0, 0);

    fn diff(
        from: (CivilDate, CivilTime),
        to: (CivilDate, CivilTime),
        largest: TemporalUnit,
        increment: i128,
        smallest: TemporalUnit,
        mode: Mode,
    ) -> DifferenceFields {
        difference_plain_date_time(
            AnyCalendarKind::Iso,
            from,
            to,
            largest,
            increment,
            smallest,
            mode,
        )
        .expect("in-range inputs")
    }

    fn exact(
        from: (CivilDate, CivilTime),
        to: (CivilDate, CivilTime),
        largest: TemporalUnit,
    ) -> DifferenceFields {
        diff(from, to, largest, 1, TemporalUnit::Nanosecond, Mode::Trunc)
    }

    #[test]
    fn identical_date_times_have_a_zero_difference() {
        let point = ((2020, 1, 1), (1, 2, 3, 4, 5, 6));
        assert_eq!(exact(point, point, TemporalUnit::Year), [0; 10]);
    }

    #[test]
    fn a_time_unit_largest_unit_folds_every_whole_day_into_the_time_fields() {
        let from = ((2020, 1, 1), MIDNIGHT);
        let to = ((2020, 1, 3), (5, 0, 0, 0, 0, 0));
        assert_eq!(
            exact(from, to, TemporalUnit::Hour),
            [0, 0, 0, 0, 53, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            exact(from, to, TemporalUnit::Minute),
            [0, 0, 0, 0, 0, 3180, 0, 0, 0, 0]
        );
        let nanoseconds = exact(from, to, TemporalUnit::Nanosecond);
        assert_eq!(nanoseconds[9], 53 * 3_600_000_000_000);
        assert_eq!(nanoseconds[..9], [0; 9]);
    }

    #[test]
    fn a_time_of_day_running_against_the_date_direction_borrows_one_day() {
        // 2020-01-02T12:00 -> 2020-01-03T06:00 is 18 hours, not "1 day - 6 hours".
        let early = ((2020, 1, 2), (12, 0, 0, 0, 0, 0));
        let late = ((2020, 1, 3), (6, 0, 0, 0, 0, 0));
        assert_eq!(
            exact(early, late, TemporalUnit::Day),
            [0, 0, 0, 0, 18, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            exact(late, early, TemporalUnit::Day),
            [0, 0, 0, 0, -18, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn day_and_calendar_rounding_measure_the_time_of_day() {
        let from = ((2020, 1, 1), MIDNIGHT);
        let half_day_later = ((2020, 1, 2), (12, 0, 0, 0, 0, 0));
        // One day and twelve hours: `ceil` reaches two days, `trunc` stays at one.
        let day = TemporalUnit::Day;
        assert_eq!(
            diff(from, half_day_later, day, 1, day, Mode::Ceil),
            [0, 0, 0, 2, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            diff(from, half_day_later, day, 1, day, Mode::Trunc),
            [0, 0, 0, 1, 0, 0, 0, 0, 0, 0]
        );
        // One month and twelve hours: `ceil` to months reaches two months.
        let month = TemporalUnit::Month;
        assert_eq!(
            diff(
                from,
                ((2020, 2, 1), (12, 0, 0, 0, 0, 0)),
                month,
                1,
                month,
                Mode::Ceil
            ),
            [0, 2, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn rounding_a_time_remainder_up_bubbles_into_the_larger_calendar_units() {
        // 1y 11m 30d 23:59:59.999999999 rounded up at microseconds is exactly two years.
        assert_eq!(
            diff(
                ((1970, 1, 1), MIDNIGHT),
                ((1971, 12, 31), (23, 59, 59, 999, 999, 999)),
                TemporalUnit::Year,
                1,
                TemporalUnit::Microsecond,
                Mode::Expand
            ),
            [2, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn a_rounded_up_day_stays_in_hours_when_hours_are_the_largest_unit() {
        // Test262 `until/bubble-time-unit.js`: the rounding overflows a day,
        // but with `largestUnit: "hours"` there is no day field to carry into.
        assert_eq!(
            diff(
                ((2025, 6, 14), MIDNIGHT),
                ((2025, 6, 14), (14, 0, 0, 0, 0, 0)),
                TemporalUnit::Hour,
                12,
                TemporalUnit::Hour,
                Mode::Ceil
            ),
            [0, 0, 0, 0, 24, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn a_calendar_bracket_beyond_the_representable_range_is_reported() {
        // Rounding 1970-01-01..1971-01-01 to 100,000,000-month steps needs a
        // date 8 million years out: no `Some` result exists, so the caller
        // can raise the specification's `RangeError`.
        assert_eq!(
            difference_plain_date_time(
                AnyCalendarKind::Iso,
                ((1970, 1, 1), MIDNIGHT),
                ((1971, 1, 1), MIDNIGHT),
                TemporalUnit::Month,
                100_000_000,
                TemporalUnit::Month,
                Mode::Trunc,
            ),
            None
        );
    }

    #[test]
    fn a_nanosecond_total_beyond_i64_is_kept_exact() {
        // 600 years of nanoseconds exceeds `i64`; the fields are `i128` so the
        // caller can round the true total to a Number instead of a wrapped one.
        let result = exact(
            ((2000, 1, 1), MIDNIGHT),
            ((2600, 1, 1), MIDNIGHT),
            TemporalUnit::Nanosecond,
        );
        let days = i128::from(
            plain_date::iso_date_to_epoch_days((2600, 1, 1))
                - plain_date::iso_date_to_epoch_days((2000, 1, 1)),
        );
        assert_eq!(result[9], days * NANOSECONDS_PER_DAY);
        assert!(result[9] > i128::from(i64::MAX));
    }

    #[test]
    fn a_bracket_the_argument_overshoots_slides_one_increment_outward() {
        // A duration claiming one month for a target two and a half months
        // out — what an inconsistent calendar difference can hand back. The
        // first bracket [Feb 1, Mar 1] misses the target, so the window slides
        // to [Mar 1, Apr 1] and the result counts as expanded.
        let origin = Point {
            calendar: AnyCalendarKind::Iso,
            date: (2020, 1, 1),
            time: MIDNIGHT,
        };
        let (nudged, position, expanded) = nudge_to_calendar_unit(
            origin,
            epoch_nanoseconds((2020, 3, 15), MIDNIGHT),
            InternalDuration::new((0, 1, 0, 0), 0),
            1,
            TemporalUnit::Month,
            Mode::Trunc,
        )
        .expect("in-range bracket");
        assert_eq!(nudged.date(), (0, 2, 0, 0));
        assert_eq!(position, epoch_nanoseconds((2020, 3, 1), MIDNIGHT));
        assert!(expanded);
    }

    #[test]
    fn a_bracket_far_outside_the_representable_years_is_reported_not_overflowed() {
        // Two billion years fits an `i32` year but not the limit check's day
        // arithmetic: the result must be `None`, never an overflow panic.
        assert_eq!(
            difference_plain_date_time(
                AnyCalendarKind::Iso,
                ((2020, 1, 1), MIDNIGHT),
                ((2021, 1, 1), MIDNIGHT),
                TemporalUnit::Year,
                2_000_000_000,
                TemporalUnit::Year,
                Mode::Trunc,
            ),
            None
        );
    }

    #[test]
    fn the_round_up_decision_follows_the_unsigned_rounding_modes() {
        const MODES: [Mode; 9] = [
            Mode::Ceil,
            Mode::Floor,
            Mode::Expand,
            Mode::Trunc,
            Mode::HalfCeil,
            Mode::HalfFloor,
            Mode::HalfExpand,
            Mode::HalfTrunc,
            Mode::HalfEven,
        ];
        // The position is `numerator / 4` of the way between the brackets.
        let up = |mode, numerator, r1, sign| rounds_up(numerator, 4, r1, 1, sign, mode);
        for mode in MODES {
            for sign in [1, -1] {
                assert!(up(mode, 4, 0, sign), "{mode:?} at the far bracket");
                assert!(!up(mode, 0, 0, sign), "{mode:?} at the near bracket");
            }
        }
        // (mode, up at 1/4 for sign +1, at 1/4 for sign -1, at 3/4 for +1, at 3/4 for -1)
        let quarters = [
            (Mode::Ceil, true, false, true, false),
            (Mode::Floor, false, true, false, true),
            (Mode::Expand, true, true, true, true),
            (Mode::Trunc, false, false, false, false),
            (Mode::HalfCeil, false, false, true, true),
            (Mode::HalfFloor, false, false, true, true),
            (Mode::HalfExpand, false, false, true, true),
            (Mode::HalfTrunc, false, false, true, true),
            (Mode::HalfEven, false, false, true, true),
        ];
        for (mode, low_positive, low_negative, high_positive, high_negative) in quarters {
            assert_eq!(up(mode, 1, 0, 1), low_positive, "{mode:?} +1/4");
            assert_eq!(up(mode, 1, 0, -1), low_negative, "{mode:?} -1/4");
            assert_eq!(up(mode, 3, 0, 1), high_positive, "{mode:?} +3/4");
            assert_eq!(up(mode, 3, 0, -1), high_negative, "{mode:?} -3/4");
        }
        // Exactly halfway: (mode, sign +1, sign -1); `halfEven` follows `r1`'s parity.
        let halves = [
            (Mode::HalfCeil, true, false),
            (Mode::HalfFloor, false, true),
            (Mode::HalfExpand, true, true),
            (Mode::HalfTrunc, false, false),
        ];
        for (mode, positive, negative) in halves {
            assert_eq!(up(mode, 2, 0, 1), positive, "{mode:?} +1/2");
            assert_eq!(up(mode, 2, 0, -1), negative, "{mode:?} -1/2");
        }
        assert!(
            up(Mode::HalfEven, 2, 1, 1),
            "an odd lower bracket rounds up"
        );
        assert!(!up(Mode::HalfEven, 2, 2, 1), "an even lower bracket stays");
        assert!(
            up(Mode::HalfEven, 2, -1, -1),
            "a negative odd bracket rounds out"
        );
        // `halfEven` measures parity in whole increments, not raw counts.
        assert!(
            !rounds_up(2, 4, 6, 3, 1, Mode::HalfEven),
            "6 / 3 = 2 is even"
        );
        assert!(rounds_up(2, 4, 9, 3, 1, Mode::HalfEven), "9 / 3 = 3 is odd");
    }

    #[test]
    fn a_weeks_bubble_target_is_only_used_when_weeks_are_the_largest_unit() {
        // 1 week 6 days 23:59:59.999999999 rounded up to whole hours is 24
        // hours into day 7 — exactly two weeks.
        let from = ((2020, 1, 1), MIDNIGHT);
        let to = ((2020, 1, 14), (23, 59, 59, 999, 999, 999));
        assert_eq!(
            diff(
                from,
                to,
                TemporalUnit::Week,
                1,
                TemporalUnit::Hour,
                Mode::Expand
            ),
            [0, 0, 2, 0, 0, 0, 0, 0, 0, 0]
        );
        // With months as the largest unit the same rounding stays at 14 days:
        // a weeks count is never introduced unasked.
        assert_eq!(
            diff(
                from,
                to,
                TemporalUnit::Month,
                1,
                TemporalUnit::Hour,
                Mode::Expand
            ),
            [0, 0, 0, 14, 0, 0, 0, 0, 0, 0]
        );
    }
}
