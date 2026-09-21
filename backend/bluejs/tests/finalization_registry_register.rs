// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `FinalizationRegistry.prototype.register`: an explicit `undefined`
//! unregister token is the same as none, and only a non-registered Symbol or
//! an Object may be a target or token.

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
fn an_explicit_undefined_unregister_token_is_accepted() {
    assert_true(
        r#"(function() {
          var registry = new FinalizationRegistry(function() {});
          var target = {};
          var symbol = Symbol("held");
          if (registry.register(target, 1, undefined) !== undefined) return "object target";
          if (registry.register(symbol, 1, undefined) !== undefined) return "symbol target";
          if (registry.register(Symbol.hasInstance, undefined, undefined) !== undefined) return "well-known symbol target";
          if (registry.register(target, 1, symbol) !== undefined || registry.register(target, 1, target) !== undefined) return "tokens";
          // Values that cannot be held weakly are still rejected, as are registered symbols.
          for (var bad of [[1, undefined, undefined], [target, 1, 5], [Symbol.for("registered"), 1, undefined], [target, 1, Symbol.for("registered")], [target, target, undefined]]) {
            try { registry.register(bad[0], bad[1], bad[2]); return "accepted " + String(bad[0]); }
            catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          if (registry.unregister(target) !== true) return "unregister";
          return true;
        })()"#,
    );
}
