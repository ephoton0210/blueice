// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Map.groupBy` (ECMA-262 24.1.2.1) and the upsert methods
//! `Map.prototype.getOrInsert` / `getOrInsertComputed`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn evaluate_err(source: &str) -> RuntimeError {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .expect_err(&format!("{source}\n  -> expected an error, got a value"))
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
fn the_methods_have_their_specified_shape() {
    assert_true(
        r#"(function() {
          const check = (object, key, length) => {
            const d = Object.getOwnPropertyDescriptor(object, key);
            if (!d || typeof d.value !== "function") return key + " missing";
            if (d.value.name !== key || d.value.length !== length) return key + " name/length";
            if (!d.writable || d.enumerable || !d.configurable) return key + " attributes";
            try { new d.value(); return key + " is a constructor"; }
            catch (e) { if (!(e instanceof TypeError)) return key + " wrong error"; }
            return null;
          };
          return check(Map, "groupBy", 2) || check(Map.prototype, "getOrInsert", 2)
            || check(Map.prototype, "getOrInsertComputed", 2) || true;
        })()"#,
    );
}

#[test]
fn group_by_collects_values_in_first_seen_key_order() {
    assert_true(
        r#"(function() {
          const map = Map.groupBy([1, 2, 3, 4, 5], (n) => (n % 2 ? "odd" : "even"));
          if (!(map instanceof Map) || Object.getPrototypeOf(map) !== Map.prototype) return "not a Map";
          if ([...map.keys()].join() !== "odd,even") return "key order " + [...map.keys()].join();
          if (map.get("odd").join() !== "1,3,5" || map.get("even").join() !== "2,4") return "groups";
          if (!Array.isArray(map.get("odd"))) return "group is not an array";
          // Keys keep their identity (no property-key coercion); -0 becomes +0; NaN groups together.
          const m = Map.groupBy([1, 2, 3], (n) => (n === 1 ? -0 : n === 2 ? 0 : NaN));
          if (m.size !== 2 || !Object.is([...m.keys()][0], 0)) return "-0 handling";
          const o = {};
          const objects = Map.groupBy([1, 2], () => o);
          if (objects.get(o).length !== 2) return "object keys";
          const n = Map.groupBy("abca", (c) => c);
          if (n.get("a").length !== 2) return "string iteration";
          if (Map.groupBy([], () => 0).size !== 0) return "empty";
          return true;
        })()"#,
    );
}

#[test]
fn group_by_passes_value_and_index_and_closes_the_iterator_on_throw() {
    assert_true(
        r#"(function() {
          const seen = [];
          Map.groupBy(["a", "b"], function(v, i) { seen.push(v + i, this === undefined || this === globalThis); return 0; });
          if (seen.join() !== "a0,true,b1,true") return "callback arguments " + seen.join();
          let closed = 0;
          const iterable = {
            [Symbol.iterator]() {
              return { next() { return { done: false, value: 1 }; }, return() { closed++; return {}; } };
            },
          };
          try { Map.groupBy(iterable, () => { throw new RangeError("x"); }); return "no throw"; }
          catch (e) { if (!(e instanceof RangeError) || closed !== 1) return "closing on callback throw"; }
          try { Map.groupBy([], 1); return "non-callable accepted"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          try { Map.groupBy(undefined, () => 0); return "undefined accepted"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error for undefined items"; }
          return true;
        })()"#,
    );
}

#[test]
fn get_or_insert_returns_the_existing_or_newly_inserted_value() {
    assert_true(
        r#"(function() {
          const m = new Map([["a", 1]]);
          if (m.getOrInsert("a", 2) !== 1) return "existing value";
          if (m.getOrInsert("b", 3) !== 3 || m.get("b") !== 3) return "inserted value";
          const zero = new Map();
          zero.getOrInsert(-0, "z");
          if (!Object.is([...zero.keys()][0], 0)) return "-0 key is not normalized";
          if (zero.getOrInsert(0, "other") !== "z") return "+0 lookup";
          // Insertion order and NaN handling.
          zero.getOrInsert(NaN, 1);
          if (zero.getOrInsert(NaN, 2) !== 1) return "NaN";
          for (const bad of [undefined, null, 1, "s", {}, new Set(), new WeakMap()]) {
            try { Map.prototype.getOrInsert.call(bad, 1, 2); return "brand check"; }
            catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          return true;
        })()"#,
    );
}

#[test]
fn get_or_insert_computed_calls_the_callback_only_for_a_missing_key() {
    assert_true(
        r#"(function() {
          const m = new Map([["a", 1]]);
          let calls = 0;
          const cb = function(key) { calls++; args = [key, this === undefined || this === globalThis, arguments.length]; return key + "!"; };
          let args;
          if (m.getOrInsertComputed("a", cb) !== 1 || calls !== 0) return "present key must not call the callback";
          if (m.getOrInsertComputed("b", cb) !== "b!" || m.get("b") !== "b!") return "computed value";
          if (args.join() !== "b,true,1") return "callback arguments " + args.join();
          // The canonical key (-0 -> +0) reaches the callback.
          let received;
          m.getOrInsertComputed(-0, (k) => { received = k; return 1; });
          if (!Object.is(received, 0)) return "callback received -0";
          // A callback that inserts the same key first is overwritten.
          m.getOrInsertComputed("c", () => { m.set("c", "callback"); return "final"; });
          if (m.get("c") !== "final") return "mutation from the callback was kept";
          // A throwing callback leaves the map untouched.
          const before = m.size;
          try { m.getOrInsertComputed("d", () => { throw new EvalError("x"); }); return "no throw"; }
          catch (e) { if (!(e instanceof EvalError) || m.size !== before || m.has("d")) return "throwing callback"; }
          // The callable check precedes the lookup, even for a present key.
          try { m.getOrInsertComputed("a", 1); return "non-callable accepted"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          return true;
        })()"#,
    );
}

#[test]
fn upsert_methods_reject_a_non_map_receiver() {
    assert!(matches!(
        evaluate_err("Map.prototype.getOrInsertComputed.call(new Set(), 1, () => 1)"),
        RuntimeError::TypeError(_)
    ));
}
