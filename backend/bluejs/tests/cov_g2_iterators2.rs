// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The last rarely-taken iterator paths: allocations that only fail when the
//! preceding operation leaves live ballast right before them (a source whose
//! `next` ends in `b()` and returns a pre-built result), source and
//! `return()` errors, and the `Iterator.prototype` setters.
mod cov_g2_support;
mod cov_g2_sweep;

use cov_g2_support::expect_true;
use cov_g2_sweep::{sweep_cold, sweep_ops, Mode};

const WARMUP: &str = "Iterator.from([1]).map(x => x).toArray();
    Iterator.zip([[1]]).toArray(); Iterator.zipKeyed({a: [1]}).toArray();
    [1].values().windows(1).toArray(); [1].values().chunks(1).toArray();
    (async () => { for await (var x of [1]) {} })(); 0";

/// A source of `n` values whose `next` allocates ballast last and returns
/// pre-built results, so the allocation right after it is the first one to
/// see the ballast.
const SOURCE: &str = "function src(n) { var i = 0; var r = { value: 1, done: false };
      var d = { value: undefined, done: true };
      return { [Symbol.iterator]() { return this; }, next() { b(); return i++ < n ? r : d; } }; }";

fn sweep(mode: Mode, body: &str) {
    sweep_ops(mode, WARMUP, &format!("{SOURCE} {body}"), 16);
}

#[test]
fn allocations_right_after_a_source_step() {
    sweep(
        Mode::PLAIN,
        "var z = Iterator.zip([src(2), src(2)]); var zr = z.next();
         var k = Iterator.zipKeyed({ a: src(2), b: src(2) }); var kr = k.next();
         var w = src(3).values ? null : null;
         var win = Iterator.from(src(3)).windows(2); var wr = win.next(); var wr2 = win.next();
         var arr = Iterator.from(src(2)).toArray();
         return zr.value.length === 2 && kr.value.a === 1 && wr.value.length === 2 &&
           wr2.value.length === 2 && arr.length === 2;",
    );
}

#[test]
fn allocations_right_after_a_synchronous_source_step_in_for_await() {
    sweep(
        Mode::JOBS,
        "var live = { value: {}, done: false }; var last = { value: {}, done: true };
         var mk = (r) => ({ [Symbol.iterator]() { return { next() { b(); return r; }, return() { return {}; } }; } });
         (async () => { for await (var x of mk(live)) break; for await (var y of mk(last)) break; })();
         return true;",
    );
}

#[test]
fn strict_zip_reports_a_throwing_remaining_source() {
    expect_true(
        "var thrower = { [Symbol.iterator]() { return { next() { throw 'second'; } }; } };
         var it = Iterator.zip([[], thrower], { mode: 'strict' });
         try { it.next(); false; } catch (e) { e === 'second'; }",
    );
}

#[test]
fn flat_map_return_closes_its_inner_iterator() {
    expect_true(
        "var log = [];
         function inner(fail) { return { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; },
           return() { log.push('inner'); if (fail) throw 'inner-fail'; return {}; } }; } }; }
         var ok = [1].values().flatMap(() => inner(false)); ok.next(); ok.return();
         var bad = [1].values().flatMap(() => inner(true)); bad.next();
         var caught; try { bad.return(); } catch (e) { caught = e; }
         log.join() === 'inner,inner' && caught === 'inner-fail'",
    );
}

#[test]
fn reduce_reports_a_source_that_throws_after_the_first_step() {
    expect_true(
        "var n = 0; var src = { [Symbol.iterator]() { return { next() { if (n++ < 2) return { value: n, done: false }; throw 'late'; } }; } };
         var caught; try { Iterator.from(src).reduce((a, b) => a + b); } catch (e) { caught = e; }
         var n2 = 0;
         var src2 = { [Symbol.iterator]() { return { next() { if (n2++ < 1) return { value: 1, done: false }; throw 'late2'; } }; } };
         var caught2; try { Iterator.from(src2).reduce((a, b) => a + b, 0); } catch (e) { caught2 = e; }
         caught === 'late' && caught2 === 'late2'",
    );
}

#[test]
fn prototype_setters_report_receiver_failures() {
    expect_true(
        "var d = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor');
         var o = Object.create(Iterator.prototype);
         Object.defineProperty(o, 'constructor', { set(v) { throw 'setter'; }, configurable: true });
         var t1; try { d.set.call(o, 2); } catch (e) { t1 = e; }
         var frozen = Object.freeze(Object.create(Iterator.prototype));
         var t2; try { d.set.call(frozen, 2); } catch (e) { t2 = e instanceof TypeError; }
         t1 === 'setter' && t2",
    );
}

#[test]
fn a_cold_for_await_over_a_plain_synchronous_iterable_allocates_its_handlers() {
    sweep_cold(
        Mode::JOBS,
        "var it = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; } }; } };
         (async () => { for await (var x of it) break; })(); true",
        16,
    );
}
