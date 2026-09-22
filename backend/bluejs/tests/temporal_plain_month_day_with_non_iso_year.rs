// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for
//! `intl402/Temporal/PlainMonthDay/prototype/with/fields-missing-properties.js`:
//! `Temporal.PlainMonthDay.prototype.with` on a non-`iso8601` calendar must
//! throw `TypeError` for a bare ordinal `{ month }` override with no
//! `year`, because the receiver itself has no `year` field to merge in
//! (`Temporal.PlainMonthDay.prototype` has no `year`/`month` getter at
//! all -- see `ISODateToFields(calendar, isoDate, MONTH-DAY)` in the spec,
//! whose field set for this type is only `monthCode`/`day`).
//!
//! Before this fix, `temporal_month_day_with`
//! (`backend/bluejs/src/vm/temporal.rs`) always fell back to the receiver's
//! own *derived* calendar year (`temporal_calendar_fields`'s `base.year`)
//! regardless of calendar, so `with({ month: 12 })` silently succeeded
//! instead of throwing.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// The fixture's own case.
#[test]
fn bare_ordinal_month_without_year_throws_for_a_non_iso_calendar() {
    let program = compile(
        &parse(
            r#"
        const calendarMonthDay = Temporal.PlainMonthDay.from(
            { year: 2021, month: 1, day: 15, calendar: "gregory" }
        );
        calendarMonthDay.with({ month: 12 });
    "#,
        )
        .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("expected TypeError, got {other:?}"),
    }
}

/// An explicit `year` alongside the ordinal `month` is legitimate input and
/// must still succeed.
#[test]
fn bare_ordinal_month_with_an_explicit_year_still_succeeds() {
    assert_true(
        r#"
        const calendarMonthDay = Temporal.PlainMonthDay.from(
            { year: 2021, month: 1, day: 15, calendar: "gregory" }
        );
        calendarMonthDay.with({ month: 12, year: 2021 }).monthCode === "M12"
    "#,
    );
}

/// A `monthCode`-only override (no ordinal `month`, no `year`) must still
/// succeed for a non-ISO calendar -- `icu_calendar`'s own reference-year
/// derivation covers that case, unaffected by this fix.
#[test]
fn month_code_only_override_still_succeeds_without_a_year() {
    assert_true(
        r#"
        const calendarMonthDay = Temporal.PlainMonthDay.from(
            { year: 2021, month: 1, day: 15, calendar: "gregory" }
        );
        calendarMonthDay.with({ monthCode: "M12" }).monthCode === "M12"
    "#,
    );
}

/// A bare ordinal `month` on the ISO calendar is unaffected -- the fixed
/// 1972 reference year always resolves it.
#[test]
fn bare_ordinal_month_on_iso_calendar_is_unaffected() {
    assert_true(
        r#"
        const md = new Temporal.PlainMonthDay(1, 15);
        md.with({ month: 12 }).monthCode === "M12"
    "#,
    );
}
