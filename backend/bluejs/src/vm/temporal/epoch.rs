// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral epoch-nanosecond arithmetic.
//!
//! Represented as [`num_bigint::BigInt`], not a fixed-width integer:
//! `crate::heap::TemporalValue::epoch_nanoseconds` (the JS-visible
//! `Temporal.Instant`/`ZonedDateTime` epoch field) is already `BigInt`
//! throughout this engine, backing BlueJS's native JS `BigInt` support, and
//! is read with ordinary `BigInt` division (e.g. for `epochMilliseconds`).
//! Phase 26's plan originally proposed `i128` here on the theory that Rust's
//! native 128-bit integer is simpler than Gecko's bespoke 96-bit `Int96` —
//! true in isolation, but `BigInt` is what the rest of this engine already
//! uses at this exact boundary, so keeping it avoids constant conversion
//! friction rather than removing any.

use num_bigint::BigInt;

/// `(year, month, day)`.
pub(crate) type CivilDate = (i32, u8, u8);
/// `(hour, minute, second, millisecond, microsecond, nanosecond)`.
pub(crate) type CivilTime = (u8, u8, u8, u16, u16, u16);

/// Converts a calendar date, time-of-day and UTC offset into epoch
/// nanoseconds, using a Howard Hinnant-style `days_from_civil` calculation.
pub(crate) fn nanoseconds_since_epoch(
    (year, month, day): CivilDate,
    (hour, minute, second, millisecond, microsecond, nanosecond): CivilTime,
    offset_seconds: i32,
) -> BigInt {
    let adjusted_year = i64::from(year) - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let march_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * march_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let milliseconds = days * 86_400_000
        + i64::from(hour) * 3_600_000
        + i64::from(minute) * 60_000
        + i64::from(second) * 1_000
        + i64::from(millisecond);
    BigInt::from(milliseconds) * 1_000_000_u32
        + BigInt::from(microsecond) * 1_000_u32
        + BigInt::from(nanosecond)
        - BigInt::from(offset_seconds) * 1_000_000_000_u32
}

/// Returns whether `value` is within Temporal's supported Instant range:
/// ±10^8 days (≈ ±273,972.6 years) around the epoch, in nanoseconds.
pub(crate) fn is_in_instant_range(value: &BigInt) -> bool {
    let limit = BigInt::from(8_640_000_000_000_000_i64) * 1_000_000_u32;
    value >= &-limit.clone() && value <= &limit
}

/// The inverse of [`nanoseconds_since_epoch`]'s date/time steps: decomposes
/// an epoch-nanosecond value (UTC) into `(year, month, day)` and
/// `(hour, minute, second, millisecond, microsecond, nanosecond)`, via the
/// Howard Hinnant `civil_from_days` algorithm. Callers within this crate
/// hold values already checked by [`is_in_instant_range`], whose ±10^8-day
/// span fits comfortably in `i64` throughout.
pub(crate) fn instant_fields(value: &BigInt) -> (CivilDate, CivilTime) {
    let day_ns = BigInt::from(86_400_000_000_000_i64);
    let mut days = value / &day_ns;
    let mut ns_of_day = value % &day_ns;
    if ns_of_day < BigInt::from(0) {
        ns_of_day += &day_ns;
        days -= 1;
    }
    let days: i64 = days
        .try_into()
        .expect("Instant-range values keep their day count within i64");
    let ns_of_day: i64 = ns_of_day
        .try_into()
        .expect("a single day's nanoseconds fit in i64");
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u8;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u8;
    let year = if month <= 2 { year + 1 } else { year } as i32;

    let hour = (ns_of_day / 3_600_000_000_000) as u8;
    let minute = ((ns_of_day / 60_000_000_000) % 60) as u8;
    let second = ((ns_of_day / 1_000_000_000) % 60) as u8;
    let millisecond = ((ns_of_day / 1_000_000) % 1_000) as u16;
    let microsecond = ((ns_of_day / 1_000) % 1_000) as u16;
    let nanosecond = (ns_of_day % 1_000) as u16;
    (
        (year, month, day),
        (hour, minute, second, millisecond, microsecond, nanosecond),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_the_unix_epoch_to_zero_nanoseconds() {
        assert_eq!(
            nanoseconds_since_epoch((1970, 1, 1), (0, 0, 0, 0, 0, 0), 0),
            BigInt::from(0)
        );
    }

    #[test]
    fn applies_the_utc_offset_and_sub_second_fields() {
        let value = nanoseconds_since_epoch((1970, 1, 1), (1, 0, 0, 0, 0, 1), 3_600);
        assert_eq!(value, BigInt::from(1));
    }

    #[test]
    fn enforces_the_symmetric_instant_range() {
        let limit = BigInt::from(8_640_000_000_000_000_i64) * 1_000_000_u32;
        assert!(is_in_instant_range(&limit));
        assert!(is_in_instant_range(&-limit.clone()));
        assert!(!is_in_instant_range(&(limit + 1)));
    }

    #[test]
    fn decomposes_a_known_instant_matching_a_pinned_test262_fixture() {
        // 217178610_123_456_789n == 1976-11-18T15:23:30.123456789Z, from
        // Test262's built-ins/Temporal/Instant/prototype/since/roundingmode-ceil.js.
        let value = BigInt::from(217_178_610_123_456_789_i64);
        assert_eq!(
            instant_fields(&value),
            ((1976, 11, 18), (15, 23, 30, 123, 456, 789))
        );
    }

    #[test]
    fn decomposes_the_unix_epoch() {
        assert_eq!(
            instant_fields(&BigInt::from(0)),
            ((1970, 1, 1), (0, 0, 0, 0, 0, 0))
        );
    }

    #[test]
    fn decomposes_an_instant_before_the_epoch() {
        // One nanosecond before the epoch is the last nanosecond of 1969.
        assert_eq!(
            instant_fields(&BigInt::from(-1)),
            ((1969, 12, 31), (23, 59, 59, 999, 999, 999))
        );
    }

    #[test]
    fn round_trips_through_nanoseconds_since_epoch_across_a_range_of_dates() {
        for (year, month, day, hour, minute, second, ms, us, ns) in [
            (1970, 1, 1, 0, 0, 0, 0, 0, 0),
            (2000, 2, 29, 12, 0, 0, 0, 0, 0),
            (1969, 12, 31, 23, 59, 59, 999, 999, 999),
            (2286, 11, 20, 17, 46, 39, 0, 0, 0),
            (1677, 9, 21, 0, 12, 43, 145, 224, 192),
        ] {
            let epoch =
                nanoseconds_since_epoch((year, month, day), (hour, minute, second, ms, us, ns), 0);
            assert_eq!(
                instant_fields(&epoch),
                ((year, month, day), (hour, minute, second, ms, us, ns)),
                "round trip for {year}-{month}-{day}",
            );
        }
    }
}
