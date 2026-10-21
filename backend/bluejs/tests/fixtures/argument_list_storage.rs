// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny argument lists check ownership at their collection boundary.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    run(&mut vm, "globalThis.list=['ab',3];0;");
    vm.stack.clear();
    vm
}
fn list(vm: &mut Vm) -> Value {
    vm.lookup_global_name("list").unwrap().unwrap()
}
fn collect<T>(
    vm: &mut Vm,
    list: &Value,
    operation: impl FnOnce(&mut Vm, &Vec<Value>) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    vm.with_array_like_values(list, |vm, values| operation(vm, &values.values))
}
fn native_bytes(vm: &Vm) -> usize {
    vm.host_scheduling_snapshot().payload_bytes - vm.own_scheduling_usage().payload_bytes
}

#[cfg_attr(test, test)]
fn argument_list_storage_roots_do_not_copy_primitive_buffers() {
    let mut vm = page();
    let list = list(&mut vm);
    collect(&mut vm, &list, |vm, values| {
        assert_eq!(
            values,
            &vec![Value::String("ab".into()), Value::Number(3.0)]
        );
        assert!(!vm
            .stack
            .iter()
            .any(|value| matches!(value, Value::String(_))));
        Ok(())
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn argument_list_storage_rust_held_values_retain_native_credit() {
    let mut vm = page();
    let list = list(&mut vm);
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    collect(&mut vm, &list, |vm, values| {
        vm.synchronize_scheduling_usage()?;
        let bytes = host_scheduling::slots(values) + host_scheduling::values_bytes(values);
        assert!(native_bytes(vm) >= before + bytes,
            "argument allocation needs {bytes} independently owned bytes; before={before}, during={}", native_bytes(vm));
        Ok(())
    }).unwrap();
}

#[cfg_attr(test, test)]
fn argument_list_storage_getter_error_restores_the_existing_stack_prefix() {
    let mut vm = page();
    run(
        &mut vm,
        "Object.defineProperty(list,'1',{get:function(){throw 7;}});0;",
    );
    vm.stack.clear();
    vm.stack.push(Value::Number(9.0));
    let list = list(&mut vm);
    assert_eq!(
        collect(&mut vm, &list, |_, _| Ok(())),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(vm.stack, vec![Value::Number(9.0)]);
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn finished(vm: &Vm) {
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.pending_tail_call.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn argument_list_storage_objects_survive_growth_and_scoped_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        r#"
        globalThis.list={length:6};
        for(let index=0;index<6;index++) {
            Object.defineProperty(list,index,{get:function(){
                if(index===5)$262.gc();
                return {index:index};
            }});
        }
        0;
    "#,
    );
    let list = list(&mut vm);
    let ids = collect(&mut vm, &list, |vm, values| {
        let ids = values
            .iter()
            .filter_map(Value::object_id)
            .collect::<Vec<_>>();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        assert_eq!(ids.len(), 6);
        for (index, id) in ids.iter().copied().enumerate() {
            assert_eq!(
                vm.heap.get_own(id, "index")?,
                Some(Value::Number(index as f64))
            );
        }
        Ok(ids)
    })
    .unwrap();
    finished(&vm);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(ids.into_iter().all(|id| !vm.heap.contains(id)));
}

#[cfg_attr(test, test)]
fn argument_list_storage_heap_adoption_releases_source_native_credit() {
    let mut source = page();
    let mut target = Vm::default();
    target.use_bounded_host_microtask_checkpoints().unwrap();
    source.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&source);
    let list = list(&mut source);
    let array = source
        .with_array_like_values(&list, |_, values| values.into_array(&mut target))
        .unwrap();
    assert_eq!(native_bytes(&source), before);
    assert_eq!(
        target
            .heap
            .get_own(array.object_id().unwrap(), "0")
            .unwrap(),
        Some(Value::String("ab".into()))
    );
    assert_eq!(
        target
            .heap
            .get_own(array.object_id().unwrap(), "length")
            .unwrap(),
        Some(Value::Number(2.0))
    );
    finished(&source);
    finished(&target);
}

#[cfg_attr(test, test)]
fn argument_list_storage_primitive_owner_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let list = list(&mut vm);
    let values = vm
        .with_array_like_values(&list, |_, values| Ok(values))
        .unwrap();
    let bytes =
        host_scheduling::slots(&values.values) + host_scheduling::values_bytes(&values.values);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(
        values.values,
        vec![Value::String("ab".into()), Value::Number(3.0)]
    );
    drop(values);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn argument_list_storage_apply_and_reflect_apply_preserve_this() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function combine(text,n){'use strict';return this+text+n;}
        combine.apply('ab',['cd',2])==='abcd2'
          && Reflect.apply(combine,'xy',['z',3])==='xyz3';
    "#
        ),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_validation_precedes_getters_and_null_apply_is_empty() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var reads=0;var caught=0;
        var list={get length(){reads++;return 0;}};
        try{Reflect.apply(7,null,list);}catch(error){if(error instanceof TypeError)caught++;}
        try{Function.prototype.apply.call(7,null,list);}catch(error){if(error instanceof TypeError)caught++;}
        try{Reflect.construct(function(){},list,7);}catch(error){if(error instanceof TypeError)caught++;}
        function count(){return arguments.length;}
        reads===0 && caught===3 && count.apply(null,null)===0 && count.apply(null,undefined)===0;
    "#
        ),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_reflect_construct_preserves_proxy_new_target() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        class Parent{constructor(text){this.text=text;}}
        function Alternate(){}
        var reads=0;
        var target=new Proxy(Alternate,{get:function(object,key,receiver){
            if(key==='prototype'){reads++;$262.gc();}
            return Reflect.get(object,key,receiver);
        }});
        var object=Reflect.construct(Parent,['ab'],target);
        object.text==='ab' && Object.getPrototypeOf(object)===Alternate.prototype && reads===1;
    "#
        ),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_spread_calls_and_construction_preserve_results() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function add(a,b){'use strict';return a+b;}
        class C{constructor(text){this.text=text;}}
        add(...[1,2])===3 && (new C(...['ab'])).text==='ab';
    "#
        ),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_super_spread_preserves_derived_new_target() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        class Parent{constructor(text,n){$262.gc();this.text=text;this.n=n;}}
        class Child extends Parent{constructor(...values){super(...values);}}
        var object=new Child('ab',3);
        object.text==='ab' && object.n===3 && Object.getPrototypeOf(object)===Child.prototype;
    "#
        ),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_direct_eval_spread_preserves_lexical_context_and_throws() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        (function(){let outer=7;var value=eval(...['outer+1']);
            var caught=false;try{eval(...['throw 9;']);}catch(error){caught=error===9;}
            return value===8 && caught;
        })();
    "#
        ),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_getter_order_and_length_coercion_are_unchanged() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var trace='';
        var list={get length(){trace+='L';return{valueOf:function(){trace+='C';$262.gc();return 2;}};},
            get 0(){trace+='0';return 1;},get 1(){trace+='1';return 2;}};
        Reflect.apply((a,b)=>a+b,null,list)===3 && trace==='LC01';
    "#
        ),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_build_string_roots_earlier_getter_outputs() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let args = run(
        &mut vm,
        r#"({get loneCodePoints(){return[65];},get ranges(){$262.gc();return[[66,67]];}});"#,
    );
    assert_eq!(
        vm.test262_build_string(&args).unwrap(),
        Value::String("ABC".into())
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_property_helpers_root_all_getter_outputs() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let args = run(
        &mut vm,
        r#"({get regExp(){return /^a$/u;},get matchStrings(){$262.gc();return['a'];},get nonMatchStrings(){$262.gc();return['b'];}});"#,
    );
    assert_eq!(
        vm.test262_test_property_of_strings(&args).unwrap(),
        Value::Undefined
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_terminal_marker_restores_scope_and_prefix() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.stack.push(Value::Number(9.0));
    let list = list(&mut vm);
    let result = collect(&mut vm, &list, |_, _| {
        let _ = scope.fail();
        Ok(7)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.stack, vec![Value::Number(9.0)]);
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_active_owner_does_not_rebind_neutral_policy() {
    let mut owner_vm = page();
    let view = observer(&owner_vm);
    let mut neutral = Vm::default();
    run(&mut neutral, "globalThis.list=['ab',3];0;");
    let list = list(&mut neutral);
    let guard = realm_reentrancy::register_active(&mut owner_vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    collect(&mut neutral, &list, |vm, values| {
        assert!(vm.host_scheduling.is_none());
        let bytes = host_scheduling::slots(values) + host_scheduling::values_bytes(values);
        assert!(view.host_scheduling_snapshot().payload_bytes >= before + bytes);
        Ok(())
    })
    .unwrap();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert!(neutral.host_scheduling.is_none());
    finished(&neutral);
    drop(guard);
}

#[cfg_attr(test, test)]
fn argument_list_storage_existing_root_owner_covers_neutral_conversion() {
    let owner_vm = page();
    let view = observer(&owner_vm);
    let owner = owner_vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .unwrap();
    let mut neutral = Vm::default();
    run(&mut neutral, "globalThis.list=['ab',3];0;");
    let list = list(&mut neutral);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let roots = RealmCallRoots::new(1, Some(&owner)).unwrap();
    roots
        .push(&neutral.heap, list.object_id().unwrap())
        .unwrap();
    neutral
        .with_realm_call_roots(roots, |vm| {
            let current = view.host_scheduling_snapshot().payload_bytes;
            collect(vm, &list, |vm, values| {
                assert!(vm.host_scheduling.is_none());
                let bytes = host_scheduling::slots(values) + host_scheduling::values_bytes(values);
                assert!(view.host_scheduling_snapshot().payload_bytes >= current + bytes);
                Ok(())
            })?;
            assert_eq!(view.host_scheduling_snapshot().payload_bytes, current);
            Ok(())
        })
        .unwrap();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert!(neutral.host_scheduling.is_none());
    finished(&neutral);
}

#[cfg_attr(test, test)]
fn argument_list_storage_call_adoption_reuses_the_collected_allocation() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        "var realm=$262.createRealm();var target=realm.evalScript('(text,probe)=>probe()');0;",
    );
    let caller = vm.object_prototype.heap;
    let child = vm.test262_realms.values().next().unwrap().vm.heap();
    let address = Rc::new(std::cell::Cell::new(None));
    let seen = address.clone();
    vm.install_host_function("probe", 0, move |_: &[HostValue]| {
        let same = realm_reentrancy::with_active_vm(caller, child, |vm| {
            Ok(seen.get() == Some((vm.arguments.as_ptr() as usize, vm.arguments.capacity())))
        })
        .unwrap();
        Ok(HostValue::Bool(same))
    })
    .unwrap();
    run(&mut vm, "globalThis.list=['ab',probe];0;");
    let list = list(&mut vm);
    let target = vm.lookup_global_name("target").unwrap().unwrap();
    assert_eq!(
        vm.with_array_like_values(&list, |vm, values| {
            address.set(Some((
                values.values.as_ptr() as usize,
                values.values.capacity(),
            )));
            values.call_native(vm, target, Value::Undefined, false)
        })
        .unwrap(),
        Value::Bool(true)
    );
    finished(&vm);
}

#[cfg_attr(test, test)]
fn argument_list_storage_empty_and_non_object_lists_restore_roots() {
    let mut vm = page();
    let empty = run(&mut vm, "({length:0});");
    assert_eq!(
        collect(&mut vm, &empty, |_, values| Ok(values.len())).unwrap(),
        0
    );
    assert_eq!(
        collect(&mut vm, &Value::Null, |_, _| Ok(())),
        Err(RuntimeError::TypeError(
            "argument list must be an object".into()
        ))
    );
    finished(&vm);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_argument_list_storage_contracts() {
        argument_list_storage_roots_do_not_copy_primitive_buffers();
        argument_list_storage_rust_held_values_retain_native_credit();
        argument_list_storage_getter_error_restores_the_existing_stack_prefix();
        argument_list_storage_objects_survive_growth_and_scoped_gc();
        argument_list_storage_heap_adoption_releases_source_native_credit();
        argument_list_storage_primitive_owner_outlives_the_vm();
        argument_list_storage_apply_and_reflect_apply_preserve_this();
        argument_list_storage_validation_precedes_getters_and_null_apply_is_empty();
        argument_list_storage_reflect_construct_preserves_proxy_new_target();
        argument_list_storage_spread_calls_and_construction_preserve_results();
        argument_list_storage_super_spread_preserves_derived_new_target();
        argument_list_storage_direct_eval_spread_preserves_lexical_context_and_throws();
        argument_list_storage_getter_order_and_length_coercion_are_unchanged();
        argument_list_storage_build_string_roots_earlier_getter_outputs();
        argument_list_storage_property_helpers_root_all_getter_outputs();
        argument_list_storage_terminal_marker_restores_scope_and_prefix();
        argument_list_storage_active_owner_does_not_rebind_neutral_policy();
        argument_list_storage_existing_root_owner_covers_neutral_conversion();
        argument_list_storage_call_adoption_reuses_the_collected_allocation();
        argument_list_storage_empty_and_non_object_lists_restore_roots();
    }
}
