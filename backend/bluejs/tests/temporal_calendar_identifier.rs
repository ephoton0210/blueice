// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_calendar_identifier`
//! (`backend/bluejs/src/vm/temporal.rs`), the shared `ToTemporalCalendarIdentifier`
//! helper behind every property-bag `calendar` field and `withCalendar`
//! argument across `PlainDate`/`PlainDateTime`/`PlainYearMonth`/
//! `PlainMonthDay`. Pinned directly from Test262's
//! `PlainYearMonth/prototype/equals/argument-propertybag-calendar-*.js`
//! fixtures (equally applicable to `PlainMonthDay`, which shares the same
//! helper).
//!
//! Three real bugs fixed together:
//! 1. A bracket-less string (e.g. `"2020-01-01"`) always fell straight to
//!    treating the *whole string* as a calendar-ID literal, which always
//!    failed since no real calendar ID looks like a date -- it must instead
//!    parse as a recognized Temporal string shape and imply `"iso8601"`
//!    when unannotated.
//! 2. A Temporal object (`PlainDate`/`PlainDateTime`/`PlainMonthDay`/
//!    `PlainYearMonth`/`ZonedDateTime`) supplied as the `calendar` value
//!    must yield its own internal calendar directly (`ToTemporalCalendar`
//!    step 1.a's fast path), never reading its `calendar`/`calendarId`
//!    JS-visible properties.
//! 3. Anything that is not a String primitive (and not the fast-path
//!    object above) is a `TypeError` immediately -- no `ToString`
//!    coercion, unlike most other Temporal string arguments.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

fn assert_throws(source: &str, expect_type_error: bool) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match (Vm::default().execute(&program), expect_type_error) {
        (Err(RuntimeError::TypeError(_)), true) => {}
        (Err(RuntimeError::RangeError(_)), false) => {}
        other => panic!("{source}\n  -> unexpected result: {other:?}"),
    }
}

/// The fixture's own eight unannotated/annotated date, dateTime,
/// year-month and month-day forms, all implying `"iso8601"`.
#[test]
fn unannotated_or_iso_annotated_date_shaped_strings_resolve_to_iso8601() {
    for calendar in [
        "2020-01-01",
        "2020-01-01[u-ca=iso8601]",
        "2020-01-01T00:00:00.000000000",
        "2020-01-01T00:00:00.000000000[u-ca=iso8601]",
        "01-01",
        "01-01[u-ca=iso8601]",
        "2020-01",
        "2020-01[u-ca=iso8601]",
    ] {
        let source = format!(
            r#"
            const instance = new Temporal.PlainYearMonth(2019, 6);
            instance.equals({{ year: 2019, monthCode: "M06", calendar: "{calendar}" }})
        "#
        );
        assert_true(&source);
    }
}

/// A leap-second ISO string is still a valid (unannotated) calendar string.
#[test]
fn leap_second_string_is_a_valid_calendar_string() {
    assert_true(
        r#"
        const instance = new Temporal.PlainYearMonth(2019, 6);
        instance.equals({ year: 2019, monthCode: "M06", calendar: "2016-12-31T23:59:60" })
    "#,
    );
}

/// A Temporal object's own internal calendar is used directly, without
/// reading its `calendar`/`calendarId` properties.
#[test]
fn temporal_object_calendar_fast_path_skips_property_reads() {
    assert_true(
        r#"
        const plainDate = new Temporal.PlainDate(2000, 5, 2, "iso8601");
        Object.defineProperty(plainDate, "calendar", {
            get() { throw new Error("should not read calendar"); },
        });
        Object.defineProperty(plainDate, "calendarId", {
            get() { throw new Error("should not read calendarId"); },
        });
        const yearmonth = new Temporal.PlainYearMonth(2000, 5);
        yearmonth.equals({ year: 2005, month: 6, calendar: plainDate });
        true
    "#,
    );
}

/// Non-string, non-fast-path values throw `TypeError`, not a coerced
/// `RangeError`.
#[test]
fn non_string_non_object_values_throw_type_error() {
    for expr in [
        "null",
        "true",
        "1",
        "1n",
        "Symbol()",
        "{}",
        "new Temporal.Duration()",
    ] {
        let source = format!(
            r#"
            const instance = new Temporal.PlainYearMonth(2000, 5);
            instance.equals({{ year: 2019, monthCode: "M11", day: 1, calendar: {expr} }});
        "#
        );
        assert_throws(&source, true);
    }
}

/// A negative-zero extended year is still rejected as invalid (`RangeError`,
/// not `TypeError`) -- confirms the fix didn't loosen this existing check.
#[test]
fn negative_zero_extended_year_is_still_a_range_error() {
    assert_throws(
        r#"
        const instance = new Temporal.PlainYearMonth(2000, 5);
        instance.equals({ year: 1976, month: 11, day: 18, calendar: "-000000-10-31" });
    "#,
        false,
    );
}
