// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny weak tables and detached preparations verify module source ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm.next_host_native_gc_bytes = usize::MAX;
    vm.next_host_heap_gc_bytes = usize::MAX;
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn sample_table(count: usize) -> usize {
    host_scheduling::map_slots(&HashMap::<ObjectId, String>::with_capacity(count))
}
fn owned_native_bytes(vm: &Vm) -> usize {
    vm.own_scheduling_usage().payload_bytes
        + vm.heap.root_storage_bytes()
        + vm.heap.collector_storage_bytes()
        + vm.heap.realm_pin_storage_bytes()
        + vm.heap.finalization_index_bytes()
}
#[cfg_attr(test, test)]
fn referrer_storage_closure_preparation_keeps_its_lease_after_vm_drop() {
    let mut vm = page();
    vm.active_module_name = Some("home.js".into());
    let observer = observer(&vm);
    let prepared = vm.prepare_closure_module_referrer().unwrap();
    drop(vm);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        sample_table(1) + "home.js".len()
    );
    drop(prepared);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn referrer_storage_call_preparation_keeps_its_lease_after_vm_drop() {
    let mut vm = page();
    let state = vm.heap.alloc_object(None).unwrap();
    vm.module_closure_referrers.reserve(1);
    vm.module_closure_referrers.insert(state, "home.js".into());
    let observer = observer(&vm);
    let prepared = vm
        .prepare_call_module_referrer(&Value::Object(state))
        .unwrap();
    drop(vm);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        "home.js".len()
    );
    drop(prepared);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn referrer_storage_sparse_table_reclaims_through_admitted_replacement() {
    let mut vm = page();
    vm.module_closure_referrers.reserve(7);
    let state = vm.heap.alloc_object(None).unwrap();
    vm.module_closure_referrers.reserve(1);
    vm.module_closure_referrers.insert(state, "home.js".into());
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 3);
    assert_eq!(
        vm.module_closure_referrers.get(&state).map(String::as_str),
        Some("home.js")
    );
}

fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn table(vm: &mut Vm, count: usize) -> Vec<ObjectId> {
    vm.module_closure_referrers.reserve(count);
    (0..count)
        .map(|_| {
            let id = vm.heap.alloc_object(None).unwrap();
            vm.module_closure_referrers.insert(id, "home.js".into());
            id
        })
        .collect()
}
fn sparse() -> (Vm, ObjectId) {
    let mut vm = page();
    vm.module_closure_referrers.reserve(7);
    let id = table(&mut vm, 1)[0];
    (vm, id)
}
#[cfg_attr(test, test)]
fn referrer_storage_deletion_releases_names_without_refunding_backing() {
    let mut vm = page();
    let ids = table(&mut vm, 7);
    let backing = vm.module_closure_referrers.retained_bytes();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    for (index, id) in ids.iter().take(6).enumerate() {
        vm.module_closure_referrers.remove(id);
        assert_eq!(vm.module_closure_referrers.retained_bytes(), backing);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            before - (index + 1) * "home.js".len()
        );
    }
}
#[cfg_attr(test, test)]
fn referrer_storage_compaction_without_headroom_is_nonterminal() {
    let (mut vm, id) = sparse();
    let _peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    assert_eq!(
        vm.module_closure_referrers.get(&id).map(String::as_str),
        Some("home.js")
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn referrer_storage_short_reported_compaction_capacity_refunds_candidate() {
    let (mut vm, _) = sparse();
    let _peer = leave_room(&mut vm, sample_table(1) - 1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn referrer_storage_exact_compaction_transfers_owner_before_refund() {
    let (mut vm, id) = sparse();
    let old = vm.module_closure_referrers.retained_bytes();
    let _peer = leave_room(&mut vm, sample_table(1));
    vm.reclaim_scheduling_maps();
    assert_eq!(
        vm.module_closure_referrers.retained_bytes(),
        sample_table(1)
    );
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
    assert!(vm.module_closure_referrers.contains_key(&id));
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn referrer_storage_optional_refusal_can_retry_after_peer_release() {
    let (mut vm, _) = sparse();
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    drop(peer);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 3);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn referrer_storage_empty_backing_releases_after_terminal_failure() {
    let mut vm = page();
    let ids = table(&mut vm, 7);
    for id in ids {
        vm.module_closure_referrers.remove(&id);
    }
    let bytes = vm.module_closure_referrers.retained_bytes();
    vm.host_scheduling.as_ref().unwrap().fail();
    vm.reconcile_scheduling_ownership();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 0);
    assert_eq!(vm.module_closure_referrers.retained_bytes(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn referrer_storage_pending_publication_preserves_sparse_gc_backing() {
    let mut vm = page();
    let ids = table(&mut vm, 7);
    let root = vm.heap.root(ids[0]).unwrap();
    vm.active_module_name = Some("child.js".into());
    let prepared = vm.prepare_closure_module_referrer().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.module_closure_referrers.len(), 1);
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    drop(prepared);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.module_closure_referrers.capacity(), 3);
    vm.heap.unroot(root).unwrap();
}
#[cfg_attr(test, test)]
fn referrer_storage_abandoned_preparation_releases_empty_reserved_backing() {
    let mut vm = page();
    table(&mut vm, 7);
    vm.active_module_name = Some("child.js".into());
    let prepared = vm.prepare_closure_module_referrer().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.module_closure_referrers.is_empty());
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    drop(prepared);
    assert_eq!(vm.pending_module_referrers.get(), 0);
    vm.release_empty_module_referrer_storage();
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.module_closure_referrers.retained_bytes(), 0);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn referrer_storage_replacement_requires_full_candidate_not_net_growth() {
    let mut vm = page();
    let ids = table(&mut vm, 3);
    vm.active_module_name = Some("child.js".into());
    let old = vm.module_closure_referrers.retained_bytes();
    let _peer = leave_room(&mut vm, sample_table(4) - old + "child.js".len());
    let heap = vm.heap.stats();
    let usage = vm.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(
        vm.prepare_closure_module_referrer(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.module_closure_referrers.retained_bytes(), old);
    assert!(ids
        .iter()
        .all(|id| vm.module_closure_referrers.contains_key(id)));
    assert_eq!(vm.pending_module_referrers.get(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, usage);
}
#[cfg_attr(test, test)]
fn referrer_storage_interleaved_preparations_preserve_each_reserved_slot() {
    for reverse in [false, true] {
        let mut vm = page();
        table(&mut vm, 1);
        vm.active_module_name = Some("child.js".into());
        let first = vm.prepare_closure_module_referrer().unwrap();
        let second = vm.prepare_closure_module_referrer().unwrap();
        let third = vm.prepare_closure_module_referrer().unwrap();
        assert_eq!(vm.pending_module_referrers.get(), 3);
        let ids: Vec<_> = (0..3)
            .map(|_| vm.heap.alloc_object(None).unwrap())
            .collect();
        if reverse {
            third.publish(&mut vm, ids[2]);
            first.publish(&mut vm, ids[0]);
            second.publish(&mut vm, ids[1]);
        } else {
            first.publish(&mut vm, ids[0]);
            second.publish(&mut vm, ids[1]);
            third.publish(&mut vm, ids[2]);
        }
        assert_eq!(vm.pending_module_referrers.get(), 0);
        assert_eq!(vm.module_closure_referrers.len(), 4);
        assert_eq!(vm.module_closure_referrers.capacity(), 7);
        assert!(ids
            .iter()
            .all(|id| vm.module_closure_referrers.get(id).map(String::as_str) == Some("child.js")));
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn referrer_storage_multiple_preparations_keep_separate_post_vm_owners() {
    let mut vm = page();
    vm.active_module_name = Some("home.js".into());
    let first = vm.prepare_closure_module_referrer().unwrap();
    let second = vm.prepare_closure_module_referrer().unwrap();
    let observer = observer(&vm);
    drop(vm);
    let bytes = sample_table(1) + "home.js".len();
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes * 2);
    drop(first);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(second);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn referrer_storage_prepared_publication_needs_no_admission_after_abort() {
    let mut vm = page();
    vm.active_module_name = Some("home.js".into());
    let id = vm.heap.alloc_object(None).unwrap();
    let prepared = vm.prepare_closure_module_referrer().unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    prepared.publish(&mut vm, id);
    assert_eq!(
        vm.module_closure_referrers.get(&id).map(String::as_str),
        Some("home.js")
    );
    assert_eq!(vm.pending_module_referrers.get(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        owned_native_bytes(&vm)
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn referrer_storage_shared_staging_guard_defers_optional_reclamation() {
    let (mut vm, _) = sparse();
    vm.preserve_promise_table_capacity = true;
    vm.reclaim_module_referrer_storage();
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    vm.preserve_promise_table_capacity = false;
    vm.reclaim_module_referrer_storage();
    assert_eq!(vm.module_closure_referrers.capacity(), 3);
}
#[cfg_attr(test, test)]
fn referrer_storage_prepared_source_survives_ambient_name_change() {
    let mut vm = page();
    vm.active_module_name = Some("home.js".into());
    let prepared = vm.prepare_closure_module_referrer().unwrap();
    vm.active_module_name = Some("other.js".into());
    let id = vm.heap.alloc_object(None).unwrap();
    prepared.publish(&mut vm, id);
    assert_eq!(
        vm.module_closure_referrers.get(&id).map(String::as_str),
        Some("home.js")
    );
    assert_eq!(vm.active_module_name.as_deref(), Some("other.js"));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        owned_native_bytes(&vm)
    );
}
#[cfg_attr(test, test)]
fn referrer_storage_call_installation_transfers_exact_budget_once() {
    let mut vm = page();
    let id = table(&mut vm, 1)[0];
    vm.active_module_name = Some("caller.js".into());
    let _peer = leave_room(&mut vm, "home.js".len());
    let mut prepared = vm.prepare_call_module_referrer(&Value::Object(id)).unwrap();
    let previous = prepared.install(&mut vm).unwrap();
    let caller_charge = vm.charge_execution_control(previous.as_ref().unwrap().capacity());
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
    assert_eq!(vm.active_module_name.as_deref(), Some("home.js"));
    vm.active_module_name = previous;
    drop(caller_charge);
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - "home.js".len()
    );
}
#[cfg_attr(test, test)]
fn referrer_storage_absent_and_empty_sources_keep_distinct_zero_byte_paths() {
    let mut vm = page();
    let id = vm.heap.alloc_object(None).unwrap();
    vm.active_module_name = Some("caller.js".into());
    let mut absent = vm.prepare_call_module_referrer(&Value::Object(id)).unwrap();
    assert!(absent.install(&mut vm).is_none());
    vm.module_closure_referrers.reserve(1);
    vm.module_closure_referrers.insert(id, String::new());
    let _peer = leave_room(&mut vm, 0);
    let mut empty = vm.prepare_call_module_referrer(&Value::Object(id)).unwrap();
    let previous = empty.install(&mut vm).unwrap();
    assert_eq!(vm.active_module_name.as_deref(), Some(""));
    vm.active_module_name = previous;
    drop(empty);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn referrer_storage_growth_preserves_old_entries_and_owns_new_backing() {
    let mut vm = page();
    let ids = table(&mut vm, 3);
    vm.active_module_name = Some("child.js".into());
    let id = vm.heap.alloc_object(None).unwrap();
    let prepared = vm.prepare_closure_module_referrer().unwrap();
    prepared.publish(&mut vm, id);
    assert_eq!(
        vm.module_closure_referrers.retained_bytes(),
        sample_table(4)
    );
    for old in ids {
        assert_eq!(
            vm.module_closure_referrers.get(&old).map(String::as_str),
            Some("home.js")
        );
    }
    assert_eq!(
        vm.module_closure_referrers.get(&id).map(String::as_str),
        Some("child.js")
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        owned_native_bytes(&vm)
    );
}
#[cfg_attr(test, test)]
fn referrer_storage_gc_freed_slot_discards_unneeded_complete_candidate() {
    let mut vm = page();
    table(&mut vm, 3);
    vm.active_module_name = Some("child.js".into());
    let prepared = vm.prepare_closure_module_referrer().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.module_closure_referrers.is_empty());
    assert_eq!(vm.module_closure_referrers.capacity(), 3);
    let id = vm.heap.alloc_object(None).unwrap();
    prepared.publish(&mut vm, id);
    assert_eq!(
        vm.module_closure_referrers.retained_bytes(),
        sample_table(1)
    );
    assert_eq!(vm.module_closure_referrers.len(), 1);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        owned_native_bytes(&vm)
    );
}

#[cfg_attr(test, test)]
fn referrer_storage_exact_publication_transfers_empty_spare_and_full_tables() {
    for count in [0, 1, 3] {
        let mut vm = page();
        table(&mut vm, count);
        let id = vm.heap.alloc_object(None).unwrap();
        vm.active_module_name = Some("child.js".into());
        let bytes = "child.js".len()
            + if count == vm.module_closure_referrers.capacity() {
                sample_table(count + 1)
            } else {
                0
            };
        let before = vm.own_scheduling_usage().payload_bytes;
        let _peer = leave_room(&mut vm, bytes);
        let prepared = vm.prepare_closure_module_referrer().unwrap();
        prepared.publish(&mut vm, id);
        assert!(vm.check_scheduling_failure().is_ok());
        assert_eq!(vm.pending_module_referrers.get(), 0);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - bytes - before + vm.own_scheduling_usage().payload_bytes
        );
        assert_eq!(
            vm.module_closure_referrers.get(&id).map(String::as_str),
            Some("child.js")
        );
    }
}

#[cfg_attr(test, test)]
fn referrer_storage_older_candidate_cannot_replace_larger_backing_during_new_growth() {
    let mut vm = page();
    vm.active_module_name = Some("home.js".into());
    let oldest = vm.prepare_closure_module_referrer().unwrap();
    let second = vm.prepare_closure_module_referrer().unwrap();
    let third = vm.prepare_closure_module_referrer().unwrap();
    let fourth = vm.prepare_closure_module_referrer().unwrap();
    let ids: Vec<_> = (0..8)
        .map(|_| vm.heap.alloc_object(None).unwrap())
        .collect();
    fourth.publish(&mut vm, ids[3]);
    second.publish(&mut vm, ids[1]);
    third.publish(&mut vm, ids[2]);
    for id in &ids[4..7] {
        vm.prepare_closure_module_referrer()
            .unwrap()
            .publish(&mut vm, *id);
    }
    assert_eq!(vm.module_closure_referrers.len(), 6);
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    let newest = vm.prepare_closure_module_referrer().unwrap();
    // The oldest owns a three-slot candidate, and the newest a larger growth
    // candidate. Its already-reserved slot fits in the current seven-slot map.
    oldest.publish(&mut vm, ids[0]);
    assert_eq!(vm.module_closure_referrers.capacity(), 7);
    newest.publish(&mut vm, ids[7]);
    assert!(vm.module_closure_referrers.capacity() > 7);
    assert_eq!(
        vm.module_closure_referrers.retained_bytes(),
        sample_table(8)
    );
    assert_eq!(vm.module_closure_referrers.len(), 8);
    assert_eq!(vm.pending_module_referrers.get(), 0);
    assert!(ids
        .iter()
        .all(|id| vm.module_closure_referrers.contains_key(id)));
    assert!(vm.check_scheduling_failure().is_ok());
}

impl crate::Vm {
    /// Runs finite retained module-source contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_referrer_storage_contracts() {
        referrer_storage_closure_preparation_keeps_its_lease_after_vm_drop();
        referrer_storage_call_preparation_keeps_its_lease_after_vm_drop();
        referrer_storage_sparse_table_reclaims_through_admitted_replacement();
        referrer_storage_deletion_releases_names_without_refunding_backing();
        referrer_storage_compaction_without_headroom_is_nonterminal();
        referrer_storage_short_reported_compaction_capacity_refunds_candidate();
        referrer_storage_exact_compaction_transfers_owner_before_refund();
        referrer_storage_optional_refusal_can_retry_after_peer_release();
        referrer_storage_empty_backing_releases_after_terminal_failure();
        referrer_storage_pending_publication_preserves_sparse_gc_backing();
        referrer_storage_abandoned_preparation_releases_empty_reserved_backing();
        referrer_storage_replacement_requires_full_candidate_not_net_growth();
        referrer_storage_interleaved_preparations_preserve_each_reserved_slot();
        referrer_storage_multiple_preparations_keep_separate_post_vm_owners();
        referrer_storage_prepared_publication_needs_no_admission_after_abort();
        referrer_storage_shared_staging_guard_defers_optional_reclamation();
        referrer_storage_prepared_source_survives_ambient_name_change();
        referrer_storage_call_installation_transfers_exact_budget_once();
        referrer_storage_absent_and_empty_sources_keep_distinct_zero_byte_paths();
        referrer_storage_growth_preserves_old_entries_and_owns_new_backing();
        referrer_storage_gc_freed_slot_discards_unneeded_complete_candidate();
        referrer_storage_exact_publication_transfers_empty_spare_and_full_tables();
        referrer_storage_older_candidate_cannot_replace_larger_backing_during_new_growth();
    }
}
