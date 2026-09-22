// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
        "assert.throws(TypeError,()=>formatSimpleValue({toString(){throw new TypeError();}}))",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Undefined,
            "{source}"
        );
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
