// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Map` and `Set` iteration (ECMA-262 24.1/24.2 and 24.3/24.4).
//!
//! The collections stored their entries in an insertion-ordered structure
//! designed for live iteration, but never exposed it: `Set.prototype.values`,
//! `keys`, `entries`, `forEach` and `clear` (and the `Map` equivalents) did not
//! exist, and `[Symbol.iterator]` returned an iterator whose `next` reported
//! "done" immediately. So `[...new Set([1, 2])]` was empty and
//! `new Set(iterable).values().next()` threw `TypeError: value is not
//! callable` -- which is how Test262's Temporal `toLocaleString` calendar
//! fixtures (`calendars.values().next().value`) first failed.

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

fn assert_string(source: &str, expected: &str) {
    match evaluate(source) {
        Value::String(actual) => assert_eq!(actual.to_utf8().unwrap(), expected, "{source}"),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

fn assert_type_error(source: &str) {
    assert!(
        matches!(evaluate_err(source), RuntimeError::TypeError(_)),
        "{source}"
    );
}

/// The methods exist with the specified identity, name, length and attributes.
#[test]
fn the_iteration_methods_have_their_specified_shape() {
    assert_true(
        r#"(function() {
          const check = (object, key, name, length) => {
            const d = Object.getOwnPropertyDescriptor(object, key);
            if (!d || typeof d.value !== "function") return name + " missing";
            if (d.value.name !== name) return name + " has name " + d.value.name;
            if (d.value.length !== length) return name + " has length " + d.value.length;
            if (!d.writable || d.enumerable || !d.configurable) return name + " attributes";
            return null;
          };
          const problems = [
            check(Set.prototype, "values", "values", 0),
            check(Set.prototype, "entries", "entries", 0),
            check(Set.prototype, "forEach", "forEach", 1),
            check(Set.prototype, "clear", "clear", 0),
            check(Set.prototype, Symbol.iterator, "values", 0),
            check(Map.prototype, "keys", "keys", 0),
            check(Map.prototype, "values", "values", 0),
            check(Map.prototype, "entries", "entries", 0),
            check(Map.prototype, "forEach", "forEach", 1),
            check(Map.prototype, "clear", "clear", 0),
            check(Map.prototype, Symbol.iterator, "entries", 0),
          ].filter((problem) => problem !== null);
          if (problems.length) return problems.join("; ");
          return Set.prototype.keys === Set.prototype.values
              && Set.prototype[Symbol.iterator] === Set.prototype.values
              && Map.prototype[Symbol.iterator] === Map.prototype.entries
              && Set.prototype.entries !== Set.prototype.values
              && Map.prototype.keys !== Map.prototype.values
              && Map.prototype.values !== Map.prototype.entries
              ? true : "identity";
        })()"#,
    );
}

/// The iterator prototypes chain to `%IteratorPrototype%` and carry their tags.
#[test]
fn the_iterators_have_the_specified_prototypes() {
    assert_true(
        r#"(function() {
          const setIterator = new Set([1]).values();
          const mapIterator = new Map([[1, 2]]).entries();
          const iteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));
          const setProto = Object.getPrototypeOf(setIterator);
          const mapProto = Object.getPrototypeOf(mapIterator);
          if (Object.getPrototypeOf(setProto) !== iteratorPrototype) return "set chain";
          if (Object.getPrototypeOf(mapProto) !== iteratorPrototype) return "map chain";
          if (setProto === mapProto) return "shared prototype";
          if (setProto[Symbol.toStringTag] !== "Set Iterator") return "set tag";
          if (mapProto[Symbol.toStringTag] !== "Map Iterator") return "map tag";
          if (Object.prototype.toString.call(setIterator) !== "[object Set Iterator]") return "set string";
          if (setProto.next.length !== 0 || setProto.next.name !== "next") return "next shape";
          if (setIterator[Symbol.iterator]() !== setIterator) return "self iterator";
          return Object.getOwnPropertyNames(setProto).sort().join() === "next" ? true : "own names";
        })()"#,
    );
}

#[test]
fn a_set_iterates_in_insertion_order() {
    assert_string("[...new Set([3, 1, 2, 1, 3])].join()", "3,1,2");
    assert_string("[...new Set([3, 1, 2]).values()].join()", "3,1,2");
    assert_string("[...new Set([3, 1, 2]).keys()].join()", "3,1,2");
    assert_string(
        "[...new Set(['a', 'b']).entries()].map((entry) => entry.join('|')).join()",
        "a|a,b|b",
    );
    assert_string("Array.from(new Set(['x', 'y'])).join()", "x,y");
    assert_string(
        "(function() { const out = []; for (const v of new Set([5, 6])) out.push(v); return out.join(); })()",
        "5,6",
    );
    assert_string(
        "(function() { const [a, b] = new Set([7, 8, 9]); return a + ',' + b; })()",
        "7,8",
    );
}

/// `SameValueZero`: `NaN` is one entry, `-0` is stored as `+0`.
#[test]
fn a_set_uses_same_value_zero() {
    assert_true(
        r#"(function() {
          const values = [...new Set([0, -0, NaN, NaN, "0"])];
          return values.length === 3 && Object.is(values[0], 0) && Number.isNaN(values[1])
              && values[2] === "0" ? true : values.length;
        })()"#,
    );
}

#[test]
fn a_map_iterates_in_insertion_order() {
    let map = "new Map([['a', 1], ['b', 2], ['c', 3]])";
    assert_string(
        &format!("[...{map}].map((entry) => entry.join('=')).join()"),
        "a=1,b=2,c=3",
    );
    assert_string(
        &format!("[...{map}.entries()].map((entry) => entry.join('=')).join()"),
        "a=1,b=2,c=3",
    );
    assert_string(&format!("[...{map}.keys()].join()"), "a,b,c");
    assert_string(&format!("[...{map}.values()].join()"), "1,2,3");
    assert_string(
        &format!(
            "(function() {{ const out = []; for (const [k, v] of {map}) out.push(k + v); return out.join(); }})()"
        ),
        "a1,b2,c3",
    );
    // Updating an existing key keeps its position; deleting and re-adding moves it.
    assert_string(
        "(function() { const m = new Map([['a', 1], ['b', 2]]); m.set('a', 9); \
         return [...m.values()].join(); })()",
        "9,2",
    );
    assert_string(
        "(function() { const m = new Map([['a', 1], ['b', 2]]); m.delete('a'); m.set('a', 1); \
         return [...m.keys()].join(); })()",
        "b,a",
    );
}

/// The iterators track the collection *live*, as the specification requires.
#[test]
fn iteration_observes_additions_and_deletions_made_while_iterating() {
    // An entry added during iteration is visited.
    assert_string(
        "(function() { const s = new Set([1]); const seen = []; \
         for (const v of s) { seen.push(v); if (v < 4) s.add(v + 1); } return seen.join(); })()",
        "1,2,3,4",
    );
    // An entry deleted before it is reached is skipped.
    assert_string(
        "(function() { const s = new Set([1, 2, 3]); const seen = []; \
         for (const v of s) { seen.push(v); if (v === 1) s.delete(2); } return seen.join(); })()",
        "1,3",
    );
    // Deleting and re-adding moves an entry to the end, so it is visited again.
    assert_string(
        "(function() { const s = new Set([1, 2]); const seen = []; let moved = false; \
         for (const v of s) { seen.push(v); if (v === 1 && !moved) { moved = true; s.delete(1); s.add(1); } } \
         return seen.join(); })()",
        "1,2,1",
    );
    assert_string(
        "(function() { const m = new Map([[1, 'a'], [2, 'b']]); const seen = []; \
         for (const [k] of m) { seen.push(k); if (k === 1) m.set(3, 'c'); } return seen.join(); })()",
        "1,2,3",
    );
    // `clear` during iteration ends it, but entries added afterwards are visited.
    assert_string(
        "(function() { const s = new Set([1, 2, 3]); const seen = []; \
         for (const v of s) { seen.push(v); if (v === 1) { s.clear(); s.add(9); } } return seen.join(); })()",
        "1,9",
    );
}

/// Once an iterator reports done it stays done, even if entries arrive later.
#[test]
fn an_exhausted_iterator_stays_exhausted() {
    assert_true(
        r#"(function() {
          const s = new Set([1]);
          const it = s.values();
          if (it.next().value !== 1) return "first";
          const done = it.next();
          if (done.done !== true || done.value !== undefined) return "done record";
          s.add(2);
          const again = it.next();
          return again.done === true && again.value === undefined ? true : "revived";
        })()"#,
    );
    assert_true(
        r#"(function() {
          const m = new Map();
          const it = m.entries();
          const first = it.next();
          m.set(1, 2);
          return first.done === true && it.next().done === true ? true : "revived";
        })()"#,
    );
}

/// Every step returns a fresh `{ value, done }` record.
#[test]
fn iteration_results_are_fresh_records() {
    assert_true(
        r#"(function() {
          const it = new Set([1, 2]).values();
          const a = it.next();
          const b = it.next();
          return a !== b && Object.keys(a).join() === "value,done" && a.done === false ? true : "records";
        })()"#,
    );
}

#[test]
fn for_each_visits_entries_with_value_key_and_collection() {
    assert_string(
        "(function() { const out = []; new Set([1, 2]).forEach(function (v, k, s) { \
         out.push(v + ':' + k + ':' + (s instanceof Set) + ':' + (this.tag)); }, { tag: 'T' }); \
         return out.join(); })()",
        "1:1:true:T,2:2:true:T",
    );
    assert_string(
        "(function() { const out = []; new Map([['a', 1], ['b', 2]]).forEach(function (v, k, m) { \
         out.push(k + v + (m instanceof Map)); }); return out.join(); })()",
        "a1true,b2true",
    );
    // Callbacks observe live mutations and `forEach` returns undefined.
    assert_string(
        "(function() { const s = new Set([1]); const seen = []; \
         const result = s.forEach((v) => { seen.push(v); if (v < 3) s.add(v + 1); }); \
         return seen.join() + '/' + result; })()",
        "1,2,3/undefined",
    );
    // An entry the callback deletes before it is reached is skipped, and one it
    // deletes and re-adds is visited again at its new position; `clear` ends
    // the loop for the entries that were still ahead.
    assert_string(
        "(function() { const s = new Set([1, 2, 3]); const seen = []; \
         s.forEach((v) => { seen.push(v); if (v === 1) s.delete(2); }); return seen.join(); })()",
        "1,3",
    );
    assert_string(
        "(function() { const m = new Map([['a', 1], ['b', 2]]); const seen = []; let again = true; \
         m.forEach((v, k) => { seen.push(k); if (k === 'a' && again) { again = false; m.delete('a'); m.set('a', 9); } }); \
         return seen.join(); })()",
        "a,b,a",
    );
    assert_string(
        "(function() { const s = new Set([1, 2, 3]); const seen = []; \
         s.forEach((v) => { seen.push(v); if (v === 1) s.clear(); }); return seen.join(); })()",
        "1",
    );
    // An empty collection never calls the callback.
    assert_string(
        "(function() { let called = false; new Set().forEach(() => { called = true; }); \
         new Map().forEach(() => { called = true; }); return String(called); })()",
        "false",
    );
}

#[test]
fn for_each_requires_a_callable_before_iterating() {
    assert_type_error("new Set([1]).forEach()");
    assert_type_error("new Set([1]).forEach({})");
    assert_type_error("new Map([[1, 2]]).forEach(1)");
    // The check happens even for an empty collection.
    assert_type_error("new Set().forEach(null)");
    assert_type_error("new Map().forEach(undefined)");
}

/// An exception thrown by the callback propagates and stops iteration.
#[test]
fn for_each_propagates_callback_exceptions() {
    assert_string(
        "(function() { let n = 0; try { new Set([1, 2, 3]).forEach(() => { n++; throw new RangeError('x'); }); } \
         catch (e) { return e.name + n; } return 'none'; })()",
        "RangeError1",
    );
}

#[test]
fn clear_empties_the_collection() {
    assert_true(
        r#"(function() {
          const s = new Set([1, 2, 3]);
          const result = s.clear();
          if (result !== undefined || s.size !== 0 || s.has(1)) return "set clear";
          s.add(4);
          if ([...s].join() !== "4") return "set reuse";
          const m = new Map([[1, 2]]);
          if (m.clear() !== undefined || m.size !== 0 || m.has(1) || m.get(1) !== undefined) return "map clear";
          m.set(5, 6);
          return [...m].join() === "5,6" ? true : "map reuse";
        })()"#,
    );
}

/// The receiver must be a real `Map`/`Set`, and each iterator's `next` only
/// accepts its own kind of iterator.
#[test]
fn methods_check_their_receivers() {
    for source in [
        "Set.prototype.values.call({})",
        "Set.prototype.values.call(new Map())",
        "Set.prototype.entries.call([])",
        "Set.prototype.forEach.call({}, () => {})",
        "Set.prototype.clear.call(new Map())",
        "Map.prototype.keys.call(new Set())",
        "Map.prototype.values.call({})",
        "Map.prototype.entries.call(new Set())",
        "Map.prototype.forEach.call(new Set(), () => {})",
        "Map.prototype.clear.call({})",
        "Set.prototype.values.call(undefined)",
        "Map.prototype[Symbol.iterator].call(1)",
    ] {
        assert_type_error(source);
    }
    for source in [
        "new Set().values().next.call({})",
        "new Set().values().next.call(new Map().entries())",
        "new Map().entries().next.call(new Set().values())",
        "new Set().values().next.call([][Symbol.iterator]())",
        "new Map().keys().next.call(undefined)",
    ] {
        assert_type_error(source);
    }
    // `next` is not tied to the iterator it was read from: it steps whichever
    // Map iterator it is called on, in that iterator's own kind (values -> 4).
    assert_true("new Map([[1, 2]]).keys().next.call(new Map([[3, 4]]).values()).value === 4");
}

/// The collection constructors now consume iterables produced by other
/// collections (previously an empty iteration).
#[test]
fn constructors_accept_other_collections() {
    assert_true(
        r#"(function() {
          const source = new Set([1, 2, 3]);
          const copy = new Set(source);
          const fromMap = new Map(new Map([["a", 1], ["b", 2]]));
          const keys = new Set(new Map([["a", 1], ["b", 2]]).keys());
          return copy.size === 3 && copy.has(2) && fromMap.get("b") === 2 && keys.has("a") && keys.size === 2
              ? true : copy.size + "/" + fromMap.size + "/" + keys.size;
        })()"#,
    );
    assert_true(
        r#"(function() {
          const [first, ...rest] = new Set(["x", "y", "z"]);
          return first === "x" && rest.join() === "y,z" && Math.max(...new Set([3, 9, 4])) === 9;
        })()"#,
    );
}

/// Iterating a collection reachable only through its iterator survives
/// garbage collection while other allocation churns.
#[test]
fn an_iterator_keeps_its_collection_alive_across_collections() {
    assert_true(
        r#"(function() {
          const make = () => {
            const s = new Set();
            for (let i = 0; i < 300; i++) s.add({ id: i });
            return s.values();
          };
          const it = make();
          for (let round = 0; round < 40; round++) {
            const junk = [];
            for (let i = 0; i < 400; i++) junk.push({ round, i, text: "garbage" + i });
          }
          let count = 0;
          let last = -1;
          for (let step = it.next(); !step.done; step = it.next()) {
            if (step.value.id !== count) return "order " + step.value.id + " at " + count;
            last = step.value.id;
            count++;
          }
          return count === 300 && last === 299 ? true : "count " + count;
        })()"#,
    );
    assert_true(
        r#"(function() {
          const it = (() => new Map([[{ k: 1 }, { v: 1 }], [{ k: 2 }, { v: 2 }]]).entries())();
          for (let round = 0; round < 40; round++) {
            const junk = [];
            for (let i = 0; i < 400; i++) junk.push([i, "x" + i]);
          }
          const [[k1, v1], [k2, v2]] = [...it];
          return k1.k === 1 && v1.v === 1 && k2.k === 2 && v2.v === 2;
        })()"#,
    );
}

/// A collection holding many entries iterates all of them, and a large number
/// of deleted entries does not hide the live ones.
#[test]
fn large_collections_iterate_completely() {
    assert_true(
        r#"(function() {
          const s = new Set();
          for (let i = 0; i < 2000; i++) s.add(i);
          for (let i = 0; i < 2000; i += 2) s.delete(i);
          let count = 0, sum = 0;
          for (const v of s) { count++; sum += v; }
          return count === 1000 && sum === 1000000 ? true : count + "/" + sum;
        })()"#,
    );
}
