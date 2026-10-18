// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny callback values and filter selections verify native ownership.

use super::*;

type Element = NativeValue;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

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

fn element(vm: &Vm, object: ObjectId, index: usize) -> Element {
    vm.typed_array_owned_element(object, index, None).unwrap()
}

fn value(element: &Element) -> &Value {
    element.value()
}

#[cfg_attr(test, test)]
fn typed_callback_storage_decoded_bigint_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new BigInt64Array([1099511627776n]);")
        .object_id()
        .unwrap();
    let element = element(&vm, object, 0);
    let retained = host_scheduling::value_payload_bytes(value(&element));
    assert_eq!(retained, 8);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(element);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn native_bytes_in_species(keep: bool) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let observed = Rc::new(std::cell::Cell::new(None));
    let output = observed.clone();
    vm.install_host_function("keep", 0, move |_: &[crate::HostValue]| {
        Ok(crate::HostValue::Bool(keep))
    })
    .unwrap();
    vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        r#"
        var source=new BigInt64Array([1n,2n]);
        source.constructor={[Symbol.species]:function(n){observe();return new BigInt64Array(n);}};
        source.filter(function(){return keep();});0;
    "#,
    );
    observed.get().unwrap()
}

#[cfg_attr(test, test)]
fn typed_callback_storage_selected_values_keep_credit_through_species() {
    let empty = native_bytes_in_species(false);
    let selected = native_bytes_in_species(true);
    let minimum = 2 * std::mem::size_of::<Value>() + 8;
    assert!(
        selected >= empty + minimum,
        "selected={selected}, empty={empty}, minimum={minimum}"
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_filter_reads_later_values_after_callbacks() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new Uint8Array([1,2]);
        source.filter((value,index)=>{if(index===0)source[1]=7;return true;}).join();
    "#
        ),
        Value::String("1,7".into())
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_filter_keeps_original_selected_values() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array([1n,2n]);
        var result=source.filter((value,index)=>{source[index]=9n;return true;});
        result.join()==='1,2'&&source.join()==='9,9';
    "#
        ),
        Value::Bool(true)
    );
}

fn native_bytes(vm: &Vm) -> usize {
    vm.host_scheduling_snapshot().payload_bytes - vm.own_scheduling_usage().payload_bytes
}

#[cfg_attr(test, test)]
fn typed_callback_storage_selected_move_conserves_credit() {
    let mut vm = page();
    let object = run(&mut vm, "new BigInt64Array([1n]);")
        .object_id()
        .unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let item = element(&vm, object, 0);
    let mut selected = TransportVector::with_capacity(0, owner.as_ref()).unwrap();
    selected.push_owned(item).unwrap();
    let retained = selected.retained_slots() + host_scheduling::values_bytes(&selected);
    assert_eq!(native_bytes(&vm), before + retained);
    drop(selected);
    assert_eq!(native_bytes(&vm), before);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_selected_growth_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new BigInt64Array([1n,2n,3n,4n,5n]);")
        .object_id()
        .unwrap();
    let owner = vm.argument_list_owner().unwrap();
    let mut selected = TransportVector::with_capacity(0, owner.as_ref()).unwrap();
    for index in 0..5 {
        selected.push_owned(element(&vm, object, index)).unwrap();
    }
    let retained = selected.retained_slots() + host_scheduling::values_bytes(&selected);
    drop(owner);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(selected);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_borrowed_transfer_refusal_preserves_credit() {
    let first = page();
    let second = page();
    let first_view = observer(&first);
    let second_view = observer(&second);
    let mut left = first
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(2)
        .unwrap();
    let mut right = second
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(3)
        .unwrap();
    assert_eq!(
        left.absorb_from(&mut right),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!((left.bytes(), right.bytes()), (2, 3));
    drop(first);
    drop(second);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, 2);
    assert_eq!(second_view.host_scheduling_snapshot().payload_bytes, 3);
    drop(left);
    drop(right);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(second_view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_scalar_terminal_cleanup() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new BigInt64Array([1n]);")
        .object_id()
        .unwrap();
    let item = element(&vm, object, 0);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(item.validate_owner(), Err(RuntimeError::SchedulingLimit));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 4);
    drop(item);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_predicates_preserve_order_receiver_and_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array([1n,2n,3n]),log='',okay=true;
        var every=source.every(function(value,index,array){$262.gc();log+=index;okay=okay&&this.tag===7&&array===source;return value<2n;},{tag:7});
        var some=source.some(function(value,index){log+=index;return value===2n;});
        var each=source.forEach(function(value,index){log+=index;});
        okay&&!every&&some&&each===undefined&&log==='0101012';
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn typed_callback_storage_find_keeps_original_values_in_both_directions() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var forward=new BigInt64Array([1n,2n]),reverse=new BigInt64Array([1n,2n]);
        var first=forward.find((value,index)=>{forward[index]=9n;return true;});
        var last=reverse.findLast((value,index)=>{reverse[index]=9n;return true;});
        first===1n&&last===2n&&forward.findIndex(()=>true)===0&&reverse.findLastIndex(()=>true)===1&&
            forward.find(()=>false)===undefined&&forward.findLastIndex(()=>false)===-1;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_map_roots_coercion_objects_during_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new Float64Array([1,2]);
        source.map(value=>({get valueOf(){$262.gc();return function(){$262.gc();return value+1;};}})).join();
    "#
        ),
        Value::String("2,3".into())
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn typed_callback_storage_map_keeps_coerced_bigints_through_writes() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array([1n,2n]);
        source.map(value=>String(value+1n)).join();
    "#
        ),
        Value::String("2,3".into())
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_species_throw_restores_prefix_and_roots() {
    let mut vm = page();
    run(&mut vm, "var source=new BigInt64Array([1n,2n]);source.constructor={[Symbol.species]:function(){throw 7;}};var predicate=()=>true;0;");
    let source = vm.lookup_global_name("source").unwrap().unwrap();
    let predicate = vm.lookup_global_name("predicate").unwrap().unwrap();
    let owner = vm.argument_list_owner().unwrap();
    let outer = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    outer.push(source.object_id().unwrap());
    vm.stack.push(Value::Number(9.0));
    vm.synchronize_scheduling_usage().unwrap();
    vm.with_realm_call_roots(outer.clone(), |vm| {
        assert_eq!(
            vm.typed_array_callback_method(&source, &[predicate], TypedArrayCallbackMethod::Filter),
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        );
        assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &outer));
        assert_eq!(vm.stack, vec![Value::Number(9.0)]);
        Ok(())
    })
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn typed_callback_storage_species_runs_after_callbacks_without_rereading() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array([1n,2n]),log='';
        source.constructor={[Symbol.species]:function(n){log+='c';source[0]=9n;return new BigInt64Array(n);}};
        var result=source.filter((value,index)=>{log+=index;return true;});
        log==='01c'&&result.join()==='1,2'&&source[0]===9n;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_empty_filter_constructs_one_empty_result() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array(),calls=0,constructed=0;
        source.constructor={[Symbol.species]:function(n){constructed++;if(n!==0)throw 7;return new BigInt64Array(n);}};
        var result=source.filter(()=>{calls++;return true;});calls===0&&constructed===1&&result.length===0;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_shrink_preserves_missing_callback_values() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var buffer=new ArrayBuffer(2,{maxByteLength:4}),source=new Uint8Array(buffer),seen=[];
        source.set([1,2]);var result=source.filter((value,index)=>{seen.push(value);if(index===0)buffer.resize(0);return false;});
        seen.length===2&&seen[0]===1&&seen[1]===undefined&&result.length===0;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_missing_bigint_selection_preserves_type_error() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var buffer=new ArrayBuffer(16,{maxByteLength:16}),source=new BigInt64Array(buffer),caught=false;
        source.set([1n,2n]);try{source.filter((value,index)=>{if(index===0)buffer.resize(0);return true;});}
        catch(error){caught=error instanceof TypeError;}caught;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_primitive_receiver_is_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var receiver='abcdefghijklmnopqrstuvwxyz0123456789';
        new Uint8Array([1,2]).map(function(value){'use strict';return value+this.length;},receiver).join();
    "#
        ),
        Value::String("37,38".into())
    );
}

#[cfg_attr(test, test)]
fn typed_callback_storage_nested_callbacks_restore_roots() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        new BigInt64Array([1n,2n]).filter((value,index)=>{
            if(index===0)new Float64Array([1,2]).map(inner=>{$262.gc();return inner+1;});return true;
        }).join();
    "#
        ),
        Value::String("1,2".into())
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn typed_callback_storage_adopted_string_outlives_the_vm() {
    let vm = page();
    let view = observer(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let item = NativeValue::adopt(
        Value::String(JsString::from_code_units(vec![0x61; 32])),
        owner.as_ref(),
    )
    .unwrap();
    let retained = host_scheduling::value_payload_bytes(item.value());
    assert!(retained > 0);
    drop(owner);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(item);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_object_roots_have_consumer_lifetime() {
    let mut vm = page();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let owner = vm.argument_list_owner().unwrap();
    let item = NativeValue::adopt(Value::Object(object), owner.as_ref()).unwrap();
    item.with_value(&mut vm, |vm, value| {
        vm.with_roots(|heap| heap.try_collect_major())?;
        assert!(vm.heap.contains(value.object_id().unwrap()));
        Ok(())
    })
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}

#[cfg_attr(test, test)]
fn typed_callback_storage_callback_throw_releases_prior_selections() {
    let mut vm = page();
    let view = observer(&vm);
    assert_eq!(
        run(
            &mut vm,
            r#"
        var marker={},caught=false;try{new BigInt64Array([1n,2n]).filter((value,index)=>{if(index===1)throw marker;return true;});}
        catch(error){caught=error===marker;}caught;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_terminal_marker_releases_prior_selections() {
    let mut vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
        let _ = scope.fail();
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let script = crate::compile(&crate::parse("new BigInt64Array([1n,2n]).filter((value,index)=>{if(index===1)markTerminal();return true;});").unwrap()).unwrap();
    assert_eq!(
        vm.execute_script(&script),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.realm_call_roots.is_none());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_neutral_root_loan_preserves_policy() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "new BigInt64Array([1n]);")
        .object_id()
        .unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(object);
    let item = neutral
        .with_realm_call_roots(roots, |vm| vm.typed_array_owned_element(object, 0, None))
        .unwrap();
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 4);
    drop(item);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_callback_storage_zero_payload_keeps_terminal_identity() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new BigInt64Array(0);").object_id().unwrap();
    let item = element(&vm, object, 0);
    assert_eq!(item.value(), &Value::Undefined);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(item.validate_owner(), Err(RuntimeError::SchedulingLimit));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_typed_callback_storage_contracts() {
        typed_callback_storage_decoded_bigint_outlives_the_vm();
        typed_callback_storage_selected_values_keep_credit_through_species();
        typed_callback_storage_filter_reads_later_values_after_callbacks();
        typed_callback_storage_filter_keeps_original_selected_values();
        typed_callback_storage_selected_move_conserves_credit();
        typed_callback_storage_selected_growth_outlives_the_vm();
        typed_callback_storage_borrowed_transfer_refusal_preserves_credit();
        typed_callback_storage_scalar_terminal_cleanup();
        typed_callback_storage_predicates_preserve_order_receiver_and_gc();
        typed_callback_storage_find_keeps_original_values_in_both_directions();
        typed_callback_storage_map_roots_coercion_objects_during_gc();
        typed_callback_storage_map_keeps_coerced_bigints_through_writes();
        typed_callback_storage_species_throw_restores_prefix_and_roots();
        typed_callback_storage_species_runs_after_callbacks_without_rereading();
        typed_callback_storage_empty_filter_constructs_one_empty_result();
        typed_callback_storage_shrink_preserves_missing_callback_values();
        typed_callback_storage_missing_bigint_selection_preserves_type_error();
        typed_callback_storage_primitive_receiver_is_preserved();
        typed_callback_storage_nested_callbacks_restore_roots();
        typed_callback_storage_adopted_string_outlives_the_vm();
        typed_callback_storage_object_roots_have_consumer_lifetime();
        typed_callback_storage_callback_throw_releases_prior_selections();
        typed_callback_storage_terminal_marker_releases_prior_selections();
        typed_callback_storage_neutral_root_loan_preserves_policy();
        typed_callback_storage_zero_payload_keeps_terminal_identity();
    }
}
