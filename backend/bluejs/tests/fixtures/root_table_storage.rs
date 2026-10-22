// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed small root tables and numeric native allowances verify admission.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn fixture() -> (Vm, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    // Bootstrap roots occupy two of the initial three hash slots.
    vm.heap.root(object).unwrap();
    vm.with_roots(|_| Ok(())).unwrap();
    (vm, object)
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
fn root_table_storage_refusal_precedes_registration_and_preserves_roots() {
    let (mut vm, object) = fixture();
    let before = vm.heap.stats();
    let state = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.with_roots(|heap| heap.root(object)).map(|_| ()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.heap.root_storage_state(), state);
    assert!(vm.heap.contains(object));
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn bounded_heap(vm: &Vm) -> Heap {
    let mut heap = Heap::default();
    heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget())
        .unwrap();
    heap
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    }
}

#[cfg_attr(test, test)]
fn root_table_storage_bootstrap_capacity_attaches_once() {
    let mut vm = Vm::default();
    let bytes = vm.heap.root_storage_bytes();
    assert!(bytes > 0);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, bytes);
    let _peer = leave_room(&mut vm, 0);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_joint_replacement_refuses_a_net_growth_only_allowance() {
    let (mut vm, object) = fixture();
    let old = vm.heap.root_storage_bytes();
    let mut probe = Heap::default();
    let id = probe.alloc_object(None).unwrap();
    for _ in 0..4 {
        probe.root(id).unwrap();
    }
    let room = probe.root_storage_bytes() - old;
    let _peer = leave_room(&mut vm, room);
    let state = vm.heap.root_storage_state();
    assert_eq!(vm.heap.root(object), Err(HeapError::RootStorageLimit));
    assert_eq!(vm.heap.root_storage_state(), state);
    assert_eq!(vm.heap.root_storage_bytes(), old);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_reported_capacity_refusal_refunds_candidate_storage() {
    let mut vm = bounded_vm();
    let mut heap = bounded_heap(&vm);
    let id = heap.alloc_object(None).unwrap();
    let room = std::mem::size_of::<(RootId, ObjectId)>();
    let _peer = leave_room(&mut vm, room);
    let before = heap.root_storage_state();
    assert_eq!(heap.root(id), Err(HeapError::RootStorageLimit));
    assert_eq!(heap.root_storage_state(), before);
    assert_eq!(heap.root_storage_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_exact_full_replacement_allowance_refunds_the_old_allocation() {
    let (mut vm, object) = fixture();
    let old = vm.heap.root_storage_bytes();
    let mut probe = Heap::default();
    let id = probe.alloc_object(None).unwrap();
    for _ in 0..4 {
        probe.root(id).unwrap();
    }
    let bytes = probe.root_storage_bytes();
    let _peer = leave_room(&mut vm, bytes);
    vm.heap.root(object).unwrap();
    assert_eq!(vm.heap.root_storage_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_spare_slot_registers_without_native_growth_at_full_quota() {
    let mut vm = bounded_vm();
    let id = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let bytes = vm.heap.root_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.heap.root(id).unwrap();
    assert_eq!(vm.heap.root_storage_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn root_table_storage_last_removal_releases_capacity_after_terminal_failure() {
    let mut vm = bounded_vm();
    let mut heap = bounded_heap(&vm);
    let id = heap.alloc_object(None).unwrap();
    let root = heap.root(id).unwrap();
    let bytes = heap.root_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    heap.unroot(root).unwrap();
    assert_eq!(heap.root_storage_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_partial_removal_keeps_the_allocation_owned() {
    let vm = bounded_vm();
    let mut heap = bounded_heap(&vm);
    let id = heap.alloc_object(None).unwrap();
    let mut roots = Vec::new();
    for _ in 0..16 {
        roots.push(heap.root(id).unwrap());
    }
    let bytes = heap.root_storage_bytes();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    for root in roots.drain(..15) {
        heap.unroot(root).unwrap();
    }
    assert_eq!(heap.root_storage_bytes(), bytes);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    heap.unroot(roots.pop().unwrap()).unwrap();
    assert_eq!(heap.root_storage_bytes(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
}

#[cfg_attr(test, test)]
fn root_table_storage_heap_owner_can_outlive_the_original_vm() {
    let vm = bounded_vm();
    let observed = observer(&vm);
    let mut heap = bounded_heap(&vm);
    let id = heap.alloc_object(None).unwrap();
    heap.root(id).unwrap();
    let bytes = heap.root_storage_bytes();
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(heap);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn root_table_storage_joint_activation_collector_refusal_preserves_roots_and_refunds_staging() {
    let mut vm = bounded_vm();
    let mut heap = Heap::default();
    let id = heap.alloc_object(None).unwrap();
    let root = heap.root(id).unwrap();
    heap.try_collect_major().unwrap();
    let bytes = heap.root_storage_bytes();
    let state = heap.root_storage_state();
    let before = heap.stats();
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(
        heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget()),
        Err(HeapError::CollectorStorageLimit)
    );
    assert_eq!(heap.root_storage_state(), state);
    assert_eq!(heap.stats(), before);
    assert_eq!(heap.collector_storage_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
    assert_eq!(heap.unroot(root), Ok(id));
}

#[cfg_attr(test, test)]
fn root_table_storage_joint_activation_root_refusal_preserves_live_registrations() {
    let mut vm = bounded_vm();
    let mut heap = Heap::default();
    let id = heap.alloc_object(None).unwrap();
    let root = heap.root(id).unwrap();
    heap.try_collect_major().unwrap();
    let before = heap.stats();
    let bytes = heap.root_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget()),
        Err(HeapError::RootStorageLimit)
    );
    assert_eq!(heap.stats(), before);
    assert_eq!(heap.root_storage_bytes(), bytes);
    assert_eq!(heap.unroot(root), Ok(id));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_late_activation_attaches_root_and_collector_caches_once() {
    let mut vm = Vm::default();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let bytes = vm.heap.root_storage_bytes() + vm.heap.collector_storage_bytes();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, bytes);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, bytes);
}

#[cfg_attr(test, test)]
fn root_table_storage_foreign_binding_keeps_both_original_native_owners() {
    let first = bounded_vm();
    let second = bounded_vm();
    let mut heap = bounded_heap(&first);
    let id = heap.alloc_object(None).unwrap();
    heap.root(id).unwrap();
    heap.try_collect_major().unwrap();
    let before = first.host_scheduling_snapshot().payload_bytes;
    let other = second.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        heap.bind_native_budget(
            second
                .host_scheduling
                .as_ref()
                .unwrap()
                .heap_native_budget()
        ),
        Err(HeapError::InvalidConfig)
    );
    assert_eq!(first.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(second.host_scheduling_snapshot().payload_bytes, other);
}

#[cfg_attr(test, test)]
fn root_table_storage_released_realm_pins_refund_the_empty_table() {
    let mut vm = bounded_vm();
    let mut heap = bounded_heap(&vm);
    let id = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(id).unwrap();
    let bytes = heap.root_storage_bytes() + heap.realm_pin_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    drop(pin);
    heap.release_realm_roots();
    assert_eq!(heap.root_storage_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_minor_and_major_collection_observe_registered_roots() {
    let vm = bounded_vm();
    let mut heap = bounded_heap(&vm);
    let live = heap.alloc_object(None).unwrap();
    let root = heap.root(live).unwrap();
    let dead = heap.alloc_object(None).unwrap();
    heap.try_collect_major().unwrap();
    assert!(heap.contains(live));
    assert!(!heap.contains(dead));
    let young = heap.alloc_object(None).unwrap();
    let young_root = heap.root(young).unwrap();
    heap.try_collect_minor().unwrap();
    assert!(heap.contains(young));
    assert_eq!(heap.stats().nursery_objects, 0);
    heap.unroot(root).unwrap();
    heap.unroot(young_root).unwrap();
    heap.try_collect_major().unwrap();
    assert!(!heap.contains(live));
    assert!(!heap.contains(young));
}

#[cfg_attr(test, test)]
fn root_table_storage_foreign_objects_refuse_before_native_admission() {
    let (mut vm, _) = fixture();
    let mut other = Heap::default();
    let foreign = other.alloc_object(None).unwrap();
    let state = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.heap.root(foreign),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(vm.heap.root_storage_state(), state);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn root_table_storage_invalid_unroot_does_not_refund_live_capacity() {
    let (vm, _) = fixture();
    let mut other = Heap::default();
    let object = other.alloc_object(None).unwrap();
    let root = other.root(object).unwrap();
    let mut vm = vm;
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(vm.heap.unroot(root), Err(HeapError::InvalidRoot(root)));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn root_table_storage_released_root_id_is_never_reused() {
    let vm = bounded_vm();
    let mut heap = bounded_heap(&vm);
    let id = heap.alloc_object(None).unwrap();
    let first = heap.root(id).unwrap();
    heap.unroot(first).unwrap();
    let second = heap.root(id).unwrap();
    assert_ne!(first, second);
    assert_eq!(heap.unroot(first), Err(HeapError::InvalidRoot(first)));
    assert_eq!(heap.unroot(second), Ok(id));
}

#[cfg_attr(test, test)]
fn root_table_storage_terminal_cleanup_can_reuse_an_existing_slot() {
    let mut vm = bounded_vm();
    let id = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let bytes = vm.heap.root_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    let root = vm.heap.root(id).unwrap();
    assert_eq!(vm.heap.root_storage_bytes(), bytes);
    vm.heap.unroot(root).unwrap();
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

fn rejected_fixture() -> (Vm, ObjectId) {
    let mut vm = bounded_vm();
    let promise = vm.new_promise().unwrap();
    vm.promises.get_mut(&promise).unwrap().status = PromiseStatus::Rejected(Value::Undefined);
    vm.host_rejections = Some(host_rejections::HostRejections::default());
    vm.host_rejections.as_mut().unwrap().pending.reserve(4);
    let (len, capacity, _) = vm.heap.root_insertion_state();
    assert!(capacity - len < 32);
    for _ in len..capacity {
        vm.heap.root(promise).unwrap();
    }
    vm.synchronize_scheduling_usage().unwrap();
    (vm, promise)
}

#[cfg_attr(test, test)]
fn root_table_storage_rejection_producer_latches_growth_refusal_without_publishing() {
    let (mut vm, promise) = rejected_fixture();
    let before = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    vm.track_host_rejection(promise);
    assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    assert_eq!(vm.heap.root_storage_state(), before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_rejection_producer_handles_identifier_refusal_without_panicking() {
    let (mut vm, promise) = rejected_fixture();
    vm.heap.allow_root_registrations(0);
    let before = vm.heap.root_storage_state();
    vm.track_host_rejection(promise);
    assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    assert_eq!(vm.heap.root_storage_state(), before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn root_table_storage_retained_capacity_has_a_native_owner() {
    let (mut vm, object) = fixture();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let storage_before = vm.heap.root_storage_bytes();
    vm.heap.root(object).unwrap();
    assert!(
        vm.host_scheduling_snapshot().payload_bytes > before,
        "root table growth keeps native ownership"
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        vm.heap.root_storage_bytes() - storage_before
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_root_table_storage_contracts() {
        root_table_storage_refusal_precedes_registration_and_preserves_roots();
        root_table_storage_bootstrap_capacity_attaches_once();
        root_table_storage_joint_replacement_refuses_a_net_growth_only_allowance();
        root_table_storage_reported_capacity_refusal_refunds_candidate_storage();
        root_table_storage_exact_full_replacement_allowance_refunds_the_old_allocation();
        root_table_storage_spare_slot_registers_without_native_growth_at_full_quota();
        root_table_storage_last_removal_releases_capacity_after_terminal_failure();
        root_table_storage_partial_removal_keeps_the_allocation_owned();
        root_table_storage_heap_owner_can_outlive_the_original_vm();
        root_table_storage_joint_activation_collector_refusal_preserves_roots_and_refunds_staging();
        root_table_storage_joint_activation_root_refusal_preserves_live_registrations();
        root_table_storage_late_activation_attaches_root_and_collector_caches_once();
        root_table_storage_foreign_binding_keeps_both_original_native_owners();
        root_table_storage_released_realm_pins_refund_the_empty_table();
        root_table_storage_minor_and_major_collection_observe_registered_roots();
        root_table_storage_foreign_objects_refuse_before_native_admission();
        root_table_storage_invalid_unroot_does_not_refund_live_capacity();
        root_table_storage_released_root_id_is_never_reused();
        root_table_storage_terminal_cleanup_can_reuse_an_existing_slot();
        root_table_storage_rejection_producer_latches_growth_refusal_without_publishing();
        root_table_storage_rejection_producer_handles_identifier_refusal_without_panicking();
        root_table_storage_retained_capacity_has_a_native_owner();
    }
}
