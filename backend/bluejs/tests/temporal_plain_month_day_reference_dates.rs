// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainMonthDay`'s calendar-field
//! resolution in non-ISO calendars (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! A `PlainMonthDay` stores an ISO *reference date*, and Intl.Era-monthcode
//! defines it precisely: resolve the fields (using `year` if one was given, to
//! decide whether the day exists), then take the **latest ISO date at or before
//! 1972-12-31** that has the resulting `monthCode` and day (falling back to the
//! earliest one after it, and constraining the day/leap month away when no
//! ISO date near 1972 has it). Three things went wrong:
//!
//! - A `year` in the bag decided the *reference year* too: `{ year: 2021,
//!   monthCode: "M02", day: 29, calendar: "gregory" }` came back as
//!   2021-02-28 instead of 1972-02-28, a Hebrew `year: 5781` as 2020, and a
//!   string `2023-01-01[u-ca=hebrew]` kept 2023 (`reference-year-1972.js`,
//!   `reference-date-noniso-calendar.js`, the Chinese/Dangi
//!   `*-calendar-dates.js` and `*leap-month-with-year-from-options-bag*.js`).
//! - A non-ISO bag did not need a `year` for an ordinal `month` when a
//!   `monthCode` was also present, and `CalendarResolveFields`' rule that every
//!   missing-field `TypeError` comes before any range check was not followed
//!   (`calendarresolvefields-error-ordering-{chinese,hebrew,islamic}.js`).
//! - `year`, `era`/`eraYear`, `month` and `monthCode` given together were not
//!   cross-checked (`fields-overspecified.js`).

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

const PRELUDE: &str = r#"
const failures = [];
const MD = Temporal.PlainMonthDay;
// The ISO year of the stored reference date.
function refYear(monthDay) {
  return Number(monthDay.toString({ calendarName: "always" }).match(/^(-?\d+|[+-]\d{6})-/)[1]);
}
function monthDay(label, fn, monthCode, day, referenceYear) {
  try {
    const value = fn();
    const got = [value.monthCode, value.day, refYear(value)].join();
    const expected = [monthCode, day, referenceYear].join();
    if (got !== expected) failures.push(label + ": expected " + expected + " got " + got);
    return value;
  } catch (e) {
    failures.push(label + ": threw " + e.name + ": " + e.message);
  }
}
function expect(label, type, fn) {
  try { fn(); failures.push(label + ": did not throw " + type.name); }
  catch (e) { if (!(e instanceof type)) failures.push(label + ": expected " + type.name + " got " + e.name + ": " + e.message); }
}
function done() { return failures.length ? "\n" + failures.join("\n") : "ok"; }
"#;

/// `reference-year-1972.js`, all of it.
#[test]
fn reference_year_is_the_latest_iso_year_at_or_before_1972() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
// A supplied year only decides whether the day exists; the reference year is still 1972.
monthDay("gregory year+monthCode", () => MD.from({{ year: 2021, monthCode: "M02", day: 29, calendar: "gregory" }}), "M02", 28, 1972);
monthDay("gregory year+month", () => MD.from({{ year: 2021, month: 2, day: 29, calendar: "gregory" }}, {{ overflow: "constrain" }}), "M02", 28, 1972);
expect("gregory year+monthCode reject", RangeError,
  () => MD.from({{ year: 2021, monthCode: "M02", day: 29, calendar: "gregory" }}, {{ overflow: "reject" }}));
expect("gregory year+month reject", RangeError,
  () => MD.from({{ year: 2021, month: 2, day: 29, calendar: "gregory" }}, {{ overflow: "reject" }}));

monthDay("hebrew M01-01", () => MD.from({{ monthCode: "M01", day: 1, calendar: "hebrew" }}), "M01", 1, 1972);
// Adar I does not occur in 1972: the latest year before it that has it.
monthDay("hebrew M05L-01", () => MD.from({{ monthCode: "M05L", day: 1, calendar: "hebrew" }}), "M05L", 1, 1970);
monthDay("hebrew 5781 year+monthCode", () => MD.from({{ year: 5781, monthCode: "M02", day: 30, calendar: "hebrew" }}), "M02", 29, 1972);
monthDay("hebrew 5781 year+month", () => MD.from({{ year: 5781, month: 2, day: 30, calendar: "hebrew" }}, {{ overflow: "constrain" }}), "M02", 29, 1972);
// Cheshvan 30 does not occur in 1972 but does in 1971.
monthDay("hebrew M02-30", () => MD.from({{ monthCode: "M02", day: 30, calendar: "hebrew" }}), "M02", 30, 1971);
expect("hebrew 5781 year+monthCode reject", RangeError,
  () => MD.from({{ year: 5781, monthCode: "M02", day: 30, calendar: "hebrew" }}, {{ overflow: "reject" }}));
expect("hebrew 5781 year+month reject", RangeError,
  () => MD.from({{ year: 5781, month: 2, day: 30, calendar: "hebrew" }}, {{ overflow: "reject" }}));
// Two Hebrew M04-26 dates fall in ISO 1972: the later one wins.
const later = monthDay("hebrew M04-26", () => MD.from({{ monthCode: "M04", day: 26, calendar: "hebrew" }}), "M04", 26, 1972);
if (later !== undefined && later.toString() !== "1972-12-31[u-ca=hebrew]") failures.push("hebrew M04-26 date: " + later.toString());
return done();
}})()
"#
    ));
}

/// `reference-date-noniso-calendar.js`: a string's own ISO year is discarded
/// too; the same reference date comes out of `PlainDate.prototype.toPlainMonthDay`.
#[test]
fn string_and_to_plain_month_day_derive_the_same_reference_year() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
// 2023-01-01 is 8 Tevet in the Hebrew calendar; 8 Tevet occurs in ISO 1972.
const fromString = monthDay("string", () => MD.from("2023-01-01[u-ca=hebrew]"), "M04", 8, 1972);
const fromDate = monthDay("toPlainMonthDay",
  () => Temporal.PlainDate.from("2023-01-01[u-ca=hebrew]").toPlainMonthDay(), "M04", 8, 1972);
if (fromString !== undefined && fromDate !== undefined && !fromString.equals(fromDate)) failures.push("string vs toPlainMonthDay differ");
// A leap-month date in a Chinese year: the reference year comes from the month/day alone.
const chinese = Temporal.PlainDate.from({{ year: 2001, monthCode: "M04L", day: 15, calendar: "chinese" }});
monthDay("chinese toPlainMonthDay", () => chinese.toPlainMonthDay(), "M04L", 15, 1963);
monthDay("chinese string", () => MD.from(chinese.toString()), "M04L", 15, 1963);
return done();
}})()
"#
    ));
}

/// `chinese-calendar-dates.js`, `dangi-calendar-dates.js` and the Chinese/Dangi
/// `PlainMonthDay.prototype.monthCode` fixtures: leap-month reference years.
#[test]
fn chinese_and_dangi_leap_months_get_their_own_reference_years() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
// [calendar, [year, ordinal month, monthCode, day, referenceYear]...]
const CASES = {{
  chinese: [
    [2001, 5, "M04L", 15, 1963], [2000, 6, "M06", 29, 1972],
    [1971, 6, "M05L", 1, 1971], [1974, 5, "M04L", 1, 1963], [1976, 9, "M08L", 1, 1957],
    [1979, 7, "M06L", 1, 1960], [1987, 7, "M06L", 1, 1960], [1993, 4, "M03L", 1, 1966],
    [2004, 3, "M02L", 1, 1947], [2006, 8, "M07L", 1, 1968], [2012, 5, "M04L", 1, 1963],
    [2017, 7, "M06L", 1, 1960], [2023, 3, "M02L", 1, 1947], [2044, 8, "M07L", 1, 1968],
  ],
  dangi: [
    [2001, 5, "M04L", 15, 1963], [2000, 6, "M06", 29, 1972],
    [1971, 6, "M05L", 1, 1971], [1974, 5, "M04L", 1, 1963], [1976, 9, "M08L", 1, 1957],
    [1979, 7, "M06L", 1, 1960], [1993, 4, "M03L", 1, 1966], [2004, 3, "M02L", 1, 1947],
    [2006, 8, "M07L", 1, 1968], [2012, 4, "M03L", 1, 1966], [2017, 6, "M05L", 1, 1971],
    [2023, 3, "M02L", 1, 1947], [2044, 8, "M07L", 1, 1968],
  ],
}};
for (const [calendar, rows] of Object.entries(CASES)) {{
  for (const [year, month, monthCode, day, referenceYear] of rows) {{
    const label = calendar + " " + year + " " + monthCode + "-" + day;
    const byYear = monthDay(label + " year+month", () => MD.from({{ year, month, day, calendar }}), monthCode, day, referenceYear);
    const byCode = monthDay(label + " monthCode", () => MD.from({{ monthCode, day, calendar }}), monthCode, day, referenceYear);
    if (byYear !== undefined && byCode !== undefined && !byYear.equals(byCode)) failures.push(label + ": equals is false");
    // `month: 15` is out of range in every year: constrained to the last month, else refused.
    expect(label + " month 15 reject", RangeError, () => MD.from({{ year, month: 15, day: 1, calendar }}, {{ overflow: "reject" }}));
    const constrained = MD.from({{ year, month: 15, day: 1, calendar }});
    if (constrained.monthCode !== "M12") failures.push(label + ": month 15 constrained to " + constrained.monthCode);
  }}
  expect(calendar + " M15 reject", RangeError, () => MD.from({{ monthCode: "M15", day: 1, calendar }}, {{ overflow: "reject" }}));
  expect(calendar + " M15 constrain", RangeError, () => MD.from({{ monthCode: "M15", day: 1, calendar }}));
}}
return done();
}})()
"#
    ));
}

/// `chinese-dangi-leap-month-with-year-from-options-bag.js`: a leap month/day
/// that does not occur near 1972 falls back to the non-leap month, with the
/// reference year of *that* month-day.
#[test]
fn a_leap_month_day_that_never_occurs_constrains_to_the_common_month() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const candidates = [
  // ICU4X years
  [1651, "M01L", 29, 1972], [1461, "M01L", 30, 1970], [1765, "M02L", 30, 1972], [1718, "M08L", 30, 1971],
  [-5738, "M09L", 30, 1972], [-4098, "M10L", 30, 1972], [-2173, "M11L", 30, 1970], [1403, "M12L", 29, 1972],
  [-180, "M12L", 30, 1972],
  // ICU4C years
  [1898, "M01L", 29, 1972], [1898, "M01L", 30, 1970], [1830, "M02L", 30, 1972], [1843, "M09L", 30, 1972],
  [1737, "M10L", 30, 1972], [1889, "M11L", 30, 1970], [1879, "M12L", 29, 1972], [1784, "M12L", 30, 1972],
];
for (const calendar of ["chinese", "dangi"]) {{
  for (const [year, monthCode, day, referenceYear] of candidates) {{
    const date = Temporal.PlainDate.from({{ calendar, year, monthCode, day }});
    // Only a year that really has this leap month/day is a valid probe.
    if (date.monthCode !== monthCode || date.day !== day) continue;
    const label = calendar + " " + year + " " + monthCode + "-" + day;
    try {{
      const pmd = MD.from({{ calendar, year, monthCode, day }});
      if (refYear(pmd) !== referenceYear) failures.push(label + ": reference year " + refYear(pmd) + ", expected " + referenceYear);
      const fromDate = date.toPlainMonthDay();
      if (refYear(fromDate) !== referenceYear) failures.push(label + " toPlainMonthDay: reference year " + refYear(fromDate));
    }} catch (e) {{
      failures.push(label + ": threw " + e.name + ": " + e.message);
    }}
  }}
}}
return done();
}})()
"#
    ));
}

/// `chinese-dangi-leap-month-with-year-from-options-bag-overflow-reject.js`:
/// the same month-days are refused under `overflow: "reject"`.
#[test]
fn a_leap_month_day_that_never_occurs_is_refused_under_reject() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const nonexistent = [
  [1234, "M01L", 29], [1234, "M01L", 30], [1234, "M02L", 30], [1234, "M08L", 30], [1234, "M09L", 30],
  [1234, "M10L", 30], [1234, "M11L", 30], [1234, "M12L", 29], [1234, "M12L", 30],
  [1651, "M01L", 29], [1461, "M01L", 30], [1765, "M02L", 30], [1718, "M08L", 30], [-5738, "M09L", 30],
  [-4098, "M10L", 30], [-2173, "M11L", 30], [1403, "M12L", 29], [-180, "M12L", 30],
  [1898, "M01L", 29], [1898, "M01L", 30], [1830, "M02L", 30], [1843, "M09L", 30], [1737, "M10L", 30],
  [1890, "M11L", 30], [1879, "M12L", 29], [1784, "M12L", 30],
];
for (const calendar of ["chinese", "dangi"]) {{
  for (const [year, monthCode, day] of nonexistent) {{
    expect(calendar + " " + year + " " + monthCode + "-" + day, RangeError,
           () => MD.from({{ calendar, year, monthCode, day }}, {{ overflow: "reject" }}));
  }}
}}
return done();
}})()
"#
    ));
}

/// `calendarresolvefields-error-ordering-{chinese,hebrew,islamic}.js`: every
/// missing-field `TypeError` (a `year` for an ordinal `month`, a `month` or
/// `monthCode`, a `day`) beats every `RangeError` (a `month`/`monthCode`
/// conflict, a day out of range).
#[test]
fn missing_fields_are_type_errors_before_any_range_error() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
// [calendar, sample year, monthCode/month pair that agree in that year's leap layout, monthCode of a valid month]
const SETUPS = [
  ["chinese", 2020, "M01"],
  ["hebrew", 5784, "M01"],
  ["islamic-civil", 1445, "M01"],
];
for (const [calendar, year, validCode] of SETUPS) {{
  const label = (text) => calendar + ": " + text;
  // A `month` needs a `year`, even next to a conflicting `monthCode`.
  expect(label("missing year"), TypeError, () => MD.from({{ calendar, monthCode: "M04", month: 5, day: 1 }}));
  // Neither `month` nor `monthCode`, next to an out-of-range day.
  expect(label("missing month"), TypeError, () => MD.from({{ calendar, year, day: 32 }}, {{ overflow: "reject" }}));
  // No `day`, next to a monthCode/month conflict.
  expect(label("missing day"), TypeError, () => MD.from({{ calendar, year, monthCode: "M04", month: 5 }}));
  // An `undefined` year is missing.
  expect(label("undefined year"), TypeError,
         () => MD.from({{ calendar, year: undefined, monthCode: "M04", month: 5, day: 1 }}));
  // With every required field present, the conflict is a RangeError.
  expect(label("month/monthCode conflict"), RangeError,
         () => MD.from({{ calendar, year, monthCode: "M04", month: 5, day: 1 }}));
  expect(label("day out of range"), RangeError,
         () => MD.from({{ calendar, year, monthCode: validCode, day: 32 }}, {{ overflow: "reject" }}));
}}
// No RangeError when `month` is the ordinal of the `monthCode` *in the given year*,
// even though the reference date's own ordinal differs: the PlainDate made from
// the result's string has the plain ordinal.
for (const [calendar, year, monthCode, month, plainOrdinal] of [["chinese", 2004, "M04", 5, 4], ["hebrew", 5784, "M06", 7, 6]]) {{
  try {{
    const pmd = MD.from({{ calendar, year, monthCode, month, day: 1 }});
    const pd = Temporal.PlainDate.from(pmd.toString());
    if (pmd.monthCode !== monthCode) failures.push(calendar + ": monthCode " + pmd.monthCode);
    if (pd.monthCode !== monthCode) failures.push(calendar + ": PlainDate monthCode " + pd.monthCode);
    if (pd.month !== plainOrdinal) failures.push(calendar + ": PlainDate month " + pd.month + ", expected " + plainOrdinal);
  }} catch (e) {{
    failures.push(calendar + " " + year + " " + monthCode + "/" + month + ": " + e.name + ": " + e.message);
  }}
}}
return done();
}})()
"#
    ));
}

/// `fields-overspecified.js`: fields that disagree with each other.
#[test]
fn overspecified_fields_must_agree() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const gregory = {{ calendar: "gregory", day: 1 }};
// eraYear and year must be consistent, whichever month spelling is used.
expect("era/eraYear vs year with monthCode", RangeError,
       () => MD.from(Object.assign({{ era: "ce", eraYear: 2024, year: 2023, monthCode: "M01" }}, gregory)));
expect("era/eraYear vs year with month", RangeError,
       () => MD.from(Object.assign({{ era: "ce", eraYear: 2024, year: 2023, month: 1 }}, gregory)));
// monthCode and month must be consistent.
expect("monthCode vs month", RangeError,
       () => MD.from(Object.assign({{ year: 2024, monthCode: "M01", month: 2 }}, gregory)));
// Consistent overspecification is fine.
monthDay("consistent", () => MD.from(Object.assign({{ era: "ce", eraYear: 2024, year: 2024, monthCode: "M01", month: 1 }}, gregory)), "M01", 1, 1972);
// One of era/eraYear is a TypeError; an unknown era a RangeError; a calendar without eras ignores both.
expect("era only", TypeError, () => MD.from(Object.assign({{ era: "ce", year: 2024, monthCode: "M01" }}, gregory)));
expect("eraYear only", TypeError, () => MD.from(Object.assign({{ eraYear: 2024, year: 2024, monthCode: "M01" }}, gregory)));
expect("unknown era", RangeError, () => MD.from(Object.assign({{ era: "xyz", eraYear: 2024, monthCode: "M01" }}, gregory)));
monthDay("chinese ignores era", () => MD.from({{ era: "xyz", eraYear: 1, monthCode: "M01", day: 1, calendar: "chinese" }}), "M01", 1, 1972);
return done();
}})()
"#
    ));
}

/// `dont-calculate-month-info-for-out-of-range-year.js`: the supplied year is
/// discarded from the result, but a year (or era year) outside Temporal's range
/// is still a `RangeError` in every calendar.
#[test]
fn a_year_far_outside_the_range_bails_out_in_every_calendar() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const testData = [
  ["buddhist", "M02", 29, "be"], ["chinese", "M06L", 30], ["coptic", "M13", 6, "am"], ["dangi", "M06L", 30],
  ["ethioaa", "M13", 6, "aa"], ["ethiopic", "M13", 6, "aa"], ["gregory", "M02", 29, "ce", "bce"],
  ["hebrew", "M05L", 29, "am"], ["indian", "M01", 31, "shaka"], ["islamic-civil", "M12", 30, "ah", "bh"],
  ["islamic-tbla", "M12", 30, "ah", "bh"], ["islamic-umalqura", "M12", 30, "ah", "bh"],
  ["japanese", "M02", 29, "reiwa", "bce"], ["persian", "M12", 30, "ap"], ["roc", "M02", 29, "roc", "broc"],
];
for (const [calendar, monthCode, day, posEra, negEra] of testData) {{
  expect(calendar + " year -999999", RangeError, () => MD.from({{ year: -999999, monthCode, day, calendar }}));
  expect(calendar + " year +999999", RangeError, () => MD.from({{ year: 999999, monthCode, day, calendar }}));
  if (posEra) {{
    expect(calendar + " era year +999999 " + posEra, RangeError,
           () => MD.from({{ eraYear: 999999, era: posEra, monthCode, day, calendar }}));
    if (negEra) {{
      expect(calendar + " era year +999999 " + negEra, RangeError,
             () => MD.from({{ eraYear: 999999, era: negEra, monthCode, day, calendar }}));
    }} else {{
      expect(calendar + " era year -999999 " + posEra, RangeError,
             () => MD.from({{ eraYear: -999999, era: posEra, monthCode, day, calendar }}));
    }}
  }}
}}
return done();
}})()
"#
    ));
}
