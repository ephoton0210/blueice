// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `RoundRelativeDuration` specialized to a calendar date pair (no time
//! component): [`round_calendar_duration`] and the anchor-relative
//! month/year bracketing ([`round_month_or_year`]) it rounds with.

use super::super::calendar::calendar_months_per_year;
use super::super::epoch::CivilDate;
use super::super::rounding;
use super::calendar_add::calendar_add_date;
use super::calendar_difference::{calendar_difference_date, DateUnit};
use super::iso_date::{compare_iso_date, iso_date_to_epoch_days};
use super::month_structure::calendar_has_leap_months;
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

/// Rounds a signed whole-`unit` count (`unit` being `Month` or `Year`, whose
/// length varies by calendar position) to the nearest multiple of
/// `increment`, per `mode` — the anchor-relative algorithm every Temporal
/// implementation uses for calendar-unit rounding: since a "year" or "month"
/// has no fixed length, "round to the nearest 0.5 years" only means
/// something measured against a concrete anchor date. `count` whole `unit`s
/// from `start`, *plus* `fixed_years` additional years applied alongside
/// (used only for `unit == Month`, so a leap-month calendar's own `years`
/// value can be carried through the boundary probe unchanged rather than
/// flattened into a total month count — see
/// [`round_calendar_duration`]'s own `DateUnit::Month` branch for why; `0`
/// for `unit == Year`, where there is no separate months remainder), lands
/// on `lower`; one more `unit` lands on `upper`; `end`'s exact fractional
/// position (in epoch days) between them is the basis for rounding.
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
) -> i64 {
    let boundary = |n: i64| -> CivilDate {
        let (years, months) = match unit {
            DateUnit::Year => (n, 0),
            DateUnit::Month => (fixed_years, n),
            DateUnit::Week | DateUnit::Day => {
                unreachable!("round_month_or_year is only called for Month/Year units")
            }
        };
        calendar_add_date(calendar, start, years, months, 0, 0, false)
            .expect("constrain-mode addition always succeeds for a representable date")
    };
    let lower = boundary(count);
    let upper = boundary(count + sign);
    let total_span =
        (iso_date_to_epoch_days(upper) - iso_date_to_epoch_days(lower)).unsigned_abs() as i64;
    let progressed =
        (iso_date_to_epoch_days(end) - iso_date_to_epoch_days(lower)).unsigned_abs() as i64;

    let magnitude = i128::from(count.unsigned_abs());
    let increment_i128 = increment.max(1);
    let lower_multiple = (magnitude / increment_i128) * increment_i128;
    let upper_multiple = lower_multiple + increment_i128;
    let extra = magnitude - lower_multiple;
    let numerator = extra * i128::from(total_span) + i128::from(progressed);
    let denominator = increment_i128 * i128::from(total_span);
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
                    (lower_multiple / increment_i128) % 2 != 0
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
    sign * (final_magnitude as i64)
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn round_calendar_duration(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
    smallest_unit: DateUnit,
    increment: i128,
    mode: blueice_ecma402::NumberRoundingMode,
) -> (i64, i64, i64, i64) {
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };
    let (years, months, weeks, days) = calendar_difference_date(calendar, start, end, largest_unit);
    match smallest_unit {
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
        // Leap-month calendars (`chinese`/`dangi`/`hebrew`) don't have a
        // constant months-per-year, so folding `years` into a flat total
        // month count and re-splitting it back via `/ months_per_year,
        // % months_per_year` afterward (this branch's own previous approach,
        // still correct and kept for every other calendar -- including the
        // 13-month `coptic`/`ethiopic`/`ethioaa`, where `months_per_year` is
        // 13 rather than 12) is unsound for them: a `since`/`until`
        // `largestUnit: "years"` decomposition's own `months` remainder can
        // genuinely exceed 11 when a leap month is crossed (e.g. 2001's
        // Chinese `M04L` makes some single reported "year" span 13 months),
        // so re-deriving it from `years * 12 + months` silently produces a
        // *different* (and wrong) quantity than the one
        // `calendar_difference_date_leap_month` itself already computed.
        // Ported from Gecko's own `ComputeNudgeWindow`, which never flattens
        // in the first place for *any* calendar: it keeps `years` fixed and
        // rounds only the `months` remainder in place (`startDuration =
        // {years, r1}`). Confirmed empirically against the pinned Test262
        // corpus that this is specifically a leap-month-calendar problem,
        // not a general one (`PlainYearMonth`'s own default `since`/`until`
        // smallest-unit is `"month"`, so it always exercises this branch,
        // even with no explicit rounding option requested).
        DateUnit::Month if calendar_has_leap_months(calendar) && largest_unit == DateUnit::Year => {
            let rounded_months = round_month_or_year(
                calendar,
                start,
                years,
                end,
                DateUnit::Month,
                months,
                sign,
                increment,
                mode,
            );
            (years, rounded_months, 0, 0)
        }
        DateUnit::Month => {
            let months_per_year = calendar_months_per_year(calendar);
            let total_months = if largest_unit == DateUnit::Year {
                years * months_per_year + months
            } else {
                months
            };
            let rounded_months = round_month_or_year(
                calendar,
                start,
                0,
                end,
                DateUnit::Month,
                total_months,
                sign,
                increment,
                mode,
            );
            if largest_unit == DateUnit::Year {
                (
                    rounded_months / months_per_year,
                    rounded_months % months_per_year,
                    0,
                    0,
                )
            } else {
                (0, rounded_months, 0, 0)
            }
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
            );
            (rounded_years, 0, 0, 0)
        }
    }
}
