// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Annex B.3.5: in `for (var x = init in obj)` an anonymous function or class
//! initializer is named after `x`, exactly as in `var x = init`.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn an_anonymous_initializer_is_named_after_the_binding() {
    for (initializer, name) in [
        ("function() {}", "head"),
        ("function named() {}", "named"),
        ("function*() {}", "head"),
        ("async function() {}", "head"),
        ("() => {}", "head"),
        ("async () => {}", "head"),
        ("class {}", "head"),
        ("class named {}", "named"),
        ("(function() {})", "head"),
    ] {
        let source = format!(
            "for (var head = {initializer} in {{}}) {{}}
             head.name === {name:?}"
        );
        assert_eq!(evaluate(&source), Value::Bool(true), "{initializer}");
    }
}

#[test]
fn a_class_with_a_static_name_member_keeps_it() {
    assert_eq!(
        evaluate("for (var head = class { static name = 'own'; } in {}) {} head.name === 'own'"),
        Value::Bool(true)
    );
}
