// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real bug in `Vm::temporal_date_add`
//! (`Temporal.PlainDate`/`PlainDateTime.prototype.add`/`subtract`,
//! `backend/bluejs/src/vm/temporal.rs`) — Phase 26 Stage 2, second slice
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `temporal_date_add` computed the result via `plain_date::calendar_add_date`
//! and constructed the resulting `TemporalValue` without ever checking it
//! against Temporal's own exact representable range
//! (`-271821-04-19`..`+275760-09-13`, exclusive at the day-and-nanosecond
//! boundary for a `PlainDateTime`) -- `calendar_add_date`'s own
//! `regulate_iso_date`/`balance_iso_date` only range-check that the landing
//! year fits an `i32` and the day fits its month, which is a much wider
//! range. Confirmed directly by Test262's `add/limits.js`: subtracting one
//! day from the exact minimum `Temporal.PlainDate` silently produced a
//! valid-but-unrepresentable `-271821-04-18` instead of throwing
//! `RangeError`. `alloc_temporal_value` performs no range validation of its
//! own -- the same gap `Temporal.PlainDateTime.prototype.round` had (see
//! `temporal_plain_date_time_round_day_unit.rs`), fixed here for `add`/
//! `subtract` the same way: an explicit `epoch::is_date_within_limits`/
//! `is_date_time_within_limits` check after computing the result.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate_err(source: &str) -> RuntimeError {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .expect_err(&format!("{source}\n  -> expected an error, got a value"))
}

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `add/limits.js`'s own four cases: subtracting a day from the minimum, or
/// adding a day to the maximum, must throw `RangeError` regardless of the
/// `overflow` option (arithmetic that crosses the representable boundary is
/// never merely constrained).
#[test]
fn add_throws_when_crossing_the_representable_range_boundary() {
    for overflow in ["reject", "constrain"] {
        let source = format!(
            r#"Temporal.PlainDate.from("-271821-04-19").add({{ days: -1 }}, {{ overflow: "{overflow}" }})"#
        );
        let error = evaluate_err(&source);
        assert!(
            matches!(error, RuntimeError::RangeError(_)),
            "{overflow}: {error:?}"
        );

        let source = format!(
            r#"Temporal.PlainDate.from("+275760-09-13").add({{ days: 1 }}, {{ overflow: "{overflow}" }})"#
        );
        let error = evaluate_err(&source);
        assert!(
            matches!(error, RuntimeError::RangeError(_)),
            "{overflow}: {error:?}"
        );
    }
}

/// A representable in-range date one day away from each boundary must still
/// succeed -- this fix's own check must not be off-by-one in the other
/// direction.
#[test]
fn add_still_succeeds_one_day_inside_each_boundary() {
    assert_true(
        r#"
        (function() {
          const min = Temporal.PlainDate.from("-271821-04-20").add({ days: -1 });
          const max = Temporal.PlainDate.from("+275760-09-12").add({ days: 1 });
          return min.toString() === "-271821-04-19" && max.toString() === "+275760-09-13";
        })()
        "#,
    );
}
