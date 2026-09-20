// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for non-ISO calendar dates at the extremes of
//! Temporal's supported range (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! Three separate limits made every such date fail:
//!
//! 1. Turning a stored ISO date into a calendar date went through
//!    `icu_calendar::Date::try_new_iso`, which only accepts years
//!    `-9999..=9999`. Every getter, `withCalendar` and `from` on an
//!    extreme-year date in a non-ISO calendar therefore threw a spurious
//!    "invalid Temporal ISO date" (`calendar::iso_date_from_civil` now builds it
//!    from a rata die instead).
//! 2. A property bag's `eraYear` was range-checked to `-9999..=9999`, and its
//!    `year` to Temporal's ISO years, both far narrower than a real calendar
//!    year (`gregory` era year 275760, `islamic-civil` 283583, `ethioaa`
//!    281247, ...). Both are now plain integers, so
//! 3. the exact representable-range check moved to the *resolved* ISO
//!    date(-time), and must still reject a bag that lands outside it
//!    (`a_bag_resolving_outside_the_supported_range_is_still_a_range_error`).
//!
//! The tables below are copied from `intl402/Temporal/PlainDate/from/
//! extreme-dates.js`, `.../prototype/withCalendar/extreme-dates.js` and
//! `PlainYearMonth/from/extreme-dates.js` (the same rows drive the
//! `PlainDateTime`/`ZonedDateTime` variants, which differ only in the wall-clock
//! time of the extremes).

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

/// `[calendar, minYear, minMonth, minMonthCode, minDay, minEra, minEraYear,
/// maxYear, maxMonth, maxMonthCode, maxDay, maxEra, maxEraYear]`: the earliest
/// and latest representable *date* in each calendar (ISO -271821-04-19 and
/// +275760-09-13).
const DATE_TABLE: &str = r#"
const DATE_TABLE = [
  ["buddhist", -271278, 4, "M04", 19, "be", -271278, 276303, 9, "M09", 13, "be", 276303],
  ["coptic", -272099, 3, "M03", 23, "am", -272099, 275471, 5, "M05", 22, "am", 275471],
  ["ethioaa", -266323, 3, "M03", 23, "aa", -266323, 281247, 5, "M05", 22, "aa", 281247],
  ["ethiopic", -271823, 3, "M03", 23, "aa", -266323, 275747, 5, "M05", 22, "am", 275747],
  ["gregory", -271821, 4, "M04", 19, "bce", 271822, 275760, 9, "M09", 13, "ce", 275760],
  ["hebrew", -268058, 11, "M11", 4, "am", -268058, 279517, 10, "M09", 11, "am", 279517],
  ["indian", -271899, 1, "M01", 29, "shaka", -271899, 275682, 6, "M06", 22, "shaka", 275682],
  ["islamic-civil", -280804, 3, "M03", 21, "bh", 280805, 283583, 5, "M05", 23, "ah", 283583],
  ["islamic-tbla", -280804, 3, "M03", 22, "bh", 280805, 283583, 5, "M05", 24, "ah", 283583],
  ["islamic-umalqura", -280804, 3, "M03", 21, "bh", 280805, 283583, 5, "M05", 23, "ah", 283583],
  ["japanese", -271821, 4, "M04", 19, "bce", 271822, 275760, 9, "M09", 13, "reiwa", 273742],
  ["persian", -272442, 1, "M01", 9, "ap", -272442, 275139, 7, "M07", 12, "ap", 275139],
  ["roc", -273732, 4, "M04", 19, "broc", 273733, 273849, 9, "M09", 13, "roc", 273849],
];
const ERA_ALIASES = { ad: "ce", bc: "bce" };
const era = (value) => ERA_ALIASES[value] || value;
"#;

const PRELUDE: &str = r#"
const failures = [];
function check(label, got, expected) {
  if (got !== expected) failures.push(label + ": expected " + String(expected) + " got " + String(got));
}
// year, month, monthCode, day, era, eraYear of a date-like value.
function fields(value) {
  return [value.year, value.month, value.monthCode, value.day, era(value.era), value.eraYear].join();
}
function done() { return failures.length ? "\n" + failures.join("\n") : "ok"; }
"#;

/// `PlainDate/from/extreme-dates.js`: the bag spells the year three ways at once
/// (`year`, `era`+`eraYear`) plus both month spellings, all consistent.
#[test]
fn plain_date_from_at_the_extremes_of_every_calendar() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
{DATE_TABLE}
for (const [calendar, minYear, minMonth, minMonthCode, minDay, minEra, minEraYear,
            maxYear, maxMonth, maxMonthCode, maxDay, maxEra, maxEraYear] of DATE_TABLE) {{
  try {{
    const min = Temporal.PlainDate.from({{ calendar, year: minYear, era: minEra, eraYear: minEraYear,
                                          month: minMonth, monthCode: minMonthCode, day: minDay }});
    check(calendar + " min", fields(min), [minYear, minMonth, minMonthCode, minDay, minEra, minEraYear].join());
    const max = Temporal.PlainDate.from({{ calendar, year: maxYear, era: maxEra, eraYear: maxEraYear,
                                          month: maxMonth, monthCode: maxMonthCode, day: maxDay }});
    check(calendar + " max", fields(max), [maxYear, maxMonth, maxMonthCode, maxDay, maxEra, maxEraYear].join());
  }} catch (e) {{
    failures.push(calendar + ": " + e.name + ": " + e.message);
  }}
}}
return done();
}})()
"#
    ));
}

/// `withCalendar/extreme-dates.js` for `PlainDate`, `PlainDateTime` and
/// `ZonedDateTime`: the ISO extremes converted into each calendar.
#[test]
fn with_calendar_at_the_extremes_for_every_date_type() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
{DATE_TABLE}
const minDate = new Temporal.PlainDate(-271821, 4, 19);
const maxDate = new Temporal.PlainDate(275760, 9, 13);
const minDateTime = new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 1);
const maxDateTime = new Temporal.PlainDateTime(275760, 9, 13, 23, 59, 59, 999, 999, 999);
const minZoned = new Temporal.ZonedDateTime(-86400_0000_0000_000_000_000n, "UTC");
const maxZoned = new Temporal.ZonedDateTime(86400_0000_0000_000_000_000n, "UTC");
for (const [calendar, minYear, minMonth, minMonthCode, minDay, minEra, minEraYear,
            maxYear, maxMonth, maxMonthCode, maxDay, maxEra, maxEraYear] of DATE_TABLE) {{
  try {{
    const minFields = [minYear, minMonth, minMonthCode, minDay, minEra, minEraYear].join();
    const maxFields = [maxYear, maxMonth, maxMonthCode, maxDay, maxEra, maxEraYear].join();
    check(calendar + " PlainDate min", fields(minDate.withCalendar(calendar)), minFields);
    check(calendar + " PlainDate max", fields(maxDate.withCalendar(calendar)), maxFields);
    check(calendar + " PlainDateTime min", fields(minDateTime.withCalendar(calendar)), minFields);
    check(calendar + " PlainDateTime max", fields(maxDateTime.withCalendar(calendar)), maxFields);
    // The ZonedDateTime extremes are exactly +-8.64e21 ns: midnight UTC of ISO
    // -271821-04-20 (one day after the PlainDate minimum) and of +275760-09-13.
    const zonedMin = minZoned.withCalendar(calendar);
    check(calendar + " ZonedDateTime min year", zonedMin.year, minYear);
    check(calendar + " ZonedDateTime min era", era(zonedMin.era), minEra);
    check(calendar + " ZonedDateTime min eraYear", zonedMin.eraYear, minEraYear);
    const zonedMax = maxZoned.withCalendar(calendar);
    check(calendar + " ZonedDateTime max year", zonedMax.year, maxYear);
    check(calendar + " ZonedDateTime max monthCode", zonedMax.monthCode, maxMonthCode);
    check(calendar + " ZonedDateTime max day", zonedMax.day, maxDay);
  }} catch (e) {{
    failures.push(calendar + ": " + e.name + ": " + e.message);
  }}
}}
return done();
}})()
"#
    ));
}

/// `PlainDateTime/from/extreme-dates.js` and `ZonedDateTime/from/
/// extreme-dates.js` differ from `PlainDate`'s only by the time-of-day of the
/// extremes (the minimum has one nanosecond, the maximum is the last
/// nanosecond of its day); the ZonedDateTime extremes are exactly +-8.64e21 ns.
#[test]
fn plain_date_time_and_zoned_date_time_from_at_the_extremes() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
{DATE_TABLE}
for (const [calendar, minYear, minMonth, minMonthCode, minDay, minEra, minEraYear,
            maxYear, maxMonth, maxMonthCode, maxDay, maxEra, maxEraYear] of DATE_TABLE) {{
  try {{
    const min = Temporal.PlainDateTime.from({{ calendar, year: minYear, era: minEra, eraYear: minEraYear,
                                              month: minMonth, monthCode: minMonthCode, day: minDay, nanosecond: 1 }});
    check(calendar + " PlainDateTime min", fields(min), [minYear, minMonth, minMonthCode, minDay, minEra, minEraYear].join());
    check(calendar + " PlainDateTime min ns", min.nanosecond, 1);
    const max = Temporal.PlainDateTime.from({{ calendar, year: maxYear, era: maxEra, eraYear: maxEraYear,
                                              month: maxMonth, monthCode: maxMonthCode, day: maxDay,
                                              hour: 23, minute: 59, second: 59, millisecond: 999, microsecond: 999, nanosecond: 999 }});
    check(calendar + " PlainDateTime max", fields(max), [maxYear, maxMonth, maxMonthCode, maxDay, maxEra, maxEraYear].join());
    check(calendar + " PlainDateTime max time", [max.hour, max.minute, max.second].join(), "23,59,59");

    // `ZonedDateTime` extremes are midnight UTC of the day after the PlainDate minimum.
    const zMin = Temporal.ZonedDateTime.from({{ calendar, year: minYear, era: minEra, eraYear: minEraYear,
                                               month: minMonth, monthCode: minMonthCode, day: minDay + 1, timeZone: "UTC" }});
    check(calendar + " ZonedDateTime min ns", String(zMin.epochNanoseconds), "-8640000000000000000000");
  }} catch (e) {{
    failures.push(calendar + ": " + e.name + ": " + e.message);
  }}
}}
return done();
}})()
"#
    ));
}

/// `PlainYearMonth/from/extreme-dates.js`: `[calendar, minYear, minMonth,
/// minMonthCode, minEra, minEraYear, minISODay, maxYear, maxMonth, maxMonthCode,
/// maxEra, maxEraYear, maxISODay]`. The earliest year-month is the first month
/// whose first day is after ISO -271821-04-19.
#[test]
fn plain_year_month_from_at_the_extremes_of_every_calendar() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const ERA_ALIASES = {{ ad: "ce", bc: "bce" }};
const era = (value) => ERA_ALIASES[value] || value;
const TABLE = [
  ["buddhist", -271278, 5, "M05", "be", -271278, 1, 276303, 9, "M09", "be", 276303, 1],
  ["coptic", -272099, 4, "M04", "am", -272099, 27, 275471, 6, "M06", "am", 275471, 22],
  ["ethioaa", -266323, 4, "M04", "aa", -266323, 27, 281247, 6, "M06", "aa", 281247, 22],
  ["ethiopic", -271823, 4, "M04", "aa", -266323, 27, 275747, 6, "M06", "am", 275747, 22],
  ["gregory", -271821, 5, "M05", "bce", 271822, 1, 275760, 9, "M09", "ce", 275760, 1],
  ["hebrew", -268058, 12, "M12", "am", -268058, 16, 279517, 10, "M09", "am", 279517, 3],
  ["indian", -271899, 2, "M02", "shaka", -271899, 21, 275682, 7, "M07", "shaka", 275682, 23],
  ["islamic-civil", -280804, 4, "M04", "bh", 280805, 29, 283583, 6, "M06", "ah", 283583, 21],
  ["islamic-tbla", -280804, 4, "M04", "bh", 280805, 28, 283583, 6, "M06", "ah", 283583, 20],
  ["islamic-umalqura", -280804, 4, "M04", "bh", 280805, 29, 283583, 6, "M06", "ah", 283583, 21],
  ["japanese", -271821, 5, "M05", "bce", 271822, 1, 275760, 9, "M09", "reiwa", 273742, 1],
  ["persian", -272442, 2, "M02", "ap", -272442, 12, 275139, 7, "M07", "ap", 275139, 2],
  ["roc", -273732, 5, "M05", "broc", 273733, 1, 273849, 9, "M09", "roc", 273849, 1],
];
const isoDay = (yearMonth) => Number(yearMonth.toString({{ calendarName: "always" }}).slice(1).split("-")[2].slice(0, 2));
for (const [calendar, minYear, minMonth, minMonthCode, minEra, minEraYear, minISODay,
            maxYear, maxMonth, maxMonthCode, maxEra, maxEraYear, maxISODay] of TABLE) {{
  try {{
    const min = Temporal.PlainYearMonth.from({{ calendar, year: minYear, era: minEra, eraYear: minEraYear,
                                               month: minMonth, monthCode: minMonthCode }});
    check(calendar + " min", [min.year, min.month, min.monthCode, era(min.era), min.eraYear, isoDay(min)].join(),
          [minYear, minMonth, minMonthCode, minEra, minEraYear, minISODay].join());
    const max = Temporal.PlainYearMonth.from({{ calendar, year: maxYear, era: maxEra, eraYear: maxEraYear,
                                               month: maxMonth, monthCode: maxMonthCode }});
    check(calendar + " max", [max.year, max.month, max.monthCode, era(max.era), max.eraYear, isoDay(max)].join(),
          [maxYear, maxMonth, maxMonthCode, maxEra, maxEraYear, maxISODay].join());
  }} catch (e) {{
    failures.push(calendar + ": " + e.name + ": " + e.message);
  }}
}}
// `PlainYearMonth.prototype.with` can reach the minimum valid year-month too
// (`with/minimum-valid-year-month.js`): the reference day is the first of the
// month even though it precedes the minimum *date*.
try {{
  const apr2000 = new Temporal.PlainYearMonth(2000, 4, "gregory");
  const min = apr2000.with({{ year: -271821 }});
  check("with min", [min.year, min.month, min.monthCode, era(min.era), min.eraYear, isoDay(min)].join(),
        [-271821, 4, "M04", "bce", 271822, 1].join());
}} catch (e) {{
  failures.push("with min: " + e.name + ": " + e.message);
}}
return done();
}})()
"#
    ));
}

/// The calendars whose far-past/far-future dates are only approximated (the
/// lunisolar ones, and `islamic-umalqura` outside 1300-1500 AH) still accept
/// them and are exact inside the accurate range.
#[test]
fn lunisolar_and_umalqura_calendars_accept_far_dates_and_are_exact_where_documented() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
for (const calendar of ["chinese", "dangi"]) {{
  for (const year of [-250000, 250000]) {{
    try {{
      const date = Temporal.PlainDate.from({{ calendar, year, month: 1, day: 1 }});
      check(calendar + " far date year " + year, date.year, year);
      const ym = Temporal.PlainYearMonth.from({{ calendar, year, month: 1 }});
      check(calendar + " far year-month year " + year, ym.year, year);
    }} catch (e) {{
      failures.push(calendar + " " + year + ": " + e.name + ": " + e.message);
    }}
  }}
}}
const chineseMin = Temporal.PlainDate.from({{ calendar: "chinese", year: 1900, month: 1, day: 1 }});
check("chinese 1900", [chineseMin.year, chineseMin.monthCode, chineseMin.day].join(), "1900,M01,1");
check("chinese 1900 iso", chineseMin.withCalendar("iso8601").toString(), "1900-01-31");
const chineseMax = Temporal.PlainDate.from({{ calendar: "chinese", year: 2100, month: 12, day: 29 }});
check("chinese 2100", [chineseMax.year, chineseMax.monthCode, chineseMax.day].join(), "2100,M12,29");
const dangiMax = Temporal.PlainDate.from({{ calendar: "dangi", year: 2050, month: 13, day: 29 }});
check("dangi 2050", [dangiMax.year, dangiMax.month, dangiMax.monthCode, dangiMax.day].join(), "2050,13,M12,29");
const umalquraMin = Temporal.PlainDate.from({{ calendar: "islamic-umalqura", year: 1300, month: 1, day: 1 }});
check("umalqura 1300", [umalquraMin.era, umalquraMin.eraYear].join(), "ah,1300");
const umalquraMax = Temporal.PlainDate.from({{ calendar: "islamic-umalqura", year: 1500, month: 12, day: 30 }});
check("umalqura 1500", [umalquraMax.era, umalquraMax.eraYear, umalquraMax.day].join(), "ah,1500,30");
return done();
}})()
"#
    ));
}

/// The bag's `year` used to be bounded to Temporal's ISO years, which doubled as
/// the range check. Now that it is a plain calendar year, the resolved ISO
/// date(-time) must still be judged against Temporal's exact range.
#[test]
fn a_bag_resolving_outside_the_supported_range_is_still_a_range_error() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
function expect(label, fn) {{
  try {{ fn(); failures.push(label + ": did not throw"); }}
  catch (e) {{ if (!(e instanceof RangeError)) failures.push(label + ": " + e.name + ": " + e.message); }}
}}
function accepts(label, fn) {{
  try {{ fn(); }} catch (e) {{ failures.push(label + ": " + e.name + ": " + e.message); }}
}}
// ISO: one day either side of the edges.
expect("iso year 275761", () => Temporal.PlainDate.from({{ year: 275761, month: 1, day: 1 }}));
expect("iso max + 1 day", () => Temporal.PlainDate.from({{ year: 275760, month: 9, day: 14 }}));
accepts("iso max", () => Temporal.PlainDate.from({{ year: 275760, month: 9, day: 13 }}));
expect("iso min - 1 day", () => Temporal.PlainDate.from({{ year: -271821, month: 4, day: 18 }}));
accepts("iso min", () => Temporal.PlainDate.from({{ year: -271821, month: 4, day: 19 }}));
// A PlainDateTime is judged at nanosecond precision: midnight of the minimum date is out.
expect("iso datetime min midnight", () => Temporal.PlainDateTime.from({{ year: -271821, month: 4, day: 19 }}));
accepts("iso datetime min + 1ns", () => Temporal.PlainDateTime.from({{ year: -271821, month: 4, day: 19, nanosecond: 1 }}));
expect("iso datetime max + 1 day", () => Temporal.PlainDateTime.from({{ year: 275760, month: 9, day: 14 }}));
// ZonedDateTime: the instant, not the local date, is the limit.
accepts("zoned max", () => Temporal.ZonedDateTime.from({{ year: 275760, month: 9, day: 13, timeZone: "UTC" }}));
expect("zoned max + 1ns", () => Temporal.ZonedDateTime.from({{ year: 275760, month: 9, day: 13, nanosecond: 1, timeZone: "UTC" }}));
// Non-ISO calendars, whose years are wider than ISO's and unbounded by the field read.
for (const [calendar, beyond] of [["gregory", 275761], ["hebrew", 279518], ["islamic-civil", 283584], ["coptic", 275472]]) {{
  expect(calendar + " past the maximum year", () => Temporal.PlainDate.from({{ calendar, year: beyond, month: 12, day: 1 }}));
  expect(calendar + " far past the maximum", () => Temporal.PlainDate.from({{ calendar, year: 2000000000, month: 1, day: 1 }}));
  expect(calendar + " far before the minimum", () => Temporal.PlainDate.from({{ calendar, year: -2000000000, month: 1, day: 1 }}));
  expect(calendar + " datetime past the maximum", () => Temporal.PlainDateTime.from({{ calendar, year: beyond, month: 12, day: 1 }}));
}}
return done();
}})()
"#
    ));
}

/// The calendar getters that read the *calendar's* date must not panic or throw
/// at the extremes either (they used to go through `try_new_iso`).
#[test]
fn getters_work_at_the_extremes_of_every_calendar() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
{DATE_TABLE}
const min = new Temporal.PlainDate(-271821, 4, 19);
const max = new Temporal.PlainDate(275760, 9, 13);
for (const [calendar] of DATE_TABLE) {{
  for (const [label, date] of [["min", min], ["max", max]]) {{
    const value = date.withCalendar(calendar);
    try {{
      const day = value.dayOfYear;
      if (!(day >= 1 && day <= value.daysInYear)) failures.push(calendar + " " + label + " dayOfYear " + day + " of " + value.daysInYear);
      if (!(value.monthsInYear >= 12 && value.monthsInYear <= 13)) failures.push(calendar + " " + label + " monthsInYear " + value.monthsInYear);
      if (!(value.daysInMonth >= 28 && value.daysInMonth <= 31)) failures.push(calendar + " " + label + " daysInMonth " + value.daysInMonth);
      if (typeof value.inLeapYear !== "boolean") failures.push(calendar + " " + label + " inLeapYear");
      // Round trip through the ISO calendar.
      if (value.withCalendar("iso8601").toString() !== date.toString()) failures.push(calendar + " " + label + " round trip");
    }} catch (e) {{
      failures.push(calendar + " " + label + ": " + e.name + ": " + e.message);
    }}
  }}
}}
return done();
}})()
"#
    ));
}
