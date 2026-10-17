// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn cleanup_selection_survives_live_cells_unregister_and_another_collection() {
    let mut heap = Heap::default();
    let registry = heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    let _registry = heap.root(registry).unwrap();
    let live = heap.alloc_object(None).unwrap();
    let _live = heap.root(live).unwrap();
    let token = heap.alloc_object(None).unwrap();
    let _token = heap.root(token).unwrap();
    for (index, keep, cancellable) in [
        (0, true, false),
        (1, false, false),
        (2, true, false),
        (3, false, false),
        (4, false, true),
        (5, true, false),
    ] {
        let target = if keep {
            live
        } else {
            heap.alloc_object(None).unwrap()
        };
        heap.finalization_registry_register(
            registry,
            Value::Object(target),
            Value::Number(index as f64),
            cancellable.then_some(Value::Object(token)),
        )
        .unwrap();
    }
    heap.collect_major();
    assert_eq!(
        heap.pending_finalization_registries().collect::<Vec<_>>(),
        [registry]
    );
    let first = heap
        .take_finalization_registry_cleanup_cell(registry)
        .unwrap()
        .unwrap()
        .1;
    assert_eq!(first, Value::Number(1.0));
    assert!(heap
        .finalization_registry_unregister(registry, Value::Object(token))
        .unwrap());
    let next = heap.alloc_object(None).unwrap();
    heap.finalization_registry_register(registry, Value::Object(next), Value::Number(6.0), None)
        .unwrap();
    heap.collect_minor();
    let mut delivered = Vec::new();
    while let Some((_, value)) = heap
        .take_finalization_registry_cleanup_cell(registry)
        .unwrap()
    {
        delivered.push(value);
    }
    assert_eq!(delivered, [Value::Number(3.0), Value::Number(6.0)]);
    assert!(heap.pending_finalization_registries().next().is_none());
    let ObjectKind::FinalizationRegistry { cells, .. } = &heap.objects[&registry].kind else {
        panic!("registry brand");
    };
    assert_eq!(cells.len(), 3);
    assert!(cells
        .iter()
        .all(|cell| cell.target == Some(WeakCollectionKey::Object(live))));
}

#[test]
fn collecting_dead_registries_releases_the_cleanup_index() {
    let mut heap = Heap::default();
    for _ in 0..128 {
        let registry = heap
            .alloc_finalization_registry(Value::Undefined, None)
            .unwrap();
        let target = heap.alloc_object(None).unwrap();
        heap.finalization_registry_register(registry, Value::Object(target), Value::Null, None)
            .unwrap();
    }
    heap.collect_major();
    assert!(heap.pending_finalization_registries().next().is_none());
    assert_eq!(heap.pending_finalization.capacity(), 0);
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn cleanup_cell_delivery_validates_the_registry_before_using_its_index() {
    let mut heap = Heap::default();
    let foreign = Heap::default().alloc_object(None).unwrap();
    assert_eq!(
        heap.take_finalization_registry_cleanup_cell(foreign),
        Err(HeapError::InvalidObject(foreign))
    );
    let ordinary = heap.alloc_object(None).unwrap();
    assert_eq!(
        heap.take_finalization_registry_cleanup_cell(ordinary),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    let registry = heap
        .alloc_finalization_registry(Value::Undefined, None)
        .unwrap();
    assert_eq!(
        heap.take_finalization_registry_cleanup_cell(registry),
        Ok(None)
    );
}
