// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calendar-agnostic duration time-unit math.
//!
//! Mirrors SpiderMonkey's own split (`TemporalTypes.h`, read as this
//! project's porting reference): `TimeDuration` (hours..nanoseconds) is a
//! separate combinator from `DateDuration` (years/months/weeks/days),
//! because only the latter needs calendar-aware balancing against a
//! `relativeTo`. This module implements `TimeDuration` — used directly by
//! `Temporal.Instant` (calendar-agnostic). `Temporal.PlainTime`/
//! `Temporal.Duration`'s own arithmetic will extend this module's public
//! surface via TDD from their own call sites, rather than speculatively
//! ahead of them; see the source-modularity note in
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`.

use super::rounding::{self, TemporalUnit, TimeUnit};

/// A calendar-agnostic span of time, held as exact total nanoseconds.
///
/// `i128` comfortably covers this: each Duration Record field is bounded to
/// magnitude 2^53 at the ECMA-402 host boundary
/// (`blueice_ecma402::DurationRecord`), and even the largest field (hours)
/// converted to nanoseconds stays several orders of magnitude below `i128`'s
/// range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TimeDuration {
    total_nanoseconds: i128,
}

impl TimeDuration {
    /// Wraps an already-computed exact nanosecond total.
    pub(crate) fn from_nanoseconds(total_nanoseconds: i128) -> Self {
        Self { total_nanoseconds }
    }

    /// Combines already-integral hour/minute/second/millisecond/
    /// microsecond/nanosecond fields (as `blueice_ecma402::DurationRecord`
    /// stores them) into their exact total.
    pub(crate) fn from_fields(
        hours: i128,
        minutes: i128,
        seconds: i128,
        milliseconds: i128,
        microseconds: i128,
        nanoseconds: i128,
    ) -> Self {
        let total_nanoseconds = hours * 3_600_000_000_000
            + minutes * 60_000_000_000
            + seconds * 1_000_000_000
            + milliseconds * 1_000_000
            + microseconds * 1_000
            + nanoseconds;
        Self { total_nanoseconds }
    }

    /// The exact total, in nanoseconds.
    pub(crate) fn total_nanoseconds(self) -> i128 {
        self.total_nanoseconds
    }

    /// `-self`.
    pub(crate) fn negated(self) -> Self {
        Self {
            total_nanoseconds: -self.total_nanoseconds,
        }
    }

    /// Rounds the exact total to the nearest multiple of
    /// `increment * smallest_unit.nanoseconds()`, per `mode`.
    pub(crate) fn round(
        self,
        smallest_unit: TimeUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Self {
        let step = smallest_unit.nanoseconds() * increment;
        Self {
            total_nanoseconds: rounding::round_to_increment(self.total_nanoseconds, step, mode),
        }
    }

    /// Balances the exact total into `(hour, minute, second, millisecond,
    /// microsecond, nanosecond)` fields, with everything above `largest`
    /// folded — unbounded — into `largest`'s own field, and every unit at or
    /// below `largest` a properly bounded remainder. This is what
    /// `Instant.prototype.until`/`since`'s `largestUnit` option needs: e.g.
    /// with `largest = Second`, the hour/minute contribution is folded into
    /// a (potentially large) `second` value rather than appearing as
    /// separate `hour`/`minute` fields.
    pub(crate) fn balance_to(self, largest: TimeUnit) -> [i64; 6] {
        const UNITS: [TimeUnit; 6] = [
            TimeUnit::Hour,
            TimeUnit::Minute,
            TimeUnit::Second,
            TimeUnit::Millisecond,
            TimeUnit::Microsecond,
            TimeUnit::Nanosecond,
        ];
        let sign = self.total_nanoseconds.signum();
        let mut remaining = self.total_nanoseconds.unsigned_abs();
        let mut fields = [0_i64; 6];
        let mut folding = true;
        for (index, unit) in UNITS.into_iter().enumerate() {
            if folding && unit != largest {
                continue;
            }
            folding = false;
            let unit_ns = unit.nanoseconds().unsigned_abs();
            fields[index] = (remaining / unit_ns) as i64;
            remaining %= unit_ns;
        }
        for field in &mut fields {
            *field *= sign as i64;
        }
        fields
    }

    /// `ToInternalDurationRecordWith24HourDays`'s time component: the whole
    /// record collapsed into one exact nanosecond total, counting each `days`
    /// field as 86,400 seconds.
    ///
    /// Only valid when the record's `years`/`months`/`weeks` are all zero —
    /// those units have no fixed length, so a caller must reject them (or
    /// resolve them against a calendar) before reaching here. `days` is safe
    /// because Temporal fixes a day at 24 hours except relative to a
    /// `Temporal.ZonedDateTime`.
    pub(crate) fn from_record_with_24_hour_days(record: &blueice_ecma402::DurationRecord) -> Self {
        debug_assert!(record.years == 0 && record.months == 0 && record.weeks == 0);
        Self::from_nanoseconds(
            record.days * 86_400_000_000_000
                + Self::from_fields(
                    record.hours,
                    record.minutes,
                    record.seconds,
                    record.milliseconds,
                    record.microseconds,
                    record.nanoseconds,
                )
                .total_nanoseconds(),
        )
    }

    /// Rounds the exact total to the nearest multiple of `step` nanoseconds,
    /// per `mode`. The unit-plus-increment form is [`TimeDuration::round`];
    /// this takes the already-multiplied step so a caller can round to a unit
    /// [`TimeUnit`] does not name (`day`).
    pub(crate) fn rounded_to_step(
        self,
        step: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Self {
        Self {
            total_nanoseconds: rounding::round_to_increment(self.total_nanoseconds, step, mode),
        }
    }

    /// `TemporalDurationFromInternal`'s field decomposition, for a `largest` of
    /// `day` or smaller: `[days, hours, minutes, seconds, milliseconds,
    /// microseconds, nanoseconds]`, with everything above `largest` folded —
    /// unbounded — into `largest`'s own field.
    ///
    /// This is [`TimeDuration::balance_to`] extended with the `days` field
    /// `Temporal.Duration` needs (`Temporal.Instant` has no `days` to balance
    /// into) and widened to `i128`, because folding a whole duration into
    /// `nanoseconds` can exceed `i64`.
    pub(crate) fn balance_with_days(self, largest: TemporalUnit) -> [i128; 7] {
        const UNITS: [TemporalUnit; 7] = [
            TemporalUnit::Day,
            TemporalUnit::Hour,
            TemporalUnit::Minute,
            TemporalUnit::Second,
            TemporalUnit::Millisecond,
            TemporalUnit::Microsecond,
            TemporalUnit::Nanosecond,
        ];
        debug_assert!(largest <= TemporalUnit::Day);
        let sign = self.total_nanoseconds.signum();
        let mut remaining = self.total_nanoseconds.unsigned_abs();
        let mut fields = [0_i128; 7];
        let mut folding = true;
        for (index, unit) in UNITS.into_iter().enumerate() {
            if folding && unit != largest {
                continue;
            }
            folding = false;
            let unit_nanoseconds = unit
                .nanoseconds()
                .expect("day and every smaller unit has an exact length")
                .unsigned_abs();
            fields[index] = (remaining / unit_nanoseconds) as i128;
            remaining %= unit_nanoseconds;
        }
        for field in &mut fields {
            *field *= sign;
        }
        fields
    }

    /// `TotalTimeDuration`: the exact total expressed in `unit`, rounded once
    /// to the nearest Number. `unit` must have an exact length.
    pub(crate) fn total_in(self, unit: TemporalUnit) -> f64 {
        rounding::exact_ratio_to_f64(
            self.total_nanoseconds,
            unit.nanoseconds()
                .expect("a calendar unit has no exact length to total in"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ecma402::NumberRoundingMode as Mode;

    #[test]
    fn combines_fields_into_an_exact_total() {
        let duration = TimeDuration::from_fields(1, 2, 3, 4, 5, 6);
        assert_eq!(
            duration.total_nanoseconds(),
            3_600_000_000_000 + 120_000_000_000 + 3_000_000_000 + 4_000_000 + 5_000 + 6
        );
    }

    #[test]
    fn negation_is_exact() {
        let duration = TimeDuration::from_fields(1, 0, 0, 0, 0, 0);
        assert_eq!(duration.negated().total_nanoseconds(), -3_600_000_000_000);
        assert_eq!(duration.negated().negated(), duration);
    }

    #[test]
    fn rounds_to_an_hour_increment() {
        // 1976-11-18T14:23:30.123456789Z rounded to the nearest 4 hours,
        // matching Test262's rounding-increments.js first case
        // (hour, increment 4 -> 16:00).
        let duration = TimeDuration::from_fields(14, 23, 30, 123, 456, 789);
        let rounded = duration.round(TimeUnit::Hour, 4, Mode::HalfExpand);
        assert_eq!(rounded.total_nanoseconds(), 16 * 3_600_000_000_000);
    }

    #[test]
    fn rounds_to_a_minute_increment() {
        let duration = TimeDuration::from_fields(14, 23, 30, 123, 456, 789);
        let rounded = duration.round(TimeUnit::Minute, 15, Mode::HalfExpand);
        assert_eq!(
            rounded.total_nanoseconds(),
            14 * 3_600_000_000_000 + 30 * 60_000_000_000
        );
    }

    #[test]
    fn balance_to_hour_bounds_every_field_below_it() {
        let duration = TimeDuration::from_fields(26, 3, 4, 5, 6, 7);
        assert_eq!(duration.balance_to(TimeUnit::Hour), [26, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn balance_to_second_folds_hours_and_minutes_into_seconds() {
        // 1h1m = 3,660s, folded entirely into the seconds field.
        let duration = TimeDuration::from_fields(1, 1, 0, 0, 0, 0);
        assert_eq!(
            duration.balance_to(TimeUnit::Second),
            [0, 0, 3_660, 0, 0, 0]
        );
    }

    #[test]
    fn balance_to_preserves_sign() {
        let duration = TimeDuration::from_fields(-26, -3, -4, -5, -6, -7);
        assert_eq!(
            duration.balance_to(TimeUnit::Hour),
            [-26, -3, -4, -5, -6, -7]
        );
    }

    fn record(
        days: i128,
        hours: i128,
        minutes: i128,
        seconds: i128,
        milliseconds: i128,
        microseconds: i128,
        nanoseconds: i128,
    ) -> blueice_ecma402::DurationRecord {
        blueice_ecma402::DurationRecord::try_new(
            0,
            0,
            0,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        )
        .expect("test records are valid")
    }

    #[test]
    fn a_record_days_field_counts_as_twenty_four_hours() {
        assert_eq!(
            TimeDuration::from_record_with_24_hour_days(&record(1, 0, 0, 0, 0, 0, 0))
                .total_nanoseconds(),
            86_400_000_000_000
        );
        // Test262's add/balance-negative-result.js: -1 day plus -60 hours.
        assert_eq!(
            TimeDuration::from_record_with_24_hour_days(&record(-1, -60, 0, 0, 0, 0, 0))
                .balance_with_days(TemporalUnit::Day),
            [-3, -12, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn balance_with_days_folds_everything_above_the_largest_unit() {
        // Test262's add/basic.js "balancing positive": P50DT50H50M50.5005005S
        // doubled is 104 days, 5:41:41.001001000.
        let doubled = TimeDuration::from_record_with_24_hour_days(&record(
            100, 100, 100, 100, 1_000, 1_000, 1_000,
        ));
        assert_eq!(
            doubled.balance_with_days(TemporalUnit::Day),
            [104, 5, 41, 41, 1, 1, 0]
        );
        // Test262's round/balance-subseconds.js, largestUnit "seconds".
        let subseconds = TimeDuration::from_record_with_24_hour_days(&record(
            0,
            0,
            0,
            0,
            999,
            999_999,
            999_999_999,
        ));
        assert_eq!(
            subseconds.balance_with_days(TemporalUnit::Second),
            [0, 0, 0, 2, 998, 998, 999]
        );
        // Folding a whole day into nanoseconds overflows i64, which is why
        // this method is i128-wide where `balance_to` is not.
        assert_eq!(
            TimeDuration::from_record_with_24_hour_days(&record(1, 0, 0, 0, 0, 0, 0))
                .balance_with_days(TemporalUnit::Nanosecond),
            [0, 0, 0, 0, 0, 0, 86_400_000_000_000]
        );
    }

    #[test]
    fn rounding_to_a_day_step_matches_test262_expected_values() {
        // round/round-negative-result.js: -60 hours to the nearest day.
        let day = TemporalUnit::Day.nanoseconds().unwrap();
        assert_eq!(
            TimeDuration::from_record_with_24_hour_days(&record(0, -60, 0, 0, 0, 0, 0))
                .rounded_to_step(day, Mode::HalfExpand)
                .balance_with_days(TemporalUnit::Day),
            [-3, 0, 0, 0, 0, 0, 0]
        );
        // round/roundingincrement-days-large.js: 1 day, ceil, 1e8-1 days.
        assert_eq!(
            TimeDuration::from_record_with_24_hour_days(&record(1, 0, 0, 0, 0, 0, 0))
                .rounded_to_step(day * (100_000_000 - 1), Mode::Ceil)
                .balance_with_days(TemporalUnit::Day),
            [99_999_999, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn total_in_uses_the_exact_ratio() {
        let duration = TimeDuration::from_record_with_24_hour_days(&record(5, 5, 5, 5, 5, 5, 5));
        assert_eq!(
            duration.total_in(TemporalUnit::Nanosecond),
            450_305_005_005_005.0
        );
        assert_eq!(
            duration.total_in(TemporalUnit::Day),
            5.0 + 18_305_005.005_005 / 86_400_000.0
        );
        assert_eq!(
            TimeDuration::from_record_with_24_hour_days(&record(0, -1, 0, 0, 0, 0, 0))
                .total_in(TemporalUnit::Minute),
            -60.0
        );
    }
}
