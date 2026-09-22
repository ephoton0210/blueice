// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `PlainDate`/`PlainDateTime` conversion, comparison
//! and `with` behaviours (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`):
//!
//! - `PlainDate.compare` compared the time-of-day a *string* argument had
//!   parsed into its value, so `"2000-05-02T15:23"` was not equal to
//!   `2000-05-02` (`compare/argument-string-time-separators.js`,
//!   `compare/leap-second.js`). A `PlainDate` has no time.
//! - `PlainDate.prototype.toPlainMonthDay` on the ISO calendar kept the date's
//!   own year, but `CalendarMonthDayFromFields` uses the reference ISO year
//!   1972 (`toPlainMonthDay/basic.js`).
//! - `PlainDateTime.prototype.with` bounded each time field (`minute` 0..=59)
//!   instead of applying `RegulateTime`, so `with({ minute: 67 })` threw even
//!   under the default `overflow: "constrain"` (`with/overflow-undefined.js`);
//!   its month/day reads were also truncated to `u8` instead of constrained.
//! - `withCalendar()` defaulted a missing argument to `"iso8601"`, but
//!   `ToTemporalCalendarIdentifier` throws a `TypeError` for `undefined`
//!   (`withCalendar/missing-argument.js`).
//! - `PlainDate.from` / `PlainDateTime.from` of a `PlainDateTime`, `PlainDate`
//!   or `ZonedDateTime` read its getters (`year`, `month`, ...) instead of its
//!   internal slots (`from/argument-plaindatetime.js`,
//!   `from/argument-plaindate.js`, `from/argument-zoneddatetime-slots.js`).

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn evaluate_err(source: &str) -> RuntimeError {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .expect_err(&format!("{source}\n  -> expected an error, got a value"))
}

fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

fn assert_string(source: &str, expected: &str) {
    match evaluate(source) {
        Value::String(actual) => assert_eq!(actual.to_utf8().unwrap(), expected, "{source}"),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

fn assert_range_error(source: &str) {
    assert!(
        matches!(evaluate_err(source), RuntimeError::RangeError(_)),
        "{source}"
    );
}

fn assert_type_error(source: &str) {
    assert!(
        matches!(evaluate_err(source), RuntimeError::TypeError(_)),
        "{source}"
    );
}

/// The time-of-day in a string argument is ignored by `PlainDate.compare`, in
/// either position, for every date/time separator and for a leap second.
#[test]
fn plain_date_compare_ignores_the_time_of_a_string_argument() {
    assert_true(
        r#"(function() {
          const date = new Temporal.PlainDate(2000, 5, 2);
          for (const text of ["2000-05-02T15:23", "2000-05-02t15:23", "2000-05-02 15:23",
                              "2000-05-02T23:59:59.999999999", "2000-05-02T00:00:00"]) {
            if (Temporal.PlainDate.compare(text, date) !== 0) return "first " + text;
            if (Temporal.PlainDate.compare(date, text) !== 0) return "second " + text;
          }
          return true;
        })()"#,
    );
    assert_true(
        r#"(function() {
          const date = new Temporal.PlainDate(2016, 12, 31);
          const inputs = ["2016-12-31T23:59:60",
                          { year: 2016, month: 12, day: 31, hour: 23, minute: 59, second: 60 }];
          for (const arg of inputs) {
            if (Temporal.PlainDate.compare(arg, date) !== 0) return "first";
            if (Temporal.PlainDate.compare(date, arg) !== 0) return "second";
          }
          return true;
        })()"#,
    );
}

/// The date still decides the order: only the *time* stopped mattering.
#[test]
fn plain_date_compare_still_orders_by_date() {
    assert_true(
        r#"Temporal.PlainDate.compare("2000-05-03T00:00", new Temporal.PlainDate(2000, 5, 2)) === 1
        && Temporal.PlainDate.compare("2000-05-01T23:59", new Temporal.PlainDate(2000, 5, 2)) === -1
        && Temporal.PlainDate.compare(new Temporal.PlainDate(2000, 5, 2), new Temporal.PlainDate(2000, 5, 2)) === 0"#,
    );
}

/// `PlainDateTime.compare` and `PlainDate.prototype.equals` keep their own rules.
#[test]
fn date_time_compare_and_equals_still_use_the_time() {
    assert_true(
        r#"Temporal.PlainDateTime.compare("2000-05-02T15:23", new Temporal.PlainDateTime(2000, 5, 2, 15, 23)) === 0
        && Temporal.PlainDateTime.compare("2000-05-02T15:24", new Temporal.PlainDateTime(2000, 5, 2, 15, 23)) === 1
        && new Temporal.PlainDate(2000, 5, 2).equals("2000-05-02T15:23")
        && !new Temporal.PlainDateTime(2000, 5, 2, 1).equals(new Temporal.PlainDateTime(2000, 5, 2, 2))"#,
    );
}

/// A `PlainDate` built from a string with a time also has no time to leak into
/// a conversion back to `PlainDateTime`.
#[test]
fn a_plain_date_parsed_from_a_string_with_a_time_converts_at_midnight() {
    assert_string(
        r#"Temporal.PlainDate.from("2000-05-02T15:23").toPlainDateTime().toString()"#,
        "2000-05-02T00:00:00",
    );
}

/// `toPlainMonthDay/basic.js`: an ISO `PlainMonthDay`'s reference ISO year is 1972.
#[test]
fn to_plain_month_day_uses_the_reference_iso_year() {
    assert_true(
        r#"(function() {
          const monthDay = new Temporal.PlainDate(1970, 12, 24, "iso8601").toPlainMonthDay();
          return monthDay.monthCode === "M12" && monthDay.day === 24
              && monthDay.toString({ calendarName: "always" }) === "1972-12-24[u-ca=iso8601]";
        })()"#,
    );
    // Feb 29 exists in the reference year 1972; other years do not change that.
    assert_string(
        r#"new Temporal.PlainDate(2000, 2, 29).toPlainMonthDay().toString({ calendarName: "always" })"#,
        "1972-02-29[u-ca=iso8601]",
    );
    assert_string(
        r#"new Temporal.PlainDate(-5000, 7, 4).toPlainMonthDay().toString({ calendarName: "always" })"#,
        "1972-07-04[u-ca=iso8601]",
    );
    assert_string(
        r#"new Temporal.PlainDateTime(2021, 1, 31, 5).toPlainDate().toPlainMonthDay().toString({ calendarName: "always" })"#,
        "1972-01-31[u-ca=iso8601]",
    );
}

/// `with/overflow-undefined.js`: the default, an explicit `undefined`, a plain
/// object and a function all mean `constrain`.
#[test]
fn plain_date_time_with_constrains_time_fields_by_default() {
    let datetime = "new Temporal.PlainDateTime(2000, 5, 2, 12)";
    for options in [", { overflow: undefined }", ", {}", ", () => {}", ""] {
        assert_string(
            &format!("{datetime}.with({{ minute: 67 }}{options}).toString()"),
            "2000-05-02T12:59:00",
        );
    }
    assert_string(
        &format!(
            "{datetime}.with({{ hour: 99, minute: 99, second: 99, millisecond: 9999, \
             microsecond: 9999, nanosecond: 9999 }}).toString()"
        ),
        "2000-05-02T23:59:59.999999999",
    );
    // Negative and huge finite values clamp too (`RegulateTime`).
    assert_string(
        &format!("{datetime}.with({{ hour: -3, second: 1e12 }}).toString()"),
        "2000-05-02T00:00:59",
    );
}

/// `overflow: "reject"` still rejects an out-of-range time field, and every
/// field is still validated as a number.
#[test]
fn plain_date_time_with_reject_and_invalid_values_still_throw() {
    let datetime = "new Temporal.PlainDateTime(2000, 5, 2, 12)";
    for field in [
        "hour: 24",
        "minute: 60",
        "second: 60",
        "millisecond: 1000",
        "microsecond: 1000",
        "nanosecond: 1000",
        "hour: -1",
    ] {
        assert_range_error(&format!(
            r#"{datetime}.with({{ {field} }}, {{ overflow: "reject" }})"#
        ));
    }
    assert_range_error(&format!("{datetime}.with({{ minute: Infinity }})"));
    assert_range_error(&format!("{datetime}.with({{ minute: -Infinity }})"));
    assert_range_error(&format!("{datetime}.with({{ minute: NaN }})"));
    assert_range_error(&format!(
        r#"{datetime}.with({{ minute: 5 }}, {{ overflow: "bad" }})"#
    ));
    // A fractional value truncates: 5.9 minutes is minute 5.
    assert_string(
        &format!("{datetime}.with({{ minute: 5.9 }}).toString()"),
        "2000-05-02T12:05:00",
    );
}

/// The same defaulting for the date fields: month and day are constrained by
/// the calendar, however large the number (they were truncated through `u8`).
#[test]
fn plain_date_with_constrains_large_month_and_day_values() {
    let date = "new Temporal.PlainDate(2021, 3, 15)";
    assert_string(
        &format!("{date}.with({{ month: 13 }}).toString()"),
        "2021-12-15",
    );
    assert_string(
        &format!("{date}.with({{ month: 999999 }}).toString()"),
        "2021-12-15",
    );
    // 259 and 261 are 3 and 5 modulo 256: truncating through `u8` picked March/the 5th.
    assert_string(
        &format!("{date}.with({{ month: 259 }}).toString()"),
        "2021-12-15",
    );
    assert_string(
        &format!("{date}.with({{ day: 261 }}).toString()"),
        "2021-03-31",
    );
    assert_string(
        &format!("{date}.with({{ day: 4294967 }}).toString()"),
        "2021-03-31",
    );
    assert_range_error(&format!(
        r#"{date}.with({{ month: 13 }}, {{ overflow: "reject" }})"#
    ));
    assert_range_error(&format!(
        r#"{date}.with({{ day: 300 }}, {{ overflow: "reject" }})"#
    ));
    assert_range_error(&format!(r#"{date}.with({{ month: 0 }})"#));
    assert_range_error(&format!(r#"{date}.with({{ day: 0 }})"#));
}

/// `withCalendar/missing-argument.js`, for both receivers (and the zoned one).
#[test]
fn with_calendar_requires_an_argument() {
    for receiver in [
        r#"Temporal.PlainDate.from("1976-11-18")"#,
        r#"Temporal.PlainDateTime.from("1976-11-18T15:23")"#,
        r#"Temporal.ZonedDateTime.from("1976-11-18T15:23+00:00[UTC]")"#,
    ] {
        assert_type_error(&format!("{receiver}.withCalendar()"));
        assert_type_error(&format!("{receiver}.withCalendar(undefined)"));
        assert_type_error(&format!("{receiver}.withCalendar(null)"));
        assert_type_error(&format!("{receiver}.withCalendar(1)"));
        assert_string(
            &format!(r#"{receiver}.withCalendar("iso8601").calendarId"#),
            "iso8601",
        );
        assert_string(
            &format!(r#"{receiver}.withCalendar("gregory").calendarId"#),
            "gregory",
        );
    }
}

const OBSERVE_PLAIN_DATE_TIME: &str = r#"
  const log = [];
  const datetime = new Temporal.PlainDateTime(2000, 5, 2, 12, 34, 56, 987, 654, 321, "iso8601");
  const descriptors = Object.getOwnPropertyDescriptors(Temporal.PlainDateTime.prototype);
  for (const property of ["year", "month", "monthCode", "day", "hour", "minute", "second",
                          "millisecond", "microsecond", "nanosecond"]) {
    Object.defineProperty(datetime, property, {
      get() { log.push("get " + property); return descriptors[property].get.call(this); },
    });
  }
  Object.defineProperty(datetime, "calendar", { get() { log.push("get calendar"); return "iso8601"; } });
"#;

/// `PlainDate.from(plainDateTime)` reads slots, never getters.
#[test]
fn plain_date_from_a_plain_date_time_reads_slots() {
    assert_true(&format!(
        r#"(function() {{
          {OBSERVE_PLAIN_DATE_TIME}
          const result = Temporal.PlainDate.from(datetime);
          return log.length === 0 && result.toString() === "2000-05-02";
        }})()"#
    ));
    // `overflow` is still validated for a slot-carrying argument.
    assert_range_error(&format!(
        r#"(function() {{
          {OBSERVE_PLAIN_DATE_TIME}
          return Temporal.PlainDate.from(datetime, {{ overflow: "bad" }});
        }})()"#
    ));
}

/// `PlainDateTime.from(plainDate)` reads slots and assumes midnight.
#[test]
fn plain_date_time_from_a_plain_date_reads_slots() {
    assert_true(
        r#"(function() {
          const log = [];
          const date = new Temporal.PlainDate(2000, 5, 2, "iso8601");
          const descriptors = Object.getOwnPropertyDescriptors(Temporal.PlainDate.prototype);
          for (const property of ["year", "month", "monthCode", "day"]) {
            Object.defineProperty(date, property, {
              get() { log.push("get " + property); return descriptors[property].get.call(this); },
            });
          }
          for (const property of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) {
            Object.defineProperty(date, property, { get() { log.push("get " + property); return undefined; } });
          }
          Object.defineProperty(date, "calendar", { get() { log.push("get calendar"); return "iso8601"; } });
          const result = Temporal.PlainDateTime.from(date);
          return log.length === 0 && result.toString() === "2000-05-02T00:00:00"
              && result.calendarId === "iso8601";
        })()"#,
    );
}

/// `PlainDate.from(zonedDateTime)` / `PlainDateTime.from(zonedDateTime)` read
/// slots (`argument-zoneddatetime-slots.js`) and take the *local* wall-clock
/// fields, including in a named IANA zone.
#[test]
fn from_a_zoned_date_time_reads_local_slots() {
    assert_true(
        r#"(function() {
          const log = [];
          const descriptors = Object.getOwnPropertyDescriptors(Temporal.ZonedDateTime.prototype);
          const getters = ["year", "month", "monthCode", "day", "hour", "minute", "second",
                           "millisecond", "microsecond", "nanosecond", "calendarId"];
          for (const property of getters) {
            Object.defineProperty(Temporal.ZonedDateTime.prototype, property, {
              get() { log.push("get " + property); return descriptors[property].get.call(this); },
            });
          }
          const zdt = new Temporal.ZonedDateTime(0n, "UTC");
          const date = Temporal.PlainDate.from(zdt);
          const datetime = Temporal.PlainDateTime.from(zdt);
          return log.length === 0 && date.toString() === "1970-01-01"
              && datetime.toString() === "1970-01-01T00:00:00";
        })()"#,
    );
    // 23:30 PST is already the next UTC day, so a UTC-based conversion is off by one.
    assert_string(
        r#"Temporal.PlainDate.from(Temporal.ZonedDateTime.from("2020-01-15T23:30:00-08:00[America/Los_Angeles]")).toString()"#,
        "2020-01-15",
    );
    assert_string(
        r#"Temporal.PlainDateTime.from(Temporal.ZonedDateTime.from("2020-01-15T23:30:00-08:00[America/Los_Angeles]")).toString()"#,
        "2020-01-15T23:30:00",
    );
    // 01:30 at +14:00 is still the previous UTC day.
    assert_string(
        r#"Temporal.PlainDate.from(Temporal.ZonedDateTime.from("2020-03-08T01:30:00+14:00[Pacific/Kiritimati]")).toString()"#,
        "2020-03-08",
    );
    assert_string(
        r#"Temporal.PlainDateTime.from(Temporal.ZonedDateTime.from("2020-11-01T01:30:00-08:00[America/Los_Angeles]")).toString()"#,
        "2020-11-01T01:30:00",
    );
    // A non-ISO calendar carries over.
    assert_string(
        r#"Temporal.PlainDate.from(Temporal.ZonedDateTime.from("2020-03-08T12:00:00+00:00[UTC][u-ca=gregory]")).calendarId"#,
        "gregory",
    );
}

/// Objects that are *not* PlainDate/PlainDateTime/ZonedDateTime slots are
/// still read as property bags: a `PlainYearMonth` has `year`/`month`, no `day`.
#[test]
fn other_temporal_objects_are_still_property_bags() {
    assert_type_error("Temporal.PlainDate.from(new Temporal.PlainYearMonth(2000, 5))");
    assert_string(
        r#"Temporal.PlainDate.from({ year: 2000, month: 5, day: 2 }).toString()"#,
        "2000-05-02",
    );
    assert_type_error("Temporal.PlainDate.from(new Temporal.PlainTime(1, 2))");
    assert_range_error(r#"Temporal.PlainDate.from("not a date")"#);
}

/// `PlainTime` reads a `ZonedDateTime`'s slots the same way: the local time of
/// day in any zone -- a named IANA zone used to be a `RangeError` ("supports
/// UTC and fixed offsets"). 23:30 PST is 07:30 UTC the next day.
#[test]
fn plain_time_from_a_zoned_date_time_reads_the_local_time() {
    let los_angeles = r#"Temporal.ZonedDateTime.from("2020-01-15T23:30:45.123456789-08:00[America/Los_Angeles]")"#;
    assert_string(
        &format!("Temporal.PlainTime.from({los_angeles}).toString()"),
        "23:30:45.123456789",
    );
    assert_string(
        r#"Temporal.PlainTime.from(Temporal.ZonedDateTime.from("2020-03-08T01:30:00+14:00[Pacific/Kiritimati]")).toString()"#,
        "01:30:00",
    );
    assert_string(
        r#"Temporal.PlainTime.from(new Temporal.ZonedDateTime(0n, "+05:30")).toString()"#,
        "05:30:00",
    );
    // `compare`/`equals` go through the same conversion.
    assert_true(&format!(
        r#"Temporal.PlainTime.compare({los_angeles}, "23:30:45.123456789") === 0
        && new Temporal.PlainTime(23, 30, 45, 123, 456, 789).equals({los_angeles})"#
    ));
}

/// `ToIntegerWithTruncation` happens before `RegulateTime` clamps: 24.9 hours
/// truncates to hour 24 (out of range), which `constrain` clamps to 23 and
/// `reject` rejects; 23.9 truncates to a valid 23, accepted under both.
#[test]
fn with_truncates_a_fractional_time_field_before_regulating_it() {
    let datetime = "new Temporal.PlainDateTime(2000, 5, 2, 12)";
    assert_string(
        &format!("{datetime}.with({{ hour: 24.9 }}).toString()"),
        "2000-05-02T23:00:00",
    );
    assert_string(
        &format!("{datetime}.with({{ hour: 23.9 }}).toString()"),
        "2000-05-02T23:00:00",
    );
    assert_range_error(&format!(
        r#"{datetime}.with({{ hour: 24.9 }}, {{ overflow: "reject" }})"#
    ));
    assert_string(
        &format!(r#"{datetime}.with({{ hour: 23.9 }}, {{ overflow: "reject" }}).toString()"#),
        "2000-05-02T23:00:00",
    );
}
