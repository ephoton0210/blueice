// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_plain_month_day_from_fields`
//! (`backend/bluejs/src/vm/temporal.rs`), pinned directly from Test262's
//! `built-ins/Temporal/PlainMonthDay/from/monthcode-invalid.js`.
//!
//! Three real bugs found and fixed together, all in the `iso8601`-calendar
//! fast path (`plain_month_day::iso_month_day_from_fields`'s caller):
//!
//! 1. A `monthCode` supplied *alongside* a numeric `month` was never
//!    syntax-checked or resolved at all -- the code silently preferred
//!    `month` and ignored `monthCode` entirely, so a malformed code like
//!    `"m1"` (wrong case) went unnoticed, and a code that *conflicts* with
//!    `month` (`{ month: 12, monthCode: "M11" }`) was never rejected.
//! 2. A syntactically well-formed but numerically out-of-range or
//!    leap-suffixed `monthCode` (`"M00"`, `"M19"`, `"M99"`, `"M13"`,
//!    `"M00L"`, `"M05L"`, `"M13L"`) resolved via a naive
//!    `.strip_prefix('M')?.parse()`, which happened to reject the `L`-suffixed
//!    ones by accident (they don't parse as a bare integer) but *silently
//!    constrained* the bare out-of-range ones under the default `overflow`
//!    (`"M19"` clamped to month 12) instead of always throwing `RangeError`
//!    -- `monthCode` suitability is a distinct check from a numeric field's
//!    own constrain/reject regulation and must reject regardless of
//!    `overflow`.
//! 3. `monthCode`'s syntax was validated too late (interleaved with the
//!    calendar resolution, after `year` was already coerced), so a
//!    syntactically malformed code did not reliably take priority over a
//!    later field's own conversion error -- fixed by validating `monthCode`
//!    syntax immediately once it is coerced to a string, ahead of `year`'s
//!    own numeric coercion.

use blueice_bluejs::{compile, parse, RuntimeError, Vm};

fn run(source: &str) -> Result<blueice_bluejs::Value, RuntimeError> {
    let program = compile(&parse(source).unwrap()).unwrap();
    Vm::default().execute(&program)
}

fn assert_range_error(source: &str) {
    match run(source) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("{source}\n  -> expected RangeError, got: {other:?}"),
    }
}

/// A malformed `monthCode` is rejected even when a numeric `month` is also
/// present (previously silently ignored in favor of `month`).
#[test]
fn malformed_month_code_is_rejected_even_alongside_a_numeric_month() {
    for code in ["m1", "M1", "m01"] {
        assert_range_error(&format!(
            r#"Temporal.PlainMonthDay.from({{ monthCode: "{code}", day: 17 }})"#
        ));
        assert_range_error(&format!(
            r#"Temporal.PlainMonthDay.from({{ month: 1, monthCode: "{code}", day: 17 }})"#
        ));
    }
}

/// A `month`/`monthCode` pair that disagree is a `RangeError`.
#[test]
fn conflicting_month_and_month_code_is_a_range_error() {
    assert_range_error(
        r#"Temporal.PlainMonthDay.from({ year: 2021, month: 12, monthCode: "M11", day: 17 })"#,
    );
}

/// A well-formed but out-of-range or leap-suffixed `monthCode` always
/// throws `RangeError`, regardless of `overflow` -- never silently
/// constrained the way a numeric `month` field is.
#[test]
fn out_of_range_or_leap_month_codes_are_always_rejected() {
    for code in ["M00", "M19", "M99", "M13", "M00L", "M05L", "M13L"] {
        assert_range_error(&format!(
            r#"Temporal.PlainMonthDay.from({{ monthCode: "{code}", day: 17 }})"#
        ));
        assert_range_error(&format!(
            r#"Temporal.PlainMonthDay.from({{ monthCode: "{code}", day: 17 }}, {{ overflow: "reject" }})"#
        ));
    }
}

/// `monthCode` syntax is validated before `year`'s own numeric coercion --
/// a malformed code (`"L99M"`) throws `RangeError` even when `year` is a
/// `Symbol` that would otherwise throw `TypeError`.
#[test]
fn malformed_month_code_syntax_is_checked_before_year_coercion() {
    match run(r#"Temporal.PlainMonthDay.from({ day: 1, monthCode: "L99M", year: Symbol() })"#) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("expected RangeError, got: {other:?}"),
    }
}

/// A well-formed but calendar-unsuitable `monthCode` (`"M99L"`) does *not*
/// short-circuit early -- `year`'s own `Symbol` conversion still throws
/// `TypeError` first, since the code's suitability is only checked once an
/// actual calendar resolution is attempted.
#[test]
fn well_formed_but_unsuitable_month_code_does_not_preempt_year_type_error() {
    match run(r#"Temporal.PlainMonthDay.from({ day: 1, monthCode: "M99L", year: Symbol() })"#) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("expected TypeError, got: {other:?}"),
    }
}

/// A valid `monthCode` (alone, or agreeing with `month`) is unaffected by
/// these fixes.
#[test]
fn valid_month_code_still_resolves() {
    let result = run(r#"Temporal.PlainMonthDay.from({ monthCode: "M06", day: 15 }).monthCode"#)
        .unwrap();
    assert_eq!(result, blueice_bluejs::Value::String("M06".into()));
    let result =
        run(r#"Temporal.PlainMonthDay.from({ month: 6, monthCode: "M06", day: 15 }).monthCode"#)
            .unwrap();
    assert_eq!(result, blueice_bluejs::Value::String("M06".into()));
}
