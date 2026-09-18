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

/// Converts a calendar date, time-of-day and UTC offset into epoch
/// nanoseconds, using a Howard Hinnant-style `days_from_civil` calculation.
pub(crate) fn nanoseconds_since_epoch(
    (year, month, day): (i32, u8, u8),
    (hour, minute, second, millisecond, microsecond, nanosecond): (u8, u8, u8, u16, u16, u16),
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
}
