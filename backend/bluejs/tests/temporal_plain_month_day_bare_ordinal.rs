// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real bug pinned by the Test262 fixture
//! `built-ins/Temporal/PlainMonthDay/prototype/equals/basic.js`:
//! `Vm::temporal_plain_month_day_from_fields`
//! (`backend/bluejs/src/vm/temporal.rs`) unconditionally required either a
//! `monthCode` or a `year` field, on the theory that an ordinal `month`
//! alone can never resolve a reference year (true for a general non-ISO
//! calendar, where a leap-month calendar's ordinal-month identity varies by
//! year). But for the `iso8601` calendar specifically, ordinal `month` and
//! `monthCode` always correspond 1:1 (`month: 1` is always `"M01"`), so a
//! bare `{ month, day }` property bag -- no `monthCode`, no `year` -- is
//! spec-valid and must resolve against the fixed ISO reference year (1972),
//! exactly as `Temporal.PlainMonthDay.from("01-22")`'s own short string form
//! already does.
//!
//! Fixed by requiring `monthCode`/`year` only when the calendar is not
//! `iso8601`, and by synthesizing the equivalent `monthCode` from a bare
//! ordinal `month` for the `iso8601` case before calling
//! `plain_month_day::month_day_from_fields` -- `icu_calendar`'s own
//! `MissingFieldsStrategy::Ecma` reference-year derivation only fires from a
//! `monthCode`+`day` pair, never a bare ordinal `month`+`day`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// The fixture's own case: a bare ordinal `{ month, day }` property bag
/// resolves against the ISO calendar's fixed 1972 reference year.
#[test]
fn bare_ordinal_month_and_day_resolve_for_the_iso_calendar() {
    assert_true(
        r#"
        const md1 = Temporal.PlainMonthDay.from("01-22");
        md1.equals({ month: 1, day: 22 }) === true
    "#,
    );
    assert_true(
        r#"
        const md2 = Temporal.PlainMonthDay.from("12-15");
        md2.equals({ month: 1, day: 22 }) === false
    "#,
    );
}

/// A non-ISO calendar still requires `monthCode` or `year` alongside a bare
/// ordinal `month` -- this fix must not weaken that check.
#[test]
fn non_iso_calendar_still_requires_month_code_or_year() {
    let program = compile(
        &parse(r#"Temporal.PlainMonthDay.from({ month: 1, day: 1, calendar: "hebrew" });"#)
            .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("expected TypeError, got {other:?}"),
    }
}
