// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny cells and managed byte allowances verify actual backing ownership.

use super::*;

fn fixture() -> (Heap, ObjectId, ObjectId, ObjectId) {
    let mut heap = Heap::default();
    let registry = heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    let target = heap.alloc_object(None).unwrap();
    let token = heap.alloc_object(None).unwrap();
    for object in [registry, target, token] {
        heap.root(object).unwrap();
    }
    (heap, registry, target, token)
}
fn register(
    heap: &mut Heap,
    registry: ObjectId,
    target: ObjectId,
    token: Option<ObjectId>,
) -> Result<(), HeapError> {
    heap.finalization_registry_register(
        registry,
        Value::Object(target),
        Value::Undefined,
        token.map(Value::Object),
    )
}
fn slot_bytes() -> usize {
    Heap::finalization_cell_slot_bytes()
}

#[cfg_attr(test, test)]
fn finalization_backing_reported_capacity_has_a_managed_owner() {
    let (mut heap, registry, target, _) = fixture();
    let before = heap.stats().managed_bytes;
    register(&mut heap, registry, target, None).unwrap();
    let (_, capacity) = heap.finalization_cell_storage_state(registry);
    assert!(capacity > 1);
    assert_eq!(heap.stats().managed_bytes - before, capacity * slot_bytes());
}

#[cfg_attr(test, test)]
fn finalization_backing_full_slot_admission_precedes_registration() {
    let (mut heap, registry, target, _) = fixture();
    let limit = heap.allow_only(slot_bytes());
    assert_eq!(
        register(&mut heap, registry, target, None),
        Err(HeapError::HeapLimitExceeded { limit })
    );
    assert_eq!(heap.finalization_cell_storage_state(registry), (0, 0));
    assert!(heap.contains(target));
}

#[cfg_attr(test, test)]
fn finalization_backing_partial_removal_keeps_allocated_slots_charged() {
    let (mut heap, registry, target, token) = fixture();
    register(&mut heap, registry, target, Some(token)).unwrap();
    register(&mut heap, registry, target, None).unwrap();
    let before = heap.stats().managed_bytes;
    let capacity = heap.finalization_cell_storage_state(registry).1;
    assert_eq!(
        heap.finalization_registry_unregister(registry, Value::Object(token)),
        Ok(true)
    );
    assert_eq!(
        heap.finalization_cell_storage_state(registry),
        (1, capacity)
    );
    assert_eq!(heap.stats().managed_bytes, before);
}

#[cfg_attr(test, test)]
fn finalization_backing_exact_initial_capacity_allowance_publishes_actual_bytes() {
    let (mut heap, registry, target, _) = fixture();
    let before = heap.stats().managed_bytes;
    heap.allow_only(4 * slot_bytes());
    register(&mut heap, registry, target, None).unwrap();
    assert_eq!(heap.finalization_cell_storage_state(registry), (1, 4));
    assert_eq!(heap.stats().managed_bytes, before + 4 * slot_bytes());
}

#[cfg_attr(test, test)]
fn finalization_backing_one_byte_short_refuses_before_vector_publication() {
    let (mut heap, registry, target, _) = fixture();
    let before = heap.stats().managed_bytes;
    let limit = heap.allow_only(4 * slot_bytes() - 1);
    assert_eq!(
        register(&mut heap, registry, target, None),
        Err(HeapError::HeapLimitExceeded { limit })
    );
    assert_eq!(heap.finalization_cell_storage_state(registry), (0, 0));
    assert_eq!(heap.stats().managed_bytes, before);
}

#[cfg_attr(test, test)]
fn finalization_backing_spare_slot_accepts_zero_payload_at_full_heap_allowance() {
    let (mut heap, registry, target, _) = fixture();
    register(&mut heap, registry, target, None).unwrap();
    let before = heap.stats();
    heap.allow_only(0);
    register(&mut heap, registry, target, None).unwrap();
    assert_eq!(heap.stats().managed_bytes, before.managed_bytes);
    assert_eq!(heap.stats().major_collections, before.major_collections);
    assert_eq!(heap.finalization_cell_storage_state(registry), (2, 4));
}

#[cfg_attr(test, test)]
fn finalization_backing_spare_slot_charges_only_new_primitive_payload() {
    let (mut heap, registry, target, _) = fixture();
    register(&mut heap, registry, target, None).unwrap();
    let before = heap.stats().managed_bytes;
    heap.allow_only(8);
    heap.finalization_registry_register(
        registry,
        Value::Object(target),
        Value::String("tiny".into()),
        None,
    )
    .unwrap();
    assert_eq!(heap.finalization_cell_storage_state(registry), (2, 4));
    assert_eq!(heap.stats().managed_bytes, before + 8);
}

#[cfg_attr(test, test)]
fn finalization_backing_spare_slot_payload_refusal_keeps_the_original_cells() {
    let (mut heap, registry, target, _) = fixture();
    register(&mut heap, registry, target, None).unwrap();
    let before = heap.stats().managed_bytes;
    let limit = heap.allow_only(7);
    assert_eq!(
        heap.finalization_registry_register(
            registry,
            Value::Object(target),
            Value::String("tiny".into()),
            None
        ),
        Err(HeapError::HeapLimitExceeded { limit })
    );
    assert_eq!(heap.finalization_cell_storage_state(registry), (1, 4));
    assert_eq!(heap.stats().managed_bytes, before);
}

#[cfg_attr(test, test)]
fn finalization_backing_replacement_requires_complete_coexisting_capacity() {
    for accepts in [false, true] {
        let (mut heap, registry, target, token) = fixture();
        for _ in 0..4 {
            register(&mut heap, registry, target, Some(token)).unwrap();
        }
        let before = heap.stats().managed_bytes;
        let room = if accepts { 8 } else { 4 } * slot_bytes();
        let limit = heap.allow_only(room);
        let result = register(&mut heap, registry, target, Some(token));
        if accepts {
            assert_eq!(result, Ok(()));
            assert_eq!(heap.finalization_cell_storage_state(registry), (5, 8));
            assert_eq!(heap.stats().managed_bytes, before + 4 * slot_bytes());
        } else {
            assert_eq!(result, Err(HeapError::HeapLimitExceeded { limit }));
            assert_eq!(heap.finalization_cell_storage_state(registry), (4, 4));
            assert_eq!(heap.stats().managed_bytes, before);
        }
        assert!(heap
            .finalization_registry_unregister(registry, Value::Object(token))
            .unwrap());
        assert_eq!(heap.finalization_cell_storage_state(registry), (0, 0));
    }
}

#[cfg_attr(test, test)]
fn finalization_backing_partial_unregister_refunds_payload_without_refunding_slots() {
    let (mut heap, registry, target, token) = fixture();
    heap.finalization_registry_register(
        registry,
        Value::Object(target),
        Value::String("tiny".into()),
        Some(Value::Object(token)),
    )
    .unwrap();
    register(&mut heap, registry, target, None).unwrap();
    let before = heap.stats().managed_bytes;
    assert!(heap
        .finalization_registry_unregister(registry, Value::Object(token))
        .unwrap());
    assert_eq!(heap.finalization_cell_storage_state(registry), (1, 4));
    assert_eq!(heap.stats().managed_bytes, before - 8);
}

#[cfg_attr(test, test)]
fn finalization_backing_final_unregister_releases_all_slots_and_payload() {
    let (mut heap, registry, target, token) = fixture();
    let before = heap.stats().managed_bytes;
    for _ in 0..2 {
        heap.finalization_registry_register(
            registry,
            Value::Object(target),
            Value::String("tiny".into()),
            Some(Value::Object(token)),
        )
        .unwrap();
    }
    assert_eq!(heap.stats().managed_bytes, before + 4 * slot_bytes() + 16);
    assert!(heap
        .finalization_registry_unregister(registry, Value::Object(token))
        .unwrap());
    assert_eq!(heap.finalization_cell_storage_state(registry), (0, 0));
    assert_eq!(heap.stats().managed_bytes, before);
}

fn ready_fixture(live_last: bool) -> (Heap, ObjectId) {
    let mut heap = Heap::default();
    let registry = heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    heap.root(registry).unwrap();
    for index in 0..2 {
        let target = heap.alloc_object(None).unwrap();
        if live_last && index == 1 {
            heap.root(target).unwrap();
        }
        let holdings = if index == 0 {
            Value::String("tiny".into())
        } else {
            Value::Undefined
        };
        heap.finalization_registry_register(registry, Value::Object(target), holdings, None)
            .unwrap();
    }
    heap.try_collect_major().unwrap();
    (heap, registry)
}

#[cfg_attr(test, test)]
fn finalization_backing_delivery_refunds_payload_then_the_final_allocation() {
    let (mut heap, registry) = ready_fixture(false);
    let before = heap.stats().managed_bytes;
    let first = heap
        .take_finalization_registry_cleanup_cell(registry)
        .unwrap()
        .unwrap()
        .1;
    assert_eq!(first, Value::String("tiny".into()));
    assert_eq!(heap.finalization_cell_storage_state(registry), (1, 4));
    assert_eq!(heap.stats().managed_bytes, before - 8);
    heap.take_finalization_registry_cleanup_cell(registry)
        .unwrap()
        .unwrap();
    assert_eq!(heap.finalization_cell_storage_state(registry), (0, 0));
    assert_eq!(heap.stats().managed_bytes, before - 8 - 4 * slot_bytes());
}

#[cfg_attr(test, test)]
fn finalization_backing_standalone_partial_drain_retains_the_live_backing_owner() {
    let (mut heap, registry) = ready_fixture(true);
    let before = heap.stats().managed_bytes;
    // Refused publication must leave the source cell and its bytes intact.
    assert_eq!(
        heap.drain_finalization_registry_cleanup_jobs(|_, holdings| Err(((), holdings))),
        Err(())
    );
    assert_eq!(heap.finalization_cell_storage_state(registry), (2, 4));
    assert_eq!(heap.stats().managed_bytes, before);
    let mut delivered = Vec::new();
    heap.drain_finalization_registry_cleanup_jobs(|_, holdings| {
        delivered.push(holdings);
        Ok::<_, ((), Value)>(())
    })
    .unwrap();
    assert_eq!(delivered, [Value::String("tiny".into())]);
    assert_eq!(heap.finalization_cell_storage_state(registry), (1, 4));
    assert_eq!(heap.stats().managed_bytes, before - 8);
}

#[cfg_attr(test, test)]
fn finalization_backing_standalone_empty_drain_releases_the_allocation() {
    let (mut heap, registry) = ready_fixture(false);
    let before = heap.stats().managed_bytes;
    let mut delivered = Vec::new();
    heap.drain_finalization_registry_cleanup_jobs(|_, holdings| {
        delivered.push(holdings);
        Ok::<_, ((), Value)>(())
    })
    .unwrap();
    assert_eq!(delivered, [Value::String("tiny".into()), Value::Undefined]);
    assert_eq!(heap.finalization_cell_storage_state(registry), (0, 0));
    assert_eq!(heap.stats().managed_bytes, before - 8 - 4 * slot_bytes());
}

#[cfg_attr(test, test)]
fn finalization_backing_dead_registry_collection_releases_the_complete_charge() {
    let mut heap = Heap::default();
    let registry = heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    let root = heap.root(registry).unwrap();
    let target = heap.alloc_object(None).unwrap();
    heap.finalization_registry_register(
        registry,
        Value::Object(target),
        Value::String("tiny".into()),
        None,
    )
    .unwrap();
    heap.try_collect_major().unwrap();
    heap.unroot(root).unwrap();
    heap.try_collect_major().unwrap();
    assert_eq!(heap.stats().managed_bytes, 0);
    assert!(!heap.contains(registry));
}

type RegistryIds = (ObjectId, ObjectId, ObjectId);
fn document_pair(extra: usize) -> (Heap, Heap, RegistryIds, RegistryIds) {
    let (mut owner, registry, target, token) = fixture();
    let (mut child, child_registry, child_target, child_token) = fixture();
    owner.allow_only(child.stats().managed_bytes + extra);
    owner.begin_document_heap_budget();
    child.inherit_document_heap_budget(&owner).unwrap();
    (
        owner,
        child,
        (registry, target, token),
        (child_registry, child_target, child_token),
    )
}

#[cfg_attr(test, test)]
fn finalization_backing_rust_held_child_preserves_its_document_charge() {
    let (mut owner, registry, target, _) = fixture();
    register(&mut owner, registry, target, None).unwrap();
    owner.begin_document_heap_budget();
    let (mut child, registry, target, _) = fixture();
    register(&mut child, registry, target, None).unwrap();
    child.inherit_document_heap_budget(&owner).unwrap();
    let bytes = child.stats().managed_bytes;
    let mut observed = Heap::default();
    observed.inherit_document_heap_budget(&owner).unwrap();
    drop(owner);
    assert_eq!(observed.document_heap_bytes(), bytes);
    drop(child);
    assert_eq!(observed.document_heap_bytes(), 0);
}

#[cfg_attr(test, test)]
fn finalization_backing_shared_document_refuses_another_backing_allocation() {
    let (mut owner, mut child, (registry, target, _), (child_registry, child_target, _)) =
        document_pair(4 * slot_bytes());
    register(&mut owner, registry, target, None).unwrap();
    let before = owner.document_heap_bytes();
    let limit = owner.document_heap_budget().unwrap().1;
    assert_eq!(before, limit);
    assert_eq!(
        register(&mut child, child_registry, child_target, None),
        Err(HeapError::HeapLimitExceeded { limit })
    );
    assert_eq!(owner.document_heap_bytes(), before);
    assert_eq!(
        child.finalization_cell_storage_state(child_registry),
        (0, 0)
    );
    assert_eq!(owner.finalization_cell_storage_state(registry), (1, 4));
}

#[cfg_attr(test, test)]
fn finalization_backing_shared_document_spare_slots_add_no_header_charge() {
    let (mut owner, child, (registry, target, token), _) = document_pair(4 * slot_bytes());
    register(&mut owner, registry, target, Some(token)).unwrap();
    let before = owner.document_heap_bytes();
    register(&mut owner, registry, target, None).unwrap();
    assert!(owner
        .finalization_registry_unregister(registry, Value::Object(token))
        .unwrap());
    assert_eq!(owner.finalization_cell_storage_state(registry), (1, 4));
    assert_eq!(owner.document_heap_bytes(), before);
    assert_eq!(child.document_heap_bytes(), before);
}

#[cfg_attr(test, test)]
fn finalization_backing_shared_document_reuses_released_allocation_bytes() {
    let (mut owner, mut child, (registry, target, token), (child_registry, child_target, _)) =
        document_pair(4 * slot_bytes());
    register(&mut owner, registry, target, Some(token)).unwrap();
    let before = owner.document_heap_bytes();
    assert!(owner
        .finalization_registry_unregister(registry, Value::Object(token))
        .unwrap());
    assert_eq!(owner.document_heap_bytes(), before - 4 * slot_bytes());
    register(&mut child, child_registry, child_target, None).unwrap();
    assert_eq!(owner.document_heap_bytes(), before);
}

#[cfg_attr(test, test)]
fn finalization_backing_late_document_attachment_counts_capacity_once() {
    let (mut owner, _, _, _) = fixture();
    owner.begin_document_heap_budget();
    let (mut child, registry, target, _) = fixture();
    register(&mut child, registry, target, None).unwrap();
    let expected = owner.stats().managed_bytes + child.stats().managed_bytes;
    child.inherit_document_heap_budget(&owner).unwrap();
    child.inherit_document_heap_budget(&owner).unwrap();
    assert_eq!(owner.document_heap_bytes(), expected);
    let mut other = Heap::default();
    other.begin_document_heap_budget();
    let before = other.document_heap_bytes();
    assert_eq!(
        child.inherit_document_heap_budget(&other),
        Err(HeapError::InvalidConfig)
    );
    assert_eq!(owner.document_heap_bytes(), expected);
    assert_eq!(other.document_heap_bytes(), before);
}

#[cfg_attr(test, test)]
fn finalization_backing_registration_protects_all_inputs_during_collection() {
    let (mut heap, registry, target, token) = fixture();
    let holdings = heap.alloc_object(None).unwrap();
    let before = heap.stats().major_collections;
    heap.allow_only(4 * slot_bytes());
    heap.finalization_registry_register(
        registry,
        Value::Object(target),
        Value::Object(holdings),
        Some(Value::Object(token)),
    )
    .unwrap();
    assert!(heap.stats().major_collections > before);
    assert!(heap.contains(holdings));
    assert!(heap.contains(target));
    assert!(heap.contains(token));
    assert_eq!(heap.finalization_cell_storage_state(registry), (1, 4));
}

#[cfg_attr(test, test)]
fn finalization_backing_invalid_registry_refuses_before_collection() {
    let (mut heap, _, target, token) = fixture();
    let ordinary = heap.alloc_object(None).unwrap();
    heap.root(ordinary).unwrap();
    heap.allow_only(0);
    let before = heap.stats();
    assert_eq!(
        register(&mut heap, ordinary, target, Some(token)),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(heap.stats(), before);
}

#[cfg_attr(test, test)]
fn finalization_backing_invalid_target_and_token_refuse_before_admission() {
    let (mut heap, registry, target, _) = fixture();
    heap.allow_only(0);
    let before = heap.stats();
    let foreign = Heap::default().alloc_object(None).unwrap();
    assert_eq!(
        register(&mut heap, registry, foreign, None),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(
        register(&mut heap, registry, target, Some(foreign)),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(
        heap.finalization_registry_register(registry, Value::Number(0.0), Value::Undefined, None),
        Err(HeapError::InvalidWeakTarget)
    );
    assert_eq!(heap.stats(), before);
}

#[cfg_attr(test, test)]
fn finalization_backing_terminal_document_cleanup_refunds_actual_backing() {
    let (mut heap, registry, target, token) = fixture();
    heap.allow_only(4 * slot_bytes() + 8);
    heap.begin_document_heap_budget();
    heap.finalization_registry_register(
        registry,
        Value::Object(target),
        Value::String("tiny".into()),
        Some(Value::Object(token)),
    )
    .unwrap();
    let before = heap.document_heap_bytes();
    assert!(matches!(
        heap.finalization_registry_register(
            registry,
            Value::Object(target),
            Value::String("tiny".into()),
            None
        ),
        Err(HeapError::HeapLimitExceeded { .. })
    ));
    assert!(heap.check_document_heap_budget().is_err());
    assert!(heap
        .finalization_registry_unregister(registry, Value::Object(token))
        .unwrap());
    assert_eq!(heap.document_heap_bytes(), before - 4 * slot_bytes() - 8);
    assert_eq!(heap.finalization_cell_storage_state(registry), (0, 0));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_finalization_backing_contracts() {
        finalization_backing_reported_capacity_has_a_managed_owner();
        finalization_backing_full_slot_admission_precedes_registration();
        finalization_backing_partial_removal_keeps_allocated_slots_charged();
        finalization_backing_exact_initial_capacity_allowance_publishes_actual_bytes();
        finalization_backing_one_byte_short_refuses_before_vector_publication();
        finalization_backing_spare_slot_accepts_zero_payload_at_full_heap_allowance();
        finalization_backing_spare_slot_charges_only_new_primitive_payload();
        finalization_backing_spare_slot_payload_refusal_keeps_the_original_cells();
        finalization_backing_replacement_requires_complete_coexisting_capacity();
        finalization_backing_partial_unregister_refunds_payload_without_refunding_slots();
        finalization_backing_final_unregister_releases_all_slots_and_payload();
        finalization_backing_delivery_refunds_payload_then_the_final_allocation();
        finalization_backing_standalone_partial_drain_retains_the_live_backing_owner();
        finalization_backing_standalone_empty_drain_releases_the_allocation();
        finalization_backing_dead_registry_collection_releases_the_complete_charge();
        finalization_backing_rust_held_child_preserves_its_document_charge();
        finalization_backing_shared_document_refuses_another_backing_allocation();
        finalization_backing_shared_document_spare_slots_add_no_header_charge();
        finalization_backing_shared_document_reuses_released_allocation_bytes();
        finalization_backing_late_document_attachment_counts_capacity_once();
        finalization_backing_registration_protects_all_inputs_during_collection();
        finalization_backing_invalid_registry_refuses_before_collection();
        finalization_backing_invalid_target_and_token_refuse_before_admission();
        finalization_backing_terminal_document_cleanup_refunds_actual_backing();
    }
}
