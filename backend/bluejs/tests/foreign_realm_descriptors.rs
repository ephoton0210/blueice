// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Objects of another Test262 realm (`$262.createRealm()`) are facades in the
//! caller's heap. Their [[GetOwnProperty]] and [[DefineOwnProperty]] are
//! forwarded into the owning realm, so descriptor APIs, `Object.keys` and
//! `Reflect.ownKeys` agree with the object's real properties.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn own_property_descriptors_cross_the_realm_boundary() {
    assert_true(
        r#"(function() {
          var other = $262.createRealm().global;
          var name = Object.getOwnPropertyDescriptor(other.Error, "name");
          if (!name || name.value !== "Error" || name.writable || name.enumerable || !name.configurable) return "data descriptor";
          var stack = Object.getOwnPropertyDescriptor(other.Error.prototype, "stack");
          if (!stack || typeof stack.get !== "function" || typeof stack.set !== "function") return "accessor descriptor";
          if (stack.get === Object.getOwnPropertyDescriptor(Error.prototype, "stack").get) return "the two realms share a getter";
          if (Object.getOwnPropertyDescriptor(other.Error.prototype, "definitelyMissing") !== undefined) return "missing property";
          var target = new other.Object();
          target.visible = 1;
          Object.defineProperty(target, "hidden", { value: 2, enumerable: false, configurable: true });
          if (Object.keys(target).join() !== "visible") return "Object.keys: " + Object.keys(target).join();
          if (Reflect.ownKeys(target).join() !== "visible,hidden") return "ownKeys";
          var symbol = Symbol("s");
          target[symbol] = 3;
          if (Object.getOwnPropertyDescriptor(target, symbol).value !== 3) return "symbol key";
          return true;
        })()"#,
    );
}

#[test]
fn define_own_property_is_forwarded_into_the_owning_realm() {
    assert_true(
        r#"(function() {
          var other = $262.createRealm().global;
          var target = new other.Object();
          Object.defineProperty(target, "hidden", { value: 2, enumerable: false, configurable: true });
          var d = Object.getOwnPropertyDescriptor(target, "hidden");
          if (!d || d.value !== 2 || d.enumerable || d.writable || !d.configurable) return "descriptor after defineProperty";
          if (Object.keys(target).length !== 0) return "hidden property is enumerable";
          if (!Object.hasOwn(target, "hidden")) return "not an own property in the owning realm";
          Object.defineProperty(target, "accessor", { get() { return 7; }, configurable: true });
          if (target.accessor !== 7) return "accessor";
          var frozen = new other.Object();
          Object.freeze(frozen);
          try { Object.defineProperty(frozen, "x", { value: 1 }); return "frozen object accepted a property"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          // A detached foreign TypedArray rejects redefinition of its indices.
          var sample = new other.Uint8Array([0]);
          var desc = Object.getOwnPropertyDescriptor(sample, "0");
          if (!desc || desc.value !== 0) return "typed array element descriptor";
          $262.detachArrayBuffer(sample.buffer);
          try { Object.defineProperty(sample, "0", desc); return "detached typed array accepted a redefinition"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error for detached typed array"; }
          return true;
        })()"#,
    );
}
