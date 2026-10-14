// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Immutable ArrayBuffer proposal's `ArrayBuffer.prototype` methods and
//! write guards with wrong receivers, detached and immutable sources, and
//! user code that resizes or detaches the source during argument conversion.
//! The expected results are BlueJS's own (the reference engines used elsewhere
//! do not implement the proposal).

use blueice_bluejs::{compile, parse, Value, Vm};

/// Evaluates `expr` and reports its `String` conversion, or the error it threw.
fn observe(expr: &str) -> String {
    let source = format!(
        "(function(){{ try {{ return String({expr}) }} catch (e) {{ return 'throws ' + e.constructor.name + ': ' + e.message }} }})()"
    );
    let mut vm = Vm::default();
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => format!("{other:?}"),
    }
}

include!("cov_g1_tables/immutable_array_buffers.in");

#[test]
fn immutable_array_buffers_report_the_reference_outcomes() {
    let mut mismatches = Vec::new();
    for (expr, expected) in CASES {
        let actual = observe(expr);
        if actual != *expected {
            mismatches.push(format!(
                "{expr}\n    expected {expected}\n    actual   {actual}"
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
