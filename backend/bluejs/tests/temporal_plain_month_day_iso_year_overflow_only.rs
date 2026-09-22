// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for
//! `built-ins/Temporal/PlainMonthDay/{from,prototype/with}/
//! iso-year-used-only-for-overflow.js`: ported directly from Gecko's own
//! ISO-specific branch of `CalendarMonthDayFromFields`
//! (`development/browser_core/reference/gecko/js/src/builtin/temporal/Calendar.cpp`,
//! around its `ISOMonthDayFromFields`-equivalent): a supplied `year` is used
//! **only** to regulate the resolved `day` against that year's own
//! leap-year-ness (e.g. is 29 February valid), but the `iso8601` calendar's
//! `PlainMonthDay` result always reports the fixed reference year `1972` --
//! never the supplied year, however large or small.
//!
//! Before this fix, both `temporal_plain_month_day_from_fields` and
//! `temporal_month_day_with` (`backend/bluejs/src/vm/temporal.rs`) passed
//! the supplied `year` straight through to `icu_calendar`, which returned
//! *that* year in its result rather than always reporting `1972` --
//! observable via `PlainMonthDay.prototype.toString({ calendarName: "always" })`,
//! whose date portion carries the internal reference year (per
//! `TemporalHelpers.assertPlainMonthDay`'s own `referenceISOYear` check).

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

fn reference_year_check(constructor_call: &str) -> String {
    format!(
        r#"
        const result = {constructor_call};
        const isoYear = Number(result.toString({{ calendarName: "always" }}).split("-")[0]);
        isoYear === 1972
    "#
    )
}

/// `from()`'s own four cases: an out-of-range common year still reports
/// 1972 while correctly constraining 29 February; a genuinely out-of-range
/// leap year still throws under `overflow: "reject"`; an out-of-range leap
/// year keeps 29 February valid.
#[test]
fn from_reports_the_fixed_reference_year_regardless_of_the_supplied_year() {
    assert_true(&reference_year_check(
        r#"Temporal.PlainMonthDay.from({ year: -999999, month: 1, day: 1 })"#,
    ));
    assert_true(&reference_year_check(
        r#"Temporal.PlainMonthDay.from({ year: -999999, monthCode: "M02", day: 29 })"#,
    ));
    assert_true(
        r#"
        const commonResult = Temporal.PlainMonthDay.from({ year: -999999, monthCode: "M02", day: 29 });
        commonResult.day === 28
    "#,
    );
    assert_true(&reference_year_check(
        r#"Temporal.PlainMonthDay.from({ year: -1000000, monthCode: "M02", day: 29 })"#,
    ));
    assert_true(
        r#"
        const leapResult = Temporal.PlainMonthDay.from({ year: -1000000, monthCode: "M02", day: 29 });
        leapResult.day === 29
    "#,
    );
}

/// `from()` still rejects under `overflow: "reject"` when the supplied
/// (out-of-range) year is a common year and the day genuinely overflows.
#[test]
fn from_still_rejects_under_overflow_reject() {
    let program = compile(
        &parse(
            r#"
        Temporal.PlainMonthDay.from(
            { year: -999999, monthCode: "M02", day: 29 },
            { overflow: "reject" }
        );
    "#,
        )
        .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("expected RangeError, got {other:?}"),
    }
}

/// `with()`'s own three cases, mirroring `from()`'s.
#[test]
fn with_reports_the_fixed_reference_year_regardless_of_the_supplied_year() {
    assert_true(&reference_year_check(
        r#"new Temporal.PlainMonthDay(1, 1, "iso8601", 1972).with({ year: -999999 })"#,
    ));
    assert_true(
        r#"
        const leap = new Temporal.PlainMonthDay(2, 29, "iso8601", 1972);
        const commonResult = leap.with({ year: -999999 });
        commonResult.day === 28
    "#,
    );
    assert_true(
        r#"
        const leap = new Temporal.PlainMonthDay(2, 29, "iso8601", 1972);
        const leapResult = leap.with({ year: -1000000 });
        leapResult.day === 29
    "#,
    );
}

/// `with()` still rejects under `overflow: "reject"`.
#[test]
fn with_still_rejects_under_overflow_reject() {
    let program = compile(
        &parse(
            r#"
        const leap = new Temporal.PlainMonthDay(2, 29, "iso8601", 1972);
        leap.with({ year: -999999 }, { overflow: "reject" });
    "#,
        )
        .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("expected RangeError, got {other:?}"),
    }
}
