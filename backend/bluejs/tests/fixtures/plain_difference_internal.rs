// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ecma402::NumberRoundingMode as Mode;

const MIDNIGHT: CivilTime = (0, 0, 0, 0, 0, 0);

fn diff(
    from: (CivilDate, CivilTime),
    to: (CivilDate, CivilTime),
    largest: TemporalUnit,
    increment: i128,
    smallest: TemporalUnit,
    mode: Mode,
) -> DifferenceFields {
    difference_plain_date_time(
        AnyCalendarKind::Iso,
        from,
        to,
        largest,
        increment,
        smallest,
        mode,
    )
    .expect("in-range inputs")
}

fn exact(
    from: (CivilDate, CivilTime),
    to: (CivilDate, CivilTime),
    largest: TemporalUnit,
) -> DifferenceFields {
    diff(from, to, largest, 1, TemporalUnit::Nanosecond, Mode::Trunc)
}

#[cfg_attr(test, test)]
fn identical_date_times_have_a_zero_difference() {
    let point = ((2020, 1, 1), (1, 2, 3, 4, 5, 6));
    assert_eq!(exact(point, point, TemporalUnit::Year), [0; 10]);
}

#[cfg_attr(test, test)]
fn a_time_unit_largest_unit_folds_every_whole_day_into_the_time_fields() {
    let from = ((2020, 1, 1), MIDNIGHT);
    let to = ((2020, 1, 3), (5, 0, 0, 0, 0, 0));
    assert_eq!(
        exact(from, to, TemporalUnit::Hour),
        [0, 0, 0, 0, 53, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        exact(from, to, TemporalUnit::Minute),
        [0, 0, 0, 0, 0, 3180, 0, 0, 0, 0]
    );
    let nanoseconds = exact(from, to, TemporalUnit::Nanosecond);
    assert_eq!(nanoseconds[9], 53 * 3_600_000_000_000);
    assert_eq!(nanoseconds[..9], [0; 9]);
}

#[cfg_attr(test, test)]
fn a_same_day_difference_is_the_time_of_day_alone_in_either_direction() {
    // Equal dates have no date direction to disagree with, so there is no
    // borrow: 12:00 -> 06:00 is -6 hours, never "-1 day + 18 hours".
    let noon = ((2020, 1, 2), (12, 0, 0, 0, 0, 0));
    let six = ((2020, 1, 2), (6, 0, 0, 0, 0, 0));
    assert_eq!(
        exact(noon, six, TemporalUnit::Year),
        [0, 0, 0, 0, -6, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        exact(six, noon, TemporalUnit::Year),
        [0, 0, 0, 0, 6, 0, 0, 0, 0, 0]
    );
}

#[cfg_attr(test, test)]
fn a_time_of_day_running_against_the_date_direction_borrows_one_day() {
    // 2020-01-02T12:00 -> 2020-01-03T06:00 is 18 hours, not "1 day - 6 hours".
    let early = ((2020, 1, 2), (12, 0, 0, 0, 0, 0));
    let late = ((2020, 1, 3), (6, 0, 0, 0, 0, 0));
    assert_eq!(
        exact(early, late, TemporalUnit::Day),
        [0, 0, 0, 0, 18, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        exact(late, early, TemporalUnit::Day),
        [0, 0, 0, 0, -18, 0, 0, 0, 0, 0]
    );
}

#[cfg_attr(test, test)]
fn day_and_calendar_rounding_measure_the_time_of_day() {
    let from = ((2020, 1, 1), MIDNIGHT);
    let half_day_later = ((2020, 1, 2), (12, 0, 0, 0, 0, 0));
    // One day and twelve hours: `ceil` reaches two days, `trunc` stays at one.
    let day = TemporalUnit::Day;
    assert_eq!(
        diff(from, half_day_later, day, 1, day, Mode::Ceil),
        [0, 0, 0, 2, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        diff(from, half_day_later, day, 1, day, Mode::Trunc),
        [0, 0, 0, 1, 0, 0, 0, 0, 0, 0]
    );
    // One month and twelve hours: `ceil` to months reaches two months.
    let month = TemporalUnit::Month;
    assert_eq!(
        diff(
            from,
            ((2020, 2, 1), (12, 0, 0, 0, 0, 0)),
            month,
            1,
            month,
            Mode::Ceil
        ),
        [0, 2, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[cfg_attr(test, test)]
fn rounding_a_time_remainder_up_bubbles_into_the_larger_calendar_units() {
    // 1y 11m 30d 23:59:59.999999999 rounded up at microseconds is exactly two years.
    assert_eq!(
        diff(
            ((1970, 1, 1), MIDNIGHT),
            ((1971, 12, 31), (23, 59, 59, 999, 999, 999)),
            TemporalUnit::Year,
            1,
            TemporalUnit::Microsecond,
            Mode::Expand
        ),
        [2, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[cfg_attr(test, test)]
fn a_rounded_up_day_stays_in_hours_when_hours_are_the_largest_unit() {
    // Test262 `until/bubble-time-unit.js`: the rounding overflows a day,
    // but with `largestUnit: "hours"` there is no day field to carry into.
    assert_eq!(
        diff(
            ((2025, 6, 14), MIDNIGHT),
            ((2025, 6, 14), (14, 0, 0, 0, 0, 0)),
            TemporalUnit::Hour,
            12,
            TemporalUnit::Hour,
            Mode::Ceil
        ),
        [0, 0, 0, 0, 24, 0, 0, 0, 0, 0]
    );
}

#[cfg_attr(test, test)]
fn a_calendar_bracket_beyond_the_representable_range_is_reported() {
    // Rounding 1970-01-01..1971-01-01 to 100,000,000-month steps needs a
    // date 8 million years out: no `Some` result exists, so the caller
    // can raise the specification's `RangeError`.
    assert_eq!(
        difference_plain_date_time(
            AnyCalendarKind::Iso,
            ((1970, 1, 1), MIDNIGHT),
            ((1971, 1, 1), MIDNIGHT),
            TemporalUnit::Month,
            100_000_000,
            TemporalUnit::Month,
            Mode::Trunc,
        ),
        None
    );
}

#[cfg_attr(test, test)]
fn a_nanosecond_total_beyond_i64_is_kept_exact() {
    // 600 years of nanoseconds exceeds `i64`; the fields are `i128` so the
    // caller can round the true total to a Number instead of a wrapped one.
    let result = exact(
        ((2000, 1, 1), MIDNIGHT),
        ((2600, 1, 1), MIDNIGHT),
        TemporalUnit::Nanosecond,
    );
    let days = i128::from(
        plain_date::iso_date_to_epoch_days((2600, 1, 1))
            - plain_date::iso_date_to_epoch_days((2000, 1, 1)),
    );
    assert_eq!(result[9], days * NANOSECONDS_PER_DAY);
    assert!(result[9] > i128::from(i64::MAX));
}

#[cfg_attr(test, test)]
fn a_bracket_the_argument_overshoots_slides_one_increment_outward() {
    // Jan 31 12:00 -> Mar 1 06:00 has no whole month (Jan 31 + 1 month
    // constrains to Feb 29), so the calendar difference is days-only —
    // yet the argument lies past the first month bracket
    // [Jan 31, Feb 29]. The window slides to [Feb 29, Mar 31] and the
    // result counts as expanded even under `trunc`.
    let noon = (12, 0, 0, 0, 0, 0);
    let origin = Point {
        calendar: AnyCalendarKind::Iso,
        date: (2020, 1, 31),
        time: noon,
    };
    let eighteen_hours = 18 * 3_600_000_000_000;
    let (nudged, position, expanded) = nudge_to_calendar_unit(
        origin,
        epoch_nanoseconds((2020, 3, 1), (6, 0, 0, 0, 0, 0)),
        InternalDuration::new((0, 0, 0, 29), eighteen_hours),
        1,
        TemporalUnit::Month,
        Mode::Trunc,
    )
    .expect("in-range bracket");
    assert_eq!(nudged.date(), (0, 1, 0, 0));
    assert_eq!(position, epoch_nanoseconds((2020, 2, 29), noon));
    assert!(expanded);
}

#[cfg_attr(test, test)]
fn a_bracket_far_outside_the_representable_years_is_reported_not_overflowed() {
    // Two billion years still fits an `i32` year, so the calendar addition
    // succeeds and only the range check can reject it: `None`, never an
    // overflow panic on the way there.
    assert_eq!(
        difference_plain_date_time(
            AnyCalendarKind::Iso,
            ((2020, 1, 1), MIDNIGHT),
            ((2021, 1, 1), MIDNIGHT),
            TemporalUnit::Year,
            2_000_000_000,
            TemporalUnit::Year,
            Mode::Trunc,
        ),
        None
    );
}

fn date_diff(
    from: CivilDate,
    to: CivilDate,
    largest: TemporalUnit,
    increment: i128,
    smallest: TemporalUnit,
    mode: Mode,
) -> Option<DifferenceFields> {
    difference_plain_date(
        AnyCalendarKind::Iso,
        from,
        to,
        largest,
        increment,
        smallest,
        mode,
    )
}

#[cfg_attr(test, test)]
fn a_plain_date_difference_without_rounding_is_the_calendar_difference() {
    let day = TemporalUnit::Day;
    assert_eq!(
        date_diff(
            (2020, 1, 1),
            (2020, 1, 1),
            TemporalUnit::Year,
            1,
            day,
            Mode::Trunc
        ),
        Some([0; 10])
    );
    assert_eq!(
        date_diff(
            (2020, 1, 1),
            (2021, 3, 15),
            TemporalUnit::Year,
            1,
            day,
            Mode::Trunc
        ),
        Some([1, 2, 0, 14, 0, 0, 0, 0, 0, 0])
    );
    assert_eq!(
        date_diff(
            (2021, 3, 15),
            (2020, 1, 1),
            TemporalUnit::Month,
            1,
            day,
            Mode::Trunc
        ),
        Some([0, -14, 0, -14, 0, 0, 0, 0, 0, 0])
    );
    // A `largest` of days folds the whole span into days.
    assert_eq!(
        date_diff((2020, 2, 1), (2021, 2, 1), day, 1, day, Mode::Trunc),
        Some([0, 0, 0, 366, 0, 0, 0, 0, 0, 0])
    );
}

#[cfg_attr(test, test)]
fn a_plain_date_month_increment_rounds_only_the_months_remainder() {
    // 1 year 3 months: the 5-month bracket is [1y 0m, 1y 5m], 90 of 151 days in.
    let (year, month) = (TemporalUnit::Year, TemporalUnit::Month);
    let round = |mode| date_diff((2020, 1, 1), (2021, 4, 1), year, 5, month, mode);
    assert_eq!(round(Mode::Trunc), Some([1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    assert_eq!(
        round(Mode::HalfExpand),
        Some([1, 5, 0, 0, 0, 0, 0, 0, 0, 0])
    );
    assert_eq!(round(Mode::HalfTrunc), Some([1, 5, 0, 0, 0, 0, 0, 0, 0, 0]));
    // Backwards: 91 of the bracket's 152 days, so the same decisions with
    // every field negated (`floor`/`ceil` swap under negation).
    let back = |mode| date_diff((2021, 4, 1), (2020, 1, 1), year, 5, month, mode);
    assert_eq!(back(Mode::Trunc), Some([-1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    assert_eq!(
        back(Mode::HalfExpand),
        Some([-1, -5, 0, 0, 0, 0, 0, 0, 0, 0])
    );
    assert_eq!(back(Mode::Ceil), Some([-1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    assert_eq!(back(Mode::Floor), Some([-1, -5, 0, 0, 0, 0, 0, 0, 0, 0]));
}

#[cfg_attr(test, test)]
fn a_plain_date_week_bracket_keeps_the_months_a_month_largest_unit_reports() {
    // 1 month 9 days: the whole-week bracket starts at the month, so the
    // rounded weeks are reported in the `weeks` field alongside the month
    // (`NudgeToCalendarUnit`'s `{ years, months, r1 }` start duration),
    // not folded into days.
    let (month, week) = (TemporalUnit::Month, TemporalUnit::Week);
    let round = |mode| date_diff((2020, 1, 1), (2020, 2, 10), month, 1, week, mode);
    assert_eq!(round(Mode::Trunc), Some([0, 1, 1, 0, 0, 0, 0, 0, 0, 0]));
    assert_eq!(round(Mode::Expand), Some([0, 1, 2, 0, 0, 0, 0, 0, 0, 0]));
    // Two whole weeks past the month need no rounding whatever the mode.
    assert_eq!(
        date_diff((2020, 1, 1), (2020, 2, 15), month, 1, week, Mode::Expand),
        Some([0, 1, 2, 0, 0, 0, 0, 0, 0, 0])
    );
}

#[cfg_attr(test, test)]
fn a_plain_date_day_increment_rounds_whole_days_and_can_bubble() {
    let day = TemporalUnit::Day;
    assert_eq!(
        date_diff((2020, 1, 1), (2020, 1, 20), day, 7, day, Mode::Ceil),
        Some([0, 0, 0, 21, 0, 0, 0, 0, 0, 0])
    );
    // 1 month 28 days rounded up to a 30-day step is 1 month 30 days, which
    // reaches past Mar 1: with months as the largest unit it bubbles to 2.
    assert_eq!(
        date_diff(
            (2020, 1, 1),
            (2020, 2, 29),
            TemporalUnit::Month,
            30,
            day,
            Mode::Expand
        ),
        Some([0, 2, 0, 0, 0, 0, 0, 0, 0, 0])
    );
    // A whole month needs no rounding whatever the mode or increment.
    assert_eq!(
        date_diff(
            (2020, 1, 1),
            (2020, 2, 1),
            TemporalUnit::Month,
            30,
            day,
            Mode::Expand
        ),
        Some([0, 1, 0, 0, 0, 0, 0, 0, 0, 0])
    );
}

#[cfg_attr(test, test)]
fn a_plain_date_bracket_beyond_the_representable_range_is_reported() {
    // Rounding one year to 100,000,000-month steps needs a date millions
    // of years out.
    for unit in [TemporalUnit::Year, TemporalUnit::Month, TemporalUnit::Week] {
        assert_eq!(
            date_diff(
                (1970, 1, 1),
                (1971, 1, 1),
                unit,
                100_000_000,
                unit,
                Mode::Trunc
            ),
            None,
            "{unit:?}"
        );
    }
    // Days round arithmetically, with no bracket to place on the calendar.
    assert_eq!(
        date_diff(
            (1970, 1, 1),
            (1971, 1, 1),
            TemporalUnit::Day,
            100_000_000,
            TemporalUnit::Day,
            Mode::Trunc
        ),
        Some([0; 10])
    );
}

#[cfg_attr(test, test)]
fn the_round_up_decision_follows_the_unsigned_rounding_modes() {
    const MODES: [Mode; 9] = [
        Mode::Ceil,
        Mode::Floor,
        Mode::Expand,
        Mode::Trunc,
        Mode::HalfCeil,
        Mode::HalfFloor,
        Mode::HalfExpand,
        Mode::HalfTrunc,
        Mode::HalfEven,
    ];
    // The position is `numerator / 4` of the way between the brackets.
    let up = |mode, numerator, r1, sign| rounds_up(numerator, 4, r1, 1, sign, mode);
    for mode in MODES {
        for sign in [1, -1] {
            assert!(up(mode, 4, 0, sign), "{mode:?} at the far bracket");
            assert!(!up(mode, 0, 0, sign), "{mode:?} at the near bracket");
        }
    }
    // (mode, up at 1/4 for sign +1, at 1/4 for sign -1, at 3/4 for +1, at 3/4 for -1)
    let quarters = [
        (Mode::Ceil, true, false, true, false),
        (Mode::Floor, false, true, false, true),
        (Mode::Expand, true, true, true, true),
        (Mode::Trunc, false, false, false, false),
        (Mode::HalfCeil, false, false, true, true),
        (Mode::HalfFloor, false, false, true, true),
        (Mode::HalfExpand, false, false, true, true),
        (Mode::HalfTrunc, false, false, true, true),
        (Mode::HalfEven, false, false, true, true),
    ];
    for (mode, low_positive, low_negative, high_positive, high_negative) in quarters {
        assert_eq!(up(mode, 1, 0, 1), low_positive, "{mode:?} +1/4");
        assert_eq!(up(mode, 1, 0, -1), low_negative, "{mode:?} -1/4");
        assert_eq!(up(mode, 3, 0, 1), high_positive, "{mode:?} +3/4");
        assert_eq!(up(mode, 3, 0, -1), high_negative, "{mode:?} -3/4");
    }
    // Exactly halfway: (mode, sign +1, sign -1); `halfEven` follows `r1`'s parity.
    let halves = [
        (Mode::HalfCeil, true, false),
        (Mode::HalfFloor, false, true),
        (Mode::HalfExpand, true, true),
        (Mode::HalfTrunc, false, false),
    ];
    for (mode, positive, negative) in halves {
        assert_eq!(up(mode, 2, 0, 1), positive, "{mode:?} +1/2");
        assert_eq!(up(mode, 2, 0, -1), negative, "{mode:?} -1/2");
    }
    assert!(
        up(Mode::HalfEven, 2, 1, 1),
        "an odd lower bracket rounds up"
    );
    assert!(!up(Mode::HalfEven, 2, 2, 1), "an even lower bracket stays");
    assert!(
        up(Mode::HalfEven, 2, -1, -1),
        "a negative odd bracket rounds out"
    );
    // `halfEven` measures parity in whole increments, not raw counts.
    assert!(
        !rounds_up(2, 4, 6, 3, 1, Mode::HalfEven),
        "6 / 3 = 2 is even"
    );
    assert!(rounds_up(2, 4, 9, 3, 1, Mode::HalfEven), "9 / 3 = 3 is odd");
}

#[cfg_attr(test, test)]
fn a_weeks_bubble_target_is_only_used_when_weeks_are_the_largest_unit() {
    // 1 week 6 days 23:59:59.999999999 rounded up to whole hours is 24
    // hours into day 7 — exactly two weeks.
    let from = ((2020, 1, 1), MIDNIGHT);
    let to = ((2020, 1, 14), (23, 59, 59, 999, 999, 999));
    assert_eq!(
        diff(
            from,
            to,
            TemporalUnit::Week,
            1,
            TemporalUnit::Hour,
            Mode::Expand
        ),
        [0, 0, 2, 0, 0, 0, 0, 0, 0, 0]
    );
    // With months as the largest unit the same rounding stays at 14 days:
    // a weeks count is never introduced unasked.
    assert_eq!(
        diff(
            from,
            to,
            TemporalUnit::Month,
            1,
            TemporalUnit::Hour,
            Mode::Expand
        ),
        [0, 0, 0, 14, 0, 0, 0, 0, 0, 0]
    );
}

fn total(from: (CivilDate, CivilTime), to: (CivilDate, CivilTime), unit: TemporalUnit) -> f64 {
    let (numerator, denominator) =
        difference_plain_date_time_total(AnyCalendarKind::Iso, from, to, unit)
            .expect("in-range inputs");
    rounding::exact_ratio_to_f64(numerator, denominator)
}

#[cfg_attr(test, test)]
fn a_total_in_an_exact_unit_is_the_plain_ratio_of_nanoseconds() {
    let from = ((2020, 1, 1), MIDNIGHT);
    let to = ((2020, 1, 2), (12, 0, 0, 0, 0, 0));
    assert_eq!(total(from, from, TemporalUnit::Hour), 0.0);
    assert_eq!(total(from, to, TemporalUnit::Day), 1.5);
    assert_eq!(total(from, to, TemporalUnit::Hour), 36.0);
    assert_eq!(total(to, from, TemporalUnit::Day), -1.5);
    assert_eq!(
        total(from, to, TemporalUnit::Nanosecond),
        129_600_000_000_000.0
    );
}

#[cfg_attr(test, test)]
fn a_calendar_total_measures_the_position_inside_its_bracket() {
    // 2020-01-31 + 1 month is 2020-02-29; the window to 2020-03-31 is 31
    // days, and 10 hours into it is 10/744 of a month.
    let from = ((2020, 1, 31), MIDNIGHT);
    let to = ((2020, 2, 29), (10, 0, 0, 0, 0, 0));
    assert_eq!(total(from, to, TemporalUnit::Month), 1.0134408602150538);
    // Two whole years is exactly 2, with no drift.
    assert_eq!(
        total(
            ((2020, 2, 29), MIDNIGHT),
            ((2022, 2, 28), MIDNIGHT),
            TemporalUnit::Year
        ),
        2.0
    );
    // Backwards from 2020-03-01 to 2020-02-15: the window runs back to
    // 2020-02-01 (29 days), and 15 of them are covered, counted toward zero.
    assert_eq!(
        total(
            ((2020, 3, 1), MIDNIGHT),
            ((2020, 2, 15), MIDNIGHT),
            TemporalUnit::Month
        ),
        -15.0 / 29.0
    );
}

#[cfg_attr(test, test)]
fn a_week_total_counts_seven_day_windows() {
    let from = ((2021, 3, 1), MIDNIGHT);
    let to = ((2021, 3, 11), (12, 0, 0, 0, 0, 0));
    // 1 week + 3.5 days.
    assert_eq!(total(from, to, TemporalUnit::Week), 1.5);
}

impl crate::Vm {
    /// Runs retained unit contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_plain_difference_arithmetic_contracts() {
        identical_date_times_have_a_zero_difference();
        a_time_unit_largest_unit_folds_every_whole_day_into_the_time_fields();
        a_same_day_difference_is_the_time_of_day_alone_in_either_direction();
        a_time_of_day_running_against_the_date_direction_borrows_one_day();
        day_and_calendar_rounding_measure_the_time_of_day();
        rounding_a_time_remainder_up_bubbles_into_the_larger_calendar_units();
        a_rounded_up_day_stays_in_hours_when_hours_are_the_largest_unit();
        a_calendar_bracket_beyond_the_representable_range_is_reported();
        a_nanosecond_total_beyond_i64_is_kept_exact();
        a_bracket_the_argument_overshoots_slides_one_increment_outward();
        a_bracket_far_outside_the_representable_years_is_reported_not_overflowed();
        a_plain_date_difference_without_rounding_is_the_calendar_difference();
        a_plain_date_month_increment_rounds_only_the_months_remainder();
        a_plain_date_week_bracket_keeps_the_months_a_month_largest_unit_reports();
        a_plain_date_day_increment_rounds_whole_days_and_can_bubble();
        a_plain_date_bracket_beyond_the_representable_range_is_reported();
        the_round_up_decision_follows_the_unsigned_rounding_modes();
        a_weeks_bubble_target_is_only_used_when_weeks_are_the_largest_unit();
        a_total_in_an_exact_unit_is_the_plain_ratio_of_nanoseconds();
        a_calendar_total_measures_the_position_inside_its_bracket();
        a_week_total_counts_seven_day_windows();
    }
}
