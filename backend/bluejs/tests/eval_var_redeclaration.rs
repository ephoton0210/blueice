// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A sloppy direct eval may re-declare, with an initializer, a `var` that an
//! earlier eval in the same function already created. The second declaration
//! creates no new binding: it initializes the existing one.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn eval_var_with_initializer_reuses_a_binding_from_an_earlier_eval() {
    assert_eq!(
        evaluate("function t(){ eval('var x = 1;'); eval('var x = 2;'); return x; } t()"),
        Value::Number(2.0)
    );
}

#[test]
fn eval_var_patterns_reuse_bindings_from_an_earlier_eval() {
    assert_eq!(
        evaluate(
            "function t(){ eval('var x = 1, y = 1;'); eval('var [x] = [3]; var {y} = {y: 4};'); \
             return x * 10 + y; } t()"
        ),
        Value::Number(34.0)
    );
}

#[test]
fn eval_var_reinitialization_does_not_leak_to_the_global_object() {
    assert_eq!(
        evaluate(
            "function t(){ eval('var leak = 1;'); eval('var leak = 2;'); \
             return typeof globalThis.leak; } t()"
        ),
        Value::String("undefined".into())
    );
}
