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

use icu_calendar::AnyCalendarKind;

/// Resolves a Temporal/ECMA-402 calendar identifier to its ICU4X calendar
/// kind, or `None` if the identifier is not (yet) recognized.
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
        "islamic" | "islamic-civil" | "islamic-rgsa" => AnyCalendarKind::HijriTabularTypeIIFriday,
        "islamic-tbla" => AnyCalendarKind::HijriTabularTypeIIThursday,
        "islamic-umalqura" => AnyCalendarKind::HijriUmmAlQura,
        "japanese" => AnyCalendarKind::Japanese,
        "persian" => AnyCalendarKind::Persian,
        "roc" => AnyCalendarKind::Roc,
        _ => return None,
    })
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
            "islamic",
            "islamic-civil",
            "islamic-rgsa",
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
            "islamic",
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
            "islamic",
            "islamic-civil",
            "islamic-rgsa",
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
        assert_eq!(calendar_kind("islamicc"), None);
    }
}
