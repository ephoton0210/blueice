// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property access, classes and private names, `for-in`, object rest and
//! spread, and function calls (`properties.rs`, `closures.rs`): the error and
//! edge completions ordinary programs never take.
mod cov_g2_support;

use blueice_bluejs::Value;
use cov_g2_support::{
    expect_async_true, expect_script_true, expect_true, sweep_instruction_budget,
};

const PRELUDE: &str = "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
    function isType(f) { return thrown(f) instanceof TypeError; }
    function isRange(f) { return thrown(f) instanceof RangeError; }
    function isRef(f) { return thrown(f) instanceof ReferenceError; }
    var boom = { valueOf() { throw 'boom'; }, toString() { throw 'boom'; } };";

fn check(body: &str) {
    expect_true(&format!("{PRELUDE}\n{body}"));
}

#[test]
fn primitive_receivers_reach_their_prototype_methods() {
    check(
        "'abc'.propertyIsEnumerable('0') === true && 'abc'.hasOwnProperty('length') &&
           Symbol('d').description === 'd' && true.toString() === 'true' && 1n.toString() === '1' &&
           (5).toFixed(1) === '5.0' && isType(() => null.x) && isType(() => undefined.x) &&
           isType(() => { 'use strict'; null.x = 1; }) && isType(() => { undefined.x = 1; }) &&
           (function () { 'use strict'; return isType(() => { 'abc'.length = 1; }); })() &&
           (function () { 'abc'.foo = 1; return 'abc'.foo === undefined; })()",
    );
}

#[test]
fn global_property_writes_respect_read_only_bindings() {
    expect_script_true(
        "var g = 1; Object.defineProperty(globalThis, 'g', { writable: false });
         g = 2; var sloppy = g === 1;
         var strictFailed = (function () { 'use strict'; try { g = 3; return false; } catch (e) { return e instanceof TypeError; } })();
         sloppy && strictFailed && typeof globalThis.g === 'number'",
    );
}

#[test]
fn for_in_walks_prototype_chains_with_shadowing_deletion_and_cycles() {
    check(
        "function keys(o) { var out = []; for (var k in o) out.push(k); return out.join(); }
         var base = { a: 1, b: 2 }; var derived = Object.create(base); derived.c = 3;
         Object.defineProperty(derived, 'a', { value: 0, enumerable: false });
         var cyclicA = new Proxy({ x: 1 }, { getPrototypeOf() { return cyclicB; } });
         var cyclicB = new Proxy({ y: 2 }, { getPrototypeOf() { return cyclicA; } });
         var deleting = { p: 1, q: 2, r: 3 };
         var seen = []; for (var k in deleting) { seen.push(k); delete deleting.q; }
         keys(derived) === 'c,b' && keys('ab') === '0,1' && keys(null) === '' && keys(undefined) === '' &&
           keys(5) === '' && keys(cyclicA) === 'x,y' && seen.join() === 'p,r' &&
           keys({ [Symbol()]: 1, s: 2 }) === 's' &&
           thrown(() => { for (var k in new Proxy({}, { ownKeys() { throw 'keys'; } })) ; }) === 'keys' &&
           thrown(() => { for (var k in new Proxy({ a: 1 }, { getOwnPropertyDescriptor() { throw 'desc'; } })) ; }) === 'desc' &&
           thrown(() => { for (var k in new Proxy({ a: 1 }, { getPrototypeOf() { throw 'proto'; } })) ; }) === 'proto' &&
           keys(new Proxy({ a: 1 }, { ownKeys() { return ['a', 'gone']; }, getOwnPropertyDescriptor(t, k) { return k === 'a' ? { value: 1, enumerable: true, configurable: true } : undefined; } })) === 'a'",
    );
}

#[test]
fn object_rest_and_spread_copy_enumerable_own_properties() {
    check(
        "var { a, ...rest } = { a: 1, b: 2, c: 3 };
         var nothing = { ...null, ...undefined, ...5, ...'xy' };
         var proxied = { ...new Proxy({ p: 1, q: 2 }, { ownKeys() { return ['p', 'gone', 'q']; },
                                                       getOwnPropertyDescriptor(t, k) { return k === 'gone' ? undefined : Reflect.getOwnPropertyDescriptor(t, k); } }) };
         rest.b === 2 && Object.keys(rest).join() === 'b,c' && nothing[0] === 'x' && Object.keys(proxied).join() === 'p,q' &&
           thrown(() => ({ ...{ get x() { throw 'g'; } } })) === 'g' &&
           thrown(() => { var { ...r } = null; }) instanceof TypeError &&
           thrown(() => { var { [boom]: x, ...r } = {}; }) === 'boom' &&
           thrown(() => ({ ...new Proxy({}, { ownKeys() { throw 'ok'; } }) })) === 'ok' &&
           thrown(() => ({ ...new Proxy({ a: 1 }, { getOwnPropertyDescriptor() { throw 'gopd'; } }) })) === 'gopd' &&
           (function () { var { 0: x, ...r } = [7, 8]; return x === 7 && r[1] === 8; })()",
    );
}

#[test]
fn object_rest_charges_its_steps_against_the_budget() {
    sweep_instruction_budget(
        "var { a, b, ...rest } = { a: 1, b: 2, c: 3, d: 4 }; var copy = { ...rest, e: 5 };
         rest.c + copy.e === 8",
        Value::Bool(true),
    );
}

#[test]
fn class_heritage_is_validated() {
    check(
        "var protoThrows = new Proxy(function () {}, { get(t, k) { if (k === 'prototype') throw 'proto'; return t[k]; } });
         var badProto = function () {}; badProto.prototype = 1;
         function Plain() {} Plain.prototype = null;
         isType(() => { class X extends 5 {} }) && isType(() => { class X extends (() => {}) {} }) &&
           isType(() => { class X extends badProto {} }) &&
           thrown(() => { class X extends protoThrows {} }) === 'proto' &&
           (function () { class X extends Plain {} return Object.getPrototypeOf(X.prototype) === null; })() &&
           (function () { class X extends null {} return Object.getPrototypeOf(X) === Function.prototype && isType(() => new X()); })() &&
           isType(() => { class X extends Object { constructor() { super(); } } X.__proto__ = 1; new X(); }) === false",
    );
}

#[test]
fn super_calls_and_property_accesses_report_their_failures() {
    check(
        "class A { constructor() { this.made = true; } get g() { return 'g'; } }
         class NoSuper extends A { constructor() {} }
         class Returns extends A { constructor() { super(); return 1; } }
         class Overrides extends A { constructor() { return { own: true }; } }
         class Reassigns extends A { constructor() { super(); super(); } }
         var obj = { __proto__: null, m() { return super.x; } };
         var setObj = { __proto__: null, m() { super.x = 1; } };
         class StrictSet extends A { static set() { super.g = 1; } }
         Object.defineProperty(StrictSet, 'g', { value: 0, writable: false });
         isRef(() => new NoSuper()) && isType(() => new Returns()) && new Overrides().own &&
           isRef(() => new Reassigns()) && isType(() => obj.m()) && isType(() => setObj.m()) &&
           isType(() => StrictSet.set()) &&
           (function () { class B extends A { constructor() { super(); this.z = super.g; } } return new B().z === 'g'; })() &&
           isType(() => { class C extends A { constructor() { Object.setPrototypeOf(C, Math.max); super(); } } new C(); })",
    );
}

#[test]
fn private_members_check_brands_slots_and_accessors() {
    check(
        "class C {
           #field = 1; #method() { return 'm'; } set #wo(v) {} get #ro() { return 1; }
           static read(o) { return o.#field; }
           static write(o, v) { o.#field = v; }
           static callMethod(o) { return o.#method(); }
           static assignMethod(o) { o.#method = 1; }
           static getSetterOnly(o) { return o.#wo; }
           static setGetterOnly(o) { o.#ro = 1; }
           static has(o) { return #field in o; }
         }
         var inst = new C();
         C.read(inst) === 1 && (C.write(inst, 5), C.read(inst) === 5) && C.callMethod(inst) === 'm' &&
           isType(() => C.read({})) && isType(() => C.read(1)) && isType(() => C.write({}, 1)) &&
           isType(() => C.callMethod({})) && isType(() => C.assignMethod(inst)) &&
           isType(() => C.getSetterOnly(inst)) && isType(() => C.setGetterOnly(inst)) && C.has(inst) && !C.has({})",
    );
}

#[test]
fn private_names_reject_uninitialized_reads_and_double_stamping() {
    check(
        "class Uninitialized { a = this.#b; #b = 1; }
         class UninitializedWrite { a = (this.#b = 2); #b; }
         class Base { constructor(o) { return o; } }
         class Stamper extends Base { #stamp = 1; static has(o) { return #stamp in o; } }
         class Ordinary { #x = 1; static has(o) { return #x in o; } static getter(o) { return o.#w; } get #w() { return 1; } static setW(o) { o.#w = 1; } set #v(x) {} static getV(o) { return o.#v; } static setNoSetter(o) { o.#w = 2; } }
         var target = {}; new Stamper(target);
         isType(() => new Uninitialized()) && isType(() => new UninitializedWrite()) &&
           isType(() => new Stamper(target)) && Stamper.has(target) && isType(() => Ordinary.has(1)) &&
           isType(() => new Stamper(Object.preventExtensions({}))) &&
           Ordinary.has(new Ordinary()) && !Ordinary.has({}) &&
           isType(() => Ordinary.setW(new Ordinary())) && isType(() => Ordinary.getV(new Ordinary())) &&
           isType(() => Ordinary.setNoSetter(new Ordinary())) &&
           thrown(() => new Stamper(new Proxy({}, { isExtensible() { throw 'ext'; } }))) === 'ext'",
    );
}

#[test]
fn computed_keys_name_anonymous_functions() {
    check(
        "var s = Symbol('d'), anon = Symbol();
         var o = { [1]: function () {}, [true]: () => {}, [s]: function () {}, [anon]: function () {}, [null]: class {},
                   get [s]() { return 1; }, set [anon](v) {} };
         var named = { [1n]: function () {} };
         class K { static [1]() {} static get [s]() { return 1; } static name() {} static ['x'] = function () {}; }
         o[1].name === '1' && o[true].name === 'true' && o[null].name === 'null' &&
           Object.getOwnPropertyDescriptor(o, s).get.name === 'get [d]' &&
           Object.getOwnPropertyDescriptor(o, anon).set.name === 'set ' &&
           named[1].name === '1' && K[1].name === '1' && typeof K.name === 'function' && K.x.name === 'x'",
    );
}

#[test]
fn function_calls_cover_receivers_constructors_and_generators() {
    check(
        "function sloppy() { return this; }
         function strict() { 'use strict'; return this; }
         var arrow = () => 1;
         class K {}
         typeof sloppy.call(1) === 'object' && sloppy.call(undefined) === globalThis && strict.call(undefined) === undefined &&
           strict.call(1) === 1 && isType(() => new arrow()) && isType(() => K()) &&
           (function () { function* g() {} g.prototype = null; return Object.getPrototypeOf(g()) === Object.getPrototypeOf(function* () {}).prototype; })() &&
           (function () { function* g(a = (g.prototype = null)) {} return typeof g() === 'object'; })() &&
           (function () { async function* g() {} g.prototype = 1; return typeof g() === 'object'; })() &&
           (function () { function f() { eval('var x = 1'); delete x; return typeof x; } return f() === 'undefined'; })() &&
           (function () { function f(a = eval('1')) { return a; } return f() === 1; })() &&
           (function () { function f() { return new.target; } return new f() === f && f() === undefined; })()",
    );
}

#[test]
fn async_functions_close_iterators_when_they_throw_synchronously() {
    expect_async_true(
        "var log = [];
         var iterable = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; }, return() { log.push('closed'); return {}; } }; } };
         async function f() { for (var x of iterable) { throw 'sync'; } }
         async function g() { try { for (var x of iterable) { await null; throw 'later'; } } catch (e) { return e; } }
         async function withReturnValue() { return { get then() { throw 'then'; } }; }
         f().then(() => 'no', e => e).then(v => g().then(w => [v, w])).then(([v, w]) => withReturnValue().then(() => 'no', e => [v, w, e])).then(r => {
           globalThis.result = r.join() === 'sync,later,then' && log.length === 2 || r.join() + log.length;
         });",
    );
}
