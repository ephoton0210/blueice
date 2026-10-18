// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny descriptor inputs, Proxy GC, and ordinary definition controls.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn buffered_string() -> JsString {
    JsString::from_code_units(vec![0x61; 32])
}

fn input(vm: &Vm) -> PropertyDefinitionInput {
    PropertyDefinitionInput::new(
        vm,
        buffered_string().into(),
        PropertyDescriptor::data(Value::String(buffered_string()), true, true, true),
    )
    .unwrap()
}

#[cfg_attr(test, test)]
fn property_definition_storage_input_outlives_the_vm() {
    let vm = page();
    let view = observer(&vm);
    let input = input(&vm);
    let bytes = definition_bytes(&input.key, &input.descriptor);
    assert_eq!(bytes, 128);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn property_definition_storage_neutral_consumer_checks_original_owner() {
    let source = page();
    let input = input(&source);
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Bool(true))
        })
        .unwrap();
    let object = run(
        &mut neutral,
        "new Proxy({},{defineProperty(){return markTerminal();}});",
    )
    .object_id()
    .unwrap();
    assert_eq!(
        input.define(&mut neutral, object),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_proxy_revocation_keeps_captured_target_through_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var target={},handler={get defineProperty(){rev.revoke();$262.gc();return function(){return true;};}};
        var rev=Proxy.revocable(target,handler);target=null;handler=null;
        Object.defineProperty(rev.proxy,'x',{value:7,writable:true,enumerable:true,configurable:true})===rev.proxy;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn property_definition_storage_proxy_descriptor_fields_and_false_result_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var seen=false,proxy=new Proxy({},{defineProperty(target,key,descriptor){seen=key==='x'&&descriptor.value===7&&descriptor.writable&&descriptor.enumerable&&descriptor.configurable;return true;}});
        var defined=Object.defineProperty(proxy,'x',{value:7,writable:true,enumerable:true,configurable:true})===proxy;
        var refused=Reflect.defineProperty(new Proxy({},{defineProperty(){return false;}}),'x',{value:1});
        defined&&seen&&!refused;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn property_definition_storage_array_species_definitions_preserve_map_and_filter() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[1,,3],log='';source.constructor={[Symbol.species]:function(length){
            return new Proxy({length},{defineProperty(target,key,descriptor){log+=key;return Reflect.defineProperty(target,key,descriptor);}});
        }};
        var mapped=source.map(value=>value+1),filtered=source.filter(()=>true);
        mapped[0]===2&&mapped[2]===4&&filtered[0]===1&&filtered[1]===3&&log==='0201';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn property_definition_storage_all_field_payloads_keep_independent_credit() {
    let vm = page();
    let view = observer(&vm);
    let input = PropertyDefinitionInput::new(
        &vm,
        buffered_string().into(),
        PropertyDescriptor {
            value: Some(Value::String(buffered_string())),
            get: Some(Value::String(buffered_string())),
            set: Some(Value::String(buffered_string())),
            ..PropertyDescriptor::default()
        },
    )
    .unwrap();
    let bytes = definition_bytes(&input.key, &input.descriptor);
    assert_eq!(bytes, 256);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn native_bytes_at_delegated_trap(large: bool) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let seen = Rc::new(std::cell::Cell::new(None));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let object = run(
        &mut vm,
        "new Proxy(new Proxy({},{defineProperty(){observe();return true;}}),{});",
    )
    .object_id()
    .unwrap();
    let value = if large {
        Value::String(buffered_string())
    } else {
        Value::Number(1.0)
    };
    PropertyDefinitionInput::new(
        &vm,
        "x".into(),
        PropertyDescriptor::data(value, true, true, true),
    )
    .unwrap()
    .define(&mut vm, object)
    .unwrap();
    seen.get().unwrap()
}

#[cfg_attr(test, test)]
fn property_definition_storage_delegation_does_not_charge_one_payload_twice() {
    let small = native_bytes_at_delegated_trap(false);
    let large = native_bytes_at_delegated_trap(true);
    assert_eq!(large - small, 64, "large={large}, small={small}");
}

#[cfg_attr(test, test)]
fn property_definition_storage_ordinary_heap_handoff_releases_native_credit() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "({});").object_id().unwrap();
    assert!(input(&vm).define(&mut vm, object).unwrap());
    assert_eq!(
        vm.heap
            .get(object, PropertyName::from(buffered_string()))
            .unwrap(),
        Value::String(buffered_string())
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn property_definition_storage_key_copy_keeps_its_own_payload() {
    let vm = page();
    let view = observer(&vm);
    let input = input(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let copied = crate::vm::native_value_storage::NativeValue::copied_property_key(
        &input.key,
        owner.as_ref(),
    )
    .unwrap();
    drop(owner);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 192);
    drop(copied);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 128);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn property_definition_storage_symbol_keys_keep_identity_and_descriptor_flags() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var key=Symbol('descriptor-key'),seen=false,target={},proxy=new Proxy(target,{defineProperty(object,name,descriptor){seen=name===key&&descriptor.value===7&&descriptor.configurable;return Reflect.defineProperty(object,name,descriptor);}});
        Reflect.defineProperty(proxy,key,{value:7,configurable:true})&&seen&&target[key]===7;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn property_definition_storage_missing_descriptor_fields_stay_absent() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var seen=false,proxy=new Proxy({},{defineProperty(target,key,descriptor){seen=Object.keys(descriptor).length===0;return true;}});
        Reflect.defineProperty(proxy,'x',{})&&seen;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn property_definition_storage_accessor_functions_survive_descriptor_mutation_and_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var seen=0,getter=()=>7,setter=value=>{seen=value;},descriptor={get:getter,set:setter,configurable:true,enumerable:true};
        var proxy=new Proxy({},{get defineProperty(){descriptor.get=undefined;descriptor.set=undefined;getter=null;setter=null;$262.gc();return function(target,key,input){$262.gc();return Reflect.defineProperty(target,key,input);};}});
        var result=Reflect.defineProperty(proxy,'x',descriptor);proxy.x=9;result&&proxy.x===7&&seen===9;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_fresh_trap_survives_one_fixed_gc_schedule() {
    let mut config = crate::VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var target={},proxy=new Proxy(target,new Proxy({},{get(object,trap){if(trap==='defineProperty')return function(target,key,descriptor){return Reflect.defineProperty(target,key,descriptor);};}}));
        Object.defineProperty(proxy,'x',{value:7,configurable:true})===proxy&&target.x===7;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn property_definition_storage_revoked_proxy_still_rejects_definition() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"var rev=Proxy.revocable({},{}),caught=false;rev.revoke();try{Reflect.defineProperty(rev.proxy,'x',{value:1});}catch(error){caught=error instanceof TypeError;}caught;"),Value::Bool(true));
}

#[cfg_attr(test, test)]
fn property_definition_storage_document_adoption_keeps_destination_policy() {
    let source = page();
    let source_view = observer(&source);
    let before = source_view.host_scheduling_snapshot().payload_bytes;
    let input = input(&source);
    let mut destination = page();
    let destination_view = observer(&destination);
    let object = run(&mut destination, "({});").object_id().unwrap();
    assert!(input.define(&mut destination, object).unwrap());
    assert_eq!(source_view.host_scheduling_snapshot().payload_bytes, before);
    assert!(destination.host_scheduling.is_some());
    drop(source);
    drop(destination);
    assert_eq!(source_view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(destination_view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn property_definition_storage_source_failure_does_not_poison_the_destination() {
    let source = page();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let input = input(&source);
    let mut destination = page();
    destination
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Bool(true))
        })
        .unwrap();
    let object = run(
        &mut destination,
        "new Proxy({},{defineProperty(){return markTerminal();}});",
    )
    .object_id()
    .unwrap();
    assert_eq!(
        input.define(&mut destination, object),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(destination
        .argument_list_owner()
        .unwrap()
        .as_ref()
        .unwrap()
        .reserve_peer(0)
        .is_ok());
    assert_eq!(run(&mut destination, "21+21;"), Value::Number(42.0));
    assert!(destination.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_failed_destination_does_not_enter_the_trap() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let input = input(&source);
    let mut destination = page();
    let called = Rc::new(std::cell::Cell::new(false));
    let output = called.clone();
    destination
        .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
            output.set(true);
            Ok(crate::HostValue::Bool(true))
        })
        .unwrap();
    let object = run(
        &mut destination,
        "new Proxy({},{defineProperty(){return observe();}});",
    )
    .object_id()
    .unwrap();
    let _ = destination.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        input.define(&mut destination, object),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!called.get());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert!(source
        .argument_list_owner()
        .unwrap()
        .as_ref()
        .unwrap()
        .reserve_peer(0)
        .is_ok());
    drop(destination);
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn property_definition_storage_zero_payload_keeps_terminal_identity() {
    let source = page();
    let view = observer(&source);
    let input =
        PropertyDefinitionInput::new(&source, "".into(), PropertyDescriptor::default()).unwrap();
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "({});").object_id().unwrap();
    assert_eq!(
        input.define(&mut neutral, object),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.heap.get_own(object, "").unwrap().is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_throw_restores_the_outer_root_chain_and_stack() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new Proxy({},{defineProperty(){throw 7;}});")
        .object_id()
        .unwrap();
    let owner = vm.argument_list_owner().unwrap();
    let roots = crate::vm::realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(object);
    vm.stack.push(Value::Number(9.0));
    vm.synchronize_scheduling_usage().unwrap();
    let input = input(&vm);
    vm.with_realm_call_roots(roots.clone(), |vm| {
        assert_eq!(
            input.define(vm, object),
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        );
        assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &roots));
        assert_eq!(vm.stack, vec![Value::Number(9.0)]);
        Ok(())
    })
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
    drop(roots);
    drop(owner);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn property_definition_storage_terminal_getter_does_not_delegate() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
        let _ = scope.fail();
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let object = run(
        &mut vm,
        "var target={};new Proxy(target,{get defineProperty(){return markTerminal();}});",
    )
    .object_id()
    .unwrap();
    let input = input(&vm);
    assert_eq!(
        input.define(&mut vm, object),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_terminal_trap_result_is_not_accepted_as_false() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
        let _ = scope.fail();
        Ok(crate::HostValue::Bool(false))
    })
    .unwrap();
    let object = run(
        &mut vm,
        "new Proxy({},{defineProperty(){return markTerminal();}});",
    )
    .object_id()
    .unwrap();
    let input = input(&vm);
    assert_eq!(
        input.define(&mut vm, object),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_typed_coercion_keeps_object_inputs_rooted() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array(1),value={[Symbol.toPrimitive](){$262.gc();return 7n;}};
        Reflect.defineProperty(source,'0',{value})&&source[0]===7n;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_forward_definitions_keep_values_and_accessors() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var child=$262.createRealm(),target=child.evalScript('({})'),text='abcdefghijklmnopqrstuvwxyz0123456789';
        Object.defineProperty(target,'data',{value:text,configurable:true});
        Object.defineProperty(target,'access',{get(){ $262.gc();return text;},configurable:true});
        target.data===text&&target.access===text&&Object.getOwnPropertyDescriptor(target,'data').value===text;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_reverse_definitions_keep_values_and_error_realms() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var child=$262.createRealm(),target={};child.global.target=target;
        var okay=child.evalScript("var text='abcdefghijklmnopqrstuvwxyz0123456789';Object.defineProperty(target,'data',{value:text,configurable:true});Object.defineProperty(target,'access',{get(){$262.gc();return text;},configurable:true});target.data===text&&target.access===text;");
        var bad=new Proxy({},{defineProperty:Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get});
        child.global.bad=bad;child.global.parentTypeError=TypeError;
        okay&&child.evalScript("try{Reflect.defineProperty(bad,'x',{value:1});false;}catch(error){error instanceof parentTypeError&&!(error instanceof TypeError);}");
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn property_definition_storage_tail_called_traps_finish_results_and_throws() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function accept(){'use strict';return true;}function trap(){'use strict';return accept();}
        var marker={};function fail(){'use strict';throw marker;}function bad(){'use strict';return fail();}
        var accepted=Reflect.defineProperty(new Proxy({},{defineProperty:trap}),'x',{value:1}),caught=false;
        try{Reflect.defineProperty(new Proxy({},{defineProperty:bad}),'x',{value:1});}catch(error){caught=error===marker;}accepted&&caught;
    "#
        ),
        Value::Bool(true)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_property_definition_storage_contracts() {
        property_definition_storage_input_outlives_the_vm();
        property_definition_storage_neutral_consumer_checks_original_owner();
        property_definition_storage_proxy_revocation_keeps_captured_target_through_gc();
        property_definition_storage_proxy_descriptor_fields_and_false_result_are_preserved();
        property_definition_storage_array_species_definitions_preserve_map_and_filter();
        property_definition_storage_all_field_payloads_keep_independent_credit();
        property_definition_storage_delegation_does_not_charge_one_payload_twice();
        property_definition_storage_ordinary_heap_handoff_releases_native_credit();
        property_definition_storage_key_copy_keeps_its_own_payload();
        property_definition_storage_symbol_keys_keep_identity_and_descriptor_flags();
        property_definition_storage_missing_descriptor_fields_stay_absent();
        property_definition_storage_accessor_functions_survive_descriptor_mutation_and_gc();
        property_definition_storage_fresh_trap_survives_one_fixed_gc_schedule();
        property_definition_storage_revoked_proxy_still_rejects_definition();
        property_definition_storage_document_adoption_keeps_destination_policy();
        property_definition_storage_source_failure_does_not_poison_the_destination();
        property_definition_storage_failed_destination_does_not_enter_the_trap();
        property_definition_storage_zero_payload_keeps_terminal_identity();
        property_definition_storage_throw_restores_the_outer_root_chain_and_stack();
        property_definition_storage_terminal_getter_does_not_delegate();
        property_definition_storage_terminal_trap_result_is_not_accepted_as_false();
        property_definition_storage_typed_coercion_keeps_object_inputs_rooted();
        property_definition_storage_forward_definitions_keep_values_and_accessors();
        property_definition_storage_reverse_definitions_keep_values_and_error_realms();
        property_definition_storage_tail_called_traps_finish_results_and_throws();
    }
}
