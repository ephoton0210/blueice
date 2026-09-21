// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
fn object_prevent_extensions_exposes_the_heap_internal_method() {
    for source in [
        "(function(){let object={present:1};return Object.preventExtensions(object)===object&&!Object.isExtensible(object)&&object.present===1&&Object.preventExtensions(1)===1;})()",
        "(function(){'use strict';let object={};Object.preventExtensions(object);try{object.added=1;return false;}catch(error){return error instanceof TypeError;}})()",
        "({own:1}).hasOwnProperty('own')&&!({own:1}).hasOwnProperty('missing')",
    ] {
        assert_eq!(
            Vm::default()
                .execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn frozen_test262_global_keeps_error_constructors_available() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "Object.preventExtensions(globalThis);let caught=false;try{eval('var unavailable')}catch(error){caught=error instanceof TypeError;}caught";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_detached_eval_uses_an_isolated_global() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "var other=$262.createRealm().global;var otherEval=other.eval;otherEval('var x=23;');typeof x==='undefined'&&other.x===23";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_membrane_preserves_foreign_object_identity_and_internal_slots() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "var other=$262.createRealm().global;var value=other.eval('var state=0;({get value(){state++;return state}})');var regexp=other.eval('/a/g');value.value===1&&other.eval('state')===1&&RegExp(regexp)!==regexp&&RegExp(regexp).toString()==='/a/g'";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_shares_the_agent_global_symbol_registry() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "var other=$262.createRealm().global;var local=Symbol.for('blueice');var foreign=other.Symbol.for('blueice');local===foreign&&other.eval(\"Symbol.for('blueice')\")===local&&Symbol.keyFor(foreign)==='blueice'&&other.Symbol.keyFor(local)==='blueice'";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_forwards_own_property_keys_from_foreign_intrinsics() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        var other = $262.createRealm().global;
        var expected = Reflect.ownKeys(Math);
        var actual = Reflect.ownKeys(other.Math);
        var same = actual.length === expected.length;
        for (var index = 0; index < expected.length; index++)
            same = same && actual[index] === expected[index];
        same
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_forwards_set_with_the_explicit_receiver() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        var other = $262.createRealm().global;
        other.eval(`
            var hits = 0;
            var expected;
            var object = {
                set value(value) {
                    'use strict';
                    if (this !== expected || value !== 'blueice') throw 'wrong receiver';
                    hits++;
                }
            };
        `);
        var local = {};
        other.expected = local;
        var localResult = Reflect.set(other.object, 'value', 'blueice', local) && other.hits === 1;
        other.expected = other.object;
        var foreignResult = Reflect.set(other.object, 'value', 'blueice', other.object) && other.hits === 2;
        other.expected = undefined;
        var primitiveResult = Reflect.set(other.object, 'value', 'blueice', undefined) && other.hits === 3;
        localResult && foreignResult && primitiveResult
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn dynamic_async_constructor_observes_a_revoked_proxy_new_target() {
    let source = r#"
        var constructor = (async function() {}).constructor;
        var revocations = 0;
        var handle = Proxy.revocable(function() {}, {
            get(target, key) {
                if (key === 'prototype') {
                    revocations++;
                    handle.revoke();
                    return undefined;
                }
                return Reflect.get(target, key);
            }
        });
        var caught = false;
        try { Reflect.construct(constructor, [], handle.proxy); }
        catch (error) { caught = error instanceof TypeError; }
        caught && revocations === 1
    "#;
    assert_eq!(
        Vm::default()
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn promise_constructor_observes_a_revoked_proxy_new_target() {
    let source = r#"
        var revocations = 0;
        var handle = Proxy.revocable(function() {}, {
            get(target, key) {
                if (key === 'prototype') {
                    revocations++;
                    handle.revoke();
                    return undefined;
                }
                return Reflect.get(target, key);
            }
        });
        var caught = false;
        try { Reflect.construct(Promise, [function() {}], handle.proxy); }
        catch (error) { caught = error instanceof TypeError; }
        caught && revocations === 1
    "#;
    assert_eq!(
        Vm::default()
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_eval_exposes_a_callable_completion_without_foreign_heap_handles() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "var other=$262.createRealm().global;var fn=other.eval('(0, async function* () {})');var prototype=Object.getPrototypeOf(fn.prototype);fn.prototype=undefined;Object.getPrototypeOf(fn())===prototype";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn sloppy_global_eval_annex_b_function_does_not_block_a_later_lexical() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source =
        "eval('if(true){function test262Fn(){}}');$262.evalScript('let test262Fn=1');test262Fn===1";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn global_function_declarations_follow_existing_property_rules() {
    for source in [
        "Object.defineProperty(globalThis,'replaceable',{value:0,configurable:true});Object.preventExtensions(globalThis);$262.evalScript('function replaceable(){}');let descriptor=Object.getOwnPropertyDescriptor(globalThis,'replaceable');typeof replaceable==='function'&&descriptor.writable&&descriptor.enumerable&&!descriptor.configurable",
        "Object.defineProperty(globalThis,'incompatible',{value:0,writable:false,enumerable:true,configurable:false});(function(){try{$262.evalScript('var mustNotExist;function incompatible(){}');return false;}catch(error){return error instanceof TypeError&&typeof mustNotExist==='undefined';}})()",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn sloppy_block_functions_keep_their_lexical_binding_and_update_the_outer_var() {
    let mut vm = Vm::default();
    let source = "var initial,current;{function f(){initial=f;f=123;current=f;return 'block';}}f();initial()==='block'&&current===123&&f()==='block'";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("typeof f").unwrap()).unwrap())
            .unwrap(),
        Value::String("function".into())
    );

    let mut strict_vm = Vm::default();
    assert_eq!(
        strict_vm
            .execute_script(
                &compile(&parse("'use strict';{function hidden(){}}typeof hidden").unwrap())
                    .unwrap()
            )
            .unwrap(),
        Value::String("undefined".into())
    );
}

#[test]
fn sloppy_if_clause_functions_use_the_annex_b_synthetic_block() {
    let source = "var initial,current;if(true)function f(){initial=f;f=123;current=f;return 'if';}f();initial()==='if'&&current===123&&f()==='if'";
    assert_eq!(
        Vm::default()
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn sloppy_block_functions_cross_a_simple_catch_parameter() {
    let source =
        "try{throw null;}catch(f){{function f(){return 1;}}}typeof f==='function'&&f()===1";
    assert_eq!(
        Vm::default()
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn class_declaration_bindings_are_mutable_lexicals() {
    assert_eq!(
        Vm::default()
            .execute_script(
                &compile(&parse("class Declaration{};Declaration=1;Declaration===1").unwrap())
                    .unwrap(),
            )
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn global_lexicals_shadow_configurable_intrinsic_properties() {
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script(
            &compile(
                &parse("let Array;let descriptor=Object.getOwnPropertyDescriptor(globalThis,'Array');Array===undefined&&typeof globalThis.Array==='function'&&descriptor.configurable&&!descriptor.enumerable&&descriptor.writable")
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
        Value::Bool(true)
    );
    vm.install_test262_harness().unwrap();
    assert!(matches!(
        vm.execute(&compile(&parse("$262.evalScript('let undefined')").unwrap()).unwrap()),
        Err(RuntimeError::SyntaxError(_))
    ));
}

#[test]
fn function_constructor_compiles_global_source_without_capturing_caller_bindings() {
    for source in [
        "let hidden=1;let fn=Function('return this;');fn()===globalThis&&Function('a','b','return a+b;')(2,3)===5&&Function('return typeof hidden;')()==='undefined'",
        "Function('<!--','')()===undefined&&Function('\\n-->','')()===undefined&&Function('a','//','')()===undefined&&(function(){try{Function('-->','');return false}catch(error){return error instanceof SyntaxError}})()",
        "(function(){try{Function('return )');return false;}catch(error){return error instanceof SyntaxError;}})()",
    ] {
        assert_eq!(
            Vm::default()
                .execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn function_intrinsic_graph_and_restricted_properties_follow_the_realm_contract() {
    let source = r#"
        let caller = Object.getOwnPropertyDescriptor(Function.prototype, 'caller');
        let arguments = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments');
        let callerThrows = false;
        let argumentsThrows = false;
        try { Function.prototype.caller; } catch (error) { callerThrows = error instanceof TypeError; }
        try { Function.prototype.arguments = 1; } catch (error) { argumentsThrows = error instanceof TypeError; }
        let constructed = new Function('return 7;');
        let strict = Function('"use strict"; return 1;');
        let AsyncFunction = Object.getPrototypeOf(async function() {}).constructor;
        let dynamicAsync = AsyncFunction('return 1;');
        let object = { method() {} };
        let names = Object.getOwnPropertyNames(Function);
        let lengthIndex = names.indexOf('length');
        let nameIndex = names.indexOf('name');
        Object.getPrototypeOf(Function) === Function.prototype &&
          Function.prototype.constructor === Function &&
          Function.prototype.isPrototypeOf(Function) &&
          constructed.constructor === Function && constructed() === 7 &&
          constructed.caller === undefined && constructed.arguments === null &&
          !strict.hasOwnProperty('caller') && !strict.hasOwnProperty('arguments') &&
          !dynamicAsync.hasOwnProperty('caller') && !dynamicAsync.hasOwnProperty('arguments') &&
          !object.method.hasOwnProperty('caller') && !object.method.hasOwnProperty('arguments') &&
          caller.get === caller.set && caller.enumerable === false && caller.configurable === true &&
          arguments.get === caller.get && arguments.set === caller.set &&
          callerThrows && argumentsThrows &&
          lengthIndex >= 0 && nameIndex === lengthIndex + 1 &&
          Object.prototype.isPrototypeOf.call(null, 0) === false
    "#;
    assert_eq!(
        Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn construct_enforces_derived_return_and_revoked_proxy_realm_contracts() {
    let source = r#"
        let derivedReturnThrows = false;
        let derivedThisThrows = false;
        let revokedProxyThrows = false;
        try { new (class extends Object { constructor() { return null; } })(); }
        catch (error) { derivedReturnThrows = error instanceof TypeError; }
        try { new (class extends Object { constructor() {} })(); }
        catch (error) { derivedThisThrows = error instanceof ReferenceError; }
        let handle;
        handle = Proxy.revocable(function() {}, { get() { handle.revoke(); } });
        try { new handle.proxy(); }
        catch (error) { revokedProxyThrows = error instanceof TypeError; }
        derivedReturnThrows && derivedThisThrows && revokedProxyThrows
    "#;
    assert_eq!(
        Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn class_call_error_uses_the_class_realm() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm().global;
        let C = other.eval('(class {})');
        let foreignTypeError = other.TypeError;
        let caught;
        try { C(); } catch (error) { caught = error; }
        caught.constructor === foreignTypeError && caught.constructor !== TypeError
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn foreign_function_calls_keep_error_and_constructor_prototype_realms() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm().global;
        let foreignFunction = new other.Function();
        let foreignError = false;
        try { foreignFunction.apply(null, false); }
        catch (error) { foreignError = error.constructor === other.TypeError; }

        let C = new other.Function();
        C.prototype = null;
        let localFunction = Reflect.construct(Function, [], C);
        let localConstructor = Reflect.construct(function() {}.bind(), [], C);
        let localObject = Reflect.construct(Object, [], C);
        let localIdentity = {};
        let echo = new other.Function('return this;');
        let transportedIdentity = echo.call(localIdentity) === localIdentity;
        let compareTransported = new other.Function(
          'return arguments[0] === arguments[1];'
        );
        let transportedAlias = compareTransported(localIdentity, localIdentity);

        let realmA = $262.createRealm().global;
        let realmB = $262.createRealm().global;
        let newTarget = new realmB.Function();
        newTarget.prototype = null;
        let foreignFunctionWithForeignTarget = Reflect.construct(
          realmA.Function, [''], newTarget
        );
        let boundForeignTarget = realmA.Function.prototype.bind.call(newTarget);
        let boundForeignObject = Reflect.construct(realmA.Object, [], boundForeignTarget);
        let realmAObject = new realmA.Function('return {};')();
        let realmBIdentity = new realmB.Function('return this;');
        let crossRealmTransportedIdentity =
          realmBIdentity.call(realmAObject) === realmAObject;

        foreignError &&
          Object.getPrototypeOf(foreignFunction) === other.Function.prototype &&
          Object.getPrototypeOf(localFunction) === other.Function.prototype &&
          Object.getPrototypeOf(localConstructor) === other.Object.prototype &&
          Object.getPrototypeOf(localObject) === other.Object.prototype &&
          transportedIdentity &&
          transportedAlias &&
          Object.getPrototypeOf(foreignFunctionWithForeignTarget) === realmB.Function.prototype &&
          Object.getPrototypeOf(foreignFunctionWithForeignTarget.prototype) === realmA.Object.prototype &&
          Object.getPrototypeOf(boundForeignObject) === realmB.Object.prototype &&
          crossRealmTransportedIdentity
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn intl_constructors_select_foreign_new_target_intrinsics() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm().global;
        let newTarget = new other.Function();
        newTarget.prototype = undefined;
        let collator = Reflect.construct(Intl.Collator, [], newTarget);
        let dateTime = Reflect.construct(Intl.DateTimeFormat, [], newTarget);
        let numberFormat = Reflect.construct(Intl.NumberFormat, [], newTarget);
        let displayNames = Reflect.construct(Intl.DisplayNames, ['en', {type: 'language'}], newTarget);
        let duration = Reflect.construct(Intl.DurationFormat, ['en'], newTarget);
        let list = Reflect.construct(Intl.ListFormat, [], newTarget);
        let plural = Reflect.construct(Intl.PluralRules, [], newTarget);
        let relativeTime = Reflect.construct(Intl.RelativeTimeFormat, [], newTarget);
        let segmenter = Reflect.construct(Intl.Segmenter, [], newTarget);
        let locale = Reflect.construct(Intl.Locale, ['de'], newTarget);
          Object.getPrototypeOf(collator) === other.Intl.Collator.prototype &&
          Object.getPrototypeOf(dateTime) === other.Intl.DateTimeFormat.prototype &&
          Object.getPrototypeOf(numberFormat) === other.Intl.NumberFormat.prototype &&
          Object.getPrototypeOf(displayNames) === other.Intl.DisplayNames.prototype &&
          Object.getPrototypeOf(duration) === other.Intl.DurationFormat.prototype &&
          Object.getPrototypeOf(list) === other.Intl.ListFormat.prototype &&
          Object.getPrototypeOf(plural) === other.Intl.PluralRules.prototype &&
          Object.getPrototypeOf(relativeTime) === other.Intl.RelativeTimeFormat.prototype &&
          Object.getPrototypeOf(segmenter) === other.Intl.Segmenter.prototype &&
          Object.getPrototypeOf(locale) === other.Intl.Locale.prototype
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn foreign_proxy_revocable_retains_caller_realm_target_and_handler() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm().global;
        let handle;
        handle = other.Proxy.revocable(function() {}, {
            get() { handle.revoke(); }
        });
        let revoked = false;
        try { new handle.proxy(); }
        catch (error) { revoked = error instanceof TypeError; }
        revoked
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn foreign_proxy_and_constructor_membranes_preserve_realms() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm();
        let proxy = other.global.eval(
          'new Proxy(function() {}, { apply(_, __, args) { return args; } })'
        );
        let arguments = proxy();

        let realm1 = $262.createRealm().global;
        let realm2 = $262.createRealm().global;
        let realm3 = $262.createRealm().global;
        let newTarget = new realm1.Function();
        newTarget.prototype = false;
        let newTargetProxy = new realm2.Proxy(newTarget, {});
        let array = Reflect.construct(realm3.Array, [], newTargetProxy);

        let foreignThrow = other.evalScript(`
          (function() {
            let handle = Proxy.revocable(function() {}, {});
            handle.revoke();
            return handle.proxy();
          })
        `);
        let foreignError = false;
        try { foreignThrow(); }
        catch (error) { foreignError = error.constructor === other.global.TypeError; }

        arguments.constructor === Array &&
          Object.getPrototypeOf(arguments) === Array.prototype &&
          array instanceof realm1.Array &&
          Object.getPrototypeOf(array) === realm1.Array.prototype &&
          foreignError
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn reflect_construct_rejects_date_now_as_a_non_constructor() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        typeof Date === 'function' &&
          typeof Date.now === 'function' &&
          (function() {
            try { Reflect.construct(Date.now, []); }
            catch (error) { return error instanceof TypeError; }
            return false;
          })()
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn is_html_dda_host_object_keeps_strict_equality_ordinary() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_is_html_dda().unwrap();
    for source in [
        "(function(){let value=$262.IsHTMLDDA;return !value;})()",
        "(function(){let value=$262.IsHTMLDDA;return value==null&&value==undefined;})()",
        "(function(){let value=$262.IsHTMLDDA;return typeof value==='undefined';})()",
        "(function(){let value=$262.IsHTMLDDA;switch(value){case undefined:return 1;case null:return 2;case value:return 3;}})()===3",
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
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Undefined,
            "{source}"
        );
    }
    for source in [
        "assert.throws(TypeError,1)",
        "assert.compareArray([1],[1,2])",
        "assert.compareArray([0],[-0])",
    ] {
        assert!(
            matches!(
                vm.execute(&compile(&parse(source).unwrap()).unwrap()),
                Err(RuntimeError::Test262(_))
            ),
            "{source}"
        );
    }
    assert!(matches!(
        vm.execute(
            &compile(&parse("assert.throws(RangeError,()=>''.padStart(10000000))").unwrap())
                .unwrap()
        ),
        Err(RuntimeError::StringLimit { .. })
    ));
    assert_eq!(
        vm.execute(&compile(&parse("typeof Test262Error").unwrap()).unwrap())
            .unwrap(),
        Value::String("function".into())
    );
    for source in [
        "Error.prototype.toString.call({name:'',message:'x'}) === 'x'",
        "new Error('x',{}).message === 'x'",
        "Error.prototype.toString.call({}) === 'Error'",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
    assert!(matches!(
        vm.execute(&compile(&parse("Error.prototype.toString.call(1)").unwrap()).unwrap()),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn immutable_array_buffers_stay_immutable_across_test262_realms() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    // A view built in one realm over the other realm's immutable buffer keeps
    // the buffer immutable: writes are refused and host detachment throws.
    let source = "let other=$262.createRealm().global;let parentBuffer=new ArrayBuffer(4).transferToImmutable();let childView=new other.Uint8Array(parentBuffer);let childBuffer=new other.ArrayBuffer(4).transferToImmutable();let parentView=new Uint8Array(childBuffer);let fills=0;for(const view of [childView,parentView]){try{view.fill(1)}catch(error){if(error.name===\"TypeError\")fills++}}childView[0]=9;parentView[0]=9;let detaches=0;for(const buffer of [parentBuffer,childBuffer]){try{$262.detachArrayBuffer(buffer)}catch(error){if(error.name===\"TypeError\")detaches++}}fills===2&&detaches===2&&childView.buffer.immutable&&parentView.buffer.immutable&&childView[0]===0&&parentView[0]===0&&Reflect.set(parentView,0,5)===false&&parentBuffer.byteLength===4&&childBuffer.byteLength===4";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}
