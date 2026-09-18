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

/// The non-ISO generalization of [`difference_iso_date`] used only for the
/// three leap-month calendars (`Chinese`/`Dangi`/`Hebrew`), where
/// `monthsPerYear` varies by year so [`calendar_difference_date_fixed_months`]'s
/// constant-12 carry does not apply. This keeps the estimate-then-bubble
/// shape the fixed-months/ISO paths used before this module's Gecko-ported
/// rewrite, **still using an unconstrained candidate for the years-estimate
/// surpass check** (the part that rewrite fixed generally), but bubbling
/// months one at a time through the already-constraining [`calendar_add_date`]
/// rather than porting Gecko's own `DifferenceNonISODateWithLeapMonth`
/// (which compares by `monthCode`, not ordinal month, specifically so an
/// ordinal-month comparison across two years with a *different* leap-month
/// position can't silently misorder — a real, narrower gap than the
/// constrain-before-compare bug this module's other rewrites close).
/// Documented, not silent: `development/browser_core/phase-26-ecma262-temporal/PLAN.md`
/// tracks completing this to match Gecko's `monthCode`-based algorithm
/// exactly as a follow-up.
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

    let land = |years: i64, months: i64| -> CivilDate {
        calendar_add_date(calendar, start, years, months, 0, 0, false)
            .expect("constrain-mode calendar regulation always succeeds")
    };

    let (y1, m1, d1) = to_calendar_ordinal(calendar, start);
    let two = to_calendar_ordinal(calendar, end);

    // Unconstrained years-only correction: `start`'s own ordinal month/day
    // carried into `y1 + years` without re-resolving them against that
    // year's own month structure. This only risks misordering when `y1 +
    // years == two.0` *and* that year's leap-month position differs from
    // `start`'s own year — see this function's own doc comment.
    let mut years = two.0 - y1;
    if surpasses(sign, (y1 + years, m1, d1), two) {
        years -= sign;
    }

    let mut months = 0_i64;
    let mut mid = land(years, 0);
    loop {
        let candidate_months = months + sign;
        let candidate = land(years, candidate_months);
        let candidate_ordinal = to_calendar_ordinal(calendar, candidate);
        // Use the *unconstrained* day (`d1`, `start`'s own) for the surpass
        // test rather than `candidate_ordinal`'s already-day-constrained
        // one, per [`difference_iso_date`]'s own doc comment on why this
        // matters.
        let candidate_raw = (candidate_ordinal.0, candidate_ordinal.1, d1);
        if surpasses(sign, candidate_raw, two) {
            break;
        }
        months = candidate_months;
        mid = candidate;
        if compare_date_tuple(candidate_raw, two) == Ordering::Equal {
            break;
        }
    }

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(mid);
    if largest_unit == DateUnit::Month {
        // Fold years into months using each crossed year's own real month
        // count (`months_in_year_for`), not a fixed constant.
        let mut total_months = months;
        let mut probe_year = y1;
        let mut remaining = years;
        while remaining != 0 {
            let step = if remaining > 0 { 1 } else { -1 };
            let this_year = if step > 0 { probe_year } else { probe_year - 1 };
            let count = i64::from(
                months_in_year_for(calendar, i32::try_from(this_year).unwrap_or(if this_year > 0 {
                    i32::MAX
                } else {
                    i32::MIN
                }))
                .unwrap_or(12),
            );
            total_months += count * step;
            probe_year += step;
            remaining -= step;
        }
        (0, total_months, 0, days)
    } else {
        (years, months, 0, days)
    }
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
}
