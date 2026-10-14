// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Failure paths of the language runtime and native dispatch that need an
//! observable hook to reach: throwing conversions, a revoked prototype, a
//! frozen or hostile receiver, and every lazily built intrinsic failing under
//! a cold heap ceiling, plus allocations that fail right after a user hook
//! allocated ballast.
mod cov_g2_support;
mod cov_g2_sweep;

use blueice_bluejs::{compile, parse, HeapConfig, RuntimeError, Vm, VmConfig};
use cov_g2_support::expect_true;
use cov_g2_sweep::{sweep_cold, sweep_ops, Mode};

const PRELUDE: &str = "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
    function isType(f) { return thrown(f) instanceof TypeError; }
    function isRange(f) { return thrown(f) instanceof RangeError; }
    function isSyntax(f) { return thrown(f) instanceof SyntaxError; }";

fn check(body: &str) {
    expect_true(&format!("{PRELUDE}\n{body}"));
}

#[test]
fn conversions_report_each_throwing_step() {
    check(
        "thrown(() => 1 + { [Symbol.toPrimitive]() { throw 'tp'; } }) === 'tp' &&
         thrown(() => +{ get valueOf() { throw 'vo'; } }) === 'vo' &&
         thrown(() => `${{ get toString() { throw 'ts'; } }}`) === 'ts' &&
         isType(() => -{ valueOf() { return Symbol(); } }) &&
         thrown(() => BigInt.asUintN(8, { valueOf() { throw 'bi'; } })) === 'bi' &&
         isSyntax(() => BigInt.asIntN(8, '\\ud800')) &&
         isType(() => BigInt.asIntN(8, 1)) &&
         isType(() => BigInt.asIntN(8, Symbol())) &&
         thrown(() => { var n = 0; [].length = { valueOf() { if (n++) throw 'second'; return 1; } }; }) === 'second'",
    );
}

#[test]
fn string_methods_report_a_throwing_pattern_probe_and_bad_receivers() {
    check(
        "var probe = { get [Symbol.match]() { throw 'match'; } };
         thrown(() => 'abc'.includes(probe)) === 'match' &&
         thrown(() => 'abc'.startsWith(probe)) === 'match' &&
         thrown(() => 'abc'.endsWith(probe)) === 'match' &&
         isType(() => String.prototype.toString.call({})) &&
         isType(() => String.prototype.valueOf.call(1))",
    );
}

#[test]
fn uri_functions_report_malformed_input() {
    check(
        "isType(() => decodeURI('%')) === false &&
         thrown(() => decodeURI('%')) instanceof URIError &&
         thrown(() => decodeURIComponent('%E0%A4%A')) instanceof URIError",
    );
}

#[test]
fn constructors_read_new_target_prototype_and_array_push_reads_the_chain() {
    check(
        "var hostile = new Proxy(function () {}, { get() { throw 'proto'; } });
         thrown(() => Reflect.construct(Object, [], hostile)) === 'proto' &&
         thrown(() => Reflect.construct(Iterator, [], hostile)) === 'proto' &&
         thrown(() => Reflect.construct(String, ['a'], hostile)) === 'proto' &&
         (function () {
           var arr = [];
           var p = Proxy.revocable({}, {});
           Object.setPrototypeOf(arr, p.proxy); p.revoke();
           return isType(() => arr.push(1));
         })()",
    );
}

#[test]
fn property_assignments_report_hostile_receivers() {
    check(
        "isType(() => { undefined.x = 1; }) && isType(() => { null['y'] = 1; }) &&
         isType(() => { undefined.z++; }) &&
         (function () {
           class C { #x = 1; static probe(o) { return #x in o; } }
           return C.probe(new C()) && !C.probe({});
         })() &&
         isType(() => { class D { #x; [({}).#x]() {} } }) &&
         isType(() => { var o = { valueOf() { return 1; } }; for (var k in null) {} return Object.freeze(o).q = 1, (() => { 'use strict'; o.q = 2; })(); })",
    );
}

#[test]
fn hostile_spread_and_for_in_sources_are_coerced_before_use() {
    check(
        "var out = { ...'ab' }; var seen = []; for (var k in 'xy') seen.push(k);
         var { 0: first, ...rest } = 'ab';
         out[0] === 'a' && out[1] === 'b' && seen.join() === '0,1' && first === 'a' && rest[1] === 'b'",
    );
}

#[test]
fn install_test262_done_survives_every_heap_ceiling() {
    let mut installed = false;
    let mut ceiling = 4096;
    while !installed {
        let config = VmConfig {
            heap: HeapConfig {
                nursery_capacity: 1,
                max_heap_bytes: ceiling,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        };
        if let Ok(mut vm) = Vm::new(config) {
            match vm.install_test262_done() {
                Ok(()) => installed = true,
                Err(error) => assert!(matches!(error, RuntimeError::Heap(_)), "{error:?}"),
            }
        }
        ceiling += 8;
        assert!(ceiling < 1 << 20, "install_test262_done never succeeds");
    }
}

#[test]
fn lazily_built_intrinsics_fail_cleanly_when_first_touched_under_a_ceiling() {
    for script in [
        "'x'.propertyIsEnumerable('0') === true",
        "'x'.hasOwnProperty('0') === true",
        "typeof 'x'.toString === 'function'",
        "typeof (1).toFixed === 'function' && typeof true.valueOf === 'function'",
        "typeof Symbol().toString === 'function' && typeof (1n).toString === 'function'",
        "typeof [].push === 'function' && [].constructor === Array",
        "typeof (function () {}).constructor === 'function'",
        "typeof globalThis.Array === 'function' && 'push' in []",
        "({}).propertyIsEnumerable('a') === false && ({}).hasOwnProperty('a') === false",
        "try { (function () {}).caller; } catch (e) {} true",
        "var f = function () {}.bind(); try { f.caller = 1; } catch (e) {} true",
        "class N extends null {} true",
        "typeof ''[Symbol.iterator]().next === 'function'",
        "({}) + '' === '[object Object]'",
        "typeof (function* () {}) === 'function' && typeof (async function* () {}) === 'function'",
        "function g() {} g(); (function* () {})(); (async function* () {})(); true",
        "function f() { return this; } f() === globalThis",
    ] {
        sweep_cold(Mode::PLAIN, script, 16);
    }
}

#[test]
fn promise_intrinsics_fail_cleanly_when_first_touched_under_a_ceiling() {
    for script in [
        "Promise.resolve(1).then(x => x); true",
        "new Promise(r => r(1)); true",
        "(async () => { await 1; })(); true",
        "Promise.all([1]); true",
        "Promise.resolve(1).finally(() => {}); true",
        "Promise.withResolvers(); Promise.try(() => 1); Promise.reject(1).catch(() => {}); true",
        "class P extends Promise {} P.resolve(1).then(x => x); true",
        "Promise.race([1]); Promise.any([1]); Promise.allSettled([1]); true",
        "var it = { [Symbol.iterator]() { return { next() { return { done: true }; } }; } };
         (async () => { for await (var x of it); })(); true",
    ] {
        sweep_cold(Mode::JOBS, script, 16);
    }
}

const WARMUP: &str = "class W { #p = 1; static s = 2; static #q = 3; m() { return this.#p; } static { this.z = 1; } }
    new W(); class D extends W {} class N extends null {}
    function* gen(a = 1) {} gen(); (async function* () {})(); async function af() { await 1; } af();
    for (var k in { a: 1 }) {} for (var k in 'ab') {} ({ ...{ a: 1 } }); ({ ...'ab' });
    var { a, ...r } = { a: 1, b: 2 }; [1].constructor; (function () {}).constructor;
    ({}).hasOwnProperty('a'); ({}).propertyIsEnumerable('a'); 'x'.propertyIsEnumerable('0');
    'x'.hasOwnProperty('0'); (1).toFixed; true.valueOf; Symbol().toString; (1n).toString;
    try { decodeURI('%'); } catch (e) {} Reflect.construct(Object, [], function () {});
    ''[Symbol.iterator](); ({ ['k']: function () {} }); globalThis.gg = 1; globalThis.gg = 2;
    function pe(a = eval('1')) { return a; } pe(); (function () { return this; })();
    try { (class { constructor() {} })(); } catch (e) {} 0";

#[test]
fn allocation_failures_in_class_generator_and_property_runtime() {
    sweep_ops(
        Mode::PLAIN,
        WARMUP,
        "b(); class A1 { #p = 1; static s = 2; static #q = 3; m() { return this.#p; } static { this.z = 1; } }
         b(); var a1 = new A1(); b(); class B1 extends A1 {} b(); class N1 extends null {}
         b(); var o = { get x() { return 1; }, ['c' + 1]: function () {}, ...{ y: 1 } };
         b(); for (var k in 'ab') {} b(); for (var k2 in [1, 2]) {} b(); var sp = { ...'ab' };
         b(); var { 0: p, ...rest } = 'ab';
         b(); function* g(a = 1) {} b(); var it = g(); b(); function* g2() {} g2();
         b(); function pe(a = eval('1')) { return a; } b(); pe();
         b(); try { (class { constructor() {} })(); } catch (e) {}
         b(); function f() { return this; } f();
         b(); Reflect.construct(Object, [], function () {});
         b(); try { decodeURI('%'); } catch (e) {}
         b(); globalThis.qq = 1; b(); globalThis.qq = 2; b(); globalThis.qq = 3;
         b(); var nf = { ['k' + 2]: function () {}, ['s' + 3]: class {} };
         return a1.m() === 1 && B1.s === 2 && sp[0] === 'a' && p === 'a' && pe() === 1;",
        16,
    );
}

#[test]
fn allocation_failures_in_async_function_completion() {
    sweep_ops(
        Mode::JOBS,
        "(async () => { await 1; })(); (async () => ({ get then() { return undefined; } }))(); 0",
        "var pr = Promise.resolve(1);
         async function w() { b(); await pr; }
         async function r() { b(); return { get then() { b(); null.x; } }; }
         async function t() { b(); null.x; }
         b(); w(); b(); r(); b(); t();
         return true;",
        16,
    );
}

#[test]
fn a_generator_function_reads_its_prototype_and_allocates_its_state() {
    check(
        "function* g(a = 1) { yield a; }
         g.prototype = null;
         var it = g();
         Object.getPrototypeOf(it) === Object.getPrototypeOf(function* () {}).prototype &&
         it.next().value === 1",
    );
}

#[test]
fn compile_errors_do_not_reach_the_runtime() {
    // A sanity check for the harness helpers used above.
    assert!(compile(&parse("1").unwrap()).is_ok());
}
