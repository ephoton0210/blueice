// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the representable-range check every Temporal value
//! creation must make (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `CreateTemporalDate` throws a `RangeError` unless `ISODateWithinLimits`
//! holds (a date's *noon* is representable, so `-271821-04-19` .. `+275760-09-13`),
//! `CreateTemporalDateTime` unless `ISODateTimeWithinLimits` holds (one day
//! narrower at the low end: the earliest date-time is
//! `-271821-04-19T00:00:00.000000001`), and `CreateTemporalYearMonth` unless
//! `ISOYearMonthWithinLimits` holds. `Vm::alloc_temporal_value` -- the one
//! function every creation path ends in -- validated nothing, and the numeric
//! constructor only bounded each field on its own (a year in
//! `-271821..=275760`), so `new Temporal.PlainDate(275760, 9, 14)`,
//! `Temporal.PlainDate.from({ year: -271821, month: 4, day: 18 })`,
//! `min.toPlainDateTime(midnight)`, `minDateTime.with({ nanosecond: 0 })` and
//! `minDateTime.withPlainTime(midnight)` all produced out-of-range values.
//!
//! `PlainDateTime.prototype.toString` had the sibling bug: it rounds the value
//! (`RoundISODateTime`) and must throw if the *rounded* result leaves the range.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn evaluate_err(source: &str) -> RuntimeError {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .expect_err(&format!("{source}\n  -> expected an error, got a value"))
}

fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

fn assert_range_error(source: &str) {
    assert!(
        matches!(evaluate_err(source), RuntimeError::RangeError(_)),
        "{source}"
    );
}

fn assert_string(source: &str, expected: &str) {
    match evaluate(source) {
        Value::String(actual) => assert_eq!(actual.to_utf8().unwrap(), expected, "{source}"),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

/// `PlainDate/limits.js`: one day past either end throws, the ends construct.
#[test]
fn the_plain_date_constructor_enforces_the_representable_range() {
    assert_range_error("new Temporal.PlainDate(-271821, 4, 18)");
    assert_range_error("new Temporal.PlainDate(275760, 9, 14)");
    assert_string(
        "new Temporal.PlainDate(-271821, 4, 19).toString()",
        "-271821-04-19",
    );
    assert_string(
        "new Temporal.PlainDate(275760, 9, 13).toString()",
        "+275760-09-13",
    );
    // Every year field is in the per-field bound, so only the range check can reject these.
    assert_range_error("new Temporal.PlainDate(-271821, 1, 1)");
    assert_range_error("new Temporal.PlainDate(275760, 12, 31)");
}

/// `PlainDateTime/limits.js`: the low end is one day narrower than a `PlainDate`'s.
#[test]
fn the_plain_date_time_constructor_enforces_the_representable_range() {
    assert_range_error("new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 0)");
    assert_range_error("new Temporal.PlainDateTime(-271821, 4, 18, 23, 59, 59, 999, 999, 999)");
    assert_range_error("new Temporal.PlainDateTime(275760, 9, 14, 0, 0, 0, 0, 0, 0)");
    assert_string(
        "new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 1).toString()",
        "-271821-04-19T00:00:00.000000001",
    );
    assert_string(
        "new Temporal.PlainDateTime(275760, 9, 13, 23, 59, 59, 999, 999, 999).toString()",
        "+275760-09-13T23:59:59.999999999",
    );
}

/// `PlainYearMonth` / `PlainMonthDay`'s own creation limits stay enforced.
#[test]
fn the_year_month_constructor_enforces_its_range() {
    assert_range_error("new Temporal.PlainYearMonth(-271821, 3)");
    assert_range_error("new Temporal.PlainYearMonth(275760, 10)");
    assert_string(
        "new Temporal.PlainYearMonth(-271821, 4).toString()",
        "-271821-04",
    );
    assert_string(
        "new Temporal.PlainYearMonth(275760, 9).toString()",
        "+275760-09",
    );
}

/// The range check precedes reading `newTarget.prototype`
/// (`CreateTemporalDate` step 1, before `OrdinaryCreateFromConstructor`).
#[test]
fn the_range_error_precedes_the_new_target_prototype_lookup() {
    assert_true(
        r#"(function() {
          let read = false;
          const newTarget = new Proxy(function () {}, {
            get(target, key) { if (key === "prototype") read = true; return target[key]; },
          });
          try {
            Reflect.construct(Temporal.PlainDate, [275760, 9, 14], newTarget);
          } catch (error) {
            return error instanceof RangeError && read === false;
          }
          return false;
        })()"#,
    );
}

/// `PlainDate/from/limits.js` and `PlainDateTime/from/limits.js`: a property
/// bag is out of range whatever the overflow mode (`constrain` only regulates
/// the fields against their own calendar, never against the range).
#[test]
fn from_a_property_bag_enforces_the_representable_range() {
    for overflow in ["reject", "constrain"] {
        let options = format!(r#"{{ overflow: "{overflow}" }}"#);
        assert_range_error(&format!(
            "Temporal.PlainDate.from({{ year: -271821, month: 4, day: 18 }}, {options})"
        ));
        assert_range_error(&format!(
            "Temporal.PlainDate.from({{ year: 275760, month: 9, day: 14 }}, {options})"
        ));
        assert_range_error(&format!(
            "Temporal.PlainDateTime.from({{ year: -271821, month: 4, day: 19 }}, {options})"
        ));
        assert_range_error(&format!(
            "Temporal.PlainDateTime.from({{ year: 275760, month: 9, day: 14 }}, {options})"
        ));
    }
    assert_string(
        r#"Temporal.PlainDate.from({ year: -271821, month: 4, day: 19 }).toString()"#,
        "-271821-04-19",
    );
    assert_string(
        r#"Temporal.PlainDate.from({ year: 275760, month: 9, day: 13 }).toString()"#,
        "+275760-09-13",
    );
    assert_string(
        r#"Temporal.PlainDateTime.from({ year: -271821, month: 4, day: 19, nanosecond: 1 }).toString()"#,
        "-271821-04-19T00:00:00.000000001",
    );
    assert_string(
        r#"Temporal.PlainDateTime.from({ year: 275760, month: 9, day: 13, hour: 23, minute: 59,
             second: 59, millisecond: 999, microsecond: 999, nanosecond: 999 }).toString()"#,
        "+275760-09-13T23:59:59.999999999",
    );
}

/// The string path already checked the range; it must stay that way.
#[test]
fn from_a_string_enforces_the_representable_range() {
    for value in ["-271821-04-18", "+275760-09-14"] {
        assert_range_error(&format!(r#"Temporal.PlainDate.from("{value}")"#));
    }
    for value in ["-271821-04-19T00:00", "+275760-09-14T00:00"] {
        assert_range_error(&format!(r#"Temporal.PlainDateTime.from("{value}")"#));
    }
    assert_string(
        r#"Temporal.PlainDateTime.from("-271821-04-19T00:00:00.000000001").toString()"#,
        "-271821-04-19T00:00:00.000000001",
    );
}

/// `PlainDate/prototype/toPlainDateTime/limits.js`.
#[test]
fn to_plain_date_time_enforces_the_representable_range() {
    let prelude = r#"
        const midnight = new Temporal.PlainTime(0, 0);
        const firstNs = new Temporal.PlainTime(0, 0, 0, 0, 0, 1);
        const lastNs = new Temporal.PlainTime(23, 59, 59, 999, 999, 999);
        const min = new Temporal.PlainDate(-271821, 4, 19);
        const max = new Temporal.PlainDate(275760, 9, 13);
    "#;
    assert_range_error(&format!("{prelude} min.toPlainDateTime(midnight)"));
    assert_range_error(&format!("{prelude} min.toPlainDateTime()"));
    assert_string(
        &format!("{prelude} max.toPlainDateTime(midnight).toString()"),
        "+275760-09-13T00:00:00",
    );
    assert_string(
        &format!("{prelude} min.toPlainDateTime(firstNs).toString()"),
        "-271821-04-19T00:00:00.000000001",
    );
    assert_string(
        &format!("{prelude} max.toPlainDateTime(lastNs).toString()"),
        "+275760-09-13T23:59:59.999999999",
    );
}

/// `PlainDateTime/prototype/with/throws-if-combined-date-time-outside-valid-iso-range.js`
/// and `withPlainTime/throws-if-combined-date-time-outside-valid-iso-range.js`.
#[test]
fn with_and_with_plain_time_enforce_the_representable_range() {
    let min = "new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 1)";
    assert_range_error(&format!("{min}.with({{ nanosecond: 0 }})"));
    assert_range_error(&format!("{min}.withPlainTime(new Temporal.PlainTime())"));
    assert_range_error(&format!("{min}.withPlainTime()"));
    assert_string(
        &format!("{min}.with({{ nanosecond: 2 }}).toString()"),
        "-271821-04-19T00:00:00.000000002",
    );
    assert_string(
        &format!("{min}.withPlainTime(new Temporal.PlainTime(0, 0, 0, 0, 0, 5)).toString()"),
        "-271821-04-19T00:00:00.000000005",
    );
}

/// `PlainDateTime/prototype/toString/rounding-edge-of-range.js`: the *rounded*
/// value must still be representable.
#[test]
fn to_string_rounding_must_stay_within_the_representable_range() {
    assert_range_error(
        r#"new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 1)
             .toString({ smallestUnit: "second" })"#,
    );
    assert_range_error(
        r#"new Temporal.PlainDateTime(275760, 9, 13, 23, 59, 59, 999)
             .toString({ smallestUnit: "second", roundingMode: "halfExpand" })"#,
    );
    // Rounding that stays in range still works, including right at an edge.
    assert_string(
        r#"new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 1)
             .toString({ smallestUnit: "nanosecond" })"#,
        "-271821-04-19T00:00:00.000000001",
    );
    assert_string(
        r#"new Temporal.PlainDateTime(275760, 9, 13, 23, 59, 59, 999)
             .toString({ smallestUnit: "second", roundingMode: "floor" })"#,
        "+275760-09-13T23:59:59",
    );
    assert_string(
        r#"new Temporal.PlainDateTime(2000, 1, 1, 23, 59, 59, 999)
             .toString({ smallestUnit: "second", roundingMode: "halfExpand" })"#,
        "2000-01-02T00:00:00",
    );
}

/// `PlainYearMonth/prototype/toPlainDate/limits.js`: the constructed date must
/// be within `PlainDate`'s range even though the year-month is not.
#[test]
fn year_month_to_plain_date_enforces_the_representable_range() {
    let min = r#"Temporal.PlainYearMonth.from("-271821-04")"#;
    let max = r#"Temporal.PlainYearMonth.from("+275760-09")"#;
    assert_range_error(&format!("{min}.toPlainDate({{ day: 18 }})"));
    assert_string(
        &format!("{min}.toPlainDate({{ day: 19 }}).toString()"),
        "-271821-04-19",
    );
    assert_range_error(&format!("{max}.toPlainDate({{ day: 14 }})"));
    assert_string(
        &format!("{max}.toPlainDate({{ day: 13 }}).toString()"),
        "+275760-09-13",
    );
}

/// Arithmetic near the ends keeps reporting the ordinary `RangeError`.
#[test]
fn arithmetic_at_the_edges_is_a_range_error() {
    assert_range_error("new Temporal.PlainDate(275760, 9, 13).add({ days: 1 })");
    assert_range_error("new Temporal.PlainDate(-271821, 4, 19).subtract({ days: 1 })");
    assert_range_error(
        "new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 1).subtract({ nanoseconds: 1 })",
    );
    assert_string(
        "new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 2).subtract({ nanoseconds: 1 }).toString()",
        "-271821-04-19T00:00:00.000000001",
    );
}

/// The year-month range (`ISOYearMonthWithinLimits`) is checked at creation
/// too, for every route into a `PlainYearMonth`: a property bag with either
/// overflow mode, `with`, and `PlainDate.toPlainYearMonth`.
#[test]
fn year_month_creation_enforces_its_range_on_every_route() {
    for overflow in ["reject", "constrain"] {
        let options = format!(r#"{{ overflow: "{overflow}" }}"#);
        assert_range_error(&format!(
            "Temporal.PlainYearMonth.from({{ year: -271821, month: 3 }}, {options})"
        ));
        assert_range_error(&format!(
            "Temporal.PlainYearMonth.from({{ year: 275760, month: 10 }}, {options})"
        ));
    }
    assert_string(
        "Temporal.PlainYearMonth.from({ year: -271821, month: 4 }).toString()",
        "-271821-04",
    );
    assert_string(
        "Temporal.PlainYearMonth.from({ year: 275760, month: 9 }).toString()",
        "+275760-09",
    );
    assert_range_error("new Temporal.PlainYearMonth(275760, 9).with({ month: 10 })");
    assert_range_error("new Temporal.PlainYearMonth(-271821, 4).with({ month: 3 })");
    assert_string(
        "new Temporal.PlainDate(275760, 9, 13).toPlainYearMonth().toString()",
        "+275760-09",
    );
}
