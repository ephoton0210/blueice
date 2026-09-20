// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Temporal.PlainYearMonth` calendar-field resolution (Phase 26
//! Stage 2's `plain_year_month.rs`/`plain_month_day.rs` slice,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `PlainYearMonth`'s date arithmetic (`add`/`subtract`/`until`/`since`)
//! reuses [`super::plain_date`]'s calendar-agnostic date math directly --
//! `calendar_add_date`/`calendar_difference_date` (and
//! `plain_date_time_difference`'s rounding on top of them) all already operate
//! on a plain ISO `(year, month, day)` triple with no
//! notion of which Temporal type stores it, so this module only adds what is
//! genuinely new for this type: `CalendarYearMonthFromFields` (resolving a
//! year+month calendar-field bag -- deliberately with no `day` field -- to
//! the calendar's own reference day for that month) and this type's own
//! `toString` shape.

use super::epoch::CivilDate;
use super::plain_date::{format_calendar_annotation, format_iso_date, ShowCalendar};
use icu_calendar::options::{
    DateFromFieldsOptions, MissingFieldsStrategy, Overflow as IcuOverflow,
};
use icu_calendar::types::DateFields;
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, Iso};

/// The calendar fields `CalendarYearMonthFromFields` reads: either an
/// `(era, eraYear)` pair or a plain `extendedYear` identifies the year, and
/// either `monthCode` or `ordinalMonth` identifies the month. `day` is
/// deliberately never a field here -- it is always derived, never supplied.
#[derive(Default)]
pub(crate) struct YearMonthFields<'a> {
    pub era: Option<&'a str>,
    pub era_year: Option<i32>,
    pub extended_year: Option<i32>,
    pub month_code: Option<&'a str>,
    pub ordinal_month: Option<u8>,
}

/// `CalendarYearMonthFromFields`: resolves year+month fields to a concrete
/// ISO date at the calendar's reference day for that month.
///
/// `icu_calendar`'s `MissingFieldsStrategy::Ecma` sets `day` to `1` whenever
/// a year and a month are both present but no day is -- exactly Temporal's
/// own reference-day rule, for every calendar (not only `iso8601`); see
/// `icu_calendar::options::MissingFieldsStrategy`'s own doc comment and
/// `Date::try_from_fields`'s doctests, which this module's own behavior was
/// verified against directly.
pub(crate) fn year_month_from_fields(
    calendar: AnyCalendarKind,
    fields: &YearMonthFields,
    reject: bool,
) -> Result<CivilDate, ()> {
    let mut date_fields = DateFields::default();
    if let (Some(era), Some(era_year)) = (fields.era, fields.era_year) {
        date_fields.era = Some(era.as_bytes());
        date_fields.era_year = Some(era_year);
    } else {
        date_fields.extended_year = fields.extended_year;
    }
    if let Some(month_code) = fields.month_code {
        date_fields.month_code = Some(month_code.as_bytes());
    }
    date_fields.ordinal_month = fields.ordinal_month;
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(if reject {
        IcuOverflow::Reject
    } else {
        IcuOverflow::Constrain
    });
    options.missing_fields_strategy = Some(MissingFieldsStrategy::Ecma);
    let date =
        Date::try_from_fields(date_fields, options, AnyCalendar::new(calendar)).map_err(|_| ())?;
    let iso = date.to_calendar(Iso);
    Ok((
        iso.year().extended_year(),
        iso.month().number(),
        iso.day_of_month().0,
    ))
}

/// `TemporalYearMonthToString`'s date portion: the short `YYYY-MM` form when
/// the calendar is `iso8601` and the calendar annotation would otherwise be
/// omitted, else the full `YYYY-MM-DD` ISO date (the stored reference day).
pub(crate) fn format_year_month(date: CivilDate, calendar: &str, show: ShowCalendar) -> String {
    let (year, month, _) = date;
    let mut result = if show == ShowCalendar::Always
        || show == ShowCalendar::Critical
        || calendar != "iso8601"
    {
        format_iso_date(date)
    } else {
        let year_text = if (0..=9999).contains(&year) {
            format!("{year:04}")
        } else {
            format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
        };
        format!("{year_text}-{month:02}")
    };
    result.push_str(&format_calendar_annotation(calendar, show));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_year_month_fields_to_the_first_of_the_month() {
        let fields = YearMonthFields {
            extended_year: Some(2020),
            ordinal_month: Some(5),
            ..Default::default()
        };
        assert_eq!(
            year_month_from_fields(AnyCalendarKind::Iso, &fields, false),
            Ok((2020, 5, 1))
        );
    }

    #[test]
    fn constrains_an_out_of_range_month_by_default() {
        let fields = YearMonthFields {
            extended_year: Some(2020),
            ordinal_month: Some(13),
            ..Default::default()
        };
        assert_eq!(
            year_month_from_fields(AnyCalendarKind::Iso, &fields, false),
            Ok((2020, 12, 1))
        );
    }

    #[test]
    fn rejects_an_out_of_range_month_when_asked() {
        let fields = YearMonthFields {
            extended_year: Some(2020),
            ordinal_month: Some(13),
            ..Default::default()
        };
        assert_eq!(
            year_month_from_fields(AnyCalendarKind::Iso, &fields, true),
            Err(())
        );
    }

    #[test]
    fn resolves_a_month_code_the_same_as_an_ordinal_month() {
        let fields = YearMonthFields {
            extended_year: Some(2020),
            month_code: Some("M05"),
            ..Default::default()
        };
        assert_eq!(
            year_month_from_fields(AnyCalendarKind::Iso, &fields, false),
            Ok((2020, 5, 1))
        );
    }

    #[test]
    fn resolves_a_non_iso_calendar_year_month_to_the_first_of_that_month() {
        // Gregorian is offset from ISO by no fields at all, so this should
        // agree exactly with the ISO fast path above -- confirms
        // `calendar_add_date`'s own generalization pattern is safe to reuse
        // for a non-ISO `AnyCalendarKind` here too.
        let fields = YearMonthFields {
            extended_year: Some(2020),
            ordinal_month: Some(5),
            ..Default::default()
        };
        assert_eq!(
            year_month_from_fields(AnyCalendarKind::Gregorian, &fields, false),
            Ok((2020, 5, 1))
        );
    }

    #[test]
    fn formats_the_short_year_month_form_for_the_iso_calendar() {
        assert_eq!(
            format_year_month((2000, 5, 1), "iso8601", ShowCalendar::Auto),
            "2000-05"
        );
        assert_eq!(
            format_year_month((-271_821, 4, 19), "iso8601", ShowCalendar::Auto),
            "-271821-04"
        );
    }

    #[test]
    fn formats_the_full_date_when_the_calendar_annotation_is_forced_or_non_iso() {
        assert_eq!(
            format_year_month((2000, 5, 1), "iso8601", ShowCalendar::Always),
            "2000-05-01[u-ca=iso8601]"
        );
        assert_eq!(
            format_year_month((2000, 5, 1), "hebrew", ShowCalendar::Auto),
            "2000-05-01[u-ca=hebrew]"
        );
        // `Never` only suppresses the calendar annotation, not the date's
        // own short-form choice -- an `iso8601` calendar still gets the
        // short `YYYY-MM` form here, matching `Auto`.
        assert_eq!(
            format_year_month((2000, 5, 1), "iso8601", ShowCalendar::Never),
            "2000-05"
        );
    }
}
