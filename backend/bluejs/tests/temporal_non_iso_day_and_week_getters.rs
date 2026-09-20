// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the `dayOfYear`, `weekOfYear` and `yearOfWeek`
//! getters on non-ISO calendars (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `conversion/getters.rs` computed all three from the value's stored ISO
//! fields for every calendar. That is only right for `iso8601`:
//!
//! - `dayOfYear` is a position within the *calendar's own* year, so a Hebrew
//!   or Coptic date is not `iso_day_of_year` of its ISO reference date
//!   (`intl402/Temporal/{PlainDate,PlainDateTime,ZonedDateTime}/prototype/
//!   dayOfYear/non-iso-calendar-basic.js` walks every day of a year in fifteen
//!   calendars and expects 1, 2, 3, ...).
//! - `weekOfYear` and `yearOfWeek` are ISO-8601 week numbering. Calendars
//!   without a well-defined week numbering return `undefined` — every
//!   supported non-ISO calendar, `gregory` included
//!   (`.../weekOfYear/non-iso-week-of-year.js`, `.../yearOfWeek/
//!   non-iso-week-of-year.js`, and `ZonedDateTime/construct-non-utc-non-iso.js`).
//!
//! `dayOfWeek` and `daysInWeek` are calendar-invariant and stay numeric.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// Every script ends in `"ok"` or a newline-joined list of mismatches.
fn assert_ok(source: &str) {
    match evaluate(source) {
        Value::String(text) if text == "ok" => {}
        Value::String(text) => panic!("mismatches:{}\n{source}", text.to_utf8().unwrap()),
        other => panic!("expected \"ok\" or a mismatch list, got {other:?}\n{source}"),
    }
}

/// `dayOfYear/non-iso-calendar-basic.js`'s calendar/year table (each year is
/// the one containing ISO 1970), walked day by day through the whole year.
#[test]
fn day_of_year_counts_from_the_first_day_of_the_calendars_own_year() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  const sampleYears = {
    buddhist: 2513, chinese: 1969, coptic: 1686, dangi: 1969, ethioaa: 7462,
    ethiopic: 1962, gregory: 1970, hebrew: 5730, indian: 1891, "islamic-civil": 1389,
    "islamic-tbla": 1389, "islamic-umalqura": 1389, japanese: 1970, persian: 1348, roc: 59,
  };
  const oneDay = new Temporal.Duration(0, 0, 0, 1);
  for (const [calendar, year] of Object.entries(sampleYears)) {
    let date = Temporal.PlainDate.from({ year, month: 1, day: 1, calendar });
    let expected = 1;
    while (date.year === year) {
      if (date.dayOfYear !== expected) {
        failures.push(calendar + " " + date.monthCode + "-" + date.day + ": dayOfYear " + date.dayOfYear + ", expected " + expected);
        break;
      }
      date = date.add(oneDay);
      expected++;
    }
    // The walk must end exactly on the calendar's own year length.
    const first = Temporal.PlainDate.from({ year, month: 1, day: 1, calendar });
    if (expected - 1 !== first.daysInYear) {
      failures.push(calendar + ": walked " + (expected - 1) + " days, daysInYear " + first.daysInYear);
    }
  }
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// Every date-carrying type reads the same getters; `ZonedDateTime` uses its
/// *local* date (the wall-clock date in the zone), not the UTC one.
#[test]
fn day_of_year_agrees_across_plain_date_plain_date_time_and_zoned_date_time() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  const check = (label, got, expected) => {
    if (got !== expected) failures.push(label + ": expected " + expected + " got " + got);
  };
  // 1976-11-18 is gregory day 323 (`ZonedDateTime/construct-non-utc-non-iso.js`).
  const date = new Temporal.PlainDate(1976, 11, 18, "gregory");
  check("PlainDate gregory", date.dayOfYear, 323);
  check("PlainDateTime gregory", new Temporal.PlainDateTime(1976, 11, 18, 15, 23, 0, 0, 0, 0, "gregory").dayOfYear, 323);
  check("ZonedDateTime gregory", new Temporal.ZonedDateTime(217178610123456789n, "Europe/Vienna", "gregory").dayOfYear, 323);
  // 23:00Z is already midnight of the 19th in Vienna (+01:00): the local date counts.
  const late = new Temporal.ZonedDateTime(217206000000000000n, "Europe/Vienna", "gregory");
  check("ZonedDateTime local date", late.dayOfYear, 324);
  // In a calendar whose year does not start on ISO January 1st, dayOfYear is
  // still the distance from that calendar's own first day of the year, plus one.
  for (const calendar of ["coptic", "ethiopic", "persian", "indian", "islamic-civil", "hebrew", "chinese"]) {
    const local = date.withCalendar(calendar);
    const first = Temporal.PlainDate.from({ year: local.year, month: 1, day: 1, calendar });
    check(calendar + " PlainDate", local.dayOfYear, first.until(local, { largestUnit: "days" }).days + 1);
    check(calendar + " ZonedDateTime", late.withCalendar(calendar).dayOfYear, first.until(local.add({ days: 1 }), { largestUnit: "days" }).days + 1);
  }
  // Hebrew Tishrei 1 is day 1 of the Hebrew year, whatever its ISO date.
  const roshHashanah = Temporal.PlainDate.from({ year: 5784, monthCode: "M01", day: 1, calendar: "hebrew" });
  check("hebrew M01-01", roshHashanah.dayOfYear, 1);
  check("hebrew last day", roshHashanah.add({ days: roshHashanah.daysInYear - 1 }).dayOfYear, roshHashanah.daysInYear);
  // ISO stays the ISO ordinal.
  check("iso8601", new Temporal.PlainDate(2024, 3, 1).dayOfYear, 61);
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// `weekOfYear/non-iso-week-of-year.js` and `yearOfWeek/non-iso-week-of-year.js`
/// for all three date-carrying types, plus `ZonedDateTime/construct-non-utc-
/// non-iso.js`'s `weekOfYear`/`yearOfWeek`/`daysInWeek` expectations.
#[test]
fn week_numbering_is_undefined_outside_the_iso_calendar() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  const calendars = ["buddhist", "chinese", "coptic", "dangi", "ethioaa", "ethiopic", "gregory", "hebrew",
                     "indian", "islamic-civil", "islamic-tbla", "islamic-umalqura", "japanese", "persian", "roc"];
  for (const calendar of calendars) {
    const values = {
      PlainDate: new Temporal.PlainDate(2024, 1, 1, calendar),
      PlainDateTime: new Temporal.PlainDateTime(2024, 1, 1, 12, 0, 0, 0, 0, 0, calendar),
      ZonedDateTime: new Temporal.ZonedDateTime(1704110400000000000n, "UTC", calendar),
    };
    for (const [type, value] of Object.entries(values)) {
      if (value.weekOfYear !== undefined) failures.push(type + " " + calendar + " weekOfYear: " + value.weekOfYear);
      if (value.yearOfWeek !== undefined) failures.push(type + " " + calendar + " yearOfWeek: " + value.yearOfWeek);
      // Calendar-invariant getters keep their numeric answers.
      if (value.dayOfWeek !== 1) failures.push(type + " " + calendar + " dayOfWeek: " + value.dayOfWeek);
      if (value.daysInWeek !== 7) failures.push(type + " " + calendar + " daysInWeek: " + value.daysInWeek);
    }
  }
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// The ISO calendar is untouched: week numbers still follow ISO 8601,
/// including the year-boundary weeks that belong to the neighbouring year.
#[test]
fn iso_week_numbering_is_unchanged() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  const check = (label, got, expected) => {
    if (got !== expected) failures.push(label + ": expected " + expected + " got " + got);
  };
  // 2021-01-01 (Friday) belongs to ISO week 53 of 2020; 2018-12-31 to week 1 of 2019.
  const d2021 = new Temporal.PlainDate(2021, 1, 1);
  check("2021-01-01 week", d2021.weekOfYear, 53);
  check("2021-01-01 yearOfWeek", d2021.yearOfWeek, 2020);
  const d2018 = new Temporal.PlainDate(2018, 12, 31);
  check("2018-12-31 week", d2018.weekOfYear, 1);
  check("2018-12-31 yearOfWeek", d2018.yearOfWeek, 2019);
  const zoned = new Temporal.ZonedDateTime(1609459200000000000n, "UTC");
  check("zoned week", zoned.weekOfYear, 53);
  check("zoned yearOfWeek", zoned.yearOfWeek, 2020);
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}
