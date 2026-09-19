// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real, previously-unwired gap: per Gecko's own
//! `PlainYearMonth.cpp` `PlainYearMonth_prototype_properties` table
//! (`development/browser_core/reference/gecko/js/src/builtin/temporal/PlainYearMonth.cpp`),
//! `Temporal.PlainYearMonth.prototype` has `daysInYear`/`daysInMonth`/
//! `inLeapYear` getters alongside `monthsInYear` -- but
//! `backend/bluejs/src/vm/temporal.rs`'s `PlainYearMonth` getter
//! installation table only had `monthsInYear`, and the `temporal_getter`
//! dispatch's `DaysInMonth`/`DaysInYear`/`InLeapYear` arms explicitly
//! restricted themselves to `PlainDate | PlainDateTime`, so the three
//! getters were entirely absent from `PlainYearMonth.prototype` -- not
//! merely wrong, unset. `temporal_calendar_fields` itself already computes
//! these fields correctly for any `TemporalValue` regardless of kind
//! (confirmed by `PlainDate`/`PlainDateTime` already using them), so this is
//! a narrow "wire it up" gap, not a missing algorithm.
//!
//! Expected values pinned directly from the real Test262 fixtures:
//! `built-ins/Temporal/PlainYearMonth/prototype/{daysInMonth,daysInYear,
//! inLeapYear}/basic.js`.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `PlainYearMonth/prototype/daysInMonth/basic.js`'s own four cases.
#[test]
fn plain_year_month_days_in_month_matches_the_pinned_fixture() {
    assert_true("(new Temporal.PlainYearMonth(1976, 2)).daysInMonth === 29");
    assert_true("(new Temporal.PlainYearMonth(1976, 11)).daysInMonth === 30");
    assert_true("(new Temporal.PlainYearMonth(1976, 12)).daysInMonth === 31");
    assert_true("(new Temporal.PlainYearMonth(1977, 2)).daysInMonth === 28");
}

/// `PlainYearMonth/prototype/daysInYear/basic.js`'s own two cases.
#[test]
fn plain_year_month_days_in_year_matches_the_pinned_fixture() {
    assert_true("(new Temporal.PlainYearMonth(1976, 11)).daysInYear === 366");
    assert_true("(new Temporal.PlainYearMonth(1977, 11)).daysInYear === 365");
}

/// `PlainYearMonth/prototype/inLeapYear/basic.js`'s own two cases.
#[test]
fn plain_year_month_in_leap_year_matches_the_pinned_fixture() {
    assert_true("(new Temporal.PlainYearMonth(1976, 11)).inLeapYear === true");
    assert_true("(new Temporal.PlainYearMonth(1977, 11)).inLeapYear === false");
}
