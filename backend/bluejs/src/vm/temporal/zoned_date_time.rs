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

use super::epoch::{CivilDate, CivilTime};
use super::plain_date::{self, DateUnit};
use super::time_zone::{Disambiguation, TimeZone};
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

/// The unrounded calendar-date portion of `DifferenceZonedDateTime`: the
/// years/months/weeks/days between two zoned local dates at `largest_unit`
/// granularity (exactly [`plain_date::calendar_difference_date`]), plus the
/// *exact* nanosecond remainder once that whole date part is applied to
/// `date1`.
///
/// Deliberately does not derive the day count from elapsed nanoseconds (the
/// naive approach, and the one that would need an unbounded correction loop
/// for a large date range — the exact class of performance bug
/// `plain_date.rs`'s own rounding rewrite already found and fixed for
/// `PlainDate`, documented there). [`plain_date::calendar_difference_date`]'s
/// own `days` output is already defined as an exact ISO-epoch-day count from
/// its "years+months+weeks" landing date to `end`, so re-adding that same
/// count in epoch days always lands exactly on `date2` — no probing or
/// bisection needed: the time remainder is then just `ns2` minus the instant
/// of `date2` at the *start* time-of-day, resolved through the zone.
#[allow(clippy::too_many_arguments)]
pub(crate) fn difference_zoned_date_time(
    zone: &TimeZone,
    calendar: AnyCalendarKind,
    ns1: &BigInt,
    date1: CivilDate,
    time1: CivilTime,
    ns2: &BigInt,
    date2: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64, i128) {
    if ns1 == ns2 {
        return (0, 0, 0, 0, 0);
    }
    let (years, months, weeks, days) =
        plain_date::calendar_difference_date(calendar, date1, date2, largest_unit);
    // `Disambiguation::Compatible` always resolves (it is only ever `Err`
    // for `Reject`), so the fallback here is unreachable in practice; it
    // exists only so this function stays total rather than panicking.
    let end_ns = zone
        .epoch_nanoseconds_for(date2, time1, Disambiguation::Compatible)
        .unwrap_or_else(|_| ns2.clone());
    let time_remainder = ns2 - &end_ns;
    let time_remainder_i128 = i128::try_from(&time_remainder)
        .expect("a same-day-or-adjacent remainder around an Instant-range value fits in i128");
    (years, months, weeks, days, time_remainder_i128)
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
            .epoch_nanoseconds_for((2000, 4, 1), (12, 0, 0, 0, 0, 0), Disambiguation::Compatible)
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
            .epoch_nanoseconds_for((2000, 4, 1), (2, 30, 0, 0, 0, 0), Disambiguation::Compatible)
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
            .epoch_nanoseconds_for((2000, 4, 2), (3, 30, 0, 0, 0, 0), Disambiguation::Compatible)
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
            DateUnit::Day,
        );
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
                DateUnit::Day,
            ),
            (0, 0, 0, 0, 0)
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
