// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the closed set of calendar identifiers Temporal
//! accepts (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `calendar::calendar_kind` used to also map the legacy ECMA-402 identifiers
//! `"islamic"` and `"islamic-rgsa"` to a tabular Hijri variant, so Temporal
//! happily built `PlainDate`/`PlainDateTime`/`PlainYearMonth`/`PlainMonthDay`/
//! `ZonedDateTime` values in a calendar that is not one of its 16 supported
//! ones. `AvailableCalendars()` (Gecko's closed `CalendarId` enum) is
//! `iso8601`, `buddhist`, `chinese`, `coptic`, `dangi`, `ethioaa`, `ethiopic`,
//! `gregory`, `hebrew`, `indian`, `islamic-civil`, `islamic-tbla`,
//! `islamic-umalqura`, `japanese`, `persian` and `roc`; `"islamic"` and
//! `"islamic-rgsa"` exist only as `Intl.DateTimeFormat` fallbacks, never in
//! Temporal, and the `intl402/Temporal/*/from/islamic{,-rgsa}.js` fixtures
//! (ten files, five types) pin the resulting `RangeError`. The two spelling
//! aliases CLDR defines for supported calendars (`islamicc`,
//! `ethiopic-amete-alem`) keep canonicalizing.

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
function rangeError(label, fn) {
  try { fn(); failures.push(label + ": did not throw"); }
  catch (e) { if (!(e instanceof RangeError)) failures.push(label + ": threw " + e.name); }
}
function done() { return failures.length ? "\n" + failures.join("\n") : "ok"; }
"#;

/// `intl402/Temporal/{PlainDate,PlainDateTime,PlainYearMonth,PlainMonthDay,
/// ZonedDateTime}/from/islamic{,-rgsa}.js`, plus the same identifier reached
/// through every other entry point that canonicalizes one.
#[test]
fn islamic_and_islamic_rgsa_are_not_temporal_calendars() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
for (const calendar of ["islamic", "islamic-rgsa"]) {{
  const bag = (extra) => Object.assign({{ calendar }}, extra);
  rangeError(calendar + " PlainDate.from", () => Temporal.PlainDate.from(bag({{ year: 1500, month: 1, day: 1 }})));
  rangeError(calendar + " PlainDateTime.from", () => Temporal.PlainDateTime.from(bag({{ year: 1500, month: 1, day: 1 }})));
  rangeError(calendar + " PlainYearMonth.from", () => Temporal.PlainYearMonth.from(bag({{ year: 1500, month: 1 }})));
  rangeError(calendar + " PlainMonthDay.from", () => Temporal.PlainMonthDay.from(bag({{ year: 1500, month: 1, day: 1 }})));
  rangeError(calendar + " ZonedDateTime.from", () =>
    Temporal.ZonedDateTime.from(bag({{ year: 1500, month: 1, day: 1, timeZone: "UTC" }})));

  rangeError(calendar + " PlainDate constructor", () => new Temporal.PlainDate(2000, 1, 1, calendar));
  rangeError(calendar + " PlainDateTime constructor", () => new Temporal.PlainDateTime(2000, 1, 1, 0, 0, 0, 0, 0, 0, calendar));
  rangeError(calendar + " PlainYearMonth constructor", () => new Temporal.PlainYearMonth(2000, 1, calendar));
  rangeError(calendar + " PlainMonthDay constructor", () => new Temporal.PlainMonthDay(1, 1, calendar));
  rangeError(calendar + " ZonedDateTime constructor", () => new Temporal.ZonedDateTime(0n, "UTC", calendar));

  const date = new Temporal.PlainDate(2000, 1, 1);
  rangeError(calendar + " withCalendar", () => date.withCalendar(calendar));
  rangeError(calendar + " string annotation", () => Temporal.PlainDate.from("2000-01-01[u-ca=" + calendar + "]"));
}}
return done();
}})()
"#
    ));
}

/// The aliases and every supported Hijri variant keep working, so the
/// tightening above cannot have swallowed a neighbour.
#[test]
fn supported_islamic_calendars_and_aliases_are_unaffected() {
    assert_ok(
        r#"
(function() {
  const failures = [];
  const id = (calendar) => new Temporal.PlainDate(2000, 1, 1, calendar).calendarId;
  const expected = [
    ["islamic-civil", "islamic-civil"],
    ["islamic-tbla", "islamic-tbla"],
    ["islamic-umalqura", "islamic-umalqura"],
    ["islamicc", "islamic-civil"],
    ["ethiopic-amete-alem", "ethioaa"],
  ];
  for (const [input, canonical] of expected) {
    try {
      const got = id(input);
      if (got !== canonical) failures.push(input + ": expected " + canonical + " got " + got);
    } catch (e) { failures.push(input + ": threw " + e.name); }
  }
  return failures.length ? "\n" + failures.join("\n") : "ok";
})()
"#,
    );
}
