// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The seven `Set.prototype` set-algebra methods (ECMA-262 24.2.4):
//! `union`, `intersection`, `difference`, `symmetricDifference`,
//! `isSubsetOf`, `isSupersetOf` and `isDisjointFrom`, and the GetSetRecord
//! protocol every one of them applies to its argument.

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
fn the_methods_have_their_specified_shape() {
    assert_true(
        r#"(function() {
          const lengths = {
            union: 1, intersection: 1, difference: 1, symmetricDifference: 1,
            isSubsetOf: 1, isSupersetOf: 1, isDisjointFrom: 1,
          };
          for (const name of Object.keys(lengths)) {
            const d = Object.getOwnPropertyDescriptor(Set.prototype, name);
            if (!d || typeof d.value !== "function") return name + " missing";
            if (d.value.name !== name || d.value.length !== lengths[name]) return name + " name/length";
            if (!d.writable || d.enumerable || !d.configurable) return name + " attributes";
            try { new d.value(new Set()); return name + " is a constructor"; }
            catch (e) { if (!(e instanceof TypeError)) return name + " wrong error"; }
            for (const bad of [undefined, null, 1, {}, new Map(), new WeakSet(), []]) {
              try { d.value.call(bad, new Set()); return name + " brand check"; }
              catch (e) { if (!(e instanceof TypeError)) return name + " wrong brand error"; }
            }
          }
          return true;
        })()"#,
    );
}

#[test]
fn the_algebra_over_two_sets_matches_the_specified_results_and_order() {
    assert_true(
        r#"(function() {
          const j = (s) => [...s].join();
          const a = new Set([1, 2, 3, 4]);
          const b = new Set([3, 4, 5, 6]);
          if (j(a.union(b)) !== "1,2,3,4,5,6") return "union " + j(a.union(b));
          if (j(a.intersection(b)) !== "3,4") return "intersection " + j(a.intersection(b));
          if (j(a.difference(b)) !== "1,2") return "difference";
          if (j(a.symmetricDifference(b)) !== "1,2,5,6") return "symmetricDifference " + j(a.symmetricDifference(b));
          if (a.isSubsetOf(b) || !new Set([3]).isSubsetOf(b) || !new Set().isSubsetOf(b)) return "isSubsetOf";
          if (!b.isSupersetOf(new Set([3, 6])) || b.isSupersetOf(a) || !a.isSupersetOf(new Set())) return "isSupersetOf";
          if (a.isDisjointFrom(b) || !a.isDisjointFrom(new Set([9]))) return "isDisjointFrom";
          // The result is a fresh plain Set, never a subclass instance or `this`.
          class Sub extends Set {}
          const r = new Sub([1]).union(new Set([2]));
          if (Object.getPrototypeOf(r) !== Set.prototype || r === a) return "result species";
          if (a.union(a) === a) return "same set returned";
          // Intersection follows the receiver's order when it is no larger than the argument,
          // and the argument's order when the receiver is larger.
          if (j(new Set([1, 2, 3]).intersection(new Set([3, 2, 1, 0]))) !== "1,2,3") return "intersection order (this <= other)";
          if (j(new Set([1, 2, 3]).intersection(new Set([3, 2]))) !== "3,2") return "intersection order (this > other)";
          // -0 is canonicalized to +0 wherever a value comes from the other set.
          const z = new Set([1]).union(new Set([-0]));
          if (!Object.is([...z][1], 0)) return "-0 in union";
          return true;
        })()"#,
    );
}

#[test]
fn set_like_arguments_are_read_through_size_has_and_keys() {
    assert_true(
        r#"(function() {
          const log = [];
          const setLike = {
            get size() { log.push("size"); return 2; },
            get has() { log.push("get has"); return (v) => { log.push("has " + v); return v === 1; }; },
            get keys() { log.push("get keys"); return function() { log.push("keys"); return [1, 9][Symbol.iterator](); }; },
          };
          const s = new Set([1, 2, 3]);
          log.length = 0;
          const u = s.union(setLike);
          if (log.join() !== "size,get has,get keys,keys") return "union protocol " + log.join();
          if ([...u].join() !== "1,2,3,9") return "union values";
          log.length = 0;
          s.isSubsetOf(setLike);
          if (log.join() !== "size,get has,get keys") return "isSubsetOf reads " + log.join();
          // A plain array is not a set-like (no `size`); an object whose size is NaN is rejected up front.
          for (const bad of [[1], { size: undefined, has() {}, keys() {} }, { size: NaN, has() {}, keys() {} },
                             { size: "x", has() {}, keys() {} }, { size: 1n, has() {}, keys() {} }]) {
            try { s.union(bad); return "accepted a bad set-like"; }
            catch (e) { if (!(e instanceof TypeError)) return "wrong error " + e; }
          }
          try { s.union({ size: -1, has() {}, keys() {} }); return "negative size accepted"; }
          catch (e) { if (!(e instanceof RangeError)) return "negative size must be a RangeError"; }
          // Size is coerced with ToNumber then ToIntegerOrInfinity: fractions truncate, Infinity is allowed.
          s.union({ size: Infinity, has() {}, keys() { return [][Symbol.iterator](); } });
          s.union({ size: 0.5, has() {}, keys() { return [][Symbol.iterator](); } });
          try { s.union({ size: 1, has: 1, keys() {} }); return "non-callable has"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong has error"; }
          try { s.union({ size: 1, has() {}, keys: 1 }); return "non-callable keys"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong keys error"; }
          for (const prim of [1, "s", true, undefined, null, Symbol()]) {
            try { s.union(prim); return "primitive accepted"; }
            catch (e) { if (!(e instanceof TypeError)) return "wrong primitive error"; }
          }
          try { s.union({ size: 1, has() {}, keys() { return 1; } }); return "non-object iterator"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong iterator error"; }
          return true;
        })()"#,
    );
}

#[test]
fn methods_choose_between_probing_has_and_iterating_keys_by_size() {
    assert_true(
        r#"(function() {
          const make = (size, log) => ({
            size,
            has(v) { log.push("has " + v); return [2, 3].includes(v); },
            keys() { log.push("keys"); return [2, 3][Symbol.iterator](); },
          });
          let log = [];
          new Set([1, 2]).intersection(make(5, log));
          if (log.join() !== "has 1,has 2") return "small this probes has: " + log.join();
          log = [];
          new Set([1, 2, 3, 4]).intersection(make(2, log));
          if (log.join() !== "keys") return "large this iterates keys: " + log.join();
          log = [];
          new Set([1, 2]).difference(make(5, log));
          if (log.join() !== "has 1,has 2") return "difference small this: " + log.join();
          log = [];
          new Set([1, 2, 3]).difference(make(1, log));
          if (log.join() !== "keys") return "difference large this: " + log.join();
          log = [];
          if (new Set([1, 2, 3]).isSubsetOf(make(2, log)) !== false || log.length !== 0) return "isSubsetOf size shortcut";
          log = [];
          if (new Set([1]).isSupersetOf(make(2, log)) !== false || log.length !== 0) return "isSupersetOf size shortcut";
          log = [];
          new Set([1, 2]).isDisjointFrom(make(5, log));
          if (log.join() !== "has 1,has 2") return "isDisjointFrom small this: " + log.join();
          return true;
        })()"#,
    );
}

#[test]
fn early_exit_closes_the_keys_iterator() {
    assert_true(
        r#"(function() {
          let closed = 0;
          const other = (values) => ({
            size: values.length,
            has() { return true; },
            keys() {
              let i = 0;
              return { next() { return i < values.length ? { done: false, value: values[i++] } : { done: true }; },
                       return() { closed++; return {}; } };
            },
          });
          if (new Set([1, 2, 3]).isSupersetOf(other([1, 9])) !== false || closed !== 1) return "isSupersetOf close " + closed;
          closed = 0;
          if (new Set([1, 2, 3, 4]).isDisjointFrom(other([9, 3])) !== false || closed !== 1) return "isDisjointFrom close " + closed;
          closed = 0;
          if (new Set([1, 2, 3]).isSupersetOf(other([1, 2])) !== true || closed !== 0) return "no close on exhaustion";
          return true;
        })()"#,
    );
}

#[test]
fn mutation_of_the_receiver_during_has_is_observed_like_the_specification() {
    assert_true(
        r#"(function() {
          // isSubsetOf: growth of the receiver during `has` extends the walk.
          const receiver = new Set([1, 2]);
          const seen = [];
          const other = {
            size: 10,
            has(v) { seen.push(v); if (v === 1) receiver.add(3); return true; },
            keys() { return [][Symbol.iterator](); },
          };
          receiver.isSubsetOf(other);
          if (seen.join() !== "1,2,3") return "growth during has: " + seen.join();
          // difference works on a copy: deleting from the receiver does not change which elements are probed.
          const r2 = new Set([1, 2, 3]);
          const seen2 = [];
          const res = r2.difference({
            size: 10,
            has(v) { seen2.push(v); if (v === 1) r2.delete(2); return v === 3; },
            keys() { return [][Symbol.iterator](); },
          });
          if (seen2.join() !== "1,2,3" || [...res].join() !== "1,2") return "difference copy " + seen2.join() + " / " + [...res].join();
          // intersection removes and re-adds an element during `has`: it is appended once.
          const r3 = new Set([1, 2]);
          let churned = false;
          const res3 = r3.intersection({
            size: 10,
            has(v) { if (v === 1 && !churned) { churned = true; r3.delete(1); r3.add(1); } return true; },
            keys() { return [][Symbol.iterator](); },
          });
          if ([...res3].join() !== "1,2" && [...res3].join() !== "2,1") return "re-add " + [...res3].join();
          return true;
        })()"#,
    );
}

#[test]
fn symmetric_difference_handles_duplicates_from_the_other_set() {
    assert_true(
        r#"(function() {
          const a = new Set([1, 2, 3]);
          // A keys() that repeats a value: 4 is added, then a second 4 is "already in result" and not in this: kept once.
          // 1 is in this: first occurrence removes it, the repeat is a no-op removal.
          const other = { size: 5, has() {}, keys() { return [4, 4, 1, 1, 9][Symbol.iterator](); } };
          const r = a.symmetricDifference(other);
          if ([...r].join() !== "2,3,4,9") return "symmetricDifference " + [...r].join();
          return true;
        })()"#,
    );
}
