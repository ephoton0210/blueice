// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise capabilities, species lookups, `finally`, async-generator yields
//! and `Array.fromAsync`: the failure and re-entrancy paths that ordinary
//! promise chains never take.
mod cov_g2_support;

use cov_g2_support::{expect_async_true, expect_true};

/// Records what each named async scenario settles with, then reports whether
/// every scenario matched its expectation.
const HARNESS: &str = "var results = {};
    function scenario(name, promise, expected) {
      results[name] = 'pending';
      promise.then(v => { results[name] = 'ok:' + v; }, e => { results[name] = 'err:' + (e instanceof Error ? e.constructor.name : e); });
      expectations.push([name, expected]);
    }
    var expectations = [];
    function finish() {
      var chain = Promise.resolve();
      for (var i = 0; i < 60; i++) chain = chain.then(() => {});
      chain.then(() => {
        var bad = expectations.filter(([n, e]) => results[n] !== e).map(([n, e]) => n + '=' + results[n] + ' (want ' + e + ')');
        globalThis.result = bad.length === 0 ? true : bad.join('; ');
      });
    }
    function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }";

fn scenarios(body: &str) {
    expect_async_true(&format!("{HARNESS}\n{body}\nfinish();"));
}

#[test]
fn custom_capability_constructors_are_validated() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         class Twice { constructor(ex) { ex(() => {}, () => {}); ex(() => {}, () => {}); } }
         class NotCallable { constructor(ex) { ex(1, 2); } }
         class ThrowsInResolve { constructor(ex) { ex(() => { throw 'resolve'; }, () => {}); } }
         class ThrowsInReject { constructor(ex) { ex(() => {}, () => { throw 'reject'; }); } }
         thrown(() => Promise.resolve.call(Twice, 1)) instanceof TypeError &&
           thrown(() => Promise.reject.call(NotCallable, 1)) instanceof TypeError &&
           thrown(() => Promise.resolve.call(ThrowsInResolve, 1)) === 'resolve' &&
           thrown(() => Promise.reject.call(ThrowsInReject, 1)) === 'reject' &&
           thrown(() => Promise.withResolvers.call(Twice)) instanceof TypeError &&
           thrown(() => Promise.try.call(NotCallable, () => { throw 1; })) instanceof TypeError &&
           thrown(() => Promise.resolve.call(1, 1)) instanceof TypeError &&
           thrown(() => Promise.reject.call(1, 1)) instanceof TypeError &&
           thrown(() => Promise.try.call(1, () => 1)) instanceof TypeError &&
           thrown(() => Promise.resolve.call(function () {}, 1)) instanceof TypeError",
    );
}

#[test]
fn the_promise_constructor_validates_its_arguments() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         var NT = new Proxy(function () {}, { get(t, k) { if (k === 'prototype') throw 'proto'; return t[k]; } });
         thrown(() => Promise(() => {})) instanceof TypeError && thrown(() => new Promise(1)) instanceof TypeError &&
           thrown(() => Reflect.construct(Promise, [() => {}], NT)) === 'proto' &&
           new Promise(() => { throw 1; }) instanceof Promise &&
           (function () { var r; new Promise(res => { res(1); throw 2; }); return true; })()",
    );
}

#[test]
fn resolving_a_promise_observes_constructor_then_and_itself() {
    scenarios(
        "var p = Promise.resolve(1);
         Object.defineProperty(p, 'constructor', { get() { throw 'ctor'; } });
         var self; var selfPromise = new Promise(r => { self = r; }); self(selfPromise);
         var thenGetter = new Promise(r => r({ get then() { throw 'then'; } }));
         var nonCallableThen = new Promise(r => r({ then: 1 }));
         var chained = new Promise(r => r({ then(res) { res('thenable'); } }));
         scenario('ctor getter', (async () => { try { Promise.resolve(p); return 'no'; } catch (e) { return e; } })(), 'ok:ctor');
         scenario('self', selfPromise, 'err:TypeError');
         scenario('then getter', thenGetter, 'err:then');
         scenario('non-callable then', nonCallableThen.then(v => typeof v.then), 'ok:number');
         scenario('thenable', chained, 'ok:thenable');
         scenario('await ctor getter', (async () => { try { await p; return 'no'; } catch (e) { return e; } })(), 'ok:ctor');
         scenario('promise reject', Promise.reject(new RangeError('x')), 'err:RangeError');",
    );
}

#[test]
fn species_lookup_failures_reach_then_catch_and_finally() {
    scenarios(
        "function fresh() { return Promise.resolve(1); }
         function withCtor(value) { var p = fresh(); Object.defineProperty(p, 'constructor', value); return p; }
         function attempt(make, call) { try { return Promise.resolve(call(make())); } catch (e) { return Promise.resolve('threw:' + (e instanceof Error ? e.constructor.name : e)); } }
         scenario('ctor getter', attempt(() => withCtor({ get() { throw 'g'; } }), p => p.then()), 'ok:threw:g');
         scenario('ctor primitive', attempt(() => withCtor({ value: 1 }), p => p.then()), 'ok:threw:TypeError');
         scenario('species getter', attempt(() => withCtor({ value: { get [Symbol.species]() { throw 's'; } } }), p => p.then()), 'ok:threw:s');
         scenario('species non-constructor', attempt(() => withCtor({ value: { [Symbol.species]: 1 } }), p => p.then()), 'ok:threw:TypeError');
         scenario('species undefined', attempt(() => withCtor({ value: { [Symbol.species]: undefined } }), p => p.then(x => x)), 'ok:1');
         scenario('species null', attempt(() => withCtor({ value: { [Symbol.species]: null } }), p => p.then(x => x)), 'ok:1');
         scenario('ctor undefined', attempt(() => withCtor({ value: undefined }), p => p.then(x => x)), 'ok:1');
         scenario('finally ctor getter', attempt(() => withCtor({ get() { throw 'f'; } }), p => p.finally(() => {})), 'ok:threw:f');
         scenario('catch non-callable then', attempt(() => { var p = fresh(); p.then = 1; return p; }, p => p.catch(() => {})), 'ok:threw:TypeError');
         scenario('finally non-object', attempt(() => 1, p => Promise.prototype.finally.call(p)), 'ok:threw:TypeError');",
    );
}

#[test]
fn finally_chains_the_original_settlement_and_reports_its_own_failures() {
    scenarios(
        "class Sub extends Promise {}
         var thenables = { then(res) { res('inner'); } };
         scenario('passes value', Promise.resolve('v').finally(() => 'ignored'), 'ok:v');
         scenario('passes reason', Promise.reject('r').finally(() => 'ignored'), 'err:r');
         scenario('non-callable', Promise.resolve('v').finally(1), 'ok:v');
         scenario('rejects from handler', Promise.resolve('v').finally(() => { throw 'own'; }), 'err:own');
         scenario('waits', Promise.resolve('v').finally(() => Promise.resolve('w')), 'ok:v');
         scenario('waits on thenable', Promise.resolve('v').finally(() => thenables), 'ok:v');
         scenario('subclass', Sub.resolve('v').finally(() => 1).then(v => v + ':' + (Sub.resolve(1) instanceof Sub)), 'ok:v:true');
         scenario('handler returns rejection', Promise.resolve('v').finally(() => Promise.reject('late')), 'err:late');",
    );
}

#[test]
fn promise_try_and_static_helpers_settle_through_their_capability() {
    scenarios(
        "scenario('try value', Promise.try(() => 5), 'ok:5');
         scenario('try throw', Promise.try(() => { throw 'x'; }), 'err:x');
         scenario('try args', Promise.try((a, b) => a + b, 1, 2), 'ok:3');
         scenario('withResolvers', (function () { var r = Promise.withResolvers(); r.resolve('w'); return r.promise; })(), 'ok:w');
         scenario('resolve same', (function () { var p = Promise.resolve(1); return Promise.resolve(p) === p ? Promise.resolve('same') : Promise.resolve('diff'); })(), 'ok:same');
         scenario('all settled', Promise.allSettled([1, Promise.reject(2)]).then(r => r.map(x => x.status).join()), 'ok:fulfilled,rejected');
         scenario('resolving twice', new Promise(r => { r(1); r(2); }), 'ok:1');",
    );
}

#[test]
fn async_generators_yield_pending_promises() {
    scenarios(
        "var resolveLater;
         async function* gen() { yield new Promise(r => { resolveLater = r; }); yield 2; }
         var it = gen();
         var first = it.next();
         resolveLater('late');
         scenario('pending yield', first.then(r => r.value), 'ok:late');
         var rejectLater;
         async function* rejecting() { try { yield new Promise((_, rej) => { rejectLater = rej; }); } catch (e) { yield 'caught ' + e; } }
         var rit = rejecting(); var rfirst = rit.next(); rejectLater('bad');
         scenario('pending rejected yield', rfirst.then(r => r.value, e => 'err:' + e), 'ok:caught bad');
         async function* delegating() { yield* (async function* () { yield 'a'; })(); }
         scenario('delegate', delegating().next().then(r => r.value), 'ok:a');",
    );
}

#[test]
fn array_from_async_validates_its_arguments() {
    scenarios(
        "scenario('bad mapper', Array.fromAsync([], 1), 'err:TypeError');
         scenario('async getter', Array.fromAsync({ get [Symbol.asyncIterator]() { throw 'ag'; } }), 'err:ag');
         scenario('sync getter', Array.fromAsync({ get [Symbol.iterator]() { throw 'sg'; } }), 'err:sg');
         scenario('next throws', Array.fromAsync({ [Symbol.asyncIterator]() { return { next() { throw 'nt'; } }; } }), 'err:nt');
         scenario('sync next throws', Array.fromAsync({ [Symbol.iterator]() { return { next() { throw 'snt'; } }; } }), 'err:snt');
         scenario('method returns non-object', Array.fromAsync({ [Symbol.asyncIterator]() { return 1; } }), 'err:TypeError');
         scenario('null', Array.fromAsync(null), 'err:TypeError');
         scenario('plain', Array.fromAsync([1, Promise.resolve(2), 3]).then(a => a.join()), 'ok:1,2,3');
         scenario('mapped', Array.fromAsync([1, 2], x => x * 2).then(a => a.join()), 'ok:2,4');
         scenario('mapped with this', Array.fromAsync([1], function (x) { return this.k + x; }, { k: 10 }).then(a => a.join()), 'ok:11');
         scenario('async iterable', Array.fromAsync({ async *[Symbol.asyncIterator]() { yield 1; yield 2; } }).then(a => a.join()), 'ok:1,2');",
    );
}

#[test]
fn array_from_async_handles_array_likes_and_their_failures() {
    scenarios(
        "scenario('array-like', Array.fromAsync({ length: 2, 0: 'a', 1: Promise.resolve('b') }).then(a => a.join()), 'ok:a,b');
         scenario('length getter', Array.fromAsync({ get length() { throw 'len'; } }), 'err:len');
         scenario('element getter', Array.fromAsync({ length: 1, get 0() { throw 'el'; } }), 'err:el');
         scenario('rejected element', Array.fromAsync({ length: 1, 0: Promise.reject('rej') }), 'err:rej');
         scenario('mapper throws', Array.fromAsync({ length: 1, 0: 1 }, () => { throw 'map'; }), 'err:map');
         scenario('mapped rejection', Array.fromAsync({ length: 1, 0: 1 }, () => Promise.reject('mrej')), 'err:mrej');
         scenario('mapped array-like', Array.fromAsync({ length: 2, 0: 1, 1: 2 }, x => x + 1).then(a => a.join()), 'ok:2,3');
         scenario('bad constructor length', Array.fromAsync.call(function () { return Object.freeze([]); }, { length: 0 }), 'err:TypeError');
         scenario('bad constructor define', Array.fromAsync.call(function () { return Object.freeze([]); }, { length: 1, 0: 1 }), 'err:TypeError');
         var ctorGetter = Promise.resolve(1); Object.defineProperty(ctorGetter, 'constructor', { get() { throw 'cg'; } });
         scenario('element with bad constructor', Array.fromAsync({ length: 1, 0: ctorGetter }), 'err:cg');
         scenario('receiver not a constructor', Array.fromAsync.call({}, { length: 1, 0: 5 }).then(a => a.join()), 'ok:5');",
    );
}

#[test]
fn array_from_async_closes_iterators_when_it_fails() {
    scenarios(
        "var log = [];
         function source(retResult) {
           var i = 0;
           return { [Symbol.asyncIterator]() { return this; },
                    next() { return Promise.resolve(i++ < 3 ? { value: i, done: false } : { done: true }); },
                    return() { log.push('return'); return retResult(); } };
         }
         scenario('mapper throws', Array.fromAsync(source(() => ({})), () => { throw 'm'; }), 'err:m');
         scenario('mapped rejects', Array.fromAsync(source(() => ({})), () => Promise.reject('mr')), 'err:mr');
         scenario('return rejects', Array.fromAsync(source(() => Promise.reject('rr')), () => { throw 'm2'; }), 'err:m2');
         scenario('return throws', Array.fromAsync(source(() => { throw 'rt'; }), () => { throw 'm3'; }), 'err:m3');
         var noReturn = { [Symbol.asyncIterator]() { var i = 0; return { next() { return Promise.resolve(i++ ? { done: true } : { value: 1, done: false }); } }; } };
         scenario('no return', Array.fromAsync(noReturn, () => { throw 'm4'; }), 'err:m4');
         var badReturn = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ value: 1, done: false }); }, get return() { throw 'gr'; } }; } };
         scenario('return getter throws', Array.fromAsync(badReturn, () => { throw 'm5'; }), 'err:m5');
         scenario('define fails', Array.fromAsync.call(function () { return Object.freeze([]); }, source(() => ({}))), 'err:TypeError');
         scenario('next result not an object', Array.fromAsync({ [Symbol.asyncIterator]() { return { next() { return Promise.resolve(1); } }; } }), 'err:TypeError');
         scenario('next result done getter', Array.fromAsync({ [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ get done() { throw 'dg'; } }); } }; } }), 'err:dg');
         scenario('rejected next', Array.fromAsync({ [Symbol.asyncIterator]() { return { next() { return Promise.reject('rn'); } }; } }), 'err:rn');",
    );
}

#[test]
fn async_generator_delegation_awaits_pending_yielded_promises() {
    scenarios(
        "var later, laterReject;
         var pending = new Promise(r => { later = r; });
         var pendingRejected = new Promise((_, rej) => { laterReject = rej; });
         function delegating(value) {
           var inner = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ value, done: false }); } }; } };
           return (async function* () { yield* inner; })();
         }
         var first = delegating(pending).next();
         var second = delegating(pendingRejected).next();
         later('resolved late'); laterReject('rejected late');
         scenario('pending value', first.then(r => r.value), 'ok:resolved late');
         scenario('pending rejection', second.then(r => r.value, e => 'err:' + e), 'err:rejected late');",
    );
}
