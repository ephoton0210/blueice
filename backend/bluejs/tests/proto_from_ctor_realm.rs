// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! GetPrototypeFromConstructor falls back to the intrinsic of the *new
//! target's* realm when its `prototype` is not an object. String, Promise,
//! RegExp and the four function-kind constructors join the constructors that
//! already did, including a constructor of another realm called with a
//! third realm's new target.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let value = vm
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    match value {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn intrinsic_constructors_take_the_fallback_prototype_from_the_new_targets_realm() {
    assert_true(
        r#"(function() {
          var other = $262.createRealm().global;
          function newTargetWithout(prototype) {
            var C = new other.Function();
            C.prototype = prototype;
            return C;
          }
          var GeneratorFunction = Object.getPrototypeOf(function*() {}).constructor;
          var AsyncFunction = Object.getPrototypeOf(async function() {}).constructor;
          var AsyncGeneratorFunction = Object.getPrototypeOf(async function*() {}).constructor;
          var otherGeneratorFunction = other.eval("(0, function*() {})").constructor;
          var otherAsyncFunction = other.eval("(0, async function() {})").constructor;
          var otherAsyncGeneratorFunction = other.eval("(0, async function*() {})").constructor;
          var cases = [
            ["String", String, [], other.String.prototype],
            ["Promise", Promise, [function() {}], other.Promise.prototype],
            ["RegExp", RegExp, [], other.RegExp.prototype],
            ["GeneratorFunction", GeneratorFunction, [], otherGeneratorFunction.prototype],
            ["AsyncFunction", AsyncFunction, [], otherAsyncFunction.prototype],
            ["AsyncGeneratorFunction", AsyncGeneratorFunction, [], otherAsyncGeneratorFunction.prototype],
          ];
          for (var [name, constructor, args, expected] of cases) {
            for (var value of [undefined, null, 1, "s", Symbol()]) {
              var made = Reflect.construct(constructor, args, newTargetWithout(value));
              if (Object.getPrototypeOf(made) !== expected) return name + " with prototype " + String(typeof value);
            }
            // An object-valued prototype, and a same-realm new target, are unaffected.
            var custom = {};
            var C = function() {}.bind();
            C.prototype = custom;
            if (Object.getPrototypeOf(Reflect.construct(constructor, args, C)) !== custom) return name + " custom prototype";
          }
          return true;
        })()"#,
    );
}

#[test]
fn a_foreign_dynamic_function_constructor_uses_the_new_targets_realm_for_its_prototype() {
    assert_true(
        r#"(function() {
          var realmA = $262.createRealm().global;
          var realmB = $262.createRealm().global;
          var aGeneratorFunction = realmA.eval("(function* () {})").constructor;
          var bGeneratorFunction = realmB.eval("(function* () {})").constructor;
          var aGeneratorPrototype = Object.getPrototypeOf(realmA.eval("(function* () {})").prototype);
          var newTarget = new realmB.Function();
          newTarget.prototype = null;
          var fn = Reflect.construct(aGeneratorFunction, ["calls += 1;"], newTarget);
          if (Object.getPrototypeOf(fn) !== bGeneratorFunction.prototype) return "function prototype";
          if (Object.getPrototypeOf(fn.prototype) !== aGeneratorPrototype) return "generator prototype stays in the callee realm";
          return true;
        })()"#,
    );
}
