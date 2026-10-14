// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Set algebra methods (`union`, `intersection`, ...) against set-likes
//! that throw at every step, and receivers that change while a `has` runs.
mod cov_g2_support;

use cov_g2_support::expect_true;

const PRELUDE: &str = "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
    var methods = ['union', 'intersection', 'difference', 'symmetricDifference', 'isSubsetOf', 'isSupersetOf', 'isDisjointFrom'];
    function attempt(method, other, items) { var s = new Set(items || [1, 2, 3]); return thrown(() => s[method](other)); }
    function nextThrows() { return { size: 0, has() { return false; }, keys() { return { next() { throw 'next'; } }; } }; }";

fn check(body: &str) {
    expect_true(&format!("{PRELUDE}\n{body}"));
}

#[test]
fn every_method_reads_the_set_record_once_and_reports_each_failure() {
    check(
        "methods.every(m => attempt(m, { get size() { throw 'size'; }, has() {}, keys() {} }) === 'size') &&
           methods.every(m => attempt(m, { size: 0, get has() { throw 'has'; }, keys() {} }) === 'has') &&
           methods.every(m => attempt(m, { size: 0, has() {}, get keys() { throw 'keys'; } }) === 'keys') &&
           methods.every(m => attempt(m, { size: 0, has: 1, keys() {} }) instanceof TypeError) &&
           methods.every(m => attempt(m, { size: 0, has() {}, keys: 1 }) instanceof TypeError) &&
           methods.every(m => attempt(m, { size: NaN, has() {}, keys() {} }) instanceof TypeError) &&
           methods.every(m => attempt(m, { size: -1, has() {}, keys() {} }) instanceof RangeError) &&
           methods.every(m => attempt(m, 5) instanceof TypeError)",
    );
}

#[test]
fn key_iteration_errors_surface_from_every_method_that_iterates() {
    check(
        "['union', 'symmetricDifference', 'isSupersetOf', 'intersection', 'difference', 'isDisjointFrom']
           .every(m => attempt(m, nextThrows()) === 'next') &&
           thrown(() => new Set([1]).union({ size: 0, has() {}, keys() { throw 'open'; } })) === 'open' &&
           thrown(() => new Set([1]).union({ size: 0, has() {}, keys() { return 5; } })) instanceof TypeError &&
           thrown(() => new Set([1]).union({ size: 0, has() {}, keys() { return { get next() { throw 'nx'; } }; } })) === 'nx'",
    );
}

#[test]
fn has_errors_surface_from_every_method_that_probes() {
    check(
        "['intersection', 'difference', 'isSubsetOf', 'isDisjointFrom']
           .every(m => attempt(m, { size: 100, has() { throw 'has'; }, keys() {} }) === 'has') &&
           attempt('difference', { size: 100, has() { return { valueOf() { return true; } }; }, keys() {} }) === 'none'",
    );
}

#[test]
fn closing_a_key_iterator_early_reports_a_failing_return() {
    check(
        "function closing(values) {
           var i = 0;
           return { size: 0, has() { return false; },
                    keys() { return { next() { return i < values.length ? { value: values[i++], done: false } : { done: true }; },
                                      return() { throw 'close'; } }; } };
         }
         attempt('isSupersetOf', closing([99])) === 'close' && attempt('isDisjointFrom', closing([1])) === 'close' &&
           attempt('isSupersetOf', closing([1, 2])) === 'none' && attempt('isDisjointFrom', closing([99])) === 'none'",
    );
}

#[test]
fn tombstoned_and_changing_receivers_are_walked_live() {
    check(
        "function withTombstone() { var s = new Set([1, 2, 3, 4]); s.delete(2); return s; }
         var big = { size: 100, has() { return true; }, keys() { return [1, 3].values(); } };
         var small = { size: 0, has() { return false; }, keys() { return [1, 3].values(); } };
         var log = [];
         var deleting = new Set([1, 2, 3]);
         var deletesWhileProbing = { size: 100, has(v) { deleting.delete(2); log.push(v); return true; }, keys() {} };
         var results = methods.map(m => [...(function (r) { return r instanceof Set ? r : [r]; })(withTombstone()[m](big))].join());
         var lively = deleting.isSubsetOf(deletesWhileProbing);
         var fresh = new Set([1, 2, 3]);
         var inter = fresh.intersection({ size: 100, has(v) { fresh.delete(2); return true; }, keys() {} });
         var addsWhileProbing = new Set([1]);
         var visited = [];
         addsWhileProbing.isDisjointFrom({ size: 100, has(v) { visited.push(v); if (v === 1) addsWhileProbing.add(5); return false; }, keys() {} });
         results.length === 7 && lively === true && log.join() === '1,3' && [...inter].join() === '1,3' &&
           visited.join() === '1,5' && withTombstone().union(small).size === 3",
    );
}
