// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Own-property attributes of methods defined by object literals and classes
//! (§15.4.4 MethodDefinition evaluation, §15.7.10 ClassDefinition evaluation).

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

fn text(source: &str) -> String {
    match evaluate(source) {
        Value::String(text) => text.to_utf8().unwrap(),
        other => panic!("{source}: expected a string, got {other:?}"),
    }
}

#[test]
fn object_literal_methods_are_enumerable_writable_and_configurable() {
    assert_eq!(text("Object.keys({ m() {}, n: 1, *g() {}, async a() {} }).join()"), "m,n,g,a");
    assert_eq!(
        text(
            "var d = Object.getOwnPropertyDescriptor({ m() {} }, 'm');
             [d.writable, d.enumerable, d.configurable].join()"
        ),
        "true,true,true"
    );
}

#[test]
fn class_methods_stay_non_enumerable() {
    assert_eq!(text("Object.keys(class { static m() {} }).join()"), "");
    assert_eq!(text("Object.keys((class { m() {} }).prototype).join()"), "");
    assert_eq!(
        text(
            "var d = Object.getOwnPropertyDescriptor((class { m() {} }).prototype, 'm');
             [d.writable, d.enumerable, d.configurable].join()"
        ),
        "true,false,true"
    );
}
