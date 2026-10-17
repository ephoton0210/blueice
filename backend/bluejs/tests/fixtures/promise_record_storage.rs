// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small rooted Promise tables and scalar native/managed allowances.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn fixture(count: usize) -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    for _ in 0..count {
        let promise = vm.new_promise().unwrap();
        vm.heap.root(promise).unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn candidate_bytes(vm: &Vm) -> usize {
    host_scheduling::map_slots(&HashMap::<ObjectId, PromiseRecord>::with_capacity(
        vm.promises.len() + 1,
    ))
}

#[cfg_attr(test, test)]
fn promise_record_storage_refuses_net_only_native_growth_before_heap_allocation() {
    let mut vm = fixture(3);
    assert_eq!(vm.promises.len(), vm.promises.capacity());
    let capacity = vm.promises.capacity();
    let before = vm.heap.stats();
    let bytes = candidate_bytes(&vm) - host_scheduling::map_slots(&vm.promises);
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.promises.len(), 3);
    assert_eq!(vm.promises.capacity(), capacity);
}

#[cfg_attr(test, test)]
fn promise_record_storage_managed_refusal_does_not_publish_native_capacity() {
    let mut vm = fixture(3);
    let capacity = vm.promises.capacity();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.new_promise(),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.promises.len(), 3);
    assert_eq!(vm.promises.capacity(), capacity);
}

#[cfg_attr(test, test)]
fn promise_record_storage_trace_refusal_keeps_old_records_and_capacity() {
    let mut vm = fixture(3);
    vm.extend_async_frame_roots([vm.object_prototype; 4].into_iter())
        .unwrap();
    vm.gc_trace_cache = Default::default();
    let before = vm.heap.stats();
    let capacity = vm.promises.capacity();
    let bytes = candidate_bytes(&vm);
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.promises.len(), 3);
    assert_eq!(vm.promises.capacity(), capacity);
    assert_eq!(vm.heap.stats(), before);
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn gc_fixture(count: usize, keep: usize) -> (Vm, Vec<ObjectId>) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    let mut objects = Vec::new();
    let mut roots = Vec::new();
    for _ in 0..count {
        let object = vm.new_promise().unwrap();
        roots.push(vm.heap.root(object).unwrap());
        objects.push(object);
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    for root in roots.into_iter().skip(keep) {
        vm.heap.unroot(root).unwrap();
    }
    vm.synchronize_scheduling_usage().unwrap();
    (vm, objects)
}

#[cfg_attr(test, test)]
fn promise_record_storage_exact_full_candidate_publishes_one_pending_record() {
    let mut vm = fixture(3);
    let old = host_scheduling::map_slots(&vm.promises);
    let bytes = candidate_bytes(&vm);
    let _peer = leave_room(&mut vm, bytes);
    let promise = vm.new_promise().unwrap();
    assert_eq!(vm.promises.len(), 4);
    assert_eq!(
        vm.promises.capacity() * std::mem::size_of::<(ObjectId, PromiseRecord)>(),
        bytes
    );
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert!(!vm.promises[&promise].handled);
    assert!(!vm.preserve_promise_table_capacity);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}
#[cfg_attr(test, test)]
fn promise_record_storage_reported_capacity_one_byte_short_refunds_the_candidate() {
    let mut vm = fixture(3);
    let observed = observer(&vm);
    let before = vm.heap.stats();
    let room = candidate_bytes(&vm) - 1;
    let _peer = leave_room(&mut vm, room);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.promises.capacity(), 3);
    assert_eq!(vm.promises.len(), 3);
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}
#[cfg_attr(test, test)]
fn promise_record_storage_spare_slot_is_usable_at_full_native_allowance() {
    let mut vm = fixture(2);
    let capacity = vm.promises.capacity();
    let _peer = leave_room(&mut vm, 0);
    vm.new_promise().unwrap();
    assert_eq!(vm.promises.len(), 3);
    assert_eq!(vm.promises.capacity(), capacity);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_record_storage_managed_refusal_refunds_native_bytes_for_sibling_realms() {
    let mut vm = fixture(3);
    let observed = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.heap.allow_only(0);
    assert!(matches!(
        vm.new_promise(),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.promises.capacity(), 3);
    assert!(!vm.preserve_promise_table_capacity);
}
#[cfg_attr(test, test)]
fn promise_record_storage_trace_refusal_restores_capacity_policy_and_shared_bytes() {
    let mut vm = fixture(3);
    vm.extend_async_frame_roots([vm.object_prototype; 4].into_iter())
        .unwrap();
    vm.gc_trace_cache = Default::default();
    let observed = observer(&vm);
    let room = candidate_bytes(&vm);
    let _peer = leave_room(&mut vm, room);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert!(!vm.preserve_promise_table_capacity);
    assert_eq!(vm.async_frame_roots, [vm.object_prototype; 4]);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}
#[cfg_attr(test, test)]
fn promise_record_storage_native_refusal_does_not_activate_a_creation_scope() {
    let mut vm = fixture(3);
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert!(!vm.preserve_promise_table_capacity);
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.promises.capacity(), 3);
}
#[cfg_attr(test, test)]
fn promise_record_storage_gc_preserves_previously_admitted_spare_slots() {
    let (mut vm, old) = gc_fixture(2, 0);
    let capacity = vm.promises.capacity();
    let collections = vm.heap.stats().major_collections;
    vm.heap.allow_only(0);
    let promise = vm.new_promise().unwrap();
    assert!(vm.heap.stats().major_collections > collections);
    assert!(old.iter().all(|object| !vm.heap.contains(*object)));
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(vm.promises.capacity(), capacity);
    assert!(vm.heap.contains(promise));
    assert!(!vm.preserve_promise_table_capacity);
}
#[cfg_attr(test, test)]
fn promise_record_storage_gc_freed_slot_discards_the_complete_candidate() {
    let (mut vm, old) = gc_fixture(3, 0);
    let capacity = vm.promises.capacity();
    let candidate = candidate_bytes(&vm);
    vm.heap.allow_only(0);
    vm.new_promise().unwrap();
    assert!(old.iter().all(|object| !vm.heap.contains(*object)));
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(vm.promises.capacity(), capacity);
    assert!(candidate > host_scheduling::map_slots(&vm.promises));
}
#[cfg_attr(test, test)]
fn promise_record_storage_normal_collection_reclaims_the_table_after_creation() {
    let (mut vm, _) = gc_fixture(2, 0);
    vm.heap.allow_only(0);
    let promise = vm.new_promise().unwrap();
    assert_eq!(vm.promises.len(), 1);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise));
    assert!(vm.promises.is_empty());
    assert_eq!(vm.promises.capacity(), 0);
    assert!(!vm.preserve_promise_table_capacity);
}
#[cfg_attr(test, test)]
fn promise_record_storage_live_result_edges_survive_gc_and_committed_replacement() {
    let mut vm = fixture(3);
    let mut promises: Vec<_> = vm.promises.keys().copied().collect();
    promises.sort_unstable_by_key(|object| object.serial);
    let held = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.settle_promise(promises[0], PromiseStatus::Fulfilled(Value::Object(held)))
        .unwrap();
    let garbage = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let collections = vm.heap.stats().major_collections;
    vm.heap.allow_only(0);
    let promise = vm.new_promise().unwrap();
    assert!(vm.heap.stats().major_collections > collections);
    assert!(!vm.heap.contains(garbage));
    assert!(vm.heap.contains(held));
    assert!(
        matches!(vm.promises[&promises[0]].status, PromiseStatus::Fulfilled(Value::Object(object)) if object == held)
    );
    assert!(vm.promises.contains_key(&promise));
    assert!(promises
        .iter()
        .all(|object| vm.promises.contains_key(object)));
    assert_eq!(vm.promises.len(), 4);
    assert_eq!(vm.promises.capacity(), 7);
}
#[cfg_attr(test, test)]
fn promise_record_storage_refusal_preserves_existing_status_and_reactions() {
    let mut vm = fixture(3);
    let mut promises: Vec<_> = vm.promises.keys().copied().collect();
    promises.sort_unstable_by_key(|object| object.serial);
    vm.enqueue_promise_reaction(
        promises[0],
        PromiseReaction::Then(PromiseThenReaction {
            target: ReactionTarget::Native(promises[2]),
            on_fulfilled: Value::Undefined,
            on_rejected: Value::Undefined,
        }),
    );
    vm.settle_promise(
        promises[1],
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    let bytes = candidate_bytes(&vm) - 1;
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.promises[&promises[0]].reactions.len(), 1);
    assert!(
        matches!(&vm.promises[&promises[1]].status, PromiseStatus::Fulfilled(value) if *value == Value::String("tiny".into()))
    );
    assert_eq!(vm.promises.len(), 3);
}
#[cfg_attr(test, test)]
fn promise_record_storage_vm_destruction_refunds_committed_creation() {
    let mut vm = fixture(3);
    let observed = observer(&vm);
    vm.new_promise().unwrap();
    assert!(observed.host_scheduling_snapshot().payload_bytes > 0);
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_record_storage_failed_document_does_not_poison_another_document() {
    let mut left = fixture(3);
    let mut right = fixture(3);
    let _peer = leave_room(&mut left, 0);
    assert_eq!(left.new_promise(), Err(RuntimeError::SchedulingLimit));
    let promise = right.new_promise().unwrap();
    assert!(right.promises.contains_key(&promise));
    assert!(right.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_record_storage_host_neutral_capacity_joins_document_policy_before_growth() {
    let mut vm = Vm::default();
    for _ in 0..3 {
        let promise = vm.new_promise().unwrap();
        vm.heap.root(promise).unwrap();
    }
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.promises.len(), 3);
    assert_eq!(vm.promises.capacity(), 3);
    assert_eq!(vm.heap.stats(), before);
}
#[cfg_attr(test, test)]
fn promise_record_storage_gc_keeps_spare_slots_at_full_native_allowance_then_reclaims_them() {
    let (mut vm, old) = gc_fixture(2, 0);
    let bytes = host_scheduling::map_slots(&vm.promises);
    vm.heap.allow_only(0);
    let _peer = leave_room(&mut vm, 0);
    let promise = vm.new_promise().unwrap();
    assert!(old.iter().all(|object| !vm.heap.contains(*object)));
    assert!(vm.heap.contains(promise));
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.promises.is_empty());
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
}
#[cfg_attr(test, test)]
fn promise_record_storage_empty_managed_refusal_refunds_and_permits_a_finite_retry() {
    let mut vm = fixture(0);
    let observed = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.heap.allow_only(0);
    assert!(matches!(
        vm.new_promise(),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(vm.promises.len(), 0);
    assert_eq!(vm.promises.capacity(), 0);
    assert!(!vm.preserve_promise_table_capacity);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, before);
    vm.heap.allow_only(1024);
    let promise = vm.new_promise().unwrap();
    assert!(vm.promises.contains_key(&promise));
    assert_eq!(vm.promises.len(), 1);
}
#[cfg_attr(test, test)]
fn promise_record_storage_gc_prunes_dead_records_but_preserves_a_live_owner() {
    let (mut vm, old) = gc_fixture(3, 1);
    vm.settle_promise(
        old[0],
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    vm.heap.allow_only(0);
    let promise = vm.new_promise().unwrap();
    assert!(vm.heap.contains(old[0]));
    assert!(!vm.heap.contains(old[1]));
    assert!(!vm.heap.contains(old[2]));
    assert!(vm.promises.contains_key(&promise));
    assert_eq!(vm.promises.len(), 2);
    assert!(
        matches!(&vm.promises[&old[0]].status, PromiseStatus::Fulfilled(value) if *value == Value::String("tiny".into()))
    );
    assert_eq!(vm.promises.capacity(), 3);
}
#[cfg_attr(test, test)]
fn promise_record_storage_custom_prototype_survives_collection_during_creation() {
    let mut vm = fixture(2);
    let prototype = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let garbage = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.allow_only(0);
    let promise = vm.allocate_promise_record(prototype).unwrap();
    assert!(!vm.heap.contains(garbage));
    assert!(vm.heap.contains(prototype));
    assert_eq!(vm.heap.prototype(promise), Ok(Some(prototype)));
    assert!(vm.promises.contains_key(&promise));
    assert!(!vm.preserve_promise_table_capacity);
}

#[cfg_attr(test, test)]
fn promise_record_storage_empty_cached_trace_refusal_preserves_existing_capacity() {
    let mut vm = fixture(0);
    vm.promises.reserve(1);
    vm.extend_async_frame_roots([vm.object_prototype; 4].into_iter())
        .unwrap();
    vm.gc_trace_cache = Default::default();
    let capacity = vm.promises.capacity();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.promises.capacity(), capacity);
    assert!(!vm.preserve_promise_table_capacity);
}

#[cfg_attr(test, test)]
fn promise_record_storage_empty_cache_is_reclaimed_after_a_real_collection_refusal() {
    let mut vm = fixture(0);
    vm.promises.reserve(1);
    let bytes = host_scheduling::map_slots(&vm.promises);
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let collections = vm.heap.stats().major_collections;
    vm.heap.allow_only(0);
    assert!(matches!(
        vm.new_promise(),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert!(vm.heap.stats().major_collections > collections);
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used - bytes);
    assert!(!vm.preserve_promise_table_capacity);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_record_storage_contracts() {
        promise_record_storage_empty_cached_trace_refusal_preserves_existing_capacity();
        promise_record_storage_empty_cache_is_reclaimed_after_a_real_collection_refusal();
        promise_record_storage_refuses_net_only_native_growth_before_heap_allocation();
        promise_record_storage_managed_refusal_does_not_publish_native_capacity();
        promise_record_storage_trace_refusal_keeps_old_records_and_capacity();
        promise_record_storage_exact_full_candidate_publishes_one_pending_record();
        promise_record_storage_reported_capacity_one_byte_short_refunds_the_candidate();
        promise_record_storage_spare_slot_is_usable_at_full_native_allowance();
        promise_record_storage_managed_refusal_refunds_native_bytes_for_sibling_realms();
        promise_record_storage_trace_refusal_restores_capacity_policy_and_shared_bytes();
        promise_record_storage_native_refusal_does_not_activate_a_creation_scope();
        promise_record_storage_gc_preserves_previously_admitted_spare_slots();
        promise_record_storage_gc_freed_slot_discards_the_complete_candidate();
        promise_record_storage_normal_collection_reclaims_the_table_after_creation();
        promise_record_storage_live_result_edges_survive_gc_and_committed_replacement();
        promise_record_storage_refusal_preserves_existing_status_and_reactions();
        promise_record_storage_vm_destruction_refunds_committed_creation();
        promise_record_storage_failed_document_does_not_poison_another_document();
        promise_record_storage_host_neutral_capacity_joins_document_policy_before_growth();
        promise_record_storage_gc_keeps_spare_slots_at_full_native_allowance_then_reclaims_them();
        promise_record_storage_empty_managed_refusal_refunds_and_permits_a_finite_retry();
        promise_record_storage_gc_prunes_dead_records_but_preserves_a_live_owner();
        promise_record_storage_custom_prototype_survives_collection_during_creation();
    }
}
