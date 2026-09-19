// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Temporal.ZonedDateTime` arithmetic (Phase 26 Stage 2, third
//! and final slice,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `ZonedDateTime` composes `PlainDateTime` + `TimeZone` + `Instant`: a
//! calendar day here is not always 86,400 seconds (a named zone's day can be
//! 23 or 25 hours across a DST transition), which is this type's entire
//! reason to exist over `Instant`. The algorithm shapes below are ported
//! from Gecko's `AddZonedDateTime`/`DifferenceZonedDateTime`/
//! `RoundZonedDateTimeInstant`
//! (`development/browser_core/reference/gecko/js/src/builtin/temporal/ZonedDateTime.cpp`).
//!
//! No `Value`/heap/Realm coupling, matching every other module in this
//! directory — directly unit-testable without a VM.

use super::duration_math;
use super::epoch::{self, CivilDate, CivilTime};
use super::plain_date::{self, DateUnit};
use super::rounding;
use super::time_zone::{AmbiguousLocalTime, Disambiguation, TimeZone};
use icu_calendar::AnyCalendarKind;
use num_bigint::BigInt;

/// `AddZonedDateTime`: adds a duration (calendar years/months/weeks/days,
/// plus an exact time-nanosecond remainder) to an instant in `zone`.
///
/// The date portion is carried through the calendar first, anchored at the
/// zone's local wall-clock date/time for `epoch_ns` (`local_date`/
/// `local_time` — already known to every caller from the value's own stored
/// local fields), then re-resolved to an instant via the zone
/// (`"compatible"` disambiguation, matching `AddZonedDateTime`'s own fixed
/// choice); only then does the *exact* time duration apply, as plain
/// nanosecond addition to that resolved instant. When there is no date
/// component at all, this degenerates to `AddInstant` — pure nanosecond
/// arithmetic, with no zone or calendar consulted at all, which matters near
/// a DST transition: adding `PT1H` must always mean exactly one hour of
/// elapsed time, never "the same wall-clock hour later".
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_zoned_date_time(
    zone: &TimeZone,
    calendar: AnyCalendarKind,
    epoch_ns: &BigInt,
    local_date: CivilDate,
    local_time: CivilTime,
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    time_nanoseconds: i128,
    reject: bool,
) -> Option<BigInt> {
    if years == 0 && months == 0 && weeks == 0 && days == 0 {
        return Some(epoch_ns + BigInt::from(time_nanoseconds));
    }
    let added_date =
        plain_date::calendar_add_date(calendar, local_date, years, months, weeks, days, reject)?;
    let intermediate_ns = zone
        .epoch_nanoseconds_for(added_date, local_time, Disambiguation::Compatible)
        .ok()?;
    Some(intermediate_ns + BigInt::from(time_nanoseconds))
}

/// `DifferenceZonedDateTime`: the years/months/weeks/days between two zoned
/// local dates at `largest_unit` granularity (exactly
/// [`plain_date::calendar_difference_date`]), plus the *exact* nanosecond
/// remainder once that whole date part is applied to `date1`.
///
/// The naive version of this (used by an earlier slice of this same pass) —
/// take `calendar_difference_date(date1, date2)` as-is, and the remainder as
/// `ns2` minus the instant of `date2` at `date1`'s own time-of-day — is
/// correct whenever that remainder's sign already agrees with the date part
/// (the common case), but *not* in general: `date1`'s time-of-day is often
/// later in the day than `date2`'s actual local time (from `ns2`), which
/// makes that naive remainder land on the *wrong* side of zero relative to
/// the overall direction, producing a `years`/`months`/`weeks`/`days` and a
/// time remainder with **opposite** signs — the exact
/// `DurationRecord::try_new` "common sign" `RangeError`
/// `since/negative-epochnanoseconds.js`,
/// `since/reversibility-of-differences.js` and the `argument-at-limits.js`/
/// `intercalary-month-{coptic,ethiopic,ethioaa}.js` fixtures all hit before
/// this fix.
///
/// Ported directly from Gecko's own `DifferenceZonedDateTime`
/// (`ZonedDateTime.cpp`): finds the correct anchor date by *day-correcting*
/// `date2` (by 0, 1, or up to 2 days for a positive overall direction —
/// `maxDayCorrection`'s own `1 + (sign > 0)`, since a positive difference can
/// need to cross two short/DST-shortened local days to find a consistent
/// candidate) until resolving `(candidate, time1)` through the zone produces
/// a remainder whose sign actually agrees with the overall direction, then
/// computes the calendar date difference from `date1` to *that* candidate
/// (not to `date2` directly) — still exactly
/// [`plain_date::calendar_difference_date`], no bisection or bounded-loop
/// day-count derivation needed, since the loop only ever runs 1-3 iterations
/// regardless of how far apart `date1`/`date2` are.
///
/// `None` only on a genuine representable-range overflow while resolving a
/// candidate instant (`argument-at-limits.js`-style fixtures near the ends of
/// the `Instant` range), or if every day-correction candidate is exhausted
/// without finding a consistent sign (Gecko's own
/// `JSMSG_TEMPORAL_ZONED_DATE_TIME_INCONSISTENT_INSTANT`, not expected to be
/// reachable in practice for a real IANA zone but kept total rather than
/// panicking); the caller maps it to the spec's own `RangeError`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn difference_zoned_date_time(
    zone: &TimeZone,
    calendar: AnyCalendarKind,
    ns1: &BigInt,
    date1: CivilDate,
    time1: CivilTime,
    ns2: &BigInt,
    date2: CivilDate,
    time2: CivilTime,
    largest_unit: DateUnit,
) -> Option<(i64, i64, i64, i64, i128)> {
    if ns1 == ns2 {
        return Some((0, 0, 0, 0, 0));
    }
    if date1 == date2 {
        let diff = i128::try_from(ns2 - ns1)
            .expect("a same-day remainder around an Instant-range value fits in i128");
        return Some((0, 0, 0, 0, diff));
    }
    let sign: i64 = if (ns2 - ns1).sign() == num_bigint::Sign::Minus {
        -1
    } else {
        1
    };
    let max_day_correction: i64 = if sign > 0 { 2 } else { 1 };
    let mut day_correction: i64 = 0;

    let wall_time_diff = duration_math::time_fields_to_nanoseconds(
        time2.0, time2.1, time2.2, time2.3, time2.4, time2.5,
    ) - duration_math::time_fields_to_nanoseconds(
        time1.0, time1.1, time1.2, time1.3, time1.4, time1.5,
    );
    if (wall_time_diff.signum() as i64) == -sign {
        day_correction += 1;
    }

    loop {
        if day_correction > max_day_correction {
            return None;
        }
        let candidate = plain_date::add_iso_date(date2, 0, 0, 0, -day_correction * sign, false)?;
        let candidate_ns = zone
            .epoch_nanoseconds_for(candidate, time1, Disambiguation::Compatible)
            .ok()?;
        if !epoch::is_in_instant_range(&candidate_ns) {
            return None;
        }
        let time_duration = i128::try_from(ns2 - &candidate_ns).expect(
            "a bounded-day-correction remainder around an Instant-range value fits in i128",
        );
        let time_sign = time_duration.signum() as i64;
        if sign != -time_sign {
            let (years, months, weeks, days) =
                plain_date::calendar_difference_date(calendar, date1, candidate, largest_unit);
            return Some((years, months, weeks, days, time_duration));
        }
        day_correction += 1;
    }
}

/// The exact elapsed length, in nanoseconds, of the wall-clock day
/// containing `date` in `zone` — 86,400e9 on an ordinary day, but 82,800e9
/// (23h) or 90,000e9 (25h) across a DST transition. `GetStartOfDay`'s own
/// definition of a day's boundary, not a fixed UTC-day assumption —
/// `ZonedDateTime`'s entire reason to have its own rounding/`hoursInDay`
/// behaviour distinct from `Instant`'s.
pub(crate) fn day_length_nanoseconds(zone: &TimeZone, date: CivilDate) -> i128 {
    let start = zone.start_of_day(date);
    let next = plain_date::add_iso_date(date, 0, 0, 0, 1, false)
        .expect("a representable date's next calendar day is also representable");
    let end = zone.start_of_day(next);
    i128::try_from(&end - &start).expect("one day's length fits in i128 many times over")
}

/// The rounded date-only outcome of [`nudge_to_calendar_unit`]: the picked
/// `years`/`months`/`weeks`/`days` candidate, the epoch instant it actually
/// resolves to (needed by [`bubble_relative_duration`]'s own boundary
/// probes), and whether rounding picked the larger of its two bracketing
/// candidates (`didExpandCalendarUnit` — whether bubbling can apply at all).
pub(crate) struct CalendarUnitNudge {
    pub(crate) years: i64,
    pub(crate) months: i64,
    pub(crate) weeks: i64,
    pub(crate) days: i64,
    pub(crate) epoch_nanoseconds: BigInt,
    pub(crate) expanded: bool,
}

/// `NudgeToCalendarUnit`: rounds an unrounded calendar-date duration
/// (`duration`, already decomposed at some `largest_unit` granularity by
/// [`plain_date::calendar_difference_date`]) to the nearest multiple of
/// `increment` `unit`s, per `mode`.
///
/// This is [`plain_date::round_month_or_year`]'s own anchor-relative
/// fractional-position algorithm, but measuring the fraction in **exact
/// nanoseconds through the zone** between the two bracketing calendar-date
/// candidates, rather than in epoch days — the day-length-aware version
/// `ZonedDateTime` needs and a plain (unzoned) date pair does not: since a
/// zoned day can be 23, 24 or 25 real hours, "halfway between these two
/// candidate months" is only well-defined once measured in real elapsed
/// time, not in a calendar-day count. Directly ported from Gecko's
/// `NudgeToCalendarUnit` (`Duration.cpp`); reuses
/// [`plain_date::calendar_add_date`]/[`plain_date::calendar_difference_date`]
/// exactly as already shipped — no changes to either.
///
/// `None` only on a genuine representable-range overflow while resolving one
/// of the two candidate instants (`argument-at-limits.js`-style fixtures
/// near the ends of the `Instant` range) — the caller maps it to the spec's
/// own `RangeError`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn nudge_to_calendar_unit(
    zone: &TimeZone,
    calendar: AnyCalendarKind,
    date1: CivilDate,
    time1: CivilTime,
    dest_epoch_ns: &BigInt,
    duration: (i64, i64, i64, i64),
    unit: DateUnit,
    increment: i128,
    sign: i64,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<CalendarUnitNudge> {
    let (years, months, weeks, days) = duration;
    let trunc = |value: i64| -> i64 {
        rounding::round_to_increment(
            i128::from(value),
            increment,
            blueice_ecma402::NumberRoundingMode::Trunc,
        ) as i64
    };
    let step = increment as i64 * sign;

    let (start_tuple, end_tuple, r1) = match unit {
        DateUnit::Year => {
            let r1 = trunc(years);
            ((r1, 0, 0, 0), (r1 + step, 0, 0, 0), r1)
        }
        DateUnit::Month => {
            let r1 = trunc(months);
            ((years, r1, 0, 0), (years, r1 + step, 0, 0), r1)
        }
        DateUnit::Week => {
            // Steps 3.b-3.e: brackets `duration`'s own `weeks`/`days` split
            // (from whatever `largest_unit` decomposed it at) back into a
            // single "how many whole weeks from `date1`" count, by measuring
            // the years+months-only landing date's plain ISO-day distance to
            // the full years+months+weeks+days landing date.
            let weeks_start =
                plain_date::calendar_add_date(calendar, date1, years, months, 0, 0, false)?;
            let weeks_end = plain_date::add_iso_date(weeks_start, 0, 0, 0, days, false)?;
            let (_, _, extra_weeks, _) = plain_date::calendar_difference_date(
                calendar,
                weeks_start,
                weeks_end,
                DateUnit::Week,
            );
            let r1 = trunc(weeks + extra_weeks);
            ((years, months, r1, 0), (years, months, r1 + step, 0), r1)
        }
        DateUnit::Day => {
            let r1 = trunc(days);
            (
                (years, months, weeks, r1),
                (years, months, weeks, r1 + step),
                r1,
            )
        }
    };

    let resolve = |(y, mo, w, d): (i64, i64, i64, i64)| -> Option<BigInt> {
        let date = plain_date::calendar_add_date(calendar, date1, y, mo, w, d, false)?;
        zone.epoch_nanoseconds_for(date, time1, Disambiguation::Compatible)
            .ok()
    };
    let start_ns = resolve(start_tuple)?;
    let end_ns = resolve(end_tuple)?;

    let mut numerator = i128::try_from(dest_epoch_ns - &start_ns)
        .expect("a same-bracket remainder around an Instant-range value fits in i128");
    let mut denominator = i128::try_from(&end_ns - &start_ns)
        .expect("a same-bracket span around an Instant-range value fits in i128");
    if denominator < 0 {
        numerator = -numerator;
        denominator = -denominator;
    }

    let expanded = nudge_expand_decision(numerator, denominator, r1, increment, sign, mode);
    let (final_tuple, final_ns) = if expanded {
        (end_tuple, end_ns)
    } else {
        (start_tuple, start_ns)
    };
    Some(CalendarUnitNudge {
        years: final_tuple.0,
        months: final_tuple.1,
        weeks: final_tuple.2,
        days: final_tuple.3,
        epoch_nanoseconds: final_ns,
        expanded,
    })
}

/// `ApplyUnsignedRoundingMode`, specialized to the exact
/// numerator/denominator fraction [`nudge_to_calendar_unit`] measures —
/// identical decision tree to [`plain_date::round_month_or_year`]'s own
/// (already Test262-verified) `round_up` match, just renamed to this
/// function's own `r1`/`increment` naming.
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
                (i128::from(r1.unsigned_abs()) / increment.max(1)) % 2 != 0
            } else {
                2 * numerator > denominator
            }
        }
    }
}

/// `BubbleRelativeDuration`: after [`nudge_to_calendar_unit`] expands to its
/// larger candidate, checks whether that expansion should keep bubbling up
/// into successively coarser units, up to (and including) `largest_unit` —
/// e.g. rounding 11 months up to 12 becomes 1 year, 0 months when
/// `largest_unit` is `"years"` (Test262's
/// `round-cross-unit-boundary.js`). A `"weeks"` count is only ever a
/// bubbling target when `largest_unit` itself is `"weeks"` — matching
/// Gecko's own `unit != Week || largestUnit == Week` guard, since a
/// standalone "weeks" component is never introduced unless the caller
/// actually asked for one.
///
/// `None` only on the same genuine representable-range overflow
/// [`nudge_to_calendar_unit`] can hit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bubble_relative_duration(
    zone: &TimeZone,
    calendar: AnyCalendarKind,
    date1: CivilDate,
    time1: CivilTime,
    nudge: &CalendarUnitNudge,
    largest_unit: DateUnit,
    smallest_unit: DateUnit,
    sign: i64,
) -> Option<(i64, i64, i64, i64)> {
    let (mut years, mut months, mut weeks, mut days) =
        (nudge.years, nudge.months, nudge.weeks, nudge.days);
    if smallest_unit == largest_unit {
        return Some((years, months, weeks, days));
    }
    let mut unit = smallest_unit;
    while date_unit_rank(unit) > date_unit_rank(largest_unit) {
        unit = one_coarser_date_unit(unit);
        if unit == DateUnit::Week && largest_unit != DateUnit::Week {
            continue;
        }
        let end_tuple = match unit {
            DateUnit::Year => (years + sign, 0, 0, 0),
            DateUnit::Month => (years, months + sign, 0, 0),
            DateUnit::Week => (years, months, weeks + sign, 0),
            DateUnit::Day => unreachable!("Day is never a bubbling target"),
        };
        let end = plain_date::calendar_add_date(
            calendar,
            date1,
            end_tuple.0,
            end_tuple.1,
            end_tuple.2,
            end_tuple.3,
            false,
        )?;
        let end_ns: Result<BigInt, AmbiguousLocalTime> =
            zone.epoch_nanoseconds_for(end, time1, Disambiguation::Compatible);
        let end_ns = end_ns.ok()?;
        let beyond_end = &nudge.epoch_nanoseconds - &end_ns;
        let beyond_end_sign = match beyond_end.sign() {
            num_bigint::Sign::Minus => -1_i64,
            num_bigint::Sign::NoSign => 0,
            num_bigint::Sign::Plus => 1,
        };
        if beyond_end_sign != -sign {
            years = end_tuple.0;
            months = end_tuple.1;
            weeks = end_tuple.2;
            days = 0;
        } else {
            break;
        }
    }
    Some((years, months, weeks, days))
}

/// Coarseness rank for bubbling purposes only (lower = coarser) — `DateUnit`
/// itself derives no `Ord` since [`plain_date::calendar_difference_date`]'s
/// own callers never need to compare it, but bubbling needs to walk from
/// `smallest_unit` up toward `largest_unit` one step at a time.
fn date_unit_rank(unit: DateUnit) -> u8 {
    match unit {
        DateUnit::Year => 0,
        DateUnit::Month => 1,
        DateUnit::Week => 2,
        DateUnit::Day => 3,
    }
}

fn one_coarser_date_unit(unit: DateUnit) -> DateUnit {
    match unit {
        DateUnit::Day => DateUnit::Week,
        DateUnit::Week => DateUnit::Month,
        DateUnit::Month => DateUnit::Year,
        DateUnit::Year => unreachable!("Year is the coarsest DateUnit"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() -> TimeZone {
        TimeZone::Offset(0)
    }

    #[test]
    fn add_with_no_date_component_is_pure_nanosecond_arithmetic() {
        let start = BigInt::from(1_000_000_000_i64);
        let result = add_zoned_date_time(
            &utc(),
            AnyCalendarKind::Iso,
            &start,
            (1970, 1, 1),
            (0, 0, 1, 0, 0, 0),
            0,
            0,
            0,
            0,
            2_000_000_000,
            false,
        );
        assert_eq!(result, Some(BigInt::from(3_000_000_000_i64)));
    }

    /// 2000-04-02 is `America/Los_Angeles`' spring-forward day (clocks moved
    /// from 02:00 PST directly to 03:00 PDT). Adding one calendar day to a
    /// noon receiver — well clear of the 02:00-03:00 gap on either side — is
    /// still real *elapsed* time, not a naive 24h: since the offset changes
    /// from -8h to -7h across the crossed transition, one calendar day here
    /// is only 23 real hours.
    #[test]
    fn add_one_day_across_a_spring_forward_transition_is_23_real_hours() {
        let zone = TimeZone::Iana("America/Los_Angeles");
        let start = zone
            .epoch_nanoseconds_for(
                (2000, 4, 1),
                (12, 0, 0, 0, 0, 0),
                Disambiguation::Compatible,
            )
            .unwrap();
        let result = add_zoned_date_time(
            &zone,
            AnyCalendarKind::Iso,
            &start,
            (2000, 4, 1),
            (12, 0, 0, 0, 0, 0),
            0,
            0,
            0,
            1,
            0,
            false,
        )
        .unwrap();
        assert_eq!(&result - &start, BigInt::from(23_i64 * 3_600_000_000_000));
    }

    /// A skipped local time (inside the gap itself) still resolves via
    /// `"compatible"` disambiguation rather than failing the whole
    /// operation — `AddZonedDateTime`'s own fixed choice, matching
    /// `Temporal.PlainDateTime.prototype.toZonedDateTime`'s default.
    #[test]
    fn add_lands_inside_a_gap_and_resolves_via_compatible_disambiguation() {
        let zone = TimeZone::Iana("America/Los_Angeles");
        let start = zone
            .epoch_nanoseconds_for(
                (2000, 4, 1),
                (2, 30, 0, 0, 0, 0),
                Disambiguation::Compatible,
            )
            .unwrap();
        let result = add_zoned_date_time(
            &zone,
            AnyCalendarKind::Iso,
            &start,
            (2000, 4, 1),
            (2, 30, 0, 0, 0, 0),
            0,
            0,
            0,
            1,
            0,
            false,
        )
        .unwrap();
        // 2000-04-02T02:30 does not exist; "compatible" (== "later" for a
        // spring-forward gap) resolves it to 03:30 PDT.
        let expected = zone
            .epoch_nanoseconds_for(
                (2000, 4, 2),
                (3, 30, 0, 0, 0, 0),
                Disambiguation::Compatible,
            )
            .unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn difference_across_a_spring_forward_day_reports_one_calendar_day_and_no_time_remainder() {
        let zone = TimeZone::Iana("America/Los_Angeles");
        let start_date = (2000, 4, 1);
        let start_time = (12, 0, 0, 0, 0, 0);
        let ns1 = zone
            .epoch_nanoseconds_for(start_date, start_time, Disambiguation::Compatible)
            .unwrap();
        let end_date = (2000, 4, 2);
        let ns2 = zone
            .epoch_nanoseconds_for(end_date, start_time, Disambiguation::Compatible)
            .unwrap();
        let (years, months, weeks, days, remainder_ns) = difference_zoned_date_time(
            &zone,
            AnyCalendarKind::Iso,
            &ns1,
            start_date,
            start_time,
            &ns2,
            end_date,
            start_time,
            DateUnit::Day,
        )
        .unwrap();
        assert_eq!((years, months, weeks, days), (0, 0, 0, 1));
        assert_eq!(remainder_ns, 0);
        // The real elapsed time is 23h, not the naive 24h a fixed-day
        // assumption would report.
        assert_eq!(&ns2 - &ns1, BigInt::from(23_i64 * 3_600_000_000_000));
    }

    #[test]
    fn difference_of_identical_instants_is_exactly_zero() {
        let zone = utc();
        let ns = BigInt::from(1_000_000_000_i64);
        let date = (1970, 1, 1);
        let time = (0, 0, 1, 0, 0, 0);
        assert_eq!(
            difference_zoned_date_time(
                &zone,
                AnyCalendarKind::Iso,
                &ns,
                date,
                time,
                &ns,
                date,
                time,
                DateUnit::Day,
            ),
            Some((0, 0, 0, 0, 0))
        );
    }

    #[test]
    fn day_length_is_23_hours_on_a_spring_forward_day_and_24_on_an_ordinary_one() {
        let zone = TimeZone::Iana("America/Los_Angeles");
        assert_eq!(
            day_length_nanoseconds(&zone, (2000, 4, 2)),
            23 * 3_600_000_000_000
        );
        assert_eq!(
            day_length_nanoseconds(&zone, (2000, 1, 1)),
            24 * 3_600_000_000_000
        );
    }

    /// 2000-10-29 is `America/Los_Angeles`' fall-back day (the last Sunday
    /// of October, pre-2007 US DST rule).
    #[test]
    fn day_length_is_25_hours_on_a_fall_back_day() {
        let zone = TimeZone::Iana("America/Los_Angeles");
        assert_eq!(
            day_length_nanoseconds(&zone, (2000, 10, 29)),
            25 * 3_600_000_000_000
        );
    }

    #[test]
    fn day_length_is_always_24_hours_in_a_fixed_offset_or_utc_zone() {
        assert_eq!(
            day_length_nanoseconds(&utc(), (2024, 6, 1)),
            24 * 3_600_000_000_000
        );
        assert_eq!(
            day_length_nanoseconds(&TimeZone::Offset(-300), (2024, 6, 1)),
            24 * 3_600_000_000_000
        );
    }
}
