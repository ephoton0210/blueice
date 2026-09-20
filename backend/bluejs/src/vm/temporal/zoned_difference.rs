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
use super::rounding::{self, TemporalUnit, TimeUnit};
use super::time_zone::{Disambiguation, TimeZone};
use super::zoned_date_time;
use icu_calendar::AnyCalendarKind;
use num_bigint::BigInt;

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
            .ok()?;
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
    let step = i64::try_from(increment).ok()?.checked_mul(sign)?;
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
            let weeks_end = plain_date::add_iso_date(weeks_start, 0, 0, 0, duration.days, false)?;
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
    let mut denominator = nanoseconds_between(&window.start_nanoseconds, &window.end_nanoseconds)?;
    if denominator < 0 {
        numerator = -numerator;
        denominator = -denominator;
    }
    let total = (
        i128::from(window.r1)
            .checked_mul(denominator)?
            .checked_add(numerator.checked_mul(i128::from(sign))?)?,
        denominator,
    );
    let rounded_up = numerator == denominator
        || nudge_expand_decision(numerator, denominator, window.r1, increment, sign, mode);
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

/// `ApplyUnsignedRoundingMode` over the exact `numerator / denominator`
/// position (`0 <= numerator <= denominator`, both non-negative) inside a
/// window whose lower candidate is `r1`: whether the value rounds to the
/// window's far end. `sign` is the duration's direction; `r1 / increment`
/// supplies `halfEven`'s cardinality (whether the lower candidate's own
/// multiple is even).
fn nudge_expand_decision(
    numerator: i128,
    denominator: i128,
    r1: i64,
    increment: i128,
    sign: i64,
    mode: blueice_ecma402::NumberRoundingMode,
) -> bool {
    use blueice_ecma402::NumberRoundingMode as Mode;
    if denominator == 0 || numerator == 0 {
        return false;
    }
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
                (i128::from(r1) / increment) % 2 != 0
            } else {
                2 * numerator > denominator
            }
        }
    }
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
    let end = plain_date::add_iso_date(start, 0, 0, 0, sign, false)?;
    if !epoch::is_date_time_within_limits(end, origin.time) {
        return None;
    }
    let start_nanoseconds = origin.resolve(start)?;
    let end_nanoseconds = origin.resolve(end)?;
    let day_span = nanoseconds_between(&start_nanoseconds, &end_nanoseconds)?;
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
            days: duration.days.checked_add(day_delta)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ecma402::NumberRoundingMode as Mode;

    const HOUR: i128 = 3_600_000_000_000;
    const DAY: i128 = 24 * HOUR;

    fn zone(identifier: &str) -> TimeZone {
        super::super::time_zone::parse_identifier(identifier).expect("a valid zone identifier")
    }

    /// Runs `difference_with_rounding` between two `(date, time)` wall-clock
    /// instants in `zone`, compatible disambiguation.
    fn difference(
        zone: &TimeZone,
        from: (CivilDate, CivilTime),
        to: (CivilDate, CivilTime),
        largest: TemporalUnit,
        increment: i128,
        smallest: TemporalUnit,
        mode: Mode,
    ) -> Option<InternalDuration> {
        let from_ns = zone
            .epoch_nanoseconds_for(from.0, from.1, Disambiguation::Compatible)
            .unwrap();
        let to_ns = zone
            .epoch_nanoseconds_for(to.0, to.1, Disambiguation::Compatible)
            .unwrap();
        let origin = ZonedOrigin {
            zone,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &from_ns,
            date: from.0,
            time: from.1,
        };
        difference_with_rounding(&origin, &to_ns, largest, increment, smallest, mode)
    }

    fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> (CivilDate, CivilTime) {
        ((year, month, day), (hour, minute, 0, 0, 0, 0))
    }

    #[test]
    fn direction_is_that_of_the_first_non_zero_field_and_positive_when_blank() {
        assert_eq!(InternalDuration::ZERO.direction(), 1);
        let negative_time = InternalDuration {
            time_nanoseconds: -1,
            ..InternalDuration::ZERO
        };
        assert_eq!(negative_time.direction(), -1);
        // A date field decides before the time part is looked at.
        let mixed = InternalDuration {
            days: -1,
            ..InternalDuration::from_date(0, 0, 0, 0)
        };
        assert_eq!(mixed.direction(), -1);
    }

    #[test]
    fn a_time_largest_unit_is_a_plain_instant_difference() {
        let utc = zone("UTC");
        let result = difference(
            &utc,
            at(2020, 1, 1, 0, 0),
            at(2020, 1, 3, 12, 30),
            TemporalUnit::Hour,
            1,
            TemporalUnit::Hour,
            Mode::HalfExpand,
        )
        .unwrap();
        // 60.5 hours, halfExpand -> 61.
        assert_eq!(result.time_nanoseconds, 61 * HOUR);
        assert_eq!(
            (result.years, result.months, result.weeks, result.days),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn rounding_hours_up_to_the_day_length_carries_one_more_day() {
        // `intl402/.../until/dst-rounding-result.js`: 2 days 23:59 rounds to 24
        // hours, which is the whole day.
        let offset = zone("-08:00");
        let result = difference(
            &offset,
            at(2020, 1, 1, 0, 0),
            at(2020, 1, 3, 23, 59),
            TemporalUnit::Day,
            1,
            TemporalUnit::Hour,
            Mode::HalfExpand,
        )
        .unwrap();
        assert_eq!((result.days, result.time_nanoseconds), (3, 0));
    }

    #[test]
    fn the_carry_uses_the_real_length_of_a_short_dst_day() {
        // Vancouver springs forward on 2000-04-02: that day is 23 hours long, so
        // 23:36 on the wall clock is only 22h36m of elapsed time, which rounds
        // (to the hour) to the whole day.
        let vancouver = zone("America/Vancouver");
        let result = difference(
            &vancouver,
            at(2000, 4, 2, 0, 0),
            at(2000, 4, 2, 23, 36),
            TemporalUnit::Day,
            1,
            TemporalUnit::Hour,
            Mode::HalfExpand,
        )
        .unwrap();
        assert_eq!((result.days, result.time_nanoseconds), (1, 0));
    }

    #[test]
    fn the_excess_is_rounded_again_to_the_same_increment() {
        // `adjust-rounded-duration-days.js`: 13 hours ceil'd to a 12-hour
        // increment is 24, which overshoots a 23-hour day by an hour; that
        // excess is itself ceil'd to 12 hours.
        let new_york = zone("America/New_York");
        let origin_ns = new_york
            .epoch_nanoseconds_for(
                (2024, 3, 10),
                (0, 0, 0, 0, 0, 0),
                Disambiguation::Compatible,
            )
            .unwrap();
        let origin = ZonedOrigin {
            zone: &new_york,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &origin_ns,
            date: (2024, 3, 10),
            time: (0, 0, 0, 0, 0, 0),
        };
        let duration = InternalDuration {
            time_nanoseconds: 13 * HOUR,
            ..InternalDuration::ZERO
        };
        let (nudged, carried, _) =
            nudge_to_zoned_time(&origin, &duration, 12, TimeUnit::Hour, Mode::Ceil).unwrap();
        assert!(carried);
        assert_eq!((nudged.days, nudged.time_nanoseconds), (1, 12 * HOUR));
    }

    #[test]
    fn a_remainder_that_stays_inside_its_day_does_not_carry() {
        let utc = zone("UTC");
        let origin_ns = BigInt::from(0);
        let origin = ZonedOrigin {
            zone: &utc,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &origin_ns,
            date: (1970, 1, 1),
            time: (0, 0, 0, 0, 0, 0),
        };
        let duration = InternalDuration {
            time_nanoseconds: 5 * HOUR + 20 * 60 * 1_000_000_000,
            ..InternalDuration::ZERO
        };
        let (nudged, carried, landing) =
            nudge_to_zoned_time(&origin, &duration, 1, TimeUnit::Hour, Mode::Trunc).unwrap();
        assert!(!carried);
        assert_eq!((nudged.days, nudged.time_nanoseconds), (0, 5 * HOUR));
        assert_eq!(landing, BigInt::from(5 * HOUR));
    }

    #[test]
    fn expanding_a_sub_day_remainder_bubbles_up_to_largest_unit() {
        // `round-cross-unit-boundary.js`: two years less one nanosecond, rounded
        // up to microseconds, is exactly two years.
        let utc = zone("UTC");
        let result = difference(
            &utc,
            at(1970, 1, 1, 0, 0),
            ((1971, 12, 31), (23, 59, 59, 999, 999, 999)),
            TemporalUnit::Year,
            1,
            TemporalUnit::Microsecond,
            Mode::Expand,
        )
        .unwrap();
        assert_eq!(result, InternalDuration::from_date(2, 0, 0, 0));
    }

    #[test]
    fn bubbling_stops_at_the_largest_unit() {
        // 1 year 11 months rounded up to 2 years only with `largestUnit` year.
        let utc = zone("UTC");
        let from = at(2022, 1, 1, 0, 0);
        let to = at(2023, 12, 25, 0, 0);
        let years = difference(
            &utc,
            from,
            to,
            TemporalUnit::Year,
            1,
            TemporalUnit::Month,
            Mode::Expand,
        )
        .unwrap();
        assert_eq!(years, InternalDuration::from_date(2, 0, 0, 0));
        let months = difference(
            &utc,
            from,
            to,
            TemporalUnit::Month,
            1,
            TemporalUnit::Month,
            Mode::Expand,
        )
        .unwrap();
        assert_eq!(months, InternalDuration::from_date(0, 24, 0, 0));
    }

    #[test]
    fn a_weeks_smallest_unit_never_bubbles() {
        let utc = zone("UTC");
        // 3 weeks 6 days rounded up to weeks, largest year: 4 weeks, not a month.
        let result = difference(
            &utc,
            at(2021, 3, 1, 0, 0),
            at(2021, 3, 28, 0, 0),
            TemporalUnit::Year,
            1,
            TemporalUnit::Week,
            Mode::Expand,
        )
        .unwrap();
        assert_eq!(result, InternalDuration::from_date(0, 0, 4, 0));
    }

    #[test]
    fn blank_duration_at_the_end_of_the_range_still_needs_the_next_day() {
        // `NudgeToZonedTime` step 4: even a zero duration must be able to
        // resolve the following day's start.
        let utc = zone("UTC");
        let last = (275_760, 9, 13);
        let origin_ns = utc
            .epoch_nanoseconds_for(last, (0, 0, 0, 0, 0, 0), Disambiguation::Compatible)
            .unwrap();
        let origin = ZonedOrigin {
            zone: &utc,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &origin_ns,
            date: last,
            time: (0, 0, 0, 0, 0, 0),
        };
        assert!(nudge_to_zoned_time(
            &origin,
            &InternalDuration::ZERO,
            1,
            TimeUnit::Hour,
            Mode::Trunc
        )
        .is_none());
    }

    #[test]
    fn total_measures_progress_through_the_bracketing_calendar_unit() {
        let utc = zone("UTC");
        let from_ns = utc
            .epoch_nanoseconds_for((2019, 1, 1), (0, 0, 0, 0, 0, 0), Disambiguation::Compatible)
            .unwrap();
        let to_ns = utc
            .epoch_nanoseconds_for((2020, 7, 2), (0, 0, 0, 0, 0, 0), Disambiguation::Compatible)
            .unwrap();
        let origin = ZonedOrigin {
            zone: &utc,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &from_ns,
            date: (2019, 1, 1),
            time: (0, 0, 0, 0, 0, 0),
        };
        // 2020 is a leap year: Jan 1 -> Jul 2 is 183 of its 366 days, so exactly 1.5.
        let (numerator, denominator) =
            difference_with_total(&origin, &to_ns, TemporalUnit::Year).unwrap();
        assert_eq!(rounding::exact_ratio_to_f64(numerator, denominator), 1.5);
        // A time unit is a plain instant ratio.
        let (numerator, denominator) =
            difference_with_total(&origin, &to_ns, TemporalUnit::Hour).unwrap();
        assert_eq!(
            (numerator, denominator),
            (nanoseconds_between(&from_ns, &to_ns).unwrap(), HOUR)
        );
    }

    #[test]
    fn total_in_days_uses_the_real_day_length() {
        // 25 real hours starting at 2000-04-01T02:30 Vancouver (the next day has
        // a 23-hour length): 24/23 days.
        let vancouver = zone("America/Vancouver");
        let from_ns = vancouver
            .epoch_nanoseconds_for(
                (2000, 4, 1),
                (2, 30, 0, 0, 0, 0),
                Disambiguation::Compatible,
            )
            .unwrap();
        let to_ns = &from_ns + BigInt::from(25 * HOUR);
        let origin = ZonedOrigin {
            zone: &vancouver,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &from_ns,
            date: (2000, 4, 1),
            time: (2, 30, 0, 0, 0, 0),
        };
        let (numerator, denominator) =
            difference_with_total(&origin, &to_ns, TemporalUnit::Day).unwrap();
        assert_eq!(
            rounding::exact_ratio_to_f64(numerator, denominator),
            24.0 / 23.0
        );
    }

    #[test]
    fn a_rounding_window_whose_end_is_unrepresentable_is_rejected() {
        // `roundingincrement-addition-out-of-range.js`.
        let utc = zone("UTC");
        let from_ns = BigInt::from(0);
        let to_ns = BigInt::from(5);
        let origin = ZonedOrigin {
            zone: &utc,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &from_ns,
            date: (1970, 1, 1),
            time: (0, 0, 0, 0, 0, 0),
        };
        let largest = 100_000_000;
        assert!(difference_with_rounding(
            &origin,
            &to_ns,
            TemporalUnit::Day,
            largest + 1,
            TemporalUnit::Day,
            Mode::Trunc
        )
        .is_none());
        assert_eq!(
            difference_with_rounding(
                &origin,
                &to_ns,
                TemporalUnit::Day,
                largest,
                TemporalUnit::Day,
                Mode::Expand
            ),
            Some(InternalDuration::from_date(0, 0, 0, 100_000_000))
        );
    }

    #[test]
    fn a_destination_past_the_first_window_selects_the_shifted_one() {
        // The window is chosen from `duration`'s own `days`; if the destination
        // is later than where that lands, the next window applies and the result
        // counts as expanded.
        let utc = zone("UTC");
        let from_ns = BigInt::from(0);
        let to_ns = BigInt::from(3 * DAY);
        let origin = ZonedOrigin {
            zone: &utc,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &from_ns,
            date: (1970, 1, 1),
            time: (0, 0, 0, 0, 0, 0),
        };
        let stale = InternalDuration::from_date(0, 0, 0, 1);
        let nudge =
            nudge_to_calendar_unit(&origin, &to_ns, &stale, 1, DateUnit::Day, Mode::Trunc).unwrap();
        assert!(nudge.expanded);
        assert_eq!(nudge.duration, InternalDuration::from_date(0, 0, 0, 3));
    }

    #[test]
    fn day_rounding_measures_progress_through_the_real_day_length() {
        // `smallestUnit: "days"` with a zone is `NudgeToCalendarUnit`. Vancouver
        // skips 02:00-03:00 on 2000-04-02, so 12:30 on the wall clock is 11.5
        // elapsed hours into a 23-hour day: exactly half, which `halfExpand`
        // rounds up and `halfTrunc` down.
        let vancouver = zone("America/Vancouver");
        let result = difference(
            &vancouver,
            at(2000, 4, 2, 0, 0),
            at(2000, 4, 2, 12, 30),
            TemporalUnit::Day,
            1,
            TemporalUnit::Day,
            Mode::HalfExpand,
        )
        .unwrap();
        assert_eq!(result, InternalDuration::from_date(0, 0, 0, 1));
        let result = difference(
            &vancouver,
            at(2000, 4, 2, 0, 0),
            at(2000, 4, 2, 12, 30),
            TemporalUnit::Day,
            1,
            TemporalUnit::Day,
            Mode::HalfTrunc,
        )
        .unwrap();
        assert_eq!(result, InternalDuration::ZERO);
    }

    #[test]
    fn a_negative_difference_rounds_and_bubbles_in_its_own_direction() {
        // The mirror of `bubbling_stops_at_the_largest_unit`: measured from the
        // later date back to the earlier one everything is negative, and
        // `expand` still rounds away from zero into a full two years.
        let utc = zone("UTC");
        let result = difference(
            &utc,
            at(2023, 12, 25, 0, 0),
            at(2022, 1, 1, 0, 0),
            TemporalUnit::Year,
            1,
            TemporalUnit::Month,
            Mode::Expand,
        )
        .unwrap();
        assert_eq!(result, InternalDuration::from_date(-2, 0, 0, 0));
    }

    #[test]
    fn a_week_largest_unit_is_a_bubbling_target_for_days() {
        // Six and a half days rounded up (`expand`) to whole days is seven, which
        // completes a week -- but only because `largestUnit` is `week`.
        let utc = zone("UTC");
        let from = at(2021, 3, 1, 0, 0);
        let to = at(2021, 3, 7, 12, 0);
        let weeks = difference(
            &utc,
            from,
            to,
            TemporalUnit::Week,
            1,
            TemporalUnit::Day,
            Mode::Expand,
        )
        .unwrap();
        assert_eq!(weeks, InternalDuration::from_date(0, 0, 1, 0));
        let days = difference(
            &utc,
            from,
            to,
            TemporalUnit::Day,
            1,
            TemporalUnit::Day,
            Mode::Expand,
        )
        .unwrap();
        assert_eq!(days, InternalDuration::from_date(0, 0, 0, 7));
    }

    #[test]
    fn a_negative_time_remainder_carries_a_negative_day() {
        // `NudgeToZonedTime` with `sign == -1`: the day span is measured to the
        // *previous* day's start, and the carry is `-1`.
        let utc = zone("UTC");
        let origin_ns = BigInt::from(5 * DAY);
        let origin = ZonedOrigin {
            zone: &utc,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &origin_ns,
            date: (1970, 1, 6),
            time: (0, 0, 0, 0, 0, 0),
        };
        let duration = InternalDuration {
            time_nanoseconds: -(23 * HOUR + 40 * 60 * 1_000_000_000),
            ..InternalDuration::ZERO
        };
        let (nudged, carried, landing) =
            nudge_to_zoned_time(&origin, &duration, 1, TimeUnit::Hour, Mode::HalfExpand).unwrap();
        assert!(carried);
        assert_eq!((nudged.days, nudged.time_nanoseconds), (-1, 0));
        assert_eq!(landing, BigInt::from(4 * DAY));
    }

    #[test]
    fn into_fields_balances_time_no_higher_than_the_largest_unit() {
        let duration = InternalDuration {
            years: 1,
            months: 2,
            weeks: 3,
            days: 4,
            time_nanoseconds: 26 * HOUR + 3 * 60 * 1_000_000_000 + 4_005_006_007,
        };
        // A date-sized largest unit keeps hours as the top *time* field.
        assert_eq!(
            duration.into_fields(TemporalUnit::Year),
            [1, 2, 3, 4, 26, 3, 4, 5, 6, 7]
        );
        assert_eq!(
            duration.into_fields(TemporalUnit::Minute),
            [1, 2, 3, 4, 0, 26 * 60 + 3, 4, 5, 6, 7]
        );
        assert_eq!(
            duration.into_fields(TemporalUnit::Nanosecond)[4..],
            [
                0,
                0,
                0,
                0,
                0,
                26 * 3_600_000_000_000 + 3 * 60_000_000_000 + 4_005_006_007
            ]
        );
    }

    #[test]
    fn expansion_decisions_follow_every_rounding_mode() {
        // (numerator, denominator) positions inside a window whose lower
        // candidate is `r1`: below half, exactly half, above half.
        let (below, half, above) = ((1, 4), (2, 4), (3, 4));
        let decide = |position: (i128, i128), r1: i64, sign: i64, mode: Mode| {
            nudge_expand_decision(position.0, position.1, r1, 1, sign, mode)
        };
        // Directed modes ignore the position (once it is not exactly zero).
        assert!(decide(below, 0, 1, Mode::Ceil) && !decide(below, 0, -1, Mode::Ceil));
        assert!(!decide(below, 0, 1, Mode::Floor) && decide(below, 0, -1, Mode::Floor));
        assert!(decide(below, 0, 1, Mode::Expand) && decide(below, 0, -1, Mode::Expand));
        assert!(!decide(above, 0, 1, Mode::Trunc) && !decide(above, 0, -1, Mode::Trunc));
        // Half modes agree off the exact half...
        for mode in [
            Mode::HalfCeil,
            Mode::HalfFloor,
            Mode::HalfExpand,
            Mode::HalfTrunc,
            Mode::HalfEven,
        ] {
            assert!(
                !decide(below, 0, 1, mode) && decide(above, 0, 1, mode),
                "{mode:?}"
            );
        }
        // ...and differ exactly on it, depending on direction.
        assert!(decide(half, 0, 1, Mode::HalfCeil) && !decide(half, 0, -1, Mode::HalfCeil));
        assert!(!decide(half, 0, 1, Mode::HalfFloor) && decide(half, 0, -1, Mode::HalfFloor));
        assert!(decide(half, 0, 1, Mode::HalfExpand) && decide(half, 0, -1, Mode::HalfExpand));
        assert!(!decide(half, 0, 1, Mode::HalfTrunc) && !decide(half, 0, -1, Mode::HalfTrunc));
        // `halfEven` rounds to an even multiple of the increment.
        assert!(!decide(half, 2, 1, Mode::HalfEven) && decide(half, 3, 1, Mode::HalfEven));
        assert!(!decide(half, -2, -1, Mode::HalfEven) && decide(half, -3, -1, Mode::HalfEven));
        // A position at either end of the window never expands on its own.
        assert!(!nudge_expand_decision(0, 4, 0, 1, 1, Mode::Expand));
        assert!(!nudge_expand_decision(1, 0, 0, 1, 1, Mode::Expand));
    }
}
