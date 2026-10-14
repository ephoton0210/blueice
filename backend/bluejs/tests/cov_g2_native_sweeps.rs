// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every allocation made by the built-in natives (`dispatch.rs`,
//! `general.rs`), the lazily materialized intrinsics behind property lookups
//! (`properties.rs`) and the promise and async machinery failing in turn.
mod cov_g2_sweep;

use cov_g2_sweep::{sweep_cold, sweep_ops, Mode};

/// Warms every intrinsic the warm sweeps below touch, so the swept
/// allocations are the scripts' own.
const WARMUP: &str = "new Array(1); Array(1, 2); [1].values().next(); [1].entries().next();
    new Int8Array(2).values().next(); new Number(1); new Boolean(1); Object(1); new String('x');
    ''[Symbol.iterator]().next(); class E extends Object {} new E(); new Object(); Object(null);
    new (class extends Iterator {})(); Proxy.revocable({}, {}).revoke(); Symbol('x'); Symbol.for('y');
    BigInt(1); Object(1n); Object(Symbol()); new Date(0); /x/; new Error('e'); (function () { return arguments; })();
    Object.prototype.toString.call(Object(1n)); ({}).__defineGetter__('x', function () {});
    ({}).__lookupGetter__('x'); Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call({}, null);
    Reflect.apply(function () {}, null, []); Reflect.construct(function () {}, []);
    Function.prototype.toString.call(function () {}); parseInt('1'); parseFloat('1'); encodeURI('a'); escape('a');
    'a'.at(0); 'ab'.slice(0); 'a'.padStart(3); 'ab'.concat('c'); [...'ab']; new Map([[1, 2]]); new Set([1]);
    new WeakMap(); new WeakRef({}); JSON.stringify({}); JSON.parse('1'); Math.max(1); isNaN(1);
    var t = new Uint8Array(4); t.buffer; new DataView(new ArrayBuffer(2)); new SharedArrayBuffer(2);
    ArrayBuffer.isView(t); Uint8Array.fromBase64('AQ=='); Uint8Array.fromHex('01'); 0";

fn warm(body: &str) {
    sweep_ops(Mode::PLAIN, WARMUP, body, 16);
}

#[test]
fn allocation_failures_in_constructors_and_iterators() {
    warm(
        "b(); var a1 = new Array(3);
         b(); var a2 = Array(1, 2);
         b(); var i1 = [1, 2].values(); b(); i1.next(); b(); var i2 = [1].entries(); b(); i2.next();
         b(); var i3 = new Int8Array(2).values(); b(); i3.next();
         b(); var n1 = new Number(1); b(); var n2 = new Boolean(true); b(); var n3 = Object(1);
         b(); var s1 = new String('x'); b(); var s2 = ''[Symbol.iterator](); b(); s2.next();
         b(); class C extends Object {}; b(); var c = new C(); b(); var o1 = new Object(); b(); var o2 = Object(null);
         b(); var it = new (class extends Iterator {})();
         b(); var r = Proxy.revocable({}, {}); b(); r.revoke();
         b(); var sy = Symbol('x'); b(); var sf = Symbol.for('z');
         return a1.length === 3 && c instanceof C;",
    );
}

#[test]
fn allocation_failures_in_conversions_and_prototype_natives() {
    warm(
        "b(); var s1 = Object.prototype.toString.call(Object(1n));
         b(); var d = ({}); d.__defineGetter__('x', function () {});
         b(); var g = d.__lookupGetter__('x');
         b(); var big = BigInt(5); b(); var bs = big.toString(2); b(); var bl = big.toLocaleString('en');
         b(); var f = Function.prototype.toString.call(function () {});
         b(); Reflect.apply(function () {}, null, [1]); b(); Reflect.construct(function () {}, [1]);
         b(); var m = 'a'.at(0); b(); var sl = 'abc'.slice(1); b(); var pd = 'a'.padStart(4, 'xy');
         b(); var e = encodeURI('ä'); b(); var esc = escape('ä'); b(); var u = unescape('%E4');
         b(); var j = JSON.stringify({ a: [1] }); b(); var p = JSON.parse('{\"a\":[1]}');
         b(); var lc = 'a'.localeCompare('b'); b(); var up = 'i'.toLocaleUpperCase('tr');
         b(); var fc = String.fromCharCode(65, 66); b(); var fp = String.fromCodePoint(65);
         return s1 === '[object BigInt]' && lc < 0;",
    );
}

#[test]
fn allocation_failures_in_binary_data_natives() {
    warm(
        "b(); var ab = new ArrayBuffer(8, { maxByteLength: 16 }); b(); var v = new Uint8Array(ab); b(); var dv = new DataView(ab);
         b(); var sab = new SharedArrayBuffer(4, { maxByteLength: 8 });
         b(); var x = v.buffer; b(); var y = v.length; b(); var z = dv.buffer;
         b(); var f = Uint8Array.fromBase64('AQID'); b(); var h = Uint8Array.fromHex('0102');
         b(); var kv = [...v.entries()];
         return ab.byteLength === 8 && f.length === 3;",
    );
}

#[test]
fn allocation_failures_in_array_and_collection_natives() {
    warm(
        "b(); var m = new Map([[1, 2]]); b(); var s = new Set([1, 2]); b(); var ms = m.size; b(); var ss = s.size;
         b(); var ws = new WeakMap(); b(); var wr = new WeakRef({}); b(); var fr = new FinalizationRegistry(() => {});
         b(); var arr = [3, 1, 2]; b(); arr.push(4, 5); b(); var sorted = arr.toSorted(); b(); var flat = [[1], [2]].flat();
         b(); var fm = [1, 2].flatMap(x => [x]); b(); var at = arr.at(-1); b(); var w = arr.with(0, 9);
         b(); var sp = arr.toSpliced(0, 1); b(); var rv = arr.toReversed(); b(); var jn = arr.join('-');
         b(); var g = Object.groupBy([1, 2], x => x % 2); b(); var mg = Map.groupBy([1, 2], x => x % 2);
         return ms === 1 && ss === 2 && jn.length > 0;",
    );
}

#[test]
fn cold_sweep_of_property_lookups_on_lazy_intrinsics() {
    // Each script is the first thing to touch its intrinsic, so the swept
    // allocations are that intrinsic's own materialization.
    for script in [
        "[].constructor === Array",
        "(function () {}).constructor === Function",
        "({}).constructor === Object",
        "'abc'.propertyIsEnumerable('0')",
        "({}).hasOwnProperty('x') === false",
        "typeof (function () {}).caller !== 'x'",
        "typeof ({}).__proto__ === 'object'",
        "typeof [].push === 'function'",
        "Symbol('d').description === 'd'",
        "true.toString() === 'true'",
        "1n.toString() === '1'",
        "(5).toFixed(1) === '5.0'",
        "'x'.at(0) === 'x'",
        "typeof [][Symbol.iterator] === 'function'",
        "typeof [][Symbol.unscopables] === 'object'",
    ] {
        sweep_cold(Mode::PLAIN, script, 8);
    }
}

#[test]
fn cold_sweep_of_native_constructors() {
    for script in [
        "new Array(2).length === 2",
        "new Number(1) instanceof Number",
        "Object(1n) instanceof BigInt",
        "typeof Symbol.for('a') === 'symbol'",
        "[1].values().next().value === 1",
        "new Int8Array(2).values().next().value === 0",
        "''[Symbol.iterator]().next().done === true",
        "typeof Proxy.revocable({}, {}).revoke === 'function'",
        "class C extends Object {} new C() instanceof C",
        "Reflect.apply(function () { return 1; }, null, []) === 1",
        "Function.prototype.toString.call(function () {}).length > 0",
        "new Map([[1, 2]]).size === 1",
        "new Set([1]).size === 1",
        "JSON.stringify({ a: 1 }) === '{\"a\":1}'",
        "Iterator.from([1]).toArray().length === 1",
        "Array.fromAsync([1]) instanceof Promise",
    ] {
        sweep_cold(Mode::PLAIN, script, 8);
    }
}

#[test]
fn cold_sweep_of_promises_and_async_functions() {
    for script in [
        "new Promise(r => r(1)) instanceof Promise",
        "Promise.resolve(1).then(x => x) instanceof Promise",
        "Promise.reject(1).catch(x => x) instanceof Promise",
        "Promise.resolve(1).finally(() => {}) instanceof Promise",
        "Promise.withResolvers().promise instanceof Promise",
        "Promise.try(() => 1) instanceof Promise",
        "Promise.all([1]) instanceof Promise",
        "(async function () { await 1; })() instanceof Promise",
        "(async function () { for await (var x of [1]) {} })() instanceof Promise",
        "(function* () {})() !== undefined",
        "(async function* () {})() !== undefined",
        "class B { #x = 1; static get(o) { return o.#x; } } B.get(new B()) === 1",
        "(function (a = eval('1')) { return a; })() === 1",
        "var { a, ...r } = { a: 1, b: 2 }; r.b === 2",
        "(function () { for (var k in { a: 1 }) return k; })() === 'a'",
        "(() => { class D extends null {} return typeof D; })() === 'function'",
    ] {
        sweep_cold(Mode::JOBS, script, 8);
    }
}
