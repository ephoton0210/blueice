// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.Duration.prototype.round`/`total` with a
//! `PlainDate` `relativeTo`.
//!
//! The specification (`Duration.prototype.round` step 28, `total` step 13) adds
//! the whole duration to the anchor -- its days folded with the time part at 24
//! hours each -- and then takes `DifferencePlainDateTimeWithRounding` /
//! `DifferencePlainDateTimeWithTotal` between the anchor at midnight and the
//! landing date-time. Before this fix `round`/`total` carried a second,
//! hand-rolled copy of that algorithm that:
//!
//! * decided `NudgeToCalendarUnit`'s window from the *record's* raw
//!   years/months/weeks/days split instead of the difference between the two
//!   date-times, so a duration whose fields disagreed with where it really
//!   landed (`P1Y1H` from a leap day, `P1M10D` from January 31st) was measured
//!   against the wrong bracket (`rounding-window.js`);
//! * did the exact `total` fraction in `f64` arithmetic, drifting by a ULP
//!   (`precision-exact-mathematical-values-5.js`);
//! * did not fold days into a time `largestUnit` (`relativeto-largestunit-
//!   smallestunit-combinations.js`); and
//! * split months into years with a constant 12 (or 13) months per year, which
//!   is wrong for the lunisolar calendars whose leap years have an extra month.
//!
//! Every expectation is either taken from a real Test262 fixture (named at the
//! test) or derived by hand from the spec algorithm, as noted.

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
function assertDuration(actual, y, mo, w, d, h, mi, s, ms, us, ns, message) {
  const got = [actual.years, actual.months, actual.weeks, actual.days, actual.hours,
    actual.minutes, actual.seconds, actual.milliseconds, actual.microseconds, actual.nanoseconds];
  const want = [y, mo, w, d, h, mi, s, ms, us, ns];
  for (let i = 0; i < 10; i++) {
    if (got[i] !== want[i]) {
      throw new Error(message + ": got " + got.join(",") + " want " + want.join(","));
    }
  }
}
function assertSame(actual, expected, message) {
  if (!Object.is(actual, expected)) {
    throw new Error(message + ": got " + String(actual) + " want " + String(expected));
  }
}
function assertRangeError(fn, message) {
  try { fn(); } catch (e) {
    if (e instanceof RangeError) return;
    throw new Error(message + ": expected RangeError, got " + e);
  }
  throw new Error(message + ": expected RangeError, nothing thrown");
}
"#;

/// Runs `body` after [`PRELUDE`], failing with the JS-side message on a throw.
fn check(body: &str) {
    let source =
        format!("{PRELUDE}\ntry {{\n{body}\n\"ok\"\n}} catch (e) {{ \"FAILED: \" + e.message }}");
    let value = Vm::default()
        .execute(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{body}\n  -> {error:?}"));
    match value {
        Value::String(text) => assert_eq!(text.to_utf8().unwrap(), "ok", "{body}"),
        other => panic!("unexpected completion value {other:?}"),
    }
}

/// `built-ins/Temporal/Duration/prototype/round/rounding-window.js`
/// (https://github.com/tc39/proposal-temporal/issues/3168).
#[test]
fn round_measures_the_rounding_window_from_where_the_duration_lands() {
    check(
        r#"
        // 2020-02-29 + 1 year is 2021-02-28 (constrained); the extra hour is far below half a year.
        let d = new Temporal.Duration(1, 0, 0, 0, 1);
        let relativeTo = new Temporal.PlainDate(2020, 2, 29);
        assertDuration(d.round({ smallestUnit: "years", relativeTo }), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, "1y1h -> years");

        // 2020-01-31 + 1 month is 2020-02-29 (constrained); `expand` rounds the leftover 10 hours
        // up to a second month, even though they are a sliver of that 31-day window.
        d = new Temporal.Duration(0, 1, 0, 0, 10);
        relativeTo = new Temporal.PlainDate(2020, 1, 31);
        assertDuration(d.round({ smallestUnit: "months", roundingMode: "expand", relativeTo }),
          0, 2, 0, 0, 0, 0, 0, 0, 0, 0, "1 month 10 hours, expand -> 2 months");

        d = new Temporal.Duration(2345, 0, 0, 0, 12);
        relativeTo = new Temporal.PlainDate(2020, 2, 29);
        assertDuration(d.round({ smallestUnit: "years", roundingMode: "expand", relativeTo }),
          2346, 0, 0, 0, 0, 0, 0, 0, 0, 0, "2345y12h expand -> 2346 years");

        d = new Temporal.Duration(1);
        relativeTo = new Temporal.PlainDate(2020, 2, 29);
        assertDuration(d.round({ smallestUnit: "months", relativeTo }), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
          "a whole year is not re-expressed as 12 months");
    "#,
    );
}

/// `built-ins/Temporal/Duration/prototype/total/rounding-window.js` and
/// `.../total/precision-exact-mathematical-values-5.js`: the fraction is one
/// correctly-rounded division of exact nanosecond counts.
#[test]
fn total_is_one_correctly_rounded_division_of_exact_nanoseconds() {
    check(
        r#"
        let d = new Temporal.Duration(1, 0, 0, 0, 1);
        let relativeTo = new Temporal.PlainDate(2020, 2, 29);
        assertSame(d.total({ unit: "years", relativeTo }), 1.0001141552511414, "years");

        d = new Temporal.Duration(0, 1, 0, 0, 10);
        relativeTo = new Temporal.PlainDate(2020, 1, 31);
        assertSame(d.total({ unit: "months", relativeTo }), 1.0134408602150538, "months");

        // P5W5D from 1972-01-31: 40 days of a month window that runs 1972-02-29 .. 1972-03-31.
        // (dest - start) / (end - start) = 1 + 11/31 ~ 1.3548387096774193 as a single division.
        d = new Temporal.Duration(0, 0, 5, 5);
        assertSame(d.total({ unit: "months", relativeTo: "1972-01-31" }), 1.3548387096774193, "months 2");
    "#,
    );
}

/// `built-ins/Temporal/Duration/prototype/round/relativeto-largestunit-
/// smallestunit-combinations.js`: every `largestUnit`/`smallestUnit` pairing of
/// P5Y5M5W5DT5H5M5S5ms5us5ns relative to 2000-01-01 (and to the same instant
/// as a UTC `ZonedDateTime`).
#[test]
fn round_reports_every_largest_and_smallest_unit_combination() {
    check(
        r#"
        const duration = new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5);
        const plainRelativeTo = new Temporal.PlainDate(2000, 1, 1);
        const zonedRelativeTo = new Temporal.ZonedDateTime(63072000_000_000_000n, "UTC");
        const exactResults = {
          years: { years: [6], months: [5, 6], weeks: [5, 6, 1], days: [5, 6, 0, 10], hours: [5, 6, 0, 10, 5],
            minutes: [5, 6, 0, 10, 5, 5], seconds: [5, 6, 0, 10, 5, 5, 5], milliseconds: [5, 6, 0, 10, 5, 5, 5, 5],
            microseconds: [5, 6, 0, 10, 5, 5, 5, 5, 5], nanoseconds: [5, 6, 0, 10, 5, 5, 5, 5, 5, 5] },
          months: { months: [0, 66], weeks: [0, 66, 1], days: [0, 66, 0, 10], hours: [0, 66, 0, 10, 5],
            minutes: [0, 66, 0, 10, 5, 5], seconds: [0, 66, 0, 10, 5, 5, 5], milliseconds: [0, 66, 0, 10, 5, 5, 5, 5],
            microseconds: [0, 66, 0, 10, 5, 5, 5, 5, 5], nanoseconds: [0, 66, 0, 10, 5, 5, 5, 5, 5, 5] },
          weeks: { weeks: [0, 0, 288], days: [0, 0, 288, 2], hours: [0, 0, 288, 2, 5], minutes: [0, 0, 288, 2, 5, 5],
            seconds: [0, 0, 288, 2, 5, 5, 5], milliseconds: [0, 0, 288, 2, 5, 5, 5, 5],
            microseconds: [0, 0, 288, 2, 5, 5, 5, 5, 5], nanoseconds: [0, 0, 288, 2, 5, 5, 5, 5, 5, 5] },
          days: { days: [0, 0, 0, 2018], hours: [0, 0, 0, 2018, 5], minutes: [0, 0, 0, 2018, 5, 5],
            seconds: [0, 0, 0, 2018, 5, 5, 5], milliseconds: [0, 0, 0, 2018, 5, 5, 5, 5],
            microseconds: [0, 0, 0, 2018, 5, 5, 5, 5, 5], nanoseconds: [0, 0, 0, 2018, 5, 5, 5, 5, 5, 5] },
          hours: { hours: [0, 0, 0, 0, 48437], minutes: [0, 0, 0, 0, 48437, 5], seconds: [0, 0, 0, 0, 48437, 5, 5],
            milliseconds: [0, 0, 0, 0, 48437, 5, 5, 5], microseconds: [0, 0, 0, 0, 48437, 5, 5, 5, 5],
            nanoseconds: [0, 0, 0, 0, 48437, 5, 5, 5, 5, 5] },
          minutes: { minutes: [0, 0, 0, 0, 0, 2906225], seconds: [0, 0, 0, 0, 0, 2906225, 5],
            milliseconds: [0, 0, 0, 0, 0, 2906225, 5, 5], microseconds: [0, 0, 0, 0, 0, 2906225, 5, 5, 5],
            nanoseconds: [0, 0, 0, 0, 0, 2906225, 5, 5, 5, 5] },
          seconds: { seconds: [0, 0, 0, 0, 0, 0, 174373505], milliseconds: [0, 0, 0, 0, 0, 0, 174373505, 5],
            microseconds: [0, 0, 0, 0, 0, 0, 174373505, 5, 5], nanoseconds: [0, 0, 0, 0, 0, 0, 174373505, 5, 5, 5] },
          milliseconds: { milliseconds: [0, 0, 0, 0, 0, 0, 0, 174373505005],
            microseconds: [0, 0, 0, 0, 0, 0, 0, 174373505005, 5], nanoseconds: [0, 0, 0, 0, 0, 0, 0, 174373505005, 5, 5] },
          microseconds: { microseconds: [0, 0, 0, 0, 0, 0, 0, 0, 174373505005005],
            nanoseconds: [0, 0, 0, 0, 0, 0, 0, 0, 174373505005005, 5] },
        };
        for (const [largestUnit, entry] of Object.entries(exactResults)) {
          for (const [smallestUnit, expected] of Object.entries(entry)) {
            for (const relativeTo of [plainRelativeTo, zonedRelativeTo]) {
              const [y, mon = 0, w = 0, d = 0, h = 0, min = 0, s = 0, ms = 0, us = 0, ns = 0] = expected;
              assertDuration(duration.round({ largestUnit, smallestUnit, relativeTo }),
                y, mon, w, d, h, min, s, ms, us, ns,
                "largestUnit " + largestUnit + ", smallestUnit " + smallestUnit + ", relativeTo " + (relativeTo === plainRelativeTo ? "plain" : "zoned"));
            }
          }
        }
        for (const relativeTo of [plainRelativeTo, zonedRelativeTo]) {
          assertDuration(duration.round({ largestUnit: "nanoseconds", smallestUnit: "nanoseconds", relativeTo }),
            0, 0, 0, 0, 0, 0, 0, 0, 0, 174373505005004992, "nanoseconds with precision loss");
        }
    "#,
    );
}

/// Hebrew 5784 is a leap year (position 8 of the 19-year cycle, 5784 mod 19 = 8):
/// 13 months, Tishrei .. Elul with Adar I and Adar II. Anchoring at 1 Tishrei 5784,
/// `CalendarDateAdd` by `n` months follows that real month sequence, so 12
/// months lands on 1 Elul 5784 (still inside the year) and 13 months on
/// 1 Tishrei 5785 -- a year. Hand-derived from `DifferencePlainDateTimeWith
/// Rounding` + `BubbleRelativeDuration`; a constant 12 months per year gets
/// both wrong.
#[test]
fn round_and_total_use_the_real_month_count_of_a_hebrew_leap_year() {
    check(
        r#"
        const relativeTo = Temporal.PlainDate.from({ year: 5784, monthCode: "M01", day: 1, calendar: "hebrew" });
        const roundOptions = { largestUnit: "years", smallestUnit: "months", relativeTo };
        assertDuration(new Temporal.Duration(0, 12).round(roundOptions), 0, 12, 0, 0, 0, 0, 0, 0, 0, 0,
          "12 months stays inside a 13-month year");
        assertDuration(new Temporal.Duration(0, 13).round(roundOptions), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
          "13 months is a whole year");
        assertDuration(new Temporal.Duration(0, 14).round(roundOptions), 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
          "14 months is a year and a month");
        // The next year (5785) is not a leap year: 12 months, then 1 Tishrei 5786 is 13 + 12 months out.
        assertDuration(new Temporal.Duration(0, 25).round(roundOptions), 2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
          "25 months (13 + 12) is two years");
        assertSame(new Temporal.Duration(0, 13).total({ unit: "years", relativeTo }), 1, "13 months total years");
        assertSame(new Temporal.Duration(0, 12).total({ unit: "years", relativeTo }) < 1, true, "12 months is under a year");
        assertSame(new Temporal.Duration(0, 25).total({ unit: "years", relativeTo }), 2, "25 months total years");

        // Backwards from 1 Tishrei 5785, the year before (5784) is the 13-month one.
        const next = Temporal.PlainDate.from({ year: 5785, monthCode: "M01", day: 1, calendar: "hebrew" });
        const back = { largestUnit: "years", smallestUnit: "months", relativeTo: next };
        assertDuration(new Temporal.Duration(0, -13).round(back), -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, "-13 months is a whole year back");
        assertDuration(new Temporal.Duration(0, -12).round(back), 0, -12, 0, 0, 0, 0, 0, 0, 0, 0, "-12 months stays inside it");
        assertSame(new Temporal.Duration(0, -13).total({ unit: "years", relativeTo: next }), -1, "-13 months total years");
    "#,
    );
}

/// Chinese year 2023 has a leap month (`M02L`, between M02 and M03), so it has
/// 13 months. From 1 M01 2023 (2023-01-22), 12 months is 1 M12 (2024-01-11 --
/// still year 2023) and 13 months is the next New Year, 2024-02-10.
#[test]
fn round_and_total_use_the_real_month_count_of_a_chinese_leap_year() {
    check(
        r#"
        const relativeTo = Temporal.PlainDate.from({ year: 2023, monthCode: "M01", day: 1, calendar: "chinese" });
        assertSame(relativeTo.toString({ calendarName: "never" }), "2023-01-22", "the anchor is the 2023 New Year");
        const roundOptions = { largestUnit: "years", smallestUnit: "months", relativeTo };
        assertDuration(new Temporal.Duration(0, 12).round(roundOptions), 0, 12, 0, 0, 0, 0, 0, 0, 0, 0,
          "12 months stays inside a 13-month year");
        assertDuration(new Temporal.Duration(0, 13).round(roundOptions), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
          "13 months is a whole year");
        assertSame(new Temporal.Duration(0, 13).total({ unit: "years", relativeTo }), 1, "13 months total years");
        // `months` are counted through the leap month: M01 + 2 months is M02L, + 3 months is M03.
        assertSame(relativeTo.add({ months: 2 }).monthCode, "M02L", "M01 + 2 months is the leap month");
        assertSame(relativeTo.add({ months: 3 }).monthCode, "M03", "M01 + 3 months is M03");
    "#,
    );
}

/// The blank/limit behavior that the specification puts *before* the difference
/// (`relativeto-string-limits.js`, `relativeto-date-limits.js`) survives the
/// rewrite.
#[test]
fn a_plain_relative_to_outside_the_date_time_range_only_matters_for_a_non_blank_duration() {
    check(
        r#"
        const blank = new Temporal.Duration();
        const minutes = new Temporal.Duration(0, 0, 0, 0, 0, 5);
        for (const relativeTo of ["-271821-04-19", "-271821-04-19T01:00"]) {
          assertDuration(blank.round({ smallestUnit: "minutes", relativeTo }), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, "blank " + relativeTo);
          assertSame(blank.total({ unit: "minutes", relativeTo }), 0, "blank total " + relativeTo);
          assertRangeError(() => minutes.round({ smallestUnit: "minutes", relativeTo }), "round " + relativeTo);
          assertRangeError(() => minutes.total({ unit: "minutes", relativeTo }), "total " + relativeTo);
        }
        // The date-time the duration lands on must itself be within ISODateTimeWithinLimits,
        // which starts one nanosecond after midnight on -271821-04-19: a day back from the
        // first representable date lands exactly on that midnight.
        const first = "-271821-04-20";
        assertDuration(new Temporal.Duration(0, 0, 0, 0, -23).round({ smallestUnit: "hours", relativeTo: first }),
          0, 0, 0, 0, -23, 0, 0, 0, 0, 0, "23 hours back is representable");
        assertRangeError(() => new Temporal.Duration(0, 0, 0, 0, -24).round({ smallestUnit: "hours", relativeTo: first }),
          "24 hours back lands on the excluded midnight");
        assertRangeError(() => new Temporal.Duration(0, 0, 0, -1).total({ unit: "days", relativeTo: first }), "total, a day back");
        // A rounding window whose far end is not representable is a RangeError too.
        assertRangeError(() => new Temporal.Duration(0, 0, 0, 0, 1).round({ smallestUnit: "month", relativeTo: "+275760-09-12" }),
          "month window past the end");
        assertRangeError(() => new Temporal.Duration(0, 0, 0, 1).total({ unit: "month", relativeTo: "+275760-09-12" }),
          "month total window past the end");
        // A landing date past the last representable one is a RangeError, not a panic.
        assertRangeError(() => new Temporal.Duration(0, 0, 0, 1).round({ smallestUnit: "day", relativeTo: "+275760-09-13" }),
          "one day past the end");
        assertRangeError(() => new Temporal.Duration(0, 4294967295).round({ smallestUnit: "day", relativeTo: "2024-01-01" }),
          "huge month count");
    "#,
    );
}
