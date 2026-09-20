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
//! (`Vm::temporal_calendar_fields`, in the parent module). This module tree is
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
//!
//! The submodules, in dependency order (each only imports from those above
//! it):
//!
//! - `iso_date` — ISO week-date getters, `AddISODate`, raw-tuple comparison.
//! - `format` — `ISODateToString` and `FormatCalendarAnnotation`.
//! - `month_structure` — calendar month layout, ordinal/`Month`-identity
//!   conversions and comparison shared by add, difference and rounding.
//! - `calendar_add` — `CalendarDateAdd`, including the leap-month branch.
//! - `calendar_difference` — `CalendarDateUntil`: ISO, fixed-months and
//!   leap-month variants and their dispatcher.
//!
//! Rounding a difference (`RoundRelativeDuration`) is not part of this tree:
//! `plain_date_time_difference` (a sibling of this module) ports it for every
//! plain difference -- `PlainDate`, `PlainDateTime` and `PlainYearMonth` --
//! on top of `calendar_add_date`/`calendar_difference_date`.
//!
//! This file is only the facade: it keeps every `plain_date::*` path callers
//! already use stable.

mod calendar_add;
mod calendar_difference;
mod format;
mod iso_date;
mod month_structure;
#[cfg(test)]
mod tests;

// The module tree's crate-facing API. Not every name has an external caller
// today; the facade keeps the pre-split `plain_date::*` paths stable for all
// of them.
#[allow(unused_imports)]
pub(crate) use self::calendar_add::calendar_add_date;
#[allow(unused_imports)]
pub(crate) use self::calendar_difference::{
    calendar_difference_date, difference_iso_date, DateUnit,
};
#[allow(unused_imports)]
pub(crate) use self::format::{
    format_calendar_annotation, format_iso_date, parse_show_calendar, ShowCalendar,
};
#[allow(unused_imports)]
pub(crate) use self::iso_date::{
    add_iso_date, balance_iso_date, balance_iso_year_month, compare_iso_date,
    epoch_days_to_iso_date, is_iso_leap_year, iso_date_to_epoch_days, iso_day_of_week,
    iso_day_of_year, iso_days_in_month, iso_week_of_year, regulate_iso_date,
};
