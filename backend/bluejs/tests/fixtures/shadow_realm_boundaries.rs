// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ShadowRealm resource, ancestor, and queued-job boundaries with live heaps.

use super::*;
use crate::{compile, compile_module, parse, parse_module};

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn shadow_wrapping_rejects_another_live_heaps_handle_and_root_refusals() {
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let mut source = Vm::default();
    let mut destination = Vm::default();
    assert_eq!(
        destination.shadow_wrap_into(&mut source, Value::Object(object)),
        Err(RuntimeError::Heap(HeapError::InvalidObject(object)))
    );
    owner.heap.unroot(root).unwrap();

    for exhaust_source in [false, true] {
        let mut source = Vm::default();
        let callable = execute(&mut source, "(function() {return 42;})")
            .object_id()
            .unwrap();
        let root = source.heap.root(callable).unwrap();
        let mut destination = Vm::default();
        let before = source.heap.stats().root_registrations;
        let expected = if exhaust_source {
            source.heap.allow_root_registrations(0);
            RuntimeError::Heap(HeapError::IdExhausted)
        } else {
            let limit = destination.heap.allow_only(0);
            RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })
        };
        assert_eq!(
            destination.shadow_wrapped_function_create(&mut source, callable),
            Err(expected)
        );
        assert!(destination.shadow_wrapped_functions.is_empty() && destination.stack.is_empty());
        assert!(source.heap.contains(callable));
        assert_eq!(
            source.heap.stats().root_registrations,
            before + u64::from(!exhaust_source)
        );
        destination.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(execute(&mut destination, "21 + 21"), Value::Number(42.0));
        source.heap.unroot(root).unwrap();
        assert_eq!(execute(&mut source, "21 + 21"), Value::Number(42.0));
        assert!(
            !source.heap.contains(callable),
            "a refused wrapper retained its source root"
        );
    }
}

#[cfg_attr(test, test)]
fn shadow_ancestors_and_wrapped_this_are_validated_before_reborrowing() {
    let mut source = Vm::default();
    let callable = execute(&mut source, "(function() {return 42;})")
        .object_id()
        .unwrap();
    let root = source.heap.root(callable).unwrap();
    let mut destination = Vm::default();
    let wrapper = destination
        .shadow_wrapped_function_create(&mut source, callable)
        .unwrap();
    let wrapper_root = destination.heap.root(wrapper.object_id().unwrap()).unwrap();
    assert!(
        matches!(destination.shadow_call_wrapped(wrapper.object_id().unwrap(), Value::Undefined, Vec::new(), false),
        Err(RuntimeError::TypeError(message)) if message == "the ShadowRealm this function belongs to is no longer reachable")
    );

    let object = destination.heap.alloc_object(None).unwrap();
    let object_root = destination.heap.root(object).unwrap();
    assert!(
        matches!(destination.shadow_call_across(&mut source, callable, Value::Object(object), Vec::new()),
        Err(RuntimeError::TypeError(message)) if message == "only primitive values and functions may cross a ShadowRealm boundary")
    );
    assert!(destination.stack.is_empty() && source.stack.is_empty());
    destination.heap.unroot(object_root).unwrap();
    destination.heap.unroot(wrapper_root).unwrap();
    source.heap.unroot(root).unwrap();

    let realm = execute(&mut destination, "new ShadowRealm()");
    let record = destination.shadow_realms[&realm.object_id().unwrap()].clone();
    let held = record.vm.borrow_mut();
    assert!(
        matches!(destination.shadow_realm_evaluate(realm, Value::String("42".into())),
        Err(RuntimeError::TypeError(message)) if message == "the ShadowRealm this function belongs to is no longer reachable")
    );
    drop(held);
    assert_eq!(execute(&mut destination, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn shadow_constructor_ingress_preserves_lazy_and_new_target_errors() {
    let mut vm = Vm::default();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.shadow_realm_constructor(true),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.shadow_realms.is_empty() && vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.shadow_realm_prototype().unwrap();
    let target = execute(&mut vm, "new Proxy(function(){}, {get(target, key) {if(key === 'prototype') throw 7; return Reflect.get(target, key);}})");
    vm.new_target = target;
    assert_eq!(
        vm.shadow_realm_constructor(true),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(vm.shadow_realms.is_empty());
    vm.new_target = Value::Undefined;
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn shadow_evaluation_and_import_keep_queued_cleanup_failures_opaque() {
    let mut reentrant = Vm::default();
    assert_eq!(
        execute(
            &mut reentrant,
            r#"
        var realm = new ShadowRealm();
        var outer = realm.evaluate('(function(callback) {return callback();})');
        var source = 'globalThis.registry = new FinalizationRegistry(() => {throw 7;}); registry.register({}, 42); 42';
        var rejected = false;
        try {outer(() => realm.evaluate(source));}
        catch(error) {rejected = error instanceof TypeError;}
        rejected && realm.evaluate('21 + 21') === 42
    "#
        ),
        Value::Bool(true)
    );
    let mut caller = Vm::default();
    let mut child = Vm::default();
    let code = compile(&parse("globalThis.registry = new FinalizationRegistry(() => {throw 7;}); registry.register({}, 42); 42").unwrap()).unwrap();
    assert_eq!(
        Vm::run_evaluate(&mut caller, &mut child, &code, false),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(caller.stack.is_empty() && child.stack.is_empty());
    assert_eq!(execute(&mut child, "21 + 21"), Value::Number(42.0));

    for allocation_refusal in [false, true] {
        let mut vm = Vm::default();
        vm.set_module_loader_context("shadow/main.mjs", HashMap::from([("shadow/cleanup.mjs".into(),
            compile_module(&parse_module("globalThis.registry = new FinalizationRegistry(() => {throw 7;}); registry.register({}, 42); export const value = 42;").unwrap()).unwrap())]));
        let realm = execute(&mut vm, "new ShadowRealm()");
        let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
        if allocation_refusal {
            record.vm.borrow_mut().heap.allow_only(0);
        }
        let promise = vm
            .shadow_realm_import_value(
                realm,
                Value::String("./cleanup.mjs".into()),
                Value::String("value".into()),
            )
            .unwrap();
        vm.run_promise_jobs().unwrap();
        let PromiseStatus::Rejected(error) = &vm.promises[&promise.object_id().unwrap()].status
        else {
            panic!("the child entry or queued cleanup must reject the import");
        };
        let error = error.clone();
        assert_eq!(
            vm.get_property(&error, &"name".into()).unwrap(),
            Value::String("TypeError".into())
        );
        record.vm.borrow_mut().heap.allow_only(16 * 1024 * 1024);
        assert!(vm.stack.is_empty() && record.vm.borrow().stack.is_empty());
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_shadow_boundary_contracts() {
        shadow_wrapping_rejects_another_live_heaps_handle_and_root_refusals();
        shadow_ancestors_and_wrapped_this_are_validated_before_reborrowing();
        shadow_constructor_ingress_preserves_lazy_and_new_target_errors();
        shadow_evaluation_and_import_keep_queued_cleanup_failures_opaque();
        importing_an_uninitialized_cyclic_export_rejects_without_exposing_the_child();
    }
}

#[cfg_attr(test, test)]
fn importing_an_uninitialized_cyclic_export_rejects_without_exposing_the_child() {
    let modules = HashMap::from([
        (
            "cycle/root.mjs".into(),
            compile_module(
                &parse_module("import './dep.mjs'; await hold.promise; export let answer;")
                    .unwrap(),
            )
            .unwrap(),
        ),
        (
            "cycle/dep.mjs".into(),
            compile_module(
                &parse_module("await Promise.resolve(); export {answer} from './root.mjs';")
                    .unwrap(),
            )
            .unwrap(),
        ),
    ]);
    let mut vm = Vm::default();
    vm.set_module_loader_context("cycle/root.mjs", modules.clone());
    let realm = execute(&mut vm, "new ShadowRealm()");
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    {
        let mut child = record.vm.borrow_mut();
        execute(&mut child, "globalThis.hold = Promise.withResolvers();");
        child.set_module_loader_context("cycle/root.mjs", modules.clone());
        child
            .execute_module_graph("cycle/root.mjs", &modules)
            .unwrap();
        child.run_promise_jobs().unwrap();
    }
    let promise = vm
        .shadow_realm_import_value(
            realm,
            Value::String("./dep.mjs".into()),
            Value::String("answer".into()),
        )
        .unwrap();
    vm.run_promise_jobs().unwrap();
    let PromiseStatus::Rejected(error) = &vm.promises[&promise.object_id().unwrap()].status else {
        panic!("a cyclic export in the TDZ must reject importValue");
    };
    let error = error.clone();
    assert_eq!(
        vm.get_property(&error, &"name".into()),
        Ok(Value::String("TypeError".into()))
    );
    assert!(vm.stack.is_empty() && record.vm.borrow().stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}
