// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{RuntimeError, Value, Vm, compile, parse};

#[test]
fn harness_assertions_fail_closed() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "assert(true)",
        "assert.sameValue(NaN,NaN)",
        "assert.notSameValue(0,-0)",
        "assert.throws(TypeError,()=>''.repeat.call(null))",
        "assert.throws(RangeError,()=>''.repeat(-1))",
        "assert.throws(Error,()=>{throw new Error('x');})",
    ] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Undefined, "{source}");
    }
    for source in [
        "assert(false)",
        "assert(1)",
        "assert.sameValue(0,-0)",
        "assert.notSameValue(NaN,NaN)",
        "assert.throws(TypeError,()=>1)",
        "assert.throws(TypeError,()=>{throw 1;})",
        "assert.throws(TypeError,()=>{throw new Error();})",
        "$DONOTEVALUATE()",
    ] {
        assert!(matches!(vm.execute(&compile(&parse(source).unwrap()).unwrap()), Err(RuntimeError::Test262(_))), "{source}");
    }
    assert!(matches!(Vm::default().execute(&compile(&parse("assert(true)").unwrap()).unwrap()), Err(RuntimeError::ReferenceError(_))));
}

#[test]
fn error_constructors_and_host_globals() {
    for source in [
        "new TypeError('x').toString() === 'TypeError: x'",
        "Error().toString() === 'Error' && new Error('').message === ''",
        "new RangeError().name === 'RangeError' && new TypeError() instanceof Error",
        "let e=new Error('a',{cause:42}); e.cause === 42 && !Object.getOwnPropertyDescriptor(e,'cause').enumerable",
        "globalThis.hostValue=42; hostValue === 42 && typeof hostValue === 'number' && typeof absent === 'undefined'",
    ] {
        assert_eq!(Vm::default().execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn harness_compares_arrays_and_propagates_resource_errors() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "assert.compareArray([NaN,-0],[NaN,-0])",
        "assert.sameValue(assert._isSameValue(0,-0),false)",
        "assert.throws(ReferenceError,()=>missing)",
        "assert.throws(SyntaxError,()=>new RegExp('['))",
        "assert.throws(Test262Error,()=>assert(false))",
    ] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Undefined, "{source}");
    }
    for source in ["assert.throws(TypeError,1)", "assert.compareArray([1],[1,2])", "assert.compareArray([0],[-0])"] {
        assert!(matches!(vm.execute(&compile(&parse(source).unwrap()).unwrap()), Err(RuntimeError::Test262(_))), "{source}");
    }
    assert!(matches!(vm.execute(&compile(&parse("assert.throws(RangeError,()=>''.padStart(10000000))").unwrap()).unwrap()), Err(RuntimeError::StringLimit { .. })));
    assert_eq!(vm.execute(&compile(&parse("typeof Test262Error").unwrap()).unwrap()).unwrap(), Value::String("function".into()));
    for source in ["Error.prototype.toString.call({name:'',message:'x'}) === 'x'", "new Error('x',{}).message === 'x'", "Error.prototype.toString.call({}) === 'Error'"] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true), "{source}");
    }
    assert!(matches!(vm.execute(&compile(&parse("Error.prototype.toString.call(1)").unwrap()).unwrap()), Err(RuntimeError::TypeError(_))));
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
        "assert.throws(TypeError,()=>formatSimpleValue({toString(){throw new TypeError();}}))",
    ] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Undefined, "{source}");
    }
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
    assert!(matches!(vm.execute(&failure), Err(RuntimeError::Test262(_))));
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
    assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true));
}

#[test]
fn harness_allocation_failures_leave_the_vm_usable() {
    use blueice_bluejs::{HeapConfig, HeapError, VmConfig};
    let alive = compile(&parse("1+1").unwrap()).unwrap();
    for ceiling in (64000..125000).step_by(251) {
        let mut vm = Vm::new(VmConfig { heap: HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: ceiling }, ..Default::default() }).unwrap();
        match vm.install_test262_harness() {
            Ok(()) => {}
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {}
            error => panic!("{error:?}"),
        }
        assert_eq!(vm.execute(&alive).unwrap(), Value::Number(2.0));
    }
}

#[test]
fn classic_scripts_publish_var_and_function_bindings_without_leaking_lexicals() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let harness = compile(&parse("var offset=4; function addOffset(value){return value+offset}").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&harness).unwrap(), Value::Undefined);
    let test = compile(&parse("addOffset(3) === 7 && typeof offset === 'number'").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&test).unwrap(), Value::Bool(true));
    let lexical = compile(&parse("let secret=1; const hidden=2").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&lexical).unwrap(), Value::Undefined);
    let lookup = compile(&parse("typeof secret === 'undefined' && typeof hidden === 'undefined'").unwrap()).unwrap();
    assert_eq!(vm.execute(&lookup).unwrap(), Value::Bool(true));
}
