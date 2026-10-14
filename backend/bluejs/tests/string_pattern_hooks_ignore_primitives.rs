// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `String.prototype.replace`, `replaceAll` and `split` consult
//! `@@replace` / `@@split` only on an Object argument, like `match`,
//! `matchAll` and `search` already did: a hook installed on the wrapper
//! prototype of a primitive argument is never observed.

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
fn primitive_arguments_do_not_observe_wrapper_prototype_hooks() {
    assert_true(
        r#"(function() {
          var hits = [];
          var protos = [[Number.prototype, 1], [Boolean.prototype, true], [String.prototype, "1"], [BigInt.prototype, 1n]];
          for (var [proto, value] of protos) {
            for (var name of ["replace", "split"]) {
              Object.defineProperty(proto, Symbol[name], { configurable: true, get() { hits.push(name); throw new EvalError("hook observed"); } });
            }
            var text = "a" + String(value) + "b" + String(value) + "c";
            if (text.replace(value, "X") !== "aXb" + String(value) + "c") return "replace " + text.replace(value, "X");
            if (text.replaceAll(value, "X") !== "aXbXc") return "replaceAll";
            if (text.split(value).join("|") !== "a|b|c") return "split";
            delete proto[Symbol.replace];
            delete proto[Symbol.split];
          }
          if (hits.length) return "hooks observed: " + hits.join();
          // Objects (including wrappers) still get their hook called, with the specified arguments.
          var calls = [];
          var searcher = { [Symbol.replace](s, r) { calls.push("replace", s, r); return "replaced"; }, [Symbol.split](s, l) { calls.push("split", s, l); return ["split"]; } };
          if ("abc".replace(searcher, "R") !== "replaced" || calls.join() !== "replace,abc,R") return "object hook: " + calls.join();
          if ("abc".split(searcher, 2)[0] !== "split") return "object split hook";
          var boxed = Object(1);
          Number.prototype[Symbol.replace] = function() { return "boxed hook"; };
          if ("a1".replace(boxed, "X") !== "boxed hook") return "wrapper object hook";
          delete Number.prototype[Symbol.replace];
          return true;
        })()"#,
    );
}
