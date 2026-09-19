// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainDate.prototype.with`'s (shared
//! with `Temporal.PlainDateTime.prototype.with` via `temporal_date_with`,
//! `backend/bluejs/src/vm/temporal.rs`) `era`/`eraYear` mutual-exclusivity
//! validation, pinned directly from the real Test262 fixtures
//! `intl402/Temporal/PlainDate/prototype/with/mutually-exclusive-fields-gregory.js`,
//! `.../mutually-exclusive-fields-chinese.js`,
//! `.../calendarresolvefields-error-ordering-gregory.js`, and
//! `built-ins/Temporal/PlainDate/prototype/with/wrapping-at-end-of-month-gregory.js`
//! (via `intl402/.../wrapping-at-end-of-month-gregory.js`, its `Intl.Era`
//! flavored sibling).
//!
//! Before this pass, three separate bugs made `temporal_date_with`'s era
//! handling wrong:
//! 1. `era` without `eraYear` silently fell back to the receiver's own
//!    `eraYear` instead of throwing -- so the mutual-exclusivity
//!    `TypeError` never fired.
//! 2. `eraYear` without `era` threw `RangeError` instead of the spec's
//!    `TypeError`.
//! 3. A calendar with no era concept at all (`chinese`/`dangi`) let
//!    `era`/`eraYear` through unvalidated (silently ignored, like
//!    `iso8601`) instead of throwing `TypeError`, per
//!    `CalendarFields.cpp`'s `NonISOFieldKeysToIgnore`.
//!
//! A fourth, unrelated bug in the same function was found in the same
//! triage pass and fixed alongside it: `day` was bounded to `1..=31` at
//! the field-reading stage, so `date.with({ day: daysInMonth + 1 })`
//! (spec-valid: it must *constrain* under the default overflow, and only
//! `RangeError` under `overflow: "reject"`) threw immediately instead of
//! reaching the calendar's own `overflow` regulation --
//! `ToPositiveIntegerWithTruncation` (`CalendarFields.cpp`'s
//! `CalendarField::Day` case) has no upper bound at all.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

fn assert_throws_type_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("{source}\n  -> expected TypeError, got {other:?}"),
    }
}

fn assert_throws_range_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("{source}\n  -> expected RangeError, got {other:?}"),
    }
}

/// `era` and `eraYear` together resolve the year, excluding the receiver's
/// own `year` -- `mutually-exclusive-fields-gregory.js`'s first assertion.
#[test]
fn plain_date_era_and_era_year_together_exclude_year() {
    assert_true(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.PlainDate.from(
            { year: 1981, monthCode: "M12", day: 15, calendar: "gregory" }, options
        );
        const changed = instance.with({ era: "bce", eraYear: 1 }, options);
        changed.year === 0 && changed.month === 12 && changed.monthCode === "M12"
            && changed.day === 15 && changed.era === "bce" && changed.eraYear === 1
    "#,
    );
}

/// A bare `year` override excludes the receiver's own `era`/`eraYear` --
/// they are recomputed fresh from the new extended year.
#[test]
fn plain_date_year_alone_excludes_era_and_era_year() {
    assert_true(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.PlainDate.from(
            { year: 1981, monthCode: "M12", day: 15, calendar: "gregory" }, options
        );
        const changed = instance.with({ year: -2 }, options);
        changed.year === -2 && changed.era === "bce" && changed.eraYear === 3
    "#,
    );
}

/// `eraYear` without `era` is a `TypeError` on an era-supporting calendar --
/// previously threw `RangeError` instead.
#[test]
fn plain_date_era_year_alone_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.PlainDate.from(
            { year: 1981, monthCode: "M12", day: 15, calendar: "gregory" }
        );
        instance.with({ eraYear: 1 });
    "#,
    );
}

/// `era` without `eraYear` is likewise a `TypeError` -- previously silently
/// fell back to the receiver's own `eraYear` and never threw at all.
#[test]
fn plain_date_era_alone_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.PlainDate.from(
            { year: 1981, monthCode: "M12", day: 15, calendar: "gregory" }
        );
        instance.with({ era: "bce" });
    "#,
    );
}

/// `chinese`/`dangi` have no ICU4X era concept at all: providing
/// `era`+`eraYear` together must throw `TypeError`, unlike `iso8601` which
/// silently ignores them (`mutually-exclusive-fields-chinese.js`).
#[test]
fn plain_date_era_and_era_year_on_a_non_era_calendar_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.PlainDate.from(
            { year: 1981, monthCode: "M12", day: 15, calendar: "chinese" }
        );
        instance.with({ eraYear: 2025, era: "ce" });
    "#,
    );
}

/// `iso8601` has no eras either, but unlike `chinese`/`dangi` it must
/// silently ignore `era`/`eraYear` rather than throw (matching
/// `built-ins/Temporal/PlainDate/prototype/with/time-units-ignored.js`).
#[test]
fn plain_date_era_on_iso8601_is_silently_ignored() {
    assert_true(
        r#"
        const instance = Temporal.PlainDate.from({ year: 2020, month: 1, day: 1 });
        const changed = instance.with({ day: 30, era: "BC" });
        changed.year === 2020 && changed.month === 1 && changed.day === 30
    "#,
    );
}

/// `CalendarResolveFields` validates field types (the era mutual-exclusivity
/// `TypeError`) before it validates field ranges (a `RangeError` from a
/// month/monthCode conflict) -- `calendarresolvefields-error-ordering-
/// gregory.js`.
#[test]
fn plain_date_era_type_error_precedes_month_conflict_range_error() {
    assert_throws_type_error(
        r#"
        const plainDate = Temporal.PlainDate.from(
            { calendar: "gregory", year: 2020, month: 5, day: 15 }
        );
        plainDate.with({ era: "ce", monthCode: "M05", month: 6 });
    "#,
    );
}

/// Same ordering requirement, this time against an out-of-range `day`
/// instead of a month/monthCode conflict.
#[test]
fn plain_date_era_type_error_precedes_day_range_error() {
    assert_throws_type_error(
        r#"
        const plainDate = Temporal.PlainDate.from(
            { calendar: "gregory", year: 2020, month: 5, day: 15 }
        );
        plainDate.with({ era: "ce", day: 32 });
    "#,
    );
}

/// Once every field's *type* is valid, an actual conflict still throws
/// `RangeError`, not `TypeError` -- the ordering fix must not swallow this.
#[test]
fn plain_date_month_and_month_code_conflict_still_throws_range_error() {
    assert_throws_range_error(
        r#"
        const plainDate = Temporal.PlainDate.from(
            { calendar: "gregory", year: 2020, month: 5, day: 15 }
        );
        plainDate.with({ monthCode: "M05", month: 6 });
    "#,
    );
}

/// `date.with({ day: daysInMonth + 1 })` constrains under the default
/// overflow (`wrapping-at-end-of-month-gregory.js`'s first assertion) --
/// previously threw immediately from the too-narrow `1..=31` field bound
/// regardless of the actual month length or overflow option.
#[test]
fn plain_date_day_one_past_month_end_constrains_by_default() {
    assert_true(
        r#"
        const date = Temporal.PlainDate.from({ year: 1970, month: 1, day: 1, calendar: "gregory" });
        const daysInMonth = date.daysInMonth;
        const constrained = date.with({ day: daysInMonth + 1 });
        constrained.day === daysInMonth
    "#,
    );
}

/// The same one-past-month-end `day` still throws `RangeError` under
/// `overflow: "reject"`.
#[test]
fn plain_date_day_one_past_month_end_rejects_when_asked() {
    assert_throws_range_error(
        r#"
        const date = Temporal.PlainDate.from({ year: 1970, month: 1, day: 1, calendar: "gregory" });
        const daysInMonth = date.daysInMonth;
        date.with({ day: daysInMonth + 1 }, { overflow: "reject" });
    "#,
    );
}

/// `Temporal.PlainDateTime.prototype.with` shares `temporal_date_with` with
/// `PlainDate` -- confirm the era fix applies there too.
#[test]
fn plain_date_time_era_year_alone_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.PlainDateTime.from(
            { year: 1981, monthCode: "M12", day: 15, hour: 12, calendar: "gregory" }
        );
        instance.with({ eraYear: 1 });
    "#,
    );
}
