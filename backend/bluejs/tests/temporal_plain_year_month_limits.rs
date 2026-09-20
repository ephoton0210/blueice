// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainYearMonth`'s behaviour at the edges of
//! Temporal's range and under rounding increments (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! A `PlainYearMonth` itself may be any month from ISO -271821-04 to +275760-09,
//! but every operation that goes through a *date* works on the first day of the
//! month, and that date must be a valid `PlainDate`:
//!
//! - `add`/`subtract` (`AddDurationToYearMonth` step 9) and `since`/`until`
//!   (`DifferenceTemporalPlainYearMonth` steps 8-11) build the day-1 date of
//!   the receiver (and of the argument) and throw a `RangeError` when it is out
//!   of range -- so `-271821-04` (whose first day precedes the minimum date
//!   -271821-04-19) can be constructed but not added to, subtracted from or
//!   differenced against, while an *equal* pair short-circuits to a blank
//!   duration first. `add`/`subtract` read and validate the `overflow` option
//!   before any of those algorithmic checks.
//! - `toPlainDate` throws when the resulting date is out of range.
//! - A rounding increment applies to the *remainder* of the unit (2 years 8
//!   months with `smallestUnit: "months", roundingIncrement: 5` is 2 years 5
//!   months, not 30 months re-split), and the rounding window's far end
//!   (`start + r2` units) must itself be in range.
//!
//! Fixtures: `built-ins/Temporal/PlainYearMonth/prototype/{add,subtract,since,
//! until,toPlainDate}/...` as named on each test.

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
const YM = Temporal.PlainYearMonth;
function expect(label, type, fn) {
  try { fn(); failures.push(label + ": did not throw " + type.name); }
  catch (e) { if (!(e instanceof type)) failures.push(label + ": expected " + type.name + " got " + e.name + ": " + e.message); }
}
function check(label, got, expected) {
  if (String(got) !== String(expected)) failures.push(label + ": expected " + expected + " got " + got);
}
function done() { return failures.length ? "\n" + failures.join("\n") : "ok"; }
"#;

/// `toPlainDate/limits.js`.
#[test]
fn to_plain_date_throws_when_the_date_is_out_of_range() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const min = YM.from("-271821-04");
expect("min day 18", RangeError, () => min.toPlainDate({{ day: 18 }}));
check("min day 19", min.toPlainDate({{ day: 19 }}).toString(), "-271821-04-19");
const max = YM.from("+275760-09");
expect("max day 14", RangeError, () => max.toPlainDate({{ day: 14 }}));
check("max day 13", max.toPlainDate({{ day: 13 }}).toString(), "+275760-09-13");
// The same edges in a non-ISO calendar (the ISO date must be in range too).
const gregory = new YM(-271821, 4, "gregory");
expect("gregory min day 18", RangeError, () => gregory.toPlainDate({{ day: 18 }}));
check("gregory min day 19", gregory.toPlainDate({{ day: 19 }}).withCalendar("iso8601").toString(), "-271821-04-19");
return done();
}})()
"#
    ));
}

/// `add`/`subtract/throws-if-year-outside-valid-iso-range.js`, and the
/// options-first ordering of `options-read-before-algorithmic-validation.js`.
#[test]
fn add_and_subtract_need_a_first_day_that_is_a_valid_date() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const min = new YM(-271821, 4);
const blank = new Temporal.Duration();
expect("min add blank", RangeError, () => min.add(blank));
expect("min subtract blank", RangeError, () => min.subtract(blank));
// The month after the minimum is fine, and so is the maximum with a blank duration.
check("min+1 add blank", new YM(-271821, 5).add(blank).toString(), "-271821-05");
check("max add blank", new YM(275760, 9).add(blank).toString(), "+275760-09");
check("plain add", new YM(2000, 1).add({{ months: 14 }}).toString(), "2001-03");
check("plain subtract", new YM(2000, 1).subtract({{ years: 1, months: 1 }}).toString(), "1998-12");

// Options are read and cast before either RangeError.
const log = [];
const options = {{
  get overflow() {{
    log.push("get");
    return {{ toString() {{ log.push("toString"); return "constrain"; }} }};
  }},
}};
for (const [label, instance, duration] of [
  ["min year-month", new YM(-271821, 4), new Temporal.Duration(0, 1)],
  ["too-low unit", new YM(1999, 12), new Temporal.Duration(0, 0, 1)],
]) {{
  log.length = 0;
  expect(label, RangeError, () => instance.add(duration, options));
  check(label + " reads overflow first", log.join(), "get,toString");
  log.length = 0;
  expect(label + " (subtract)", RangeError, () => instance.subtract(duration, options));
  check(label + " (subtract) reads overflow first", log.join(), "get,toString");
}}
return done();
}})()
"#
    ));
}

/// `since`/`until/argument-string-limits.js`: the argument's *first day* must be
/// in range, so the first month of each end is out.
#[test]
fn since_and_until_arguments_at_the_edge_of_the_range() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const instance = new YM(1970, 1);
const valid = ["-271821-05", "-271821-05-01", "-271821-05-01T00:00", "+275760-09", "+275760-09-30", "+275760-09-30T23:59:59.999999999"];
const invalid = ["-271821-04", "-271821-04-30", "-271821-04-30T23:59:59.999999999", "+275760-10", "+275760-10-01", "+275760-10-01T00:00"];
for (const method of ["since", "until"]) {{
  for (const arg of valid) {{
    try {{ instance[method](arg); }} catch (e) {{ failures.push(method + " valid " + arg + ": " + e.name + ": " + e.message); }}
  }}
  for (const arg of invalid) {{
    expect(method + " invalid " + arg, RangeError, () => instance[method](arg));
  }}
}}
return done();
}})()
"#
    ));
}

/// `since`/`until/throws-if-year-outside-valid-iso-range.js`.
#[test]
fn since_and_until_throw_when_a_first_day_is_not_a_valid_date() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const min = new YM(-271821, 4);
const max = new YM(275760, 9);
const epoch = new YM(1970, 1);
for (const method of ["since", "until"]) {{
  // An equal pair is a blank duration before any date is built.
  check(method + " min vs min", min[method](min), "PT0S");
  check(method + " max vs max", max[method](max), "PT0S");
  expect(method + " min vs max", RangeError, () => min[method](max));
  expect(method + " min vs epoch", RangeError, () => min[method](epoch));
  expect(method + " epoch vs min", RangeError, () => epoch[method](min));
  // The very next month from the minimum is fine.
  check(method + " min+1", new YM(-271821, 5)[method](new YM(-271821, 6)), method === "since" ? "-P1M" : "P1M");
}}
return done();
}})()
"#
    ));
}

/// `since`/`until/roundingincrement-as-expected.js`.
#[test]
fn rounding_increments_apply_to_the_remainder_of_the_unit() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const earlier = new YM(2019, 1);
const later = new YM(2021, 9); // 2 years 8 months after `earlier`
check("since years increment 4", later.since(earlier, {{ smallestUnit: "years", roundingIncrement: 4, roundingMode: "halfExpand" }}), "P4Y");
check("since months increment 5", later.since(earlier, {{ smallestUnit: "months", roundingIncrement: 5 }}), "P2Y5M");
check("since pure months increment 10", later.since(earlier, {{ largestUnit: "months", smallestUnit: "months", roundingIncrement: 10 }}), "P30M");
check("until years increment 4", earlier.until(later, {{ smallestUnit: "years", roundingIncrement: 4, roundingMode: "halfExpand" }}), "P4Y");
check("until months increment 5", earlier.until(later, {{ smallestUnit: "months", roundingIncrement: 5 }}), "P2Y5M");
check("until pure months increment 10", earlier.until(later, {{ largestUnit: "months", smallestUnit: "months", roundingIncrement: 10 }}), "P30M");
// Rounding up past a whole year carries into the years.
check("carry into years", earlier.until(new YM(2020, 12), {{ smallestUnit: "months", roundingIncrement: 4, roundingMode: "expand" }}), "P2Y");
check("negative direction", later.until(earlier, {{ smallestUnit: "months", roundingIncrement: 5 }}), "-P2Y5M");
check("ceil vs floor", [
  earlier.until(later, {{ smallestUnit: "months", roundingIncrement: 5, roundingMode: "ceil" }}),
  earlier.until(later, {{ smallestUnit: "months", roundingIncrement: 5, roundingMode: "floor" }}),
  later.until(earlier, {{ smallestUnit: "months", roundingIncrement: 5, roundingMode: "ceil" }}),
  later.until(earlier, {{ smallestUnit: "months", roundingIncrement: 5, roundingMode: "floor" }}),
].join(), "P2Y10M,P2Y5M,-P2Y5M,-P2Y10M");
return done();
}})()
"#
    ));
}

/// `since`/`until/throws-if-rounded-date-outside-valid-iso-range.js`: the
/// rounding window's far end is `start + r2` units, which a huge increment sends
/// out of range.
#[test]
fn a_rounding_window_outside_the_valid_range_is_a_range_error() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const from = new YM(1970, 1);
const to = new YM(1971, 1);
const options = {{ roundingIncrement: 100_000_000 }};
expect("since", RangeError, () => from.since(to, options));
expect("until", RangeError, () => from.until(to, options));
// A smallestUnit of years with a huge increment too.
expect("years", RangeError, () => from.until(to, {{ smallestUnit: "years", roundingIncrement: 100_000_000 }}));
// An increment of 1 month needs no window at all, even at the edge of the range.
check("edge without rounding", new YM(275760, 8).until(new YM(275760, 9)), "P1M");
return done();
}})()
"#
    ));
}

/// The same arithmetic in a non-ISO calendar keeps its own month lengths in the
/// window: Hebrew years have 12 or 13 months.
#[test]
fn rounding_increments_use_the_calendars_own_month_counts() {
    assert_ok(&format!(
        r#"
(function() {{
{PRELUDE}
const start = Temporal.PlainYearMonth.from({{ year: 5783, monthCode: "M01", calendar: "hebrew" }}); // common year (12 months)
const end = Temporal.PlainYearMonth.from({{ year: 5785, monthCode: "M09", calendar: "hebrew" }});
// 5783 -> 5785 is two whole years (5784 is a 13-month leap year), then 8 more months.
check("unrounded", start.until(end), "P2Y8M");
check("months by increment 5", start.until(end, {{ smallestUnit: "months", roundingIncrement: 5 }}), "P2Y5M");
check("months by increment 5, ceil", start.until(end, {{ smallestUnit: "months", roundingIncrement: 5, roundingMode: "ceil" }}), "P2Y10M");
check("since mirrors until", end.since(start, {{ smallestUnit: "months", roundingIncrement: 5 }}), "P2Y5M");
// 8 is already a multiple of 4: nothing to round, even under `expand`.
check("exact multiple", start.until(end, {{ smallestUnit: "months", roundingIncrement: 4, roundingMode: "expand" }}), "P2Y8M");
// Expanding 8 to the next multiple of 7 gives 14 months, which is past the
// start of the third year: it becomes one more whole year.
check("carry into years", start.until(end, {{ smallestUnit: "months", roundingIncrement: 7, roundingMode: "expand" }}), "P3Y");
return done();
}})()
"#
    ));
}
