// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! SetFunctionName for anonymous functions and methods created by property
//! definitions (§13.2.5.5 PropertyDefinitionEvaluation, §15.4.4 MethodDefinition
//! evaluation): literal keys name at compile time, computed and symbol keys at
//! run time, accessors get a `get `/`set ` prefix.

use blueice_bluejs::{compile, parse, Value, Vm};

fn text(source: &str) -> String {
    let value = Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    match value {
        Value::String(text) => text.to_utf8().unwrap(),
        other => panic!("{source}: expected a string, got {other:?}"),
    }
}

#[test]
fn anonymous_functions_take_a_literal_property_name() {
    assert_eq!(text("({ id: function() {} }).id.name"), "id");
    assert_eq!(text("({ id: () => {} }).id.name"), "id");
    assert_eq!(text("({ id: class {} }).id.name"), "id");
    assert_eq!(text("({ id: (function() {}) }).id.name"), "id");
    assert_eq!(text("({ 'a b': function() {} })['a b'].name"), "a b");
    assert_eq!(text("({ 1: function() {} })[1].name"), "1");
    assert_eq!(text("({ id: async () => {} }).id.name"), "id");
    assert_eq!(text("({ id: function*() {} }).id.name"), "id");
}

#[test]
fn only_anonymous_function_definitions_are_named() {
    assert_eq!(text("({ xId: function x() {} }).xId.name"), "x");
    assert_eq!(text("({ xId: (0, function() {}) }).xId.name"), "");
    assert_eq!(text("var f = function() {}; ({ id: f }).id.name"), "f");
    assert_eq!(text("({ id: class { static name() {} } }).id.name.constructor === Function ? 'fn' : 'other'"), "fn");
}

#[test]
fn computed_keys_name_functions_at_run_time() {
    assert_eq!(text("({ ['a' + 'b']: function() {} }).ab.name"), "ab");
    assert_eq!(text("({ ['a' + 'b']: () => {} }).ab.name"), "ab");
    assert_eq!(text("var s = Symbol('test262'); ({ [s]: function() {} })[s].name"), "[test262]");
    assert_eq!(text("var s = Symbol(); ({ [s]: function() {} })[s].name"), "");
    assert_eq!(text("var s = Symbol('m'); ({ [s]() {} })[s].name"), "[m]");
    assert_eq!(text("var s = Symbol('m'); ({ *[s]() {} })[s].name"), "[m]");
    assert_eq!(text("var k = 'x'; ({ [k]() {} }).x.name"), "x");
    assert_eq!(
        text(
            "var s = Symbol('a'); var o = { get [s]() {}, set [s](v) {} };
             var d = Object.getOwnPropertyDescriptor(o, s); d.get.name + '|' + d.set.name"
        ),
        "get [a]|set [a]"
    );
}

#[test]
fn a_computed_key_leaves_a_named_function_alone() {
    assert_eq!(text("var k = 'x'; ({ [k]: function named() {} }).x.name"), "named");
}

#[test]
fn class_methods_take_symbol_and_computed_names() {
    assert_eq!(text("var s = Symbol('m'); (class { [s]() {} }).prototype[s].name"), "[m]");
    assert_eq!(text("var s = Symbol('m'); (class { static [s]() {} })[s].name"), "[m]");
    assert_eq!(text("var s = Symbol('m'); Object.getOwnPropertyDescriptor(class { static get [s]() {} }, s).get.name"), "get [m]");
}

#[test]
fn a_name_property_descriptor_is_not_writable_or_enumerable() {
    assert_eq!(
        text(
            "var d = Object.getOwnPropertyDescriptor(({ id: function() {} }).id, 'name');
             [d.value, d.writable, d.enumerable, d.configurable].join()"
        ),
        "id,false,false,true"
    );
}
