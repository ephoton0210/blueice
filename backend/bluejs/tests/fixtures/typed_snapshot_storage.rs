// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny local snapshots test ownership without resource-limit probes.

use super::*;

type Values = TransportVector<Value>;

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

fn snapshot(vm: &Vm, object: ObjectId, start: usize, length: usize) -> Values {
    vm.typed_array_read_values(object, start, length).unwrap()
}

fn data(values: &Values) -> &[Value] {
    values
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_local_values_keep_native_credit() {
    let mut vm = page();
    let object = run(&mut vm, "new Float64Array([1,2]);")
        .object_id()
        .unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before =
        vm.host_scheduling_snapshot().payload_bytes - vm.own_scheduling_usage().payload_bytes;
    let values = snapshot(&vm, object, 0, 2);
    assert_eq!(data(&values), &vec![Value::Number(1.0), Value::Number(2.0)]);
    let retained = values.retained_slots() + host_scheduling::values_bytes(data(&values));
    let after =
        vm.host_scheduling_snapshot().payload_bytes - vm.own_scheduling_usage().payload_bytes;
    assert!(after >= before + retained);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_local_values_outlive_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new Float64Array([1,2]);")
        .object_id()
        .unwrap();
    let values = snapshot(&vm, object, 0, 2);
    let retained = values.retained_slots() + host_scheduling::values_bytes(data(&values));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(values);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_bigint_range_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new BigInt64Array([7n,1099511627776n,-2n]);")
        .object_id()
        .unwrap();
    let values = snapshot(&vm, object, 1, 2);
    assert_eq!(
        data(&values),
        &vec![
            Value::BigInt(1099511627776u64.into()),
            Value::BigInt((-2).into())
        ]
    );
    let retained = values.retained_slots() + host_scheduling::values_bytes(data(&values));
    assert_eq!(host_scheduling::values_bytes(data(&values)), 12);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(values);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_sort_compares_the_initial_bigint_snapshot() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array([3n,1n,2n]);
        source.sort(function(a,b){source[0]=9n;return a<b?-1:a>b?1:0;});
        source.join();
    "#
        ),
        Value::String("1,2,3".into())
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_default_sort_preserves_zero_and_nan_order() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new Float64Array([NaN,0,-0,1,NaN,-1]);source.sort();
        source[0]===-1&&Object.is(source[1],-0)&&Object.is(source[2],0)&&
            source[3]===1&&Number.isNaN(source[4])&&Number.isNaN(source[5]);
    "#
        ),
        Value::Bool(true)
    );
}

fn native_bytes(vm: &Vm) -> usize {
    vm.host_scheduling_snapshot().payload_bytes - vm.own_scheduling_usage().payload_bytes
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_empty_and_invalid_ranges_release_credit() {
    let mut vm = page();
    let object = run(&mut vm, "new Uint8Array([1,2]);").object_id().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    assert!(snapshot(&vm, vm.object_prototype, 2, 0).is_empty());
    assert_eq!(
        vm.typed_array_read_values(object, 1, 2).err(),
        Some(RuntimeError::TypeError(
            "TypedArray is out of bounds".into()
        ))
    );
    assert_eq!(native_bytes(&vm), before);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_sort_fixed_permutations() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var inputs=[[1,2,3],[1,3,2],[2,1,3],[2,3,1],[3,1,2],[3,2,1]],okay=true;
        for(var input of inputs){var value=new Int16Array(input);var copied=value.toSorted((a,b)=>a-b);
            okay=okay&&copied.join()==='1,2,3'&&value.join()===input.join();
            value.sort((a,b)=>a-b);okay=okay&&value.join()==='1,2,3';}
        okay;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_stable_equivalence_groups() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var values=new Int16Array([22,11,21,12,23,13]);
        values.sort((a,b)=>Math.floor(a/10)-Math.floor(b/10));values.join();
    "#
        ),
        Value::String("11,12,13,22,21,23".into())
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_sort_merges_several_natural_runs() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        new Int16Array([2,1,4,3,6,5,8,7,10,9,12,11]).toSorted((a,b)=>a-b).join();
    "#
        ),
        Value::String("1,2,3,4,5,6,7,8,9,10,11,12".into())
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_comparator_throw_releases_ownership() {
    let mut vm = page();
    let view = observer(&vm);
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array([3n,1n,2n]),marker={},caught=false;
        try{source.sort(()=>{throw marker;});}catch(error){caught=error===marker;}
        caught&&source.join()==='3,1,2';
    "#
        ),
        Value::Bool(true)
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_comparator_coercion_throw_is_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new Uint8Array([2,1]),marker={},caught=false;
        try{source.sort(()=>({valueOf(){throw marker;}}));}catch(error){caught=error===marker;}
        caught&&source.join()==='2,1';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_sort_detach_stops_comparing() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new Uint8Array([3,2,1]),calls=0;
        var result=source.sort((a,b)=>{calls++;$262.detachArrayBuffer(source.buffer);return a-b;});
        result===source&&source.length===0&&calls===1;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_to_sorted_detach_keeps_the_snapshot() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new Uint8Array([3,2,1]);
        var result=source.toSorted((a,b)=>{if(source.length)$262.detachArrayBuffer(source.buffer);return a-b;});
        source.length===0&&result.join()==='1,2,3';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_empty_and_singleton_validate_comparators() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var calls=0,rejected=0;
        for(var input of [[],[7]]){var value=new Uint8Array(input);value.sort(()=>{calls++;return 0;});
            try{value.toSorted(null);}catch(error){if(error instanceof TypeError)rejected++;}}
        calls===0&&rejected===2&&new Uint8Array([7]).toSorted()[0]===7;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_copy_methods_preserve_overlapping_bigints() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new BigInt64Array([1n,2n,3n]);source.set(source.subarray(0,2),1);
        var copied=source.join()==='1,1,2';source.copyWithin(0,1,3);source.reverse();
        copied&&source.join()==='2,2,1'&&source.toReversed().join()==='1,2,2'&&
            source.with(1,9n).join()==='2,9,1'&&source.join()==='2,2,1';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_slice_subrange_preserves_nan_payloads() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=new Float32Array(3),bits=new Uint32Array(source.buffer);
        bits[0]=1;bits[1]=0x7f800001;bits[2]=0xffbfffff;
        var copy=source.slice(1),result=new Uint32Array(copy.buffer);
        result.length===2&&result[0]===0x7f800001&&result[1]===0xffbfffff;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_foreign_species_moves_bigint_range_through_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var other=$262.createRealm().global,source=new BigInt64Array([7n,9n]);
        other.collect=function(){$262.gc();};
        other.eval('globalThis.Target=class extends BigUint64Array{get set(){collect();return super.set;}};');
        source.constructor={[Symbol.species]:other.Target};var result=source.slice();
        result instanceof other.Target&&result.join()==='7,9';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_array_handoff_returns_native_credit() {
    let mut vm = page();
    vm.array_create_prototype().unwrap();
    let object = run(
        &mut vm,
        "globalThis.source=new BigInt64Array([7n,9n]);source;",
    )
    .object_id()
    .unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    let values = snapshot(&vm, object, 0, 2);
    assert!(native_bytes(&vm) > before);
    let array = values.into_array(&mut vm).unwrap().object_id().unwrap();
    assert_eq!(native_bytes(&vm), before);
    assert_eq!(vm.heap.get(array, "0").unwrap(), Value::BigInt(7.into()));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_array_handoff_between_documents() {
    let mut source = page();
    let view = observer(&source);
    let object = run(&mut source, "new BigInt64Array([7n,9n]);")
        .object_id()
        .unwrap();
    let values = snapshot(&source, object, 0, 2);
    let retained = values.retained_slots() + host_scheduling::values_bytes(data(&values));
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    let mut target = page();
    target.array_create_prototype().unwrap();
    target.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&target);
    let array = values.into_array(&mut target).unwrap().object_id().unwrap();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(native_bytes(&target), before);
    assert_eq!(
        target.heap.get(array, "1").unwrap(),
        Value::BigInt(9.into())
    );
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_active_owner_preserves_neutral_policy() {
    let mut source = page();
    let view = observer(&source);
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "new Float64Array([1,2]);")
        .object_id()
        .unwrap();
    let guard = realm_reentrancy::register_active(&mut source).unwrap();
    let values = snapshot(&neutral, object, 0, 2);
    assert!(neutral.host_scheduling.is_none());
    let retained = values.retained_slots();
    drop(guard);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(values);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_existing_root_owner_covers_neutral_range() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "new Float64Array([1,2]);")
        .object_id()
        .unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(object);
    let values = neutral
        .with_realm_call_roots(roots, |vm| vm.typed_array_read_values(object, 0, 2))
        .unwrap();
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    let retained = values.retained_slots();
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(values);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_control_growth_outlives_the_vm() {
    let vm = page();
    let view = observer(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let mut plan = TransportVector::capture(std::iter::empty(), owner.as_ref()).unwrap();
    for index in 0usize..6 {
        plan.push((index, index + 1)).unwrap();
    }
    let retained = plan.retained_slots();
    plan.truncate(2);
    assert_eq!(plan.retained_slots(), retained);
    drop(owner);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(plan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_terminal_marker_releases_sort_storage() {
    let mut vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
        let _ = scope.fail();
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let script = crate::compile(
        &crate::parse("new BigInt64Array([2n,1n]).sort(()=>{markTerminal();return 0;});").unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.execute_script(&script),
        Err(RuntimeError::SchedulingLimit)
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_biguint_subrange_keeps_primitive_values() {
    let mut vm = page();
    let object = run(&mut vm, "new BigUint64Array([1n,1099511627776n]);")
        .object_id()
        .unwrap();
    let values = snapshot(&vm, object, 1, 1);
    assert_eq!(data(&values), &[Value::BigInt(1099511627776u64.into())]);
    assert_eq!(host_scheduling::values_bytes(data(&values)), 8);
}

#[cfg_attr(test, test)]
fn typed_snapshot_storage_monotone_comparator_uses_one_run() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var calls=0,source=new Uint8Array([6,5,4,3,2,1]);
        source.sort((a,b)=>{calls++;return a-b;});calls===5&&source.join()==='1,2,3,4,5,6';
    "#
        ),
        Value::Bool(true)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_typed_snapshot_storage_contracts() {
        typed_snapshot_storage_local_values_keep_native_credit();
        typed_snapshot_storage_local_values_outlive_the_vm();
        typed_snapshot_storage_bigint_range_outlives_the_vm();
        typed_snapshot_storage_sort_compares_the_initial_bigint_snapshot();
        typed_snapshot_storage_default_sort_preserves_zero_and_nan_order();
        typed_snapshot_storage_empty_and_invalid_ranges_release_credit();
        typed_snapshot_storage_sort_fixed_permutations();
        typed_snapshot_storage_stable_equivalence_groups();
        typed_snapshot_storage_sort_merges_several_natural_runs();
        typed_snapshot_storage_comparator_throw_releases_ownership();
        typed_snapshot_storage_comparator_coercion_throw_is_preserved();
        typed_snapshot_storage_sort_detach_stops_comparing();
        typed_snapshot_storage_to_sorted_detach_keeps_the_snapshot();
        typed_snapshot_storage_empty_and_singleton_validate_comparators();
        typed_snapshot_storage_copy_methods_preserve_overlapping_bigints();
        typed_snapshot_storage_slice_subrange_preserves_nan_payloads();
        typed_snapshot_storage_foreign_species_moves_bigint_range_through_gc();
        typed_snapshot_storage_array_handoff_returns_native_credit();
        typed_snapshot_storage_array_handoff_between_documents();
        typed_snapshot_storage_active_owner_preserves_neutral_policy();
        typed_snapshot_storage_existing_root_owner_covers_neutral_range();
        typed_snapshot_storage_control_growth_outlives_the_vm();
        typed_snapshot_storage_terminal_marker_releases_sort_storage();
        typed_snapshot_storage_biguint_subrange_keeps_primitive_values();
        typed_snapshot_storage_monotone_comparator_uses_one_run();
    }
}
