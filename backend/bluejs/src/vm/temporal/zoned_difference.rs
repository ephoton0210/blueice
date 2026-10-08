// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral difference, rounding and total against a zoned origin: the
//! specification's `DifferenceZonedDateTimeWithRounding` and
//! `DifferenceZonedDateTimeWithTotal`, with the `RoundRelativeDuration`
//! machinery (`NudgeToCalendarUnit`, `NudgeToZonedTime`,
//! `BubbleRelativeDuration`) they are built from.
//!
//! This is the *one* implementation behind `Temporal.ZonedDateTime.prototype.
//! until`/`since` and `Temporal.Duration.prototype.round`/`total` with a
//! `ZonedDateTime` `relativeTo` — the specification defines all four in terms
//! of the same two operations, and a named zone's day is not always 24 hours
//! (23 or 25 across a DST transition; Samoa once skipped a whole day), so a
//! "unbalance to the unit, then bracket" shortcut cannot stand in for it.
//! Ported from Gecko's `RoundRelativeDuration`/`NudgeTo*`/`BubbleRelative
//! Duration` (`reference/gecko/js/src/builtin/temporal/Duration.cpp`) and
//! `DifferenceZonedDateTime*` (`ZonedDateTime.cpp`).
//!
//! No `Value`/heap/Realm coupling: every function here returns `None` for the
//! specification's `RangeError` cases (an unrepresentable date or instant),
//! which the `Vm` adapters map to the JS exception.

use super::duration_math::TimeDuration;
use super::epoch::{self, CivilDate, CivilTime};
use super::plain_date::{self, DateUnit};
use super::plain_date_time_difference;
use super::rounding::{self, TemporalUnit, TimeUnit};
use super::time_zone::{Disambiguation, TimeZone};
use super::zoned_date_time;
use icu_calendar::AnyCalendarKind;
use num_bigint::BigInt;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/zoned_difference_boundaries.rs"]
mod boundary_tests;

/// The specification's `InternalDuration`: a calendar `years`/`months`/
/// `weeks`/`days` part plus one exact nanosecond time part (kept unbalanced,
/// since how it splits into hours/minutes/... depends on the caller's
/// `largestUnit`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InternalDuration {
    pub(crate) years: i64,
    pub(crate) months: i64,
    pub(crate) weeks: i64,
    pub(crate) days: i64,
    pub(crate) time_nanoseconds: i128,
}

impl InternalDuration {
    const ZERO: Self = Self {
        years: 0,
        months: 0,
        weeks: 0,
        days: 0,
        time_nanoseconds: 0,
    };

    fn from_date(years: i64, months: i64, weeks: i64, days: i64) -> Self {
        Self {
            years,
            months,
            weeks,
            days,
            time_nanoseconds: 0,
        }
    }

    /// `TemporalDurationFromInternal(duration, largestUnit)`: the ten Duration
    /// fields, with the time part balanced into hours, minutes, ... no higher
    /// than `largest_unit` allows — and never into `days`, which only the
    /// calendar part carries (a zoned day is not a fixed number of hours).
    pub(crate) fn into_fields(self, largest_unit: TemporalUnit) -> [i64; 10] {
        let time_largest = time_unit(largest_unit.min(TemporalUnit::Hour));
        let [hours, minutes, seconds, milliseconds, microseconds, nanoseconds] =
            TimeDuration::from_nanoseconds(self.time_nanoseconds).balance_to(time_largest);
        [
            self.years,
            self.months,
            self.weeks,
            self.days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        ]
    }

    /// `InternalDurationSign(duration) < 0 ? -1 : 1` — a blank duration is
    /// treated as positive, as every caller in this module needs.
    fn direction(&self) -> i64 {
        let first_non_zero = [self.years, self.months, self.weeks, self.days]
            .into_iter()
            .find(|field| *field != 0);
        let negative = match first_non_zero {
            Some(field) => field < 0,
            None => self.time_nanoseconds < 0,
        };
        if negative {
            -1
        } else {
            1
        }
    }
}

/// The receiver a difference is measured from: the zoned instant, and the
/// zone-local wall-clock date/time it resolves to (already known to every
/// caller, so it is not re-derived here).
pub(crate) struct ZonedOrigin<'a> {
    pub(crate) zone: &'a TimeZone,
    pub(crate) calendar: AnyCalendarKind,
    pub(crate) epoch_nanoseconds: &'a BigInt,
    pub(crate) date: CivilDate,
    pub(crate) time: CivilTime,
}

impl ZonedOrigin<'_> {
    /// `CalendarDateAdd(calendar, origin date, duration, constrain)`.
    fn add_date(&self, years: i64, months: i64, weeks: i64, days: i64) -> Option<CivilDate> {
        plain_date::calendar_add_date(self.calendar, self.date, years, months, weeks, days, false)
    }

    /// `GetEpochNanosecondsFor(timeZone, date + origin's time, compatible)`,
    /// rejecting an instant outside Temporal's representable range.
    fn resolve(&self, date: CivilDate) -> Option<BigInt> {
        let resolved = self
            .zone
            .epoch_nanoseconds_for(date, self.time, Disambiguation::Compatible)
            // Named zones come from the pinned database, whose offset
            // changes are isolated by more than the two-day probe and gap
            // shift window. Fixed offsets have exactly one candidate.
            // Compatible disambiguation therefore resolves every valid
            // civil date/time here; the resulting instant can still exceed
            // Temporal's range and must be rejected below.
            .expect("compatible disambiguation resolves a valid civil date/time in the pinned zone database");
        epoch::is_in_instant_range(&resolved).then_some(resolved)
    }
}

/// `GetISODateTimeFor(timeZone, epochNanoseconds)`.
fn local_date_time(zone: &TimeZone, epoch_nanoseconds: &BigInt) -> (CivilDate, CivilTime) {
    let offset = zone.offset_nanoseconds_for(epoch_nanoseconds);
    epoch::instant_fields(&(epoch_nanoseconds + BigInt::from(offset)))
}

fn date_unit(unit: TemporalUnit) -> DateUnit {
    match unit {
        TemporalUnit::Year => DateUnit::Year,
        TemporalUnit::Month => DateUnit::Month,
        TemporalUnit::Week => DateUnit::Week,
        _ => DateUnit::Day,
    }
}

fn time_unit(unit: TemporalUnit) -> TimeUnit {
    match unit {
        TemporalUnit::Hour => TimeUnit::Hour,
        TemporalUnit::Minute => TimeUnit::Minute,
        TemporalUnit::Second => TimeUnit::Second,
        TemporalUnit::Millisecond => TimeUnit::Millisecond,
        TemporalUnit::Microsecond => TimeUnit::Microsecond,
        _ => TimeUnit::Nanosecond,
    }
}

fn nanoseconds_between(from: &BigInt, to: &BigInt) -> Option<i128> {
    i128::try_from(to - from).ok()
}

/// `DifferenceZonedDateTimeWithRounding(ns1, ns2, timeZone, calendar,
/// largestUnit, roundingIncrement, smallestUnit, roundingMode)`, with `ns1`
/// the origin and `ns2` = `destination_nanoseconds`.
///
/// A `largest_unit` finer than `day` is a plain instant difference: no zone or
/// calendar is consulted at all.
pub(crate) fn difference_with_rounding(
    origin: &ZonedOrigin,
    destination_nanoseconds: &BigInt,
    largest_unit: TemporalUnit,
    increment: i128,
    smallest_unit: TemporalUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<InternalDuration> {
    if largest_unit < TemporalUnit::Day {
        let difference = nanoseconds_between(origin.epoch_nanoseconds, destination_nanoseconds)?;
        let rounded = TimeDuration::from_nanoseconds(difference).round(
            time_unit(smallest_unit),
            increment,
            mode,
        );
        return Some(InternalDuration {
            time_nanoseconds: rounded.total_nanoseconds(),
            ..InternalDuration::ZERO
        });
    }
    let difference = difference_zoned(origin, destination_nanoseconds, largest_unit)?;
    if smallest_unit == TemporalUnit::Nanosecond && increment == 1 {
        return Some(difference);
    }
    round_relative_duration(
        origin,
        destination_nanoseconds,
        difference,
        largest_unit,
        increment,
        smallest_unit,
        mode,
    )
}

/// `DifferenceZonedDateTimeWithTotal(ns1, ns2, timeZone, calendar, unit)`,
/// as the exact fraction `(numerator, denominator)` (`denominator > 0`) the
/// caller converts to a Number once.
pub(crate) fn difference_with_total(
    origin: &ZonedOrigin,
    destination_nanoseconds: &BigInt,
    unit: TemporalUnit,
) -> Option<(i128, i128)> {
    if unit < TemporalUnit::Day {
        let difference = nanoseconds_between(origin.epoch_nanoseconds, destination_nanoseconds)?;
        let length = unit
            .nanoseconds()
            .expect("every time unit has an exact length");
        return Some((difference, length));
    }
    let difference = difference_zoned(origin, destination_nanoseconds, unit)?;
    let nudge = nudge_to_calendar_unit(
        origin,
        destination_nanoseconds,
        &difference,
        1,
        date_unit(unit),
        blueice_ecma402::NumberRoundingMode::Trunc,
    )?;
    Some(nudge.total)
}

/// `DifferenceZonedDateTime(ns1, ns2, timeZone, calendar, largestUnit)`.
fn difference_zoned(
    origin: &ZonedOrigin,
    destination_nanoseconds: &BigInt,
    largest_unit: TemporalUnit,
) -> Option<InternalDuration> {
    let (destination_date, destination_time) =
        local_date_time(origin.zone, destination_nanoseconds);
    let (years, months, weeks, days, remainder) = zoned_date_time::difference_zoned_date_time(
        origin.zone,
        origin.calendar,
        origin.epoch_nanoseconds,
        origin.date,
        origin.time,
        destination_nanoseconds,
        destination_date,
        destination_time,
        date_unit(largest_unit),
    )?;
    Some(InternalDuration {
        years,
        months,
        weeks,
        days,
        time_nanoseconds: remainder,
    })
}

/// `RoundRelativeDuration` for a zoned origin.
fn round_relative_duration(
    origin: &ZonedOrigin,
    destination_nanoseconds: &BigInt,
    duration: InternalDuration,
    largest_unit: TemporalUnit,
    increment: i128,
    smallest_unit: TemporalUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<InternalDuration> {
    let sign = duration.direction();
    // A day is only a fixed length without a zone, so with one `day` is as
    // irregular a unit as `week`/`month`/`year`.
    let (nudged, expanded, nudged_nanoseconds) = if smallest_unit >= TemporalUnit::Day {
        let nudge = nudge_to_calendar_unit(
            origin,
            destination_nanoseconds,
            &duration,
            increment,
            date_unit(smallest_unit),
            mode,
        )?;
        (nudge.duration, nudge.expanded, nudge.epoch_nanoseconds)
    } else {
        nudge_to_zoned_time(origin, &duration, increment, time_unit(smallest_unit), mode)?
    };
    // Rounding up to the next unit can itself complete a still-larger one
    // (11 months rounded up to 12 is a year), up to what `largestUnit` allows.
    if expanded && smallest_unit != TemporalUnit::Week {
        let start_unit = smallest_unit.max(TemporalUnit::Day);
        return bubble_relative_duration(
            origin,
            sign,
            nudged,
            &nudged_nanoseconds,
            largest_unit,
            start_unit,
        );
    }
    Some(nudged)
}

/// `ComputeNudgeWindow`'s result: the two bracketing calendar-unit counts
/// (`r1`, then one `increment` further in the duration's direction), and the
/// instants and date durations they land on.
struct NudgeWindow {
    r1: i64,
    start_nanoseconds: BigInt,
    end_nanoseconds: BigInt,
    start_duration: InternalDuration,
    end_duration: InternalDuration,
}

/// `ComputeNudgeWindow(sign, duration, originEpochNs, isoDateTime, timeZone,
/// calendar, increment, unit, additionalShift)`.
fn compute_nudge_window(
    origin: &ZonedOrigin,
    duration: &InternalDuration,
    sign: i64,
    increment: i128,
    unit: DateUnit,
    additional_shift: bool,
) -> Option<NudgeWindow> {
    let truncate = |value: i64| -> i64 {
        rounding::round_to_increment(
            i128::from(value),
            increment,
            blueice_ecma402::NumberRoundingMode::Trunc,
        ) as i64
    };
    let increment_i64 = i64::try_from(increment).ok()?;
    if increment_i64 <= 0 {
        return None;
    }
    // The direction is +1 or -1, so a positive increment fits either way.
    let step = increment_i64 * sign;
    // `r1` is the truncated count, moved one increment further when the
    // caller found the destination outside the first window.
    let bracket = |truncated: i64| -> Option<(i64, i64)> {
        let r1 = if additional_shift {
            truncated.checked_add(step)?
        } else {
            truncated
        };
        Some((r1, r1.checked_add(step)?))
    };
    let (r1, start_duration, end_duration) = match unit {
        DateUnit::Year => {
            let (r1, r2) = bracket(truncate(duration.years))?;
            (
                r1,
                InternalDuration::from_date(r1, 0, 0, 0),
                InternalDuration::from_date(r2, 0, 0, 0),
            )
        }
        DateUnit::Month => {
            let (r1, r2) = bracket(truncate(duration.months))?;
            (
                r1,
                InternalDuration::from_date(duration.years, r1, 0, 0),
                InternalDuration::from_date(duration.years, r2, 0, 0),
            )
        }
        DateUnit::Week => {
            // The weeks already in `duration` plus however many whole weeks
            // its `days` add up to when counted from the years/months landing.
            let weeks_start = origin.add_date(duration.years, duration.months, 0, 0)?;
            let weeks_end = plain_date::add_iso_date(weeks_start, 0, 0, 0, duration.days, false)
                .expect(
                "adding only days leaves the validated year and month unchanged before balancing",
            );
            let (_, _, until_weeks, _) = plain_date::calendar_difference_date(
                origin.calendar,
                weeks_start,
                weeks_end,
                DateUnit::Week,
            );
            let (r1, r2) = bracket(truncate(duration.weeks.checked_add(until_weeks)?))?;
            (
                r1,
                InternalDuration::from_date(duration.years, duration.months, r1, 0),
                InternalDuration::from_date(duration.years, duration.months, r2, 0),
            )
        }
        DateUnit::Day => {
            let (r1, r2) = bracket(truncate(duration.days))?;
            (
                r1,
                InternalDuration::from_date(duration.years, duration.months, duration.weeks, r1),
                InternalDuration::from_date(duration.years, duration.months, duration.weeks, r2),
            )
        }
    };
    let resolve = |bound: &InternalDuration| -> Option<BigInt> {
        let date = origin.add_date(bound.years, bound.months, bound.weeks, bound.days)?;
        origin.resolve(date)
    };
    let is_blank = start_duration == InternalDuration::ZERO;
    let start_nanoseconds = if is_blank {
        origin.epoch_nanoseconds.clone()
    } else {
        resolve(&start_duration)?
    };
    let end_nanoseconds = resolve(&end_duration)?;
    Some(NudgeWindow {
        r1,
        start_nanoseconds,
        end_nanoseconds,
        start_duration,
        end_duration,
    })
}

/// [`nudge_to_calendar_unit`]'s outcome.
pub(crate) struct CalendarNudge {
    /// The rounded date part (the time part is always zero).
    pub(crate) duration: InternalDuration,
    /// The instant `duration` lands on.
    pub(crate) epoch_nanoseconds: BigInt,
    /// Whether rounding chose the window's far end (or the window had to be
    /// shifted to contain the destination), which is what allows bubbling.
    pub(crate) expanded: bool,
    /// The exact, unrounded `total` in `unit`s as a `(numerator, denominator)`
    /// fraction, `denominator > 0`: `(r1 × den + progress × sign) / den`.
    pub(crate) total: (i128, i128),
}

/// `NudgeToCalendarUnit(sign, duration, originEpochNs, destEpochNs,
/// isoDateTime, timeZone, calendar, increment, unit, roundingMode)`: rounds
/// `duration` to the nearest multiple of `increment` `unit`s, measuring how
/// far the destination is between the two bracketing calendar-date candidates
/// in **exact elapsed nanoseconds through the zone** — the only measure that
/// is right when those days are 23 or 25 hours long.
fn nudge_to_calendar_unit(
    origin: &ZonedOrigin,
    destination_nanoseconds: &BigInt,
    duration: &InternalDuration,
    increment: i128,
    unit: DateUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<CalendarNudge> {
    let sign = duration.direction();
    let mut window = compute_nudge_window(origin, duration, sign, increment, unit, false)?;
    let mut expanded = false;
    let (start_point, end_point) = if sign > 0 {
        (&window.start_nanoseconds, &window.end_nanoseconds)
    } else {
        (&window.end_nanoseconds, &window.start_nanoseconds)
    };
    if !(start_point <= destination_nanoseconds && destination_nanoseconds <= end_point) {
        // The destination lies past the first window (a DST-shortened day made
        // the date part land early): the answer is in the next one.
        window = compute_nudge_window(origin, duration, sign, increment, unit, true)?;
        expanded = true;
    }
    let mut numerator = nanoseconds_between(&window.start_nanoseconds, destination_nanoseconds)?;
    let mut denominator = nanoseconds_between(&window.start_nanoseconds, &window.end_nanoseconds)
        .expect("two representable Temporal instants have a difference within i128");
    if denominator < 0 {
        numerator = -numerator;
        denominator = -denominator;
    }
    // Representable bracket endpoints and a common-sign date duration bound
    // the count and nanosecond distances by a 201,000,000-day upper bound.
    // The numerator product therefore fits within i128.
    let total = (
        i128::from(window.r1) * denominator + numerator * i128::from(sign),
        denominator,
    );
    // `ApplyUnsignedRoundingMode` over the exact position inside the window --
    // the same decision `PlainDateTime` rounding makes (`plain_date_time_difference`).
    let rounded_up = plain_date_time_difference::rounds_up(
        numerator,
        denominator,
        window.r1,
        increment,
        sign,
        mode,
    );
    let (duration, epoch_nanoseconds) = if rounded_up {
        (window.end_duration, window.end_nanoseconds)
    } else {
        (window.start_duration, window.start_nanoseconds)
    };
    Some(CalendarNudge {
        duration,
        epoch_nanoseconds,
        expanded: expanded || rounded_up,
        total,
    })
}

/// `NudgeToZonedTime(sign, duration, isoDateTime, timeZone, calendar,
/// increment, unit, roundingMode)`: rounds the time part of `duration` to a
/// multiple of `increment` `unit`s. If that reaches the end of the *specific*
/// day the date part lands on — whose real length (23, 24 or 25 hours) comes
/// from the zone — the day carries into `days` and the excess is rounded
/// again from the start of the next day.
///
/// The second rounding is load-bearing, not cosmetic: 13 hours rounded up to
/// the next 12-hour increment relative to a 23-hour day is `1 day 12 hours`,
/// not `1 day 1 hour`.
///
/// Returns the rounded duration, whether the day carried, and the instant the
/// rounded duration lands on.
fn nudge_to_zoned_time(
    origin: &ZonedOrigin,
    duration: &InternalDuration,
    increment: i128,
    unit: TimeUnit,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<(InternalDuration, bool, BigInt)> {
    let sign = duration.direction();
    let start = origin.add_date(
        duration.years,
        duration.months,
        duration.weeks,
        duration.days,
    )?;
    let end = plain_date::add_iso_date(start, 0, 0, 0, sign, false)
        .expect("adding one day to a validated year and month can always be balanced");
    if !epoch::is_date_time_within_limits(end, origin.time) {
        return None;
    }
    let start_nanoseconds = origin.resolve(start)?;
    let end_nanoseconds = origin.resolve(end)?;
    let day_span = nanoseconds_between(&start_nanoseconds, &end_nanoseconds)
        .expect("two representable Temporal instants have a difference within i128");
    let rounded = TimeDuration::from_nanoseconds(duration.time_nanoseconds)
        .round(unit, increment, mode)
        .total_nanoseconds();
    let beyond_day_span = rounded - day_span;
    let (carried, day_delta, time_nanoseconds, nudged_nanoseconds) =
        if beyond_day_span.signum() != -i128::from(sign) {
            let excess = TimeDuration::from_nanoseconds(beyond_day_span)
                .round(unit, increment, mode)
                .total_nanoseconds();
            (true, sign, excess, &end_nanoseconds + BigInt::from(excess))
        } else {
            (
                false,
                0,
                rounded,
                &start_nanoseconds + BigInt::from(rounded),
            )
        };
    Some((
        InternalDuration {
            // Temporal date-duration fields are bounded far below i64's
            // endpoints; a single carried day cannot overflow that domain.
            days: duration
                .days
                .checked_add(day_delta)
                .expect("a Temporal date duration and one carried day fit in i64"),
            time_nanoseconds,
            ..*duration
        },
        carried,
        nudged_nanoseconds,
    ))
}

/// `BubbleRelativeDuration(sign, duration, nudgedEpochNs, isoDateTime,
/// timeZone, calendar, largestUnit, smallestUnit)`: after a nudge rounded up,
/// checks whether that completes each successively coarser unit up to
/// `largest_unit` (rounding 11 months up to 12 makes a year), taking `start_unit`
/// as the unit the rounding happened at. A `week` count is only ever a
/// bubbling target when `largest_unit` is itself `week`.
fn bubble_relative_duration(
    origin: &ZonedOrigin,
    sign: i64,
    nudged: InternalDuration,
    nudged_nanoseconds: &BigInt,
    largest_unit: TemporalUnit,
    start_unit: TemporalUnit,
) -> Option<InternalDuration> {
    // (`TemporalUnit` orders finest first, so "coarser" is greater.)
    if start_unit >= largest_unit {
        return Some(nudged);
    }
    let mut duration = nudged;
    let mut unit = start_unit;
    while unit < largest_unit {
        unit = match unit {
            TemporalUnit::Day => TemporalUnit::Week,
            TemporalUnit::Week => TemporalUnit::Month,
            _ => TemporalUnit::Year,
        };
        if unit == TemporalUnit::Week && largest_unit != TemporalUnit::Week {
            continue;
        }
        let end_duration = match unit {
            TemporalUnit::Year => InternalDuration::from_date(duration.years + sign, 0, 0, 0),
            TemporalUnit::Month => {
                InternalDuration::from_date(duration.years, duration.months + sign, 0, 0)
            }
            _ => InternalDuration::from_date(
                duration.years,
                duration.months,
                duration.weeks + sign,
                0,
            ),
        };
        let end_date = origin.add_date(
            end_duration.years,
            end_duration.months,
            end_duration.weeks,
            end_duration.days,
        )?;
        let end_nanoseconds = origin.resolve(end_date)?;
        // `nudged_nanoseconds` may itself lie outside the representable range.
        let beyond_end = nudged_nanoseconds - &end_nanoseconds;
        let beyond_end_sign = match beyond_end.sign() {
            num_bigint::Sign::Minus => -1,
            num_bigint::Sign::NoSign => 0,
            num_bigint::Sign::Plus => 1,
        };
        if beyond_end_sign == -sign {
            break;
        }
        duration = end_duration;
    }
    Some(duration)
}

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/zoned_difference_internal.rs"]
mod tests;
