// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Method definitions in object literals create *enumerable* properties
//! (ECMA-262 13.2.5.5 / DefineMethodProperty with enumerable = true), unlike
//! class methods. The literal-method opcode had no operand slot, so the
//! enumerable flag the compiler emitted was silently dropped.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let value = Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    match value {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn object_literal_methods_are_enumerable_and_class_methods_are_not() {
    assert_true(
        r#"(function() {
          var o = { foo() {}, async bar() {}, *baz() {}, async *qux() {}, ["comp" + "uted"]() {}, plain: 1 };
          var keys = Object.keys(o).join();
          if (keys !== "foo,bar,baz,qux,computed,plain") return "keys: " + keys;
          var d = Object.getOwnPropertyDescriptor(o, "foo");
          if (!d.writable || !d.enumerable || !d.configurable) return "descriptor";
          var copy = Object.assign({}, { m() { return 1; } });
          if (typeof copy.m !== "function") return "Object.assign drops methods";
          var spread = { ...{ m() {} } };
          if (typeof spread.m !== "function") return "spread drops methods";
          var seen = [];
          for (var key in { a() {}, b() {} }) seen.push(key);
          if (seen.join() !== "a,b") return "for-in";
          class C { m() {} static s() {} }
          if (Object.keys(C.prototype).length !== 0 || Object.keys(C).length !== 0) return "class methods became enumerable";
          return true;
        })()"#,
    );
}
