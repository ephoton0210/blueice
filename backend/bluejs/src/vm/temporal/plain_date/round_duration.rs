// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `RoundRelativeDuration` specialized to a calendar date pair (no time
//! component): [`round_calendar_duration`] and the anchor-relative
//! month/year rounding window ([`round_month_or_year`], Gecko's
//! `NudgeToCalendarUnit`) it rounds with.

use super::super::epoch::{self, CivilDate};
use super::calendar_add::calendar_add_date;
use super::calendar_difference::{calendar_difference_date, DateUnit};
use super::iso_date::{compare_iso_date, iso_date_to_epoch_days};
use icu_calendar::AnyCalendarKind;
use std::cmp::Ordering;

/// `NudgeToCalendarUnit` for `unit` being `Month` or `Year` (whose length
/// varies by calendar position): rounds the signed whole-`unit` `count` to a
/// multiple of `increment`, per `mode`, measured against a concrete anchor --
/// "round to the nearest 0.5 years" only means something relative to a date.
///
/// The rounding *window* is the pair of dates `r1` and `r2` units from `start`,
/// where `r1` is `count` truncated to a multiple of `increment` and `r2 = r1 +
/// increment` (`fixed_years` additional whole years are applied alongside for
/// `unit == Month`, so the months *remainder* of a `largestUnit: "years"`
/// difference is rounded in place rather than flattened into a total -- with an
/// increment of 5 months, 2 years 8 months is 2 years 5 months, not 30 months
/// re-split). `end`'s exact fractional position between the two window dates
/// (in epoch days) decides whether the value rounds to `r1` or `r2`.
///
/// `None` when either window date leaves Temporal's representable range -- the
/// spec's `CalendarDateAdd` `RangeError`, reachable with a large increment
/// (`PlainYearMonth/prototype/{since,until}/throws-if-rounded-date-outside-
/// valid-iso-range.js`).
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn round_month_or_year(
    calendar: AnyCalendarKind,
    start: CivilDate,
    fixed_years: i64,
    end: CivilDate,
    unit: DateUnit,
    count: i64,
    sign: i64,
    increment: i128,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<i64> {
    let increment = increment.max(1);
    let magnitude = i128::from(count.unsigned_abs());
    let lower_multiple = (magnitude / increment) * increment;
    let upper_multiple = lower_multiple + increment;
    let boundary = |multiple: i128| -> Option<CivilDate> {
        let n = i64::try_from(multiple).ok()? * sign;
        let (years, months) = match unit {
            DateUnit::Year => (n, 0),
            DateUnit::Month => (fixed_years, n),
            DateUnit::Week | DateUnit::Day => {
                unreachable!("round_month_or_year is only called for Month/Year units")
            }
        };
        calendar_add_date(calendar, start, years, months, 0, 0, false)
            .filter(|date| epoch::is_date_within_limits(*date))
    };
    let (lower, upper) = boundary(lower_multiple).zip(boundary(upper_multiple))?;
    let total_span =
        (iso_date_to_epoch_days(upper) - iso_date_to_epoch_days(lower)).unsigned_abs() as i64;
    let progressed =
        (iso_date_to_epoch_days(end) - iso_date_to_epoch_days(lower)).unsigned_abs() as i64;

    let numerator = i128::from(progressed);
    let denominator = i128::from(total_span);
    let round_up = if denominator == 0 || numerator == 0 {
        false
    } else {
        use blueice_ecma402::NumberRoundingMode as Mode;
        match mode {
            Mode::Ceil => sign > 0,
            Mode::Floor => sign < 0,
            Mode::Expand => true,
            Mode::Trunc => false,
            Mode::HalfCeil => {
                if sign > 0 {
                    2 * numerator >= denominator
                } else {
                    2 * numerator > denominator
                }
            }
            Mode::HalfFloor => {
                if sign < 0 {
                    2 * numerator >= denominator
                } else {
                    2 * numerator > denominator
                }
            }
            Mode::HalfExpand => 2 * numerator >= denominator,
            Mode::HalfTrunc => 2 * numerator > denominator,
            Mode::HalfEven => {
                if 2 * numerator == denominator {
                    (lower_multiple / increment) % 2 != 0
                } else {
                    2 * numerator > denominator
                }
            }
        }
    };
    let final_magnitude = if round_up {
        upper_multiple
    } else {
        lower_multiple
    };
    i64::try_from(final_magnitude)
        .ok()
        .map(|value| sign * value)
}

/// `RoundRelativeDuration` for PlainYearMonth: computes the *unrounded*
/// calendar duration from `start` to `end` in years/months (exactly
/// [`calendar_difference_date`]), then rounds the trailing month or year
/// remainder to the nearest multiple of `increment` per `mode`.
/// The caller handles equal dates before reaching this function, and
/// `largest_unit` and `smallest_unit` are always Month or Year. This is
/// deliberately *not* "bubble
/// `smallest_unit` steps from `start`, then re-decompose at `largest_unit`"
/// — Test262's `PlainDate/prototype/since/exact-multiple-of-larger-unit.js`
/// is the fixture that rules that shape out: rounding a `{ largestUnit:
/// "months", smallestUnit: "weeks" }` difference that is *exactly* one month
/// must report `{ months: 1 }` in every rounding mode, not a `weeks`-sized
/// wobble around a month that isn't a whole number of weeks.
///
/// `None` when the rounding window leaves Temporal's range (see
/// [`round_month_or_year`]); callers report that as a `RangeError`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn round_calendar_duration(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
    smallest_unit: DateUnit,
    increment: i128,
    mode: blueice_ecma402::NumberRoundingMode,
) -> Option<(i64, i64, i64, i64)> {
    let sign = if compare_iso_date(start, end) == Ordering::Less {
        1_i64
    } else {
        -1
    };
    let (years, months, _, _) = calendar_difference_date(calendar, start, end, largest_unit);
    Some(if smallest_unit == DateUnit::Month {
        // Gecko's `ComputeNudgeWindow` never flattens `years` into a total
        // month count, for *any* calendar: it keeps `years` fixed and rounds
        // only the `months` remainder in place (`startDuration = {years,
        // r1}`). Flattening (`years * monthsPerYear + months`, re-split
        // afterwards) agrees with that only for an increment of 1; with any
        // other increment the window's `r1` is a multiple of the increment of
        // the *remainder*, not of the total (`PlainYearMonth/prototype/
        // {since,until}/roundingincrement-as-expected.js`: 2 years 8 months,
        // increment 5, is 2 years 5 months). It is also unsound for a
        // leap-month calendar, whose `months` remainder can exceed 11 when a
        // leap month is crossed.
        let fixed_years = if largest_unit == DateUnit::Year {
            years
        } else {
            0
        };
        let rounded_months = round_month_or_year(
            calendar,
            start,
            fixed_years,
            end,
            DateUnit::Month,
            months,
            sign,
            increment,
            mode,
        )?;
        // `BubbleRelativeDuration`, only after the months were rounded *up*
        // (`DidExpandCalendarUnit`): the expanded value may land exactly on
        // (or past) the start of the next larger unit, in which case it
        // becomes one more whole year with no months. An unrounded value
        // must never bubble -- a leap-month calendar's own difference can
        // legitimately report `0y 12m` even though `start + 12 months`
        // and `start + 1 year` are the same date once the leap month
        // constrains away (`chinese` `M04L` -> the next year's `M04`).
        let expanded = rounded_months.unsigned_abs() > months.unsigned_abs();
        if largest_unit == DateUnit::Year && expanded {
            let rounded = calendar_add_date(calendar, start, years, rounded_months, 0, 0, false);
            let next_year = calendar_add_date(calendar, start, years + sign, 0, 0, 0, false);
            if rounded.zip(next_year).is_some_and(|(rounded, next_year)| {
                compare_iso_date(rounded, next_year) as i64 * sign >= 0
            }) {
                return Some((years + sign, 0, 0, 0));
            }
        }
        (fixed_years, rounded_months, 0, 0)
    } else {
        let rounded_years = round_month_or_year(
            calendar,
            start,
            0,
            end,
            DateUnit::Year,
            years,
            sign,
            increment,
            mode,
        )?;
        (rounded_years, 0, 0, 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ecma402::NumberRoundingMode::{Expand, HalfEven, Trunc};

    const ISO: AnyCalendarKind = AnyCalendarKind::Iso;

    #[test]
    fn equal_dates_have_an_empty_difference_at_any_granularity() {
        for unit in [
            DateUnit::Year,
            DateUnit::Month,
            DateUnit::Week,
            DateUnit::Day,
        ] {
            assert_eq!(
                round_calendar_duration(ISO, (2020, 5, 5), (2020, 5, 5), unit, unit, 3, Trunc),
                Some((0, 0, 0, 0))
            );
        }
    }

    #[test]
    fn an_increment_beyond_i64_leaves_the_range() {
        assert_eq!(
            round_calendar_duration(
                ISO,
                (1970, 1, 1),
                (1971, 1, 1),
                DateUnit::Year,
                DateUnit::Month,
                i128::MAX,
                Trunc
            ),
            None
        );
    }

    #[test]
    fn half_even_ties_round_to_the_even_multiple() {
        // 2019-01-01 to 2019-02-15 is 1 month 14 days, exactly half of
        // February's 28 days: the odd month count rounds up to 2.
        assert_eq!(
            round_calendar_duration(
                ISO,
                (2019, 1, 1),
                (2019, 2, 15),
                DateUnit::Month,
                DateUnit::Month,
                1,
                HalfEven
            ),
            Some((0, 2, 0, 0))
        );
        // 2019-02-01 to 2019-02-15 is 0 months 14 days, half of 28: the even
        // count stays.
        assert_eq!(
            round_calendar_duration(
                ISO,
                (2019, 2, 1),
                (2019, 2, 15),
                DateUnit::Month,
                DateUnit::Month,
                1,
                HalfEven
            ),
            Some((0, 0, 0, 0))
        );
    }

    #[test]
    fn expanded_months_that_stay_below_a_year_do_not_bubble() {
        // 2 years 7 months rounded up to an increment of 5 months is 10
        // months: still short of the next year.
        assert_eq!(
            round_calendar_duration(
                ISO,
                (2019, 1, 1),
                (2021, 8, 1),
                DateUnit::Year,
                DateUnit::Month,
                5,
                Expand
            ),
            Some((2, 10, 0, 0))
        );
    }
}
