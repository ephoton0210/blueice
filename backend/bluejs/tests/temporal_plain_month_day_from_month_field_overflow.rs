// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_plain_month_day_from_fields`
//! (`backend/bluejs/src/vm/temporal.rs`), pinned directly from Test262's
//! `built-ins/Temporal/PlainMonthDay/from/overflow.js`.
//!
//! The real bug: the property bag's `month` field was read with
//! `self.temporal_integer(&month, 1, 99, "month")` -- an artificially narrow
//! upper bound that threw `RangeError` at the field-reading stage for any
//! `month` above 99, before the calendar's own `overflow` regulation ever
//! ran. `ToPositiveIntegerWithTruncation` (`CalendarFields.cpp`'s
//! `CalendarField::MonthCode`/`Month` case) has **no** upper bound at all --
//! the same class of fix already applied to this function's own `day`
//! (`1..=i32::MAX`) and `year` (`i32::MIN..=i32::MAX`) fields, which `month`
//! was inconsistently left out of. `{ year: 2021, month: 999999, day: 500 }`
//! under the default `overflow: "constrain"` must succeed as `M12`/day 31 (a
//! huge ordinal month constrains to December, exactly like an
//! out-of-range `day`), not throw immediately.
//!
//! A second, related bug found while fixing the first: naively casting the
//! widened `i32` month down to the `u8` `ordinal_month`/regulation-month
//! parameter with a bare `as u8` truncates via wraparound (`999999 as u8`
//! happens to be `63`, which is coincidentally still `> 12` for this one
//! value, but is not a general-purpose fix) instead of saturating -- fixed
//! by clamping to `u8::MAX` before the cast, so any out-of-range month stays
//! unambiguously out of range for the downstream `clamp(1, 12)`/`reject`
//! check regardless of exactly how large the original value was.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// The fixture's own headline case: a huge ordinal `month` constrains to
/// December rather than throwing at the field-read stage.
#[test]
fn huge_ordinal_month_constrains_to_december() {
    assert_true(
        r#"
        const r = Temporal.PlainMonthDay.from({ year: 2021, month: 999999, day: 500 }, { overflow: "constrain" });
        r.monthCode === "M12" && r.day === 31
    "#,
    );
}

/// An even larger month value (beyond `u8::MAX`) must still constrain
/// correctly rather than wrapping around to a small, spuriously in-range
/// value.
#[test]
fn a_month_value_beyond_u8_max_still_constrains_to_december() {
    assert_true(
        r#"
        const r = Temporal.PlainMonthDay.from({ year: 2021, month: 100000000, day: 1 }, { overflow: "constrain" });
        r.monthCode === "M12"
    "#,
    );
}

/// The same huge month under `overflow: "reject"` still throws `RangeError`
/// (never silently succeeds because of a wraparound coincidence).
#[test]
fn a_month_value_beyond_u8_max_still_rejects_under_overflow_reject() {
    let program = compile(
        &parse(
            r#"Temporal.PlainMonthDay.from({ year: 2021, month: 100000000, day: 1 }, { overflow: "reject" })"#,
        )
        .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("expected RangeError, got: {other:?}"),
    }
}

/// A month of `0` or negative is still rejected immediately, even under
/// `overflow: "constrain"` -- this fix must not weaken that existing check
/// (a month must be *positive*; only the missing upper bound was the bug).
#[test]
fn zero_or_negative_month_is_still_rejected_even_with_constrain() {
    for month in ["-99999", "-1", "0"] {
        let source = format!(
            r#"Temporal.PlainMonthDay.from({{ year: 2021, month: {month}, day: 1 }}, {{ overflow: "constrain" }})"#
        );
        let program = compile(&parse(&source).unwrap()).unwrap();
        match Vm::default().execute(&program) {
            Err(RuntimeError::RangeError(_)) => {}
            other => panic!("{source}\n  -> expected RangeError, got: {other:?}"),
        }
    }
}
