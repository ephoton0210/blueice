// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real bug in `Vm::temporal_calendar_identifier`
//! (`ToTemporalCalendarIdentifier`, `backend/bluejs/src/vm/temporal.rs`) —
//! Phase 26 Stage 2, second slice
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! A property-bag `calendar` field (or a `withCalendar` argument) may be a
//! full ISO date/date-time/year-month/month-day/time string, not only a
//! bare calendar ID. The previous implementation only recognized this when
//! the string contained a `[u-ca=...]` annotation bracket (`value.find('[')`);
//! an *unannotated* ISO string like `"2020-01-01"` has no bracket at all, so
//! it fell straight through to a bare-calendar-ID lookup, which of course
//! rejects `"2020-01-01"` as not a recognized calendar name, throwing
//! `RangeError: invalid Temporal calendar`.
//!
//! But an unannotated ISO string always means the `iso8601` calendar (the
//! calendar defaults to `iso8601` whenever no `u-ca` annotation is present)
//! — confirmed directly by Test262's
//! `built-ins/Temporal/PlainDate/prototype/equals/argument-propertybag-calendar-iso-string.js`,
//! which passes eight unannotated and annotated ISO string shapes (full
//! date, date-time, year-month, month-day, each with and without a
//! `[u-ca=iso8601]` suffix) as a property-bag `calendar` value and expects
//! all eight to resolve to `iso8601` rather than throw.
//!
//! Fixed by trying every ISO string production this crate has a parser for
//! (date-time, year-month, month-day, time) before falling back to a bare
//! calendar ID lookup, extracting the first `u-ca=` annotation from
//! whichever one matches (defaulting to `iso8601` when none is present).

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// The exact eight string shapes
/// `equals/argument-propertybag-calendar-iso-string.js` iterates over: full
/// date, full date-time, year-month, month-day, each with and without a
/// `[u-ca=iso8601]` suffix.
#[test]
fn unannotated_and_annotated_iso_strings_all_resolve_to_iso8601() {
    assert_true(
        r#"
        (function() {
          const instance = new Temporal.PlainDate(1976, 11, 18);
          const calendars = [
            "2020-01-01",
            "2020-01-01[u-ca=iso8601]",
            "2020-01-01T00:00:00.000000000",
            "2020-01-01T00:00:00.000000000[u-ca=iso8601]",
            "01-01",
            "01-01[u-ca=iso8601]",
            "2020-01",
            "2020-01[u-ca=iso8601]",
          ];
          for (const calendar of calendars) {
            const arg = { year: 1976, monthCode: "M11", day: 18, calendar };
            if (instance.equals(arg) !== true) {
              return false;
            }
          }
          return true;
        })()
        "#,
    );
}

/// A non-ISO annotated string still resolves to the *annotated* calendar,
/// not `iso8601` — the bracket-based path this fix's rewrite preserves.
#[test]
fn annotated_non_iso_calendar_string_resolves_to_that_calendar() {
    assert_true(
        r#"
        (function() {
          const arg = { year: 1976, monthCode: "M11", day: 18, calendar: "2024-05-16[u-ca=hebrew]" };
          const date = Temporal.PlainDate.from(arg);
          return date.calendarId === "hebrew";
        })()
        "#,
    );
}

/// A bare calendar ID (no ISO grammar shape at all) still resolves via the
/// plain calendar-name lookup fallback.
#[test]
fn bare_calendar_id_still_resolves() {
    assert_true(
        r#"
        (function() {
          const date = Temporal.PlainDate.from({ year: 1976, monthCode: "M11", day: 18, calendar: "gregory" });
          return date.calendarId === "gregory";
        })()
        "#,
    );
}
