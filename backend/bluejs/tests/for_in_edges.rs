// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `for-in` head evaluation (§14.7.5.6 ForIn/OfHeadEvaluation): a `null` or
//! `undefined` subject runs no iteration instead of throwing, and other
//! primitives are boxed.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn for_in_over_null_or_undefined_runs_no_iterations() {
    for subject in ["null", "undefined", "void 0"] {
        assert_eq!(
            evaluate(&format!("var n = 0; for (var k in {subject}) n++; n")),
            Value::Number(0.0),
            "{subject}"
        );
        assert_eq!(
            evaluate(&format!("var n = 0; for (let k in {subject}) {{ n++; }} n")),
            Value::Number(0.0),
            "{subject}"
        );
        assert_eq!(
            evaluate(&format!("var n = 0; var k; for (k in {subject}) n++; n")),
            Value::Number(0.0),
            "{subject}"
        );
    }
}

#[test]
fn for_in_over_null_leaves_an_undefined_completion_value() {
    assert_eq!(
        evaluate("eval('1; for (var a in undefined) { }')"),
        Value::Undefined
    );
    assert_eq!(
        evaluate("eval('2; for (var b in null) { 3; }')"),
        Value::Undefined
    );
    assert_eq!(
        evaluate("eval('4; for (var c in null);')"),
        Value::Undefined
    );
}

#[test]
fn for_in_still_boxes_other_primitives() {
    assert_eq!(
        evaluate("var s = ''; for (var k in 'ab') s += k; s"),
        Value::String("01".into())
    );
    assert_eq!(
        evaluate("var n = 0; for (var k in 5) n++; n"),
        Value::Number(0.0)
    );
}
