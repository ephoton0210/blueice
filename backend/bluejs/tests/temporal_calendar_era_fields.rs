// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for how a property bag's `era`/`eraYear` fields resolve
//! (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`), for the five
//! types that read calendar fields: `PlainDate`, `PlainDateTime`,
//! `ZonedDateTime`, `PlainYearMonth` and `PlainMonthDay`.
//!
//! `CalendarExtraFields`/`CalendarResolveFields` (Intl.Era-monthcode):
//!
//! - A calendar **without eras** (`iso8601`, `chinese`, `dangi`) has no `era`
//!   or `eraYear` field at all: a bag carrying them resolves through `year`
//!   exactly as if they were absent, and without a `year` it is a `TypeError`
//!   (`intl402/Temporal/*/from/calendar-not-supporting-eras.js`). The
//!   `PlainDate`/`PlainDateTime`/`ZonedDateTime` path used to forward them to
//!   `icu_calendar`, which rejected the era for these calendars.
//! - A calendar **with eras** needs `era` and `eraYear` together: one without
//!   the other is a `TypeError`, not the `RangeError` `eraYear`-only used to
//!   report (`.../from/one-of-era-erayear-undefined.js`).
//! - An unknown era name is a `RangeError` (`.../from/calendar-invalid-era.js`).
//! - `year` given next to `era`/`eraYear` must agree with them.

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

/// `expect(label, ErrorType, fn)` and the five `from` entry points, each taking
/// the calendar-specific fields and adding what its type needs (a day, a time
/// zone).
const PRELUDE: &str = r#"
const failures = [];
function expect(label, type, fn) {
  try { fn(); failures.push(label + ": did not throw " + type.name); }
  catch (e) { if (!(e instanceof type)) failures.push(label + ": expected " + type.name + " got " + e.name + ": " + e.message); }
}
function accepts(label, fn) {
  try { return fn(); } catch (e) { failures.push(label + ": threw " + e.name + ": " + e.message); }
}
const makers = {
  PlainDate: (fields) => Temporal.PlainDate.from(Object.assign({ day: 1 }, fields)),
  PlainDateTime: (fields) => Temporal.PlainDateTime.from(Object.assign({ day: 1, hour: 12 }, fields)),
  ZonedDateTime: (fields) => Temporal.ZonedDateTime.from(Object.assign({ day: 1, hour: 12, timeZone: "UTC" }, fields)),
  PlainYearMonth: (fields) => Temporal.PlainYearMonth.from(fields),
};
function done() { return failures.length ? "\n" + failures.join("\n") : "ok"; }
"#;

/// `calendar-invalid-era.js` for `PlainDate`, `PlainDateTime` and
/// `ZonedDateTime` (plus `PlainYearMonth`, which already behaved).
#[test]
fn an_unknown_era_is_a_range_error_for_calendars_with_eras_and_ignored_without() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const withEras = ["buddhist", "coptic", "ethioaa", "ethiopic", "gregory", "hebrew", "indian",
                  "islamic-civil", "islamic-tbla", "islamic-umalqura", "japanese", "persian", "roc"];
const withoutEras = ["chinese", "dangi"];
for (const [type, make] of Object.entries(makers)) {{
  for (const calendar of withEras) {{
    expect(type + " " + calendar + " xyz era", RangeError,
           () => make({{ year: 2025, month: 1, era: "xyz", eraYear: 2025, calendar }}));
  }}
  for (const calendar of withoutEras) {{
    const result = accepts(type + " " + calendar + " ignores era",
                           () => make({{ year: 2025, month: 1, era: "xyz", eraYear: 2025, calendar }}));
    if (result !== undefined && result.year !== 2025) failures.push(type + " " + calendar + ": year " + result.year);
  }}
}}
// PlainMonthDay carries no year of its own, but reads era/eraYear the same way.
for (const calendar of withEras) {{
  expect("PlainMonthDay " + calendar + " xyz era", RangeError,
         () => Temporal.PlainMonthDay.from({{ year: 2025, monthCode: "M01", day: 1, era: "xyz", eraYear: 2025, calendar }}));
}}
for (const calendar of withoutEras) {{
  accepts("PlainMonthDay " + calendar + " ignores era",
          () => Temporal.PlainMonthDay.from({{ monthCode: "M01", day: 1, era: "xyz", eraYear: 2025, calendar }}));
}}
return done();
}})()
"#
    ));
}

/// `calendar-not-supporting-eras.js`: for `iso8601`, `chinese` and `dangi` the
/// era fields neither resolve the year nor get in its way.
#[test]
fn era_fields_are_ignored_by_calendars_without_eras() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
for (const calendar of ["iso8601", "chinese", "dangi"]) {{
  for (const [type, make] of Object.entries(makers)) {{
    const withYear = accepts(type + " " + calendar + " year",
      () => make({{ era: "foobar", eraYear: 1, year: 2025, monthCode: "M01", calendar }}));
    if (withYear !== undefined) {{
      if (withYear.year !== 2025) failures.push(type + " " + calendar + ": year " + withYear.year);
      if (withYear.era !== undefined || withYear.eraYear !== undefined) {{
        failures.push(type + " " + calendar + ": era " + withYear.era + " eraYear " + withYear.eraYear);
      }}
      if (withYear.calendarId !== calendar) failures.push(type + " " + calendar + ": calendarId " + withYear.calendarId);
    }}
    expect(type + " " + calendar + " no year", TypeError,
           () => make({{ era: "foobar", eraYear: 1, monthCode: "M01", calendar }}));
  }}
}}
return done();
}})()
"#
    ));
}

/// `one-of-era-erayear-undefined.js`: era without eraYear, and eraYear without
/// era, for a calendar that has eras.
#[test]
fn era_and_era_year_must_be_supplied_together() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
for (const calendar of ["gregory", "hebrew", "japanese", "islamic-civil", "coptic"]) {{
  for (const [type, make] of Object.entries(makers)) {{
    const base = {{ year: 2000, month: 5, calendar }};
    expect(type + " " + calendar + " era only", TypeError,
           () => make(Object.assign({{ era: calendar === "gregory" ? "ce" : "am" }}, base)));
    expect(type + " " + calendar + " eraYear only", TypeError, () => make(Object.assign({{ eraYear: 1 }}, base)));
    // `undefined` counts as absent.
    expect(type + " " + calendar + " eraYear undefined", TypeError,
           () => make(Object.assign({{ era: calendar === "gregory" ? "ce" : "am", eraYear: undefined }}, base)));
  }}
  const monthDayBase = {{ year: 2000, monthCode: "M05", day: 1, calendar }};
  expect("PlainMonthDay " + calendar + " era only", TypeError,
         () => Temporal.PlainMonthDay.from(Object.assign({{ era: calendar === "gregory" ? "ce" : "am" }}, monthDayBase)));
  expect("PlainMonthDay " + calendar + " eraYear only", TypeError,
         () => Temporal.PlainMonthDay.from(Object.assign({{ eraYear: 1 }}, monthDayBase)));
}}
return done();
}})()
"#
    ));
}

/// Both fields together resolve the year on their own (no `year` needed), and
/// a `year` next to them must agree.
#[test]
fn era_year_pair_resolves_the_year_and_must_agree_with_an_explicit_year() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
for (const [type, make] of Object.entries(makers)) {{
  const alone = accepts(type + " era+eraYear", () => make({{ era: "ce", eraYear: 2024, month: 3, calendar: "gregory" }}));
  if (alone !== undefined && alone.year !== 2024) failures.push(type + " era+eraYear: year " + alone.year);
  const bce = accepts(type + " bce", () => make({{ era: "bce", eraYear: 5, month: 3, calendar: "gregory" }}));
  if (bce !== undefined && bce.year !== -4) failures.push(type + " bce: year " + bce.year);
  // Consistent explicit year.
  accepts(type + " consistent", () => make({{ era: "ce", eraYear: 2024, year: 2024, month: 3, calendar: "gregory" }}));
  // Inconsistent explicit year.
  expect(type + " inconsistent year", RangeError,
         () => make({{ era: "ce", eraYear: 2024, year: 2023, month: 3, calendar: "gregory" }}));
  // A non-finite era year is a range error, not a type error.
  expect(type + " infinite eraYear", RangeError, () => make({{ era: "ce", eraYear: Infinity, month: 3, calendar: "gregory" }}));
}}
return done();
}})()
"#
    ));
}

/// A calendar's field list decides which properties of the bag are read at all:
/// `chinese`/`dangi`/`iso8601` never touch `era`/`eraYear` (a getter there would
/// be observable), a calendar with eras reads both, in alphabetical order with
/// the other fields.
#[test]
fn only_calendars_with_eras_read_the_era_properties() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
function observed(fields) {{
  const log = [];
  const bag = {{}};
  for (const [key, value] of Object.entries(fields)) {{
    Object.defineProperty(bag, key, {{ enumerable: true, get() {{ log.push(key); return value; }} }});
  }}
  return {{ bag, log }};
}}
const cases = [
  ["PlainDate", (bag) => Temporal.PlainDate.from(bag), {{ day: 1, year: 2025, month: 3 }}],
  ["PlainDateTime", (bag) => Temporal.PlainDateTime.from(bag), {{ day: 1, year: 2025, month: 3 }}],
  ["ZonedDateTime", (bag) => Temporal.ZonedDateTime.from(bag), {{ day: 1, year: 2025, month: 3, timeZone: "UTC" }}],
  ["PlainYearMonth", (bag) => Temporal.PlainYearMonth.from(bag), {{ year: 2025, month: 3 }}],
];
for (const [type, from, fields] of cases) {{
  for (const calendar of ["iso8601", "chinese", "dangi"]) {{
    const {{ bag, log }} = observed(Object.assign({{ era: "ce", eraYear: 2025, calendar }}, fields));
    accepts(type + " " + calendar, () => from(bag));
    if (log.includes("era") || log.includes("eraYear")) failures.push(type + " " + calendar + " read " + log.join());
  }}
  const {{ bag, log }} = observed(Object.assign({{ era: "ce", eraYear: 2025, calendar: "gregory" }}, fields));
  accepts(type + " gregory", () => from(bag));
  if (!log.includes("era") || !log.includes("eraYear")) failures.push(type + " gregory did not read the era fields: " + log.join());
  if (log.indexOf("era") > log.indexOf("eraYear")) failures.push(type + " gregory read eraYear before era: " + log.join());
}}
return done();
}})()
"#
    ));
}
