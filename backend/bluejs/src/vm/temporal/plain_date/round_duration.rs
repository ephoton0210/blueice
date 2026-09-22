// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `RoundRelativeDuration` specialized to a calendar date pair (no time
//! component): [`round_calendar_duration`] and the anchor-relative
//! month/year rounding window ([`round_month_or_year`], Gecko's
//! `NudgeToCalendarUnit`) it rounds with.

use super::super::epoch::{self, CivilDate};
use super::super::rounding;
use super::calendar_add::calendar_add_date;
use super::calendar_difference::{calendar_difference_date, DateUnit};
use super::iso_date::{compare_iso_date, iso_date_to_epoch_days};
use icu_calendar::AnyCalendarKind;
use std::cmp::Ordering;

/// Rounds a signed whole-unit `count` to the nearest multiple of
/// `increment`, per `mode`, where the unit's own length is fixed (a day or a
/// week — always exactly 1 or 7 days regardless of where on the calendar it
/// falls). No anchor date is needed here, unlike [`round_month_or_year`]:
/// [`rounding::round_to_increment`] already implements exactly this.
fn round_fixed_length_count(
    count: i64,
    increment: i128,
    mode: blueice_ecma402::NumberRoundingMode,
) -> i64 {
    rounding::round_to_increment(i128::from(count), increment, mode) as i64
}

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
        let date = calendar_add_date(calendar, start, years, months, 0, 0, false)?;
        epoch::is_date_within_limits(date).then_some(date)
    };
    let lower = boundary(lower_multiple)?;
    let upper = boundary(upper_multiple)?;
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

/// `RoundRelativeDuration`, specialized to a calendar date pair (no time
/// component): computes the *unrounded* calendar duration from `start` to
/// `end` at `largest_unit` granularity (exactly [`calendar_difference_date`]
/// — years/months down to a day-granularity remainder), then rounds only the
/// trailing remainder to the nearest multiple of `increment`
/// `smallest_unit`s, per `mode`. This is deliberately *not* "bubble
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
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return Some((0, 0, 0, 0)),
    };
    let (years, months, weeks, days) = calendar_difference_date(calendar, start, end, largest_unit);
    Some(match smallest_unit {
        DateUnit::Day => {
            let rounded_days = round_fixed_length_count(days, increment, mode);
            (years, months, weeks, rounded_days)
        }
        // A week is always exactly 7 days regardless of calendar or
        // position, so — like `Day` — this needs no anchor date at all,
        // whether or not `largest_unit` itself has a `weeks` slot to put
        // the rounded value in.
        DateUnit::Week => {
            let total_days = weeks * 7 + days;
            let rounded = round_fixed_length_count(total_days, increment * 7, mode);
            if largest_unit == DateUnit::Week {
                (years, months, rounded / 7, 0)
            } else {
                (years, months, 0, rounded)
            }
        }
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
        DateUnit::Month => {
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
                let rounded =
                    calendar_add_date(calendar, start, years, rounded_months, 0, 0, false);
                let next_year = calendar_add_date(calendar, start, years + sign, 0, 0, 0, false);
                if let (Some(rounded), Some(next_year)) = (rounded, next_year) {
                    if compare_iso_date(rounded, next_year) as i64 * sign >= 0 {
                        return Some((years + sign, 0, 0, 0));
                    }
                }
            }
            (fixed_years, rounded_months, 0, 0)
        }
        DateUnit::Year => {
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
        }
    })
}
