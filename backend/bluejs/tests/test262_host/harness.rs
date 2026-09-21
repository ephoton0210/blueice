// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluejs::VmConfig;

#[test]
fn native_uri_decode_fixture_helper_exhaustively_checks_the_shared_decode_operation() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "__bluejsTest262DecodeUriExhaustive(decodeURI,3)",
        "__bluejsTest262DecodeUriExhaustive(decodeURIComponent,3)",
        "assert.throws(TypeError,()=>__bluejsTest262DecodeUriExhaustive(decodeURI,2));true",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn complete_core_harness_helpers_are_available() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "assert(isPrimitive(null) && !isPrimitive({}) && isNegativeZero(-0) && !isNegativeZero(0))",
        "assert(compareArray([1],[1]) && !compareArray([1],[2]) && !compareArray([],[1]))",
        "assert.sameValue(compareArray.format([1,'x',Symbol('s')]),'[1, x, Symbol(s)]')",
        "assert.sameValue(formatIdentityFreeValue({}),undefined)",
        "assert.sameValue(formatIdentityFreeValue('x'),'\"x\"')",
        "assert.sameValue(formatIdentityFreeValue(-0),'-0')",
        "assert.sameValue(formatSimpleValue(Symbol('s')),'Symbol(s)')",
        "assert.sameValue(formatSimpleValue({toString(){return 'x';}}),'x')",
        "assert.sameValue(formatSimpleValue({toString:0,valueOf:0}),'[object Object]')",
        "assert.throws(Test262Error,()=>assert.compareArray('x','x'))",
        // A TypeError from ToString is absorbed (upstream inspects the caught
        // value's `name`, so a script-thrown one counts too); any other
        // exception propagates.
        "assert.sameValue(formatSimpleValue({toString(){throw new TypeError();}}),'[object Object]')",
        "assert.throws(RangeError,()=>formatSimpleValue({toString(){throw new RangeError();}}))",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Undefined,
            "{source}"
        );
    }
}

/// The corpus's own `sta.js` and `assert.js`, executed unchanged in a VM that
/// has no native harness. This is the oracle the native `assert` family is
/// measured against: the two must agree on every outcome and every message.
fn upstream_assert_vm() -> Vm {
    let mut vm = Vm::default();
    for source in [
        include_str!("../../../../development/browser_core/reference/test262/harness/sta.js"),
        include_str!("../../../../development/browser_core/reference/test262/harness/assert.js"),
    ] {
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
    }
    vm
}

/// Runs one expression and describes its completion as a string: the result's
/// type and text, or the thrown value's constructor name and message with the
/// message's type. Both are read through ordinary JavaScript, so the native
/// and upstream helpers are compared by what a test can observe.
fn probe(vm: &mut Vm, expression: &str) -> String {
    let source = format!(
        "(function () {{
           function describe(v) {{ return typeof v + ':' + String(v); }}
           try {{
             return 'returned ' + describe((function () {{ return ({expression}); }})());
           }} catch (e) {{
             if (e === null || typeof e !== 'object') return 'threw primitive ' + describe(e);
             return 'threw ' + describe(e.constructor && e.constructor.name) + ' with message ' +
               describe(e.message);
           }}
         }})()"
    );
    match vm
        .execute_script(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap()
    {
        // A message may hold an unpaired surrogate, which a Rust string
        // cannot: spell such a unit out so it still takes part in the
        // comparison.
        Value::String(text) => char::decode_utf16(text.as_code_units().iter().copied())
            .map(|unit| {
                unit.map_or_else(
                    |error| format!("\\u{{{:04x}}}", error.unpaired_surrogate()),
                    String::from,
                )
            })
            .collect(),
        other => panic!("probe of {expression} completed with {other:?}"),
    }
}

fn assert_native_matches_upstream(expressions: &[&str]) {
    // A one-object nursery makes nearly every allocation a collection point,
    // so a native helper that leaves a value unrooted fails here instead of
    // depending on where an ordinary collection happens to land.
    let mut stressed = VmConfig::default();
    stressed.heap.nursery_capacity = 1;
    let mut upstream = upstream_assert_vm();
    let mut disagreements = Vec::new();
    for config in [VmConfig::default(), stressed] {
        let mut native = Vm::new(config).unwrap();
        native.install_test262_harness().unwrap();
        for expression in expressions {
            let expected = probe(&mut upstream, expression);
            let actual = probe(&mut native, expression);
            if actual != expected {
                disagreements.push(format!(
                    "{expression}\n    upstream: {expected}\n    native:   {actual}"
                ));
            }
        }
    }
    assert!(
        disagreements.is_empty(),
        "native assert helpers disagree with the upstream harness:\n{}",
        disagreements.join("\n")
    );
}

#[test]
fn native_assert_reports_exactly_what_the_upstream_harness_reports() {
    assert_native_matches_upstream(&[
        "assert(true)",
        "assert(false)",
        "assert(false, 'custom')",
        "assert(false, undefined)",
        "assert(false, '')",
        // The message is passed to `new Test262Error(message)` as it is: a
        // falsy one becomes "", any other keeps its type and value.
        "assert(false, 0)",
        "assert(false, null)",
        "assert(false, NaN)",
        "assert(false, 5)",
        "assert(false, true)",
        "assert(false, 1n)",
        "assert(false, {})",
        "assert(false, { toString() { return 'object message'; } })",
        "assert(false, Symbol('symbol message'))",
        "assert(false, '\\ud800 lone surrogate')",
        "assert.sameValue(1, 2, '\\udc00')",
        "assert.throws(TypeError, () => {}, '\\ud800')",
        "assert.compareArray([1], [2], '\\ud800')",
        "assert(1)",
        "assert('s')",
        "assert('quote\"and\\\\slash\\n')",
        "assert(undefined)",
        "assert(null, 'n')",
        "assert(-0)",
        "assert(0)",
        "assert(NaN)",
        "assert(1n)",
        "assert({})",
        "assert(Symbol('x'))",
        "assert(function f() {})",
        "assert({ toString() { throw new RangeError('boom'); } })",
        "assert({ toString() { throw new TypeError('boom'); } })",
        "assert({ toString: 0, valueOf: 0 })",
        "assert(Object.create(null))",
        "assert(Object.getPrototypeOf(async function () {}))",
    ]);
}

#[test]
fn native_same_value_assertions_report_exactly_what_the_upstream_harness_reports() {
    assert_native_matches_upstream(&[
        "assert.sameValue(1, 1)",
        "assert.sameValue(NaN, NaN)",
        "assert.sameValue(0, -0)",
        "assert.sameValue(-0, 0, 'zeros')",
        "assert.sameValue(1, 2)",
        "assert.sameValue('a', 'b', 'a message')",
        "assert.sameValue('a\"b', 'a\\\\b')",
        "assert.sameValue({}, {})",
        "assert.sameValue(Symbol('a'), Symbol('a'))",
        "assert.sameValue(1n, 1)",
        "assert.sameValue(2n, 3n)",
        "assert.sameValue(null, undefined)",
        "assert.sameValue(true, false, '')",
        "assert.sameValue(1, 2, 5)",
        "assert.sameValue(1, 2, null)",
        "assert.sameValue(1, 2, { toString() { return 'object message'; } })",
        "assert.sameValue(1, 2, Symbol('message'))",
        "assert.sameValue({ toString() { throw new RangeError('boom'); } }, 1)",
        "assert.sameValue(1, { toString() { throw new TypeError('boom'); } })",
        "assert.sameValue(1, 2, { toString() { throw new RangeError('boom'); } })",
        "assert.sameValue(Object.getPrototypeOf(async function () {}), 1)",
        "assert.notSameValue(1, 2)",
        "assert.notSameValue(0, -0)",
        "assert.notSameValue(1, 1)",
        "assert.notSameValue(1, 1, 'unwanted')",
        "assert.notSameValue(NaN, NaN)",
        "assert.notSameValue('a', 'a', { toString() { return 'obj'; } })",
        "assert.notSameValue(-0, -0)",
        "assert.notSameValue(null, null, Symbol())",
        "assert._isSameValue(1, 1) + ',' + assert._isSameValue(0, -0) + ',' + assert._isSameValue(NaN, NaN)",
    ]);
}

#[test]
fn native_throws_assertion_reports_exactly_what_the_upstream_harness_reports() {
    assert_native_matches_upstream(&[
        "assert.throws(TypeError, () => { throw new TypeError('x'); })",
        "assert.throws(TypeError, () => {})",
        "assert.throws(TypeError, () => {}, 'no throw')",
        "assert.throws(TypeError, () => { throw new RangeError('x'); })",
        "assert.throws(TypeError, () => { throw new RangeError('x'); }, 'wrong one')",
        "assert.throws(TypeError, () => { throw 1; })",
        "assert.throws(TypeError, () => { throw null; }, 'null')",
        "assert.throws(TypeError, () => { throw undefined; })",
        "assert.throws(TypeError, () => { throw 'text'; })",
        "assert.throws(TypeError, () => { throw Symbol('s'); })",
        // A thrown function is not an object as far as this helper is
        // concerned, whatever its `constructor` property says.
        "assert.throws(Function, () => { throw function () {}; })",
        "assert.throws(TypeError, () => { throw { constructor: TypeError }; })",
        "assert.throws(TypeError, 1)",
        "assert.throws(TypeError)",
        "assert.throws(TypeError, null, 'msg')",
        "assert.throws(TypeError, () => { null.x; })",
        "assert.throws(TypeError, () => { null.x; }, 'engine TypeError')",
        "assert.throws(ReferenceError, () => { undeclaredVariable; })",
        "assert.throws(SyntaxError, () => { eval('var'); })",
        "assert.throws(RangeError, () => { new Array(-1); })",
        "assert.throws(RangeError, () => { null.x; })",
        "assert.throws(Test262Error, () => { throw new Test262Error('x'); })",
        "assert.throws(Test262Error, () => { assert(false); })",
        "assert.throws(Test262Error, () => { assert.sameValue(1, 2); })",
        "assert.throws(TypeError, () => { assert(false); })",
        "assert.throws(Error, () => { throw new TypeError(); })",
        "assert.throws(function TypeError() {}, () => { throw new TypeError(); })",
        "assert.throws({}, () => { throw new TypeError(); })",
        "assert.throws(undefined, () => {})",
        "assert.throws(TypeError, () => {}, { toString() { return 'obj'; } })",
        "assert.throws(TypeError, () => { throw new TypeError(); }, Symbol('unused'))",
        "assert.throws(TypeError, () => { throw new RangeError(); }, Symbol('used'))",
        "assert.throws(TypeError, () => { throw { get constructor() { throw new RangeError('getter'); } }; })",
        "assert.throws(TypeError, () => { throw Object.create(null); })",
        "assert.throws(TypeError, () => { throw new Proxy(new TypeError(), {}); })",
        "assert.throws(TypeError, () => { throw new Proxy({}, { get() { return TypeError; } }); })",
        "assert.throws(RangeError, () => { throw new Proxy({}, { get() { return TypeError; } }); })",
        "assert.throws(TypeError, () => { throw Object.create(TypeError.prototype); })",
        "assert.throws(TypeError, () => { class Sub extends TypeError {} throw new Sub(); })",
        "(function () { var calls = 0; assert.throws(TypeError, function () { calls += arguments.length + (this === undefined ? 100 : 1); throw new TypeError(); }); return calls; })()",
        "assert.throws.length + ',' + assert.sameValue.length + ',' + assert.notSameValue.length + ',' + assert.length",
    ]);
}

#[test]
fn native_compare_array_reports_exactly_what_the_upstream_harness_reports() {
    assert_native_matches_upstream(&[
        "assert.compareArray([1, 2], [1, 2])",
        "assert.compareArray([], [])",
        "assert.compareArray([1, 2], [1, 3])",
        "assert.compareArray([1], [1, 2], 'shorter')",
        "assert.compareArray([1, 2], [1], 'longer')",
        "assert.compareArray([NaN], [NaN])",
        "assert.compareArray([0], [-0])",
        "assert.compareArray([Symbol()], [Symbol('desc')])",
        "assert.compareArray([1, , 3], [1, 2, 3])",
        "assert.compareArray([1, , 3], [1, , 3])",
        "assert.compareArray([undefined], [])",
        "assert.compareArray([null, undefined, 'x'], [])",
        "assert.compareArray({ length: 2, 0: 'a', 1: 'b' }, ['a', 'b'])",
        "assert.compareArray(['a', 'b'], { length: 2, 0: 'a', 1: 'b' })",
        "assert.compareArray({ length: 3, 0: 0, 1: 'a', 2: undefined }, [], 'array-like')",
        "assert.compareArray({}, {})",
        "assert.compareArray({}, { length: 1 })",
        "assert.compareArray((function () { return arguments; })(1, 'a'), [1, 'a'])",
        "assert.compareArray([], (function () { return arguments; })(0, 'a', undefined), '[] and arguments')",
        "assert.compareArray('abc', [])",
        "assert.compareArray([], 'abc', 'text')",
        "assert.compareArray(null, [])",
        "assert.compareArray(undefined, [], 'foo')",
        "assert.compareArray()",
        "assert.compareArray([])",
        "assert.compareArray([], undefined, 'foo')",
        "assert.compareArray(1, [])",
        "assert.compareArray(0, [])",
        "assert.compareArray(false, [])",
        "assert.compareArray([], 1n, 'big')",
        "assert.compareArray(Symbol('s'), [])",
        "assert.compareArray([], [], Symbol('message'))",
        "assert.compareArray([1], [2], Symbol('message'))",
        "assert.compareArray([1], [2], { toString() { return 'obj'; } })",
        "assert.compareArray([1], [2], 1n)",
        "assert.compareArray([1], [2], () => {})",
        "assert.compareArray([], [], true)",
        "assert.compareArray([1, 'two', 3n, null, undefined, true, {}, Symbol('s')], [])",
        "compareArray([1], [1]) + ',' + compareArray([1], [2]) + ',' + compareArray([], [1]) + ',' + compareArray([NaN], [NaN]) + ',' + compareArray([0], [-0])",
        "compareArray({ length: '1', 0: 'x' }, { length: '1', 0: 'x' })",
        "compareArray({ length: '1', 0: 'x' }, { length: 1, 0: 'x' })",
        "compareArray({}, {})",
        "compareArray.format([1, 'x', Symbol('s')])",
        "compareArray.format([1, , 3])",
        "compareArray.format([])",
        "compareArray.format({ length: 2, 0: 'a' })",
        "compareArray.format('abc')",
        "compareArray.format(null)",
        "compareArray.format({ length: 1, 0: { toString() { throw new RangeError('boom'); } } })",
        "(function () { var reads = []; compareArray({ get length() { reads.push('a.length'); return 1; }, get 0() { reads.push('a[0]'); return 1; } }, { get length() { reads.push('b.length'); return 1; }, get 0() { reads.push('b[0]'); return 1; } }); return reads.join(); })()",
        "compareArray.length + ',' + compareArray.format.length + ',' + assert.compareArray.length",
    ]);
}

#[test]
fn native_formatting_and_predicate_helpers_match_the_upstream_harness() {
    assert_native_matches_upstream(&[
        "isPrimitive(null) + ',' + isPrimitive(undefined) + ',' + isPrimitive(0) + ',' + isPrimitive('') + ',' + isPrimitive('s') + ',' + isPrimitive(1n) + ',' + isPrimitive(Symbol()) + ',' + isPrimitive(false) + ',' + isPrimitive(true)",
        "isPrimitive({}) + ',' + isPrimitive([]) + ',' + isPrimitive(function () {}) + ',' + isPrimitive(Object.create(null))",
        "isPrimitive()",
        "isNegativeZero(-0) + ',' + isNegativeZero(0) + ',' + isNegativeZero(NaN) + ',' + isNegativeZero(-1) + ',' + isNegativeZero(0n) + ',' + isNegativeZero('-0') + ',' + isNegativeZero({})",
        "formatIdentityFreeValue('text')",
        "formatIdentityFreeValue('quote\"backslash\\\\newline\\n tab\\t nul\\u0000 emoji\\u{1f600} lone\\ud800')",
        "formatIdentityFreeValue('')",
        "formatIdentityFreeValue(-0)",
        "formatIdentityFreeValue(0)",
        "formatIdentityFreeValue(42.5)",
        "formatIdentityFreeValue(NaN)",
        "formatIdentityFreeValue(-Infinity)",
        "formatIdentityFreeValue(true)",
        "formatIdentityFreeValue(undefined)",
        "formatIdentityFreeValue(null)",
        "formatIdentityFreeValue(12n)",
        "formatIdentityFreeValue(-12n)",
        "formatIdentityFreeValue(Symbol('s'))",
        "formatIdentityFreeValue({})",
        "formatIdentityFreeValue(function () {})",
        "formatSimpleValue('text')",
        "formatSimpleValue(-0)",
        "formatSimpleValue(0)",
        "formatSimpleValue(1e21)",
        "formatSimpleValue(12n)",
        "formatSimpleValue(undefined)",
        "formatSimpleValue(null)",
        "formatSimpleValue(false)",
        "formatSimpleValue(Symbol('s'))",
        "formatSimpleValue(Symbol())",
        "formatSimpleValue({ toString() { return 'custom'; } })",
        "formatSimpleValue({ toString: 0, valueOf: 0 })",
        "formatSimpleValue({ toString() { throw new TypeError('boom'); } })",
        "formatSimpleValue({ toString() { throw new RangeError('boom'); } })",
        "formatSimpleValue({ toString() { throw { name: 'TypeError' }; } })",
        "formatSimpleValue({ toString() { throw 'primitive'; } })",
        "formatSimpleValue({ toString() { throw null; } })",
        "formatSimpleValue({ toString() { throw undefined; } })",
        "formatSimpleValue(Object.create(null))",
        "formatSimpleValue([1, [2, 3]])",
        "formatSimpleValue(function f() {})",
        "formatSimpleValue(Object.getPrototypeOf(async function () {}))",
        "formatSimpleValue(new Proxy({}, {}))",
        "assert._toString === formatSimpleValue && assert._formatIdentityFreeValue === formatIdentityFreeValue",
        "formatSimpleValue.length + ',' + formatIdentityFreeValue.length + ',' + isPrimitive.length + ',' + isNegativeZero.length",
    ]);
}

#[test]
fn native_property_helpers_validate_descriptors_and_constructibility() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "verifyProperty(Math,'PI',{value:Math.PI,writable:false,enumerable:false,configurable:false});true",
        "verifyCallableProperty(Math,'abs','abs',1);true",
        "verifyPrimordialCallableProperty(Math,'abs','abs',1);true",
        "verifyEqualTo(Math,'PI',Math.PI);true",
        "verifyNotWritable(Math,'PI');verifyNotEnumerable(Math,'PI');verifyNotConfigurable(Math,'PI');true",
        "verifyWritable(Math,'abs');verifyEnumerable({x:1},'x');verifyConfigurable({x:1},'x');true",
        "verifyPrimordialProperty(Math,'PI',{value:Math.PI,writable:false,enumerable:false,configurable:false});true",
        "let o={};Object.defineProperty(o,'x',{get:function getter(){return 1},set:undefined,enumerable:false,configurable:true});verifyAccessorProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,set:undefined});verifyPrimordialAccessorProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,set:undefined});true",
        "isConstructor(function(){}) && !isConstructor(()=>{})",
        "assert.throws(Test262Error,()=>isConstructor(1));assert.throws(Test262Error,()=>verifyProperty(Math,'PI',undefined));true",
        "assert.throws(TypeError,()=>verifyProperty(1,'x',{}));true",
        "verifyCallableProperty(Math,'abs','abs',1,{writable:true,enumerable:false,configurable:true});true",
        "verifyCallableProperty(Math,'abs',undefined,1);verifyCallableProperty(Math,'abs','abs',1,{writable:true,enumerable:false});true",
        "assert.throws(Test262Error,()=>verifyCallableProperty(Math,'abs','wrong',1));true",
        "let o={};Object.defineProperty(o,Symbol.iterator,{value:function(){},writable:true,enumerable:false,configurable:true});assert.throws(Test262Error,()=>verifyCallableProperty(o,Symbol.iterator,undefined,0));true",
        "let f=function f(){};Object.defineProperty(f,'name',{configurable:false});let o={};Object.defineProperty(o,'f',{value:f,writable:true,enumerable:false,configurable:true});assert.throws(Test262Error,()=>verifyCallableProperty(o,'f','f',0,{writable:true,enumerable:false,configurable:true}));assert.throws(Test262Error,()=>verifyCallableProperty(o,'f','f',0));true",
        "let o={};Object.defineProperty(o,'x',{get:function(){return 1},enumerable:false,configurable:true});assert.throws(Test262Error,()=>verifyAccessorProperty(o,'x',{get:undefined}));assert.throws(Test262Error,()=>verifyAccessorProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,enumerable:true}));assert.throws(Test262Error,()=>verifyAccessorProperty(Math,'PI',{}));true",
        "assert.throws(Test262Error,()=>verifyProperty(Math,'PI',{unknown:1}));true",
    ] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true), "{source}");
    }
    let failure = compile(&parse("verifyProperty(Math,'PI',{writable:true})").unwrap()).unwrap();
    assert!(matches!(
        vm.execute(&failure),
        Err(RuntimeError::Test262(_))
    ));
    for source in [
        "verifyCallableProperty(Math,'PI','PI',0)",
        "let o={};Object.defineProperty(o,'f',{value:function f(){},writable:false,enumerable:false,configurable:true});verifyCallableProperty(o,'f','f',0)",
        "let o={};Object.defineProperty(o,Symbol.iterator,{value:function(){},writable:true,enumerable:false,configurable:true});verifyCallableProperty(o,Symbol.iterator,undefined,0)",
        "let f=function f(){};Object.defineProperty(f,'name',{configurable:false});let o={};Object.defineProperty(o,'f',{value:f,writable:true,enumerable:false,configurable:true});verifyCallableProperty(o,'f','f',0,{writable:true,enumerable:false,configurable:true})",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        assert!(matches!(vm.execute(&compile(&parse(source).unwrap()).unwrap()), Err(RuntimeError::Test262(_))), "{source}");
    }
    let source = "let o={};Object.defineProperty(o,'x',{get:function(){return 1},set:undefined,enumerable:false,configurable:true});verifyProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,set:undefined})";
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn native_accessor_helper_checks_the_name_and_length_getter_form() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    // The `{ name, length }` form describes the accessor function itself, as
    // the corpus's own propertyHelper.js does; omitted fields default to the
    // built-in accessor conventions ("get "/"set " + key, length 0/1).
    for source in [
        "verifyPrimordialAccessorProperty(ArrayBuffer.prototype,'byteLength',{get:{name:'get byteLength',length:0},set:undefined});true",
        "verifyPrimordialAccessorProperty(ArrayBuffer.prototype,'byteLength',{get:{},set:undefined});true",
        "verifyAccessorProperty(ArrayBuffer,Symbol.species,{get:{},set:undefined});true",
        "let o={set x(v){}};verifyAccessorProperty(o,'x',{set:{},enumerable:true});verifyAccessorProperty(o,'x',{set:{name:'set x',length:1},enumerable:true});true",
        "let o={set x(v){}};assert.throws(Test262Error,()=>verifyAccessorProperty(o,'x',{set:{length:2},enumerable:true}));assert.throws(Test262Error,()=>verifyAccessorProperty(o,'x',{set:{name:'x'},enumerable:true}));true",
        "verifyAccessorProperty(ArrayBuffer.prototype,'byteLength',{get:{},set:undefined,configurable:true,enumerable:false});true",
        // A wrong name, a wrong length, a non-accessor and a missing
        // property are all reported as Test262Errors.
        "assert.throws(Test262Error,()=>verifyAccessorProperty(ArrayBuffer.prototype,'byteLength',{get:{name:'get length'}}));true",
        "assert.throws(Test262Error,()=>verifyAccessorProperty(ArrayBuffer.prototype,'byteLength',{get:{length:1}}));true",
        "assert.throws(Test262Error,()=>verifyAccessorProperty(ArrayBuffer.prototype,'byteLength',{get:{},set:{}}));true",
        "assert.throws(Test262Error,()=>verifyAccessorProperty(Math,'PI',{get:{}}));true",
        "assert.throws(Test262Error,()=>verifyAccessorProperty(Math,'missing',{get:{}}));true",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn harness_allocation_failures_leave_the_vm_usable() {
    use blueice_bluejs::{HeapConfig, HeapError, VmConfig};
    let alive = compile(&parse("1+1").unwrap()).unwrap();
    for ceiling in (64000..125000).step_by(251) {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: 1,
                major_threshold_bytes: 256,
                max_heap_bytes: ceiling,
            },
            ..Default::default()
        })
        .unwrap();
        match vm.install_test262_harness() {
            Ok(()) => {}
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {}
            error => panic!("{error:?}"),
        }
        assert_eq!(vm.execute(&alive).unwrap(), Value::Number(2.0));
    }
}

#[test]
fn classic_scripts_publish_var_function_and_lexical_bindings_in_one_realm() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let harness =
        compile(&parse("var offset=4; function addOffset(value){return value+offset}").unwrap())
            .unwrap();
    assert_eq!(vm.execute_script(&harness).unwrap(), Value::Undefined);
    let test =
        compile(&parse("addOffset(3) === 7 && typeof offset === 'number'").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&test).unwrap(), Value::Bool(true));
    let lexical = compile(&parse("let secret=1; const hidden=2").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&lexical).unwrap(), Value::Undefined);
    let lookup = compile(&parse("secret === 1 && hidden === 2").unwrap()).unwrap();
    assert_eq!(vm.execute(&lookup).unwrap(), Value::Bool(true));
}

#[test]
fn classic_scripts_keep_global_var_and_lexical_bindings_live_across_scripts() {
    let mut vm = Vm::default();
    let script = |source: &str| compile(&parse(source).unwrap()).unwrap();

    assert_eq!(
        vm.execute_script(&script(
            "var counter=1;function readCounter(){return counter}let lexical=4;const fixed=7;function readLexical(){return lexical+fixed}",
        ))
        .unwrap(),
        Value::Undefined
    );
    assert_eq!(
        vm.execute_script(&script("counter=2;globalThis.counter=3;counter"))
            .unwrap(),
        Value::Number(3.0)
    );
    assert_eq!(
        vm.execute_script(&script("lexical=5;lexical")).unwrap(),
        Value::Number(5.0)
    );
    assert_eq!(
        vm.execute_script(&script("readCounter()")),
        Ok(Value::Number(3.0))
    );
    assert_eq!(
        vm.execute_script(&script("readLexical()")),
        Ok(Value::Number(12.0))
    );
    assert_eq!(
        vm.execute_script(&script("globalThis.lexical")),
        Ok(Value::Undefined)
    );
    assert!(matches!(
        vm.execute_script(&script("let lexical=0")),
        Err(RuntimeError::SyntaxError(_))
    ));
    assert!(matches!(
        vm.execute_script(&script("fixed=0")),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(
        vm.execute_script(&script("readCounter()===3&&readLexical()===12"))
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test262_eval_script_enters_the_current_realm_without_discarding_the_caller() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let evaluate =
        |source: &str, vm: &mut Vm| vm.execute(&compile(&parse(source).unwrap()).unwrap());

    assert_eq!(
        evaluate(
            "$262.evalScript('var shared=1;function readShared(){return shared};let lexical=4;const fixed=7');shared=2;globalThis.shared=3;lexical=5;readShared()===3&&lexical+fixed===12&&globalThis.lexical===undefined",
            &mut vm,
        ),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        evaluate("$262.evalScript('6')", &mut vm),
        Ok(Value::Number(6.0))
    );
    assert_eq!(
        evaluate(
            "let caught=false;try{$262.evalScript('const malformed =')}catch(error){caught=error instanceof SyntaxError;}caught",
            &mut vm,
        ),
        Ok(Value::Bool(true))
    );
}

/// `import(spec, {with: attributesProxy})`'s attribute enumeration goes
/// through the same Proxy-observant `EnumerableOwnPropertyNames` path as
/// `Object.keys`/etc (round 1's `evaluate_import_call_arguments`), and that
/// enumerated `type` value must actually reach `ensure_json_module`'s
/// routing decision -- not just be validated and discarded.
#[test]
fn json_module_dynamic_import_reads_type_attribute_through_a_proxy() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_json_module_sources(HashMap::from([(
        "json-proxy-attrs/data.json".to_string(),
        "262".to_string(),
    )]));
    vm.set_module_loader_context("json-proxy-attrs/main.js", HashMap::new());
    let source = "var log = [];\nvar options = {\n  with: new Proxy({}, {\n    ownKeys: function() {\n      return [\"type\"];\n    },\n    get(_, name) {\n      log.push(name);\n      return \"json\";\n    },\n    getOwnPropertyDescriptor(target, name) {\n      return {configurable: true, enumerable: true, value: \"json\"};\n    },\n  })\n};\n\nimport('./data.json', options)\n  .then(function(module) {\n    assert.sameValue(module.default, 262);\n  })\n  .then($DONE, $DONE);\n\nassert.sameValue(log.length, 1);\nassert.sameValue(log[0], \"type\");\n";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}
