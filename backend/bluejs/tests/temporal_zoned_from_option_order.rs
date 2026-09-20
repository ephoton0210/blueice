// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `ToTemporalZonedDateTime`'s observable protocol (Phase 26 Stage 3 --
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`), shared by
//! `ZonedDateTime.from` and the `equals`/`since`/`until`/`compare` argument conversions.
//!
//! - **Order.** The argument is read in full before `options` is touched. A property bag's fields
//!   are read alphabetically -- `offset` between `nanosecond` and `second`, `timeZone` between
//!   `second` and `year`, each converted the moment it is read -- and only then are `options`'
//!   `disambiguation`, `offset` and `overflow` read, in that order. A `ZonedDateTime` argument and
//!   a parsed string use the same option order. (Previously `overflow` came first, options were
//!   read *before* the fields, and `calendar` was read twice.)
//! - **Limits.** With `offset: "prefer"` or `"reject"` the wall-clock *date* must be within
//!   +/-10^8 days of the epoch (`InterpretISODateTimeOffset` step 6), so a string that names the
//!   minimum instant through an earlier wall-clock date is still a `RangeError`. `"use"` and
//!   `"ignore"` only need the instant itself to be in range.

use blueice_bluejs::{compile, parse, HeapConfig, RuntimeError, Value, Vm, VmConfig};

const PRELUDE: &str = r#"
const fails = [];
function nameOf(e) {
  if (e instanceof RangeError) return "RangeError";
  if (e instanceof TypeError) return "TypeError";
  if (e instanceof SyntaxError) return "SyntaxError";
  return "other:" + String(e);
}
function expect(kind, label, f) {
  let outcome = "none";
  try { f(); } catch (e) { outcome = nameOf(e); }
  if (outcome !== kind) fails.push(label + " -> " + outcome + " (expected " + kind + ")");
}
function same(label, actual, expected) {
  if (!Object.is(actual, expected)) fails.push(label + " => " + String(actual) + " !== " + String(expected));
}
function sameArray(label, actual, expected) {
  if (actual.length !== expected.length || actual.some((v, i) => v !== expected[i])) {
    fails.push(label + " => [" + actual.join(", ") + "] !== [" + expected.join(", ") + "]");
  }
}
// A bag whose every read returns a fresh converting object (like Test262's `propertyBagObserver`),
// logging each `get`, `toString` and `valueOf`. Property names in `plain` are returned as-is.
function observe(log, name, bag, plain = []) {
  return new Proxy(bag, {
    get(target, key) {
      if (typeof key === "symbol") return undefined;
      log.push("get " + name + "." + key);
      const value = target[key];
      if (value === undefined || plain.includes(key)) return value;
      return {
        toString() { log.push("call " + name + "." + key + ".toString"); return String(value); },
        valueOf() { log.push("call " + name + "." + key + ".valueOf"); return value; },
      };
    },
    has() { return true; },
  });
}
function gets(log) { return log.filter((entry) => entry.startsWith("get ") && entry.split(".").length === 2); }
const T = Temporal;
const FIELDS = { year: 2001, month: 5, monthCode: "M05", day: 2, hour: 6, minute: 54, second: 32,
                 millisecond: 987, microsecond: 654, nanosecond: 321, offset: "+00:00",
                 calendar: "iso8601", timeZone: "UTC" };
const FIELD_ORDER = ["calendar", "day", "hour", "microsecond", "millisecond", "minute", "month",
                     "monthCode", "nanosecond", "offset", "second", "timeZone", "year"];
const OPTION_ORDER = ["disambiguation", "offset", "overflow"];
function options(log) {
  return observe(log, "options", { overflow: "constrain", disambiguation: "compatible", offset: "reject", extra: "property" });
}
"#;

fn source(body: &str) -> String {
    format!(
        "(function() {{\n{PRELUDE}\n{body}\nreturn fails.length === 0 ? true : fails.join(\"\\n\");\n}})()"
    )
}

fn run(body: &str) {
    let program =
        compile(&parse(&source(body)).expect("test script parses")).expect("test script compiles");
    match Vm::default().execute(&program) {
        Ok(Value::Bool(true)) => {}
        Ok(Value::String(text)) => panic!("failing cases:\n{}", text.to_utf8().unwrap()),
        other => panic!("unexpected result: {other:?}"),
    }
}

/// Every allocation may collect, so an object-valued `Get` result that is not rooted across a
/// later allocation cannot survive by luck (see `temporal_property_bag_gc_rooting.rs`).
fn run_gc_stress(body: &str) {
    let program =
        compile(&parse(&source(body)).expect("test script parses")).expect("test script compiles");
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    match vm.execute(&program) {
        Ok(Value::Bool(true)) => {}
        Ok(Value::String(text)) => panic!("failing cases:\n{}", text.to_utf8().unwrap()),
        Err(RuntimeError::Heap(error)) => panic!("GC rooting bug: {error}"),
        other => panic!("unexpected result: {other:?}"),
    }
}

const ORDER_BODY: &str = r#"
  // `ZonedDateTime.from`: fields, then options.
  {
    const log = [];
    const item = observe(log, "item", FIELDS, ["calendar", "timeZone"]);
    T.ZonedDateTime.from(item, options(log));
    sameArray("from bag: item then options", gets(log),
      FIELD_ORDER.map((k) => "get item." + k).concat(OPTION_ORDER.map((k) => "get options." + k)));
  }
  // The conversions behind `equals` / `since` / `until` / `compare` read the fields and no options.
  const instance = new T.ZonedDateTime(988786472987654321n, "UTC");
  for (const [label, call] of [
    ["equals", (other) => instance.equals(other)],
    ["since", (other) => instance.since(other)],
    ["until", (other) => instance.until(other)],
    ["compare (first)", (other) => T.ZonedDateTime.compare(other, instance)],
    ["compare (second)", (other) => T.ZonedDateTime.compare(instance, other)],
  ]) {
    const log = [];
    call(observe(log, "other", FIELDS, ["calendar", "timeZone"]));
    sameArray(label + " bag reads", gets(log), FIELD_ORDER.map((k) => "get other." + k));
  }
  // A `ZonedDateTime` argument and a string read only the options, in the same order.
  for (const [label, item] of [["ZonedDateTime", new T.ZonedDateTime(0n, "UTC")],
                               ["string", "2001-05-02T06:54:32.987654321+00:00[UTC]"]]) {
    const log = [];
    T.ZonedDateTime.from(item, options(log));
    sameArray(label + " argument: options only", gets(log), OPTION_ORDER.map((k) => "get options." + k));
  }
  // The full call sequence (not just the `get`s) for a bag: each field is converted straight after
  // its own read, and each option too.
  {
    const log = [];
    T.ZonedDateTime.from(observe(log, "item", { year: 2001, month: 5, day: 2, offset: "+00:00", timeZone: "UTC" }, ["calendar", "timeZone"]), options(log));
    sameArray("interleaving", log, [
      "get item.calendar",
      "get item.day", "call item.day.valueOf",
      "get item.hour", "get item.microsecond", "get item.millisecond", "get item.minute",
      "get item.month", "call item.month.valueOf",
      "get item.monthCode", "get item.nanosecond",
      "get item.offset", "call item.offset.toString",
      "get item.second", "get item.timeZone",
      "get item.year", "call item.year.valueOf",
      "get options.disambiguation", "call options.disambiguation.toString",
      "get options.offset", "call options.offset.toString",
      "get options.overflow", "call options.overflow.toString",
    ]);
  }
  // A missing `timeZone` is a `TypeError`, but only once every field has been read.
  {
    const log = [];
    expect("TypeError", "missing timeZone", () => T.ZonedDateTime.from(observe(log, "item", { year: 2001, month: 5, day: 2 }, ["calendar", "timeZone"]), options(log)));
    sameArray("missing timeZone: fields read, options untouched", gets(log), FIELD_ORDER.map((k) => "get item." + k));
  }
  // A primitive `options` throws only after the bag has been read.
  {
    const log = [];
    expect("TypeError", "null options", () => T.ZonedDateTime.from(observe(log, "item", FIELDS, ["calendar", "timeZone"]), null));
    sameArray("null options: item fully read first", gets(log), FIELD_ORDER.map((k) => "get item." + k));
  }
  // `timeZone` and `offset` are converted where they are read: before a later field's TypeError.
  expect("RangeError", "invalid timeZone before year", () => T.ZonedDateTime.from({ year: Symbol(), month: 1, day: 1, timeZone: "Not/AZone" }));
  expect("RangeError", "malformed offset before year", () => T.ZonedDateTime.from({ year: Symbol(), month: 1, day: 1, timeZone: "UTC", offset: "--00:00" }));
  expect("TypeError", "unsuitable offset after year", () => T.ZonedDateTime.from({ year: Symbol(), month: 1, day: 1, timeZone: "UTC", offset: "+04:30" }));
  expect("TypeError", "non-string timeZone", () => T.ZonedDateTime.from({ year: 2001, month: 1, day: 1, timeZone: 5 }));
  // A monthCode that is well-formed but not an ISO month is judged after the options are read.
  {
    const log = [];
    expect("RangeError", "M08L", () => T.ZonedDateTime.from({ year: 2025, monthCode: "M08L", day: 14, timeZone: "UTC" }, options(log)));
    sameArray("M08L: options read before the calendar rejects the code", gets(log), OPTION_ORDER.map((k) => "get options." + k));
  }
  // The resolved value is right, too.
  {
    const zdt = T.ZonedDateTime.from({ year: 2001, month: 5, day: 2, hour: 6, minute: 54, second: 32, millisecond: 987, microsecond: 654, nanosecond: 321, offset: "+00:00", timeZone: "UTC" });
    same("epoch nanoseconds", zdt.epochNanoseconds, 988786472987654321n);
    same("toString", zdt.toString(), "2001-05-02T06:54:32.987654321+00:00[UTC]");
    same("offset ignored for a mismatch", T.ZonedDateTime.from({ year: 2001, month: 5, day: 2, timeZone: "UTC", offset: "+04:30" }, { offset: "ignore" }).offset, "+00:00");
    same("offset used", T.ZonedDateTime.from({ year: 2001, month: 5, day: 2, timeZone: "UTC", offset: "+04:30" }, { offset: "use" }).epochNanoseconds, 988745400000000000n);
    expect("RangeError", "offset reject", () => T.ZonedDateTime.from({ year: 2001, month: 5, day: 2, timeZone: "UTC", offset: "+04:30" }, { offset: "reject" }));
  }
"#;

/// `ZonedDateTime/from/order-of-operations.js` and the `equals`/`since`/`until`/`compare`
/// `order-of-operations.js` fixtures.
#[test]
fn the_argument_is_read_before_the_options_in_the_documented_order() {
    run(ORDER_BODY);
}

/// The same protocol with every allocation a collection point: a converting object returned by a
/// property read must stay reachable until its own `valueOf`/`toString` has run.
#[test]
fn the_argument_protocol_survives_a_collection_on_every_allocation() {
    run_gc_stress(ORDER_BODY);
}

/// `ZonedDateTime/{from,compare,prototype/{equals,since,until}}/argument-string-limits.js`.
#[test]
fn strings_at_the_edge_of_the_representable_range() {
    run(r#"
      const valid = ["-271821-04-20T00:00Z[UTC]", "-271821-04-19T23:00-01:00[-01:00]",
                     "-271821-04-19T00:01-23:59[-23:59]", "+275760-09-13T00:00Z[UTC]",
                     "+275760-09-13T01:00+01:00[+01:00]", "+275760-09-13T23:59+23:59[+23:59]"];
      // `use` and `ignore` need only the instant to be in range.
      for (const offset of ["use", "ignore"]) {
        for (const arg of valid) expect("none", offset + " " + arg, () => T.ZonedDateTime.from(arg, { offset }));
      }
      // `prefer` and `reject` also need the wall-clock *date* to be within 10^8 days.
      const validForPreferReject = valid.filter((arg) => !arg.startsWith("-271821-04-19"));
      const invalidForPreferReject = ["-271821-04-19T23:00-01:00[-01:00]", "-271821-04-19T00:00:01-23:59[-23:59]"];
      for (const offset of ["prefer", "reject"]) {
        for (const arg of validForPreferReject) expect("none", offset + " " + arg, () => T.ZonedDateTime.from(arg, { offset }));
        for (const arg of invalidForPreferReject) expect("RangeError", offset + " " + arg, () => T.ZonedDateTime.from(arg, { offset }));
      }
      // Outside the range whichever way the offset is treated.
      const invalid = ["-271821-04-19T23:59:59.999999999Z[UTC]", "-271821-04-19T23:00-00:59[-00:59]",
                       "-271821-04-19T00:00:00-23:59[-23:59]", "+275760-09-13T00:00:00.000000001Z[UTC]",
                       "+275760-09-13T01:00+00:59[+00:59]", "+275760-09-14T00:00+23:59[+23:59]"];
      for (const offset of ["use", "ignore", "prefer", "reject"]) {
        for (const arg of invalid) expect("RangeError", offset + " " + arg, () => T.ZonedDateTime.from(arg, { offset }));
      }
      // The other conversions use the default `reject`.
      const instance = new T.ZonedDateTime(0n, "UTC");
      for (const arg of invalidForPreferReject.concat(invalid)) {
        expect("RangeError", "compare " + arg, () => T.ZonedDateTime.compare(arg, instance));
        expect("RangeError", "compare (second) " + arg, () => T.ZonedDateTime.compare(instance, arg));
        expect("RangeError", "equals " + arg, () => instance.equals(arg));
        expect("RangeError", "since " + arg, () => instance.since(arg));
        expect("RangeError", "until " + arg, () => instance.until(arg));
      }
      for (const arg of validForPreferReject) {
        expect("none", "compare " + arg, () => T.ZonedDateTime.compare(arg, instance));
        expect("none", "equals " + arg, () => instance.equals(arg));
      }
    "#);
}

/// `ZonedDateTime/prototype/toString/options-read-before-algorithmic-validation.js`: a date
/// `smallestUnit` is a `RangeError`, but only after `timeZoneName` (the last option) was read.
#[test]
fn to_string_reads_every_option_before_validating_the_smallest_unit() {
    run(r#"
      const log = [];
      const bag = { calendarName: "always", timeZoneName: "always", smallestUnit: "month",
                    fractionalSecondDigits: "auto", roundingMode: "expand", offset: "auto" };
      expect("RangeError", "date smallestUnit", () => new T.ZonedDateTime(2n, "UTC").toString(observe(log, "options", bag)));
      sameArray("all six options read", gets(log), ["calendarName", "fractionalSecondDigits", "offset",
        "roundingMode", "smallestUnit", "timeZoneName"].map((k) => "get options." + k));
      same("valid smallestUnit", new T.ZonedDateTime(2n, "UTC").toString({ smallestUnit: "second", timeZoneName: "never" }), "1970-01-01T00:00:00+00:00");
    "#);
}
