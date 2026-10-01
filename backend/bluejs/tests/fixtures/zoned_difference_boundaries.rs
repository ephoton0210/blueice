// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calendar bracket and representable-instant rejection by the numeric core.

use super::*;

#[test]
fn zoned_brackets_reject_increment_overflow_and_unrepresentable_dates() {
    let zone = super::super::time_zone::parse_identifier("UTC").unwrap();
    let ns = epoch::nanoseconds_since_epoch((2020, 1, 1), (0, 0, 0, 0, 0, 0), 0);
    let origin = ZonedOrigin {
        zone: &zone,
        calendar: AnyCalendarKind::Iso,
        epoch_nanoseconds: &ns,
        date: (2020, 1, 1),
        time: (0, 0, 0, 0, 0, 0),
    };
    let duration = InternalDuration::from_date(1, 0, 0, 0);
    for (increment, shifted) in [(i128::MAX, false), (i128::from(i64::MAX), true)] {
        assert!(
            compute_nudge_window(&origin, &duration, 1, increment, DateUnit::Year, shifted)
                .is_none()
        );
    }
    for unit in [DateUnit::Year, DateUnit::Month, DateUnit::Day] {
        let extreme = InternalDuration::from_date(i64::MAX, i64::MAX, 0, i64::MAX);
        assert!(compute_nudge_window(&origin, &extreme, 1, 1, unit, false).is_none());
        assert!(compute_nudge_window(&origin, &extreme, 1, 1, unit, true).is_none());
    }
    assert!(origin.resolve((275760, 9, 14)).is_none());
}

#[test]
fn zoned_week_brackets_and_instant_differences_reject_unrepresentable_intermediates() {
    let zone = super::super::time_zone::parse_identifier("UTC").unwrap();
    let ns = BigInt::from(0);
    let origin = ZonedOrigin {
        zone: &zone,
        calendar: AnyCalendarKind::Iso,
        epoch_nanoseconds: &ns,
        date: (1970, 1, 1),
        time: (0, 0, 0, 0, 0, 0),
    };
    for duration in [
        InternalDuration::from_date(1_000_000, 0, 0, 1),
        InternalDuration::from_date(0, 0, 0, 400_000_000),
        InternalDuration::from_date(0, 0, i64::MAX, 7),
        InternalDuration::from_date(0, 0, 100_000_000, 1),
    ] {
        assert!(compute_nudge_window(&origin, &duration, 1, 1, DateUnit::Week, false).is_none());
    }
    let beyond_i128 = BigInt::from(i128::MAX) + 1;
    assert!(difference_with_total(&origin, &beyond_i128, TemporalUnit::Hour).is_none());
    assert!(difference_with_rounding(
        &origin,
        &beyond_i128,
        TemporalUnit::Hour,
        1,
        TemporalUnit::Hour,
        blueice_ecma402::NumberRoundingMode::Trunc
    )
    .is_none());
}
