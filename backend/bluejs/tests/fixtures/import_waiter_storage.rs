// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny import registrations and scalar document peers exercise preparation.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    vm
}
fn target(vm: &mut Vm) -> ObjectId {
    let promise = vm.new_promise().unwrap();
    vm.heap.root(promise).unwrap();
    promise
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
#[cfg_attr(test, test)]
fn import_waiter_storage_existing_vector_requires_the_complete_candidate() {
    let mut vm = page();
    let promise = target(&mut vm);
    for _ in 0..4 {
        vm.enqueue_module_import_waiter("entry".into(), promise)
            .unwrap();
    }
    assert_eq!(vm.module_import_waiters["entry"].capacity(), 4);
    let _peer = leave_room(&mut vm, 4 * std::mem::size_of::<ObjectId>());
    assert_eq!(
        vm.enqueue_module_import_waiter("entry".into(), promise),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_import_waiters["entry"].len(), 4);
    assert_eq!(vm.module_import_waiters["entry"].capacity(), 4);
}
#[cfg_attr(test, test)]
fn import_waiter_storage_new_key_requires_old_and_new_tables_together() {
    let mut vm = page();
    let promise = target(&mut vm);
    for name in ["a", "b", "c"] {
        vm.enqueue_module_import_waiter(name.into(), promise)
            .unwrap();
    }
    let old = host_scheduling::map_slots(&vm.module_import_waiters);
    let candidate = host_scheduling::map_slots(&HashMap::<String, Vec<ObjectId>>::with_capacity(4));
    let room = candidate - old + 4 * std::mem::size_of::<ObjectId>() + 1;
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        vm.enqueue_module_import_waiter("d".into(), promise),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_import_waiters.len(), 3);
    assert_eq!(vm.module_import_waiters.capacity(), 3);
}

fn names(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("m{index}")).collect()
}
fn dependency_peak(input: &Vec<String>) -> usize {
    host_scheduling::slots(input)
        + input.iter().map(String::capacity).sum::<usize>()
        + HashSet::<String>::with_capacity(input.len()).capacity() * std::mem::size_of::<String>()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn deferred(vm: &mut Vm, promise: ObjectId, input: Vec<String>) {
    vm.enqueue_deferred_import_waiter(promise, promise, input)
        .unwrap();
}
#[cfg_attr(test, test)]
fn import_waiter_storage_existing_vector_exact_candidate_commits_once() {
    let mut vm = page();
    let promise = target(&mut vm);
    for _ in 0..4 {
        vm.enqueue_module_import_waiter("entry".into(), promise)
            .unwrap();
    }
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 8 * std::mem::size_of::<ObjectId>());
    vm.enqueue_module_import_waiter("entry".into(), promise)
        .unwrap();
    assert_eq!(vm.module_import_waiters["entry"].len(), 5);
    assert_eq!(vm.module_import_waiters["entry"].capacity(), 8);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - 4 * std::mem::size_of::<ObjectId>()
    );
}
#[cfg_attr(test, test)]
fn import_waiter_storage_existing_slot_discards_the_temporary_key_before_preparation() {
    let mut vm = page();
    let promise = target(&mut vm);
    vm.enqueue_module_import_waiter("entry".into(), promise)
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    let mut lookup = String::with_capacity(32);
    lookup.push_str("entry");
    vm.enqueue_module_import_waiter(lookup, promise).unwrap();
    assert_eq!(vm.module_import_waiters["entry"], [promise, promise]);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn import_waiter_storage_new_key_exact_coexisting_candidates_preserve_old_targets() {
    let mut vm = page();
    let promise = target(&mut vm);
    for name in ["a", "b", "c"] {
        vm.enqueue_module_import_waiter(name.into(), promise)
            .unwrap();
    }
    let old = vm.module_import_waiters.retained_bytes();
    let bytes = 7 * std::mem::size_of::<(String, Vec<ObjectId>)>()
        + 4 * std::mem::size_of::<ObjectId>()
        + 1;
    let _peer = leave_room(&mut vm, bytes);
    vm.enqueue_module_import_waiter("d".into(), promise)
        .unwrap();
    assert_eq!(vm.module_import_waiters.len(), 4);
    for name in ["a", "b", "c", "d"] {
        assert_eq!(vm.module_import_waiters[name], [promise]);
    }
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}
#[cfg_attr(test, test)]
fn import_waiter_storage_new_key_one_byte_short_keeps_live_capacity_and_roots() {
    let mut vm = page();
    let promise = target(&mut vm);
    let bytes =
        3 * std::mem::size_of::<(String, Vec<ObjectId>)>() + 4 * std::mem::size_of::<ObjectId>();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(
        vm.enqueue_module_import_waiter("a".into(), promise),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.module_import_waiters.is_empty());
    assert_eq!(vm.module_import_waiters.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state(), roots);
}
#[cfg_attr(test, test)]
fn import_waiter_storage_dependency_peak_admits_both_metadata_allocations() {
    let mut vm = page();
    let input = names(3);
    let bytes = dependency_peak(&input);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let peer = leave_room(&mut vm, bytes);
    let prepared = vm.prepare_import_dependencies(input).unwrap();
    assert_eq!(prepared.pending.len(), 3);
    let retained = prepared.pending.retained_bytes();
    assert!(retained < bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes + retained
    );
    drop(prepared);
    drop(peer);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn import_waiter_storage_dependency_reported_capacity_refusal_refunds_preparation() {
    let mut vm = page();
    let input = names(1);
    let bytes = dependency_peak(&input) - 1;
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let peer = leave_room(&mut vm, bytes);
    assert!(matches!(
        vm.prepare_import_dependencies(input),
        Err(RuntimeError::SchedulingLimit)
    ));
    drop(peer);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.deferred_import_waiters.is_empty());
}
#[cfg_attr(test, test)]
fn import_waiter_storage_duplicate_names_move_once_and_refund_duplicate_buffers() {
    let mut vm = page();
    let mut first = String::with_capacity(16);
    first.push_str("entry");
    let input = vec![first, "entry".into(), "entry".into()];
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm.prepare_import_dependencies(input).unwrap();
    assert_eq!(prepared.pending.len(), 1);
    assert_eq!(prepared.pending.iter().next().unwrap().capacity(), 16);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + prepared.pending.retained_bytes()
    );
}
#[cfg_attr(test, test)]
fn import_waiter_storage_prepared_dependencies_keep_their_lease_after_vm_drop() {
    let vm = page();
    let view = observer(&vm);
    let prepared = vm.prepare_import_dependencies(names(2)).unwrap();
    let bytes = prepared.pending.retained_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn import_waiter_storage_deferred_vector_requires_a_complete_replacement() {
    let mut vm = page();
    let promise = target(&mut vm);
    for _ in 0..4 {
        deferred(&mut vm, promise, names(1));
    }
    let input = names(1);
    let budget = dependency_peak(&input) + 4 * std::mem::size_of::<DeferredImportWaiter>();
    let _peer = leave_room(&mut vm, budget);
    assert_eq!(
        vm.enqueue_deferred_import_waiter(promise, promise, input),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.deferred_import_waiters.len(), 4);
    assert_eq!(vm.deferred_import_waiters.capacity(), 4);
}
#[cfg_attr(test, test)]
fn import_waiter_storage_deferred_existing_slot_charges_the_input_peak_then_hands_off() {
    let mut vm = page();
    let promise = target(&mut vm);
    deferred(&mut vm, promise, names(1));
    let input = names(1);
    let bytes = dependency_peak(&input);
    let _peer = leave_room(&mut vm, bytes);
    deferred(&mut vm, promise, input);
    assert_eq!(vm.deferred_import_waiters.len(), 2);
    assert_eq!(vm.deferred_import_waiters.capacity(), 4);
    let retained = vm.deferred_import_waiters[1].pending.retained_bytes();
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes + retained
    );
}
#[cfg_attr(test, test)]
fn import_waiter_storage_shared_count_refusal_precedes_all_native_preparation() {
    for eager in [true, false] {
        let mut vm = page();
        let promise = target(&mut vm);
        let used = vm.host_scheduling_usage().unwrap();
        let mut peer = vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            HostSchedulingUsage {
                pending_reactions: MAX_PENDING_HOST_REACTIONS - used.pending_reactions,
                ..Default::default()
            },
            HostSchedulingUsage::default(),
        )
        .unwrap();
        let result = if eager {
            vm.enqueue_module_import_waiter("a".into(), promise)
        } else {
            vm.enqueue_deferred_import_waiter(promise, promise, names(1))
        };
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert_eq!(vm.module_import_waiters.retained_bytes(), 0);
        assert_eq!(vm.deferred_import_waiters.capacity(), 0);
    }
}
#[cfg_attr(test, test)]
fn import_waiter_storage_dependency_removal_keeps_backing_until_admitted_compaction() {
    let mut vm = page();
    let promise = target(&mut vm);
    deferred(&mut vm, promise, names(7));
    for index in 0..6 {
        vm.deferred_import_waiters[0]
            .pending
            .remove(&format!("m{index}"));
    }
    let old = vm.deferred_import_waiters[0].pending.retained_bytes();
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.deferred_import_waiters[0].pending.retained_bytes(), old);
    assert!(vm.check_scheduling_failure().is_ok());
    drop(peer);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.deferred_import_waiters[0].pending.capacity(), 3);
    assert!(vm.deferred_import_waiters[0].pending.retained_bytes() < old);
    assert!(vm.deferred_import_waiters[0].pending.contains("m6"));
}
#[cfg_attr(test, test)]
fn import_waiter_storage_dependency_compaction_reported_refusal_is_optional() {
    let mut vm = page();
    let promise = target(&mut vm);
    deferred(&mut vm, promise, names(7));
    for index in 0..6 {
        vm.deferred_import_waiters[0]
            .pending
            .remove(&format!("m{index}"));
    }
    let old = vm.deferred_import_waiters[0].pending.retained_bytes();
    let _peer = leave_room(&mut vm, 3 * std::mem::size_of::<String>() - 1);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.deferred_import_waiters[0].pending.retained_bytes(), old);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn import_waiter_storage_deferred_vector_compaction_funds_coexisting_backing() {
    let mut vm = page();
    let promise = target(&mut vm);
    for _ in 0..8 {
        deferred(&mut vm, promise, names(1));
    }
    vm.deferred_import_waiters.truncate(1);
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.deferred_import_waiters.capacity(), 8);
    assert!(vm.check_scheduling_failure().is_ok());
    drop(peer);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.deferred_import_waiters.capacity(), 1);
    assert_eq!(vm.deferred_import_waiters[0].promise, promise);
}
#[cfg_attr(test, test)]
fn import_waiter_storage_empty_cleanup_needs_no_admission_even_after_failure() {
    let mut vm = page();
    vm.module_import_waiters.reserve(1);
    vm.deferred_import_waiters.reserve(4);
    vm.synchronize_scheduling_usage().unwrap();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.module_import_waiters.retained_bytes(), 0);
    assert_eq!(vm.deferred_import_waiters.capacity(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn import_waiter_storage_registered_targets_survive_gc_until_their_owners_drop() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.enqueue_module_import_waiter("a".into(), promise)
        .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(promise));
    vm.module_import_waiters.remove("a");
    vm.reclaim_import_waiter_storage();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise));
}
#[cfg_attr(test, test)]
fn import_waiter_storage_deferred_targets_and_namespace_remain_rooted() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let root = vm.heap.root(promise).unwrap();
    let namespace = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.enqueue_deferred_import_waiter(promise, namespace, names(1))
        .unwrap();
    vm.heap.unroot(root).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(promise) && vm.heap.contains(namespace));
    vm.deferred_import_waiters.clear();
    vm.reclaim_import_waiter_storage();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise) && !vm.heap.contains(namespace));
}
#[cfg_attr(test, test)]
fn import_waiter_storage_import_map_compaction_retains_live_keys_and_targets() {
    let mut vm = page();
    let promise = target(&mut vm);
    for name in names(7) {
        vm.enqueue_module_import_waiter(name, promise).unwrap();
    }
    for name in names(6) {
        vm.module_import_waiters.remove(&name);
    }
    let old = vm.module_import_waiters.retained_bytes();
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.module_import_waiters.retained_bytes(), old);
    drop(peer);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.module_import_waiters.capacity(), 3);
    assert_eq!(vm.module_import_waiters["m6"], [promise]);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_import_waiter_storage_contracts() {
        import_waiter_storage_existing_vector_requires_the_complete_candidate();
        import_waiter_storage_new_key_requires_old_and_new_tables_together();
        import_waiter_storage_existing_vector_exact_candidate_commits_once();
        import_waiter_storage_existing_slot_discards_the_temporary_key_before_preparation();
        import_waiter_storage_new_key_exact_coexisting_candidates_preserve_old_targets();
        import_waiter_storage_new_key_one_byte_short_keeps_live_capacity_and_roots();
        import_waiter_storage_dependency_peak_admits_both_metadata_allocations();
        import_waiter_storage_dependency_reported_capacity_refusal_refunds_preparation();
        import_waiter_storage_duplicate_names_move_once_and_refund_duplicate_buffers();
        import_waiter_storage_prepared_dependencies_keep_their_lease_after_vm_drop();
        import_waiter_storage_deferred_vector_requires_a_complete_replacement();
        import_waiter_storage_deferred_existing_slot_charges_the_input_peak_then_hands_off();
        import_waiter_storage_shared_count_refusal_precedes_all_native_preparation();
        import_waiter_storage_dependency_removal_keeps_backing_until_admitted_compaction();
        import_waiter_storage_dependency_compaction_reported_refusal_is_optional();
        import_waiter_storage_deferred_vector_compaction_funds_coexisting_backing();
        import_waiter_storage_empty_cleanup_needs_no_admission_even_after_failure();
        import_waiter_storage_registered_targets_survive_gc_until_their_owners_drop();
        import_waiter_storage_deferred_targets_and_namespace_remain_rooted();
        import_waiter_storage_import_map_compaction_retains_live_keys_and_targets();
    }
}
