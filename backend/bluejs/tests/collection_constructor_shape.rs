// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor shape of `Map`, `Set`, `Promise` and `Symbol`.
//!
//! `Map.length` and `Set.length` are 0 (the iterable is an optional
//! parameter), `Map`, `Set` and `Promise` each expose the `@@species`
//! accessor ECMA-262 defines on them, and `Symbol` has [[Construct]] even
//! though calling it that way always throws.

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
fn map_and_set_have_length_zero() {
    assert_true(
        r#"(function() {
          for (const C of [Map, Set, WeakMap, WeakSet]) {
            const d = Object.getOwnPropertyDescriptor(C, "length");
            if (d.value !== 0 || d.writable || d.enumerable || !d.configurable) {
              return C.name + ".length is not 0 / non-writable / non-enumerable / configurable";
            }
          }
          return true;
        })()"#,
    );
}

#[test]
fn map_set_and_promise_expose_the_species_accessor() {
    assert_true(
        r#"(function() {
          for (const C of [Map, Set, Promise, Array, RegExp]) {
            const d = Object.getOwnPropertyDescriptor(C, Symbol.species);
            if (!d || typeof d.get !== "function" || d.set !== undefined) {
              return C.name + " has no @@species getter";
            }
            if (d.enumerable || !d.configurable) return C.name + " @@species attributes";
            if (d.get.name !== "get [Symbol.species]" || d.get.length !== 0) {
              return C.name + " @@species getter has name " + d.get.name;
            }
            if (C[Symbol.species] !== C) return C.name + "[@@species] is not the constructor";
            if (d.get.call(42) !== 42) return C.name + " @@species getter does not return `this`";
            // A getter is a plain function: it is not a constructor.
            try { new d.get(); return C.name + " @@species getter is a constructor"; }
            catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          class Sub extends Map {}
          if (Sub[Symbol.species] !== Sub) return "subclass @@species does not follow `this`";
          return true;
        })()"#,
    );
}

#[test]
fn symbol_is_a_constructor_that_always_throws_when_constructed() {
    assert_true(
        r#"(function() {
          class Sub extends Symbol {}
          try { new Symbol(); return "new Symbol() did not throw"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error for new Symbol()"; }
          try { new Sub(); return "new Sub() did not throw"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error for new Sub()"; }
          // Reflect.construct only checks [[Construct]] (IsConstructor(newTarget)) first.
          try { Reflect.construct(function() {}, [], Symbol); }
          catch (e) { return "Symbol is not accepted as a newTarget: " + e; }
          return true;
        })()"#,
    );
}
