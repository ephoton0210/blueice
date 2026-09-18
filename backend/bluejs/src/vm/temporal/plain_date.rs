// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral calendar-date arithmetic for `Temporal.PlainDate`/
//! `PlainDateTime` (Phase 26 Stage 2,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `TemporalValue` always stores a date's fields as plain ISO
//! `(year, month, day)`, regardless of the value's own `calendar` identifier
//! — the calendar only changes how those ISO fields are *presented*
//! (`Vm::temporal_calendar_fields`, in the parent module). This module is
//! therefore split into two halves:
//!
//! - Pure ISO-calendar civil-date math (`add_iso_date`, `difference_iso_date`,
//!   the ISO week-date getters) — ported directly from the spec's
//!   `AddISODate`/`DifferenceISODate`/`BalanceISODate` abstract operations,
//!   with no calendar dispatch at all. No `Value`/heap/Realm coupling.
//! - `calendar_add_date`/`calendar_difference_date`, which extend the same
//!   algorithm shape to every other closed calendar ID via `icu_calendar`,
//!   for `add`/`subtract`/`until`/`since` on a non-ISO `PlainDate`. These use
//!   `icu_calendar` directly (no `Value`/heap/Realm coupling either) — the
//!   same precedent `calendar.rs` and the parent module's own
//!   `temporal_calendar_fields` already established.

use super::epoch::CivilDate;
use super::rounding;
use icu_calendar::options::{DateFromFieldsOptions, Overflow as IcuOverflow};
use icu_calendar::types::DateFields;
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, Iso};
use std::cmp::Ordering;

/// `1` (Monday) through `7` (Sunday) — `ISODayOfWeek`. Calendar-invariant:
/// Temporal's day-of-week/week-of-year getters operate on the ISO
/// representation for every calendar, per the current spec revision.
pub(crate) fn iso_day_of_week(date: CivilDate) -> u8 {
    let days = iso_date_to_epoch_days(date);
    // 1970-01-01 (epoch day 0) is a Thursday (ISO weekday 4).
    (days.rem_euclid(7) + 3).rem_euclid(7) as u8 + 1
}

/// `ISODayOfYear`, 1-based.
pub(crate) fn iso_day_of_year(date: CivilDate) -> u16 {
    (iso_date_to_epoch_days(date) - iso_date_to_epoch_days((date.0, 1, 1)) + 1) as u16
}

pub(crate) fn is_iso_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub(crate) fn iso_days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_iso_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => unreachable!("Temporal ISO months are regulated to 1..=12"),
    }
}

/// `weeksInYear` in ISO 8601 week numbering: a year has 53 weeks iff its own
/// "day of the century" parity `p(year)` is 4 (1 January is a Thursday), or
/// the *previous* year's `p(year - 1)` is 3 (this year's 31 December is a
/// Thursday, which happens when the previous year starts on a Wednesday and
/// is either common, or leap starting on a Tuesday — `p` already folds both
/// cases together).
fn iso_weeks_in_year(year: i32) -> u8 {
    let p = |y: i64| (y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400)).rem_euclid(7);
    if p(i64::from(year)) == 4 || p(i64::from(year) - 1) == 3 {
        53
    } else {
        52
    }
}

/// `ISOWeekOfYear`: returns `(week, yearOfWeek)`, the ISO 8601 week number
/// (1..=53) and the year that week belongs to (which may differ from the
/// date's own calendar year at the turn of the year).
pub(crate) fn iso_week_of_year(date: CivilDate) -> (u8, i32) {
    let day_of_year = i64::from(iso_day_of_year(date));
    let day_of_week = i64::from(iso_day_of_week(date));
    let week = (day_of_year - day_of_week + 10).div_euclid(7);
    if week < 1 {
        (iso_weeks_in_year(date.0 - 1), date.0 - 1)
    } else if week > i64::from(iso_weeks_in_year(date.0)) {
        (1, date.0 + 1)
    } else {
        (week as u8, date.0)
    }
}

/// Days since the epoch (1970-01-01 = day 0), via the same Howard Hinnant
/// `days_from_civil` calculation [`super::epoch::nanoseconds_since_epoch`]
/// uses, without the nanosecond scaling.
pub(crate) fn iso_date_to_epoch_days(date: CivilDate) -> i64 {
    let (year, month, day) = date;
    let adjusted_year = i64::from(year) - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let march_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * march_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`iso_date_to_epoch_days`] (`civil_from_days`).
pub(crate) fn epoch_days_to_iso_date(days: i64) -> CivilDate {
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
    (year, month, day)
}

pub(crate) fn compare_iso_date(a: CivilDate, b: CivilDate) -> Ordering {
    (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2))
}

/// `BalanceISOYearMonth`: carries an out-of-range 1-based month into the
/// year field.
pub(crate) fn balance_iso_year_month(year: i64, month: i64) -> (i64, u8) {
    let zero_based = month - 1;
    let carried_year = year + zero_based.div_euclid(12);
    let carried_month = (zero_based.rem_euclid(12) + 1) as u8;
    (carried_year, carried_month)
}

/// `RegulateISODate`: clamps (`reject == false`) or rejects (`reject ==
/// true`) a day that overflows its month, assuming `year`/`month` are
/// already in range.
pub(crate) fn regulate_iso_date(year: i32, month: u8, day: i64, reject: bool) -> Option<CivilDate> {
    let max = i64::from(iso_days_in_month(year, month));
    if reject {
        if !(1..=max).contains(&day) {
            return None;
        }
        Some((year, month, day as u8))
    } else {
        Some((year, month, day.clamp(1, max) as u8))
    }
}

/// `BalanceISODate`: normalizes a `day` field of any magnitude (including
/// zero or negative) back into a valid calendar date, via epoch-day
/// arithmetic.
pub(crate) fn balance_iso_date(year: i32, month: u8, day: i64) -> CivilDate {
    let start_of_month = iso_date_to_epoch_days((year, month, 1));
    epoch_days_to_iso_date(start_of_month + day - 1)
}

/// `AddISODate`. `year`/`month` are carried first (with the day
/// constrained/rejected against the *landing* month), then `weeks`/`days`
/// are added as a flat day offset — exactly the two-phase order Test262's
/// `PlainDate/prototype/add/basic.js` pins (`2019-01-31` + 1 month is
/// `2019-02-28`, not a day-balanced March date).
pub(crate) fn add_iso_date(
    date: CivilDate,
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    reject: bool,
) -> Option<CivilDate> {
    let (carried_year, carried_month) =
        balance_iso_year_month(i64::from(date.0) + years, i64::from(date.1) + months);
    let carried_year = i32::try_from(carried_year).ok()?;
    let (year, month, day) = regulate_iso_date(carried_year, carried_month, i64::from(date.2), reject)?;
    Some(balance_iso_date(year, month, i64::from(day) + days + weeks * 7))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DateUnit {
    Year,
    Month,
    Week,
    Day,
}

/// `DifferenceISODate(y1, m1, d1, y2, m2, d2, largestUnit)`: the calendar
/// duration `(years, months, weeks, days)` — signed, all the same sign as
/// `end - start` — such that `AddISODate(start, duration) == end`.
///
/// Years are estimated directly (`end.year - start.year`) and then months
/// bubble in a loop bounded to at most ~12 iterations, *not* one bounded by
/// the total span: after the year estimate lands within a year of `end`
/// (correcting by at most one year if it overshot), only the remaining
/// within-year month offset needs bubbling. This keeps the whole function
/// O(1) even across Temporal's full ±273,000-year range.
pub(crate) fn difference_iso_date(
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };
    if !matches!(largest_unit, DateUnit::Year | DateUnit::Month) {
        let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(start);
        return if largest_unit == DateUnit::Week {
            (0, 0, days / 7, days % 7)
        } else {
            (0, 0, 0, days)
        };
    }

    let land = |years: i64, months: i64| -> CivilDate {
        let (y, m) = balance_iso_year_month(i64::from(start.0) + years, i64::from(start.1) + months);
        // A landing year outside i32 cannot occur for any representable
        // Temporal date pair, so this only ever clamps a same-year overflow.
        let y = i32::try_from(y).unwrap_or(if y > 0 { i32::MAX } else { i32::MIN });
        regulate_iso_date(y, m, i64::from(start.2), false)
            .expect("constrain-mode regulation always succeeds")
    };
    let sign_towards = |candidate: CivilDate| -> i64 {
        match compare_iso_date(candidate, end) {
            Ordering::Less => 1,
            Ordering::Greater => -1,
            Ordering::Equal => 0,
        }
    };

    let mut years = i64::from(end.0) - i64::from(start.0);
    let mut mid = land(years, 0);
    if sign_towards(mid) == -sign {
        years -= sign;
        mid = land(years, 0);
    }

    let mut months = 0_i64;
    loop {
        let candidate_months = months + sign;
        let candidate = land(years, candidate_months);
        let candidate_sign = sign_towards(candidate);
        if candidate_sign == -sign {
            break;
        }
        months = candidate_months;
        mid = candidate;
        if candidate_sign == 0 {
            break;
        }
    }

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(mid);
    if largest_unit == DateUnit::Month {
        (0, months + years * 12, 0, days)
    } else {
        (years, months, 0, days)
    }
}

/// `ISODateToString`'s date-only portion, including `ToTemporalYearMonth`'s
/// six-digit signed extended-year form for a year outside `0..=9999`.
pub(crate) fn format_iso_date(date: CivilDate) -> String {
    let (year, month, day) = date;
    let year_text = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
    };
    format!("{year_text}-{month:02}-{day:02}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShowCalendar {
    Auto,
    Always,
    Never,
    Critical,
}

pub(crate) fn parse_show_calendar(value: &str) -> Option<ShowCalendar> {
    Some(match value {
        "auto" => ShowCalendar::Auto,
        "always" => ShowCalendar::Always,
        "never" => ShowCalendar::Never,
        "critical" => ShowCalendar::Critical,
        _ => return None,
    })
}

/// `FormatCalendarAnnotation`: omits an `iso8601` calendar unless the option
/// forces it, and prefixes a critical `!` when requested.
pub(crate) fn format_calendar_annotation(calendar: &str, show: ShowCalendar) -> String {
    match show {
        ShowCalendar::Never => String::new(),
        ShowCalendar::Auto if calendar == "iso8601" => String::new(),
        ShowCalendar::Critical => format!("[!u-ca={calendar}]"),
        ShowCalendar::Auto | ShowCalendar::Always => format!("[u-ca={calendar}]"),
    }
}

/// `months_in_year` for a probe date at `(year, 1, 1)` in `calendar` — used
/// by [`calendar_add_date`] to discover how many months a *landing* year has
/// before carrying into it, since that can vary by year for a lunisolar
/// calendar's leap months.
fn months_in_year_for(calendar: AnyCalendarKind, year: i32) -> Option<u8> {
    let mut fields = DateFields::default();
    fields.extended_year = Some(year);
    fields.ordinal_month = Some(1);
    fields.day = Some(1);
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(IcuOverflow::Constrain);
    let date = Date::try_from_fields(fields, options, AnyCalendar::new(calendar)).ok()?;
    Some(date.months_in_year())
}

/// The non-ISO generalization of [`add_iso_date`]: adds `years`/`months` in
/// the target calendar's own numbering (carrying across year boundaries
/// against that landing year's *own* `months_in_year`, not the start year's),
/// regulates `day` in the resulting month/year, then folds `weeks`/`days`
/// back in as a flat ISO day offset — calendar-agnostic once a concrete date
/// exists, since every representable date has exactly one ISO form.
pub(crate) fn calendar_add_date(
    calendar: AnyCalendarKind,
    date: CivilDate,
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    reject: bool,
) -> Option<CivilDate> {
    if calendar == AnyCalendarKind::Iso {
        return add_iso_date(date, years, months, weeks, days, reject);
    }
    let iso = Date::try_new_iso(date.0, date.1, date.2).ok()?;
    let cal_date = iso.to_calendar(AnyCalendar::new(calendar));
    let start_year = cal_date.year().extended_year();
    let start_month = i64::from(cal_date.month().ordinal);
    let start_day = i64::from(cal_date.day_of_month().0);

    let mut year = i64::from(start_year) + years;
    let mut month = start_month + months;
    if months >= 0 {
        loop {
            let year32 = i32::try_from(year).ok()?;
            let months_in_year = i64::from(months_in_year_for(calendar, year32)?);
            if month <= months_in_year {
                break;
            }
            month -= months_in_year;
            year += 1;
        }
    } else {
        loop {
            if month >= 1 {
                break;
            }
            year -= 1;
            let year32 = i32::try_from(year).ok()?;
            let months_in_year = i64::from(months_in_year_for(calendar, year32)?);
            month += months_in_year;
        }
    }
    let year = i32::try_from(year).ok()?;
    let month = u8::try_from(month).ok()?;

    let mut fields = DateFields::default();
    fields.extended_year = Some(year);
    fields.ordinal_month = Some(month);
    let mut options = DateFromFieldsOptions::default();
    if reject {
        fields.day = Some(u8::try_from(start_day).ok()?);
        options.overflow = Some(IcuOverflow::Reject);
    } else {
        // Constrain the day to the landing month ourselves via a
        // Constrain-mode probe day 1, then clamp against its
        // `days_in_month` — `try_from_fields` alone already constrains an
        // out-of-range day, so this is simply always constrain-mode here,
        // with `reject` distinguishing whether an out-of-range day is an
        // error at all.
        fields.day = Some(u8::try_from(start_day.min(31)).ok()?);
        options.overflow = Some(IcuOverflow::Constrain);
    }
    let landed = Date::try_from_fields(fields, options, AnyCalendar::new(calendar)).ok()?;
    let landed_iso = landed.to_calendar(Iso);
    let landed_civil = (
        landed_iso.year().extended_year(),
        landed_iso.month().number(),
        landed_iso.day_of_month().0,
    );
    Some(balance_iso_date(
        landed_civil.0,
        landed_civil.1,
        i64::from(landed_civil.2) + days + weeks * 7,
    ))
}

/// The non-ISO generalization of [`difference_iso_date`]: identical
/// estimate-then-bubble shape, but each "add years/months to start" probe
/// goes through [`calendar_add_date`] instead of the pure-ISO fast path, so
/// the year/month carry honours the target calendar's own numbering.
pub(crate) fn calendar_difference_date(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    if calendar == AnyCalendarKind::Iso {
        return difference_iso_date(start, end, largest_unit);
    }
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };
    if !matches!(largest_unit, DateUnit::Year | DateUnit::Month) {
        let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(start);
        return if largest_unit == DateUnit::Week {
            (0, 0, days / 7, days % 7)
        } else {
            (0, 0, 0, days)
        };
    }

    let land = |years: i64, months: i64| -> CivilDate {
        calendar_add_date(calendar, start, years, months, 0, 0, false)
            .expect("constrain-mode calendar regulation always succeeds")
    };
    let sign_towards = |candidate: CivilDate| -> i64 {
        match compare_iso_date(candidate, end) {
            Ordering::Less => 1,
            Ordering::Greater => -1,
            Ordering::Equal => 0,
        }
    };

    // A calendar year does not correspond exactly to 365.25 ISO days for
    // every calendar (e.g. a Hijri year is ~354.37 days) — dividing by a
    // fixed 366 would misestimate by several percent per year, which is
    // fine for a short span but turns the correction loop below into a
    // near-linear scan (thousands of iterations, each an `icu_calendar`
    // `Date` construction) for a multi-century span. Probe the *actual*
    // length of one calendar year from `start` instead, so the estimate is
    // exact for a fixed-length calendar and close for a variable one.
    let day_span = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(start);
    let probe_year = land(sign, 0);
    let year_length = (iso_date_to_epoch_days(probe_year) - iso_date_to_epoch_days(start))
        .unsigned_abs()
        .max(1) as i64;
    let mut years = day_span / year_length;
    let mut mid = land(years, 0);
    while sign_towards(mid) == -sign {
        years -= sign;
        mid = land(years, 0);
    }
    loop {
        let candidate = land(years + sign, 0);
        if sign_towards(candidate) == -sign {
            break;
        }
        years += sign;
        mid = candidate;
    }

    let mut months = 0_i64;
    loop {
        let candidate_months = months + sign;
        let candidate = land(years, candidate_months);
        let candidate_sign = sign_towards(candidate);
        if candidate_sign == -sign {
            break;
        }
        months = candidate_months;
        mid = candidate;
        if candidate_sign == 0 {
            break;
        }
    }

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(mid);
    if largest_unit == DateUnit::Month {
        (0, months + years * 12, 0, days)
    } else {
        (years, months, 0, days)
    }
}

/// Adds `count` whole `unit`s to `date` in `calendar` (constrain mode) — the
/// single-unit specialization of [`calendar_add_date`] the rounding step
/// below walks by.
fn calendar_add_unit(calendar: AnyCalendarKind, date: CivilDate, unit: DateUnit, count: i64) -> CivilDate {
    let (years, months, weeks, days) = match unit {
        DateUnit::Year => (count, 0, 0, 0),
        DateUnit::Month => (0, count, 0, 0),
        DateUnit::Week => (0, 0, count, 0),
        DateUnit::Day => (0, 0, 0, count),
    };
    calendar_add_date(calendar, date, years, months, weeks, days, false)
        .expect("constrain-mode single-unit addition always succeeds")
}

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
/// from `start` lands on `lower`; one more lands on `upper`; `end`'s exact
/// fractional position (in epoch days) between them is the basis for
/// rounding.
#[allow(clippy::too_many_arguments)]
fn round_month_or_year(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    unit: DateUnit,
    count: i64,
    sign: i64,
    increment: i128,
    mode: blueice_ecma402::NumberRoundingMode,
) -> i64 {
    let lower = calendar_add_unit(calendar, start, unit, count);
    let upper = calendar_add_unit(calendar, start, unit, count + sign);
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
    let final_magnitude = if round_up { upper_multiple } else { lower_multiple };
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
        DateUnit::Month => {
            let total_months = if largest_unit == DateUnit::Year {
                years * 12 + months
            } else {
                months
            };
            let rounded_months = round_month_or_year(
                calendar,
                start,
                end,
                DateUnit::Month,
                total_months,
                sign,
                increment,
                mode,
            );
            if largest_unit == DateUnit::Year {
                (rounded_months / 12, rounded_months % 12, 0, 0)
            } else {
                (0, rounded_months, 0, 0)
            }
        }
        DateUnit::Year => {
            let rounded_years =
                round_month_or_year(calendar, start, end, DateUnit::Year, years, sign, increment, mode);
            (rounded_years, 0, 0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_iso_day_of_week_matching_a_known_monday() {
        // Test262's PlainDate/prototype/dayOfWeek/basic.js: 1976-11-15 is a
        // Monday (dayOfWeek 1) and the week runs through 1976-11-21 (7).
        for offset in 1_u8..=7 {
            assert_eq!(iso_day_of_week((1976, 11, 14 + offset)), offset);
        }
    }

    #[test]
    fn computes_iso_week_of_year_across_a_year_boundary() {
        // Test262's PlainDate/prototype/weekOfYear/basic.js.
        for day in 29..=31 {
            assert_eq!(iso_week_of_year((1975, 12, day)), (1, 1976));
        }
        for day in 1..=4 {
            assert_eq!(iso_week_of_year((1976, 1, day)), (1, 1976));
        }
        for day in 27..=31 {
            assert_eq!(iso_week_of_year((1976, 12, day)), (53, 1976));
        }
        for day in 1..=2 {
            assert_eq!(iso_week_of_year((1977, 1, day)), (53, 1976));
        }
    }

    #[test]
    fn reports_days_and_leap_years_matching_known_facts() {
        assert!(is_iso_leap_year(1976));
        assert!(!is_iso_leap_year(1977));
        assert!(is_iso_leap_year(2000));
        assert!(!is_iso_leap_year(1900));
        assert_eq!(iso_days_in_month(2020, 2), 29);
        assert_eq!(iso_days_in_month(2021, 2), 28);
    }

    #[test]
    fn round_trips_epoch_days_across_a_range_of_dates() {
        for date in [
            (1970, 1, 1),
            (2000, 2, 29),
            (1969, 12, 31),
            (1976, 11, 18),
            (-271_821, 4, 19),
            (275_760, 9, 13),
        ] {
            assert_eq!(epoch_days_to_iso_date(iso_date_to_epoch_days(date)), date);
        }
    }

    #[test]
    fn adds_years_months_before_weeks_and_days_like_the_spec_pins() {
        // PlainDate/prototype/add/basic.js's own worked examples.
        assert_eq!(
            add_iso_date((1976, 11, 18), 43, 0, 0, 0, false),
            Some((2019, 11, 18))
        );
        assert_eq!(
            add_iso_date((1976, 11, 18), 0, 3, 0, 0, false),
            Some((1977, 2, 18))
        );
        assert_eq!(
            add_iso_date((1976, 11, 18), 0, 0, 0, 20, false),
            Some((1976, 12, 8))
        );
        assert_eq!(
            add_iso_date((2019, 1, 31), 0, 1, 0, 0, false),
            Some((2019, 2, 28))
        );
        assert_eq!(
            add_iso_date((2020, 2, 29), 1, 0, 0, 0, false),
            Some((2021, 2, 28))
        );
        assert_eq!(
            add_iso_date((2020, 2, 29), 4, 0, 0, 0, false),
            Some((2024, 2, 29))
        );
    }

    #[test]
    fn rejects_an_overflowing_day_only_in_reject_mode() {
        assert_eq!(add_iso_date((2019, 1, 31), 0, 1, 0, 0, true), None);
        assert_eq!(
            add_iso_date((2019, 1, 31), 0, 1, 0, 0, false),
            Some((2019, 2, 28))
        );
    }

    #[test]
    fn differences_two_iso_dates_by_largest_unit() {
        assert_eq!(
            difference_iso_date((1976, 11, 18), (2019, 11, 18), DateUnit::Year),
            (43, 0, 0, 0)
        );
        assert_eq!(
            difference_iso_date((1976, 11, 18), (2019, 11, 18), DateUnit::Month),
            (0, 43 * 12, 0, 0)
        );
        assert_eq!(
            difference_iso_date((2021, 2, 19), (2021, 3, 8), DateUnit::Day),
            (0, 0, 0, 17)
        );
        assert_eq!(
            difference_iso_date((2021, 2, 19), (2021, 3, 8), DateUnit::Week),
            (0, 0, 2, 3)
        );
    }

    #[test]
    fn differences_are_the_inverse_of_add_iso_date() {
        for (start, end, unit) in [
            ((1976, 11, 18), (2019, 11, 18), DateUnit::Year),
            ((2019, 1, 31), (2019, 2, 28), DateUnit::Month),
            ((2021, 2, 19), (2021, 3, 8), DateUnit::Day),
            ((2021, 2, 19), (2021, 3, 8), DateUnit::Week),
            ((2019, 11, 18), (1976, 11, 18), DateUnit::Year),
        ] {
            let (years, months, weeks, days) = difference_iso_date(start, end, unit);
            assert_eq!(
                add_iso_date(start, years, months, weeks, days, false),
                Some(end),
                "{start:?} -> {end:?} via {unit:?}: {years} {months} {weeks} {days}"
            );
        }
    }

    #[test]
    fn formats_iso_dates_with_the_six_digit_extended_year_rule() {
        assert_eq!(format_iso_date((2000, 5, 2)), "2000-05-02");
        assert_eq!(format_iso_date((-271_821, 4, 19)), "-271821-04-19");
        assert_eq!(format_iso_date((275_760, 9, 13)), "+275760-09-13");
    }

    #[test]
    fn formats_calendar_annotations_per_the_showcalendar_option() {
        assert_eq!(format_calendar_annotation("iso8601", ShowCalendar::Auto), "");
        assert_eq!(
            format_calendar_annotation("iso8601", ShowCalendar::Always),
            "[u-ca=iso8601]"
        );
        assert_eq!(
            format_calendar_annotation("iso8601", ShowCalendar::Critical),
            "[!u-ca=iso8601]"
        );
        assert_eq!(format_calendar_annotation("iso8601", ShowCalendar::Never), "");
        assert_eq!(
            format_calendar_annotation("hebrew", ShowCalendar::Auto),
            "[u-ca=hebrew]"
        );
    }

    #[test]
    fn calendar_add_date_matches_the_pure_iso_fast_path_for_iso8601() {
        assert_eq!(
            calendar_add_date(AnyCalendarKind::Iso, (1976, 11, 18), 43, 0, 0, 0, false),
            add_iso_date((1976, 11, 18), 43, 0, 0, 0, false)
        );
    }

    #[test]
    fn rounds_calendar_durations_matching_a_real_test262_fixture() {
        // Test262's PlainDate/prototype/until/roundingmode-ceil.js: 2019-01-08
        // until 2021-09-07, rounded up (roundingMode "ceil") to each
        // smallestUnit in turn, with largestUnit implicitly bumped to match.
        use blueice_ecma402::NumberRoundingMode::Ceil;
        let start = (2019, 1, 8);
        let end = (2021, 9, 7);
        for (unit, expected_positive, expected_negative) in [
            (DateUnit::Year, (3, 0, 0, 0), (-2, 0, 0, 0)),
            (DateUnit::Month, (0, 32, 0, 0), (0, -31, 0, 0)),
            (DateUnit::Week, (0, 0, 139, 0), (0, 0, -139, 0)),
            (DateUnit::Day, (0, 0, 0, 973), (0, 0, 0, -973)),
        ] {
            assert_eq!(
                round_calendar_duration(AnyCalendarKind::Iso, start, end, unit, unit, 1, Ceil),
                expected_positive,
                "{unit:?} positive"
            );
            assert_eq!(
                round_calendar_duration(AnyCalendarKind::Iso, end, start, unit, unit, 1, Ceil),
                expected_negative,
                "{unit:?} negative"
            );
        }
    }

    #[test]
    fn rounding_an_exact_multiple_of_the_larger_unit_adds_no_spurious_remainder() {
        // Test262's PlainDate/prototype/since/exact-multiple-of-larger-unit.js:
        // a `{ largestUnit, smallestUnit }` pair where the *unrounded*
        // difference is already an exact whole `largestUnit` (here, exactly
        // one month/one year) must report that exactly, in every rounding
        // mode — not a `smallestUnit`-sized wobble computed by bubbling
        // `smallestUnit` steps from scratch.
        use blueice_ecma402::NumberRoundingMode::{Ceil, Expand, Floor, HalfEven, HalfExpand, Trunc};
        let start = (2012, 1, 1);
        for mode in [Ceil, Floor, Expand, Trunc, HalfExpand, HalfEven] {
            assert_eq!(
                round_calendar_duration(
                    AnyCalendarKind::Iso,
                    start,
                    (2012, 2, 1),
                    DateUnit::Month,
                    DateUnit::Week,
                    1,
                    mode
                ),
                (0, 1, 0, 0),
                "P1M weeks..months {mode:?}"
            );
            assert_eq!(
                round_calendar_duration(
                    AnyCalendarKind::Iso,
                    start,
                    (2013, 1, 1),
                    DateUnit::Year,
                    DateUnit::Month,
                    1,
                    mode
                ),
                (1, 0, 0, 0),
                "P1Y months..years {mode:?}"
            );
        }
    }

    #[test]
    fn calendar_add_date_carries_years_in_a_non_iso_calendar() {
        // Gregorian is a 12-month solar calendar offset from ISO by no
        // fields at all, so adding a year should land on the same ISO
        // month/day one calendar year later (calendar_add_date must not
        // silently no-op for a non-ISO AnyCalendarKind).
        let result =
            calendar_add_date(AnyCalendarKind::Gregorian, (2020, 3, 1), 1, 0, 0, 0, false);
        assert_eq!(result, Some((2021, 3, 1)));
    }
}
