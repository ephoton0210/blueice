// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.prototype.hoursInDay` at the edges of the
//! representable range (Phase 26 Stage 3 --
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `hoursInDay` is `GetStartOfDay` of the receiver's date and of the following date, subtracted.
//! Either start of day can fall outside the representable instants for a value near the edge of
//! the range, and the specification turns that into a `RangeError` -- the getter previously
//! computed the length unchecked (`hoursInDay/get-start-of-day-throws.js` and
//! `next-day-out-of-range.js`).

use blueice_bluejs::{compile, parse, Value, Vm};

fn run(body: &str) {
    let source = format!(
        r#"(function() {{
const fails = [];
function expect(kind, label, f) {{
  let outcome = "none";
  try {{ f(); }} catch (e) {{
    outcome = e instanceof RangeError ? "RangeError" : e instanceof TypeError ? "TypeError" : "other:" + String(e);
  }}
  if (outcome !== kind) fails.push(label + " -> " + outcome + " (expected " + kind + ")");
}}
function same(label, actual, expected) {{
  if (!Object.is(actual, expected)) fails.push(label + " => " + String(actual) + " !== " + String(expected));
}}
const T = Temporal;
{body}
return fails.length === 0 ? true : fails.join("\n");
}})()"#
    );
    let program =
        compile(&parse(&source).expect("test script parses")).expect("test script compiles");
    match Vm::default().execute(&program) {
        Ok(Value::Bool(true)) => {}
        Ok(Value::String(text)) => panic!("failing cases:\n{}", text.to_utf8().unwrap()),
        other => panic!("unexpected result: {other:?}"),
    }
}

/// `hoursInDay/get-start-of-day-throws.js` and `next-day-out-of-range.js`.
#[test]
fn hours_in_day_throws_when_a_start_of_day_is_not_representable() {
    run(r#"
      const min = -864n * 10n ** 19n;
      const max = 864n * 10n ** 19n;
      // Today's start of day precedes the first instant.
      expect("RangeError", "min at -01", () => new T.ZonedDateTime(min, "-01").hoursInDay);
      expect("RangeError", "min at +01", () => new T.ZonedDateTime(min, "+01").hoursInDay);
      // Tomorrow's start of day follows the last instant.
      expect("RangeError", "max at -01", () => new T.ZonedDateTime(max, "-01").hoursInDay);
      expect("RangeError", "max at UTC", () => new T.ZonedDateTime(max, "UTC").hoursInDay);
      expect("RangeError", "last date", () => new T.ZonedDateTime(86400_0000_0000_000_000_000n, "UTC").hoursInDay);
      // The very first instant is fine in UTC: its start of day *is* the first instant.
      same("min at UTC", new T.ZonedDateTime(min, "UTC").hoursInDay, 24);
      same("min at +00", new T.ZonedDateTime(min, "+00").hoursInDay, 24);
    "#);
}

/// The ordinary answers must not change: 24 hours, 23 or 25 across a DST transition.
#[test]
fn hours_in_day_is_the_real_length_of_the_day() {
    run(r#"
      same("UTC", new T.ZonedDateTime(0n, "UTC").hoursInDay, 24);
      same("offset zone", new T.ZonedDateTime(0n, "-05:00").hoursInDay, 24);
      const spring = T.PlainDateTime.from("2000-04-02T12:00").toZonedDateTime("America/Los_Angeles");
      same("spring forward", spring.hoursInDay, 23);
      const fall = T.PlainDateTime.from("2000-10-29T12:00").toZonedDateTime("America/Los_Angeles");
      same("fall back", fall.hoursInDay, 25);
      same("an ordinary day in a DST zone", T.PlainDateTime.from("2000-01-01T12:00").toZonedDateTime("America/Los_Angeles").hoursInDay, 24);
      same("far from the edge", T.PlainDateTime.from("+275760-01-01T12:00").toZonedDateTime("UTC").hoursInDay, 24);
      same("the day before the last date", new T.ZonedDateTime(8639_9999_9999_999_999_999n - 86400_000_000_000n, "UTC").hoursInDay, 24);
    "#);
}
