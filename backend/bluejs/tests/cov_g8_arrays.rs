// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Array.prototype` methods under injected faults: every observable step
//! (Proxy trap, conversion hook, callback) of each method throws in turn, and
//! every allocation and instruction of the same calls runs out in turn.

mod cov_g8_common;

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};
use cov_g8_common::{budget_sweep, check_cases, fault_sweep, heap_sweep, heap_sweep_with};

/// Receivers: a dense array, one with a hole, an array-like and an empty array,
/// all behind fault-injecting Proxies, and an array-like whose `length` is
/// converted through a fault-injecting hook.
const RECEIVERS: [&str; 6] = [
    "P([1, 2, 3])",
    "P([3, undefined, 1])",
    "P([1, , 3])",
    "P({ length: 3, 0: 'a', 1: 'b', 2: 'c' })",
    "P([])",
    "{ length: V(3), 0: 'a', 1: 'b', 2: 'c' }",
];

/// Receivers for the resource sweeps: a genuine array and an array-like.
const PLAIN_RECEIVERS: [&str; 3] = [
    "[3, 1, 2, [4], 5]",
    "[3, undefined, 1, , 2]",
    "{ length: 5, 0: 3, 1: 1, 2: 2, 3: [4], 4: 5 }",
];

/// `Array.prototype` methods with argument lists that reach their conversions.
const CALLS: &[(&str, &str)] = &[
    ("at", "V(1)"),
    ("at", "V(-1)"),
    ("fill", "V(0), V(1), V(2)"),
    ("fill", "0, V(-2), V(-1)"),
    ("push", "1, 2"),
    ("push", ""),
    ("pop", ""),
    ("shift", ""),
    ("unshift", "0, 1"),
    ("unshift", ""),
    ("reverse", ""),
    ("copyWithin", "V(0), V(1), V(3)"),
    ("copyWithin", "V(1), V(0), V(2)"),
    ("copyWithin", "0, -2"),
    ("flat", "V(1)"),
    ("flat", ""),
    ("flatMap", "F(function (x) { return [x, [x]]; })"),
    ("join", "V('-')"),
    ("join", ""),
    ("toLocaleString", ""),
    ("toString", ""),
    ("concat", "P([4]), [5], 6"),
    (
        "concat",
        "{ length: 1, 0: 'x', [Symbol.isConcatSpreadable]: true }",
    ),
    ("forEach", "F(function () {})"),
    ("filter", "F(function (x) { return x !== 2; })"),
    ("map", "F(function (x) { return x; })"),
    ("some", "F(function () { return false; })"),
    ("every", "F(function () { return true; })"),
    ("find", "F(function () { return false; })"),
    ("findIndex", "F(function () { return false; })"),
    ("findLast", "F(function () { return false; })"),
    ("findLastIndex", "F(function () { return false; })"),
    ("reduce", "F(function (a, b) { return a; })"),
    ("reduce", "F(function (a, b) { return a; }), 0"),
    ("reduceRight", "F(function (a, b) { return a; })"),
    ("reduceRight", "F(function (a, b) { return a; }), 0"),
    ("includes", "3, V(1)"),
    ("includes", "V(2)"),
    ("indexOf", "3, V(1)"),
    ("indexOf", "V(2)"),
    ("lastIndexOf", "1, V(-1)"),
    ("lastIndexOf", "V(2)"),
    ("slice", "V(1), V(3)"),
    ("slice", "-2"),
    ("splice", "V(1), V(1), 7, 8"),
    ("splice", "V(0), V(0), 9"),
    ("splice", "V(1)"),
    ("splice", ""),
    ("sort", "F(function (a, b) { return b - a; })"),
    ("sort", "F(function (a, b) { return V(b - a); })"),
    ("sort", ""),
    ("splice", "V(0), V(2)"),
    ("splice", "V(1), V(2), 5"),
    ("fill", "0, V(1)"),
    ("lastIndexOf", "1"),
    ("lastIndexOf", "1, V(Infinity)"),
    ("lastIndexOf", "1, V(-Infinity)"),
    ("lastIndexOf", "1, V(-10)"),
    ("lastIndexOf", "3, V(NaN)"),
    ("toReversed", ""),
    ("toSorted", "F(function (a, b) { return b - a; })"),
    ("toSorted", ""),
    ("toSpliced", "V(1), V(1), 7"),
    ("with", "V(1), V(9)"),
    ("with", "NaN, 9"),
    ("with", "V(-1), 9"),
    ("toSpliced", ""),
    ("toSpliced", "V(1)"),
    ("toSpliced", "V(0), V(2), 8"),
    ("keys", ""),
    ("entries", ""),
    ("values", ""),
];

/// Species-constructing receivers: a real array whose `constructor` names a
/// species whose lookup and construction are fault-injecting steps, producing
/// a Proxy for the result.
const SPECIES_SETUP: &str = "
globalThis.S = function () {
  var a = [1, 2, 3];
  a.constructor = { get [Symbol.species]() { fault(); return function (n) { fault(); return P([]); }; } };
  return a;
};";

const SPECIES_CALLS: &[(&str, &str)] = &[
    ("map", "F(function (x) { return x; })"),
    ("filter", "F(function (x) { return true; })"),
    ("slice", "V(0), V(2)"),
    ("splice", "V(0), V(1), 9"),
    ("concat", "[4]"),
    ("flat", ""),
    ("flatMap", "F(function (x) { return [x]; })"),
];

/// Whole expressions with their own fault-injecting operands.
const EXTRA_OPS: &[&str] = &[
    "A.flat.call([P([1, 2])])",
    "A.flat.call([new Proxy([], { get(t, k, r) { return k === 'length' ? V(2) : Reflect.get(t, k, r); } })])",
    "A.flatMap.call(P([1, 2]), F(function (x) { return P([x]); }))",
    "A.toLocaleString.call([{ toLocaleString: F(function () { return V('x'); }) }])",
    "A.toLocaleString.call(P([{ toLocaleString: F(function () { return 'x'; }) }, null]))",
    "A.sort.call(P([V(2), V(1), V(3)]))",
    "A.concat.call(P([1]), { length: V(1), 0: 'x', [Symbol.isConcatSpreadable]: V(true) })",
    "A.concat.call(P([1]), P({ length: 1, 0: 'x', [Symbol.isConcatSpreadable]: true }))",
];

#[test]
fn species_construction_reports_a_fault_at_every_step() {
    let mut points = 0;
    for (method, args) in SPECIES_CALLS {
        points += fault_sweep(SPECIES_SETUP, &format!("A.{method}.call(S(), {args})"));
    }
    assert!(points > 30, "{points}");
}

#[test]
fn compound_operands_report_a_fault_at_every_step() {
    let mut points = 0;
    for op in EXTRA_OPS {
        points += fault_sweep("", op);
    }
    assert!(points > 30, "{points}");
}

#[test]
fn every_method_reports_a_fault_at_every_observable_step() {
    let mut points = 0;
    for (method, args) in CALLS {
        for receiver in RECEIVERS {
            points += fault_sweep("", &format!("A.{method}.call({receiver}, {args})"));
        }
    }
    assert!(points > 1000, "{points}");
}

fn plain(args: &str) -> String {
    args.replace("P(", "(")
        .replace("V(", "(")
        .replace("F(", "(")
}

#[test]
fn every_method_rejects_a_missing_receiver() {
    for (method, args) in CALLS {
        for receiver in ["null", "undefined"] {
            let call = format!("Array.prototype.{method}.call({receiver}, {})", plain(args));
            let mut vm = blueice_bluejs::Vm::default();
            let source = format!("(function () {{ try {{ {call}; return 'returned' }} catch (e) {{ return e.constructor.name }} }})()");
            let outcome = vm.execute(
                &blueice_bluejs::compile(&blueice_bluejs::parse(&source).unwrap()).unwrap(),
            );
            assert_eq!(
                outcome,
                Ok(blueice_bluejs::Value::String("TypeError".into())),
                "{call}"
            );
        }
    }
}

#[test]
fn every_method_runs_out_of_instructions_at_every_step() {
    let mut steps = 0;
    for (method, args) in CALLS {
        for receiver in PLAIN_RECEIVERS {
            let call = format!("Array.prototype.{method}.call({receiver}, {})", plain(args));
            steps += budget_sweep(&call);
        }
    }
    assert!(steps > 100, "{steps}");
}

#[test]
fn every_method_runs_out_of_heap_at_every_allocation() {
    let mut stopped = 0;
    for (method, args) in CALLS {
        for receiver in PLAIN_RECEIVERS {
            let call = format!("Array.prototype.{method}.call({receiver}, {})", plain(args));
            stopped += heap_sweep(&call, 3000, 8);
        }
    }
    assert!(stopped > 100, "{stopped}");
}

const CASES: &[(&str, &str)] = &[
    // lastIndexOf's start index.
    ("[1, 2, 3].lastIndexOf(3)", "2"),
    ("[1, 2, 1].lastIndexOf(1, Infinity)", "2"),
    ("[1, 2, 1].lastIndexOf(1, -Infinity)", "-1"),
    ("[1, 2, 1].lastIndexOf(1, -10)", "-1"),
    ("[1, 2, 1].lastIndexOf(1, -1)", "2"),
    ("[1, 2, 1].lastIndexOf(1, -3)", "0"),
    ("[1, 2, 1].lastIndexOf(2, NaN)", "-1"),
    ("[1, 2, 1].lastIndexOf(1, NaN)", "0"),
    ("[1, 2, 1].lastIndexOf(1, 1)", "0"),
    ("[1, 2, 1].lastIndexOf(1, 5)", "2"),
    ("[].lastIndexOf(1)", "-1"),
    // fill's end index.
    ("[1, 2, 3].fill(0, 1)", "1,0,0"),
    ("[1, 2, 3].fill(0, 1, 2)", "1,0,3"),
    // reverse and splice observe a property they cannot delete.
    ("(function () { var a = [1, 2, 3]; Object.defineProperty(a, 2, { value: 3, configurable: false, writable: true, enumerable: true }); delete a[0]; a.reverse() })()", "throws TypeError: cannot delete Array property"),
    ("(function () { var a = [1, 2, 3]; Object.defineProperty(a, 0, { value: 1, configurable: false, writable: true, enumerable: true }); delete a[2]; a.reverse() })()", "throws TypeError: cannot delete Array property"),
    // toLocaleString elements.
    ("[{ toLocaleString: 5 }].toLocaleString()", "throws TypeError: Array element toLocaleString is not callable"),
    ("[{ toLocaleString() { return { toString() { throw new EvalError('s') } } } }].toLocaleString()", "throws EvalError: s"),
    ("[1, null, 2].toLocaleString()", "1,,2"),
    // sort's comparator.
    ("[2, 1].sort(5)", "throws TypeError: Array sort comparator must be callable"),
    ("[2, 1].sort({})", "throws TypeError: Array sort comparator must be callable"),
    ("[2, 1].sort(function (a, b) { return { valueOf() { throw new EvalError('cmp') } } })", "throws EvalError: cmp"),
    ("(function () { var a = [3, undefined, 1]; Object.defineProperty(a, 2, { writable: false }); a.sort() })()", "throws TypeError: cannot assign Array property"),
    // species creation.
    ("(function () { var a = [1]; a.constructor = 5; return a.map(function () {}) })()", "throws TypeError: Array constructor must be an object"),
    ("(function () { var a = [1]; a.constructor = { [Symbol.species]: null }; return Array.isArray(a.map(function (x) { return x })) })()", "true"),
    ("(function () { var a = [1]; a.constructor = { [Symbol.species]: {} }; return a.map(function () {}) })()", "throws TypeError: Array species must be a constructor"),
    ("(function () { var r = Proxy.revocable([], {}); r.revoke(); return Array.prototype.map.call(r.proxy, function () {}) })()", "throws TypeError: operation attempted on a revoked Proxy"),
    ("(function () { var r = Proxy.revocable([], {}); r.revoke(); return [r.proxy].flat() })()", "throws TypeError: operation attempted on a revoked Proxy"),
    ("Array.prototype.map.call({ length: 2 ** 32 }, function () {})", "throws RangeError: invalid Array length"),
    // length limits.
    ("Array.prototype.concat.call({ length: 1 }, { length: 2 ** 53 - 1, [Symbol.isConcatSpreadable]: true })", "throws TypeError: concatenated Array length exceeds the safe integer limit"),
    ("Array.prototype.splice.call({ length: 2 ** 53 - 1 }, 0, 0, 1)", "throws TypeError: invalid Array length"),
    ("Array.prototype.push.call({ length: 2 ** 53 - 1 }, 1)", "throws TypeError: Array.prototype.push would exceed the maximum array-like length"),
    // splice moves elements it cannot delete the destination of.
    ("(function () { var a = [0, 1, 2, 3]; delete a[3]; Object.defineProperty(a, 2, { value: 2, configurable: false, writable: true, enumerable: true }); a.splice(1, 1) })()", "throws TypeError: cannot delete Array property"),
    ("(function () { var a = [0, , 2]; Object.defineProperty(a, 2, { value: 2, configurable: false, writable: true, enumerable: true }); a.splice(0, 0, 'x') })()", "throws TypeError: cannot delete Array property"),
    // copies of array-likes that are too long.
    ("Array.prototype.toReversed.call({ length: 2 ** 32 })", "throws RangeError: invalid Array length"),
    ("Array.prototype.toSpliced.call({ length: 2 ** 53 - 1 }, 0, 0, 1)", "throws TypeError: invalid Array length"),
    ("Array.prototype.toSorted.call([], 5)", "throws TypeError: Array toSorted comparator must be callable"),
    ("[1, 2, 3].with(NaN, 9).join()", "9,2,3"),
    ("[1, 2, 3].with(-1, 9).join()", "1,2,9"),
    ("[1, 2, 3].with(3, 9)", "throws RangeError: Array.prototype.with index is out of range"),
    ("[1, 2, 3].with(-4, 9)", "throws RangeError: Array.prototype.with index is out of range"),
    // callbacks that are not callable.
    ("[1].forEach(5)", "throws TypeError: Array.prototype.forEach callback must be callable"),
];

#[test]
fn array_methods_report_the_reference_results() {
    check_cases(CASES, false);
}

const REALM_CASES: &[(&str, &str)] = &[
    // A constructor from another realm's %Array% still creates an ordinary array.
    ("(function () { var other = $262.createRealm().global; var a = [1]; a.constructor = other.Array; return Object.getPrototypeOf(a.map(function (x) { return x })) === Array.prototype })()", "true"),
    // A method of another realm creates its results with that realm's %Array.prototype%.
    ("(function () { var other = $262.createRealm().global; return Object.getPrototypeOf(other.Array.prototype.map.call(new other.Array(1, 2), function (x) { return x })) === other.Array.prototype })()", "true"),
    // A TypedArray of another realm.
    ("(function () { var other = $262.createRealm().global; return Array.prototype.toLocaleString.call(new other.Uint8Array([10, 20])) })()", "10,20"),
    // A method of another realm working on a plain object creates its result with that realm's prototype.
    ("(function () { var map = $262.createRealm().global.eval('Array.prototype.map'); return map.call({ length: 2, 0: 1, 1: 2 }, function (x) { return x; }).length })()", "2"),
    ("(function () { var toReversed = $262.createRealm().global.eval('Array.prototype.toReversed'); return toReversed.call({ length: 2, 0: 1, 1: 2 }).join() })()", "2,1"),
    ("(function () { var other = $262.createRealm().global; return other.Uint8Array.prototype.toLocaleString.call(new other.Uint8Array([10, 20])) })()", "10,20"),
    ("(function () { var other = $262.createRealm().global; var ta = new other.Uint8Array(2); $262.detachArrayBuffer(ta.buffer); return Array.prototype.toLocaleString.call(ta) })()", "throws TypeError: foreign TypedArray is detached or out of bounds"),
    ("(function () { var r = Proxy.revocable([], {}); r.revoke(); return Array.prototype.concat.call(r.proxy) })()", "throws TypeError: operation attempted on a revoked Proxy"),
];

#[test]
fn arrays_of_other_realms_report_the_reference_results() {
    check_cases(REALM_CASES, true);
}

fn run_with(config: VmConfig, source: &str) -> Result<Value, RuntimeError> {
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn joined_text_is_bounded_by_the_string_limit() {
    let limit = |max_string_bytes| VmConfig {
        max_string_bytes,
        ..VmConfig::default()
    };
    // Twelve bytes per string; the separators and the total do not fit sixteen.
    for source in [
        "['aaaaaa', 'bbbbbb'].join('-')",
        "['aaaaaa', 'bbbbbb'].join('--------')",
    ] {
        assert_eq!(
            run_with(limit(16), source),
            Err(RuntimeError::StringLimit { limit: 16 }),
            "{source}"
        );
    }
    // 'toLocaleString' itself is twenty-eight bytes of the limit: the first
    // string fills it so the comma does not fit, the second one overflows it.
    for source in [
        "['aaaaaaaaaaaaaa', 'b'].toLocaleString()",
        "['a', 'bbbbbbbbbbbbbb'].toLocaleString()",
    ] {
        assert_eq!(
            run_with(limit(28), source),
            Err(RuntimeError::StringLimit { limit: 28 }),
            "{source}"
        );
    }
}

/// A child realm whose heap is nearly full before a method asks it for an
/// intrinsic it has not created yet, so the child's own allocation fails.
const HEAVY_CHILD_CASES: &[&str] = &[
    "var other = $262.createRealm().global; other.eval('globalThis.filler = []; for (var i = 0; i < 600; i++) filler.push({});'); var a = []; a.constructor = other.Object; Array.prototype.concat.call(a);",
    "var other = $262.createRealm().global; other.eval('globalThis.filler = []; for (var i = 0; i < 600; i++) filler.push({});'); other.Array.prototype.map.call(new other.Array(1, 2), function (x) { return x; });",
];

#[test]
fn a_full_child_realm_reports_the_heap_limit() {
    let mut stopped = 0;
    for source in HEAVY_CHILD_CASES {
        stopped += heap_sweep_with(source, 6000, 8, true);
    }
    assert!(stopped > 0, "{stopped}");
}

#[test]
fn the_unscopables_object_is_built_from_an_almost_full_heap() {
    let stopped = heap_sweep("Array.prototype[Symbol.unscopables]", 250_000, 16)
        + heap_sweep("with ({}) { }", 250_000, 16);
    assert!(stopped > 10, "{stopped}");
}
