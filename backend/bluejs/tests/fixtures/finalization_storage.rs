// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small registries and scalar allowances exercise finalization ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
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
fn fixture(vm: &Vm) -> (Heap, ObjectId, ObjectId) {
    let mut heap = Heap::default();
    let registry = heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    heap.root(registry).unwrap();
    let target = heap.alloc_object(None).unwrap();
    let root = heap.root(target).unwrap();
    heap.finalization_registry_register(registry, Value::Object(target), Value::Number(1.0), None)
        .unwrap();
    heap.try_collect_major().unwrap();
    heap.unroot(root).unwrap();
    heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget())
        .unwrap();
    (heap, registry, target)
}

#[cfg_attr(test, test)]
fn finalization_storage_gc_refusal_precedes_weak_clearing_and_sweeping() {
    let mut vm = bounded_vm();
    let (mut heap, _, target) = fixture(&vm);
    let before = heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.try_collect_major(),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert!(heap.contains(target));
}

#[cfg_attr(test, test)]
fn finalization_storage_ready_index_has_a_native_owner() {
    let vm = bounded_vm();
    let (mut heap, _, _) = fixture(&vm);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    heap.try_collect_major().unwrap();
    assert!(vm.host_scheduling_snapshot().payload_bytes > before);
}

fn ready_vm() -> Vm {
    let mut vm = bounded_vm();
    let registry = vm
        .heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    vm.heap.root(registry).unwrap();
    let target = vm.heap.alloc_object(None).unwrap();
    vm.heap
        .finalization_registry_register(registry, Value::Object(target), Value::Number(1.0), None)
        .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let roots: Vec<_> = (0..4).map(|_| vm.heap.root(registry).unwrap()).collect();
    for root in roots {
        vm.heap.unroot(root).unwrap();
    }
    vm.host_finalization_tasks.reserve(4);
    vm.synchronize_scheduling_usage().unwrap();
    vm
}

#[cfg_attr(test, test)]
fn finalization_storage_page_snapshot_refuses_before_task_publication() {
    let mut vm = ready_vm();
    let before = vm.heap.stats();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.enqueue_host_finalization_tasks(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.host_finalization_tasks.is_empty());
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.heap.root_storage_state(), roots);
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    }
}
fn ready_heap(count: usize) -> (Heap, Vec<ObjectId>) {
    assert!(count <= 8);
    let mut heap = Heap::default();
    let mut registries = Vec::new();
    for value in 0..count {
        let registry = heap
            .alloc_finalization_registry(Value::Undefined, None)
            .unwrap();
        heap.root(registry).unwrap();
        let target = heap.alloc_object(None).unwrap();
        heap.finalization_registry_register(
            registry,
            Value::Object(target),
            Value::Number(value as f64),
            None,
        )
        .unwrap();
        registries.push(registry);
    }
    heap.try_collect_major().unwrap();
    (heap, registries)
}
fn bind(heap: &mut Heap, vm: &Vm) {
    heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget())
        .unwrap();
}
fn native_bytes(heap: &Heap) -> usize {
    heap.root_storage_bytes()
        + heap.collector_storage_bytes()
        + heap.realm_pin_storage_bytes()
        + heap.finalization_index_bytes()
}

#[cfg_attr(test, test)]
fn finalization_storage_minor_refusal_preserves_the_nursery_and_cells() {
    let mut vm = bounded_vm();
    let (mut heap, registry, _) = fixture(&vm);
    let young = heap.alloc_object(None).unwrap();
    heap.finalization_registry_register(registry, Value::Object(young), Value::Number(2.0), None)
        .unwrap();
    let before = heap.stats();
    let cells = heap.finalization_cell_storage_state(registry);
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.try_collect_minor(),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert_eq!(heap.finalization_cell_storage_state(registry), cells);
    assert!(heap.contains(young));
    assert!(heap.pending_finalization_registries().next().is_none());
}

#[cfg_attr(test, test)]
fn finalization_storage_minor_collection_clears_only_young_targets() {
    let vm = bounded_vm();
    let (mut heap, registry, old) = fixture(&vm);
    let young = heap.alloc_object(None).unwrap();
    heap.finalization_registry_register(registry, Value::Object(young), Value::Number(2.0), None)
        .unwrap();
    heap.try_collect_minor().unwrap();
    assert!(heap.contains(old));
    assert!(!heap.contains(young));
    assert_eq!(
        heap.take_finalization_registry_cleanup_cell(registry)
            .unwrap()
            .unwrap()
            .1,
        Value::Number(2.0)
    );
    assert!(heap
        .take_finalization_registry_cleanup_cell(registry)
        .unwrap()
        .is_none());
    assert_eq!(heap.finalization_index_bytes(), 0);
}

#[cfg_attr(test, test)]
fn finalization_storage_refusal_preserves_released_foreign_pins() {
    let mut vm = bounded_vm();
    let (mut heap, _, target) = fixture(&vm);
    let pin = heap.realm_root(target).unwrap();
    drop(pin);
    let roots = heap.root_storage_state();
    let pins = heap.realm_pin_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.try_collect_major(),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.root_storage_state(), roots);
    assert_eq!(heap.realm_pin_storage_bytes(), pins);
    assert!(heap.contains(target));
}

#[cfg_attr(test, test)]
fn finalization_storage_live_targets_need_no_fictitious_index_capacity() {
    let mut vm = bounded_vm();
    let (mut heap, _, target) = fixture(&vm);
    heap.root(target).unwrap();
    let _peer = leave_room(&mut vm, 0);
    heap.try_collect_major().unwrap();
    assert!(heap.contains(target));
    assert_eq!(heap.finalization_index_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn finalization_storage_full_existing_index_updates_without_growth() {
    let mut vm = bounded_vm();
    let (mut heap, _) = ready_heap(3);
    bind(&mut heap, &vm);
    let bytes = heap.finalization_index_bytes();
    assert!(bytes > 0);
    let _peer = leave_room(&mut vm, 0);
    heap.try_collect_major().unwrap();
    heap.try_collect_minor().unwrap();
    assert_eq!(heap.finalization_index_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn finalization_storage_partial_delivery_keeps_slots_until_final_release() {
    let mut vm = bounded_vm();
    let (mut heap, registries) = ready_heap(3);
    bind(&mut heap, &vm);
    let bytes = heap.finalization_index_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    heap.take_finalization_registry_cleanup_cell(registries[0])
        .unwrap()
        .unwrap();
    assert_eq!(heap.finalization_index_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    for registry in &registries[1..] {
        heap.take_finalization_registry_cleanup_cell(*registry)
            .unwrap()
            .unwrap();
    }
    assert_eq!(heap.finalization_index_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
}

#[cfg_attr(test, test)]
fn finalization_storage_unregister_refunds_the_final_ready_index_after_failure() {
    let mut vm = bounded_vm();
    let mut heap = Heap::default();
    let registry = heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    heap.root(registry).unwrap();
    let token = heap.alloc_object(None).unwrap();
    heap.root(token).unwrap();
    let target = heap.alloc_object(None).unwrap();
    heap.finalization_registry_register(
        registry,
        Value::Object(target),
        Value::Number(1.0),
        Some(Value::Object(token)),
    )
    .unwrap();
    heap.try_collect_major().unwrap();
    bind(&mut heap, &vm);
    let bytes = heap.finalization_index_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        heap.finalization_registry_unregister(registry, Value::Object(token)),
        Ok(true)
    );
    assert_eq!(heap.finalization_index_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
}

#[cfg_attr(test, test)]
fn finalization_storage_dead_registries_need_no_ready_index_allocation() {
    let mut vm = bounded_vm();
    let mut dead = Heap::default();
    let registry = dead
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    let root = dead.root(registry).unwrap();
    let target = dead.alloc_object(None).unwrap();
    let target_root = dead.root(target).unwrap();
    dead.finalization_registry_register(registry, Value::Object(target), Value::Null, None)
        .unwrap();
    dead.try_collect_major().unwrap();
    dead.unroot(root).unwrap();
    dead.unroot(target_root).unwrap();
    bind(&mut dead, &vm);
    let _peer = leave_room(&mut vm, 0);
    dead.try_collect_major().unwrap();
    assert_eq!(dead.stats().managed_bytes, 0);
    assert_eq!(dead.finalization_index_bytes(), 0);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn finalization_storage_rust_held_heap_retains_its_native_owners() {
    let vm = bounded_vm();
    let observed = observer(&vm);
    let (mut heap, _) = ready_heap(1);
    bind(&mut heap, &vm);
    let bytes = native_bytes(&heap);
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(heap);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn finalization_storage_late_binding_attaches_index_once() {
    let mut vm = bounded_vm();
    let (mut heap, _) = ready_heap(1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    bind(&mut heap, &vm);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + native_bytes(&heap)
    );
    let _peer = leave_room(&mut vm, 0);
    bind(&mut heap, &vm);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn finalization_storage_late_index_refusal_preserves_all_unbound_owners() {
    let mut vm = bounded_vm();
    let (mut heap, _) = ready_heap(1);
    let bytes = heap.finalization_index_bytes();
    let roots = heap.root_storage_state();
    let room = heap.root_storage_bytes() + heap.collector_storage_bytes();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget()),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
    assert_eq!(heap.root_storage_state(), roots);
    assert_eq!(heap.finalization_index_bytes(), bytes);
    assert_eq!(heap.pending_finalization_registries().len(), 1);
    assert_eq!(heap.collector_storage_bytes(), 0);
    let other = bounded_vm();
    bind(&mut heap, &other);
    assert_eq!(
        other.host_scheduling_snapshot().payload_bytes,
        other.heap.root_storage_bytes() + native_bytes(&heap)
    );
}

#[cfg_attr(test, test)]
fn finalization_storage_foreign_binding_preserves_the_original_index_owner() {
    let first = bounded_vm();
    let other = bounded_vm();
    let (mut heap, _) = ready_heap(1);
    bind(&mut heap, &first);
    let before = first.host_scheduling_snapshot().payload_bytes;
    let second_before = other.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        heap.bind_native_budget(other.host_scheduling.as_ref().unwrap().heap_native_budget()),
        Err(HeapError::InvalidConfig)
    );
    assert_eq!(first.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        other.host_scheduling_snapshot().payload_bytes,
        second_before
    );
}

#[cfg_attr(test, test)]
fn finalization_storage_registry_snapshot_retains_its_native_lease() {
    let vm = ready_vm();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = vm.prepare_finalization_registry_snapshot().unwrap();
    assert_eq!(snapshot.len(), 1);
    let bytes = snapshot.retained_bytes();
    assert!(bytes > 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn finalization_storage_registry_snapshot_outlives_its_vm() {
    let vm = ready_vm();
    let observed = observer(&vm);
    let snapshot = vm.prepare_finalization_registry_snapshot().unwrap();
    let bytes = snapshot.retained_bytes();
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(snapshot);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn finalization_storage_registry_snapshots_preserve_creation_order() {
    let mut vm = ready_vm();
    let mut expected = vec![vm.heap.pending_finalization_registries().next().unwrap()];
    for _ in 0..2 {
        let registry = vm
            .heap
            .alloc_finalization_registry(Value::Undefined, None)
            .unwrap();
        vm.heap.root(registry).unwrap();
        let target = vm.heap.alloc_object(None).unwrap();
        vm.heap
            .finalization_registry_register(registry, Value::Object(target), Value::Null, None)
            .unwrap();
        expected.push(registry);
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let snapshot = vm.prepare_finalization_registry_snapshot().unwrap();
    assert_eq!(&*snapshot, expected);
}

#[cfg_attr(test, test)]
fn finalization_storage_successful_publication_releases_only_snapshot_storage() {
    let mut vm = ready_vm();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let index = vm.heap.finalization_index_bytes();
    vm.enqueue_host_finalization_tasks().unwrap();
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.heap.finalization_index_bytes(), index);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn finalization_storage_empty_snapshot_needs_no_capacity_at_full_quota() {
    let mut vm = bounded_vm();
    let _peer = leave_room(&mut vm, 0);
    assert!(vm
        .prepare_finalization_registry_snapshot()
        .unwrap()
        .is_empty());
    vm.enqueue_host_finalization_tasks().unwrap();
    assert!(vm.host_finalization_tasks.is_empty());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn finalization_storage_standalone_transfer_preserves_cell_order_and_capacity() {
    let mut vm = Vm::default();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    vm.install_host_function("record", 1, move |args: &[HostValue]| {
        output.borrow_mut().push(args[0].clone());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("record").unwrap().unwrap();
    let registry = vm.heap.alloc_finalization_registry(callback, None).unwrap();
    vm.heap.root(registry).unwrap();
    let live = vm.heap.alloc_object(None).unwrap();
    vm.heap.root(live).unwrap();
    for value in 1..=3 {
        let target = if value == 2 {
            live
        } else {
            vm.heap.alloc_object(None).unwrap()
        };
        vm.heap
            .finalization_registry_register(
                registry,
                Value::Object(target),
                Value::Number(value as f64),
                None,
            )
            .unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let capacity = vm.heap.finalization_cell_storage_state(registry).1;
    vm.enqueue_finalization_cleanup_jobs().unwrap();
    assert_eq!(vm.promise_jobs.len(), 2);
    assert_eq!(
        vm.heap.finalization_cell_storage_state(registry),
        (1, capacity)
    );
    assert_eq!(vm.heap.finalization_index_bytes(), 0);
    vm.remaining_instructions = vm.config.instruction_budget;
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        *log.borrow(),
        [HostValue::Number(1.0), HostValue::Number(3.0)]
    );
}

#[cfg_attr(test, test)]
fn finalization_storage_transferred_object_holdings_remain_traced_in_jobs() {
    let mut vm = Vm::default();
    let registry = vm
        .heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    vm.heap.root(registry).unwrap();
    let target = vm.heap.alloc_object(None).unwrap();
    let holdings = vm.heap.alloc_object(None).unwrap();
    vm.heap
        .finalization_registry_register(
            registry,
            Value::Object(target),
            Value::Object(holdings),
            None,
        )
        .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.enqueue_finalization_cleanup_jobs().unwrap();
    assert_eq!(vm.promise_jobs.len(), 1);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(holdings));
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), (0, 0));
}

#[cfg_attr(test, test)]
fn finalization_storage_root_refusal_restores_the_document_queue_contribution() {
    let mut vm = ready_vm();
    let observed = observer(&vm);
    vm.heap.allow_root_registrations(0);
    let before = observed.host_scheduling_snapshot();
    assert!(matches!(
        vm.enqueue_host_finalization_tasks(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    ));
    assert!(vm.host_finalization_tasks.is_empty());
    assert_eq!(observed.host_scheduling_snapshot(), before);
}

#[cfg_attr(test, test)]
fn finalization_storage_reported_capacity_refusal_refunds_candidate_storage() {
    let mut vm = bounded_vm();
    let (mut heap, _, target) = fixture(&vm);
    let before = heap.stats();
    let room = std::mem::size_of::<(ObjectId, usize)>();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        heap.try_collect_major(),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert!(heap.contains(target));
    assert_eq!(heap.finalization_index_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}

#[cfg_attr(test, test)]
fn finalization_storage_complete_replacement_coexists_with_the_old_index() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let (mut heap, _) = ready_heap(3);
        bind(&mut heap, &vm);
        let old = heap.finalization_index_bytes();
        let registry = heap
            .alloc_finalization_registry(Value::Undefined, None)
            .unwrap();
        heap.root(registry).unwrap();
        let target = heap.alloc_object(None).unwrap();
        heap.finalization_registry_register(registry, Value::Object(target), Value::Null, None)
            .unwrap();
        let actual = HashMap::<ObjectId, usize>::with_capacity(6).capacity()
            * std::mem::size_of::<(ObjectId, usize)>();
        let room = if accepts { actual } else { actual - old };
        let before = heap.stats();
        let _peer = leave_room(&mut vm, room);
        let result = heap.try_collect_major();
        if accepts {
            assert_eq!(result, Ok(()));
            assert_eq!(heap.finalization_index_bytes(), actual);
            assert_eq!(heap.pending_finalization_registries().len(), 4);
            assert!(!heap.contains(target));
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES - old
            );
        } else {
            assert_eq!(result, Err(HeapError::CollectorStorageLimit));
            assert_eq!(heap.stats(), before);
            assert_eq!(heap.finalization_index_bytes(), old);
            assert_eq!(heap.pending_finalization_registries().len(), 3);
            assert!(heap.contains(target));
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES - room
            );
        }
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_finalization_storage_contracts() {
        finalization_storage_reported_capacity_refusal_refunds_candidate_storage();
        finalization_storage_complete_replacement_coexists_with_the_old_index();
        finalization_storage_gc_refusal_precedes_weak_clearing_and_sweeping();
        finalization_storage_ready_index_has_a_native_owner();
        finalization_storage_page_snapshot_refuses_before_task_publication();
        finalization_storage_minor_refusal_preserves_the_nursery_and_cells();
        finalization_storage_minor_collection_clears_only_young_targets();
        finalization_storage_refusal_preserves_released_foreign_pins();
        finalization_storage_live_targets_need_no_fictitious_index_capacity();
        finalization_storage_full_existing_index_updates_without_growth();
        finalization_storage_partial_delivery_keeps_slots_until_final_release();
        finalization_storage_unregister_refunds_the_final_ready_index_after_failure();
        finalization_storage_dead_registries_need_no_ready_index_allocation();
        finalization_storage_rust_held_heap_retains_its_native_owners();
        finalization_storage_late_binding_attaches_index_once();
        finalization_storage_late_index_refusal_preserves_all_unbound_owners();
        finalization_storage_foreign_binding_preserves_the_original_index_owner();
        finalization_storage_registry_snapshot_retains_its_native_lease();
        finalization_storage_registry_snapshot_outlives_its_vm();
        finalization_storage_registry_snapshots_preserve_creation_order();
        finalization_storage_successful_publication_releases_only_snapshot_storage();
        finalization_storage_empty_snapshot_needs_no_capacity_at_full_quota();
        finalization_storage_standalone_transfer_preserves_cell_order_and_capacity();
        finalization_storage_transferred_object_holdings_remain_traced_in_jobs();
        finalization_storage_root_refusal_restores_the_document_queue_contribution();
    }
}
