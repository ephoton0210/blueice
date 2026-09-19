// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_month_day_to_plain_date`
//! (`backend/bluejs/src/vm/temporal.rs`), pinned directly from Test262's
//! `built-ins/Temporal/PlainMonthDay/prototype/toPlainDate/limits.js`.
//!
//! Two real bugs, the same class already fixed elsewhere in this file for
//! other fields:
//!
//! 1. The `item.year` field was read with an artificially narrow bound
//!    (`-9_999..=9_999`) instead of Temporal's own unbounded field read --
//!    `toPlainDate({ year: -271821 })` (the true representable minimum
//!    year) threw immediately at the field-read stage instead of ever
//!    reaching real date resolution.
//! 2. For the `iso8601` calendar, resolution always routed through
//!    `icu_calendar::Date::try_from_fields`, whose own internal
//!    `CONSTRUCTOR_YEAR_RANGE` (`-9999..=9999`) is far narrower than
//!    Temporal's real range (`-271821-04-19` to `+275760-09-13`) -- the same
//!    "Calendar year-range getter bug" class this document's Stage 0 audit
//!    already fixed for `temporal_calendar_fields`'s getters, but not yet
//!    for this specific `toPlainDate` merge. Fixed with a dedicated
//!    `iso8601` fast path using `plain_date::regulate_iso_date` (pure Rust
//!    arithmetic, no such limit) plus a real
//!    `epoch::is_date_within_limits` check on the *resolved* date -- e.g.
//!    `jan1.toPlainDate({ year: -271821 })` must throw (one day before the
//!    true minimum), while `PlainMonthDay.from("04-19").toPlainDate({ year:
//!    -271821 })` must succeed (exactly the minimum).

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

fn assert_range_error(source: &str) {
    match run(source) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("{source}\n  -> expected RangeError, got: {other:?}"),
    }
}

/// A `year` one day before the true minimum throws, exactly at the minimum
/// succeeds.
#[test]
fn minimum_boundary_is_a_day_and_month_boundary_not_a_year_one() {
    assert_range_error(
        r#"Temporal.PlainMonthDay.from("01-01").toPlainDate({ year: -271821 })"#,
    );
    assert_range_error(
        r#"Temporal.PlainMonthDay.from("04-18").toPlainDate({ year: -271821 })"#,
    );
    assert_true(
        r#"
        const r = Temporal.PlainMonthDay.from("04-19").toPlainDate({ year: -271821 });
        r.year === -271821 && r.month === 4 && r.day === 19
    "#,
    );
    assert_true(
        r#"
        const r = Temporal.PlainMonthDay.from("01-01").toPlainDate({ year: -271820 });
        r.year === -271820 && r.month === 1 && r.day === 1
    "#,
    );
}

/// A `year` one day past the true maximum throws, exactly at the maximum
/// succeeds.
#[test]
fn maximum_boundary_is_a_day_and_month_boundary_not_a_year_one() {
    assert_range_error(
        r#"Temporal.PlainMonthDay.from("12-31").toPlainDate({ year: 275760 })"#,
    );
    assert_range_error(
        r#"Temporal.PlainMonthDay.from("09-14").toPlainDate({ year: 275760 })"#,
    );
    assert_true(
        r#"
        const r = Temporal.PlainMonthDay.from("09-13").toPlainDate({ year: 275760 });
        r.year === 275760 && r.month === 9 && r.day === 13
    "#,
    );
    assert_true(
        r#"
        const r = Temporal.PlainMonthDay.from("12-31").toPlainDate({ year: 275759 });
        r.year === 275759 && r.month === 12 && r.day === 31
    "#,
    );
}

/// An ordinary in-range year is unaffected by these fixes.
#[test]
fn ordinary_year_still_resolves() {
    assert_true(
        r#"
        const r = Temporal.PlainMonthDay.from("06-15").toPlainDate({ year: 2020 });
        r.year === 2020 && r.month === 6 && r.day === 15
    "#,
    );
}
