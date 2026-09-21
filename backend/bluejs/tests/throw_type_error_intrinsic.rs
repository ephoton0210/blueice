// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `%ThrowTypeError%` (ECMA-262 10.2.4.1): a frozen anonymous function whose
//! `length` (0) and `name` ("") are non-configurable, defined in that order.

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
fn throw_type_error_is_frozen_with_length_before_name() {
    assert_true(
        r#"(function() {
          var thrower = Object.getOwnPropertyDescriptor(function() { "use strict"; return arguments; }(), "callee").get;
          if (Object.getOwnPropertyNames(thrower).join() !== "length,name") return "own keys: " + Object.getOwnPropertyNames(thrower).join();
          var length = Object.getOwnPropertyDescriptor(thrower, "length");
          var name = Object.getOwnPropertyDescriptor(thrower, "name");
          if (length.value !== 0 || length.writable || length.enumerable || length.configurable) return "length";
          if (name.value !== "" || name.writable || name.enumerable || name.configurable) return "name";
          if (Object.isExtensible(thrower) || !Object.isFrozen(thrower)) return "not frozen";
          try { thrower(); return "did not throw"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          var callerThrower = Object.getOwnPropertyDescriptor(Function.prototype, "caller").get;
          if (callerThrower !== thrower) return "not the same %ThrowTypeError% as Function.prototype.caller";
          return true;
        })()"#,
    );
}
