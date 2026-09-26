// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How a calendar's months are laid out, shared by calendar addition,
//! difference and rounding: which calendars line up with ISO or carry leap
//! months, conversions between an ISO civil date and a calendar's
//! `(year, ordinal month, day)` / `(year, Month, day)` identity, and the
//! `Month`-identity comparison the leap-month algorithms use (Gecko's
//! `MonthCode` ordering).

use super::super::calendar::calendar_date_from_civil;
use super::super::epoch::CivilDate;
use super::iso_date::compare_date_tuple;
use icu_calendar::options::{DateFromFieldsOptions, Overflow as IcuOverflow};
use icu_calendar::types::{DateFields, Month};
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, Iso};
use std::cmp::Ordering;

/// `months_in_year` for a probe date at `(year, 1, 1)` in `calendar` — used
/// by [`calendar_add_date`](super::calendar_add::calendar_add_date) to discover how many months a *landing* year has
/// before carrying into it, since that can vary by year for a lunisolar
/// calendar's leap months.
pub(super) fn months_in_year_for(calendar: AnyCalendarKind, year: i32) -> Option<u8> {
    let mut fields = DateFields::default();
    fields.extended_year = Some(year);
    fields.ordinal_month = Some(1);
    fields.day = Some(1);
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(IcuOverflow::Constrain);
    let date = Date::try_from_fields(fields, options, AnyCalendar::new(calendar)).ok()?;
    Some(date.months_in_year())
}

/// Whether `calendar`'s year boundaries and month lengths line up exactly
/// with the ISO 8601 calendar's own — Gecko's `NonISODateUntil` dispatch
/// (`js/src/builtin/temporal/Calendar.cpp`) routes these straight through
/// `DifferenceISODate` on the value's *raw stored ISO fields*, never through
/// `icu_calendar` at all for difference purposes: `Buddhist`/`Japanese`/
/// `Roc` are the ISO calendar under a different era/year label, and
/// `Gregorian` (Temporal's `"gregory"`, distinct from `"iso8601"`) *is* the
/// ISO calendar's own proleptic-Gregorian date structure.
pub(super) fn calendar_uses_iso_date_arithmetic(calendar: AnyCalendarKind) -> bool {
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
/// rather than [`calendar_difference_date_fixed_months`](super::calendar_difference::calendar_difference_date_fixed_months)'s constant `12`,
/// matching Gecko's own `CalendarHasLeapMonths` split between
/// `DifferenceNonISODate` and `DifferenceNonISODateWithLeapMonth`.
pub(super) fn calendar_has_leap_months(calendar: AnyCalendarKind) -> bool {
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
pub(super) fn to_calendar_ordinal(calendar: AnyCalendarKind, date: CivilDate) -> (i64, i64, i64) {
    let cal_date = calendar_date_from_civil(calendar, date);
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
pub(super) fn calendar_ordinal_to_iso(
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
    fields.day = Some(day.clamp(1, 31) as u8);
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

/// `(extended_year, Month, day)` for a representable ISO civil date in
/// `calendar` — the leap-month-aware analog of [`to_calendar_ordinal`],
/// carrying the [`Month`] identity (number *and* leap flag, i.e. Gecko's own
/// `MonthCode`) instead of a flattened ordinal position. `icu_calendar`'s
/// `Month`/`MonthInfo` type already *is* Temporal's `monthCode` concept —
/// `Month::new(4)` <-> `"M04"`, `Month::leap(4)` <-> `"M04L"` — so no
/// separate month-code string type is needed here.
pub(super) fn calendar_month_identity(
    calendar: AnyCalendarKind,
    date: CivilDate,
) -> (i64, Month, i64) {
    let cal_date = calendar_date_from_civil(calendar, date);
    (
        i64::from(cal_date.year().extended_year()),
        cal_date.month().to_input(),
        i64::from(cal_date.day_of_month().0),
    )
}

/// Builds a calendar `Date` from an explicit `(year, Month, day)` identity,
/// honoring `overflow` — Gecko's `CreateDateFromCodes`. When `month` is a
/// leap month that does not recur in `year`, `icu_calendar`'s own
/// `Date::try_from_fields` already applies the same per-calendar fallback
/// Gecko's `ConstrainMonthCode` hand-codes (confirmed directly against
/// `components/calendar/src/cal/east_asian_traditional.rs`/`hebrew.rs`, not
/// assumed), so no separate fallback table is needed here: `Chinese`/
/// `Dangi` drop the leap flag and keep the same month number (`M04L` ->
/// `M04`, `intl402/Temporal/PlainDate/prototype/add/leap-months-chinese.js`'s
/// "Adding 1 year to leap month M03L lands in common-year M03"), while
/// `Hebrew`'s Adar I (`M05L`) resolves to Adar II (`M06`,
/// `intl402/Temporal/PlainDate/prototype/add/leap-months-hebrew.js`'s own
/// worked example) — both verified directly against Gecko's own
/// `js/src/builtin/temporal/Calendar.cpp`'s `ConstrainMonthCode`, which uses
/// this exact same single, calendar-dependent rule for *every* caller (both
/// `AddYearMonthDuration`'s add side and `DifferenceNonISODateWithLeapMonth`'s
/// difference side route through it identically — there is only one
/// fallback rule in Gecko's own source, not a different one per caller).
///
/// An earlier version of this module instead hand-rolled a *second*, uniform
/// "pick the next month" fallback for [`calendar_difference_date_leap_month`](super::calendar_difference::calendar_difference_date_leap_month)
/// specifically, on the theory that trusting `icu_calendar`'s own fallback
/// there couldn't reproduce
/// `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js`'s
/// "M04L-M04 backwards is -12mo not -1y" case. That theory was wrong: the
/// real bug was a *missing pre-check* in that function's own years-correction
/// step (see its own doc comment for the two-step correction Gecko's
/// `DifferenceNonISODateWithLeapMonth` actually performs), not the fallback
/// convention — with that pre-check restored, trusting `icu_calendar`'s own
/// fallback (as this function already did for the add side) reproduces every
/// named `leap-months-{chinese,dangi,hebrew}.js` `since`/`until` fixture
/// correctly too, confirmed directly against the pinned Test262 corpus.
pub(super) fn calendar_date_from_month(
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
    fields.day = Some(day.clamp(1, 31) as u8);
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(overflow);
    Date::try_from_fields(fields, options, AnyCalendar::new(calendar)).ok()
}

/// Builds a calendar `Date` (not yet converted to ISO) from an explicit
/// `(year, ordinal_month, day)` triple, always in constrain mode — the
/// ordinal-position probe [`add_year_month_duration_leap_month`](super::calendar_add::add_year_month_duration_leap_month) uses once
/// it has already crossed into a fresh year (where, per Gecko's own
/// `AddYearMonthDuration`, only the *count* of months matters, not any
/// particular month's identity).
pub(super) fn calendar_date_from_ordinal(
    calendar: AnyCalendarKind,
    year: i64,
    ordinal_month: i64,
    day: i64,
) -> Option<Date<AnyCalendar>> {
    let year = i32::try_from(year).ok()?;
    let ordinal_month = u8::try_from(ordinal_month).ok()?;
    let mut fields = DateFields::default();
    fields.extended_year = Some(year);
    fields.ordinal_month = Some(ordinal_month);
    fields.day = Some(day.clamp(1, 31) as u8);
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
/// `(i64, i64, i64)`-tuple [`compare_date_tuple`]/[`surpasses`](super::iso_date::surpasses) machinery
/// directly instead of introducing a second comparison path.
fn month_sort_key(month: Month) -> i64 {
    i64::from(month.number()) * 2 + i64::from(month.is_leap())
}

/// `CompareCalendarDate ( one, two )`: like [`compare_date_tuple`], but for
/// a `(year, Month, day)` identity rather than a raw `(year, month, day)`
/// ordinal tuple.
fn compare_calendar_identity(a: (i64, Month, i64), b: (i64, Month, i64)) -> Ordering {
    compare_date_tuple(
        (a.0, month_sort_key(a.1), a.2),
        (b.0, month_sort_key(b.1), b.2),
    )
}

/// [`surpasses`](super::iso_date::surpasses), specialized to a `(year, Month, day)` identity.
pub(super) fn surpasses_identity(
    sign: i64,
    one: (i64, Month, i64),
    two: (i64, Month, i64),
) -> bool {
    let cmp = match compare_calendar_identity(one, two) {
        Ordering::Less => -1_i64,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    cmp * sign > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_constructors_reject_unrepresentable_years_and_months() {
        let outside_year = i64::from(i32::MAX) + 1;
        let outside_month = i64::from(u8::MAX) + 1;
        let calendar = AnyCalendarKind::Iso;

        assert!(calendar_ordinal_to_iso(calendar, outside_year, 1, 1).is_none());
        assert!(calendar_ordinal_to_iso(calendar, 2024, outside_month, 1).is_none());
        // This year fits in i32 but lies outside ICU's supported calendar range.
        assert!(calendar_ordinal_to_iso(calendar, i64::from(i32::MAX), 1, 1).is_none());
        assert!(calendar_date_from_month(
            calendar,
            outside_year,
            Month::new(1),
            1,
            IcuOverflow::Reject
        )
        .is_none());
        assert!(calendar_date_from_ordinal(calendar, outside_year, 1, 1).is_none());
        assert!(calendar_date_from_ordinal(calendar, 2024, outside_month, 1).is_none());
        assert!(
            calendar_date_from_month(calendar, 2024, Month::new(13), 1, IcuOverflow::Reject)
                .is_none()
        );
    }
}
