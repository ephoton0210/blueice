// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.prototype.with`'s
//! (`temporal_zoned_date_time_with`, `backend/bluejs/src/vm/temporal.rs`)
//! `era`/`eraYear` mutual-exclusivity validation, pinned directly from the
//! real Test262 fixtures
//! `intl402/Temporal/ZonedDateTime/prototype/with/mutually-exclusive-fields-gregory.js`,
//! `.../mutually-exclusive-fields-chinese.js`, and
//! `.../wrapping-at-end-of-month-gregory.js`.
//!
//! `temporal_zoned_date_time_with` had the textually-identical bug already
//! found and fixed in `temporal_date_with` (shared by `PlainDate`/
//! `PlainDateTime`) and `temporal_year_month_with` (`PlainYearMonth`) by an
//! earlier pass, documented at the time as a known, deliberately-deferred
//! gap specific to this type (see
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`'s
//! `zoned_date_time.rs` entry, "Deliberately left open" list):
//! 1. `era` without `eraYear` silently fell back to the receiver's own
//!    `eraYear` instead of throwing.
//! 2. `eraYear` without `era` threw `RangeError` instead of `TypeError`.
//! 3. A calendar with no era concept at all (`chinese`/`dangi`) let
//!    `era`/`eraYear` through unvalidated instead of throwing `TypeError`.
//!
//! A fourth, unrelated bug in the same function -- `day` bounded to
//! `1..=31` at the field-reading stage instead of unbounded (letting the
//! calendar's own `overflow` option regulate it) -- is fixed alongside it,
//! matching the identical fix already applied to `temporal_date_with`/
//! `temporal_month_day_with`.

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
fn zoned_date_time_era_and_era_year_together_exclude_year() {
    assert_true(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.ZonedDateTime.from(
            { year: 1981, monthCode: "M12", day: 15, hour: 12, minute: 34,
              timeZone: "UTC", calendar: "gregory" }, options
        );
        const changed = instance.with({ era: "bce", eraYear: 1 }, options);
        const p = changed.toPlainDateTime();
        p.year === 0 && p.month === 12 && p.monthCode === "M12" && p.day === 15
            && changed.era === "bce" && changed.eraYear === 1
    "#,
    );
}

/// Supplying `year` alone excludes era/eraYear, deriving a fresh
/// `era`/`eraYear` from it -- `mutually-exclusive-fields-gregory.js`'s
/// second assertion.
#[test]
fn zoned_date_time_year_excludes_era_and_era_year() {
    assert_true(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.ZonedDateTime.from(
            { year: 1981, monthCode: "M12", day: 15, hour: 12, minute: 34,
              timeZone: "UTC", calendar: "gregory" }, options
        );
        const changed = instance.with({ year: -2 }, options);
        const p = changed.toPlainDateTime();
        p.year === -2 && changed.era === "bce" && changed.eraYear === 3
    "#,
    );
}

/// `eraYear` supplied without `era` is a `TypeError` --
/// `mutually-exclusive-fields-gregory.js`'s trailing assertions.
#[test]
fn zoned_date_time_era_year_without_era_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.ZonedDateTime.from(
            { year: 1981, monthCode: "M12", day: 15, hour: 12, minute: 34,
              timeZone: "UTC", calendar: "gregory" }
        );
        instance.with({ eraYear: 1 });
    "#,
    );
}

/// `era` supplied without `eraYear` is a `TypeError`.
#[test]
fn zoned_date_time_era_without_era_year_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.ZonedDateTime.from(
            { year: 1981, monthCode: "M12", day: 15, hour: 12, minute: 34,
              timeZone: "UTC", calendar: "gregory" }
        );
        instance.with({ era: "bce" });
    "#,
    );
}

/// `chinese`/`dangi` have no era concept at all: any use of `era`/`eraYear`
/// in `.with()` must throw `TypeError`, unlike `iso8601` which silently
/// ignores them -- `mutually-exclusive-fields-chinese.js`.
#[test]
fn zoned_date_time_chinese_calendar_rejects_era_and_era_year() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.ZonedDateTime.from(
            { year: 1981, monthCode: "M12", day: 15, hour: 12, minute: 34,
              timeZone: "UTC", calendar: "chinese" }
        );
        instance.with({ eraYear: 2025, era: "ce" });
    "#,
    );
}

/// `chinese`'s `month`/`monthCode` mutual exclusivity in `.with()` still
/// works correctly (unaffected by the era fix) --
/// `mutually-exclusive-fields-chinese.js`'s non-era assertions.
#[test]
fn zoned_date_time_chinese_calendar_month_excludes_month_code() {
    assert_true(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.ZonedDateTime.from(
            { year: 1981, monthCode: "M12", day: 15, hour: 12, minute: 34,
              timeZone: "UTC", calendar: "chinese" }, options
        );
        const changed = instance.with({ month: 5 }, options);
        const p = changed.toPlainDateTime();
        p.year === 1981 && p.month === 5 && p.monthCode === "M05" && p.day === 15
    "#,
    );
}

/// `day` has no upper bound at the field-reading stage: the default
/// overflow (`"constrain"`) clamps it to the real month length, and only
/// `overflow: "reject"` throws `RangeError` --
/// `wrapping-at-end-of-month-gregory.js`.
#[test]
fn zoned_date_time_with_day_past_month_end_constrains_by_default_and_rejects_on_request() {
    // January has 31 days, so `daysInMonth + 1` is 32 -- outside the old,
    // wrongly-hardcoded `1..=31` field bound, unlike a shorter month such as
    // February (where `daysInMonth + 1` can stay under 31 and would not
    // have caught this bug).
    assert_true(
        r#"
        const date = Temporal.ZonedDateTime.from({
            year: 1970, month: 1, day: 1, calendar: "gregory",
            hour: 12, minute: 34, timeZone: "UTC",
        });
        const daysInMonth = date.daysInMonth;
        const constrained = date.with({ day: daysInMonth + 1 });
        constrained.day === daysInMonth
    "#,
    );
    assert_throws_range_error(
        r#"
        const date = Temporal.ZonedDateTime.from({
            year: 1970, month: 1, day: 1, calendar: "gregory",
            hour: 12, minute: 34, timeZone: "UTC",
        });
        const daysInMonth = date.daysInMonth;
        date.with({ day: daysInMonth + 1 }, { overflow: "reject" });
    "#,
    );
}
