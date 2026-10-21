// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed small tables and numerical peers exercise native allocation ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn fixture(count: usize, keep: usize) -> (Vm, Vec<ObjectId>) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    let mut objects = Vec::new();
    let mut roots = Vec::new();
    for _ in 0..count {
        let promise = vm.new_promise().unwrap();
        objects.push(promise);
        roots.push(vm.heap.root(promise).unwrap());
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    for root in roots.into_iter().skip(keep) {
        vm.heap.unroot(root).unwrap();
    }
    vm.synchronize_scheduling_usage().unwrap();
    (vm, objects)
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
fn retained_control_map_gc_compaction_requires_detached_table_headroom() {
    let (mut vm, objects) = fixture(15, 1);
    let capacity = vm.promises.capacity();
    let bytes = host_scheduling::map_slots(&vm.promises);
    assert_eq!(capacity, 28);
    let _peer = leave_room(&mut vm, 0);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.len(), 1);
    assert!(vm.promises.capacity() > 3);
    assert_eq!(vm.promises.retained_bytes(), bytes);
    assert!(vm.heap.contains(objects[0]));
    assert!(objects.iter().skip(1).all(|id| !vm.heap.contains(*id)));
    assert!(vm.check_scheduling_failure().is_ok());
}

#[derive(Clone, Default)]
struct CollisionHasher;
impl std::hash::Hasher for CollisionHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _bytes: &[u8]) {}
}
type CollisionMap = RetainedControlMap<u64, u64, std::hash::BuildHasherDefault<CollisionHasher>>;
fn collision_map() -> CollisionMap {
    let mut data = HashMap::with_capacity_and_hasher(28, Default::default());
    for key in 0..28 {
        data.insert(key, key + 1);
    }
    let allocation_bytes = host_scheduling::map_slots(&data);
    RetainedControlMap {
        data,
        allocation_bytes,
        storage: None,
    }
}
fn sparse_map() -> CollisionMap {
    let mut map = collision_map();
    map.retain(|key, _| *key == 0);
    map
}
fn owned_scope(map: &CollisionMap) -> SchedulingScope {
    let mut scope = SchedulingScope::new();
    reconcile(&mut scope, map.retained_bytes()).unwrap();
    scope
}
fn reconcile(scope: &mut SchedulingScope, bytes: usize) -> Result<(), RuntimeError> {
    scope.check(
        HostSchedulingUsage {
            payload_bytes: bytes,
            ..Default::default()
        },
        HostSchedulingUsage::default(),
    )
}
fn usage(scope: &SchedulingScope) -> usize {
    let view = Vm {
        host_scheduling: Some(scope.child()),
        ..Vm::default()
    };
    view.host_scheduling_snapshot().payload_bytes
}
#[cfg_attr(test, test)]
fn retained_control_map_tombstones_do_not_refund_live_backing() {
    let mut map = collision_map();
    let bytes = map.retained_bytes();
    let capacity = map.capacity();
    map.retain(|key, _| *key != 12);
    assert!(
        map.capacity() < capacity,
        "fixed collision layout creates a tombstone"
    );
    assert_eq!(map.retained_bytes(), bytes);
    assert!(map.retained_bytes() > host_scheduling::map_slots(&map));
    assert_eq!(map.get(&11), Some(&12));
}
#[cfg_attr(test, test)]
fn retained_control_map_repeated_deletion_keeps_the_original_allocation() {
    let mut map = collision_map();
    let bytes = map.retained_bytes();
    for key in [12, 13, 14] {
        map.retain(|present, _| *present != key);
        assert_eq!(map.retained_bytes(), bytes);
    }
    assert_eq!(map.len(), 25);
}
#[cfg_attr(test, test)]
fn retained_control_map_empty_cache_releases_only_when_requested() {
    let mut map = collision_map();
    let bytes = map.retained_bytes();
    map.retain(|_, _| false);
    assert!(map.is_empty());
    // Every erased slot can be a tombstone: reported capacity is zero even
    // though retain did not release the original hash allocation.
    assert_eq!(map.capacity(), 0);
    assert_eq!(map.retained_bytes(), bytes);
    map.release_empty();
    assert_eq!(map.capacity(), 0);
    assert_eq!(map.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn retained_control_map_unbounded_compaction_tracks_replacement_not_high_water() {
    let mut map = sparse_map();
    let old = map.retained_bytes();
    let prepared = map.prepare_compaction(None).unwrap();
    assert!(map.commit_compaction(prepared).is_none());
    assert_eq!(map.capacity(), 3);
    assert!(map.retained_bytes() < old);
    assert_eq!(map.retained_bytes(), host_scheduling::map_slots(&map));
    assert_eq!(map.get(&0), Some(&1));
}
#[cfg_attr(test, test)]
fn retained_control_map_dropped_candidate_refunds_without_changing_backing() {
    let map = sparse_map();
    let mut scope = owned_scope(&map);
    let old = map.retained_bytes();
    let prepared = map.prepare_compaction(Some(&scope)).unwrap();
    assert_eq!(
        usage(&scope),
        old + host_scheduling::map_slots(&prepared.replacement)
    );
    drop(prepared);
    assert_eq!(usage(&scope), old);
    reconcile(&mut scope, old).unwrap();
    assert_eq!(map.get(&0), Some(&1));
}
#[cfg_attr(test, test)]
fn retained_control_map_minimum_refusal_keeps_compaction_optional() {
    let map = sparse_map();
    let mut scope = owned_scope(&map);
    let old = map.retained_bytes();
    let _peer = scope
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - old)
        .unwrap();
    assert!(map.prepare_compaction(Some(&scope)).is_none());
    assert_eq!(usage(&scope), MAX_HOST_SCHEDULING_BYTES);
    reconcile(&mut scope, old).unwrap();
    assert_eq!(map.retained_bytes(), old);
}
#[cfg_attr(test, test)]
fn retained_control_map_reported_capacity_refusal_does_not_latch_an_abort() {
    let map = sparse_map();
    let mut scope = owned_scope(&map);
    let old = map.retained_bytes();
    let room = 3 * std::mem::size_of::<(u64, u64)>() - 1;
    let _peer = scope
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - old - room)
        .unwrap();
    assert!(map.prepare_compaction(Some(&scope)).is_none());
    assert_eq!(usage(&scope), MAX_HOST_SCHEDULING_BYTES - room);
    reconcile(&mut scope, old).unwrap();
    assert_eq!(map.retained_bytes(), old);
}
#[cfg_attr(test, test)]
fn retained_control_map_exact_candidate_moves_survivors_before_refunding_its_lease() {
    let mut map = sparse_map();
    let mut scope = owned_scope(&map);
    let old = map.retained_bytes();
    let room = 3 * std::mem::size_of::<(u64, u64)>();
    let _peer = scope
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - old - room)
        .unwrap();
    let prepared = map.prepare_compaction(Some(&scope)).unwrap();
    assert_eq!(usage(&scope), MAX_HOST_SCHEDULING_BYTES);
    let charge = map.commit_compaction(prepared);
    reconcile(&mut scope, map.retained_bytes()).unwrap();
    drop(charge);
    assert_eq!(usage(&scope), MAX_HOST_SCHEDULING_BYTES - old);
    assert_eq!(map.get(&0), Some(&1));
}
#[cfg_attr(test, test)]
fn retained_control_map_terminal_accounts_never_recover_through_reclamation() {
    let mut map = sparse_map();
    let mut scope = owned_scope(&map);
    let _ = scope.fail();
    assert!(map.prepare_compaction(Some(&scope)).is_none());
    map.retain(|_, _| false);
    map.release_empty();
    assert_eq!(reconcile(&mut scope, 0), Err(RuntimeError::SchedulingLimit));
    assert_eq!(usage(&scope), 0);
    assert!(scope.try_reclamation_charge(0).is_none());
}
#[cfg_attr(test, test)]
fn retained_control_map_mandatory_refusal_still_latches_the_document_abort() {
    let scope = SchedulingScope::new();
    let peer = scope.shared_charge(MAX_HOST_SCHEDULING_BYTES).unwrap();
    assert!(matches!(
        scope.shared_charge(1),
        Err(RuntimeError::SchedulingLimit)
    ));
    drop(peer);
    assert!(matches!(
        scope.shared_charge(0),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(scope.try_reclamation_charge(0).is_none());
}
#[cfg_attr(test, test)]
fn retained_control_map_gc_exact_candidate_reclaims_sparse_backing() {
    let (mut vm, objects) = fixture(15, 1);
    let old = vm.promises.retained_bytes();
    let room = 3 * std::mem::size_of::<(ObjectId, PromiseRecord)>();
    let _peer = leave_room(&mut vm, room);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(vm.promises.capacity(), 3);
    assert_eq!(vm.promises.retained_bytes(), room);
    assert!(vm.heap.contains(objects[0]));
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}
#[cfg_attr(test, test)]
fn retained_control_map_gc_one_byte_short_keeps_the_retained_allocation() {
    let (mut vm, _) = fixture(15, 1);
    let old = vm.promises.retained_bytes();
    let room = 3 * std::mem::size_of::<(ObjectId, PromiseRecord)>() - 1;
    let _peer = leave_room(&mut vm, room);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(vm.promises.retained_bytes(), old);
    assert!(vm.promises.capacity() > 3);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn retained_control_map_gc_releases_empty_backing_without_replacement_headroom() {
    let (mut vm, objects) = fixture(15, 0);
    let old = vm.promises.retained_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.promises.is_empty());
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(vm.promises.retained_bytes(), 0);
    assert!(objects.iter().all(|id| !vm.heap.contains(*id)));
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}
#[cfg_attr(test, test)]
fn retained_control_map_skipped_compaction_can_retry_after_peer_release() {
    let (mut vm, objects) = fixture(15, 1);
    let old = vm.promises.retained_bytes();
    let peer = leave_room(&mut vm, 0);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.retained_bytes(), old);
    drop(peer);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.capacity(), 3);
    assert!(vm.heap.contains(objects[0]));
    vm.new_promise().unwrap();
    assert_eq!(vm.promises.len(), 2);
}
#[cfg_attr(test, test)]
fn retained_control_map_managed_preparation_preserves_the_admitted_allocation() {
    let (mut vm, objects) = fixture(15, 1);
    let old = vm.promises.retained_bytes();
    vm.preserve_promise_table_capacity = true;
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(vm.promises.retained_bytes(), old);
    assert!(vm.heap.contains(objects[0]));
    vm.preserve_promise_table_capacity = false;
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.capacity(), 3);
}

#[cfg_attr(test, test)]
fn retained_control_map_compaction_discards_candidates_that_do_not_reduce_backing() {
    let mut map = sparse_map();
    let prepared = map.prepare_compaction(None).unwrap();
    map.commit_compaction(prepared);
    let mut scope = owned_scope(&map);
    let bytes = map.retained_bytes();
    assert!(map.prepare_compaction(Some(&scope)).is_none());
    assert_eq!(usage(&scope), bytes);
    reconcile(&mut scope, bytes).unwrap();
    assert_eq!(map.get(&0), Some(&1));
}

#[cfg_attr(test, test)]
fn retained_control_map_sparse_backing_uses_allocation_instead_of_tombstone_headroom() {
    let mut map = collision_map();
    assert!(!map.has_sparse_backing());
    map.retain(|key, _| *key == 0);
    assert!(
        map.len() * 4 >= map.capacity(),
        "reported headroom would skip compaction"
    );
    assert!(map.has_sparse_backing());
    let prepared = map.prepare_compaction(None).unwrap();
    map.commit_compaction(prepared);
    assert!(!map.has_sparse_backing());
    assert_eq!(map.get(&0), Some(&1));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_retained_control_map_contracts() {
        retained_control_map_gc_compaction_requires_detached_table_headroom();
        retained_control_map_tombstones_do_not_refund_live_backing();
        retained_control_map_repeated_deletion_keeps_the_original_allocation();
        retained_control_map_empty_cache_releases_only_when_requested();
        retained_control_map_unbounded_compaction_tracks_replacement_not_high_water();
        retained_control_map_dropped_candidate_refunds_without_changing_backing();
        retained_control_map_minimum_refusal_keeps_compaction_optional();
        retained_control_map_reported_capacity_refusal_does_not_latch_an_abort();
        retained_control_map_exact_candidate_moves_survivors_before_refunding_its_lease();
        retained_control_map_terminal_accounts_never_recover_through_reclamation();
        retained_control_map_mandatory_refusal_still_latches_the_document_abort();
        retained_control_map_gc_exact_candidate_reclaims_sparse_backing();
        retained_control_map_gc_one_byte_short_keeps_the_retained_allocation();
        retained_control_map_gc_releases_empty_backing_without_replacement_headroom();
        retained_control_map_skipped_compaction_can_retry_after_peer_release();
        retained_control_map_managed_preparation_preserves_the_admitted_allocation();
        retained_control_map_compaction_discards_candidates_that_do_not_reduce_backing();
        retained_control_map_sparse_backing_uses_allocation_instead_of_tombstone_headroom();
    }
}
