// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_month_day_with`
//! (`backend/bluejs/src/vm/temporal.rs`), pinned directly from Test262's
//! `built-ins/Temporal/PlainMonthDay/prototype/with/basic.js` and
//! `.../with/options-wrong-type.js`.
//!
//! Two real bugs, the `with()` counterparts of the identical ones already
//! fixed in `temporal_plain_month_day_from_fields`:
//!
//! 1. A `monthCode` supplied *alongside* a numeric `month` was never
//!    cross-checked for agreement -- `with({ month: 12, monthCode: "M11" })`
//!    silently used `month` and ignored the conflicting `monthCode` instead
//!    of throwing `RangeError`.
//! 2. `options` was validated (`GetOptionsObject`/`GetTemporalOverflowOption`)
//!    *before* the property-bag fields were read and coerced, so a wrong-type
//!    `options` argument threw `TypeError` even when a field itself
//!    (`{ day: -1 }`) was already invalid and should have reported
//!    `RangeError` first -- `PrepareCalendarFields` runs strictly before
//!    `GetOptionsObject` in the real algorithm.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn run(source: &str) -> Result<Value, RuntimeError> {
    let program = compile(&parse(source).unwrap()).unwrap();
    Vm::default().execute(&program)
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `with({ month, monthCode })` that agree still resolves; that disagree is
/// a `RangeError`.
#[test]
fn with_month_and_month_code_conflict_throws_range_error() {
    assert_true(
        r#"
        const md = Temporal.PlainMonthDay.from("01-15");
        const r = md.with({ month: 12, monthCode: "M12" });
        r.monthCode === "M12" && r.day === 15
    "#,
    );
    match run(r#"Temporal.PlainMonthDay.from("01-15").with({ month: 12, monthCode: "M11" })"#) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("expected RangeError, got: {other:?}"),
    }
}

/// An invalid field (`day: -1`) is processed and throws `RangeError` before
/// a wrong-type `options` argument's own `TypeError` would otherwise fire.
#[test]
fn invalid_field_throws_range_error_even_with_wrong_type_options() {
    for options_expr in ["null", "true", "\"some string\"", "Symbol()", "1", "2n"] {
        let source = format!(
            r#"Temporal.PlainMonthDay.from("01-15").with({{ day: -1 }}, {options_expr})"#
        );
        match run(&source) {
            Err(RuntimeError::RangeError(_)) => {}
            other => panic!("{source}\n  -> expected RangeError, got: {other:?}"),
        }
    }
}

/// A valid field with wrong-type options still throws `TypeError` for the
/// options themselves, once field processing succeeds.
#[test]
fn valid_field_with_wrong_type_options_throws_type_error() {
    match run(r#"Temporal.PlainMonthDay.from("01-15").with({ day: 5 }, null)"#) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("expected TypeError, got: {other:?}"),
    }
}
