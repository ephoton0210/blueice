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

#[cfg(test)]
mod tests {
    use super::*;

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
