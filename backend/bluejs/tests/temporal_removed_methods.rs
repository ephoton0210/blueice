// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Temporal API removals that reached TC39 consensus in June 2024
//! (`staging/Temporal/removed-methods.js`, Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `ZonedDateTime.prototype` still exposed `getISOFields`, `toPlainMonthDay`
//! and `toPlainYearMonth` after the rest of the list had gone. Each removed
//! name is asserted absent by property presence, not by calling it, and the
//! sibling members that *are* still specified are asserted present so a
//! removal cannot over-reach (`PlainDate.prototype.toPlainMonthDay` and
//! `toPlainYearMonth` are current spec; a `ZonedDateTime` reaches them through
//! `toPlainDate()`).

use blueice_bluejs::{compile, parse, Value, Vm};

fn run(source: &str) {
    let script = format!(
        r#"(function() {{
const failures = [];
{source}
return failures.length === 0 ? "ok" : "\n" + failures.join("\n");
}})()"#
    );
    let value = Vm::default()
        .execute(&compile(&parse(&script).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{script}\n  -> {error:?}"));
    match value {
        Value::String(text) if text == "ok" => {}
        Value::String(text) => panic!("mismatches:{}", text.to_utf8().unwrap()),
        other => panic!("expected \"ok\" or a mismatch list, got {other:?}"),
    }
}

#[test]
fn removed_members_are_absent() {
    run(r#"
      const removed = [
        [Temporal, "Temporal", ["Calendar", "TimeZone"]],
        [Temporal.Instant, "Instant", ["fromEpochMicroseconds", "fromEpochSeconds"]],
        [Temporal.Instant.prototype, "Instant.prototype",
          ["epochMicroseconds", "epochSeconds", "toZonedDateTime"]],
        [Temporal.Now, "Now", ["plainDate", "plainDateTime", "zonedDateTime"]],
        [Temporal.PlainDate.prototype, "PlainDate.prototype", ["getCalendar", "getISOFields"]],
        [Temporal.PlainDateTime.prototype, "PlainDateTime.prototype",
          ["getCalendar", "getISOFields", "toPlainMonthDay", "toPlainYearMonth", "withPlainDate"]],
        [Temporal.PlainMonthDay.prototype, "PlainMonthDay.prototype", ["getCalendar", "getISOFields"]],
        [Temporal.PlainTime.prototype, "PlainTime.prototype",
          ["getISOFields", "toPlainDateTime", "toZonedDateTime"]],
        [Temporal.PlainYearMonth.prototype, "PlainYearMonth.prototype", ["getCalendar", "getISOFields"]],
        [Temporal.ZonedDateTime.prototype, "ZonedDateTime.prototype",
          ["epochMicroseconds", "epochSeconds", "getCalendar", "getISOFields", "getTimeZone",
           "toPlainMonthDay", "toPlainYearMonth", "withPlainDate"]],
      ];
      for (const [target, label, names] of removed) {
        for (const name of names) {
          if (name in target) failures.push(label + "." + name + " should not exist");
        }
      }
    "#);
}

#[test]
fn the_members_that_remain_specified_are_still_present() {
    run(r#"
      const zoned = ["toInstant", "toPlainDate", "toPlainTime", "toPlainDateTime", "startOfDay",
        "getTimeZoneTransition", "withCalendar", "withPlainTime", "withTimeZone"];
      for (const name of zoned) {
        if (typeof Temporal.ZonedDateTime.prototype[name] !== "function") {
          failures.push("ZonedDateTime.prototype." + name + " is missing");
        }
      }
      for (const name of ["toPlainMonthDay", "toPlainYearMonth", "toPlainDateTime", "toZonedDateTime"]) {
        if (typeof Temporal.PlainDate.prototype[name] !== "function") {
          failures.push("PlainDate.prototype." + name + " is missing");
        }
      }
      if (typeof Temporal.PlainDateTime.prototype.toZonedDateTime !== "function") {
        failures.push("PlainDateTime.prototype.toZonedDateTime is missing");
      }
    "#);
}

/// The replacement route for the removed `ZonedDateTime` conversions keeps the
/// zone's wall-clock date and the calendar.
#[test]
fn a_zoned_date_time_reaches_year_month_and_month_day_through_to_plain_date() {
    run(r#"
      const z = Temporal.ZonedDateTime.from("2020-06-15T23:30:00-04:00[America/New_York][u-ca=gregory]");
      const md = z.toPlainDate().toPlainMonthDay();
      const ym = z.toPlainDate().toPlainYearMonth();
      if (md.monthCode !== "M06" || md.day !== 15 || md.calendarId !== "gregory") {
        failures.push("month-day " + md.toString({ calendarName: "always" }));
      }
      if (ym.year !== 2020 || ym.month !== 6 || ym.calendarId !== "gregory") {
        failures.push("year-month " + ym.toString({ calendarName: "always" }));
      }
    "#);
}
