// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeError, BlueJsProgramV1,
    BlueJsSourceIdentity, Value, Vm,
};

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.install_window_aliases().unwrap();
    vm
}

#[test]
fn aliases_are_owner_selected_and_share_global_bindings() {
    let mut vm = Vm::default();
    assert_eq!(run(&mut vm, "typeof window === 'undefined' && typeof self === 'undefined' && typeof top === 'undefined' && typeof parent === 'undefined'"), Value::Bool(true));
    vm.install_window_aliases().unwrap();
    assert_eq!(run(&mut vm, "window === globalThis && self === window && top === window && parent === window && this === window"), Value::Bool(true));
    assert_eq!(run(&mut vm, "var fromScript = 41; window.fromWindow = fromScript + 1; self.fromWindow === 42 && parent.fromScript === 41 && top.fromWindow === fromWindow"), Value::Bool(true));
    assert_eq!(
        run(
            &mut vm,
            "let lexical = 7; window.lexical === undefined && lexical === 7"
        ),
        Value::Bool(true)
    );
}

#[test]
fn alias_descriptors_follow_unforgeable_and_replaceable_attributes() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "['window', 'self', 'top', 'parent'].every(function (name) {
        var descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
        var replaceable = name === 'self' || name === 'parent';
        return descriptor.enumerable && descriptor.configurable === replaceable
            && descriptor.get.name === 'get ' + name && descriptor.get.length === 0
            && !Object.hasOwn(descriptor.get, 'prototype')
            && (replaceable ? descriptor.set.name === 'set ' + name && descriptor.set.length === 1 : descriptor.set === undefined);
    })"), Value::Bool(true));
    assert_eq!(run(&mut vm, "window = 1; top = 2; window === globalThis && top === globalThis && !Reflect.deleteProperty(globalThis, 'window') && !Reflect.deleteProperty(globalThis, 'top') && !Reflect.defineProperty(globalThis, 'window', {value: 0})"), Value::Bool(true));
    assert_eq!(run(&mut vm, "['window', 'top'].every(function (name) { try { (function () { 'use strict'; globalThis[name] = 0; })(); } catch (error) { return error instanceof TypeError; } return false; })"), Value::Bool(true));
}

#[test]
fn self_and_parent_can_be_replaced_deleted_and_redefined() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "self = 1; parent = {sentinel: 2}; var descriptor = Object.getOwnPropertyDescriptor(window, 'self'); self === 1 && window.self === 1 && parent.sentinel === 2 && descriptor.value === 1 && descriptor.writable && descriptor.enumerable && descriptor.configurable && descriptor.get === undefined"), Value::Bool(true));
    assert_eq!(run(&mut vm, "delete window.self; delete window.parent; typeof self === 'undefined' && typeof parent === 'undefined' && window === top"), Value::Bool(true));
    assert_eq!(
        run(
            &mut vm,
            "Object.defineProperty(window, 'self', {value: 9}); window.self === 9"
        ),
        Value::Bool(true)
    );
}

#[test]
fn accessors_validate_receivers_and_are_not_constructors() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var getter = Object.getOwnPropertyDescriptor(window, 'window').get;
        getter() === window && getter.call(null) === window && getter.call(undefined) === window && getter.call(window) === window"), Value::Bool(true));
    assert_eq!(run(&mut vm, "var setter = Object.getOwnPropertyDescriptor(window, 'self').set;
        [function () { getter.call({}); }, function () { getter.call(1); }, function () { getter.call(Object.create(window)); }, function () { setter.call({}, 1); }, function () { new getter(); }, function () { new setter(); }].every(function (attempt) { try { attempt(); } catch (error) { return error instanceof TypeError; } return false; })"), Value::Bool(true));
    assert_eq!(
        run(
            &mut vm,
            "setter.call(null, 8); self === 8 && window === top"
        ),
        Value::Bool(true)
    );
}

#[test]
fn alias_getters_keep_the_intrinsic_global_after_globalthis_is_reassigned() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var original = window; globalThis = {fake: true}; window === original && top === original && parent === original && self === original && globalThis !== original"), Value::Bool(true));
    assert_eq!(run(&mut vm, "delete window.globalThis; !Object.hasOwn(window, 'globalThis') && window === original && self === original"), Value::Bool(true));
}

#[test]
fn a_non_extensible_global_refuses_installation_without_publishing_aliases() {
    let mut vm = Vm::default();
    run(&mut vm, "Object.preventExtensions(globalThis);");
    assert!(vm.install_window_aliases().is_err());
    assert_eq!(run(&mut vm, "['window', 'self', 'top', 'parent'].every(function (name) { return !Object.hasOwn(globalThis, name); })"), Value::Bool(true));
}

#[test]
fn alias_installation_preflights_all_names_without_invoking_accessors() {
    for collision in ["window", "self", "top", "parent"] {
        let mut vm = Vm::default();
        run(&mut vm, &format!("var calls = 0; Object.defineProperty(globalThis, '{collision}', {{get: function () {{ calls++; throw 'getter invoked'; }}, configurable: true}});"));
        assert!(vm.install_window_aliases().is_err());
        assert_eq!(run(&mut vm, "calls"), Value::Number(0.0));
        for untouched in ["window", "self", "top", "parent"] {
            if untouched != collision {
                assert_eq!(
                    run(
                        &mut vm,
                        &format!("Object.hasOwn(globalThis, '{untouched}')")
                    ),
                    Value::Bool(false)
                );
            }
        }
    }
    assert!(page().install_window_aliases().is_err());
}

#[test]
fn page_only_aliases_do_not_appear_in_shadow_realms() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "new ShadowRealm().evaluate(\"typeof window + ',' + typeof self + ',' + typeof top + ',' + typeof parent\")"), Value::String("undefined,undefined,undefined,undefined".into()));
}

fn evaluate(runtime: &mut BlueJsPageRuntime, tab: u64, source: &str) -> Value {
    let origin = BlueJsPageOrigin::new("https://aliases.test").unwrap();
    let handle = runtime
        .install_program(
            tab,
            &origin,
            BlueJsSourceIdentity::new("page:///aliases.js", format!("sha256:{source}")).unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(tab, handle).unwrap()
}

#[test]
fn navigation_and_tabs_keep_aliases_bound_to_their_own_document() {
    let origin = BlueJsPageOrigin::new("https://aliases.test").unwrap();
    let mut runtime = BlueJsPageRuntime::default();
    for tab in [1, 2] {
        runtime.open_realm(tab, origin.clone()).unwrap();
        runtime
            .configure_realm_bindings(tab, |bindings| bindings.install_window_aliases())
            .unwrap();
    }
    evaluate(&mut runtime, 1, "window.marker = 7; self = 8;");
    assert_eq!(
        evaluate(
            &mut runtime,
            2,
            "window.marker === undefined && self === window && top === parent"
        ),
        Value::Bool(true)
    );
    runtime.navigate(1, origin).unwrap();
    assert_eq!(
        evaluate(&mut runtime, 1, "typeof window"),
        Value::String("undefined".into())
    );
    runtime
        .configure_realm_bindings(1, |bindings| bindings.install_window_aliases())
        .unwrap();
    assert_eq!(
        evaluate(
            &mut runtime,
            1,
            "window.marker === undefined && self === window && top === parent"
        ),
        Value::Bool(true)
    );
    assert!(matches!(
        runtime.configure_realm_bindings(99, |bindings| bindings.install_window_aliases()),
        Err(BlueJsPageRuntimeError::UnknownRealm(99))
    ));
}
