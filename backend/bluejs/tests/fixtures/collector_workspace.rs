// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny collections and numeric peers check workspace admission.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.stack.push(Value::Object(object));
    // Prepare only VM tracing capacity; the collector itself has not run.
    vm.with_roots(|_| Ok(())).unwrap();
    vm
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn collector_workspace_major_refusal_precedes_marking_and_sweeping() {
    let mut vm = vm();
    let before = vm.heap.stats();
    let pointer = vm.stack.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn collector_workspace_minor_refusal_preserves_the_nursery() {
    let mut vm = vm();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.with_roots(|heap| {
            heap.collect_minor();
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.stack.len(), 1);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

fn bounded_heap(vm: &Vm, config: crate::HeapConfig) -> Heap {
    let mut heap = Heap::new(config).unwrap();
    heap.bind_collector_budget(vm.host_scheduling.as_ref().unwrap().collector_budget())
        .unwrap();
    heap
}

#[cfg_attr(test, test)]
fn collector_workspace_fallible_major_refuses_before_releasing_pins() {
    let mut vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    drop(pin);
    let before = heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.try_collect_major(),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert!(heap.contains(object));
    assert!(heap.take_collection_error().is_none());
}

#[cfg_attr(test, test)]
fn collector_workspace_automatic_minor_refusal_preserves_object_identity() {
    let mut vm = vm();
    let mut heap = bounded_heap(
        &vm,
        crate::HeapConfig {
            nursery_capacity: 1,
            ..Default::default()
        },
    );
    let object = heap.alloc_object(None).unwrap();
    let before = heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.alloc_object(None),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert!(heap.contains(object));
}

#[cfg_attr(test, test)]
fn collector_workspace_automatic_major_refusal_preserves_the_store() {
    let mut vm = vm();
    let mut heap = bounded_heap(
        &vm,
        crate::HeapConfig {
            major_threshold_bytes: 1,
            ..Default::default()
        },
    );
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let before = heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.set(object, "value", Value::Bool(true)),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert_eq!(heap.get(object, "value"), Ok(Value::Undefined));
}

#[cfg_attr(test, test)]
fn collector_workspace_cache_has_one_native_owner_and_reuses_at_full_quota() {
    let mut vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    heap.try_collect_major().unwrap();
    let bytes = heap.collector_storage_bytes();
    assert!(bytes > 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    let peer = leave_room(&mut vm, 0);
    heap.try_collect_minor().unwrap();
    heap.try_collect_major().unwrap();
    assert_eq!(heap.collector_storage_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(peer);
    drop(heap);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn collector_workspace_returned_marks_outlive_the_heap_without_early_refund() {
    let vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let marks = heap.realm_reachable(&HashSet::new(), &[]).unwrap();
    assert!(marks.contains(&object));
    let marks_bytes = marks.capacity() * std::mem::size_of::<ObjectId>();
    assert!(marks_bytes > 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + heap.collector_storage_bytes() + marks_bytes
    );
    drop(heap);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + marks_bytes
    );
    drop(marks);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn collector_workspace_mark_recycle_transfers_ownership_at_full_quota() {
    let mut vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let marks = heap.realm_reachable(&HashSet::new(), &[]).unwrap();
    let _peer = leave_room(&mut vm, 0);
    heap.recycle_collector_marks(marks).unwrap();
    heap.try_collect_major().unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn collector_workspace_foreign_mark_recycle_rejects_and_refunds_the_mark_owner() {
    let vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let mut other = bounded_heap(&vm, crate::HeapConfig::default());
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let marks = heap.realm_reachable(&HashSet::new(), &[]).unwrap();
    let bytes = marks.capacity() * std::mem::size_of::<ObjectId>();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        other.recycle_collector_marks(marks),
        Err(HeapError::InvalidConfig)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
    assert_eq!(other.collector_storage_bytes(), 0);
}

#[cfg_attr(test, test)]
fn collector_workspace_reported_hash_capacity_refusal_refunds_all_staging() {
    let mut vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    for _ in 0..4 {
        let object = heap.alloc_object(None).unwrap();
        heap.root(object).unwrap();
    }
    // Four Vec slots and four requested set slots fit; reported set capacity
    // is seven, so publication must still fail and refund the candidate.
    let room = 8 * std::mem::size_of::<ObjectId>();
    let _peer = leave_room(&mut vm, room);
    let before = heap.stats();
    assert_eq!(
        heap.try_collect_major(),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert_eq!(heap.collector_storage_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}

#[cfg_attr(test, test)]
fn collector_workspace_growth_refusal_restores_the_existing_cache() {
    let mut vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    for _ in 0..4 {
        let object = heap.alloc_object(None).unwrap();
        heap.root(object).unwrap();
    }
    heap.try_collect_major().unwrap();
    let bytes = heap.collector_storage_bytes();
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let before = heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.try_collect_major(),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert_eq!(heap.collector_storage_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn collector_workspace_late_binding_admits_existing_capacity() {
    let vm = vm();
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    heap.try_collect_major().unwrap();
    let bytes = heap.collector_storage_bytes();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    heap.bind_collector_budget(vm.host_scheduling.as_ref().unwrap().collector_budget())
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    heap.bind_collector_budget(vm.host_scheduling.as_ref().unwrap().collector_budget())
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
}

#[cfg_attr(test, test)]
fn collector_workspace_late_binding_refusal_drops_only_unadmitted_cache() {
    let mut vm = vm();
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    heap.try_collect_major().unwrap();
    assert!(heap.collector_storage_bytes() > 0);
    let before = heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.bind_collector_budget(vm.host_scheduling.as_ref().unwrap().collector_budget()),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert!(heap.contains(object));
    assert_eq!(heap.collector_storage_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn collector_workspace_terminal_warm_vm_reuses_native_capacity() {
    let mut vm = vm();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let bytes = vm.heap.collector_storage_bytes();
    let before = vm.heap.stats().major_collections;
    let _peer = leave_room(&mut vm, 0);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(vm.with_roots(|heap| heap.try_collect_major()), Ok(()));
    assert_eq!(vm.heap.stats().major_collections, before + 1);
    assert_eq!(vm.heap.collector_storage_bytes(), bytes);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn collector_workspace_released_pin_does_not_keep_its_target_alive() {
    let vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    drop(pin);
    heap.try_collect_major().unwrap();
    assert!(!heap.contains(object));
}

#[cfg_attr(test, test)]
fn collector_workspace_duplicate_conditional_edges_fit_one_object_queue_slot() {
    let vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let owner = heap.alloc_object(None).unwrap();
    let target = heap.alloc_object(None).unwrap();
    let third = heap.alloc_object(None).unwrap();
    heap.root(third).unwrap();
    heap.root(target).unwrap();
    heap.root(owner).unwrap();
    heap.push_scoped_roots(crate::heap::GcTraceBatch {
        roots: vec![owner; 8],
        edges: vec![(owner, target); 8],
    });
    heap.try_collect_major().unwrap();
    heap.pop_scoped_roots();
    assert!(heap.contains(target));
    assert!(heap.contains(third));
}

#[cfg_attr(test, test)]
fn collector_workspace_minor_uses_borrowed_nursery_and_remembered_edges() {
    let vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let old = heap.alloc_object(None).unwrap();
    heap.root(old).unwrap();
    heap.try_collect_major().unwrap();
    let live = heap.alloc_object(None).unwrap();
    let dead = heap.alloc_object(None).unwrap();
    let weak = heap.alloc_weak_ref(Value::Object(dead), None).unwrap();
    heap.root(weak).unwrap();
    heap.set(old, "child", Value::Object(live)).unwrap();
    heap.try_collect_minor().unwrap();
    assert!(heap.contains(old));
    assert!(heap.contains(live));
    assert!(!heap.contains(dead));
    assert_eq!(heap.weak_ref_target(weak), Ok(None));
    assert_eq!(heap.stats().nursery_objects, 0);
}

#[cfg_attr(test, test)]
fn collector_workspace_ephemeron_chain_reaches_values_and_collects_a_dead_cycle() {
    let vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let map = heap.alloc_weak_collection(true, None).unwrap();
    heap.root(map).unwrap();
    let first = heap.alloc_object(None).unwrap();
    let root = heap.root(first).unwrap();
    let second = heap.alloc_object(None).unwrap();
    heap.weak_collection_set(map, Value::Object(first), Value::Object(second))
        .unwrap();
    heap.weak_collection_set(map, Value::Object(second), Value::Object(first))
        .unwrap();
    heap.try_collect_major().unwrap();
    assert!(heap.contains(second));
    heap.unroot(root).unwrap();
    heap.try_collect_major().unwrap();
    assert!(!heap.contains(first));
    assert!(!heap.contains(second));
}

#[cfg_attr(test, test)]
fn collector_workspace_dependent_signal_follows_an_ephemeron_source() {
    let vm = vm();
    let mut heap = bounded_heap(&vm, crate::HeapConfig::default());
    let prototype = heap.alloc_object(None).unwrap();
    heap.root(prototype).unwrap();
    let map = heap.alloc_weak_collection(true, None).unwrap();
    heap.root(map).unwrap();
    let key = heap.alloc_object(None).unwrap();
    let root = heap.root(key).unwrap();
    let source = heap.alloc_abort_signal(prototype).unwrap();
    let dependent = heap.alloc_abort_signal(prototype).unwrap();
    let mut state = heap.abort_signal(dependent).unwrap().clone();
    state.dependent = true;
    state.sources.push(source);
    state.hooks.push(crate::heap::AbortHook::Host { serial: 1 });
    heap.set_abort_signal(dependent, state).unwrap();
    heap.weak_collection_set(map, Value::Object(key), Value::Object(source))
        .unwrap();
    heap.try_collect_major().unwrap();
    assert!(heap.contains(source));
    assert!(heap.contains(dependent));
    heap.unroot(root).unwrap();
    heap.try_collect_major().unwrap();
    assert!(!heap.contains(source));
    assert!(!heap.contains(dependent));
}

#[cfg_attr(test, test)]
fn collector_workspace_terminal_cold_vm_refuses_without_sweeping() {
    let mut vm = vm();
    let before = vm.heap.stats();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.with_roots(|heap| heap.try_collect_major()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.heap.collector_storage_bytes(), 0);
}

#[cfg_attr(test, test)]
fn collector_workspace_collection_hooks_propagate_noncatchable_refusal() {
    for module_exit in [false, true] {
        let mut vm = vm();
        let before = vm.heap.stats().major_collections;
        let _peer = leave_room(&mut vm, 0);
        let result = if module_exit {
            vm.finish_module_graph_execution(Err(RuntimeError::Thrown(Value::Number(7.0))))
        } else {
            vm.test262_call("gc", &[])
        };
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert!(!result.unwrap_err().is_catchable());
        assert_eq!(vm.heap.stats().major_collections, before);
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_collector_workspace_contracts() {
        collector_workspace_collection_hooks_propagate_noncatchable_refusal();
        collector_workspace_dependent_signal_follows_an_ephemeron_source();
        collector_workspace_terminal_cold_vm_refuses_without_sweeping();
        collector_workspace_major_refusal_precedes_marking_and_sweeping();
        collector_workspace_minor_refusal_preserves_the_nursery();
        collector_workspace_fallible_major_refuses_before_releasing_pins();
        collector_workspace_automatic_minor_refusal_preserves_object_identity();
        collector_workspace_automatic_major_refusal_preserves_the_store();
        collector_workspace_cache_has_one_native_owner_and_reuses_at_full_quota();
        collector_workspace_returned_marks_outlive_the_heap_without_early_refund();
        collector_workspace_mark_recycle_transfers_ownership_at_full_quota();
        collector_workspace_foreign_mark_recycle_rejects_and_refunds_the_mark_owner();
        collector_workspace_reported_hash_capacity_refusal_refunds_all_staging();
        collector_workspace_growth_refusal_restores_the_existing_cache();
        collector_workspace_late_binding_admits_existing_capacity();
        collector_workspace_late_binding_refusal_drops_only_unadmitted_cache();
        collector_workspace_terminal_warm_vm_reuses_native_capacity();
        collector_workspace_released_pin_does_not_keep_its_target_alive();
        collector_workspace_duplicate_conditional_edges_fit_one_object_queue_slot();
        collector_workspace_minor_uses_borrowed_nursery_and_remembered_edges();
        collector_workspace_ephemeron_chain_reaches_values_and_collects_a_dead_cycle();
    }
}
