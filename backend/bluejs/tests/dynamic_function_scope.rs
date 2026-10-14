// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A function made by CreateDynamicFunction has a `name` property of
//! "anonymous" but no binding of that name: its body resolves `anonymous`
//! like any free identifier, in the global scope.
use blueice_bluejs::{compile, parse, Value, Vm};

fn check(sources: &[&str]) {
    for source in sources {
        let value = Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(value, Value::Bool(true), "{source}");
    }
}

#[test]
fn the_function_name_is_not_a_binding_inside_the_body() {
    check(&[
        "new Function('return typeof anonymous')() === 'undefined'",
        "new Function('return function () { return typeof anonymous; }')()() === 'undefined'",
        "new Function('return function () { eval(\"\"); return typeof anonymous; }')()() === 'undefined'",
        "Function('a', 'return typeof anonymous')(1) === 'undefined'",
    ]);
}

#[test]
fn a_global_named_anonymous_is_visible_to_the_body() {
    check(&[
        "globalThis.anonymous = 7; new Function('return anonymous')() === 7",
        "globalThis.anonymous = 'g'; Function('return anonymous')() === 'g'",
    ]);
}

#[test]
fn the_name_property_is_still_anonymous() {
    check(&[
        "new Function('a', 'return a').name === 'anonymous'",
        "Object.getOwnPropertyDescriptor(Function(), 'name').value === 'anonymous'",
    ]);
}
