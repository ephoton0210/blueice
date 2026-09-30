// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Temporal.PlainMonthDay` calendar-field resolution (Phase 26
//! Stage 2's `plain_year_month.rs`/`plain_month_day.rs` slice,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! Unlike `PlainYearMonth`/`PlainDate`/`PlainDateTime`, `PlainMonthDay` has
//! **no** `add`/`subtract`/`until`/`since`/`compare` at all -- confirmed
//! directly against the pinned Test262 corpus (no
//! `PlainMonthDay/prototype/{add,subtract,until,since}/` or
//! `PlainMonthDay/compare/` directory exists) and against Gecko's own
//! `PlainMonthDay.cpp`, which defines no such methods either. A month-day
//! pair has no well-ordered total order in general (a leap-month calendar's
//! "day 30 of month 5" may not even exist in most years), so the spec simply
//! does not define these operations for this type.

use super::epoch::{is_date_within_limits, CivilDate};
use super::plain_date::{
    format_calendar_annotation, format_iso_date, regulate_iso_date, ShowCalendar,
};
use icu_calendar::options::{
    DateFromFieldsOptions, MissingFieldsStrategy, Overflow as IcuOverflow,
};
use icu_calendar::types::DateFields;
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, Iso};

/// The calendar fields `CalendarMonthDayFromFields` reads: `day` is always
/// present; either `monthCode` or `ordinalMonth` identifies the month (both may
/// be given, in which case they must agree); `year` (or `era`+`eraYear`) is
/// optional and only decides whether the day exists -- the result's *reference
/// year* is always derived afresh from the month code and day
/// ([`month_day_from_fields`]).
#[derive(Default)]
pub(crate) struct MonthDayFields<'a> {
    pub extended_year: Option<i32>,
    /// `era`/`era_year`: read only for a calendar that supports eras
    /// (`CalendarExtraFields`'s own conditional field expansion --
    /// requesting `year`, which `ToTemporalMonthDay`'s field list always
    /// does, also reads `era`/`eraYear` when the calendar has them).
    /// When both are supplied they resolve the year on their own, and an
    /// `extended_year` given next to them is only cross-checked against the
    /// result (`icu_calendar::Date::try_from_fields` resolves the year from
    /// `era`/`era_year` directly, without a separate era-to-extended-year
    /// conversion step of this module's own).
    pub era: Option<&'a [u8]>,
    pub era_year: Option<i32>,
    pub month_code: Option<&'a str>,
    pub ordinal_month: Option<u8>,
    pub day: u8,
}

/// `CalendarMonthDayFromFields` for a non-ISO calendar: resolves month+day (and
/// optionally year) fields to the ISO *reference date* a `PlainMonthDay` keeps.
///
/// Two steps, exactly as Intl.Era-monthcode defines it:
///
/// 1. **With a year** (`year`, or `era`+`eraYear`, which must be inside
///    Temporal's range), resolve the fields in that
///    year under `overflow`: this is what decides whether the day exists
///    (`overflow: "reject"` refuses one that does not, `"constrain"` clamps
///    it, and a leap month missing from that year falls back to its common
///    neighbour). An explicit `year` next to `era`/`eraYear`, and an ordinal
///    `month` next to a `monthCode`, must agree with the result. The year itself
///    is then **discarded**: only the resulting month code and day survive.
///    Without a year the month code and day are taken as given.
/// 2. Derive the reference year from that month code and day alone
///    (`MissingFieldsStrategy::Ecma`): the latest ISO year at or before 1972
///    that has them, else the earliest one after it -- `{ year: 2021,
///    monthCode: "M02", day: 29, calendar: "gregory" }` is `1972-02-28`, not
///    `2021-02-28`. Under `overflow: "reject"` a month-day that occurs in no
///    such year (a Chinese `M01L` 29th) is refused; under `"constrain"` it
///    falls back to the common month.
///
/// A `month` without a `monthCode` needs a year to be meaningful (an ordinal
/// month's identity varies by year); callers reject that with a `TypeError`
/// before getting here.
pub(crate) fn month_day_from_fields(
    calendar: AnyCalendarKind,
    fields: &MonthDayFields,
    reject: bool,
) -> Result<CivilDate, ()> {
    let overflow = if reject {
        IcuOverflow::Reject
    } else {
        IcuOverflow::Constrain
    };
    let has_era_pair = fields.era.is_some() && fields.era_year.is_some();
    let (month_code, day) = if has_era_pair || fields.extended_year.is_some() {
        let mut date_fields = DateFields::default();
        if has_era_pair {
            date_fields.era = fields.era;
            date_fields.era_year = fields.era_year;
        } else {
            date_fields.extended_year = fields.extended_year;
        }
        match fields.month_code {
            Some(month_code) => date_fields.month_code = Some(month_code.as_bytes()),
            None => date_fields.ordinal_month = fields.ordinal_month,
        }
        date_fields.day = Some(fields.day);
        let mut options = DateFromFieldsOptions::default();
        options.overflow = Some(overflow);
        let date = Date::try_from_fields(date_fields, options, AnyCalendar::new(calendar))
            .map_err(|_| ())?;
        if fields
            .extended_year
            .is_some_and(|year| year != date.year().extended_year())
        {
            return Err(());
        }
        if fields.month_code.is_some()
            && fields
                .ordinal_month
                .is_some_and(|month| month != date.month().ordinal)
        {
            return Err(());
        }
        // The year is discarded afterwards, but it must still be one Temporal
        // can represent: a year far outside the range bails out with a
        // `RangeError` rather than being resolved (`intl402/Temporal/
        // PlainMonthDay/from/dont-calculate-month-info-for-out-of-range-year.js`).
        let iso = date.to_calendar(Iso);
        let resolved: CivilDate = (
            iso.year().extended_year(),
            iso.month().number(),
            iso.day_of_month().0,
        );
        if !is_date_within_limits(resolved) {
            return Err(());
        }
        (
            date.month().to_input().code().to_string(),
            date.day_of_month().0,
        )
    } else {
        (fields.month_code.ok_or(())?.to_string(), fields.day)
    };

    let mut date_fields = DateFields::default();
    date_fields.month_code = Some(month_code.as_bytes());
    date_fields.day = Some(day);
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(overflow);
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

/// The `iso8601` calendar's own branch of `CalendarMonthDayFromFields`
/// (`Calendar.cpp`): a supplied `year` (or `1972` if absent) regulates the
/// resolved `day` against *that* year's own leap-year-ness (constrain/reject
/// per `overflow`), but the fixed reference year `1972` -- never the
/// supplied one -- is what survives into the result.
///
/// Deliberately bypasses `icu_calendar::Date::try_from_fields` entirely
/// (unlike [`month_day_from_fields`]'s general non-ISO path): ICU4X's own
/// internal year-range limits are far narrower than Temporal's actual
/// regulation-year domain here -- an arbitrarily large or small `year` is
/// legitimate input purely for leap-year determination and must never be
/// rejected merely for being out of `icu_calendar`'s own representable
/// range (`PlainMonthDay/from/iso-year-used-only-for-overflow.js`'s own
/// `-1000000` case is exactly this: a leap year via the Gregorian
/// divisible-by-400 rule, far outside any calendar library's usual
/// constructor bounds). [`super::plain_date::regulate_iso_date`] is pure
/// Rust arithmetic with no such limit.
pub(crate) fn iso_month_day_from_fields(
    ordinal_month: u8,
    day: u8,
    regulation_year: i32,
    reject: bool,
) -> Result<CivilDate, ()> {
    // `regulate_iso_date` assumes `month` is already `1..=12` (Temporal
    // ISO months are regulated before it is ever called elsewhere in this
    // codebase); a bare ordinal `month` read straight from user input has
    // no such guarantee (`temporal_integer`'s own field bound is the wider
    // `1..=99`), so regulate it here first rather than risk the
    // `unreachable!` in `iso_days_in_month`.
    let month = if reject {
        if !(1..=12).contains(&ordinal_month) {
            return Err(());
        }
        ordinal_month
    } else {
        ordinal_month.clamp(1, 12)
    };
    let (_, month, day) =
        regulate_iso_date(regulation_year, month, i64::from(day), reject).ok_or(())?;
    Ok((1972, month, day))
}

/// `IsValidMonthCode`'s pure grammar half, calendar-agnostic: `M` followed by
/// exactly two ASCII digits, optionally followed by `L`. This is a syntax
/// check only -- it says nothing about whether the resulting number is a
/// real month in any particular calendar (see [`iso_month_code_ordinal`] for
/// the ISO 8601 calendar's own suitability rule on top of this). Matches
/// Test262's `TemporalHelpers.ISO.monthCode`-style validation and pinned by
/// `PlainMonthDay/from/monthcode-invalid.js`'s `"m1"`/`"M1"`/`"m01"`
/// (wrong case or missing a digit) and `"L99M"` (wrong letter position)
/// cases, all of which must be rejected as malformed regardless of any
/// calendar.
pub(crate) fn is_well_formed_month_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    let digits = match bytes.len() {
        3 => &bytes[1..3],
        4 if bytes[3] == b'L' => &bytes[1..3],
        _ => return false,
    };
    bytes[0] == b'M' && digits.iter().all(u8::is_ascii_digit)
}

/// The `iso8601` calendar's own `monthCode` *suitability* rule, on top of
/// [`is_well_formed_month_code`]'s pure syntax check: no leap-month `L`
/// suffix at all (the ISO 8601 calendar has no leap months), and the
/// two-digit number must be a real month, `01`-`12`. Returns the ordinal
/// month on success. Precondition: `code` is already well-formed (this
/// function does not re-validate the shape). Pinned by
/// `PlainMonthDay/from/monthcode-invalid.js`'s `"M00"`/`"M19"`/`"M99"`/
/// `"M13"` (out-of-range, no suffix) and `"M00L"`/`"M05L"`/`"M13L"`
/// (well-formed but a leap suffix, which ISO 8601 never has) cases -- every
/// one of these must be a `RangeError` *regardless of `overflow`*, since
/// `monthCode` suitability is a distinct check from a numeric field's own
/// constrain/reject regulation.
pub(crate) fn iso_month_code_ordinal(code: &str) -> Option<u8> {
    if code.ends_with('L') {
        return None;
    }
    code[1..]
        .parse::<u8>()
        .ok()
        .filter(|month| (1..=12).contains(month))
}

/// `TemporalMonthDayToString`'s date portion: the short `MM-DD` form when the
/// calendar is `iso8601` and the calendar annotation would otherwise be
/// omitted, else the full `YYYY-MM-DD` ISO date (the stored reference year).
pub(crate) fn format_month_day(date: CivilDate, calendar: &str, show: ShowCalendar) -> String {
    let (_, month, day) = date;
    let mut result = if show == ShowCalendar::Always
        || show == ShowCalendar::Critical
        || calendar != "iso8601"
    {
        format_iso_date(date)
    } else {
        format!("{month:02}-{day:02}")
    };
    result.push_str(&format_calendar_annotation(calendar, show));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_month_and_day_to_the_1972_iso_reference_year_when_no_year_is_given() {
        // `icu_calendar`'s `MissingFieldsStrategy::Ecma` only derives a
        // reference year from a `monthCode` + `day` pair, not from a bare
        // ordinal `month` -- an ordinal month's identity varies by year, so
        // there is no well-defined reference year for one. `Vm::
        // temporal_plain_month_day_from_fields` requires a `year` whenever
        // only an ordinal `month` (no `monthCode`) is given, matching this.
        let fields = MonthDayFields {
            month_code: Some("M11"),
            day: 18,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Iso, &fields, false),
            Ok((1972, 11, 18))
        );
    }

    #[test]
    fn an_ordinal_month_alone_cannot_derive_a_reference_year() {
        let fields = MonthDayFields {
            ordinal_month: Some(11),
            day: 18,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Iso, &fields, false),
            Err(())
        );
    }

    #[test]
    fn an_explicit_year_only_decides_whether_the_day_exists() {
        // 2000 is a leap year, so 29 February exists; the reference year is
        // still derived afresh (1972, the latest leap year at or before it).
        let fields = MonthDayFields {
            extended_year: Some(2000),
            ordinal_month: Some(2),
            day: 29,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Iso, &fields, false),
            Ok((1972, 2, 29))
        );
    }

    #[test]
    fn constrains_a_leap_day_in_a_non_leap_year_by_default() {
        // 2001 is not a leap year: the day constrains to the 28th, and the
        // reference year is 1972 (not 2001).
        let fields = MonthDayFields {
            extended_year: Some(2001),
            ordinal_month: Some(2),
            day: 29,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Iso, &fields, false),
            Ok((1972, 2, 28))
        );
    }

    #[test]
    fn a_hebrew_leap_month_day_gets_a_reference_year_before_1972() {
        // Adar I (`M05L`) does not occur in 1972; the latest year at or before
        // it that has it is 1970 (`intl402/Temporal/PlainMonthDay/from/
        // reference-year-1972.js`).
        let fields = MonthDayFields {
            month_code: Some("M05L"),
            day: 1,
            ..Default::default()
        };
        let (year, _, _) = month_day_from_fields(AnyCalendarKind::Hebrew, &fields, false).unwrap();
        assert_eq!(year, 1970);
    }

    #[test]
    fn a_year_and_a_month_code_may_disagree_on_the_ordinal_only_when_consistent() {
        // Chinese 2004 has a leap M02, so M04 is ordinal month 5 there.
        let mut fields = MonthDayFields {
            extended_year: Some(2004),
            month_code: Some("M04"),
            ordinal_month: Some(5),
            day: 1,
            ..Default::default()
        };
        assert!(month_day_from_fields(AnyCalendarKind::Chinese, &fields, false).is_ok());
        // A month that does not match the code in that year is refused.
        fields.ordinal_month = Some(4);
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Chinese, &fields, false),
            Err(())
        );
    }

    #[test]
    fn an_explicit_year_must_agree_with_the_era_year() {
        let mut fields = MonthDayFields {
            extended_year: Some(2023),
            era: Some(b"ce"),
            era_year: Some(2024),
            month_code: Some("M01"),
            day: 1,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Gregorian, &fields, false),
            Err(())
        );
        fields.extended_year = Some(2024);
        assert!(month_day_from_fields(AnyCalendarKind::Gregorian, &fields, false).is_ok());
    }

    #[test]
    fn a_year_outside_temporals_range_is_refused_in_every_calendar() {
        for calendar in [
            AnyCalendarKind::Gregorian,
            AnyCalendarKind::Hebrew,
            AnyCalendarKind::Chinese,
            AnyCalendarKind::Persian,
        ] {
            for year in [-999_999, 999_999] {
                let fields = MonthDayFields {
                    extended_year: Some(year),
                    month_code: Some("M01"),
                    day: 1,
                    ..Default::default()
                };
                assert_eq!(
                    month_day_from_fields(calendar, &fields, false),
                    Err(()),
                    "{calendar:?} {year}"
                );
            }
        }
    }

    #[test]
    fn a_month_day_that_never_occurs_is_refused_under_reject_and_constrained_otherwise() {
        // Chinese M01L 29th has not occurred since 1900.
        let fields = MonthDayFields {
            extended_year: Some(1898),
            month_code: Some("M01L"),
            day: 29,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Chinese, &fields, true),
            Err(())
        );
        let (year, _, _) = month_day_from_fields(AnyCalendarKind::Chinese, &fields, false).unwrap();
        assert_eq!(year, 1972);
    }

    #[test]
    fn rejects_a_leap_day_in_a_non_leap_year_when_asked() {
        let fields = MonthDayFields {
            extended_year: Some(2001),
            ordinal_month: Some(2),
            day: 29,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Iso, &fields, true),
            Err(())
        );
    }

    #[test]
    fn resolves_a_month_code_the_same_as_an_ordinal_month() {
        let fields = MonthDayFields {
            month_code: Some("M11"),
            day: 18,
            ..Default::default()
        };
        assert_eq!(
            month_day_from_fields(AnyCalendarKind::Iso, &fields, false),
            Ok((1972, 11, 18))
        );
    }

    #[test]
    fn formats_the_short_month_day_form_for_the_iso_calendar() {
        assert_eq!(
            format_month_day((1972, 11, 18), "iso8601", ShowCalendar::Auto),
            "11-18"
        );
    }

    #[test]
    fn formats_the_full_date_when_the_calendar_annotation_is_forced_or_non_iso() {
        assert_eq!(
            format_month_day((1972, 11, 18), "iso8601", ShowCalendar::Always),
            "1972-11-18[u-ca=iso8601]"
        );
        assert_eq!(
            format_month_day((1972, 11, 18), "hebrew", ShowCalendar::Auto),
            "1972-11-18[u-ca=hebrew]"
        );
        assert_eq!(
            format_month_day((1972, 11, 18), "iso8601", ShowCalendar::Never),
            "11-18"
        );
    }

    #[test]
    fn iso_fast_path_always_reports_the_1972_reference_year() {
        assert_eq!(
            iso_month_day_from_fields(1, 1, -999_999, false),
            Ok((1972, 1, 1))
        );
    }

    #[test]
    fn iso_fast_path_uses_the_supplied_year_only_to_regulate_the_day() {
        // -999999 is a common (non-leap) year: 29 February constrains to 28.
        assert_eq!(
            iso_month_day_from_fields(2, 29, -999_999, false),
            Ok((1972, 2, 28))
        );
        assert_eq!(iso_month_day_from_fields(2, 29, -999_999, true), Err(()));
        // -1000000 is a leap year (divisible by 400): 29 February is exact.
        assert_eq!(
            iso_month_day_from_fields(2, 29, -1_000_000, false),
            Ok((1972, 2, 29))
        );
        assert_eq!(
            iso_month_day_from_fields(2, 29, -1_000_000, true),
            Ok((1972, 2, 29))
        );
    }

    #[test]
    fn iso_fast_path_regulates_an_out_of_range_ordinal_month_without_panicking() {
        assert_eq!(
            iso_month_day_from_fields(13, 1, 1972, false),
            Ok((1972, 12, 1))
        );
        assert_eq!(iso_month_day_from_fields(13, 1, 1972, true), Err(()));
        assert_eq!(
            iso_month_day_from_fields(0, 1, 1972, false),
            Ok((1972, 1, 1))
        );
    }

    #[test]
    fn well_formed_month_codes_are_accepted() {
        for code in ["M01", "M12", "M00", "M99", "M01L", "M13L", "M99L"] {
            assert!(is_well_formed_month_code(code), "{code}");
        }
    }

    #[test]
    fn malformed_month_codes_are_rejected() {
        for code in ["m1", "M1", "m01", "L99M", "M1L", "M123", "", "M", "MLL"] {
            assert!(!is_well_formed_month_code(code), "{code}");
        }
    }

    #[test]
    fn iso_month_code_ordinal_accepts_only_01_through_12_with_no_leap_suffix() {
        assert_eq!(iso_month_code_ordinal("M01"), Some(1));
        assert_eq!(iso_month_code_ordinal("M12"), Some(12));
        assert_eq!(iso_month_code_ordinal("M06"), Some(6));
    }

    #[test]
    fn iso_month_code_ordinal_rejects_out_of_range_or_leap_codes() {
        for code in ["M00", "M13", "M19", "M99", "M00L", "M05L", "M13L"] {
            assert_eq!(iso_month_code_ordinal(code), None, "{code}");
        }
    }
}
