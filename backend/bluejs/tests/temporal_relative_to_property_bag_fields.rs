// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the `relativeTo` property-bag reader shared by
//! `Temporal.Duration.prototype.{round,total}` and `Temporal.Duration.compare`.
//!
//! Every other Temporal property-bag reader takes `monthCode` through
//! `ToMonthCode` (a `String` is required and its syntax is checked when it is
//! read) and reads `month` as an unbounded positive integer, leaving
//! `overflow` to decide between constraining and rejecting. The `relativeTo`
//! reader still stringified `monthCode` with `ToString`, so `monthCode: 5`
//! surfaced as a `RangeError` for the wrong reason, and it capped `month` at
//! 99, so `month: 133` threw even though `relativeTo` resolves with
//! `constrain`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn run(source: &str) -> Result<Value, RuntimeError> {
    let program = compile(&parse(source).unwrap()).unwrap();
    Vm::default().execute(&program)
}

fn round_with(relative_to: &str) -> String {
    format!(
        "new Temporal.Duration(0, 0, 0, 1).round({{ smallestUnit: \"days\", relativeTo: {relative_to} }}).days"
    )
}

/// A non-`String` primitive `monthCode` is a `TypeError`, never a coerced
/// string.
#[test]
fn a_non_string_primitive_month_code_is_a_type_error() {
    for value in ["5", "1n", "true", "null"] {
        let source = round_with(&format!("{{ year: 2020, monthCode: {value}, day: 1 }}"));
        assert!(
            matches!(run(&source), Err(RuntimeError::TypeError(_))),
            "monthCode: {value} -> {:?}",
            run(&source)
        );
    }
}

/// A `monthCode` whose syntax is wrong is a `RangeError`.
#[test]
fn a_malformed_month_code_is_a_range_error() {
    // `{}` is not a `TypeError`: `ToPrimitive` turns it into the string
    // "[object Object]", which then fails the syntax check.
    for value in ["\"m05\"", "\"M5\"", "\"M13x\"", "\"\"", "{}"] {
        let source = round_with(&format!("{{ year: 2020, monthCode: {value}, day: 1 }}"));
        assert!(
            matches!(run(&source), Err(RuntimeError::RangeError(_))),
            "monthCode: {value} -> {:?}",
            run(&source)
        );
    }
}

/// An object `monthCode` goes through `ToPrimitive` with a string hint, so
/// its own `toString` is called and the resulting string is accepted.
#[test]
fn an_object_month_code_is_converted_through_its_to_string() {
    let source =
        round_with("{ year: 2020, monthCode: { toString() { return \"M05\"; } }, day: 1 }");
    assert_eq!(run(&source).unwrap(), Value::Number(1.0));
}

/// A well-formed `monthCode` still resolves.
#[test]
fn a_well_formed_month_code_still_resolves() {
    let source = round_with("{ year: 2020, monthCode: \"M05\", day: 1 }");
    assert_eq!(run(&source).unwrap(), Value::Number(1.0));
}

/// `month` has no upper bound at read time: `relativeTo` resolves with
/// `constrain`, which clamps an over-large month to the year's last one.
#[test]
fn an_oversized_month_is_constrained_not_rejected() {
    let source = round_with("{ year: 2020, month: 133, day: 1 }");
    assert_eq!(run(&source).unwrap(), Value::Number(1.0));
    // A month past `u8::MAX` must not wrap around to a small valid month.
    let source = round_with("{ year: 2020, month: 257, day: 1 }");
    assert_eq!(run(&source).unwrap(), Value::Number(1.0));
}

/// The clamped month really is the last one: one calendar month from
/// December 1st spans 31 days, not the 30 of a wrapped-around month.
#[test]
fn an_oversized_month_clamps_to_december() {
    let source = "new Temporal.Duration(0, 1).total({ unit: \"days\", \
                  relativeTo: { year: 2020, month: 999999, day: 1 } })";
    assert_eq!(run(source).unwrap(), Value::Number(31.0));
}
