// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

impl crate::Vm {
    /// Runs retained unit contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_zoned_difference_arithmetic_contracts() {
        direction_is_that_of_the_first_non_zero_field_and_positive_when_blank();
        a_time_largest_unit_is_a_plain_instant_difference();
        rounding_hours_up_to_the_day_length_carries_one_more_day();
        the_carry_uses_the_real_length_of_a_short_dst_day();
        the_excess_is_rounded_again_to_the_same_increment();
        a_remainder_that_stays_inside_its_day_does_not_carry();
        expanding_a_sub_day_remainder_bubbles_up_to_largest_unit();
        bubbling_stops_at_the_largest_unit();
        a_weeks_smallest_unit_never_bubbles();
        blank_duration_at_the_end_of_the_range_still_needs_the_next_day();
        total_measures_progress_through_the_bracketing_calendar_unit();
        total_in_days_uses_the_real_day_length();
        a_rounding_window_whose_end_is_unrepresentable_is_rejected();
        a_destination_past_the_first_window_selects_the_shifted_one();
        day_rounding_measures_progress_through_the_real_day_length();
        a_negative_difference_rounds_and_bubbles_in_its_own_direction();
        a_week_largest_unit_is_a_bubbling_target_for_days();
        a_negative_time_remainder_carries_a_negative_day();
        into_fields_balances_time_no_higher_than_the_largest_unit();
    }
}
