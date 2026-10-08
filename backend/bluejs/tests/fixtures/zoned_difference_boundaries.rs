// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calendar bracket and representable-instant rejection by the numeric core.

use super::*;

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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
        InternalDuration::from_date(i64::MAX / 2, 0, 0, 1),
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
    // Calendar callers reject an out-of-range destination at their zoned
    // difference, before rounding or computing a fractional calendar unit.
    let outside = epoch::nanoseconds_since_epoch((1_000_000, 1, 1), origin.time, 0);
    for unit in [
        TemporalUnit::Year,
        TemporalUnit::Month,
        TemporalUnit::Week,
        TemporalUnit::Day,
    ] {
        assert!(difference_with_total(&origin, &outside, unit).is_none());
        assert!(difference_with_rounding(
            &origin,
            &outside,
            unit,
            1,
            TemporalUnit::Nanosecond,
            blueice_ecma402::NumberRoundingMode::Trunc
        )
        .is_none());
    }
    // The whole-week addition fits, but its next rounding bracket does not.
    assert!(compute_nudge_window(
        &origin,
        &InternalDuration::from_date(0, 0, i64::MAX - 1, 7),
        1,
        1,
        DateUnit::Week,
        false
    )
    .is_none());
    // Bracket counts fit in i64 while their calendar year cannot fit in i32.
    assert!(compute_nudge_window(
        &origin,
        &InternalDuration::from_date(i64::MAX / 2, 0, 0, 0),
        1,
        1,
        DateUnit::Year,
        false
    )
    .is_none());
}

#[cfg_attr(test, test)]
fn shifted_zoned_windows_reject_an_unrepresentable_far_endpoint_and_huge_progress() {
    let zone = super::super::time_zone::parse_identifier("UTC").unwrap();
    let ns = epoch::nanoseconds_since_epoch((275_758, 1, 1), (0, 0, 0, 0, 0, 0), 0);
    let origin = ZonedOrigin {
        zone: &zone,
        calendar: AnyCalendarKind::Iso,
        epoch_nanoseconds: &ns,
        date: (275_758, 1, 1),
        time: (0, 0, 0, 0, 0, 0),
    };
    let duration = InternalDuration::from_date(1, 0, 0, 0);
    let destination = epoch::nanoseconds_since_epoch((275_760, 9, 13), (0, 0, 0, 0, 0, 0), 0);
    let mode = blueice_ecma402::NumberRoundingMode::Trunc;
    assert!(
        nudge_to_calendar_unit(&origin, &destination, &duration, 1, DateUnit::Year, mode).is_none()
    );
    let ordinary_ns = BigInt::from(0);
    let ordinary = ZonedOrigin {
        epoch_nanoseconds: &ordinary_ns,
        date: (1970, 1, 1),
        ..origin
    };
    let huge = BigInt::from(i128::MAX) * 2;
    // The first window is valid, as is its shifted successor. Reject the
    // progress before converting a larger-than-i128 fraction.
    assert!(nudge_to_calendar_unit(&ordinary, &huge, &duration, 1, DateUnit::Year, mode).is_none());
}

impl crate::Vm {
    /// Runs arithmetic contracts only in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_zoned_difference_boundary_contracts() {
        zoned_brackets_reject_increment_overflow_and_unrepresentable_dates();
        zoned_week_brackets_and_instant_differences_reject_unrepresentable_intermediates();
        non_positive_zoned_increments_are_rejected_before_rounding();
        shifted_zoned_windows_reject_an_unrepresentable_far_endpoint_and_huge_progress();
        zoned_rounding_rejects_out_of_range_local_calendar_and_instant_endpoints();
        compatible_resolution_preserves_gap_shifts_and_projection_boundaries();
    }
}

#[cfg_attr(test, test)]
fn compatible_resolution_preserves_gap_shifts_and_projection_boundaries() {
    let noon = (12, 0, 0, 0, 0, 0);
    for identifier in [
        "America/New_York",
        "Pacific/Apia",
        "Pacific/Kiritimati",
        "Pacific/Rarotonga",
        "Europe/Madrid",
    ] {
        let zone = super::super::time_zone::parse_identifier(identifier).unwrap();
        let epoch_ns = zone
            .epoch_nanoseconds_for((2000, 1, 1), noon, Disambiguation::Compatible)
            .unwrap();
        let origin = ZonedOrigin {
            zone: &zone,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &epoch_ns,
            date: (2000, 1, 1),
            time: noon,
        };
        for date in [
            (-10_000, 12, 31),
            (-9_999, 1, 1),
            (9_999, 12, 31),
            (10_000, 1, 1),
            (141_969, 12, 31),
            (141_970, 1, 1),
        ] {
            let resolved = origin
                .resolve(date)
                .expect("compatible projection boundary remains representable");
            assert!(epoch::is_in_instant_range(&resolved));
            let (actual_date, actual_time) = local_date_time(&zone, &resolved);
            let possible = zone.possible_epoch_nanoseconds(actual_date, actual_time);
            assert!(
                possible.contains(&resolved),
                "{identifier} {date:?}: resolved instant does not round-trip"
            );
        }
        if identifier == "Pacific/Apia" {
            let skipped = origin.resolve((2011, 12, 30)).unwrap();
            assert_eq!(local_date_time(&zone, &skipped), ((2011, 12, 31), noon));
        }
    }
    let zone = super::super::time_zone::parse_identifier("America/New_York").unwrap();
    let time = (2, 30, 0, 0, 0, 0);
    let epoch_ns = zone
        .epoch_nanoseconds_for((2020, 3, 7), time, Disambiguation::Compatible)
        .unwrap();
    let origin = ZonedOrigin {
        zone: &zone,
        calendar: AnyCalendarKind::Iso,
        epoch_nanoseconds: &epoch_ns,
        date: (2020, 3, 7),
        time,
    };
    let gap = origin.resolve((2020, 3, 8)).unwrap();
    assert_eq!(
        local_date_time(&zone, &gap),
        ((2020, 3, 8), (3, 30, 0, 0, 0, 0))
    );
}

#[cfg_attr(test, test)]
fn zoned_rounding_rejects_out_of_range_local_calendar_and_instant_endpoints() {
    use blueice_ecma402::NumberRoundingMode::Trunc;
    let utc = super::super::time_zone::parse_identifier("UTC").unwrap();
    let west = super::super::time_zone::parse_identifier("-23:00").unwrap();
    for (zone, date, time, days) in [
        (&utc, (275_760, 9, 12), (12, 0, 0, 0, 0, 0), 0),
        (&west, (275_760, 9, 11), (23, 0, 0, 0, 0, 0), 1),
    ] {
        let ns = zone
            .epoch_nanoseconds_for(date, time, Disambiguation::Compatible)
            .unwrap();
        assert!(epoch::is_in_instant_range(&ns));
        let origin = ZonedOrigin {
            zone,
            calendar: AnyCalendarKind::Iso,
            epoch_nanoseconds: &ns,
            date,
            time,
        };
        let mut duration = InternalDuration::from_date(0, 0, 0, days);
        duration.time_nanoseconds = 1;
        assert!(nudge_to_zoned_time(&origin, &duration, 1, TimeUnit::Hour, Trunc).is_none());
    }
    let ns = epoch::nanoseconds_since_epoch((2000, 1, 1), (0, 0, 0, 0, 0, 0), 0);
    let ordinary = ZonedOrigin {
        zone: &utc,
        calendar: AnyCalendarKind::Iso,
        epoch_nanoseconds: &ns,
        date: (2000, 1, 1),
        time: (0, 0, 0, 0, 0, 0),
    };
    assert!(nudge_to_zoned_time(
        &ordinary,
        &InternalDuration::from_date(1_000_000, 0, 0, 0),
        1,
        TimeUnit::Hour,
        Trunc
    )
    .is_none());
    assert!(nudge_to_zoned_time(
        &ordinary,
        &InternalDuration::from_date(i64::MAX / 2, 0, 0, 0),
        1,
        TimeUnit::Hour,
        Trunc
    )
    .is_none());
    assert!(bubble_relative_duration(
        &ordinary,
        1,
        InternalDuration::from_date(i64::MAX / 2, 0, 0, 0),
        &ns,
        TemporalUnit::Year,
        TemporalUnit::Month
    )
    .is_none());
    assert!(bubble_relative_duration(
        &ordinary,
        1,
        InternalDuration::from_date(1_000_000, 0, 0, 0),
        &ns,
        TemporalUnit::Year,
        TemporalUnit::Month
    )
    .is_none());

    let ns = epoch::nanoseconds_since_epoch((275_760, 8, 13), (12, 0, 0, 0, 0, 0), 0);
    let boundary = ZonedOrigin {
        epoch_nanoseconds: &ns,
        date: (275_760, 8, 13),
        time: (12, 0, 0, 0, 0, 0),
        ..ordinary
    };
    assert!(bubble_relative_duration(
        &boundary,
        1,
        InternalDuration::from_date(0, 0, 0, 1),
        &ns,
        TemporalUnit::Month,
        TemporalUnit::Day
    )
    .is_none());
}

#[cfg_attr(test, test)]
fn non_positive_zoned_increments_are_rejected_before_rounding() {
    let zone = super::super::time_zone::parse_identifier("UTC").unwrap();
    let ns = BigInt::from(0);
    let origin = ZonedOrigin {
        zone: &zone,
        calendar: AnyCalendarKind::Iso,
        epoch_nanoseconds: &ns,
        date: (1970, 1, 1),
        time: (0, 0, 0, 0, 0, 0),
    };
    for increment in [0, -1, i128::from(i64::MIN)] {
        for sign in [-1, 1] {
            let duration = InternalDuration::from_date(sign, 0, 0, 0);
            assert!(compute_nudge_window(
                &origin,
                &duration,
                sign,
                increment,
                DateUnit::Year,
                false
            )
            .is_none());
        }
    }
}
