// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every allocation made by the iterator helpers and the shared iterator
//! record operations failing in turn (see `cov_g2_support::sweep_ops`).
mod cov_g2_support;

use cov_g2_support::sweep_ops;

/// Materializes every lazily built intrinsic the sweeps below touch, so the
/// swept allocations are the scripts' own.
const WARMUP: &str = "Iterator.from([1]).map(x => x).toArray();
    Iterator.concat([1]).toArray(); Iterator.zip([[1]]).toArray();
    Iterator.zipKeyed({a: [1]}).toArray();
    [1].values().flatMap(x => [x]).toArray(); [1].values().chunks(1).toArray();
    [1].values().windows(1).toArray(); [1].values().take(1).toArray(); [1].values().drop(0).toArray();
    [1].values().filter(x => x).toArray(); [1].values().includes(1); [1].values().join();
    Iterator.zip([[1]]).return(); Iterator.concat([1]).return(); [1].values().map(x => x).return();
    [1].values().reduce((a, b) => a + b); [1].values().forEach(x => x);
    for (var x of [1]) {} var [y] = [1]; [...[1]]; new Set([1]); new Map([[1, 2]]);
    (function (s) { return s; })`a`; 0";

fn sweep(body: &str) {
    sweep_ops(WARMUP, body, 16);
}

#[test]
fn allocation_failures_in_linear_helpers() {
    sweep(
        "var src = [1, 2, 3];
         b(); var m = src.values().map(x => x * 2);
         b(); var a1 = m.next();
         b(); m.next(); b(); m.next(); b(); m.next();
         b(); var f = src.values().filter(x => x > 1);
         b(); f.next(); b(); f.next(); b(); f.next();
         b(); var t = src.values().take(2);
         b(); t.next(); b(); t.next(); b(); t.next();
         b(); var d = src.values().drop(1);
         b(); d.next(); b(); d.next(); b(); d.next();
         b(); var fm = src.values().flatMap(x => [x, x]);
         b(); fm.next(); b(); fm.next(); b(); fm.next(); b(); fm.return();
         b(); var all = src.values().toArray();
         return a1.value === 2 && all.length === 3;",
    );
}

#[test]
fn allocation_failures_in_concat() {
    sweep(
        "var log = [];
         var src = { next() { return { value: 1, done: false }; }, return() { log.push('closed'); return {}; } };
         b(); var it = Iterator.concat([1], [2]);
         b(); it.next(); b(); it.next(); b(); it.next(); b(); it.next();
         b(); var live = Iterator.concat({ [Symbol.iterator]() { return src; } });
         b(); live.next(); b(); live.return();
         return log.length === 1;",
    );
}

#[test]
fn allocation_failures_in_zip() {
    sweep(
        "var log = [];
         var src = { next() { return { value: 1, done: false }; }, return() { log.push('closed'); return {}; } };
         b(); var it = Iterator.zip([[1, 2], [3, 4]]);
         b(); it.next(); b(); it.next(); b(); it.next();
         b(); var longest = Iterator.zip([[1], [2, 3]], { mode: 'longest', padding: ['p', 'q'] });
         b(); longest.next(); b(); longest.next(); b(); longest.next();
         b(); var strict = Iterator.zip([[1], [2]], { mode: 'strict' });
         b(); strict.next(); b(); strict.next();
         b(); var keyed = Iterator.zipKeyed({ x: [1, 2], y: [3] }, { mode: 'longest', padding: { y: 0 } });
         b(); keyed.next(); b(); keyed.next(); b(); keyed.next();
         b(); var live = Iterator.zip([src, src]);
         b(); live.next(); b(); live.return();
         return log.length === 2;",
    );
}

#[test]
fn allocation_failures_in_chunks_and_windows() {
    sweep(
        "b(); var c = [1, 2, 3].values().chunks(2);
         b(); c.next(); b(); c.next(); b(); c.next();
         b(); var w = [1, 2, 3].values().windows(2);
         b(); w.next(); b(); w.next(); b(); w.next();
         b(); var p = [1].values().windows(3, 'allow-partial');
         b(); p.next(); b(); p.next();
         return true;",
    );
}

#[test]
fn allocation_failures_in_terminal_helpers() {
    sweep(
        "b(); var i = [1, 2, 3].values().includes(3);
         b(); var j = [1, 2].values().join('-');
         b(); var r = [1, 2].values().reduce((a, c) => a + c);
         b(); var e = [1, 2].values().every(x => x > 0);
         b(); var s = [1, 2].values().some(x => x > 1);
         b(); var f = [1, 2].values().find(x => x > 1);
         b(); [1, 2].values().forEach(x => x);
         return i && j === '1-2' && r === 3 && e && s && f === 2;",
    );
}

#[test]
fn allocation_failures_in_iteration_records() {
    sweep(
        "var n = 0;
         b(); for (var x of [1, 2]) n += x;
         b(); var [a, c] = [3, 4];
         b(); var s = [...new Set([5, 6])];
         b(); var st = new Set([1, 2]); b(); var mp = new Map([[1, 2]]);
         b(); var u = st.union(new Set([3]));
         b(); var it = st.intersection(new Set([2]));
         b(); var df = st.difference(new Set([2]));
         b(); var sd = st.symmetricDifference(new Set([2, 3]));
         b(); var sub = st.isSubsetOf(new Set([1, 2, 3]));
         b(); var sup = st.isSupersetOf(new Set([1]));
         b(); var dj = st.isDisjointFrom(new Set([9]));
         return n === 3 && a + c === 7 && s.length === 2 && u.size === 3 && it.size === 1 &&
           df.size === 1 && sd.size === 2 && sub && sup && dj;",
    );
}

#[test]
fn allocation_failures_in_tagged_templates_and_setters() {
    sweep(
        "function tag(s) { return s; }
         b(); var t1 = tag`a${1}b`;
         b(); var t2 = tag`c`;
         b(); var proto = Object.create(Iterator.prototype);
         b(); var d = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag);
         b(); d.set.call(proto, 'x');
         b(); var e = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor');
         b(); e.set.call(Object.create(Iterator.prototype), 'y');
         return t1.raw.length === 2 && t2.length === 1;",
    );
}
