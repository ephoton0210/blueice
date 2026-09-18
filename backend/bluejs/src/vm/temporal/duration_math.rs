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

use super::rounding::{self, TimeUnit};

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
}
