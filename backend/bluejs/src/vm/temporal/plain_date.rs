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
use icu_calendar::types::{DateFields, Month};
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
fn compare_date_tuple(a: (i64, i64, i64), b: (i64, i64, i64)) -> Ordering {
    a.cmp(&b)
}

/// `ISODateSurpasses`/`CompareSurpasses ( sign, one, two )`: whether `one`
/// has gone past `two` in the `sign` direction — the test every
/// estimate-then-correct difference algorithm below uses to detect an
/// overshoot, always against the *raw* (possibly invalid) candidate tuple.
fn surpasses(sign: i64, one: (i64, i64, i64), two: (i64, i64, i64)) -> bool {
    let cmp = match compare_date_tuple(one, two) {
        Ordering::Less => -1_i64,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    cmp * sign > 0
}

/// `DifferenceISODate(y1, m1, d1, y2, m2, d2, largestUnit)`: the calendar
/// duration `(years, months, weeks, days)` — signed, all the same sign as
/// `end - start` — such that `AddISODate(start, duration) == end`. Ported
/// directly from Gecko's `DifferenceISODate`
/// (`js/src/builtin/temporal/Calendar.cpp`): `years`/`months` are each a
/// direct field subtraction (`end.year - start.year`, `end.month -
/// start.month`), corrected by *at most one* step apiece by comparing an
/// **unconstrained** `(year, month, start.day)` candidate against `end` —
/// not a `start.day`-constrained landing date. That distinction is load-
/// bearing, not cosmetic: constraining the candidate first (e.g. via
/// [`regulate_iso_date`]) before comparing it hides exactly the "wrapping at
/// the end of a month" case Test262 pins
/// (`intl402/Temporal/PlainDate/prototype/since/wrapping-at-end-of-month-*.js`
/// — `Jan 29 -> Feb 28` must report `{ days: -30 }`, not `{ months: -1 }`,
/// because the *unconstrained* `Jan 29 + 1 month = Feb 29` candidate does
/// surpass `Feb 28`, even though `Feb 29` constrained down to `Feb 28`
/// would not). This replaces an earlier estimate-via-day-span-then-bubble-
/// one-month-at-a-time implementation that constrained every candidate
/// before comparing it, which is what let that class of case through.
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

    let (y1, m1, d1) = (i64::from(start.0), i64::from(start.1), i64::from(start.2));
    let two = (i64::from(end.0), i64::from(end.1), i64::from(end.2));

    let mut years = two.0 - y1;
    let mut months = two.1 - m1;

    if surpasses(sign, (y1 + years, m1, d1), two) {
        years -= sign;
        months += 12 * sign;
    }

    let (iy, im) = balance_iso_year_month(y1 + years, m1 + months);
    if surpasses(sign, (iy, i64::from(im), d1), two) {
        months -= sign;
    }

    if largest_unit == DateUnit::Month {
        months += years * 12;
        years = 0;
    }

    let (by, bm) = balance_iso_year_month(y1 + years, m1 + months);
    // A landing year outside i32 cannot occur for any representable
    // Temporal date pair, so this only ever clamps a same-year overflow.
    let by = i32::try_from(by).unwrap_or(if by > 0 { i32::MAX } else { i32::MIN });
    let constrained =
        regulate_iso_date(by, bm, d1, false).expect("constrain-mode regulation always succeeds");

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(constrained);
    (years, months, 0, days)
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
    if calendar_has_leap_months(calendar) {
        return calendar_add_date_leap_month(calendar, date, years, months, weeks, days, reject);
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

/// Whether `calendar`'s year boundaries and month lengths line up exactly
/// with the ISO 8601 calendar's own — Gecko's `NonISODateUntil` dispatch
/// (`js/src/builtin/temporal/Calendar.cpp`) routes these straight through
/// `DifferenceISODate` on the value's *raw stored ISO fields*, never through
/// `icu_calendar` at all for difference purposes: `Buddhist`/`Japanese`/
/// `Roc` are the ISO calendar under a different era/year label, and
/// `Gregorian` (Temporal's `"gregory"`, distinct from `"iso8601"`) *is* the
/// ISO calendar's own proleptic-Gregorian date structure.
fn calendar_uses_iso_date_arithmetic(calendar: AnyCalendarKind) -> bool {
    matches!(
        calendar,
        AnyCalendarKind::Iso
            | AnyCalendarKind::Gregorian
            | AnyCalendarKind::Buddhist
            | AnyCalendarKind::Japanese
            | AnyCalendarKind::Roc
    )
}

/// Whether `calendar` can insert a leap *month* (as opposed to only a leap
/// *day*) in some years — the three lunisolar calendars in this project's
/// closed 16-ID set. These need a variable `monthsPerYear` per landing year
/// rather than [`calendar_difference_date_fixed_months`]'s constant `12`,
/// matching Gecko's own `CalendarHasLeapMonths` split between
/// `DifferenceNonISODate` and `DifferenceNonISODateWithLeapMonth`.
fn calendar_has_leap_months(calendar: AnyCalendarKind) -> bool {
    matches!(
        calendar,
        AnyCalendarKind::Chinese | AnyCalendarKind::Dangi | AnyCalendarKind::Hebrew
    )
}

/// `ToCalendarDateWithOrdinalMonth`: the calendar-specific `(extended_year,
/// ordinal_month, day)` triple for a representable ISO civil date. Ordinal
/// month (not a `monthCode` string) is sufficient for every comparison this
/// module needs it for, since it is monotonic within a single calendar year
/// by construction (`temporal_calendar_fields`'s own doc comment: "a leap
/// month therefore increments every following ordinal").
fn to_calendar_ordinal(calendar: AnyCalendarKind, date: CivilDate) -> (i64, i64, i64) {
    let iso = Date::try_new_iso(date.0, date.1, date.2)
        .expect("a representable Temporal ISO date always converts to any calendar");
    let cal_date = iso.to_calendar(AnyCalendar::new(calendar));
    (
        i64::from(cal_date.year().extended_year()),
        i64::from(cal_date.month().ordinal),
        i64::from(cal_date.day_of_month().0),
    )
}

/// `CreateDateFrom(..., TemporalOverflow::Constrain)`: builds a
/// representable ISO civil date from a calendar-ordinal `(year,
/// ordinal_month, day)` triple, constraining an out-of-range `day` down to
/// the landing month's own length. `ordinal_month` must already be
/// normalized to `1..=` that year's own month count — this never carries a
/// month overflow itself.
fn calendar_ordinal_to_iso(
    calendar: AnyCalendarKind,
    year: i64,
    ordinal_month: i64,
    day: i64,
) -> Option<CivilDate> {
    let year = i32::try_from(year).ok()?;
    let ordinal_month = u8::try_from(ordinal_month).ok()?;
    let mut fields = DateFields::default();
    fields.extended_year = Some(year);
    fields.ordinal_month = Some(ordinal_month);
    fields.day = Some(u8::try_from(day.clamp(1, 31)).ok()?);
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(IcuOverflow::Constrain);
    let landed = Date::try_from_fields(fields, options, AnyCalendar::new(calendar)).ok()?;
    let landed_iso = landed.to_calendar(Iso);
    Some((
        landed_iso.year().extended_year(),
        landed_iso.month().number(),
        landed_iso.day_of_month().0,
    ))
}

/// `DifferenceNonISODate`: the fixed-`monthsPerYear = 12` generalization of
/// [`difference_iso_date`] for every non-ISO-aligned calendar without leap
/// months (`Coptic`/`Ethiopian`/`EthiopianAmeteAlem`/`Indian`/the three
/// Hijri variants/`Persian`) — same direct-subtraction-then-two-corrections
/// shape, just carrying year/month in the target calendar's own numbering
/// via [`to_calendar_ordinal`]/[`calendar_ordinal_to_iso`] instead of the
/// ISO fields directly.
fn calendar_difference_date_fixed_months(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    const MONTHS_PER_YEAR: i64 = 12;
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };
    let (y1, m1, d1) = to_calendar_ordinal(calendar, start);
    let two = to_calendar_ordinal(calendar, end);

    let mut years = two.0 - y1;
    let mut months = two.1 - m1;

    if surpasses(sign, (y1 + years, m1, d1), two) {
        years -= sign;
        months += MONTHS_PER_YEAR * sign;
    }

    // Gecko's own `DifferenceNonISODate`/`DifferenceISODate` normalize this
    // intermediate with a single `if > monthsPerYear {} else if < 1 {}` step
    // rather than a full modulo, which is only sound if the correction
    // above bounds `months` tightly enough that one step always suffices.
    // It does not for every calendar/date pair this engine's own
    // `to_calendar_ordinal` can produce (confirmed by a real panic on
    // `intl402/Temporal/PlainDate/prototype/since/basic-indian.js`, where a
    // single step left `bm` still outside `1..=12`) — a full `div_euclid`/
    // `rem_euclid` normalize (mirroring [`balance_iso_year_month`],
    // parameterized on `MONTHS_PER_YEAR` instead of hardcoding 12) is
    // strictly safer and exactly as correct for the in-range case.
    let normalize = |year: i64, month: i64| -> (i64, i64) {
        let zero_based = month - 1;
        (
            year + zero_based.div_euclid(MONTHS_PER_YEAR),
            zero_based.rem_euclid(MONTHS_PER_YEAR) + 1,
        )
    };

    let (by, bm) = normalize(y1 + years, m1 + months);
    if surpasses(sign, (by, bm, d1), two) {
        months -= sign;
    }

    if largest_unit == DateUnit::Month {
        months += years * MONTHS_PER_YEAR;
        years = 0;
    }

    let (fby, fbm) = normalize(y1 + years, m1 + months);
    let constrained = calendar_ordinal_to_iso(calendar, fby, fbm, d1)
        .expect("constrain-mode regulation always succeeds for a representable date");

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(constrained);
    (years, months, 0, days)
}

/// `(extended_year, Month, day)` for a representable ISO civil date in
/// `calendar` — the leap-month-aware analog of [`to_calendar_ordinal`],
/// carrying the [`Month`] identity (number *and* leap flag, i.e. Gecko's own
/// `MonthCode`) instead of a flattened ordinal position. `icu_calendar`'s
/// `Month`/`MonthInfo` type already *is* Temporal's `monthCode` concept —
/// `Month::new(4)` <-> `"M04"`, `Month::leap(4)` <-> `"M04L"` — so no
/// separate month-code string type is needed here.
fn calendar_month_identity(calendar: AnyCalendarKind, date: CivilDate) -> (i64, Month, i64) {
    let iso = Date::try_new_iso(date.0, date.1, date.2)
        .expect("a representable Temporal ISO date always converts to any calendar");
    let cal_date = iso.to_calendar(AnyCalendar::new(calendar));
    (
        i64::from(cal_date.year().extended_year()),
        cal_date.month().to_input(),
        i64::from(cal_date.day_of_month().0),
    )
}

/// Which convention to use when a requested leap [`Month`] does not recur in
/// the target year — Gecko's `CreateDateFromCodes` (`Calendar.cpp`'s
/// `CalendarError::UnknownMonthCode` branch) documents this exact ambiguity
/// with its own comment: it ships one *uniform* rule for every leap
/// calendar, "pick the next month" (`min(monthCode.ordinal() + 1, 12)`,
/// non-leap — e.g. Chinese/Dangi's `M04L` becomes `M05`, Hebrew's Adar I
/// `M05L` becomes Adar `M06`), but flags an explicit `TODO`: *"Temporal spec
/// polyfill replaces M03L with M03 for Chinese/Dangi. No idea what are the
/// 'cultural conventions' for these two calendars..."* — i.e. Gecko's own
/// authors are not confident a single uniform rule is even correct.
///
/// Confirmed empirically against this project's own pinned Test262 corpus
/// that it is not, and that the fix is *not* a second uniform rule either —
/// [`calendar_add_date`]'s own leap-month branch needs a *calendar-specific*
/// answer that in fact already matches `icu_calendar`'s own native,
/// per-calendar `Constrain` behavior with **no override at all**: `Chinese`/
/// `Dangi`'s shared `EastAsianTraditional` implementation
/// (`components/calendar/src/cal/east_asian_traditional.rs`) drops the leap
/// flag and keeps the same month number (`M03L` -> `M03`,
/// `intl402/Temporal/PlainDate/prototype/add/leap-months-chinese.js`'s
/// "Adding 1 year to leap month M03L lands in common-year M03"), while
/// `Hebrew`'s own `ordinal_from_month`
/// (`components/calendar/src/cal/hebrew.rs`) already implements Gecko's
/// "pick the next month" rule (`M05L` -> `M06`,
/// `intl402/Temporal/PlainDate/prototype/add/leap-months-hebrew.js`'s
/// "Adding 1 year to Adar I (M05L) lands in common-year Adar (M06) with
/// constrain") — verified directly against both sources, not assumed, and
/// against both fixtures independently (each calendar is its own evidence;
/// neither generalizes from the other). [`calendar_difference_date_leap_month`]'s
/// own years-correction probe, on the other hand, needs the *uniform*
/// `PickNextMonth` override for all three calendars regardless of what
/// `icu_calendar` natively does (verified against
/// `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js`'s
/// "M04L-M04 backwards is -12mo not -1y": under `Native` this instead
/// computes `-1y`, the very regression that pass's own rewrite fixed, since
/// `Chinese`/`Dangi`'s own native `SameNumberDropLeap`-equivalent behavior
/// reproduces the bug). Three real, independently-verified conventions
/// across two operations and three calendars — not a guess generalized from
/// one case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LeapMonthFallback {
    /// `min(monthCode.number() + 1, 12)`, non-leap — [`calendar_difference_date_leap_month`]'s
    /// own convention, applied uniformly regardless of calendar.
    PickNextMonth,
    /// Trust whichever fallback `icu_calendar`'s own `Date::try_from_fields`
    /// already applies under `Overflow::Constrain` for this specific
    /// calendar — [`calendar_add_date`]'s own convention (see this enum's
    /// own doc comment for why this, despite being calendar-dependent, is
    /// correct for every one of the three leap calendars).
    Native,
}

impl LeapMonthFallback {
    fn resolve(self, month: Month) -> Month {
        match self {
            LeapMonthFallback::PickNextMonth => Month::new((month.number() + 1).min(12)),
            // `calendar_date_from_month` short-circuits `Native` before ever
            // calling this — see that function's own body.
            LeapMonthFallback::Native => month,
        }
    }
}

/// Builds a calendar `Date` from an explicit `(year, Month, day)` identity,
/// honoring `overflow` — Gecko's `CreateDateFromCodes`, generalized with
/// [`LeapMonthFallback`] since (unlike Gecko) this project's own evidence
/// shows no single fallback rule is correct for every caller.
/// [`calendar_date_from_month_exact`] does the actual field-resolution
/// work; under [`LeapMonthFallback::Native`] this is a thin, un-overridden
/// pass-through to it. Under [`LeapMonthFallback::PickNextMonth`], this
/// wrapper instead detects a "month doesn't exist this year" outcome via a
/// day-independent `Reject`-mode existence probe (day-overflow and
/// month-non-existence are Gecko's own two structurally distinct error
/// cases — `UnknownMonthCode` vs. `OutOfRange` — and this keeps them
/// separate exactly the same way) and applies that override itself under
/// `Overflow::Constrain`, rather than trusting whichever behavior the
/// underlying calendar implementation happens to have.
fn calendar_date_from_month(
    calendar: AnyCalendarKind,
    year: i64,
    month: Month,
    day: i64,
    overflow: IcuOverflow,
    fallback: LeapMonthFallback,
) -> Option<Date<AnyCalendar>> {
    if fallback == LeapMonthFallback::Native {
        return calendar_date_from_month_exact(calendar, year, month, day, overflow);
    }
    if month.is_leap() && calendar_date_from_month_exact(calendar, year, month, 1, IcuOverflow::Reject).is_none() {
        // The requested leap month does not occur in `year` at all (a pure
        // day-1 existence probe, independent of the real `day`/`overflow`
        // this call ultimately needs).
        if overflow == IcuOverflow::Reject {
            return None;
        }
        return calendar_date_from_month_exact(calendar, year, fallback.resolve(month), day, overflow);
    }
    calendar_date_from_month_exact(calendar, year, month, day, overflow)
}

fn calendar_date_from_month_exact(
    calendar: AnyCalendarKind,
    year: i64,
    month: Month,
    day: i64,
    overflow: IcuOverflow,
) -> Option<Date<AnyCalendar>> {
    let year = i32::try_from(year).ok()?;
    let mut fields = DateFields::default();
    fields.extended_year = Some(year);
    fields.month = Some(month);
    fields.day = Some(u8::try_from(day.clamp(1, 31)).ok()?);
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(overflow);
    Date::try_from_fields(fields, options, AnyCalendar::new(calendar)).ok()
}

/// Builds a calendar `Date` (not yet converted to ISO) from an explicit
/// `(year, ordinal_month, day)` triple, always in constrain mode — the
/// ordinal-position probe [`add_year_month_duration_leap_month`] uses once
/// it has already crossed into a fresh year (where, per Gecko's own
/// `AddYearMonthDuration`, only the *count* of months matters, not any
/// particular month's identity).
fn calendar_date_from_ordinal(calendar: AnyCalendarKind, year: i64, ordinal_month: i64, day: i64) -> Option<Date<AnyCalendar>> {
    let year = i32::try_from(year).ok()?;
    let ordinal_month = u8::try_from(ordinal_month).ok()?;
    let mut fields = DateFields::default();
    fields.extended_year = Some(year);
    fields.ordinal_month = Some(ordinal_month);
    fields.day = Some(u8::try_from(day.clamp(1, 31)).ok()?);
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(IcuOverflow::Constrain);
    Date::try_from_fields(fields, options, AnyCalendar::new(calendar)).ok()
}

/// A single `i64` sort key for a [`Month`] that orders exactly the way
/// Gecko's own `MonthCode::operator<` does: by `number()` first, with a
/// leap month sorting immediately after its non-leap counterpart of the
/// same number (`M04 < M04L < M05`) — matches `icu_calendar`'s own derived
/// `Month: PartialOrd` (whose field order is `(number, is_leap)`), spelled
/// out explicitly here so it can feed this module's existing
/// `(i64, i64, i64)`-tuple [`compare_date_tuple`]/[`surpasses`] machinery
/// directly instead of introducing a second comparison path.
fn month_sort_key(month: Month) -> i64 {
    i64::from(month.number()) * 2 + i64::from(month.is_leap())
}

/// `CompareCalendarDate ( one, two )`: like [`compare_date_tuple`], but for
/// a `(year, Month, day)` identity rather than a raw `(year, month, day)`
/// ordinal tuple.
fn compare_calendar_identity(a: (i64, Month, i64), b: (i64, Month, i64)) -> Ordering {
    compare_date_tuple((a.0, month_sort_key(a.1), a.2), (b.0, month_sort_key(b.1), b.2))
}

/// [`surpasses`], specialized to a `(year, Month, day)` identity.
fn surpasses_identity(sign: i64, one: (i64, Month, i64), two: (i64, Month, i64)) -> bool {
    let cmp = match compare_calendar_identity(one, two) {
        Ordering::Less => -1_i64,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    cmp * sign > 0
}

/// The `CalendarHasLeapMonths` branch of Gecko's `AddYearMonthDuration`:
/// adds `years` directly (months per year is variable, so only `months`
/// needs calendar dispatch), then — if `months != 0` — re-resolves the
/// *anchor's own* `Month` identity (`anchor_month`, i.e. its `monthCode`) in
/// the `years`-shifted landing year first via [`calendar_date_from_month`]
/// (honoring `Overflow::Constrain` exactly as [`calendar_date_from_month`]'s
/// own doc comment describes — a leap month may not recur), and only then
/// bubbles `months` by ordinal position, one whole year at a time, through
/// [`calendar_date_from_ordinal`] — never re-deriving a `monthCode` mid-walk,
/// only at the very end. `day` is deliberately not threaded through here at
/// all (Gecko's own version carries it unregulated to the final result);
/// callers needing a resolved day construct it themselves afterward via
/// [`calendar_date_from_month`]. `fallback` is threaded through to that
/// initial re-resolution only — see [`LeapMonthFallback`]'s own doc comment
/// for why callers on the add side and the difference side need different
/// conventions here.
fn add_year_month_duration_leap_month(
    calendar: AnyCalendarKind,
    anchor_year: i64,
    anchor_month: Month,
    years: i64,
    months: i64,
    fallback: LeapMonthFallback,
) -> Option<(i64, Month)> {
    let mut year = anchor_year + years;
    let mut month = anchor_month;
    if months != 0 {
        let mut first_day_of_month = calendar_date_from_month(calendar, year, month, 1, IcuOverflow::Constrain, fallback)?;
        let mut remaining = months;
        if remaining > 0 {
            loop {
                let ordinal = i64::from(first_day_of_month.month().ordinal);
                let months_in_year = i64::from(first_day_of_month.months_in_year());
                if ordinal + remaining <= months_in_year {
                    break;
                }
                remaining -= months_in_year - ordinal + 1;
                year += 1;
                first_day_of_month = calendar_date_from_ordinal(calendar, year, 1, 1)?;
            }
        } else {
            loop {
                let ordinal = i64::from(first_day_of_month.month().ordinal);
                if ordinal + remaining >= 1 {
                    break;
                }
                remaining += ordinal;
                year -= 1;
                let months_in_year =
                    i64::from(months_in_year_for(calendar, i32::try_from(year).ok()?)?);
                first_day_of_month = calendar_date_from_ordinal(calendar, year, months_in_year, 1)?;
            }
        }
        let final_ordinal = i64::from(first_day_of_month.month().ordinal) + remaining;
        let final_day = calendar_date_from_ordinal(calendar, year, final_ordinal, 1)?;
        month = final_day.month().to_input();
    }
    Some((year, month))
}

/// `AddNonISODate`'s leap-month branch (`chinese`/`dangi`/`hebrew`) — the
/// add-side counterpart to [`calendar_difference_date_leap_month`], and the
/// fix for this module's own previously-documented gap: `calendar_add_date`
/// used to carry `years`/`months` through flat ordinal position for *every*
/// non-ISO calendar, which is wrong for a leap-month calendar the same way
/// the difference side was (a leap month's ordinal position shifts year to
/// year). `years`/`months` are carried through the anchor's own `Month`
/// identity instead, via [`add_year_month_duration_leap_month`] (reusing the
/// exact same year/month-bubbling machinery the difference side already
/// verified), then the day is regulated in the resulting month via
/// [`calendar_date_from_month`] honoring the caller's real `reject`/
/// `constrain` overflow — this is also where a non-recurring leap month's
/// [`LeapMonthFallback::Native`] convention applies (see that enum's own
/// doc comment for why trusting `icu_calendar`'s own per-calendar fallback,
/// and not [`LeapMonthFallback::PickNextMonth`], is the add side's
/// convention). `weeks`/`days` fold back in as a flat ISO day offset
/// afterward, exactly like the non-leap-month path in [`calendar_add_date`].
fn calendar_add_date_leap_month(
    calendar: AnyCalendarKind,
    date: CivilDate,
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    reject: bool,
) -> Option<CivilDate> {
    let (anchor_year, anchor_month, anchor_day) = calendar_month_identity(calendar, date);
    let (year, month) = add_year_month_duration_leap_month(
        calendar,
        anchor_year,
        anchor_month,
        years,
        months,
        LeapMonthFallback::Native,
    )?;
    let overflow = if reject {
        IcuOverflow::Reject
    } else {
        IcuOverflow::Constrain
    };
    let landed = calendar_date_from_month(
        calendar,
        year,
        month,
        anchor_day,
        overflow,
        LeapMonthFallback::Native,
    )?;
    let landed_iso = landed.to_calendar(Iso);
    let landed_civil: CivilDate = (
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

/// The non-ISO generalization of [`difference_iso_date`] for the three
/// leap-month calendars (`Chinese`/`Dangi`/`Hebrew`), where `monthsPerYear`
/// varies by year so [`calendar_difference_date_fixed_months`]'s constant-12
/// carry does not apply. Ported directly from Gecko's own
/// `DifferenceNonISODate`'s `CalendarHasLeapMonths` branch
/// (`js/src/builtin/temporal/Calendar.cpp`): every candidate is compared by
/// **`Month` identity** (`(year, Month, day)`, i.e. Gecko's own
/// `CalendarDate`/`MonthCode`), not by raw ordinal month — the fix over the
/// previous estimate-then-bubble-by-ordinal implementation this replaces,
/// which could misorder whenever the two years being compared put their
/// leap month in a different position (`intl402/Temporal/PlainDate/
/// prototype/since/leap-months-{chinese,dangi,hebrew}.js`).
fn calendar_difference_date_leap_month(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };

    let one = calendar_month_identity(calendar, start);
    let two = calendar_month_identity(calendar, end);

    let mut years = two.0 - one.0;

    // Years-only correction: resolve `one`'s own Month identity in the
    // `one.year + years` landing year (constrain mode — a leap month like
    // `M05L` genuinely may not recur every year), and back off by one year
    // if that surpasses `two`. Unlike `difference_iso_date`'s own
    // years-correction, this candidate *is* day-regulated at this step —
    // ported exactly as Gecko has it, since resolving a non-existent
    // `monthCode` (not merely an out-of-range day) is the thing being
    // guarded against here.
    let constrained0 = calendar_date_from_month(
        calendar,
        one.0 + years,
        one.1,
        one.2,
        IcuOverflow::Constrain,
        LeapMonthFallback::PickNextMonth,
    )
    .expect("constrain-mode regulation always succeeds for a representable date");
    let mut constrained: (i64, Month, i64) = (
        i64::from(constrained0.year().extended_year()),
        constrained0.month().to_input(),
        i64::from(constrained0.day_of_month().0),
    );
    if surpasses_identity(sign, constrained, two) {
        years -= sign;
    }

    // Add as many months as possible without surpassing `two`, bubbling one
    // month *of identity* at a time (unlike the ordinal-based version this
    // replaces, a leap month's differing position across years can't
    // misorder this — every candidate is built by re-resolving `one`'s own
    // `Month` in the target year, then walking by ordinal position only
    // within already-identity-resolved years).
    let mut months = 0_i64;
    while let Some((candidate_year, candidate_month)) =
        add_year_month_duration_leap_month(calendar, one.0, one.1, years, months + sign, LeapMonthFallback::PickNextMonth)
    {
        // `day` carries through unregulated here (Gecko's own
        // `AddYearMonthDuration` leaves it as the anchor's raw `day`),
        // matching `difference_iso_date`'s own "compare an unconstrained
        // candidate" rule for detecting a month-end overshoot correctly.
        let candidate = (candidate_year, candidate_month, one.2);
        if surpasses_identity(sign, candidate, two) {
            break;
        }
        months += sign;
        constrained = candidate;
    }

    if largest_unit == DateUnit::Month && years != 0 {
        let start_cal = Date::try_new_iso(start.0, start.1, start.2)
            .expect("a representable Temporal ISO date always converts to any calendar")
            .to_calendar(AnyCalendar::new(calendar));
        let months_until_end_of_year = |date: &Date<AnyCalendar>| -> i64 {
            i64::from(date.months_in_year()) - i64::from(date.month().ordinal) + 1
        };
        let months_since_start_of_year =
            |date: &Date<AnyCalendar>| -> i64 { i64::from(date.month().ordinal) - 1 };

        if sign > 0 {
            months += months_until_end_of_year(&start_cal);
        } else {
            months -= months_since_start_of_year(&start_cal);
        }

        // Months in each fully-crossed intervening year, using that year's
        // own real month count (not a fixed constant).
        let mut y = sign;
        while y != years {
            let probe_year = i32::try_from(one.0 + y).unwrap_or(if y > 0 { i32::MAX } else { i32::MIN });
            if let Some(count) = months_in_year_for(calendar, probe_year) {
                months += i64::from(count) * sign;
            }
            y += sign;
        }

        // Months since/until the landing year's own start/end, from `one`'s
        // own Month identity re-resolved in that year.
        if let Some(dt) = calendar_date_from_month(
            calendar,
            one.0 + years,
            one.1,
            1,
            IcuOverflow::Constrain,
            LeapMonthFallback::PickNextMonth,
        ) {
            if sign > 0 {
                months += months_since_start_of_year(&dt);
            } else {
                months -= months_until_end_of_year(&dt);
            }
        }
        years = 0;
    }

    let final_probe = calendar_date_from_month(
        calendar,
        constrained.0,
        constrained.1,
        constrained.2,
        IcuOverflow::Constrain,
        LeapMonthFallback::PickNextMonth,
    )
    .expect("constrain-mode regulation always succeeds for a representable date");
    let final_iso = final_probe.to_calendar(Iso);
    let constrained_iso: CivilDate = (
        final_iso.year().extended_year(),
        final_iso.month().number(),
        final_iso.day_of_month().0,
    );

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(constrained_iso);
    (years, months, 0, days)
}

/// The non-ISO generalization of [`difference_iso_date`]: dispatches to
/// whichever of [`calendar_uses_iso_date_arithmetic`],
/// [`calendar_difference_date_fixed_months`] or
/// [`calendar_difference_date_leap_month`] matches `calendar`, per Gecko's
/// own `NonISODateUntil` three-way split. `week`/`day` `largestUnit` is
/// always calendar-invariant pure ISO epoch-day math (every supported
/// calendar uses a 7-day week and every concrete date has exactly one ISO
/// form), matching Gecko's own "delegate to the ISO 8601 calendar for
/// weeks/days" shortcut.
pub(crate) fn calendar_difference_date(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    if calendar_uses_iso_date_arithmetic(calendar) {
        return difference_iso_date(start, end, largest_unit);
    }
    if !matches!(largest_unit, DateUnit::Year | DateUnit::Month) {
        let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(start);
        return if largest_unit == DateUnit::Week {
            (0, 0, days / 7, days % 7)
        } else {
            (0, 0, 0, days)
        };
    }
    if calendar_has_leap_months(calendar) {
        calendar_difference_date_leap_month(calendar, start, end, largest_unit)
    } else {
        calendar_difference_date_fixed_months(calendar, start, end, largest_unit)
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

    /// Regression for a real bug in this module's earlier estimate-then-
    /// bubble `difference_iso_date`/`calendar_difference_date`, found via
    /// Test262's
    /// `intl402/Temporal/PlainDate/prototype/since/wrapping-at-end-of-month-*.js`:
    /// `Jan 29 -> Feb 28` (a non-leap year) must report a 30-day difference,
    /// **not** one month, because the *unconstrained* `Jan 29 + 1 month =
    /// Feb 29` candidate surpasses `Feb 28`, even though `Feb 29`
    /// constrained down to `Feb 28` would land exactly on it. The earlier
    /// implementation compared an already-`calendar_add_date`-constrained
    /// candidate (which silently clips `Feb 29` to `Feb 28` before the
    /// comparison ever happens) and got this wrong. Covers both the pure
    /// ISO path (`difference_iso_date`) and the non-ISO-aligned fixed-
    /// months path (`calendar_difference_date` with `Gregorian`, which
    /// `wrapping-at-end-of-month-*.js`'s own `buddhist`/`gregory`/etc.
    /// variants exercise) since both were rewritten together.
    #[test]
    fn month_difference_does_not_constrain_before_detecting_an_end_of_month_overshoot() {
        for unit in [DateUnit::Year, DateUnit::Month] {
            assert_eq!(
                difference_iso_date((2020, 1, 29), (2020, 2, 28), unit),
                (0, 0, 0, 30),
                "ISO Jan 29 -> Feb 28, {unit:?}"
            );
            assert_eq!(
                calendar_difference_date(AnyCalendarKind::Gregorian, (2020, 1, 29), (2020, 2, 28), unit),
                (0, 0, 0, 30),
                "Gregorian Jan 29 -> Feb 28, {unit:?}"
            );
            assert_eq!(
                calendar_difference_date(AnyCalendarKind::Persian, (2020, 1, 29), (2020, 2, 28), unit),
                calendar_difference_date_fixed_months(AnyCalendarKind::Persian, (2020, 1, 29), (2020, 2, 28), unit),
                "Persian (fixed-months path) Jan 29 -> Feb 28 is internally consistent, {unit:?}"
            );
        }
        // Jan 30 -> Feb 28 is 29 days (one day closer than Jan 29's case),
        // and Jan 31 -> Feb 28 is 28 days -- both from the same fixture,
        // pinning the exact day-count, not just "not a whole month".
        assert_eq!(
            difference_iso_date((2020, 1, 30), (2020, 2, 28), DateUnit::Year),
            (0, 0, 0, 29)
        );
        assert_eq!(
            difference_iso_date((2020, 1, 31), (2020, 2, 28), DateUnit::Year),
            (0, 0, 0, 28)
        );
    }

    /// Regression for a real panic in an earlier draft of
    /// [`calendar_difference_date_fixed_months`]: its intermediate
    /// year/month normalization used a single `if bm > 12 {} else if bm < 1
    /// {}` step (mirroring Gecko's own `DifferenceNonISODate`), which is
    /// only sound if the preceding correction bounds the candidate month
    /// tightly enough that one step always suffices -- Gecko's own
    /// production code apparently relies on invariants this port's simpler
    /// `MONTHS_PER_YEAR`-only (no monthCode) representation doesn't
    /// preserve. Found via a real crash on
    /// `intl402/Temporal/PlainDate/prototype/since/basic-indian.js`
    /// ("negative 61 years, 3 months and 17 days", `date19430716` to
    /// `date18820330`), not by inspection. Fixed by a full `div_euclid`/
    /// `rem_euclid` normalize instead of the single-step version. This test
    /// exercises every fixed-months calendar across a multi-decade span
    /// with a large year delta, the shape that triggered the crash.
    #[test]
    fn fixed_months_difference_never_panics_across_a_large_multi_decade_span() {
        for calendar in [
            AnyCalendarKind::Coptic,
            AnyCalendarKind::Ethiopian,
            AnyCalendarKind::EthiopianAmeteAlem,
            AnyCalendarKind::Indian,
            AnyCalendarKind::HijriTabularTypeIIFriday,
            AnyCalendarKind::HijriTabularTypeIIThursday,
            AnyCalendarKind::HijriUmmAlQura,
            AnyCalendarKind::Persian,
        ] {
            for unit in [DateUnit::Year, DateUnit::Month] {
                // date19430716 -> date18820330, the exact pair
                // `basic-indian.js` panicked on.
                let (years, months, weeks, days) =
                    calendar_difference_date(calendar, (1943, 7, 16), (1882, 3, 30), unit);
                assert_eq!(weeks, 0);
                // Every field should be non-positive (end is before start)
                // and the whole-duration sign should be consistent with a
                // backward difference.
                assert!(years <= 0 && months <= 0 && days <= 0, "{calendar:?} {unit:?}: {years} {months} {days}");
            }
        }
    }

    /// Builds the ISO [`CivilDate`] for a leap-month calendar's own
    /// `(year, Month, day)` identity, for use as test input — thin wrapper
    /// around [`calendar_date_from_month`] so these tests never need a
    /// hand-computed ISO date.
    fn civil_date_from_month_code(calendar: AnyCalendarKind, year: i64, month: Month, day: i64) -> CivilDate {
        let date = calendar_date_from_month(calendar, year, month, day, IcuOverflow::Reject, LeapMonthFallback::Native)
            .expect("test fixture month codes are always valid for their stated year");
        let iso = date.to_calendar(Iso);
        (iso.year().extended_year(), iso.month().number(), iso.day_of_month().0)
    }

    /// Regression for the real bug this module's leap-month rewrite fixes:
    /// `icu_calendar`'s own `EastAsianTraditional` (`Chinese`/`Dangi`)
    /// `ordinal_from_month` falls back to the *same* month number for a
    /// leap month that doesn't recur in the requested year (`M04L` ->
    /// `M04`), but Gecko's own `CreateDateFromCodes` — and the actual
    /// Test262 fixtures — require "the next month" (`M04L` -> `M05`).
    /// Confirmed directly, not assumed: constructing `2002` (a common
    /// year) with `Month::leap(4)` under `Constrain` must resolve to
    /// ordinal 5 (`M05`, matching a plain `Month::new(5)` request), not
    /// ordinal 4. Under `Reject`, the same request must fail outright.
    #[test]
    fn calendar_date_from_month_falls_back_to_the_next_month_for_a_leap_month_that_does_not_recur() {
        let expected = calendar_date_from_month(
            AnyCalendarKind::Chinese,
            2002,
            Month::new(5),
            1,
            IcuOverflow::Reject,
            LeapMonthFallback::PickNextMonth,
        )
        .expect("M05 always exists");
        let constrained = calendar_date_from_month(
            AnyCalendarKind::Chinese,
            2002,
            Month::leap(4),
            1,
            IcuOverflow::Constrain,
            LeapMonthFallback::PickNextMonth,
        )
        .expect("a non-recurring leap month still resolves under Constrain");
        assert_eq!(constrained.month().ordinal, expected.month().ordinal);
        assert_eq!(
            constrained.to_calendar(Iso).day_of_month().0,
            expected.to_calendar(Iso).day_of_month().0
        );

        assert!(
            calendar_date_from_month(
                AnyCalendarKind::Chinese,
                2002,
                Month::leap(4),
                1,
                IcuOverflow::Reject,
                LeapMonthFallback::PickNextMonth,
            )
            .is_none(),
            "a non-recurring leap month must be rejected under Overflow::Reject"
        );

        // `M12L`'s fallback caps at 12 (stays within the same year) rather
        // than wrapping to a 13th month.
        let capped = calendar_date_from_month(
            AnyCalendarKind::Chinese,
            2002,
            Month::leap(12),
            1,
            IcuOverflow::Constrain,
            LeapMonthFallback::PickNextMonth,
        )
        .expect("M12L's fallback must still resolve");
        assert_eq!(capped.month().ordinal, 12);
    }

    /// A leap month that *does* recur in the requested year must resolve to
    /// its own genuine leap-month ordinal, not the fallback — 2001 is a
    /// real Chinese leap year with an `M04L` (per
    /// `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js`'s
    /// own comment), landing between `M04` (ordinal 4) and `M05` (ordinal
    /// 6, since the leap month itself is ordinal 5).
    #[test]
    fn calendar_date_from_month_resolves_a_genuinely_recurring_leap_month_to_its_own_ordinal() {
        let leap = calendar_date_from_month(
            AnyCalendarKind::Chinese,
            2001,
            Month::leap(4),
            1,
            IcuOverflow::Reject,
            LeapMonthFallback::PickNextMonth,
        )
        .expect("2001 has a real M04L");
        assert_eq!(leap.month().ordinal, 5);
        assert!(leap.month().to_input().is_leap());
    }

    /// [`add_year_month_duration_leap_month`] must preserve the anchor's own
    /// `Month` identity across a pure-`years` shift (no `months` component)
    /// — adding one year to `M04`(2000) lands on `M04`(2001), the same
    /// `monthCode`, even though 2001 inserts a leap month right after it.
    #[test]
    fn add_year_month_duration_leap_month_preserves_identity_across_a_pure_year_shift() {
        let (year, month) = add_year_month_duration_leap_month(
            AnyCalendarKind::Chinese,
            2000,
            Month::new(4),
            1,
            0,
            LeapMonthFallback::PickNextMonth,
        )
        .expect("a representable in-range shift always succeeds");
        assert_eq!((year, month), (2001, Month::new(4)));
    }

    /// [`add_year_month_duration_leap_month`] bubbling by one month from a
    /// leap year's own `M04` must land on that same year's `M04L` (the
    /// month immediately following it that year), not `M05` — pinning that
    /// the month-bubbling walk resolves by real ordinal position within an
    /// already-year-resolved date, not by skipping straight to the next
    /// non-leap `monthCode`.
    #[test]
    fn add_year_month_duration_leap_month_bubbles_into_the_leap_month_itself() {
        let (year, month) = add_year_month_duration_leap_month(
            AnyCalendarKind::Chinese,
            2000,
            Month::new(4),
            1,
            1,
            LeapMonthFallback::PickNextMonth,
        )
        .expect("a representable in-range shift always succeeds");
        assert_eq!(year, 2001);
        assert_eq!(month, Month::leap(4));
    }

    /// [`calendar_add_date`]'s own leap-month branch, host-neutral layer:
    /// adding 1 year to `M03L`(1966) under `Constrain` lands on `M03`(1967)
    /// -- `LeapMonthFallback::Native`, not `PickNextMonth` -- matching
    /// `intl402/Temporal/PlainDate/prototype/add/leap-months-chinese.js`'s
    /// own worked example (the VM-level integration test in
    /// `backend/bluejs/tests/temporal_leap_month_calendar_add.rs` exercises
    /// the same fact through the real `Temporal.PlainDate.prototype.add`
    /// surface, including the `overflow: "reject"` throw this test's
    /// `Constrain` case does not cover).
    #[test]
    fn calendar_add_date_leap_month_constrains_a_non_recurring_leap_month_to_the_same_number() {
        let anchor = civil_date_from_month_code(AnyCalendarKind::Chinese, 1966, Month::leap(3), 1);
        let landed = calendar_add_date(AnyCalendarKind::Chinese, anchor, 1, 0, 0, 0, false)
            .expect("constrain-mode add always succeeds for a representable date");
        let (year, month, day) = calendar_month_identity(AnyCalendarKind::Chinese, landed);
        assert_eq!((year, month, day), (1967, Month::new(3), 1));

        assert_eq!(
            calendar_add_date(AnyCalendarKind::Chinese, anchor, 1, 0, 0, 0, true),
            None,
            "overflow: reject must throw when M03L does not recur in the landing year"
        );
    }

    /// The same scenario on `hebrew`: adding 1 year to Adar I (`M05L`, 5784)
    /// under `Constrain` lands on Adar (`M06`, 5785) -- the *next* month,
    /// not `M05` -- since `icu_calendar`'s own `Hebrew::ordinal_from_month`
    /// natively implements that convention (unlike `Chinese`/`Dangi`'s
    /// shared `EastAsianTraditional` implementation, exercised above).
    /// Matches `intl402/Temporal/PlainDate/prototype/add/leap-months-hebrew.js`'s
    /// "Adding 1 year to Adar I (M05L) lands in common-year Adar (M06) with
    /// constrain" and its `overflow: "reject"` throw.
    #[test]
    fn calendar_add_date_leap_month_hebrew_picks_the_next_month_for_a_non_recurring_leap_month() {
        let anchor = civil_date_from_month_code(AnyCalendarKind::Hebrew, 5784, Month::leap(5), 1);
        let landed = calendar_add_date(AnyCalendarKind::Hebrew, anchor, 1, 0, 0, 0, false)
            .expect("constrain-mode add always succeeds for a representable date");
        let (year, month, day) = calendar_month_identity(AnyCalendarKind::Hebrew, landed);
        assert_eq!((year, month, day), (5785, Month::new(6), 1));

        assert_eq!(
            calendar_add_date(AnyCalendarKind::Hebrew, anchor, 1, 0, 0, 0, true),
            None,
            "overflow: reject must throw when Adar I does not recur in the landing year"
        );
    }

    /// [`calendar_add_date`]'s leap-month branch preserves `monthCode`
    /// identity across a pure-year shift when the leap month *does* recur
    /// (2012's `M04L` to 2020's own `M04L`, 8 years later) -- the same
    /// "Adding years to go from one M04L to the next M04L" fixture case.
    #[test]
    fn calendar_add_date_leap_month_preserves_identity_when_the_leap_month_recurs() {
        let anchor = civil_date_from_month_code(AnyCalendarKind::Chinese, 2012, Month::leap(4), 1);
        let landed = calendar_add_date(AnyCalendarKind::Chinese, anchor, 8, 0, 0, 0, true)
            .expect("2020 has a real M04L, so reject mode must succeed");
        let (year, month, day) = calendar_month_identity(AnyCalendarKind::Chinese, landed);
        assert_eq!((year, month, day), (2020, Month::leap(4), 1));
    }

    /// [`calendar_add_date`]'s leap-month branch bubbles a `months`
    /// component by real ordinal position within an already-identity-
    /// resolved year, landing correctly on the leap month itself --
    /// "adding 2 months to M03 in leap year lands in M04L (leap month)".
    #[test]
    fn calendar_add_date_leap_month_bubbles_months_into_a_leap_month() {
        let anchor = civil_date_from_month_code(AnyCalendarKind::Chinese, 2020, Month::new(3), 1);
        let landed = calendar_add_date(AnyCalendarKind::Chinese, anchor, 0, 2, 0, 0, true)
            .expect("landing on the real M04L must succeed under reject");
        let (year, month, day) = calendar_month_identity(AnyCalendarKind::Chinese, landed);
        assert_eq!((year, month, day), (2020, Month::leap(4), 1));
    }

    /// [`calendar_difference_date_leap_month`] reproduces
    /// `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js`'s
    /// own worked examples directly (the same values the VM-level
    /// `backend/bluejs/tests/temporal_leap_month_calendar_difference.rs`
    /// integration test exercises through the real `Temporal.PlainDate`
    /// surface) — this module's own host-neutral, no-VM-required layer.
    #[test]
    fn calendar_difference_date_leap_month_matches_the_real_test262_fixture_values() {
        let calendar = AnyCalendarKind::Chinese;
        let common1_month4 = civil_date_from_month_code(calendar, 2000, Month::new(4), 1);
        let leap_month4 = civil_date_from_month_code(calendar, 2001, Month::new(4), 1);
        let leap_month4l = civil_date_from_month_code(calendar, 2001, Month::leap(4), 1);
        let common2_month4 = civil_date_from_month_code(calendar, 2002, Month::new(4), 1);

        // "M04-M04 common-leap backwards is -1y" / "-12mo".
        assert_eq!(
            calendar_difference_date(calendar, leap_month4, common1_month4, DateUnit::Year),
            (-1, 0, 0, 0)
        );
        assert_eq!(
            calendar_difference_date(calendar, leap_month4, common1_month4, DateUnit::Month),
            (0, -12, 0, 0)
        );

        // The fixture's "M04L-M04 backwards is -12mo not -1y" is `since`'s
        // own (negated) value; `calendar_difference_date`'s direct,
        // un-negated `start`-to-`end` (`until`-style) direction for the
        // same pair (2001-M04L forward to 2002-M04, chronologically
        // forward, so positive) is `+12` months, `0` years -- the real bug
        // this module's rewrite fixes: the previous ordinal-based
        // comparison computed `+1y`/`0mo` here instead.
        assert_eq!(
            calendar_difference_date(calendar, leap_month4l, common2_month4, DateUnit::Year),
            (0, 12, 0, 0)
        );
        assert_eq!(
            calendar_difference_date(calendar, leap_month4l, common2_month4, DateUnit::Month),
            (0, 12, 0, 0)
        );

        // The fixture's "M04-M04L backwards is -1y -1mo" is `since`'s
        // negation of the forward direction; `calendar_difference_date`
        // itself always computes the forward (`until`-style)
        // `start`-to-`end` direction, so the un-negated value here is
        // `common1Month4` (2000-M04) to `leapMonth4L` (2001-M04L): `+1y
        // +1mo`.
        assert_eq!(
            calendar_difference_date(calendar, common1_month4, leap_month4l, DateUnit::Year),
            (1, 1, 0, 0)
        );
    }
}
