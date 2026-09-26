// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure ISO-calendar civil-date math: the ISO week-date getters and the
//! spec's `AddISODate`/`BalanceISODate`/`RegulateISODate` abstract operations,
//! plus the raw-tuple comparison ([`compare_date_tuple`]/[`surpasses`]) that
//! every estimate-then-correct difference algorithm in this module tree
//! shares. No calendar dispatch and no `Value`/heap/Realm coupling.

use super::super::epoch::CivilDate;
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
/// `days_from_civil` calculation [`super::super::epoch::nanoseconds_since_epoch`]
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
    let (year, month, day) =
        regulate_iso_date(carried_year, carried_month, i64::from(date.2), reject)?;
    Some(balance_iso_date(
        year,
        month,
        i64::from(day) + days + weeks * 7,
    ))
}

/// Raw, possibly out-of-range `(year, month, day)` tuple comparison —
/// `CompareISODate`/`CompareCalendarDate` as Gecko defines them: plain
/// lexicographic comparison with **no** per-field validity check (a `day` of
/// 29 in a 28-day month, or a `month` of 13, compares exactly as its numeric
/// value would). This is deliberately not [`compare_iso_date`]'s `CivilDate`
/// (whose `u8` fields cannot even represent an out-of-range candidate) —
/// callers here need to compare a not-yet-regulated intermediate candidate
/// against a real date, which [`ISODateSurpasses`]/[`surpasses`] needs to do
/// *before* any constraining happens, per Gecko's own `DifferenceISODate`/
/// `DifferenceNonISODate`.
pub(super) fn compare_date_tuple(a: (i64, i64, i64), b: (i64, i64, i64)) -> Ordering {
    a.cmp(&b)
}

/// `ISODateSurpasses`/`CompareSurpasses ( sign, one, two )`: whether `one`
/// has gone past `two` in the `sign` direction — the test every
/// estimate-then-correct difference algorithm below uses to detect an
/// overshoot, always against the *raw* (possibly invalid) candidate tuple.
pub(super) fn surpasses(sign: i64, one: (i64, i64, i64), two: (i64, i64, i64)) -> bool {
    let cmp = match compare_date_tuple(one, two) {
        Ordering::Less => -1_i64,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    cmp * sign > 0
}

#[cfg(test)]
mod tests {
    use super::iso_days_in_month;

    #[test]
    #[should_panic(expected = "Temporal ISO months are regulated to 1..=12")]
    fn an_unregulated_iso_month_violates_the_internal_invariant() {
        iso_days_in_month(2024, 13);
    }
}
