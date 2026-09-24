// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Iterator helpers, `Iterator.from`, `Iterator.zip`, `Iterator.zipKeyed` and
//! `Iterator.concat` over iterators and iterables whose every observable step
//! (property read, method call, callback) can be made to fail, and under every
//! instruction budget and heap size.

mod cov_g8_common;

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};
use cov_g8_common::{budget_sweep, check_cases, fault_sweep, heap_sweep};

include!("cov_g8_tables/iterators.in");

#[test]
fn iterator_helpers_report_the_reference_results() {
    check_cases(ITERATOR_CASES, false);
}

/// `mk(n)` yields 1..=n and `iterable(n)` is an iterable of it; both behind
/// fault-injecting Proxies and methods.
const SETUP: &str = "
globalThis.mk = function (n) {
  var i = 0, it = Object.create(Iterator.prototype);
  it.next = function () { fault(); i++; return i <= n ? { value: i, done: false } : { value: undefined, done: true }; };
  it.return = function () { fault(); return {}; };
  return P(it);
};
globalThis.iterable = function (n) { return { [Symbol.iterator]() { fault(); return mk(n); } }; };
globalThis.AsyncIteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf((async function* () { }).prototype));
globalThis.plain = function () { return P({ next() { fault(); return { done: true }; } }); };
";

const OPS: &[&str] = &[
    "mk(3).map(F(function (x) { return x; })).toArray()",
    "mk(3).filter(F(function (x) { return x > 1; })).toArray()",
    "mk(3).take(V(2)).toArray()",
    "mk(3).drop(V(1)).toArray()",
    "mk(3).flatMap(F(function (x) { return iterable(2); })).toArray()",
    "mk(3).flatMap(F(function (x) { return [x]; })).toArray()",
    "mk(3).reduce(F(function (a, b) { return a + b; }), V(0))",
    "mk(3).reduce(F(function (a, b) { return a + b; }))",
    "mk(3).some(F(function (x) { return x > 5; }))",
    "mk(3).every(F(function (x) { return x < 5; }))",
    "mk(3).find(F(function (x) { return x > 5; }))",
    "mk(3).forEach(F(function (x) { }))",
    "mk(3).toArray()",
    "mk(3).chunks(V(2)).toArray()",
    "mk(3).windows(V(2)).toArray()",
    "mk(3).includes(V(2), V(0))",
    "mk(3).join(V(','))",
    "(function () { var h = mk(3).map(F(function (x) { return x; })); h.next(); h.next(); h.return(); return h.next(); })()",
    "(function () { var h = mk(3).take(V(1)); h.next(); h.next(); return h.return(); })()",
    "(function () { var h = mk(3).filter(F(function (x) { return true; })); h.return(); return h.next(); })()",
    "Iterator.from(mk(2)).toArray()",
    "Iterator.from(iterable(2)).toArray()",
    "(function () { var w = Iterator.from(plain()); w.next(); return w.return(); })()",
    "Iterator.from(P('ab'))",
    "Iterator.zip(P([iterable(2), iterable(1)]), P({ mode: V('longest'), padding: P([V(0), V(9)]) })).toArray()",
    "Iterator.zip([iterable(2), iterable(2)], P({ mode: V('strict') })).toArray()",
    "Iterator.zip([iterable(2), iterable(1)], P({ mode: V('shortest') })).toArray()",
    "Iterator.zipKeyed(P({ a: iterable(2), b: iterable(1) }), P({ mode: V('longest'), padding: P({ a: V(0), b: V(9) }) })).toArray()",
    "Iterator.zipKeyed({ a: iterable(2), b: iterable(2) }, P({ mode: V('strict') })).toArray()",
    "Iterator.concat(iterable(2), iterable(1)).toArray()",
    "(function () { var c = Iterator.concat(iterable(2), iterable(1)); c.next(); return c.return(); })()",
    "Iterator.prototype[Symbol.dispose].call(P({ return() { fault(); return {}; } }))",
    "Iterator.prototype[Symbol.dispose].call(mk(2))",
    "AsyncIteratorPrototype[Symbol.asyncDispose].call(P({ return() { fault(); return Promise.resolve(1); } }))",
    "AsyncIteratorPrototype[Symbol.asyncDispose].call(P({ return() { fault(); return 5; } }))",
    "AsyncIteratorPrototype[Symbol.asyncDispose].call(P({}))",
    "(function () { function f() { return arguments.length; } return f.apply(null, P({ length: V(2), 0: V(1), 1: V(2) })); })()",
    "Reflect.apply(Math.max, null, P({ length: V(2), 0: V(1), 1: V(2) }))",
    "Reflect.construct(Array, P({ length: V(2), 0: V(1), 1: V(2) }))",
];

#[test]
fn every_iterator_step_can_fail() {
    let mut points = 0;
    for op in OPS {
        points += fault_sweep(SETUP, op);
    }
    assert!(points > 150, "{points}");
}

#[test]
fn iterator_helpers_run_out_of_instructions_and_heap() {
    for source in [
        "[1, 2, 3].values().map(x => x * 2).filter(x => x > 2).toArray().join()",
        "[1, 2, 3].values().flatMap(x => [x, x]).take(4).drop(1).toArray().join()",
        "[1, 2, 3].values().chunks(2).toArray().join('|') + [1, 2, 3].values().windows(2).toArray().join('|')",
        "Iterator.zip([[1, 2].values(), [3].values()], { mode: 'longest', padding: [0, 9] }).toArray().join('|')",
        "Iterator.zipKeyed({ a: [1, 2].values(), b: [3, 4].values() }).toArray().length",
        "Iterator.concat([1].values(), [2].values()).toArray().join() + Iterator.from({ next() { return { done: true } } }).toArray().length",
        "[1, 2, 3].values().reduce((a, b) => a + b, 0) + [1, 2].values().includes(2) + [1, 2].values().join('-')",
    ] {
        assert!(budget_sweep(source) > 0, "{source}");
        assert!(heap_sweep(source, 20_000, 8) > 0, "{source}");
    }
}

/// Every function of `Iterator`, `%Iterator.prototype%` and the async iterator
/// prototype called with receivers and arguments of the wrong kinds either
/// reports a TypeError or RangeError or returns; none may misbehave.
#[test]
fn iterator_functions_reject_receivers_and_arguments_of_the_wrong_kind() {
    let script = "(function () {
        var async = Object.getPrototypeOf(Object.getPrototypeOf((async function* () { }).prototype));
        var holders = [Iterator, Iterator.prototype, async], bad = [], odd = [undefined, null, 5, 'x', {}, [], function () { }, Symbol(), -1, NaN, Infinity, { next() { return 5; } }, { next() { return { done: true }; }, return() { return 5; } }];
        holders.forEach(function (holder) {
          Reflect.ownKeys(holder).forEach(function (key) {
            if (key === 'constructor') return;
            var d = Object.getOwnPropertyDescriptor(holder, key), f = d.value;
            if (typeof f !== 'function') return;
            odd.forEach(function (receiver) {
              odd.forEach(function (argument) {
                try { f.call(receiver, argument, argument); }
                catch (e) { if (!(e instanceof TypeError || e instanceof RangeError)) bad.push(String(key) + ':' + e); }
              });
            });
          });
        });
        return bad.join();
      })()";
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 20_000_000,
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(
        vm.execute(&compile(&parse(script).unwrap()).unwrap()),
        Ok(Value::String("".into()))
    );
}
