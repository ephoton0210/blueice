// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise, async-iteration and `Array.fromAsync` failure paths that run in the
//! job phase. A user hook allocates ballast (`b()`) and then either returns a
//! pre-built value or raises an engine error, so that the runtime's own next
//! allocation is the first one to see it.
mod cov_g2_support;
mod cov_g2_sweep;

use blueice_bluejs::Value;
use cov_g2_support::{expect_async_true, expect_true};
use cov_g2_sweep::{sweep_budget, sweep_ops, Mode};

const WARMUP: &str = "var wp = Promise.resolve(1); Promise.resolve(1).finally(() => wp);
    Promise.allSettled([{ then(r) { r(1); } }, wp]); Promise.try(() => { null.x; });
    new Promise(() => { null.x; }); new Promise(() => { throw 1; });
    Promise.resolve({ get then() { null.x; } }); Promise.reject(1).finally(() => {});
    (async () => { for await (var x of [{ then(r) { r(1); } }]) {} })();
    Array.fromAsync([1]); Array.fromAsync({ length: 1, 0: 1 }); Array.fromAsync([1], x => x);
    Array.fromAsync({ [Symbol.asyncIterator]() { return { next() { return { done: true }; }, return() { return {}; } }; } });
    try { Array.fromAsync([], 5); } catch (e) {} Array.fromAsync(5); 0";

fn sweep(body: &str) {
    sweep_ops(Mode::JOBS, WARMUP, body, 16);
}

#[test]
fn promise_machinery_allocating_right_after_a_user_hook() {
    sweep(
        "var pr = Promise.resolve(1);
         var th = { then(r) { b(); r(1); } };
         b(); Promise.resolve(1).finally(() => { b(); return pr; });
         b(); Promise.reject(2).finally(() => { b(); return pr; }).catch(() => {});
         b(); Promise.try(() => { b(); null.x; });
         b(); new Promise(() => { b(); null.x; });
         b(); new Promise(() => { b(); throw 1; });
         b(); Promise.resolve({ get then() { b(); null.x; } });
         b(); Promise.allSettled([th, th]);
         b(); Promise.allSettled([Promise.reject(th)]);
         b(); (async () => { for await (var x of [th]) {} })();
         b(); (async () => { for await (var y of [th]) break; })();
         return true;",
    );
}

#[test]
fn array_from_async_allocating_right_after_a_user_hook() {
    sweep(
        "var pr = Promise.resolve(1);
         var bad = Promise.resolve(2);
         Object.defineProperty(bad, 'constructor', { get() { b(); null.x; } });
         var frozenTarget = function () { return new Proxy({}, { defineProperty() { b(); return false; } }); };
         var iter = (next, ret) => ({ [Symbol.asyncIterator]() { return { next, return: ret }; } });
         b(); Array.fromAsync([], 5);
         b(); Array.fromAsync(5);
         b(); Array.fromAsync({ length: 1, 0: 1 }, () => { b(); null.x; });
         b(); Array.fromAsync([1], () => { b(); null.x; });
         b(); Array.fromAsync(iter(() => { b(); return { get done() { b(); null.x; } }; }));
         b(); Array.fromAsync(iter(() => { b(); null.x; }));
         b(); Array.fromAsync(iter(() => { b(); return bad; }));
         b(); Array.fromAsync.call(frozenTarget, [1]);
         b(); Array.fromAsync.call(frozenTarget, { length: 1, 0: 1 });
         var ret = { done: true };
         b(); Array.fromAsync(iter(() => pr, () => { b(); return ret; }), () => { throw 'm'; });
         return true;",
    );
}

#[test]
fn array_from_async_spends_fuel_per_element() {
    sweep_budget(
        Mode::JOBS,
        "Array.fromAsync({ length: 2, 0: 1, 1: 2 }); true",
        Value::Bool(true),
    );
}

#[test]
fn array_from_async_rejects_a_throwing_length_conversion() {
    expect_async_true(
        "Array.fromAsync({ length: { valueOf() { throw 'len'; } } }).catch(e => { globalThis.result = e === 'len'; });",
    );
}

#[test]
fn then_rejects_a_receiver_that_is_not_a_promise() {
    expect_true(
        "var t; try { Promise.prototype.then.call({}, () => {}); } catch (e) { t = e instanceof TypeError; } t",
    );
}

#[test]
fn an_async_generator_yielding_a_pending_promise_resumes_when_it_settles() {
    expect_async_true(
        "var settle; async function* g() { yield new Promise(r => { settle = r; }); }
         var it = g(); var p = it.next(); settle(5);
         p.then(r => { globalThis.result = r.value === 5 && r.done === false; });",
    );
}
