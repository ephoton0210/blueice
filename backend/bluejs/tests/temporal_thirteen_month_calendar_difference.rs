// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the `since`/`until` year/month difference of the
//! three **13-month** calendars — `coptic`, `ethiopic` and `ethioaa`, each
//! twelve 30-day months plus a 5- or 6-day intercalary `M13`
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`, Phase 26
//! Stage 3).
//!
//! `plain_date.rs`'s `calendar_difference_date_fixed_months` — Gecko's
//! `DifferenceNonISODate` for every non-ISO-aligned calendar *without* leap
//! months — hardcoded `MONTHS_PER_YEAR = 12`. That is exactly right for
//! `indian`, `persian` and the Hijri variants but wrong for the three
//! calendars whose year has 13 months: Gecko's own `CalendarMonthsPerYear`
//! returns 13 for `Coptic`/`Ethiopian`/`EthiopianAmeteAlem`. With 12 as the
//! modulus, the year-carry step mis-numbered every month at or after `M12`
//! that crossed a year boundary, and `largestUnit: "months"` folded years into
//! months at `years * 12` instead of `years * 13`.
//!
//! Every case below is taken, with the same expected values, from the real
//! Test262 fixtures
//! `intl402/Temporal/PlainDate/prototype/{until,since}/intercalary-month-{coptic,ethiopic,ethioaa}.js`
//! and `.../{until}/wrapping-at-end-of-month-{coptic,ethiopic,ethioaa}.js`,
//! reproduced through the public `Temporal.PlainDate`/`PlainDateTime`/
//! `PlainYearMonth` surface. The three calendars share the same structure and
//! differ only in the year numbers, so each table is run once per calendar.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// Every script here ends in `"ok"` on success, or a newline-joined list of
/// every mismatch, so a failing run shows all wrong cases at once.
fn assert_ok(source: &str) {
    match evaluate(source) {
        Value::String(text) if text == "ok" => {}
        Value::String(text) => panic!("mismatches:{}\n{source}", text.to_utf8().unwrap()),
        other => panic!("expected \"ok\" or a mismatch list, got {other:?}\n{source}"),
    }
}

/// The shared JS prelude: a `check(actual, [years, months, weeks, days],
/// label)` that records every mismatch instead of stopping at the first, then
/// `finish()` returns `"ok"` or the whole list, so one failing run reports
/// every wrong case at once.
const PRELUDE: &str = r#"
const failures = [];
function check(actual, expected, label) {
  const got = [actual.years, actual.months, actual.weeks, actual.days];
  if (got.join() !== expected.join()) {
    failures.push(label + ": expected " + expected.join() + " got " + got.join());
  }
}
function finish() {
  return failures.length ? "\n" + failures.join("\n") : "ok";
}
"#;

/// `until/intercalary-month-<calendar>.js`'s table, keyed by the four
/// year numbers each calendar's fixture uses (`common`, `leap`, `common2`
/// are three consecutive years; the leap year is the middle one).
fn intercalary_source(calendar: &str, common: i32) -> String {
    let leap = common + 1;
    let common2 = common + 2;
    format!(
        r#"
(function() {{
{PRELUDE}
const calendar = "{calendar}";
const options = {{ overflow: "reject" }};
const d = (year, monthCode, day) => Temporal.PlainDate.from({{ year, monthCode, day, calendar }}, options);
const commonM12 = d({common}, "M12", 1);
const commonLast = d({common}, "M13", 5);
const leapFirst = d({leap}, "M01", 1);
const leapM12 = d({leap}, "M12", 1);
const leapPenultimate = d({leap}, "M13", 5);
const leapLast = d({leap}, "M13", 6);
const common2First = d({common2}, "M01", 1);
const common2Last = d({common2}, "M13", 5);

const tests = [
  [commonLast, leapLast, "last day of common year to last day of leap year",
    ["years", 1, 0, 0, 1], ["months", 0, 13, 0, 1], ["weeks", 0, 0, 52, 2], ["days", 0, 0, 0, 366]],
  [commonLast, leapPenultimate, "last day of common year to penultimate day of leap year",
    ["years", 1, 0, 0, 0], ["months", 0, 13, 0, 0], ["weeks", 0, 0, 52, 1], ["days", 0, 0, 0, 365]],
  [leapLast, common2Last, "last day of leap year to last day of common year",
    ["years", 0, 12, 0, 29], ["months", 0, 12, 0, 29], ["weeks", 0, 0, 52, 1], ["days", 0, 0, 0, 365]],
  [commonM12, leapFirst, "2mo passing through intercalary month in common year",
    ["years", 0, 2, 0, 0], ["months", 0, 2, 0, 0], ["weeks", 0, 0, 5, 0], ["days", 0, 0, 0, 35]],
  [leapM12, common2First, "2mo passing through intercalary month in leap year",
    ["years", 0, 2, 0, 0], ["months", 0, 2, 0, 0], ["weeks", 0, 0, 5, 1], ["days", 0, 0, 0, 36]],
  [common2Last, leapLast, "backwards last day of common year to last day of leap year",
    ["years", 0, -12, 0, -5], ["months", 0, -12, 0, -5], ["weeks", 0, 0, -52, -1], ["days", 0, 0, 0, -365]],
  [common2Last, leapPenultimate, "backwards last day of common year to penultimate day of leap year",
    ["years", -1, 0, 0, 0], ["months", 0, -13, 0, 0], ["weeks", 0, 0, -52, -2], ["days", 0, 0, 0, -366]],
  [leapLast, commonLast, "backwards last day of leap year to last day of common year",
    ["years", -1, 0, 0, 0], ["months", 0, -13, 0, 0], ["weeks", 0, 0, -52, -2], ["days", 0, 0, 0, -366]],
  [leapFirst, commonM12, "backwards 2mo passing through intercalary month in common year",
    ["years", 0, -2, 0, 0], ["months", 0, -2, 0, 0], ["weeks", 0, 0, -5, 0], ["days", 0, 0, 0, -35]],
  [common2First, leapM12, "backwards 2mo passing through intercalary month in leap year",
    ["years", 0, -2, 0, 0], ["months", 0, -2, 0, 0], ["weeks", 0, 0, -5, -1], ["days", 0, 0, 0, -36]],
];

for (const [one, two, descr, ...units] of tests) {{
  for (const [largestUnit, years, months, weeks, days] of units) {{
    // `until` is receiver -> argument; `since` is exactly its negation
    // (`DifferenceTemporalPlainDate` negates the finished duration).
    check(one.until(two, {{ largestUnit }}), [years, months, weeks, days], "until " + descr + " (" + largestUnit + ")");
    check(one.since(two, {{ largestUnit }}), [-years || 0, -months || 0, -weeks || 0, -days || 0], "since " + descr + " (" + largestUnit + ")");
  }}
}}
return finish();
}})()
"#
    )
}

#[test]
fn coptic_intercalary_month_differences() {
    assert_ok(&intercalary_source("coptic", 1738));
}

#[test]
fn ethiopic_intercalary_month_differences() {
    assert_ok(&intercalary_source("ethiopic", 2014));
}

#[test]
fn ethioaa_intercalary_month_differences() {
    assert_ok(&intercalary_source("ethioaa", 7514));
}

/// The other three types' differences (`PlainDateTime`, `PlainYearMonth`,
/// `ZonedDateTime`) go through the same `calendar_difference_date`, so they
/// must agree with `PlainDate` on the 13-month year: one whole year is 13
/// months, and `largestUnit: "months"` must not fold at 12.
#[test]
fn every_difference_type_uses_thirteen_months_per_year() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  for (const [calendar, year] of [["coptic", 1738], ["ethiopic", 2014], ["ethioaa", 7514]]) {
    const dt = (y, m, d) => Temporal.PlainDateTime.from({ year: y, monthCode: m, day: d, calendar });
    const a = dt(year, "M01", 15);
    const b = dt(year + 1, "M01", 15);
    const months = a.until(b, { largestUnit: "months" });
    if (months.months !== 13 || months.years !== 0) failures.push(calendar + " datetime months: " + months.toString());
    const years = a.until(b, { largestUnit: "years" });
    if (years.years !== 1 || years.months !== 0) failures.push(calendar + " datetime years: " + years.toString());

    const ym = (y, m) => Temporal.PlainYearMonth.from({ year: y, monthCode: m, calendar });
    const ymMonths = ym(year, "M01").until(ym(year + 2, "M01"), { largestUnit: "months" });
    if (ymMonths.months !== 26) failures.push(calendar + " yearmonth months: " + ymMonths.toString());
    const ymYears = ym(year, "M12").until(ym(year + 1, "M13"), { largestUnit: "years" });
    if (ymYears.years !== 1 || ymYears.months !== 1) failures.push(calendar + " yearmonth years: " + ymYears.toString());

    const zdt = (y, m, d) =>
      Temporal.ZonedDateTime.from({ year: y, monthCode: m, day: d, hour: 9, timeZone: "UTC", calendar });
    const zMonths = zdt(year, "M01", 15).until(zdt(year + 1, "M01", 15), { largestUnit: "months" });
    if (zMonths.months !== 13 || zMonths.years !== 0) failures.push(calendar + " zoned months: " + zMonths.toString());
    // Month-and-time remainder: 1 year, 12 months (M01 -> M13), then the hours.
    const zYears = zdt(year, "M01", 3).until(zdt(year + 1, "M13", 3).add({ hours: 5 }), { largestUnit: "years" });
    if (zYears.years !== 1 || zYears.months !== 12 || zYears.hours !== 5) {
      failures.push(calendar + " zoned years: " + zYears.toString());
    }
  }
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// `until/wrapping-at-end-of-month-<calendar>.js` (identical year numbers
/// for all three calendars): day-of-month wrapping at the end of the 5/6-day
/// intercalary month, in both `largestUnit`s, plus multi-year spans that pass
/// through `M13`.
fn wrapping_source(calendar: &str) -> String {
    format!(
        r#"
(function() {{
{PRELUDE}
const calendar = "{calendar}";
const d = (year, monthCode, day) => Temporal.PlainDate.from({{ year, monthCode, day, calendar }});
const cases = [
  // Difference between end of longer month to end of following shorter month.
  [d(1970, "M12", 5), d(1970, "M13", 5), ["years", "months"], [0, 1, 0, 0], "Mesori 5th to M13 5th is one month"],
  [d(1970, "M12", 28), d(1970, "M13", 5), ["years", "months"], [0, 0, 0, 7], "Mesori 28th to M13 5th is 7 days"],
  [d(1970, "M12", 29), d(1970, "M13", 5), ["years", "months"], [0, 0, 0, 6], "Mesori 29th to M13 5th is 6 days"],
  [d(1970, "M12", 30), d(1970, "M13", 5), ["years", "months"], [0, 0, 0, 5], "Mesori 30th to M13 5th is 5 days"],
  // Leap-year Mesori to leap-year M13.
  [d(1971, "M12", 6), d(1971, "M13", 6), ["years", "months"], [0, 1, 0, 0], "Mesori 6th to M13 6th is one month"],
  [d(1971, "M12", 29), d(1971, "M13", 6), ["years", "months"], [0, 0, 0, 7], "leap Mesori 29th to M13 6th is 7 days"],
  [d(1971, "M12", 30), d(1971, "M13", 6), ["years", "months"], [0, 0, 0, 6], "leap Mesori 30th to M13 6th is 6 days"],
  // Longer month to a not-immediately-following shorter month.
  [d(1970, "M10", 5), d(1970, "M13", 5), ["years", "months"], [0, 3, 0, 0], "Paoni 5th to M13 5th is 3 months"],
  [d(1970, "M10", 6), d(1970, "M13", 5), ["years", "months"], [0, 2, 0, 29], "Paoni 6th to M13 5th is 2 months 29 days"],
  // Longer month in one year to a shorter month in a later year.
  [d(1970, "M12", 5), d(1973, "M13", 5), ["months"], [0, 40, 0, 0], "Mesori 5th 1970 to M13 5th 1973 is 40 months"],
  [d(1970, "M12", 5), d(1973, "M13", 5), ["years"], [3, 1, 0, 0], "Mesori 5th 1970 to M13 5th 1973 is 3 years 1 month"],
  [d(1970, "M12", 6), d(1973, "M13", 5), ["months"], [0, 39, 0, 29], "Mesori 6th 1970 to M13 5th 1973 is 39 months 29 days"],
  [d(1970, "M12", 7), d(1973, "M13", 5), ["years"], [3, 0, 0, 28], "Mesori 7th 1970 to M13 5th 1973 is 3 years 28 days"],
  // Passing through a month the same length or shorter than either endpoint.
  [d(1970, "M01", 29), d(1970, "M03", 28), ["months"], [0, 1, 0, 29], "Thout 29th to Hathor 28th is 1 month 29 days"],
  [d(1970, "M01", 30), d(1971, "M05", 29), ["years"], [1, 3, 0, 29], "Thout 30th 1970 to Tobi 29th 1971 is 1 year 3 months 29 days"],
];
for (const [one, two, units, expected, label] of cases) {{
  for (const largestUnit of units) {{
    check(one.until(two, {{ largestUnit }}), expected, "until " + label + " (" + largestUnit + ")");
    check(one.since(two, {{ largestUnit }}), expected.map((v) => -v || 0), "since " + label + " (" + largestUnit + ")");
  }}
}}
return finish();
}})()
"#
    )
}

#[test]
fn coptic_wrapping_at_end_of_month() {
    assert_ok(&wrapping_source("coptic"));
}

#[test]
fn ethiopic_wrapping_at_end_of_month() {
    assert_ok(&wrapping_source("ethiopic"));
}

#[test]
fn ethioaa_wrapping_at_end_of_month() {
    assert_ok(&wrapping_source("ethioaa"));
}

/// `round_calendar_duration`'s `smallestUnit: "months"` branch used to flatten
/// `years` into months at `years * 12` and re-split the rounded total at
/// `% 12`. In a 13-month calendar a difference such as `1y 12mo 2d` (M01 of
/// one year to the 3rd day of the intercalary `M13` a year later) is already a
/// balanced duration, and rounding its months *up* must carry into a whole
/// extra year (`2y 0mo`), not into a 13th month of a 12-month year.
#[test]
fn month_rounding_carries_at_thirteen_months_per_year() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  for (const [calendar, year] of [["coptic", 1738], ["ethiopic", 2014], ["ethioaa", 7514]]) {
    const start = Temporal.PlainDate.from({ year, monthCode: "M01", day: 1, calendar });
    const end = Temporal.PlainDate.from({ year: year + 1, monthCode: "M13", day: 3, calendar });
    const got = (roundingMode, largestUnit) => {
      const d = start.until(end, { largestUnit, smallestUnit: "months", roundingMode });
      return [d.years, d.months, d.weeks, d.days].join();
    };
    const cases = [
      // 12 whole months plus 2 of the 5 days of the year's intercalary month.
      ["trunc", "years", "1,12,0,0"],
      ["halfExpand", "years", "1,12,0,0"],
      ["ceil", "years", "2,0,0,0"],
      ["expand", "years", "2,0,0,0"],
      ["trunc", "months", "0,25,0,0"],
      ["ceil", "months", "0,26,0,0"],
    ];
    for (const [roundingMode, largestUnit, expected] of cases) {
      const actual = got(roundingMode, largestUnit);
      if (actual !== expected) {
        failures.push(calendar + " " + roundingMode + "/" + largestUnit + ": expected " + expected + " got " + actual);
      }
    }
    // The same span backwards: `ceil`/`floor` swap roles (toward +/- infinity),
    // and every field is negative.
    const backwards = (roundingMode) => {
      const d = end.until(start, { largestUnit: "years", smallestUnit: "months", roundingMode });
      return [d.years, d.months, d.weeks, d.days].join();
    };
    for (const [roundingMode, expected] of [["ceil", "-1,-12,0,0"], ["floor", "-2,0,0,0"], ["trunc", "-1,-12,0,0"], ["expand", "-2,0,0,0"]]) {
      const actual = backwards(roundingMode);
      if (actual !== expected) {
        failures.push(calendar + " backwards " + roundingMode + ": expected " + expected + " got " + actual);
      }
    }
  }
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// `Temporal.Duration.prototype.round`/`total` with a `PlainDate` `relativeTo`
/// in a 13-month calendar. `round` with `smallestUnit: "months"` and
/// `largestUnit: "years"` splits its rounded month total back into `years`
/// and `months` (`duration_operations.rs`), which must use the calendar's own
/// months-per-year, not a hardcoded 12.
#[test]
fn duration_round_and_total_with_a_thirteen_month_relative_to() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  for (const [calendar, year] of [["coptic", 1738], ["ethiopic", 2014], ["ethioaa", 7514]]) {
    const relativeTo = Temporal.PlainDate.from({ year, monthCode: "M01", day: 1, calendar });
    const check = (label, actual, expected) => {
      if (actual !== expected) failures.push(calendar + " " + label + ": expected " + expected + " got " + actual);
    };
    const fields = (d) => [d.years, d.months, d.weeks, d.days].join();

    // 1y 12mo from M01 lands on M13 of the following year: already balanced.
    const balanced = new Temporal.Duration(1, 12).round({ smallestUnit: "months", largestUnit: "years", relativeTo });
    check("1y12mo balanced", fields(balanced), "1,12,0,0");
    // 25 months is 1 year 12 months in a 13-month calendar, not 2y 1mo.
    const flat = new Temporal.Duration(0, 25).round({ smallestUnit: "months", largestUnit: "years", relativeTo });
    check("25mo to years", fields(flat), "1,12,0,0");
    const carry = new Temporal.Duration(0, 26).round({ smallestUnit: "months", largestUnit: "years", relativeTo });
    check("26mo to years", fields(carry), "2,0,0,0");
    check("total months", String(new Temporal.Duration(1, 12).total({ unit: "months", relativeTo })), "25");
    check("total years", String(new Temporal.Duration(0, 26).total({ unit: "years", relativeTo })), "2");
    // Negative durations split with the same 13, and keep a common sign.
    const negative = new Temporal.Duration(0, -25).round({ smallestUnit: "months", largestUnit: "years", relativeTo });
    check("-25mo to years", fields(negative), "-1,-12,0,0");
    // 13 months is exactly one year in these calendars (not 1y + 1mo as with 12).
    check("compare 13mo vs 1y", String(Temporal.Duration.compare(new Temporal.Duration(0, 13), new Temporal.Duration(1), { relativeTo })), "0");
    check("compare 14mo vs 1y", String(Temporal.Duration.compare(new Temporal.Duration(0, 14), new Temporal.Duration(1), { relativeTo })), "1");
  }
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// `add`/`subtract/leap-month-{chinese,dangi,hebrew}-numerical-months.js`:
/// starting from a numerical (ordinal) month that is the year's leap month,
/// adding or subtracting a whole year — with or without an extra month — lands
/// on a year that has no such leap month, which `overflow: "reject"` must
/// refuse. `add_year_month_duration_leap_month` resolved that landing month in
/// constrain mode whenever a `months` component was present, so `P1Y1M` sailed
/// through while plain `P1Y` correctly threw. Gecko's `AddYearMonthDuration`
/// applies the caller's overflow to that step whether or not `months` is
/// zero.
#[test]
fn leap_month_anchor_rejects_a_year_shift_onto_a_year_without_that_leap_month() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  const makers = {
    PlainDate: (calendar, year, month) => Temporal.PlainDate.from({ calendar, year, month, day: 1 }),
    PlainDateTime: (calendar, year, month) => Temporal.PlainDateTime.from({ calendar, year, month, day: 1 }),
    PlainYearMonth: (calendar, year, month) => Temporal.PlainYearMonth.from({ calendar, year, month }),
    ZonedDateTime: (calendar, year, month) =>
      Temporal.ZonedDateTime.from({ calendar, year, month, day: 1, hour: 12, timeZone: "UTC" }),
  };
  // [calendar, year, ordinal month]: month is that year's leap month.
  const anchors = [["chinese", 2012, 5], ["dangi", 2012, 4], ["hebrew", 5000, 6], ["hebrew", 5003, 6]];
  // Every anchor's neighbouring years (+-1) are common years without the leap
  // month; two years out is not guaranteed (Hebrew 5003 + 2 = 5005 is leap).
  const amounts = ["P1Y1M", new Temporal.Duration(1), "P1Y", "P1Y6M"];
  for (const [type, make] of Object.entries(makers)) {
    for (const [calendar, year, month] of anchors) {
      const instance = make(calendar, year, month);
      for (const method of ["add", "subtract"]) {
        for (const amount of amounts) {
          let threw = "no error";
          try { instance[method](amount, { overflow: "reject" }); } catch (e) { threw = e.name; }
          if (threw !== "RangeError") {
            failures.push(type + " " + calendar + " " + year + "/" + month + " " + method + " " + String(amount) + ": " + threw);
          }
        }
      }
    }
  }
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// The counterpart: when the leap month *does* recur in the landing year,
/// `overflow: "reject"` must not throw, and the month identity carries through
/// (Hebrew 5000 and 5019 are both year 3 of their 19-year cycle, so both have
/// Adar I), while `overflow: "constrain"` still falls back to the calendar's
/// own month (`M05L` -> `M06` in a common Hebrew year).
#[test]
fn leap_month_anchor_reject_still_allows_a_recurring_leap_month() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  const check = (label, actual, expected) => {
    if (actual !== expected) failures.push(label + ": expected " + expected + " got " + actual);
  };
  const adarI = Temporal.PlainDate.from({ calendar: "hebrew", year: 5000, monthCode: "M05L", day: 1 });
  const sameLeap = adarI.add("P19Y", { overflow: "reject" });
  check("P19Y reject", sameLeap.year + " " + sameLeap.monthCode, "5019 M05L");
  const nextMonth = adarI.add("P19Y1M", { overflow: "reject" });
  check("P19Y1M reject", nextMonth.year + " " + nextMonth.monthCode, "5019 M06");
  const constrained = adarI.add("P1Y1M");
  check("P1Y1M constrain", constrained.year + " " + constrained.monthCode, "5001 M07");
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}

/// `until/wrapping-at-end-of-month-hebrew.js`: Hebrew is not one of the three
/// fixed 13-month calendars (its 13th month exists only in leap years), but the
/// same fixture family pins the same end-of-month wrapping, including a start
/// on a leap month (`M05L`, Adar I) whose target year has no such month, and
/// Cheshvan/Kislev's year-to-year 29-or-30-day variation.
#[test]
fn hebrew_wrapping_at_end_of_month() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const calendar = "hebrew";
const options = {{ overflow: "reject" }};
const d = (year, monthCode, day) => Temporal.PlainDate.from({{ year, monthCode, day, calendar }}, options);
const cases = [
  [d(5783, "M07", 29), d(5783, "M08", 29), ["years", "months"], [0, 1, 0, 0], "Nisan 29th to Iyar 29th is one month"],
  [d(5783, "M07", 30), d(5783, "M08", 29), ["years", "months"], [0, 0, 0, 29], "Nisan 30th to Iyar 29th is 29 days"],
  [d(5783, "M09", 29), d(5783, "M12", 29), ["years", "months"], [0, 3, 0, 0], "Sivan 29th to Elul 29th is 3 months"],
  [d(5783, "M09", 30), d(5783, "M12", 29), ["years", "months"], [0, 2, 0, 29], "Sivan 30th to Elul 29th is 2 months 29 days"],
  [d(5783, "M11", 29), d(5786, "M04", 29), ["months"], [0, 30, 0, 0], "Av 29th 5783 to Tevet 29th 5786 is 30 months"],
  [d(5783, "M11", 29), d(5786, "M04", 29), ["years"], [2, 5, 0, 0], "Av 29th 5783 to Tevet 29th 5786 is 2 years 5 months"],
  [d(5783, "M11", 30), d(5786, "M04", 29), ["months"], [0, 29, 0, 29], "Av 30th 5783 to Tevet 29th 5786 is 29 months 29 days"],
  [d(5783, "M11", 30), d(5786, "M04", 29), ["years"], [2, 4, 0, 29], "Av 30th 5783 to Tevet 29th 5786 is 2 years 4 months 29 days"],
  [d(5783, "M02", 29), d(5784, "M02", 29), ["months"], [0, 12, 0, 0], "29th Kislev 5783 to 5784 is 12 months"],
  [d(5783, "M02", 29), d(5784, "M02", 29), ["years"], [1, 0, 0, 0], "29th Kislev 5783 to 5784 is 1 year"],
  [d(5783, "M02", 30), d(5784, "M02", 29), ["months", "years"], [0, 11, 0, 29], "30th Kislev 5783 to 29th Kislev 5784 is 11 months 29 days"],
  [d(5783, "M03", 29), d(5784, "M03", 29), ["months"], [0, 12, 0, 0], "29th Cheshvan 5783 to 5784 is 12 months"],
  [d(5783, "M03", 29), d(5784, "M03", 29), ["years"], [1, 0, 0, 0], "29th Cheshvan 5783 to 5784 is 1 year"],
  [d(5783, "M03", 30), d(5784, "M03", 29), ["months", "years"], [0, 11, 0, 29], "30th Cheshvan 5783 to 29th Cheshvan 5784 is 11 months 29 days"],
  [d(5784, "M05L", 29), d(5785, "M06", 29), ["months"], [0, 13, 0, 0], "29th Adar I 5784 to 29th Adar 5785 is 13 months"],
  [d(5784, "M05L", 29), d(5785, "M06", 29), ["years"], [1, 0, 0, 0], "29th Adar I 5784 to 29th Adar 5785 is 1 year"],
  [d(5784, "M05L", 30), d(5785, "M06", 29), ["months", "years"], [0, 12, 0, 29], "30th Adar I 5784 to 29th Adar 5785 is 12 months 29 days"],
];
for (const [one, two, units, expected, label] of cases) {{
  for (const largestUnit of units) {{
    check(one.until(two, {{ largestUnit }}), expected, "until " + label + " (" + largestUnit + ")");
    check(one.since(two, {{ largestUnit }}), expected.map((v) => -v || 0), "since " + label + " (" + largestUnit + ")");
  }}
}}
return finish();
}})()
"#
    ));
}
