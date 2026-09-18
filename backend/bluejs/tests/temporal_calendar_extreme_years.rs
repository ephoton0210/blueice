// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the ISO-calendar getter bug documented in
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`'s Stage 0
//! "Deliberately left alone" list: `icu_calendar`'s `Date::try_new_iso`
//! enforces its own internal `CONSTRUCTOR_YEAR_RANGE` (-9999..=9999, far
//! narrower than Temporal's own representable range, roughly ±271,821
//! years), so an in-range extreme-year Temporal value *constructed*
//! successfully but its `.year`/`.month`/`.monthCode`/`.day` getters threw
//! a spurious `RangeError` because `temporal_calendar_fields` routed every
//! calendar -- including `"iso8601"`, which never needs a conversion at all
//! -- through that constructor.
//!
//! Fixed by giving the `"iso8601"` calendar its own fast path in
//! `temporal_calendar_fields` (`backend/bluejs/src/vm/temporal.rs`) that
//! reads the value's own stored ISO date fields directly, never calling
//! `icu_calendar::Date::try_new_iso` at all for that calendar.
//!
//! Every boundary value here is the pinned Test262 corpus's own: the
//! `PlainDate`/`PlainYearMonth` extremes are exactly
//! `built-ins/Temporal/PlainDate/from/argument-string-limits.js`'s and
//! `built-ins/Temporal/PlainYearMonth/from/limits.js`'s own valid endpoints
//! (`-271821-04-19` / `+275760-09-13` for `PlainDate`, `{year: -271821,
//! month: 4}` / `{year: 275760, month: 9}` for `PlainYearMonth`, the latter
//! fixture asserting the exact same `year`/`month`/`monthCode` getter triple
//! this bug broke). The `era`/`eraYear` expectation is
//! `built-ins/Temporal/PlainDate/prototype/era/basic.js`'s own assertion
//! (`instance.era === undefined` for the ISO calendar) -- a second,
//! separately documented Stage 0 bug this same fast path fixes as a direct
//! consequence, since it no longer reaches `icu_calendar`'s ISO
//! `era_year_from_extended`, which unconditionally reports a `"default"`
//! era Temporal's ISO calendar does not have.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `PlainDate/from/argument-string-limits.js`'s own valid endpoints: a
/// `PlainDate` at either extreme of Temporal's representable range
/// constructs, and every ISO-calendar getter on it now reads back the exact
/// fields it was constructed with instead of throwing.
#[test]
fn plain_date_getters_work_at_the_representable_range_extremes() {
    assert_true(
        r#"
        let min = new Temporal.PlainDate(-271821, 4, 19);
        min.year === -271821 && min.month === 4 && min.monthCode === "M04"
            && min.day === 19 && min.era === undefined && min.eraYear === undefined
            && min.monthsInYear === 12
    "#,
    );
    assert_true(
        r#"
        let max = new Temporal.PlainDate(275760, 9, 13);
        max.year === 275760 && max.month === 9 && max.monthCode === "M09"
            && max.day === 13 && max.era === undefined && max.eraYear === undefined
    "#,
    );
}

/// `PlainYearMonth/from/limits.js` asserts exactly this triple --
/// `year`/`month`/`monthCode` -- at both representable extremes via
/// `TemporalHelpers.assertPlainYearMonth`, which is precisely the getter
/// path this bug broke.
#[test]
fn plain_year_month_getters_match_the_pinned_limits_fixture() {
    assert_true(
        r#"
        let min = new Temporal.PlainYearMonth(-271821, 4);
        min.year === -271821 && min.month === 4 && min.monthCode === "M04"
    "#,
    );
    assert_true(
        r#"
        let max = new Temporal.PlainYearMonth(275760, 9);
        max.year === 275760 && max.month === 9 && max.monthCode === "M09"
    "#,
    );
}

/// `PlainDateTime` shares the same `temporal_calendar_fields` dispatch as
/// `PlainDate` for its calendar (date) fields. Note: `PlainDateTime`'s
/// `hour`/`minute`/etc. getters are not wired to any prototype at all yet
/// (a separate, pre-existing Stage 0/1 gap unrelated to this fix -- only
/// `PlainTime` gets them, per `temporal.rs`'s `getters` table), so this
/// deliberately checks only the calendar-field getters this fix covers.
#[test]
fn plain_date_time_getters_work_at_the_representable_range_extremes() {
    assert_true(
        r#"
        let dt = new Temporal.PlainDateTime(-271821, 4, 19, 1, 0);
        dt.year === -271821 && dt.month === 4 && dt.day === 19 && dt.monthCode === "M04"
    "#,
    );
}

/// `PlainDate/prototype/era/basic.js`: the ISO calendar has no eras, so
/// `era`/`eraYear` are `undefined` -- not just at the extremes, but for any
/// ISO-calendar date, since the fast path applies uniformly rather than only
/// to the values `icu_calendar` alone would have rejected.
#[test]
fn iso_calendar_era_and_era_year_are_always_undefined() {
    assert_true(
        r#"
        let instance = new Temporal.PlainDate(2000, 3, 6);
        instance.era === undefined && instance.eraYear === undefined
    "#,
    );
}

/// An ordinary in-range ISO-calendar date -- comfortably inside
/// `icu_calendar`'s own `CONSTRUCTOR_YEAR_RANGE` -- is unaffected by the
/// fast path: it must keep reading back the same fields it always did.
#[test]
fn ordinary_in_range_dates_are_unaffected() {
    assert_true(
        r#"
        let date = new Temporal.PlainDate(2024, 2, 29);
        date.year === 2024 && date.month === 2 && date.monthCode === "M02"
            && date.day === 29 && date.monthsInYear === 12
    "#,
    );
}
