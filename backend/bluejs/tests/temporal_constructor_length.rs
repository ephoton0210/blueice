// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the `length` of every `Temporal.*` constructor
//! (`built-ins/Temporal/<Type>/length.js`).
//!
//! `Temporal.ZonedDateTime` reported `length` 1, sharing `Temporal.Instant`'s
//! arm of the constructor table. Its signature is
//! `ZonedDateTime(epochNanoseconds, timeZone [, calendar])`, so the spec
//! length is 2: the required `timeZone` counts and only the trailing optional
//! `calendar` does not.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = compile(&parse(source).unwrap()).unwrap();
    Vm::default().execute(&program).unwrap()
}

/// Each constructor's `length` is the number of parameters before the first
/// optional one.
#[test]
fn every_temporal_constructor_has_its_specified_length() {
    for (name, length) in [
        ("Duration", 0),
        ("Instant", 1),
        ("PlainDate", 3),
        ("PlainDateTime", 3),
        ("PlainMonthDay", 2),
        ("PlainTime", 0),
        ("PlainYearMonth", 2),
        ("ZonedDateTime", 2),
    ] {
        assert_eq!(
            evaluate(&format!("Temporal.{name}.length")),
            Value::Number(f64::from(length)),
            "Temporal.{name}.length"
        );
    }
}

/// `length` is `{ writable: false, enumerable: false, configurable: true }`
/// on every constructor, including `ZonedDateTime` whose value changed.
#[test]
fn constructor_length_has_the_builtin_function_attributes() {
    let source = r#"
        ["Duration", "Instant", "PlainDate", "PlainDateTime", "PlainMonthDay",
         "PlainTime", "PlainYearMonth", "ZonedDateTime"].every(name => {
            const d = Object.getOwnPropertyDescriptor(Temporal[name], "length");
            return d.writable === false && d.enumerable === false
                && d.configurable === true;
        })
    "#;
    assert_eq!(evaluate(source), Value::Bool(true));
}
