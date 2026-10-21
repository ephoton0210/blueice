// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny successful binary snapshots verify native ownership without quota probes.

use super::*;

type Bytes = TransportVector<u8>;
type Elements = TransportVector<Value>;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        "var realm=$262.createRealm();globalThis.buffer=new ArrayBuffer(4);0;",
    );
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn native_bytes(vm: &Vm) -> usize {
    vm.host_scheduling_snapshot().payload_bytes - vm.own_scheduling_usage().payload_bytes
}
fn bytes(vm: &Vm, object: ObjectId) -> Bytes {
    let length = vm.heap.buffer_byte_length(object).unwrap();
    TransportVector::buffer_bytes(vm, object, length, None).unwrap()
}
fn byte_data(values: &Bytes) -> &Vec<u8> {
    &values.values
}
fn elements(vm: &mut Vm, facade: ObjectId) -> Elements {
    vm.test262_foreign_typed_array_values(facade)
        .unwrap()
        .unwrap()
}
fn element_data(values: &Elements) -> &Vec<Value> {
    &values.values
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_bytes_keep_native_credit() {
    let mut vm = page();
    let object = vm
        .lookup_global_name("buffer")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    let snapshot = bytes(&vm, object);
    assert_eq!(byte_data(&snapshot), &vec![0, 0, 0, 0]);
    assert!(native_bytes(&vm) >= before + byte_data(&snapshot).capacity());
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_byte_credit_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = vm
        .lookup_global_name("buffer")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let snapshot = bytes(&vm, object);
    let retained = byte_data(&snapshot).capacity();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_typed_values_keep_native_credit() {
    let mut vm = page();
    let facade = run(&mut vm, "realm.evalScript('new Float64Array([1,2])');")
        .object_id()
        .unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    let snapshot = elements(&mut vm, facade);
    assert_eq!(
        element_data(&snapshot),
        &vec![Value::Number(1.0), Value::Number(2.0)]
    );
    let retained = host_scheduling::slots(element_data(&snapshot))
        + host_scheduling::values_bytes(element_data(&snapshot));
    assert!(native_bytes(&vm) >= before + retained);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_typed_credit_outlives_both_vms() {
    let mut vm = page();
    let view = observer(&vm);
    let facade = run(&mut vm, "realm.evalScript('new Float64Array([1,2])');")
        .object_id()
        .unwrap();
    let snapshot = elements(&mut vm, facade);
    let retained = host_scheduling::slots(element_data(&snapshot))
        + host_scheduling::values_bytes(element_data(&snapshot));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_intrinsic_buffer_clone_ignores_global_override() {
    let mut vm = page();
    run(&mut vm,"var foreign=realm.evalScript('new ArrayBuffer(3)');globalThis.ArrayBuffer=new Proxy(ArrayBuffer,{get:function(){throw 7;}});0;");
    let facade = vm
        .lookup_global_name("foreign")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let local = vm.test262_foreign_buffer_clone(facade).unwrap().unwrap();
    assert_eq!(vm.heap.buffer_byte_length(local).unwrap(), 3);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_intrinsic_typed_snapshot_ignores_global_override() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var accept=realm.evalScript('(view)=>view.length');
        realm.evalScript('var intrinsic=Uint8Array;globalThis.Uint8Array=function(){};0;');
        accept(new Uint8Array([1,2]));
    "#
        ),
        Value::Number(2.0)
    );
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_bigint_payload_outlives_both_vms() {
    let mut vm = page();
    let view = observer(&vm);
    let facade = run(&mut vm, "realm.evalScript('new BigInt64Array([1n,-2n])');")
        .object_id()
        .unwrap();
    let snapshot = elements(&mut vm, facade);
    assert_eq!(
        element_data(&snapshot),
        &vec![Value::BigInt(1.into()), Value::BigInt((-2).into())]
    );
    let retained = host_scheduling::slots(element_data(&snapshot))
        + host_scheduling::values_bytes(element_data(&snapshot));
    assert!(host_scheduling::values_bytes(element_data(&snapshot)) > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_invalid_byte_range_releases_credit() {
    let mut vm = page();
    let object = run(&mut vm, "buffer;").object_id().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    assert!(TransportVector::buffer_bytes(&vm, object, 5, None).is_err());
    assert_eq!(native_bytes(&vm), before);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_shared_read_fills_admitted_storage() {
    let mut vm = page();
    let object = run(
        &mut vm,
        "var shared=new SharedArrayBuffer(2);new Uint8Array(shared).set([3,4]);shared;",
    )
    .object_id()
    .unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    let snapshot = bytes(&vm, object);
    assert_eq!(byte_data(&snapshot), &vec![3, 4]);
    assert_eq!(native_bytes(&vm), before + byte_data(&snapshot).capacity());
    drop(snapshot);
    assert_eq!(native_bytes(&vm), before);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_immutable_adoption_releases_native_credit() {
    let mut vm = page();
    let source = vm
        .with_roots(|heap| heap.alloc_immutable_array_buffer(vec![3, 4], None))
        .unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&vm);
    let snapshot = BufferSnapshot::capture(&vm, source, None).unwrap();
    let retained = snapshot.bytes.as_ref().unwrap().values.capacity();
    assert_eq!(native_bytes(&vm), before + retained);
    let copied = snapshot.into_buffer(&mut vm).unwrap();
    assert!(vm.heap.buffer_is_immutable(copied).unwrap());
    assert_eq!(vm.heap.array_buffer_copy(copied, 0, 2).unwrap(), vec![3, 4]);
    assert_eq!(native_bytes(&vm), before);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_active_owner_preserves_neutral_policy() {
    let mut owner_vm = page();
    let view = observer(&owner_vm);
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "new ArrayBuffer(2);")
        .object_id()
        .unwrap();
    let guard = realm_reentrancy::register_active(&mut owner_vm).unwrap();
    let snapshot = bytes(&neutral, object);
    let retained = byte_data(&snapshot).capacity();
    assert!(neutral.host_scheduling.is_none());
    drop(guard);
    drop(owner_vm);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_dedup_keeps_capacity_credit_after_vm_drop() {
    let vm = page();
    let view = observer(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let mut plan = TransportVector::capture(
        [vm.object_prototype, vm.object_prototype].into_iter(),
        owner.as_ref(),
    )
    .unwrap();
    let retained = host_scheduling::slots(&plan.values);
    plan.dedup();
    assert_eq!(plan.len(), 1);
    assert_eq!(host_scheduling::slots(&plan.values), retained);
    drop(owner);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(plan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_terminal_marker_preserves_cleanup() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "buffer;").object_id().unwrap();
    let snapshot = bytes(&vm, object);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        snapshot.validate_owner(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        TransportVector::buffer_bytes(&vm, object, 4, None).err(),
        Some(RuntimeError::SchedulingLimit)
    );
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        byte_data(&snapshot).capacity()
    );
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_foreign_set_uses_internal_length() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=realm.evalScript('var source=new Uint8Array([3,4]);Object.defineProperty(source,"length",{get:function(){throw 7;}});source;');
        var target=new Uint8Array(2);target.set(source);target[0]*10+target[1];
    "#
        ),
        Value::Number(34.0)
    );
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_sibling_views_share_mirror_writes() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var foreign=realm.evalScript('globalThis.ab=new ArrayBuffer(2);ab;');
        var a=new Uint8Array(foreign),b=new Uint8Array(foreign);
        a[0]=7;b[1]=8;
        var peek=realm.evalScript('()=>new Uint8Array(ab)[0]*10+new Uint8Array(ab)[1]');
        (a[1]===8&&b[0]===7)?peek():-1;
    "#
        ),
        Value::Number(78.0)
    );
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_resize_refreshes_tracking_views() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var foreign=realm.evalScript('globalThis.ab=new ArrayBuffer(2,{maxByteLength:4});ab;');
        var view=new Uint8Array(foreign);
        var resize=realm.evalScript('()=>{ab.resize(3);new Uint8Array(ab)[2]=9;}');
        resize();view.length*10+view[2];
    "#
        ),
        Value::Number(39.0)
    );
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_foreign_detach_reaches_sibling_views() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var foreign=realm.evalScript('globalThis.ab=new ArrayBuffer(2);ab;');
        var a=new Uint8Array(foreign),b=new Uint8Array(foreign);
        var detach=realm.evalScript('()=>{$262.detachArrayBuffer(ab);}');
        detach();a.length+b.length;
    "#
        ),
        Value::Number(0.0)
    );
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_foreign_locale_string_keeps_element_order() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=realm.evalScript('new Uint8Array([1,2])');
        Uint8Array.prototype.toLocaleString.call(source);
    "#
        ),
        Value::String("1,2".into())
    );
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_element_type_error_releases_snapshot() {
    let mut vm = page();
    let view = observer(&vm);
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=realm.evalScript('new Uint8Array([1])');
        var result=false;
        try {new BigInt64Array(1).set(source);} catch(error) {result=error instanceof TypeError;}
        result;
    "#
        ),
        Value::Bool(true)
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_out_of_bounds_precedes_target_range_error() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=realm.evalScript('var ab=new ArrayBuffer(2,{maxByteLength:4});var view=new Uint8Array(ab,0,2);ab.resize(1);view;');
        var result=false;
        try {new Uint8Array(1).set(source,5);} catch(error) {result=error instanceof TypeError;}
        result;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_two_limb_biguint_keeps_payload_credit() {
    let mut vm = page();
    let view = observer(&vm);
    let facade = run(
        &mut vm,
        "realm.evalScript('new BigUint64Array([1099511627776n])');",
    )
    .object_id()
    .unwrap();
    let snapshot = elements(&mut vm, facade);
    assert_eq!(host_scheduling::values_bytes(element_data(&snapshot)), 8);
    assert_eq!(
        element_data(&snapshot),
        &vec![Value::BigInt(1099511627776u64.into())]
    );
    let retained = host_scheduling::slots(element_data(&snapshot)) + 8;
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_immutable_adoption_between_documents() {
    let mut source = page();
    let source_view = observer(&source);
    let object = source
        .with_roots(|heap| heap.alloc_immutable_array_buffer(vec![5, 6], None))
        .unwrap();
    let snapshot = BufferSnapshot::capture(&source, object, None).unwrap();
    let retained = snapshot.bytes.as_ref().unwrap().values.capacity();
    let mut target = page();
    assert!(!source
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(target.host_scheduling.as_ref().unwrap()));
    target.synchronize_scheduling_usage().unwrap();
    let before = native_bytes(&target);
    drop(source);
    assert_eq!(
        source_view.host_scheduling_snapshot().payload_bytes,
        retained
    );
    let buffer = snapshot.into_buffer(&mut target).unwrap();
    assert!(target.heap.buffer_is_immutable(buffer).unwrap());
    assert_eq!(
        target.heap.array_buffer_copy(buffer, 0, 2).unwrap(),
        vec![5, 6]
    );
    assert_eq!(source_view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(native_bytes(&target), before);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_failed_destination_releases_source_credit() {
    let mut source = page();
    let view = observer(&source);
    let object = source
        .with_roots(|heap| heap.alloc_immutable_array_buffer(vec![5, 6], None))
        .unwrap();
    let snapshot = BufferSnapshot::capture(&source, object, None).unwrap();
    let mut target = page();
    let _ = target.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        snapshot.into_buffer(&mut target),
        Err(RuntimeError::SchedulingLimit)
    );
    drop(source);
    drop(target);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_existing_root_owner_covers_neutral_snapshot() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "new ArrayBuffer(2);")
        .object_id()
        .unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    let snapshot = neutral
        .with_realm_call_roots(roots, |vm| {
            assert!(vm.host_scheduling.is_none());
            TransportVector::buffer_bytes(vm, object, 2, None)
        })
        .unwrap();
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    let retained = snapshot.values.capacity();
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn transport_buffer_storage_bounded_peer_lends_ownership_to_neutral_parent() {
    let mut neutral = Vm::default();
    neutral.install_test262_harness().unwrap();
    let facade = run(
        &mut neutral,
        "var realm=$262.createRealm();realm.evalScript('new Float64Array([1,2])');",
    )
    .object_id()
    .unwrap();
    let (realm_id, _, _, _) = neutral.test262_foreign_reference(facade).unwrap();
    let child = neutral.test262_realm_handle(realm_id).unwrap();
    let caller = neutral.object_prototype.heap;
    child
        .edit(caller, Vm::use_bounded_host_microtask_checkpoints)
        .unwrap();
    let view = child.read(caller, |vm| Ok(observer(vm))).unwrap();
    let snapshot = elements(&mut neutral, facade);
    assert!(neutral.host_scheduling.is_none());
    let retained = host_scheduling::slots(element_data(&snapshot))
        + host_scheduling::values_bytes(element_data(&snapshot));
    drop(child);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_transport_buffer_storage_contracts() {
        transport_buffer_storage_bytes_keep_native_credit();
        transport_buffer_storage_byte_credit_outlives_the_vm();
        transport_buffer_storage_typed_values_keep_native_credit();
        transport_buffer_storage_typed_credit_outlives_both_vms();
        transport_buffer_storage_intrinsic_buffer_clone_ignores_global_override();
        transport_buffer_storage_intrinsic_typed_snapshot_ignores_global_override();
        transport_buffer_storage_bigint_payload_outlives_both_vms();
        transport_buffer_storage_invalid_byte_range_releases_credit();
        transport_buffer_storage_shared_read_fills_admitted_storage();
        transport_buffer_storage_immutable_adoption_releases_native_credit();
        transport_buffer_storage_active_owner_preserves_neutral_policy();
        transport_buffer_storage_dedup_keeps_capacity_credit_after_vm_drop();
        transport_buffer_storage_terminal_marker_preserves_cleanup();
        transport_buffer_storage_foreign_set_uses_internal_length();
        transport_buffer_storage_sibling_views_share_mirror_writes();
        transport_buffer_storage_resize_refreshes_tracking_views();
        transport_buffer_storage_foreign_detach_reaches_sibling_views();
        transport_buffer_storage_foreign_locale_string_keeps_element_order();
        transport_buffer_storage_element_type_error_releases_snapshot();
        transport_buffer_storage_out_of_bounds_precedes_target_range_error();
        transport_buffer_storage_two_limb_biguint_keeps_payload_credit();
        transport_buffer_storage_immutable_adoption_between_documents();
        transport_buffer_storage_failed_destination_releases_source_credit();
        transport_buffer_storage_existing_root_owner_covers_neutral_snapshot();
        transport_buffer_storage_bounded_peer_lends_ownership_to_neutral_parent();
    }
}
