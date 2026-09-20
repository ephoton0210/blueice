// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `CalendarDateAdd` for every closed calendar ID: [`calendar_add_date`] and its
//! leap-month (`chinese`/`dangi`/`hebrew`) branch, extending `AddISODate`'s
//! shape via `icu_calendar`. No `Value`/heap/Realm coupling.

use super::super::calendar::calendar_date_from_civil;
use super::super::epoch::CivilDate;
use super::iso_date::{add_iso_date, balance_iso_date};
use super::month_structure::{
    calendar_date_from_month, calendar_date_from_ordinal, calendar_has_leap_months,
    calendar_month_identity, months_in_year_for,
};
use icu_calendar::options::{DateFromFieldsOptions, Overflow as IcuOverflow};
use icu_calendar::types::{DateFields, Month};
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, Iso};

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
    let cal_date = calendar_date_from_civil(calendar, date);
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

/// The `CalendarHasLeapMonths` branch of Gecko's `AddYearMonthDuration`:
/// adds `years` directly (months per year is variable, so only `months`
/// needs calendar dispatch), then — if `months != 0` — re-resolves the
/// *anchor's own* `Month` identity (`anchor_month`, i.e. its `monthCode`) in
/// the `years`-shifted landing year first via [`calendar_date_from_month`]
/// (honoring the caller's `overflow` exactly as [`calendar_date_from_month`]'s
/// own doc comment describes — a leap month may not recur, which `Reject`
/// must refuse even though a `months` component follows: Gecko regulates the
/// year-shifted month with the real `overflow` whether or not `months` is
/// zero, and `add`/`subtract/leap-month-*-numerical-months.js` pins
/// `P1Y1M` throwing just like `P1Y`; the difference algorithm always passes
/// `Constrain`), and only then
/// bubbles `months` by ordinal position, one whole year at a time, through
/// [`calendar_date_from_ordinal`] — never re-deriving a `monthCode` mid-walk,
/// only at the very end. `day` is deliberately not threaded through here at
/// all (Gecko's own version carries it unregulated to the final result);
/// callers needing a resolved day construct it themselves afterward via
/// [`calendar_date_from_month`].
pub(super) fn add_year_month_duration_leap_month(
    calendar: AnyCalendarKind,
    anchor_year: i64,
    anchor_month: Month,
    years: i64,
    months: i64,
    overflow: IcuOverflow,
) -> Option<(i64, Month)> {
    let mut year = anchor_year + years;
    let mut month = anchor_month;
    if months != 0 {
        let mut first_day_of_month = calendar_date_from_month(calendar, year, month, 1, overflow)?;
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
/// add-side counterpart to [`calendar_difference_date_leap_month`](super::calendar_difference::calendar_difference_date_leap_month), and the
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
/// fallback applies (see that function's own doc comment: `icu_calendar`'s
/// own native, per-calendar fallback is the single real convention Gecko's
/// own `ConstrainMonthCode` uses for every caller, add side and difference
/// side alike). `weeks`/`days` fold back in as a flat ISO day offset
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
    let overflow = if reject {
        IcuOverflow::Reject
    } else {
        IcuOverflow::Constrain
    };
    let (year, month) = add_year_month_duration_leap_month(
        calendar,
        anchor_year,
        anchor_month,
        years,
        months,
        overflow,
    )?;
    let landed = calendar_date_from_month(calendar, year, month, anchor_day, overflow)?;
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
