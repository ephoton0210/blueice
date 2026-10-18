// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small realm graphs and numeric allowances check metadata admission.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn fixture() -> (Vm, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.execute_script(&script("globalThis.realm=new ShadowRealm();0;"))
        .unwrap();
    let id = vm
        .lookup_global_name("realm")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    (vm, id)
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
fn realm_collection_metadata_refusal_precedes_heap_collection() {
    let (mut vm, id) = fixture();
    vm.realm_collection_cache = None;
    let before = vm.heap.stats().major_collections;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.collect_realm_garbage(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats().major_collections, before);
    assert!(vm.shadow_realms.contains_key(&id));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_refusal_preserves_unreachable_realm_owners() {
    let (mut vm, id) = fixture();
    vm.realm_collection_cache = None;
    let weak = vm.shadow_realms[&id].vm.downgrade();
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap.set(global, "realm", Value::Null).unwrap();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.collect_realm_garbage(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.shadow_realms.contains_key(&id));
    assert!(weak.upgrade().is_some());
    assert!(vm.heap.contains(id));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

pub(super) fn metadata_bytes(plan: &RealmCollection) -> usize {
    host_scheduling::slots(&plan.nodes)
        + host_scheduling::slots(&plan.pins)
        + host_scheduling::slots(&plan.indices)
        + host_scheduling::slots(&plan.known_owners)
        + plan.seen.capacity() * std::mem::size_of::<u64>()
        + plan
            .nodes
            .iter()
            .map(|node| {
                host_scheduling::slots(&node.instances)
                    + host_scheduling::slots(&node.wrappers)
                    + host_scheduling::slots(&node.extra)
                    + node.ignored.capacity() * std::mem::size_of::<RootId>()
            })
            .sum::<usize>()
}
fn clear_global(vm: &mut Vm, name: &str) {
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap.set(global, name, Value::Null).unwrap();
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_cached_and_rust_held_storage_has_one_owner() {
    let (mut vm, id) = fixture();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let bytes = vm.realm_collection_cache.as_ref().unwrap().storage.bytes;
    let plan = vm.prepare_realm_collection().unwrap().unwrap();
    assert_eq!(plan.len, 2);
    assert_eq!(metadata_bytes(&plan), bytes);
    assert_eq!(plan.storage.bytes, bytes);
    assert!(vm.realm_collection_cache.is_none());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.shadow_realms[&id].vm.strong_count(), 2);
    vm.recycle_realm_collection(plan);
    let cache = vm.realm_collection_cache.as_ref().unwrap();
    assert_eq!(cache.len, 0);
    assert!(cache
        .nodes
        .iter()
        .all(|node| node.record.is_none() && node.live.is_none()));
    assert_eq!(vm.shadow_realms[&id].vm.strong_count(), 1);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    vm.realm_collection_cache = None;
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_vector_refusal_preserves_prefix_and_refunds_staging() {
    let (mut vm, _) = fixture();
    let mut storage = MetadataStorage::new(vm.host_scheduling.as_ref()).unwrap();
    let mut values = Vec::<u64>::new();
    storage.reserve_vec(&mut values, 4).unwrap();
    values.extend([1, 2, 3, 4]);
    let pointer = values.as_ptr();
    let capacity = values.capacity();
    let bytes = storage.bytes;
    let _peer = leave_room(&mut vm, bytes);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        storage.reserve_vec(&mut values, 1),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(values, [1, 2, 3, 4]);
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!(values.capacity(), capacity);
    assert_eq!(storage.bytes, bytes);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    drop(values);
    drop(storage);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_reported_set_capacity_is_checked_before_publication() {
    let (mut vm, _) = fixture();
    let mut probe = HashSet::<u64>::new();
    probe.try_reserve(1).unwrap();
    let actual = probe.capacity() * std::mem::size_of::<u64>();
    drop(probe);
    let mut storage = MetadataStorage::new(vm.host_scheduling.as_ref()).unwrap();
    let mut values = HashSet::<u64>::new();
    let _peer = leave_room(&mut vm, actual - 1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        storage.reserve_set(&mut values, 1),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(values.is_empty());
    assert_eq!(values.capacity(), 0);
    assert_eq!(storage.bytes, 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_partial_discovery_refunds_records_and_buffers() {
    let (mut vm, id) = fixture();
    vm.realm_collection_cache = None;
    let mut probe = HashSet::<u64>::new();
    probe.try_reserve(1).unwrap();
    let room =
        4 * std::mem::size_of::<CollectionRealm>() + probe.capacity() * std::mem::size_of::<u64>();
    drop(probe);
    let references = vm.shadow_realms[&id].vm.strong_count();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, room);
    assert!(matches!(
        vm.prepare_realm_collection(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.shadow_realms[&id].vm.strong_count(), references);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
    assert_eq!(vm.heap.stats(), before);
    assert!(vm.realm_collection_cache.is_none());
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_direct_borrow_falls_back_at_full_scalar_quota() {
    let (mut vm, id) = fixture();
    vm.realm_collection_cache = None;
    let record = vm.shadow_realms[&id].clone();
    let child = record.vm.borrow_mut();
    let before = vm.heap.stats().major_collections;
    let child_before = child.heap.stats().major_collections;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(vm.collect_realm_garbage(), Ok(()));
    assert_eq!(vm.heap.stats().major_collections, before + 1);
    assert_eq!(child.heap.stats().major_collections, child_before);
    assert!(vm.realm_collection_cache.is_none());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_nested_borrow_refunds_discovery_before_fallback() {
    let (mut vm, id) = fixture();
    vm.execute_script(&script(
        "realm.evaluate('globalThis.nested=new ShadowRealm();0;');0;",
    ))
    .unwrap();
    let child = vm.shadow_realms[&id].clone();
    let nested = child
        .vm
        .borrow()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    let nested_guard = nested.vm.borrow_mut();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(vm.collect_realm_garbage(), Ok(()));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm
        .realm_collection_cache
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .all(|node| node.record.is_none()));
    drop(nested_guard);
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_shared_records_are_discovered_once() {
    let (mut vm, id) = fixture();
    let record = vm.shadow_realms[&id].clone();
    let alias = vm
        .execute_script(&script("globalThis.alias=new ShadowRealm();alias;"))
        .unwrap()
        .object_id()
        .unwrap();
    vm.adopt_shadow_realm(alias, record.clone());
    drop(record);
    let plan = vm.prepare_realm_collection().unwrap().unwrap();
    assert_eq!(plan.len, 2);
    assert_eq!(plan.nodes[0].instances.len(), 2);
    assert_eq!(plan.known_owners.len(), 2);
    assert_eq!(metadata_bytes(&plan), plan.storage.bytes);
    vm.recycle_realm_collection(plan);
    vm.collect_realm_garbage().unwrap();
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_duplicate_wrapper_targets_use_reserved_slots_once() {
    let (mut vm, _) = fixture();
    vm.execute_script(&script("realm.evaluate('globalThis.f=()=>42;0;');globalThis.first=realm.evaluate('f');globalThis.second=realm.evaluate('f');0;")).unwrap();
    let plan = vm.prepare_realm_collection().unwrap().unwrap();
    let home = plan
        .index_of(
            vm.shadow_wrapped_functions
                .values()
                .next()
                .unwrap()
                .home_heap,
        )
        .unwrap();
    assert!(plan.nodes[home].incoming >= 2);
    assert!(plan.nodes[home].extra.capacity() >= plan.nodes[home].incoming);
    assert!(plan.nodes[home].ignored.len() >= 2);
    vm.recycle_realm_collection(plan);
    vm.collect_realm_garbage().unwrap();
    assert_eq!(
        vm.execute_script(&script("first()+second();")).unwrap(),
        Value::Number(84.0)
    );
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_external_owner_keeps_the_shared_realm_live() {
    let (mut vm, id) = fixture();
    let record = vm.shadow_realms[&id].clone();
    let weak = record.vm.downgrade();
    let mut external = Vm::default();
    let alias = external
        .execute_script(&script("new ShadowRealm();"))
        .unwrap()
        .object_id()
        .unwrap();
    external.adopt_shadow_realm(alias, record);
    clear_global(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_some());
    drop(external);
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
    assert!(vm.shadow_realms.is_empty());
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_pending_job_keeps_its_realm_until_completion() {
    let (mut vm, id) = fixture();
    let record = vm.shadow_realms[&id].clone();
    let weak = record.vm.downgrade();
    record
        .vm
        .borrow_mut()
        .execute_script(&script("Promise.resolve().then(()=>42);0;"))
        .unwrap();
    drop(record);
    clear_global(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    let child = weak.upgrade().unwrap();
    child.borrow_mut().run_promise_jobs_bounded(8).unwrap();
    drop(child);
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_unreachable_cross_heap_wrapper_cycle_is_collectible() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.execute_script(&script("(()=>{let r=new ShadowRealm();let f=r.evaluate('(()=>{let kept;return callback=>{kept=callback;return 42;};})()');f(()=>f);return 0;})();0;")).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.shadow_realms.is_empty() && vm.shadow_wrapped_functions.is_empty());
    assert!(vm.realm_collection_cache.is_none());
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_terminal_warm_plan_reclaims_without_new_work() {
    let (mut vm, id) = fixture();
    let weak = vm.shadow_realms[&id].vm.downgrade();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: 1,
            ..Default::default()
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.collect_realm_garbage(), Ok(()));
    assert!(weak.upgrade().is_some());
    clear_global(&mut vm, "realm");
    assert_eq!(vm.collect_realm_garbage(), Ok(()));
    assert!(weak.upgrade().is_none());
    assert!(vm.realm_collection_cache.is_none());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_terminal_cold_plan_refuses_without_allocating() {
    let (mut vm, _) = fixture();
    vm.realm_collection_cache = None;
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: 1,
            ..Default::default()
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.collect_realm_garbage(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
    assert!(vm.realm_collection_cache.is_none());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_replacement_refusal_restores_the_old_cache() {
    let (mut vm, id) = fixture();
    let cache = vm.realm_collection_cache.as_ref().unwrap();
    let pointer = cache.nodes.as_ptr();
    let capacity = cache.nodes.capacity();
    let bytes = cache.storage.bytes;
    let record = vm.shadow_realms[&id].clone();
    record
        .vm
        .borrow_mut()
        .execute_script(&script("globalThis.nested=new ShadowRealm();0;"))
        .unwrap();
    drop(record);
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.prepare_realm_collection(),
        Err(RuntimeError::SchedulingLimit)
    ));
    let cache = vm.realm_collection_cache.as_ref().unwrap();
    assert_eq!(cache.nodes.as_ptr(), pointer);
    assert_eq!(cache.nodes.capacity(), capacity);
    assert_eq!(cache.storage.bytes, bytes);
    assert!(cache.nodes.iter().all(|node| node.record.is_none()));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_host_neutral_graph_releases_temporary_storage() {
    let mut vm = Vm::default();
    vm.execute_script(&script("globalThis.realm=new ShadowRealm();0;"))
        .unwrap();
    assert!(vm.realm_collection_cache.is_none());
    clear_global(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.shadow_realms.is_empty());
    assert!(vm.realm_collection_cache.is_none());
}

#[cfg_attr(test, test)]
fn realm_collection_metadata_empty_graph_needs_no_capacity_at_full_quota() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    // This case isolates realm metadata; warm the separate local collector.
    vm.collect_realm_garbage().unwrap();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(vm.collect_realm_garbage(), Ok(()));
    assert!(vm.realm_collection_cache.is_none());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_realm_collection_metadata_contracts() {
        realm_collection_metadata_refusal_precedes_heap_collection();
        realm_collection_metadata_refusal_preserves_unreachable_realm_owners();
        realm_collection_metadata_cached_and_rust_held_storage_has_one_owner();
        realm_collection_metadata_vector_refusal_preserves_prefix_and_refunds_staging();
        realm_collection_metadata_reported_set_capacity_is_checked_before_publication();
        realm_collection_metadata_partial_discovery_refunds_records_and_buffers();
        realm_collection_metadata_direct_borrow_falls_back_at_full_scalar_quota();
        realm_collection_metadata_nested_borrow_refunds_discovery_before_fallback();
        realm_collection_metadata_shared_records_are_discovered_once();
        realm_collection_metadata_duplicate_wrapper_targets_use_reserved_slots_once();
        realm_collection_metadata_external_owner_keeps_the_shared_realm_live();
        realm_collection_metadata_pending_job_keeps_its_realm_until_completion();
        realm_collection_metadata_unreachable_cross_heap_wrapper_cycle_is_collectible();
        realm_collection_metadata_terminal_warm_plan_reclaims_without_new_work();
        realm_collection_metadata_terminal_cold_plan_refuses_without_allocating();
        realm_collection_metadata_replacement_refusal_restores_the_old_cache();
        realm_collection_metadata_host_neutral_graph_releases_temporary_storage();
        realm_collection_metadata_empty_graph_needs_no_capacity_at_full_quota();
    }
}
