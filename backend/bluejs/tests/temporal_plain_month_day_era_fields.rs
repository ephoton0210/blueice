// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_plain_month_day_from_fields`
//! (`backend/bluejs/src/vm/temporal.rs`), pinned directly from Test262's
//! `intl402/Temporal/PlainMonthDay/prototype/{equals,toPlainDate}/
//! infinity-throws-rangeerror.js`.
//!
//! The real bug: `PlainMonthDay`'s own field list has no `era`/`eraYear` of
//! its own, but per `CalendarExtraFields` (`CalendarFields.cpp`), requesting
//! `year` -- which `ToTemporalMonthDay`'s field list always does -- also
//! reads `era`/`eraYear` for any calendar that supports eras. This engine
//! never read either property at all for `PlainMonthDay`, so `{ era: "ad",
//! eraYear: Infinity }` on a `"gregory"`-calendar receiver silently ignored
//! both fields (falling through to "requires monthCode or year" instead of
//! ever reaching `eraYear`'s own `Infinity` check) rather than reporting
//! `RangeError` from `eraYear`'s own out-of-range value.
//!
//! Ported directly from `temporal_plain_date_from_fields`'s own established
//! era/eraYear handling: `era`/`eraYear` (supplied together, `TypeError`
//! otherwise) resolve the year via `icu_calendar`'s own era-aware
//! `Date::try_from_fields`, mutually exclusive with a separately-supplied
//! `year`. Read only when `calendar::calendar_supports_era` is true
//! (`iso8601`/`chinese`/`dangi` never have eras, matching
//! `PrepareCalendarFields`'s own conditional field-name expansion).

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

/// `eraYear: Infinity`/`-Infinity` on an era-supporting calendar is a
/// `RangeError`, reached via `equals`'s property-bag resolution.
#[test]
fn infinite_era_year_throws_range_error_via_equals() {
    for inf in ["Infinity", "-Infinity"] {
        let source = format!(
            r#"
            const instance = new Temporal.PlainMonthDay(5, 2, "gregory");
            instance.equals({{ era: "ad", month: 5, day: 2, calendar: "gregory", eraYear: {inf} }})
        "#
        );
        match run(&source) {
            Err(RuntimeError::RangeError(_)) => {}
            other => panic!("{source}\n  -> expected RangeError, got: {other:?}"),
        }
    }
}

/// The same, via `toPlainDate`'s property-bag resolution.
#[test]
fn infinite_era_year_throws_range_error_via_to_plain_date() {
    for inf in ["Infinity", "-Infinity"] {
        let source = format!(
            r#"
            const instance = new Temporal.PlainMonthDay(5, 2, "gregory");
            instance.toPlainDate({{ era: "ad", eraYear: {inf} }})
        "#
        );
        match run(&source) {
            Err(RuntimeError::RangeError(_)) => {}
            other => panic!("{source}\n  -> expected RangeError, got: {other:?}"),
        }
    }
}

/// `era` supplied without `eraYear` (or vice versa) is a `TypeError`.
#[test]
fn era_without_era_year_is_a_type_error() {
    match run(
        r#"Temporal.PlainMonthDay.from({ era: "ad", month: 5, day: 2, calendar: "gregory" })"#,
    ) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("expected TypeError, got: {other:?}"),
    }
}

/// A real, finite `era`/`eraYear` pair resolves without error, to the
/// requested `monthCode`/`day` -- `.from()`'s own property-bag path.
///
/// Note: `era`/`eraYear` (when given) resolve the *actual* year for a
/// non-ISO calendar -- unlike `iso8601`'s own fixed-1972-reference-year
/// special case, a non-ISO `PlainMonthDay`'s underlying ISO date genuinely
/// depends on which year was supplied, so this deliberately does not assert
/// equality against a differently-constructed instance (e.g. one built
/// without a year at all, which resolves its own, likely different,
/// reference year).
#[test]
fn valid_era_and_era_year_resolves_month_and_day() {
    assert_true(
        r#"
        const md = Temporal.PlainMonthDay.from({ era: "ad", eraYear: 2021, month: 5, day: 2, calendar: "gregory" });
        md.monthCode === "M05" && md.day === 2
    "#,
    );
}

/// Two property bags with the *same* `era`/`eraYear`/`month`/`day` resolve
/// to the same underlying date, so `equals` reports `true` -- confirming
/// era resolution is actually deterministic/self-consistent, not merely
/// non-throwing.
#[test]
fn equals_with_matching_era_year_pairs_is_true() {
    assert_true(
        r#"
        const md = Temporal.PlainMonthDay.from({ era: "ad", eraYear: 2021, month: 5, day: 2, calendar: "gregory" });
        md.equals({ era: "ad", eraYear: 2021, month: 5, day: 2, calendar: "gregory" })
    "#,
    );
}

/// `toPlainDate`'s own property-bag resolution: a finite `era`/`eraYear`
/// pair (with no `year` property at all -- per the actual spec text,
/// `PrepareCalendarFields(calendar, item, « year », « », « »)` has an
/// *empty* required-field list) resolves to a real `PlainDate` whose
/// `monthCode`/`day` match the receiver.
#[test]
fn to_plain_date_with_valid_era_year_resolves() {
    assert_true(
        r#"
        const instance = new Temporal.PlainMonthDay(5, 2, "gregory");
        const date = instance.toPlainDate({ era: "ad", eraYear: 2021 });
        date.monthCode === "M05" && date.day === 2
    "#,
    );
}

/// `iso8601` (no eras at all) is unaffected -- `era`/`eraYear` are simply
/// never read, so an `Infinity` there doesn't matter and a bare ordinal
/// month/day bag keeps working exactly as before.
#[test]
fn iso8601_calendar_is_unaffected() {
    assert_true(
        r#"
        const md = Temporal.PlainMonthDay.from({ month: 1, day: 22 });
        md.equals({ month: 1, day: 22 }) === true
    "#,
    );
}
