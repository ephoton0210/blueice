// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Number.parseFloat` and `Number.parseInt` are the global functions.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// Expects `true`; a script may return a string describing the first failure.
fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn number_parse_functions_are_the_global_functions() {
    assert_true(
        r#"(function() {
          for (const name of ["parseFloat", "parseInt"]) {
            if (Number[name] !== globalThis[name]) return "Number." + name + " is not the global function";
            const d = Object.getOwnPropertyDescriptor(Number, name);
            if (!d.writable || d.enumerable || !d.configurable) return name + " attributes";
            try { new Number[name]("1"); return name + " is a constructor"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          if (parseInt.length !== 2 || parseFloat.length !== 1) return "lengths " + parseInt.length + " " + parseFloat.length;
          if (parseInt.name !== "parseInt" || parseFloat.name !== "parseFloat") return "names";
          return true;
        })()"#,
    );
}
