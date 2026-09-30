// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every allocation made by the promise machinery, async functions and
//! generators, iterator records and `Array.fromAsync` failing in turn, with
//! the promise jobs drained so the work after each `await` is swept too.
mod cov_g2_sweep;

use cov_g2_sweep::{sweep_ops, Mode};

const WARMUP: &str = "var noop = () => {};
    new Promise(r => r(1)).then(noop); Promise.reject(1).catch(noop); Promise.resolve(1).finally(noop);
    Promise.withResolvers(); Promise.try(noop); Promise.all([1]); Promise.allSettled([1]); Promise.any([1]);
    Promise.race([1]); (async function () { await 1; for await (var x of [1]) {} })();
    (function* () { yield 1; })().next();
    Array.fromAsync([1]); Array.fromAsync({ length: 1, 0: 1 });
    Array.fromAsync({ [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: true }); } }; } });
    class P extends Promise {} P.resolve(1).then(noop); new P(r => r(1)); Promise.resolve.call(P, 1);
    for (var v of [1]) {} var [d] = [1]; [...new Set([1])]; Object.fromEntries([['a', 1]]); new Map([[1, 2]]);
    new Set([1]).union(new Set([2])); 0";

fn warm(body: &str) {
    sweep_ops(Mode::JOBS, WARMUP, body, 16);
}

#[test]
fn allocation_failures_in_promise_construction_and_settlement() {
    warm(
        "b(); var p = new Promise(r => r(1));
         b(); var q = Promise.resolve(2); b(); var t = p.then(x => x); b(); var c = p.catch(noop);
         b(); var f = p.finally(noop); b(); var rj = Promise.reject(3); rj.catch(noop);
         b(); var w = Promise.withResolvers(); b(); w.resolve(1);
         b(); var tr = Promise.try(() => 1); b(); var te = Promise.try(() => { throw 1; });
         b(); var self; var sp = new Promise(r => { self = r; }); b(); self(sp);
         b(); var th = new Promise(r => r({ then(res) { res(1); } }));
         b(); var gt = new Promise(r => r({ get then() { throw 1; } }));
         b(); var al = Promise.all([1, 2]); b(); var as = Promise.allSettled([1]); b(); var an = Promise.any([1]); b(); var ra = Promise.race([1]);
         return true;",
    );
}

#[test]
fn allocation_failures_in_custom_capabilities_and_species() {
    warm(
        "class C { constructor(ex) { ex(function () {}, function () {}); } }
         class Sub extends Promise {}
         b(); var a = Promise.resolve.call(C, 1);
         b(); var r = Promise.reject.call(C, 1);
         b(); var s = Sub.resolve(1); b(); var t = s.then(noop); b(); var f = s.finally(noop);
         b(); var w = Promise.withResolvers.call(C);
         b(); var tr = Promise.try.call(C, () => { throw 1; });
         b(); var p = Promise.resolve(1); Object.defineProperty(p, 'constructor', { value: { [Symbol.species]: C } });
         b(); var sp = p.then(noop);
         return true;",
    );
}

#[test]
fn allocation_failures_in_async_functions_and_generators() {
    warm(
        "var iterable = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; }, return() { return {}; } }; } };
         b(); async function f() { await 1; return 2; } b(); var pf = f();
         b(); async function g() { for (var x of iterable) { throw 'sync'; } } b(); var pg = g();
         b(); async function h() { for await (var x of [1, 2]) { await x; } } b(); var ph = h();
         b(); async function e() { throw new Error('x'); } b(); var pe = e();
         b(); async function* ag() { yield 1; } b(); var ai = ag();
         b(); function* gen(a = 1) { yield a; } b(); var gi = gen(); b(); gi.next(); b(); gi.return(1);
         return true;",
    );
}

#[test]
fn allocation_failures_in_array_from_async() {
    warm(
        "b(); var a1 = Array.fromAsync([1, 2]);
         b(); var a2 = Array.fromAsync({ length: 2, 0: 'a', 1: Promise.resolve('b') });
         b(); var a3 = Array.fromAsync({ [Symbol.asyncIterator]() { var i = 0; return { next() { return Promise.resolve(i++ < 2 ? { value: i, done: false } : { done: true }); } }; } });
         b(); var a4 = Array.fromAsync([1, 2], x => x * 2);
         b(); var a5 = Array.fromAsync({ length: 1, 0: 1 }, x => x);
         b(); var a6 = Array.fromAsync([1], () => { throw 'x'; });
         b(); var a7 = Array.fromAsync({ [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: true }); } }; } });
         b(); var a8 = Array.fromAsync(null);
         return true;",
    );
}

#[test]
fn allocation_failures_in_iteration_records_and_helpers() {
    warm(
        "b(); for (var x of [1, 2]) {}
         b(); var [a, c] = [3, 4];
         b(); var s = [...new Set([5, 6])];
         b(); var st = new Set([1, 2]); b(); var mp = new Map([[1, 2]]); b(); var of = Object.fromEntries([['a', 1]]);
         b(); var u = st.union(new Set([3])); b(); var i = st.intersection(new Set([2])); b(); var d = st.difference(new Set([2]));
         b(); var sd = st.symmetricDifference(new Set([2, 3])); b(); var sub = st.isSubsetOf(new Set([1, 2, 3]));
         b(); var sup = st.isSupersetOf(new Set([1])); b(); var dj = st.isDisjointFrom(new Set([9]));
         b(); var it = Iterator.from({ next() { return { done: true }; } }); b(); it.next();
         b(); var tag = (function (s) { return s; })`a${1}b`; b(); var tag2 = (function (s) { return s; })`c`;
         return true;",
    );
}

#[test]
fn allocation_failures_in_classes_for_in_and_rest() {
    warm(
        "b(); class A { #x = 1; static #y = 2; get #g() { return 1; } static get(o) { return o.#x; } }
         b(); var a = new A(); b(); A.get(a);
         b(); class B extends A { constructor() { super(); this.z = super.constructor; } } b(); new B();
         b(); class N extends null {} b(); var o = { m() { return super.x; } }; b(); o.m();
         b(); var { p, ...rest } = { p: 1, q: 2, r: 3 }; b(); var sp = { ...rest, s: 4 };
         b(); for (var k in { a: 1, b: 2 }) {} b(); for (var k2 in [1, 2]) {}
         b(); var named = { [1]: function () {}, [Symbol('d')]: () => {} };
         b(); function withEval(a = eval('1')) { return a; } b(); withEval();
         b(); var arrow = () => 1; b(); arrow();
         b(); function Sloppy() { return this; } b(); Sloppy.call(1); b(); Sloppy.call(undefined);
         return true;",
    );
}
