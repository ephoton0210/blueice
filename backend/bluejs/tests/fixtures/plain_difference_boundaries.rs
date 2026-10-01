// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Arithmetic helper rejection boundaries independently of VM option limits.

use super::*;

fn origin() -> Point {
    Point {
        calendar: AnyCalendarKind::Iso,
        date: (2020, 1, 1),
        time: (0, 0, 0, 0, 0, 0),
    }
}

#[test]
fn oversized_nudge_increments_and_shifted_brackets_return_none() {
    let origin = origin();
    let position = epoch_nanoseconds(origin.date, origin.time);
    let duration = InternalDuration::new((1, 0, 0, 0), 0);
    for (increment, shifted) in [(i128::MAX, false), (i128::from(i64::MAX), true)] {
        assert!(compute_nudge_window(
            origin,
            position,
            duration,
            increment,
            TemporalUnit::Year,
            shifted
        )
        .is_none());
    }
    let extreme = InternalDuration::new((i64::MAX, 0, 0, 0), 0);
    assert!(
        compute_nudge_window(origin, position, extreme, 1, TemporalUnit::Year, false).is_none()
    );
    assert!(compute_nudge_window(origin, position, extreme, 1, TemporalUnit::Year, true).is_none());
}

#[test]
fn week_nudges_reject_unrepresentable_calendar_and_day_intermediates() {
    let origin = origin();
    let position = epoch_nanoseconds(origin.date, origin.time);
    for duration in [
        InternalDuration::new((1_000_000, 0, 0, 1), 0),
        InternalDuration::new((0, 0, 0, 400_000_000), 0),
        InternalDuration::new((0, 0, 100_000_000, 1), 0),
    ] {
        assert!(
            compute_nudge_window(origin, position, duration, 1, TemporalUnit::Week, false)
                .is_none()
        );
    }
}
