// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny branded tables verify pre-allocation admission and guarded GC storage.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.disposable_stack_prototype(false).unwrap();
    vm.disposable_stack_prototype(true).unwrap();
    vm.remaining_instructions = 100_000;
    vm.stack.reserve(16);
    vm.next_host_native_gc_bytes = usize::MAX;
    vm.next_host_heap_gc_bytes = usize::MAX;
    vm
}
type DisposalMap =
    crate::vm::retained_control_map::RetainedControlMap<ObjectId, DisposeCapabilityState>;
fn table(vm: &Vm, is_async: bool) -> &DisposalMap {
    if is_async {
        &vm.async_disposable_stacks
    } else {
        &vm.disposable_stacks
    }
}
fn table_mut(vm: &mut Vm, is_async: bool) -> &mut DisposalMap {
    if is_async {
        &mut vm.async_disposable_stacks
    } else {
        &mut vm.disposable_stacks
    }
}
fn states(vm: &mut Vm, is_async: bool, count: usize) -> Vec<ObjectId> {
    let prototype = vm.disposable_stack_prototype(is_async).unwrap();
    let map = if is_async {
        &mut vm.async_disposable_stacks
    } else {
        &mut vm.disposable_stacks
    };
    map.reserve(count);
    (0..count)
        .map(|_| {
            let id = vm.heap.alloc_object(Some(prototype)).unwrap();
            map.insert(id, DisposeCapabilityState::default());
            id
        })
        .collect()
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn candidate_bytes(count: usize) -> usize {
    host_scheduling::map_slots(&HashMap::<ObjectId, DisposeCapabilityState>::with_capacity(
        count,
    ))
}
fn guarded_gc(is_async: bool) {
    let mut vm = page();
    let ids = states(&mut vm, is_async, 7);
    let root = vm.heap.root(ids[0]).unwrap();
    vm.preserve_promise_table_capacity = true;
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(table(&vm, is_async).len(), 1);
    assert_eq!(table(&vm, is_async).capacity(), 7);
    vm.preserve_promise_table_capacity = false;
    vm.heap.unroot(root).unwrap();
}
#[cfg_attr(test, test)]
fn disposal_map_storage_sync_gc_preserves_staged_capacity() {
    guarded_gc(false);
}
#[cfg_attr(test, test)]
fn disposal_map_storage_async_gc_preserves_staged_capacity() {
    guarded_gc(true);
}
#[cfg_attr(test, test)]
fn disposal_map_storage_constructor_refuses_complete_candidate_before_managed_allocation() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 3);
        vm.new_target = vm
            .global(if is_async {
                "AsyncDisposableStack"
            } else {
                "DisposableStack"
            })
            .unwrap();
        vm.with_roots(|_| Ok(())).unwrap();
        let old = host_scheduling::map_slots(table(&vm, is_async));
        let _peer = leave_room(&mut vm, candidate_bytes(4) - old);
        let heap = vm.heap.stats();
        assert_eq!(
            vm.disposable_stack_constructor(is_async, true),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(vm.heap.stats(), heap);
        assert_eq!(table(&vm, is_async).len(), 3);
        assert!(ids.iter().all(|id| table(&vm, is_async).contains_key(id)));
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_move_refuses_complete_candidate_before_source_mutation() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 3);
        vm.with_roots(|_| Ok(())).unwrap();
        let old = host_scheduling::map_slots(table(&vm, is_async));
        let _peer = leave_room(&mut vm, candidate_bytes(4) - old);
        let heap = vm.heap.stats();
        assert_eq!(
            vm.disposable_stack_move(&Value::Object(ids[0]), is_async),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(vm.heap.stats(), heap);
        assert!(!table(&vm, is_async)[&ids[0]].disposed);
        assert_eq!(table(&vm, is_async).len(), 3);
    }
}

fn sparse(is_async: bool) -> (Vm, ObjectId) {
    let mut vm = page();
    table_mut(&mut vm, is_async).reserve(7);
    let id = states(&mut vm, is_async, 1)[0];
    (vm, id)
}
fn new_target(vm: &mut Vm, is_async: bool) {
    vm.new_target = vm
        .global(if is_async {
            "AsyncDisposableStack"
        } else {
            "DisposableStack"
        })
        .unwrap();
}
fn warm_move(vm: &mut Vm, id: ObjectId) {
    vm.extend_async_frame_roots(std::iter::once(id)).unwrap();
    vm.with_roots(|_| Ok(())).unwrap();
    vm.async_frame_roots.clear();
}
fn payload(vm: &mut Vm, id: ObjectId, is_async: bool, value: Value) {
    let resources = vm.owned_records(vec![DisposableResource {
        receiver: Value::Undefined,
        argument: Some(value),
        method: None,
        hint: if is_async {
            DisposeHint::Async
        } else {
            DisposeHint::Sync
        },
        sync_fallback: false,
    }]);
    table_mut(vm, is_async).get_mut(&id).unwrap().resources = resources;
}
fn roots(vm: &mut Vm, ids: &[ObjectId]) -> Vec<RootId> {
    ids.iter().map(|id| vm.heap.root(*id).unwrap()).collect()
}
fn prime(vm: &mut Vm) {
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
}

#[cfg_attr(test, test)]
fn disposal_map_storage_deletion_refunds_payload_but_retains_table_allocation() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 7);
        let before_payload = vm.displaced_execution_control_bytes.get();
        payload(&mut vm, ids[0], is_async, Value::String("tiny".into()));
        let bytes = vm.displaced_execution_control_bytes.get() - before_payload;
        let backing = table(&vm, is_async).retained_bytes();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        table_mut(&mut vm, is_async).remove(&ids[0]);
        assert_eq!(table(&vm, is_async).retained_bytes(), backing);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            before - bytes
        );
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_optional_compaction_without_headroom_is_nonterminal() {
    for is_async in [false, true] {
        let (mut vm, id) = sparse(is_async);
        let _peer = leave_room(&mut vm, 0);
        let before = vm.host_scheduling_snapshot().payload_bytes;
        vm.reclaim_scheduling_maps();
        assert_eq!(table(&vm, is_async).capacity(), 7);
        assert!(table(&vm, is_async).contains_key(&id));
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_short_reported_compaction_capacity_refunds_staging() {
    for is_async in [false, true] {
        let (mut vm, _) = sparse(is_async);
        let _peer = leave_room(&mut vm, candidate_bytes(1) - 1);
        let before = vm.host_scheduling_snapshot().payload_bytes;
        vm.reclaim_scheduling_maps();
        assert_eq!(table(&vm, is_async).capacity(), 7);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_exact_compaction_reconciles_before_candidate_refund() {
    for is_async in [false, true] {
        let (mut vm, id) = sparse(is_async);
        let old = table(&vm, is_async).retained_bytes();
        let _peer = leave_room(&mut vm, candidate_bytes(1));
        vm.reclaim_scheduling_maps();
        assert_eq!(table(&vm, is_async).retained_bytes(), candidate_bytes(1));
        assert!(table(&vm, is_async).contains_key(&id));
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - old
        );
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_optional_refusal_can_retry_after_peer_release() {
    for is_async in [false, true] {
        let (mut vm, _) = sparse(is_async);
        let peer = leave_room(&mut vm, 0);
        vm.reclaim_scheduling_maps();
        assert_eq!(table(&vm, is_async).capacity(), 7);
        drop(peer);
        vm.reclaim_scheduling_maps();
        assert_eq!(table(&vm, is_async).capacity(), 3);
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_empty_backing_releases_after_terminal_failure() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 7);
        for id in ids {
            table_mut(&mut vm, is_async).remove(&id);
        }
        let old = table(&vm, is_async).retained_bytes();
        vm.host_scheduling.as_ref().unwrap().fail();
        vm.reconcile_scheduling_ownership();
        let before = vm.host_scheduling_snapshot().payload_bytes;
        vm.reclaim_scheduling_maps();
        assert_eq!(table(&vm, is_async).retained_bytes(), 0);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - old);
        assert_eq!(
            vm.check_scheduling_failure(),
            Err(RuntimeError::SchedulingLimit)
        );
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_both_brands_reclaim_with_sequential_owner_refunds() {
    let (mut vm, _) = sparse(false);
    vm.async_disposable_stacks.reserve(7);
    states(&mut vm, true, 1);
    let old = vm.disposable_stacks.retained_bytes();
    let small = candidate_bytes(1);
    let _peer = leave_room(&mut vm, small);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.disposable_stacks.retained_bytes(), small);
    assert_eq!(vm.async_disposable_stacks.retained_bytes(), small);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - small - 2 * (old - small)
    );
}
#[cfg_attr(test, test)]
fn disposal_map_storage_constructor_exact_candidate_budget_transfers_once() {
    for is_async in [false, true] {
        let mut vm = page();
        states(&mut vm, is_async, 3);
        new_target(&mut vm, is_async);
        vm.with_roots(|_| Ok(())).unwrap();
        let old = table(&vm, is_async).retained_bytes();
        let _peer = leave_room(&mut vm, candidate_bytes(4));
        let value = vm.disposable_stack_constructor(is_async, true).unwrap();
        assert!(table(&vm, is_async).contains_key(&value.object_id().unwrap()));
        assert_eq!(table(&vm, is_async).retained_bytes(), candidate_bytes(4));
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - old
        );
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_move_exact_candidate_budget_preserves_resource_buffer() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 3);
        payload(&mut vm, ids[0], is_async, Value::String("tiny".into()));
        warm_move(&mut vm, ids[0]);
        let pointer = table(&vm, is_async)[&ids[0]].resources.as_ptr();
        let old = table(&vm, is_async).retained_bytes();
        let _peer = leave_room(&mut vm, candidate_bytes(4));
        let moved = vm
            .disposable_stack_move(&Value::Object(ids[0]), is_async)
            .unwrap()
            .object_id()
            .unwrap();
        assert!(table(&vm, is_async)[&ids[0]].disposed);
        assert!(table(&vm, is_async)[&ids[0]].resources.is_empty());
        assert!(!table(&vm, is_async)[&moved].disposed);
        assert_eq!(table(&vm, is_async)[&moved].resources.as_ptr(), pointer);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - old
        );
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_spare_constructor_slot_requires_no_fictitious_charge() {
    for is_async in [false, true] {
        let mut vm = page();
        states(&mut vm, is_async, 1);
        new_target(&mut vm, is_async);
        vm.with_roots(|_| Ok(())).unwrap();
        let _peer = leave_room(&mut vm, 0);
        assert!(vm.disposable_stack_constructor(is_async, true).is_ok());
        assert_eq!(table(&vm, is_async).len(), 2);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES
        );
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_spare_move_slot_reuses_admitted_roots_and_payload() {
    for is_async in [false, true] {
        let mut vm = page();
        let id = states(&mut vm, is_async, 1)[0];
        payload(&mut vm, id, is_async, Value::String("tiny".into()));
        warm_move(&mut vm, id);
        let _peer = leave_room(&mut vm, 0);
        assert!(vm
            .disposable_stack_move(&Value::Object(id), is_async)
            .is_ok());
        assert_eq!(table(&vm, is_async).len(), 2);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES
        );
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_root_preparation_refusal_preserves_source_and_table() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 3);
        payload(&mut vm, ids[0], is_async, Value::String("tiny".into()));
        assert_eq!(vm.async_frame_roots.capacity(), 0);
        vm.with_roots(|_| Ok(())).unwrap();
        let _peer = leave_room(&mut vm, candidate_bytes(4));
        let heap = vm.heap.stats();
        assert_eq!(
            vm.disposable_stack_move(&Value::Object(ids[0]), is_async),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(vm.heap.stats(), heap);
        assert!(!table(&vm, is_async)[&ids[0]].disposed);
        assert_eq!(table(&vm, is_async)[&ids[0]].resources.len(), 1);
        assert_eq!(table(&vm, is_async).len(), 3);
        assert_eq!(vm.async_frame_roots.capacity(), 0);
        assert!(!vm.preserve_promise_table_capacity);
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_constructor_heap_refusal_publishes_no_record() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 3);
        let _roots = roots(&mut vm, &ids);
        new_target(&mut vm, is_async);
        prime(&mut vm);
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.disposable_stack_constructor(is_async, true),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert_eq!(table(&vm, is_async).len(), 3);
        assert_eq!(table(&vm, is_async).retained_bytes(), candidate_bytes(3));
        assert!(!vm.preserve_promise_table_capacity);
        assert!(vm.async_frame_roots.is_empty());
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_move_heap_refusal_keeps_resources_and_disposed_state() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 3);
        let _roots = roots(&mut vm, &ids);
        payload(&mut vm, ids[0], is_async, Value::String("tiny".into()));
        warm_move(&mut vm, ids[0]);
        prime(&mut vm);
        let pointer = table(&vm, is_async)[&ids[0]].resources.as_ptr();
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.disposable_stack_move(&Value::Object(ids[0]), is_async),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert_eq!(table(&vm, is_async).len(), 3);
        assert!(!table(&vm, is_async)[&ids[0]].disposed);
        assert_eq!(table(&vm, is_async)[&ids[0]].resources.as_ptr(), pointer);
        assert!(vm.async_frame_roots.is_empty());
        assert!(!vm.preserve_promise_table_capacity);
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_constructor_gc_reuses_slots_freed_from_full_backing() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 7);
        let roots = roots(&mut vm, &ids);
        new_target(&mut vm, is_async);
        prime(&mut vm);
        for root in roots.iter().skip(1) {
            vm.heap.unroot(*root).unwrap();
        }
        let before = vm.heap.stats().major_collections;
        vm.heap.allow_only(0);
        let value = vm.disposable_stack_constructor(is_async, true).unwrap();
        assert!(vm.heap.stats().major_collections > before);
        assert_eq!(table(&vm, is_async).len(), 2);
        assert_eq!(table(&vm, is_async).capacity(), 7);
        assert!(table(&vm, is_async).contains_key(&ids[0]));
        assert!(table(&vm, is_async).contains_key(&value.object_id().unwrap()));
        assert!(!vm.preserve_promise_table_capacity);
        vm.heap.unroot(roots[0]).unwrap();
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_move_roots_source_and_resources_during_managed_gc() {
    for is_async in [false, true] {
        let mut vm = page();
        let ids = states(&mut vm, is_async, 7);
        let roots = roots(&mut vm, &ids);
        let argument = vm.heap.alloc_object(None).unwrap();
        payload(&mut vm, ids[0], is_async, Value::Object(argument));
        warm_move(&mut vm, ids[0]);
        prime(&mut vm);
        for root in roots {
            vm.heap.unroot(root).unwrap();
        }
        let before = vm.heap.stats().major_collections;
        vm.heap.allow_only(0);
        let moved = vm
            .disposable_stack_move(&Value::Object(ids[0]), is_async)
            .unwrap()
            .object_id()
            .unwrap();
        assert!(vm.heap.stats().major_collections > before);
        assert!(vm.heap.contains(argument));
        assert_eq!(table(&vm, is_async).len(), 2);
        assert!(table(&vm, is_async)[&ids[0]].disposed);
        assert_eq!(
            table(&vm, is_async)[&moved].resources[0].argument,
            Some(Value::Object(argument))
        );
        assert!(vm.async_frame_roots.is_empty());
    }
}
#[cfg_attr(test, test)]
fn disposal_map_storage_nested_guard_defers_other_brand_compaction_after_gc() {
    let mut vm = page();
    let ids = states(&mut vm, false, 7);
    let roots = roots(&mut vm, &ids);
    vm.async_disposable_stacks.reserve(7);
    let other = states(&mut vm, true, 1)[0];
    let _other_root = vm.heap.root(other).unwrap();
    new_target(&mut vm, false);
    vm.preserve_promise_table_capacity = true;
    prime(&mut vm);
    for root in roots.iter().skip(1) {
        vm.heap.unroot(*root).unwrap();
    }
    vm.preserve_promise_table_capacity = true;
    vm.heap.allow_only(0);
    assert!(vm.disposable_stack_constructor(false, true).is_ok());
    assert!(vm.preserve_promise_table_capacity);
    assert_eq!(vm.async_disposable_stacks.capacity(), 7);
    vm.preserve_promise_table_capacity = false;
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.async_disposable_stacks.capacity(), 3);
    vm.heap.unroot(roots[0]).unwrap();
}
#[cfg_attr(test, test)]
fn disposal_map_storage_public_move_preserves_brand_and_cleanup_order() {
    for is_async in [false, true] {
        let mut vm = page();
        let name = if is_async {
            "AsyncDisposableStack"
        } else {
            "DisposableStack"
        };
        let dispose = if is_async { "disposeAsync" } else { "dispose" };
        let source = format!("var log=[];var source=new {name}();source.defer(()=>log.push('a'));source.defer(()=>log.push('b'));var moved=source.move();var flags=source.disposed&&!moved.disposed;source.{dispose}();moved.{dispose}();0;");
        vm.execute_script(&crate::compile(&crate::parse(&source).unwrap()).unwrap())
            .unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        let result = vm
            .execute_script(
                &crate::compile(
                    &crate::parse("flags&&moved.disposed&&log.join(',')==='b,a';").unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(result, Value::Bool(true));
    }
}

impl crate::Vm {
    /// Runs finite disposal-map admission and GC contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_disposal_map_storage_contracts() {
        disposal_map_storage_sync_gc_preserves_staged_capacity();
        disposal_map_storage_async_gc_preserves_staged_capacity();
        disposal_map_storage_constructor_refuses_complete_candidate_before_managed_allocation();
        disposal_map_storage_move_refuses_complete_candidate_before_source_mutation();
        disposal_map_storage_deletion_refunds_payload_but_retains_table_allocation();
        disposal_map_storage_optional_compaction_without_headroom_is_nonterminal();
        disposal_map_storage_short_reported_compaction_capacity_refunds_staging();
        disposal_map_storage_exact_compaction_reconciles_before_candidate_refund();
        disposal_map_storage_optional_refusal_can_retry_after_peer_release();
        disposal_map_storage_empty_backing_releases_after_terminal_failure();
        disposal_map_storage_both_brands_reclaim_with_sequential_owner_refunds();
        disposal_map_storage_constructor_exact_candidate_budget_transfers_once();
        disposal_map_storage_move_exact_candidate_budget_preserves_resource_buffer();
        disposal_map_storage_spare_constructor_slot_requires_no_fictitious_charge();
        disposal_map_storage_spare_move_slot_reuses_admitted_roots_and_payload();
        disposal_map_storage_root_preparation_refusal_preserves_source_and_table();
        disposal_map_storage_constructor_heap_refusal_publishes_no_record();
        disposal_map_storage_move_heap_refusal_keeps_resources_and_disposed_state();
        disposal_map_storage_constructor_gc_reuses_slots_freed_from_full_backing();
        disposal_map_storage_move_roots_source_and_resources_during_managed_gc();
        disposal_map_storage_nested_guard_defers_other_brand_compaction_after_gc();
        disposal_map_storage_public_move_preserves_brand_and_cleanup_order();
    }
}
