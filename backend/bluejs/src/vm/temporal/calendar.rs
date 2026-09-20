// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The recognized Temporal calendar identifiers and their ICU4X mapping.
//!
//! Per SpiderMonkey's own `Calendar.h` (read as this project's porting
//! reference): `CalendarId` is a **closed enum**, not a general
//! object-protocol, matching the current Temporal spec revision. This module
//! is deliberately just that closed recognition table today; the full
//! per-calendar `CalendarFields`<->ISO conversion dispatch is Stage 1 Track
//! A's scope (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`),
//! reusing `icu_calendar` the way `Intl.DateTimeFormat`'s existing
//! non-ISO-calendar bridge already does.

use super::epoch::CivilDate;
use super::plain_date::iso_date_to_epoch_days;
use icu_calendar::types::RataDie;
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, Iso};

/// Resolves a canonical Temporal calendar identifier to its ICU4X calendar
/// kind, or `None` if it is not one of Temporal's 16 (`AvailableCalendars()`,
/// Gecko's closed `CalendarId` enum). The legacy ECMA-402 identifiers
/// `"islamic"` and `"islamic-rgsa"` are deliberately absent: they are
/// `Intl.DateTimeFormat` fallbacks only, and Temporal rejects them with a
/// `RangeError` (`intl402/Temporal/*/from/islamic{,-rgsa}.js`).
pub(crate) fn calendar_kind(calendar: &str) -> Option<AnyCalendarKind> {
    Some(match calendar {
        "iso8601" => AnyCalendarKind::Iso,
        "gregory" => AnyCalendarKind::Gregorian,
        "buddhist" => AnyCalendarKind::Buddhist,
        "chinese" => AnyCalendarKind::Chinese,
        "coptic" => AnyCalendarKind::Coptic,
        "dangi" => AnyCalendarKind::Dangi,
        "ethiopic" => AnyCalendarKind::Ethiopian,
        "ethioaa" => AnyCalendarKind::EthiopianAmeteAlem,
        "hebrew" => AnyCalendarKind::Hebrew,
        "indian" => AnyCalendarKind::Indian,
        "islamic-civil" => AnyCalendarKind::HijriTabularTypeIIFriday,
        "islamic-tbla" => AnyCalendarKind::HijriTabularTypeIIThursday,
        "islamic-umalqura" => AnyCalendarKind::HijriUmmAlQura,
        "japanese" => AnyCalendarKind::Japanese,
        "persian" => AnyCalendarKind::Persian,
        "roc" => AnyCalendarKind::Roc,
        _ => return None,
    })
}

/// The rata die of 1970-01-01 (rata die 1 is 0001-01-01 of the proleptic
/// Gregorian calendar), the offset between a Unix epoch-day count and
/// `icu_calendar`'s day numbering.
const UNIX_EPOCH_RATA_DIE: i64 = 719_163;

/// An `icu_calendar` ISO `Date` for a Temporal civil date anywhere in
/// Temporal's own supported range (ISO -271821-04-19 .. +275760-09-13, and the
/// year-month reference days just outside it).
///
/// This is the one place a Temporal ISO date becomes an ICU4X date.
/// `Date::try_new_iso` is not usable for that: it only accepts years
/// `-9999..=9999`, so every getter, `withCalendar` and `from` on an
/// extreme-year date in a non-ISO calendar failed with a spurious
/// "invalid Temporal ISO date" (`intl402/Temporal/*/prototype/withCalendar/
/// extreme-dates.js`). `Date::from_rata_die` covers ICU4X's fundamental range,
/// which is never smaller than Temporal's.
pub(crate) fn iso_date_from_civil(date: CivilDate) -> Date<Iso> {
    Date::from_rata_die(
        RataDie::new(iso_date_to_epoch_days(date) + UNIX_EPOCH_RATA_DIE),
        Iso,
    )
}

/// The same date in `calendar` (see [`iso_date_from_civil`] for the range).
pub(crate) fn calendar_date_from_civil(
    calendar: AnyCalendarKind,
    date: CivilDate,
) -> Date<AnyCalendar> {
    iso_date_from_civil(date).to_calendar(AnyCalendar::new(calendar))
}

/// `CalendarDayOfYear ( calendar, isoDate )` for a non-ISO calendar: the
/// 1-based position of `date` within *that calendar's* year, which starts on
/// the calendar's own first day (Tishrei 1, Nowruz, Thout 1, ...), not on ISO
/// January 1st. The ISO calendar's own ordinal is `iso_day_of_year`.
pub(crate) fn calendar_day_of_year(calendar: AnyCalendarKind, date: CivilDate) -> u16 {
    calendar_date_from_civil(calendar, date).day_of_year().0
}

/// `CalendarMonthsPerYear ( calendar )` for a calendar whose year always has
/// the same number of months: `13` for the three calendars with a 5/6-day
/// intercalary `M13` (`coptic`, `ethiopic`, `ethioaa` -- twelve 30-day months
/// plus that short thirteenth), `12` for every other one. The lunisolar
/// calendars (`chinese`, `dangi`, `hebrew`) have no constant answer -- their
/// month count depends on the year, via a leap month -- and Gecko routes them
/// through a separate leap-month difference algorithm that never asks; a
/// caller must not use this for them.
pub(crate) fn calendar_months_per_year(calendar: AnyCalendarKind) -> i64 {
    match calendar {
        AnyCalendarKind::Coptic
        | AnyCalendarKind::Ethiopian
        | AnyCalendarKind::EthiopianAmeteAlem => 13,
        _ => 12,
    }
}

/// `CalendarSupportsEra ( calendar )`, per Gecko's own `Era.h`
/// (`development/browser_core/reference/gecko/js/src/builtin/temporal/Era.h`):
/// every recognized calendar has at least one era except `iso8601`,
/// `chinese` and `dangi`, whose ICU4X representation has no era concept at
/// all. Used by `Temporal.PlainYearMonth.prototype.with` to decide whether
/// `era`/`eraYear` are recognized, mutually-exclusive-with-`year` calendar
/// fields for the receiver's own calendar (`CalendarFields.cpp`'s
/// `NonISOFieldKeysToIgnore`/`NonISOResolveFields`).
pub(crate) fn calendar_supports_era(calendar: &str) -> bool {
    !matches!(calendar, "iso8601" | "chinese" | "dangi")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_from_civil_reaches_temporals_full_range() {
        // `Date::try_new_iso` stops at year 9999; a Temporal date does not.
        for date in [
            (-271_821, 4, 19),
            (275_760, 9, 13),
            (-9_999, 1, 1),
            (10_000, 6, 30),
            (1970, 1, 1),
            (2000, 2, 29),
        ] {
            let iso = iso_date_from_civil(date);
            assert_eq!(
                (
                    iso.year().extended_year(),
                    iso.month().number(),
                    iso.day_of_month().0
                ),
                date
            );
        }
        assert_eq!(
            iso_date_from_civil((1970, 1, 1)).to_rata_die(),
            RataDie::new(UNIX_EPOCH_RATA_DIE)
        );
    }

    #[test]
    fn calendar_date_from_civil_converts_at_the_extremes_of_the_range() {
        // A date far outside `try_new_iso`'s window, into a calendar whose
        // years are numbered very differently from ISO's
        // (`intl402/Temporal/PlainDate/prototype/withCalendar/extreme-dates.js`).
        let hebrew = calendar_date_from_civil(AnyCalendarKind::Hebrew, (-271_821, 4, 19));
        assert_eq!(hebrew.year().extended_year(), -268_058);
        let coptic = calendar_date_from_civil(AnyCalendarKind::Coptic, (275_760, 9, 13));
        assert_eq!(coptic.year().extended_year(), 275_471);
    }

    #[test]
    fn calendar_day_of_year_counts_from_the_calendars_own_first_day() {
        // Hebrew year 5784 began on ISO 2023-09-16; the day before is the
        // last day of 5783.
        let hebrew = AnyCalendarKind::Hebrew;
        assert_eq!(calendar_day_of_year(hebrew, (2023, 9, 16)), 1);
        assert_eq!(calendar_day_of_year(hebrew, (2023, 9, 17)), 2);
        assert!(calendar_day_of_year(hebrew, (2023, 9, 15)) > 350);
        // Gregorian follows ISO exactly; the Persian year starts around 21 March.
        assert_eq!(
            calendar_day_of_year(AnyCalendarKind::Gregorian, (1976, 11, 18)),
            323
        );
        assert_eq!(
            calendar_day_of_year(AnyCalendarKind::Persian, (2024, 3, 20)),
            1
        );
        assert!(calendar_day_of_year(AnyCalendarKind::Persian, (2024, 3, 19)) > 360);
    }

    #[test]
    fn only_the_intercalary_month_calendars_have_thirteen_months_per_year() {
        for id in ["coptic", "ethiopic", "ethioaa"] {
            let kind = calendar_kind(id).unwrap();
            assert_eq!(calendar_months_per_year(kind), 13, "{id}");
        }
        for id in [
            "iso8601",
            "gregory",
            "buddhist",
            "indian",
            "islamic-civil",
            "islamic-tbla",
            "islamic-umalqura",
            "japanese",
            "persian",
            "roc",
        ] {
            let kind = calendar_kind(id).unwrap();
            assert_eq!(calendar_months_per_year(kind), 12, "{id}");
        }
    }

    #[test]
    fn only_iso_chinese_and_dangi_lack_era_support() {
        for id in ["iso8601", "chinese", "dangi"] {
            assert!(!calendar_supports_era(id), "{id} should not support eras");
        }
        for id in [
            "gregory",
            "buddhist",
            "coptic",
            "ethiopic",
            "ethioaa",
            "hebrew",
            "indian",
            "islamic-civil",
            "islamic-tbla",
            "islamic-umalqura",
            "japanese",
            "persian",
            "roc",
        ] {
            assert!(calendar_supports_era(id), "{id} should support eras");
        }
    }

    #[test]
    fn recognizes_every_currently_supported_calendar_id() {
        for id in [
            "iso8601",
            "gregory",
            "buddhist",
            "chinese",
            "coptic",
            "dangi",
            "ethiopic",
            "ethioaa",
            "hebrew",
            "indian",
            "islamic-civil",
            "islamic-tbla",
            "islamic-umalqura",
            "japanese",
            "persian",
            "roc",
        ] {
            assert!(calendar_kind(id).is_some(), "{id} should be recognized");
        }
    }

    #[test]
    fn rejects_unknown_or_not_yet_adopted_calendar_ids() {
        assert_eq!(calendar_kind("discordian"), None);
        // An alias, not a canonical id: `canonical_calendar_id` resolves it
        // to `islamic-civil` before this table is consulted.
        assert_eq!(calendar_kind("islamicc"), None);
    }

    #[test]
    fn rejects_the_legacy_ecma_402_islamic_identifiers() {
        assert_eq!(calendar_kind("islamic"), None);
        assert_eq!(calendar_kind("islamic-rgsa"), None);
    }
}
