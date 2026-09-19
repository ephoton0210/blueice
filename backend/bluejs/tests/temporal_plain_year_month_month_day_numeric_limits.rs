// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for two real gaps in `temporal_value_from_args`'s
//! `PlainYearMonth`/`PlainMonthDay` numeric-constructor arms
//! (`backend/bluejs/src/vm/temporal.rs`): both used a coarse per-field
//! `-271_821..=275_760` year bound, which is looser than Temporal's true
//! representable-range boundary -- a *month* boundary for `PlainYearMonth`
//! (`-271821-04` is the true minimum, not any month of `-271821`) and a
//! *day* boundary for `PlainMonthDay`'s `referenceISODay` argument
//! (`+275760-09-13` is the true maximum instant, so `+275760-09-14` must
//! still throw even though every individual field -- year, month, day --
//! is itself within its own coarse bound).
//!
//! Pinned directly from `built-ins/Temporal/PlainYearMonth/limits.js` and
//! `built-ins/Temporal/PlainMonthDay/refisoyear-out-of-range.js`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_range_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("{source}\n  -> expected RangeError, got {other:?}"),
    }
}

/// The fixture's own two out-of-range months, one per boundary year.
#[test]
fn year_month_rejects_a_month_before_or_after_the_true_boundary() {
    assert_range_error("new Temporal.PlainYearMonth(-271821, 3);");
    assert_range_error("new Temporal.PlainYearMonth(275760, 10);");
}

/// The exact boundary months still construct, with or without an explicit
/// `referenceISODay` (which must not affect year-month validity).
#[test]
fn year_month_accepts_the_true_boundary_months() {
    assert_eq!(
        evaluate("new Temporal.PlainYearMonth(-271821, 4).month"),
        Value::Number(4.0)
    );
    assert_eq!(
        evaluate(r#"new Temporal.PlainYearMonth(-271821, 4, "iso8601", 18).month"#),
        Value::Number(4.0)
    );
    assert_eq!(
        evaluate("new Temporal.PlainYearMonth(275760, 9).month"),
        Value::Number(9.0)
    );
    assert_eq!(
        evaluate(r#"new Temporal.PlainYearMonth(275760, 9, "iso8601", 14).month"#),
        Value::Number(9.0)
    );
}

/// The fixture's own two out-of-range `referenceISODay` combinations.
#[test]
fn month_day_rejects_a_reference_year_that_pushes_the_iso_date_out_of_range() {
    assert_range_error(r#"new Temporal.PlainMonthDay(9, 14, "iso8601", 275760);"#);
    assert_range_error(r#"new Temporal.PlainMonthDay(4, 18, "iso8601", -271821);"#);
}
