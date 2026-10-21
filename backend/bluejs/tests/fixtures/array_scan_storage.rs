// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny stored-index plans exercise ownership without large-length probes.

use super::*;
use crate::vm::realm_call_storage::RealmCallRoots;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn planned(vm: &mut Vm, object: ObjectId) -> IndexScan {
    let mut scan = vm.index_scan(object, 64).unwrap();
    // Exercise the production builder directly with two or three stored keys.
    // No large public length or optimization-threshold sweep is needed.
    vm.rebuild_index_scan(&mut scan, object).unwrap();
    scan
}

fn retained(scan: &IndexScan) -> usize {
    scan.stored
        .as_ref()
        .map_or(0, TransportVector::retained_slots)
}

#[cfg_attr(test, test)]
fn array_scan_storage_plan_outlives_its_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "({2:7,5:9});").object_id().unwrap();
    let scan = planned(&mut vm, object);
    let bytes = retained(&scan);
    assert!(bytes > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(scan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_scan_storage_neutral_plan_keeps_original_terminal_identity() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "({2:7,5:9});").object_id().unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    let mut scan = neutral
        .with_realm_call_roots(roots, |vm| Ok(planned(vm, object)))
        .unwrap();
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        neutral.scan_next(&mut scan, object, 0),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn array_scan_storage_neutral_getter_checks_original_owner_after_guest() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Number(7.0))
        })
        .unwrap();
    let object = run(&mut neutral, "({get 2(){return markTerminal();}});")
        .object_id()
        .unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    let mut scan = neutral
        .with_realm_call_roots(roots, |vm| Ok(planned(vm, object)))
        .unwrap();
    assert_eq!(
        neutral.array_next_present(&mut scan, object, 0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn array_scan_storage_sorted_prototype_plan_preserves_probes() {
    let mut vm = page();
    let object = run(
        &mut vm,
        "var source=Object.create({5:99,7:11});source[5]=9;source[2]=7;source;",
    )
    .object_id()
    .unwrap();
    let mut scan = planned(&mut vm, object);
    assert_eq!(&scan.stored.as_ref().unwrap()[..], &[2, 5, 7]);
    assert_eq!(
        vm.array_next_present(&mut scan, object, 0),
        Ok(Some((2, Value::Number(7.0))))
    );
    assert_eq!(
        vm.array_previous_present(&mut scan, object, 8),
        Ok(Some((7, Value::Number(11.0))))
    );
}

#[cfg_attr(test, test)]
fn array_scan_storage_public_holes_preserve_includes_and_index_search() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var source=[,2,,4];source.includes(undefined)&&source.indexOf(undefined)===-1&&source.lastIndexOf(2)===1&&source.map(value=>value+1).join()===',3,,5';"), Value::Bool(true));
}

fn loaned(vm: &mut Vm, source: &Vm, object: ObjectId) -> IndexScan {
    let owner = source.argument_list_owner().unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&vm.heap, object).unwrap();
    vm.with_realm_call_roots(roots, |vm| Ok(planned(vm, object)))
        .unwrap()
}

#[cfg_attr(test, test)]
fn array_scan_storage_plan_conserves_actual_capacity_credit() {
    let mut vm = page();
    let object = run(&mut vm, "({2:7,5:9});").object_id().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let scan = planned(&mut vm, object);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + retained(&scan)
    );
    drop(scan);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn array_scan_storage_late_root_loan_adopts_existing_capacity() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "({2:7,5:9});").object_id().unwrap();
    let mut scan = planned(&mut neutral, object);
    let epoch = scan.epoch;
    assert!(scan.owner.is_none());
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    neutral
        .with_realm_call_roots(roots, |vm| {
            assert_eq!(vm.scan_next(&mut scan, object, 0)?, Some(2));
            Ok(())
        })
        .unwrap();
    assert_eq!(scan.epoch, epoch);
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    let bytes = retained(&scan);
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(scan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_scan_storage_late_document_policy_adopts_existing_capacity() {
    let mut vm = Vm::default();
    let object = run(&mut vm, "({2:7,5:9});").object_id().unwrap();
    let mut scan = planned(&mut vm, object);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    assert_eq!(vm.scan_next(&mut scan, object, 0), Ok(Some(2)));
    let bytes = retained(&scan);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(scan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_scan_storage_dense_scans_keep_zero_byte_terminal_identity() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "({});").object_id().unwrap();
    let scan = vm.index_scan(object, 4).unwrap();
    assert!(scan.stored.is_none());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(scan.validate_owner(), Err(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn array_scan_storage_epoch_rebuild_observes_added_keys() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "({2:7,5:9});").object_id().unwrap();
    let mut scan = planned(&mut vm, object);
    let epoch = scan.epoch;
    vm.heap.set(object, "3", Value::Number(8.0)).unwrap();
    assert_eq!(vm.scan_next(&mut scan, object, 3), Ok(Some(3)));
    assert_ne!(scan.epoch, epoch);
    assert_eq!(&scan.stored.as_ref().unwrap()[..], &[2, 3, 5]);
    let bytes = retained(&scan);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(scan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_scan_storage_dense_fallback_releases_slots_and_observes_mutation() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "({2:7,5:9});").object_id().unwrap();
    let mut scan = planned(&mut vm, object);
    // Fixed algorithm bookkeeping, without repeated inputs or resource-limit changes.
    scan.work = 64;
    vm.rebuild_index_scan(&mut scan, object).unwrap();
    let work = scan.work;
    assert!(scan.stored.is_none());
    vm.heap.set(object, "1", Value::Number(4.0)).unwrap();
    assert_eq!(
        vm.array_next_present(&mut scan, object, 0),
        Ok(Some((1, Value::Number(4.0))))
    );
    assert_eq!(scan.work, work);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(scan.validate_owner(), Err(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn array_scan_storage_proxy_receivers_and_prototypes_preserve_probes() {
    let mut vm = page();
    let object = run(&mut vm, "var source={2:7,5:9},seen='';source;")
        .object_id()
        .unwrap();
    let mut scan = planned(&mut vm, object);
    run(&mut vm, "Object.setPrototypeOf(source,new Proxy({7:11},{has(target,key){seen+=key;return key in target;}}));0;");
    assert_eq!(
        vm.array_next_present(&mut scan, object, 6),
        Ok(Some((7, Value::Number(11.0))))
    );
    assert!(scan.stored.is_none());
    assert_eq!(run(&mut vm, "seen;"), Value::String("67".into()));
    let object = run(&mut vm, "seen='';new Proxy({},{has(target,key){seen+='h'+key;return key==='1';},get(target,key){seen+='g'+key;return 9;}});").object_id().unwrap();
    let mut scan = planned(&mut vm, object);
    assert!(scan.stored.is_none());
    assert_eq!(
        vm.array_next_present(&mut scan, object, 0),
        Ok(Some((1, Value::Number(9.0))))
    );
    assert_eq!(run(&mut vm, "seen;"), Value::String("h0h1g1".into()));
}

#[cfg_attr(test, test)]
fn array_scan_storage_borrowed_keys_preserve_canonical_order_and_virtual_brands() {
    let mut vm = page();
    let object = run(&mut vm, "({5:9,2:7,'02':8,a:9});").object_id().unwrap();
    let keys = vm.heap.own_integer_keys_view(object).unwrap().unwrap();
    assert_eq!(keys.collect::<Vec<_>>(), vec![5, 2]);
    assert_eq!(vm.heap.own_integer_keys(object).unwrap(), Some(vec![2, 5]));
    for source in [
        "Object('ab');",
        "new Uint8Array([8,9]);",
        "new Proxy({},{});",
    ] {
        let object = run(&mut vm, source).object_id().unwrap();
        assert!(vm.heap.own_integer_keys_view(object).unwrap().is_none());
        assert!(planned(&mut vm, object).stored.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_scan_storage_work_counts_keys_outside_the_captured_range() {
    let mut vm = page();
    let object = run(&mut vm, "({2:7,20:9});").object_id().unwrap();
    let mut scan = vm.index_scan(object, 16).unwrap();
    vm.rebuild_index_scan(&mut scan, object).unwrap();
    assert_eq!(scan.work, 4);
    assert_eq!(&scan.stored.as_ref().unwrap()[..], &[2]);
}

#[cfg_attr(test, test)]
fn array_scan_storage_dedup_keeps_actual_backing_credit() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "var source=Object.create(Object.create({2:1}));Object.getPrototypeOf(source)[2]=3;source[2]=7;source;").object_id().unwrap();
    let scan = planned(&mut vm, object);
    assert_eq!(&scan.stored.as_ref().unwrap()[..], &[2]);
    let bytes = retained(&scan);
    assert!(bytes >= std::mem::size_of::<u64>());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(scan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_scan_storage_plan_does_not_root_its_receiver_or_accept_foreign_handles() {
    let mut vm = page();
    let view = observer(&vm);
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.set(object, "2", Value::Number(7.0)).unwrap();
    let mut scan = planned(&mut vm, object);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
    assert_eq!(
        vm.scan_next(&mut scan, object, 0),
        Err(RuntimeError::Heap(HeapError::InvalidObject(object)))
    );
    let mut foreign_vm = Vm::default();
    let foreign = run(&mut foreign_vm, "({2:7});").object_id().unwrap();
    assert_eq!(
        vm.heap.own_integer_keys_view(foreign).err(),
        Some(HeapError::InvalidObject(foreign))
    );
    assert_eq!(
        vm.array_previous_present(&mut scan, foreign, 1),
        Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
    );
    let bytes = retained(&scan);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(scan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_scan_storage_getter_errors_preserve_both_probe_directions() {
    let mut vm = page();
    let object = run(&mut vm, "({get 2(){throw 7;},get 5(){throw 9;}});")
        .object_id()
        .unwrap();
    let mut scan = planned(&mut vm, object);
    assert_eq!(
        vm.array_next_present(&mut scan, object, 0),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(
        vm.array_previous_present(&mut scan, object, 6),
        Err(RuntimeError::Thrown(Value::Number(9.0)))
    );
    scan.validate_owner().unwrap();
}

#[cfg_attr(test, test)]
fn array_scan_storage_refused_rebuild_preserves_old_data_and_metadata() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "({2:7,5:9});").object_id().unwrap();
    let mut scan = planned(&mut vm, object);
    let epoch = scan.epoch;
    let work = scan.work;
    let bytes = retained(&scan);
    vm.heap.set(object, "3", Value::Number(8.0)).unwrap();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.rebuild_index_scan(&mut scan, object),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(scan.epoch, epoch);
    assert_eq!(scan.work, work);
    assert_eq!(&scan.stored.as_ref().unwrap()[..], &[2, 5]);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(scan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_scan_storage_empty_plans_keep_terminal_identity() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "({});").object_id().unwrap();
    let scan = planned(&mut vm, object);
    assert!(scan.stored.as_ref().unwrap().is_empty());
    assert_eq!(retained(&scan), 0);
    drop(vm);
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(scan.validate_owner(), Err(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn array_scan_storage_neutral_owner_wins_after_reverse_getters_and_throws() {
    for (reverse, throws) in [(true, false), (false, true), (true, true)] {
        let source = page();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let mut neutral = Vm::default();
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Number(7.0))
            })
            .unwrap();
        let getter = if throws {
            "({get 2(){markTerminal();throw 7;}});"
        } else {
            "({get 2(){return markTerminal();}});"
        };
        let object = run(&mut neutral, getter).object_id().unwrap();
        let mut scan = loaned(&mut neutral, &source, object);
        let result = if reverse {
            neutral.array_previous_present(&mut scan, object, 3)
        } else {
            neutral.array_next_present(&mut scan, object, 0)
        };
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert!(neutral.host_scheduling.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_scan_storage_promise_callback_keeps_public_scan_values_during_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "var result=0;Promise.resolve().then(()=>{var source=[,2,,4];result=source.reduce((acc,value)=>{$262.gc();return acc+value;},0);});0;");
    vm.run_promise_jobs().unwrap();
    assert_eq!(run(&mut vm, "result;"), Value::Number(6.0));
    assert!(vm.realm_call_roots.is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_array_scan_storage_contracts() {
        array_scan_storage_plan_outlives_its_vm();
        array_scan_storage_neutral_plan_keeps_original_terminal_identity();
        array_scan_storage_neutral_getter_checks_original_owner_after_guest();
        array_scan_storage_sorted_prototype_plan_preserves_probes();
        array_scan_storage_public_holes_preserve_includes_and_index_search();
        array_scan_storage_plan_conserves_actual_capacity_credit();
        array_scan_storage_late_root_loan_adopts_existing_capacity();
        array_scan_storage_late_document_policy_adopts_existing_capacity();
        array_scan_storage_dense_scans_keep_zero_byte_terminal_identity();
        array_scan_storage_epoch_rebuild_observes_added_keys();
        array_scan_storage_dense_fallback_releases_slots_and_observes_mutation();
        array_scan_storage_proxy_receivers_and_prototypes_preserve_probes();
        array_scan_storage_borrowed_keys_preserve_canonical_order_and_virtual_brands();
        array_scan_storage_work_counts_keys_outside_the_captured_range();
        array_scan_storage_dedup_keeps_actual_backing_credit();
        array_scan_storage_plan_does_not_root_its_receiver_or_accept_foreign_handles();
        array_scan_storage_getter_errors_preserve_both_probe_directions();
        array_scan_storage_refused_rebuild_preserves_old_data_and_metadata();
        array_scan_storage_empty_plans_keep_terminal_identity();
        array_scan_storage_neutral_owner_wins_after_reverse_getters_and_throws();
        array_scan_storage_promise_callback_keeps_public_scan_values_during_gc();
    }
}
