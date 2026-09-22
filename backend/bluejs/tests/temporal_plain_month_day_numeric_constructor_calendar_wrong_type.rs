// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Vm::temporal_calendar` (`backend/bluejs/src/vm/temporal.rs`),
//! the raw numeric-constructor's own positional `calendar` argument shared by
//! `Temporal.PlainDate`/`PlainDateTime`/`PlainMonthDay`/`PlainYearMonth`/
//! `ZonedDateTime`. Pinned directly from Test262's
//! `built-ins/Temporal/PlainMonthDay/calendar-wrong-type.js` (identical
//! fixtures also exist for the other four numeric constructors, per the
//! spec step "If calendar is not a String, throw a TypeError exception" --
//! no `ToString` coercion at all, unlike most other Temporal string
//! arguments).
//!
//! The real bug: `Vm::temporal_calendar` called `self.coerce_string(value)`,
//! which `ToString`-coerces *any* value (so `new Temporal.PlainMonthDay(12,
//! 15, null, 1972)` silently stringified `null` to `"null"`, then failed with
//! a `RangeError` for an unrecognized calendar id, instead of the spec's
//! immediate `TypeError` for a non-`String` value).

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn assert_type_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("{source}\n  -> expected TypeError, got: {other:?}"),
    }
}

/// Every non-`String` value Test262's own fixture lists must throw
/// `TypeError`, not a `ToString`-coerced `RangeError`.
#[test]
fn non_string_calendar_values_throw_type_error() {
    for expr in [
        "null",
        "true",
        "1",
        "1n",
        "-19761118",
        "19761118",
        "1234567890",
        "Symbol()",
        "{}",
        "new Temporal.Duration()",
    ] {
        let source = format!("new Temporal.PlainMonthDay(12, 15, {expr}, 1972)");
        assert_type_error(&source);
    }
}

/// A real `String` calendar argument is unaffected by the fix.
#[test]
fn string_calendar_values_still_work() {
    let program = compile(
        &parse("new Temporal.PlainMonthDay(12, 15, \"iso8601\", 1972).monthCode === \"M12\"")
            .unwrap(),
    )
    .unwrap();
    let result = Vm::default().execute(&program).unwrap();
    assert_eq!(result, Value::Bool(true));
}
