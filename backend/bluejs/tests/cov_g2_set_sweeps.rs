// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every allocation made by the Set algebra methods failing in turn.
mod cov_g2_sweep;

use cov_g2_sweep::{sweep_cold, sweep_ops, Mode};

const WARMUP: &str = "globalThis.setLike = function (items) { return { size: items.length, has(v) { return items.includes(v); }, keys() { return items.values(); } }; };
    var s = new Set([1, 2, 3]); s.union(setLike([2])); s.intersection(setLike([2])); s.difference(setLike([2]));
    s.symmetricDifference(setLike([2])); s.isSubsetOf(setLike([2])); s.isSupersetOf(setLike([2])); s.isDisjointFrom(setLike([2]));
    s.intersection(setLike([1, 2, 3, 4, 5])); s.difference(setLike([1, 2, 3, 4, 5])); s.isDisjointFrom(setLike([1, 2, 3, 4, 5]));
    0";

#[test]
fn allocation_failures_in_the_set_algebra() {
    sweep_ops(
        Mode::PLAIN,
        WARMUP,
        "var st = new Set([1, 2, 3]); var small = setLike([2]); var big = setLike([1, 2, 3, 4, 5]);
         b(); var u = st.union(small); b(); var u2 = st.union(big);
         b(); var i = st.intersection(small); b(); var i2 = st.intersection(big);
         b(); var d = st.difference(small); b(); var d2 = st.difference(big);
         b(); var sd = st.symmetricDifference(small); b(); var sd2 = st.symmetricDifference(big);
         b(); var sub = st.isSubsetOf(big); b(); var sub2 = st.isSubsetOf(small);
         b(); var sup = st.isSupersetOf(small); b(); var sup2 = st.isSupersetOf(setLike([7]));
         b(); var dj = st.isDisjointFrom(setLike([9])); b(); var dj2 = st.isDisjointFrom(big);
         return u.size === 3 && i.size === 1 && d.size === 2;",
        8,
    );
}

/// A set-like whose `has` and `keys().next` allocate ballast last and answer
/// from pre-built values, so the `Set` insertion right after is the first
/// allocation to see it.
const HOOKED: &str = "globalThis.hooked = function (items, hasAnswer) {
      var results = items.map(v => ({ value: v, done: false })); var done = { value: undefined, done: true };
      return { size: items.length, has(v) { b(); return hasAnswer; },
        keys() { var i = 0; return { next() { b(); return i < results.length ? results[i++] : done; } }; } }; }";

#[test]
fn set_insertions_right_after_a_user_hook() {
    sweep_ops(
        Mode::PLAIN,
        &format!("globalThis.b = function () {{}}; {WARMUP}; {HOOKED}; var w = new Set([1, 2, 3]); w.union(hooked([4], true)); w.intersection(hooked([1, 2, 3, 4, 5], true)); w.intersection(hooked([1], true)); w.symmetricDifference(hooked([4, 1], true)); w.difference(hooked([1], true)); 0"),
        "var st = new Set([1, 2, 3]);
         b(); var u = st.union(hooked([4, 5], true));
         b(); var i1 = st.intersection(hooked([1, 2, 3, 4, 5], true));
         b(); var i2 = st.intersection(hooked([2], true));
         b(); var sd = st.symmetricDifference(hooked([4, 1], true));
         b(); var d = st.difference(hooked([1], true));
         return u.size === 5 && i1.size === 3 && i2.size === 1 && sd.size === 3 && d.size === 2;",
        8,
    );
}

#[test]
fn the_set_prototype_is_built_lazily_by_the_first_set_algebra_call() {
    sweep_cold(
        Mode::PLAIN,
        "var s = new Set([1]); var o = { size: 1, has() { return true; }, keys() { return [1].values(); } };
         s.union(o).size === 1 && s.isSubsetOf(o)",
        16,
    );
}
