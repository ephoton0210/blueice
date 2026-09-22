// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_to_plain_month_day`
//! (`backend/bluejs/src/vm/temporal.rs`), the `ToTemporalMonthDay` string
//! branch. Pinned directly from Test262's
//! `built-ins/Temporal/PlainMonthDay/from/options-wrong-type.js`'s third
//! assertion.
//!
//! The real bug: the string branch validated the `options` argument (via
//! `temporal_options`/`temporal_overflow_option`) *before* parsing the
//! source string, so `Temporal.PlainMonthDay.from("1976-11-18Z", null)`
//! threw `TypeError` (invalid options) instead of `RangeError` (the `Z`
//! UTC designator is invalid on a wall-clock `PlainMonthDay` string) --
//! `ToTemporalMonthDay`'s real algorithm parses the string (step 4, throwing
//! `RangeError` for a malformed one) strictly before it ever reads the
//! `overflow` option (step 8).

use blueice_bluejs::{compile, parse, RuntimeError, Vm};

fn run(source: &str) -> Result<blueice_bluejs::Value, RuntimeError> {
    let program = compile(&parse(source).unwrap()).unwrap();
    Vm::default().execute(&program)
}

/// An invalid string throws `RangeError` regardless of how bad `options` is,
/// since string parsing happens first.
#[test]
fn invalid_string_throws_range_error_even_with_wrong_type_options() {
    for options_expr in ["null", "true", "\"some string\"", "Symbol()", "1", "2n"] {
        let source = format!(r#"Temporal.PlainMonthDay.from("1976-11-18Z", {options_expr})"#);
        match run(&source) {
            Err(RuntimeError::RangeError(_)) => {}
            other => panic!("{source}\n  -> expected RangeError, got: {other:?}"),
        }
    }
}

/// A valid string with wrong-type options still throws `TypeError` for the
/// options themselves, once parsing succeeds.
#[test]
fn valid_string_with_wrong_type_options_throws_type_error() {
    match run(r#"Temporal.PlainMonthDay.from("11-18", null)"#) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("expected TypeError, got: {other:?}"),
    }
}

/// A valid string with valid options still resolves correctly (no
/// regression in the ordinary path).
#[test]
fn valid_string_with_valid_options_resolves() {
    let result =
        run(r#"Temporal.PlainMonthDay.from("11-18", { overflow: "constrain" }).day"#).unwrap();
    assert_eq!(result, blueice_bluejs::Value::Number(18.0));
}
