// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny namespace payloads verify heap adoption, capacity and collection.

use super::*;

fn padded(cell: ObjectId) -> Vec<(JsString, ObjectId)> {
    let mut units = Vec::with_capacity(32);
    units.push(u16::from(b'x'));
    let mut exports = Vec::with_capacity(8);
    exports.push((JsString::from_code_units(units), cell));
    exports
}
fn bytes(exports: &Vec<(JsString, ObjectId)>) -> usize {
    exports.capacity() * size_of::<(JsString, ObjectId)>()
        + exports
            .iter()
            .map(|(name, _)| name.owned_heap_payload_bytes().unwrap())
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_allocation_charges_retained_capacity() {
    let mut heap = Heap::default();
    let cell = heap.alloc_object(None).unwrap();
    heap.root(cell).unwrap();
    let before = heap.stats().managed_bytes;
    let empty = heap.alloc_module_namespace(Vec::new(), false).unwrap();
    heap.root(empty).unwrap();
    let base = heap.stats().managed_bytes - before;
    let exports = padded(cell);
    let expected = bytes(&exports);
    let before = heap.stats().managed_bytes;
    heap.alloc_module_namespace(exports, false).unwrap();
    assert_eq!(heap.stats().managed_bytes - before - base, expected);
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_completion_charges_retained_capacity() {
    let mut heap = Heap::default();
    let cell = heap.alloc_object(None).unwrap();
    heap.root(cell).unwrap();
    let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
    let exports = padded(cell);
    let expected = bytes(&exports);
    let before = heap.stats().managed_bytes;
    heap.initialize_module_namespace(namespace, exports)
        .unwrap();
    assert_eq!(heap.stats().managed_bytes - before, expected);
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_completed_namespace_collection_refunds_every_byte() {
    let mut heap = Heap::default();
    let cell = heap.alloc_object(None).unwrap();
    heap.root(cell).unwrap();
    let before = heap.stats().managed_bytes;
    let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
    heap.initialize_module_namespace(namespace, vec![("x".into(), cell)])
        .unwrap();
    heap.try_collect_major().unwrap();
    assert!(!heap.contains(namespace));
    assert_eq!(heap.stats().managed_bytes, before);
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_completion_gc_protects_unrooted_input_cells() {
    let mut heap = Heap::default();
    let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
    heap.root(namespace).unwrap();
    let cell = heap.alloc_object(None).unwrap();
    heap.set(cell, "value", Value::Number(3.0)).unwrap();
    heap.next_major_bytes = heap.stats().managed_bytes + 1;
    heap.initialize_module_namespace(namespace, vec![("x".into(), cell)])
        .unwrap();
    assert!(heap.contains(cell));
    assert_eq!(heap.get(namespace, "x"), Ok(Value::Number(3.0)));
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_completion_refunds_replaced_empty_backing() {
    let mut heap = Heap::default();
    let cell = heap.alloc_object(None).unwrap();
    heap.root(cell).unwrap();
    let old = Vec::with_capacity(8);
    let old_bytes = bytes(&old);
    let namespace = heap.alloc_module_namespace(old, false).unwrap();
    let exports = vec![("x".into(), cell)];
    let new_bytes = bytes(&exports);
    let before = heap.stats().managed_bytes;
    heap.initialize_module_namespace(namespace, exports)
        .unwrap();
    assert_eq!(heap.stats().managed_bytes + old_bytes, before + new_bytes);
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_minor_collection_refunds_completed_payload() {
    let mut heap = Heap::default();
    let cell = heap.alloc_object(None).unwrap();
    heap.root(cell).unwrap();
    let before = heap.stats().managed_bytes;
    let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
    heap.initialize_module_namespace(namespace, padded(cell))
        .unwrap();
    heap.try_collect_minor().unwrap();
    assert!(!heap.contains(namespace));
    assert_eq!(heap.stats().managed_bytes, before);
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_old_namespaces_remember_new_binding_cells() {
    let mut heap = Heap::default();
    let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
    heap.root(namespace).unwrap();
    heap.try_collect_major().unwrap();
    let cell = heap.alloc_object(None).unwrap();
    heap.set(cell, "value", Value::Number(7.0)).unwrap();
    heap.initialize_module_namespace(namespace, padded(cell))
        .unwrap();
    heap.try_collect_minor().unwrap();
    assert!(heap.contains(cell));
    assert_eq!(heap.get(namespace, "x"), Ok(Value::Number(7.0)));
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_completion_admits_exact_and_refuses_short_room() {
    for shared in [false, true] {
        for short in [false, true] {
            let mut heap = Heap::default();
            let cell = heap.alloc_object(None).unwrap();
            heap.root(cell).unwrap();
            let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
            heap.root(namespace).unwrap();
            let exports = padded(cell);
            let required = bytes(&exports);
            let before = heap.stats().managed_bytes;
            let limit = before + required - usize::from(short);
            heap.config.max_heap_bytes = limit;
            if shared {
                heap.begin_document_heap_budget();
            }
            let result = heap.initialize_module_namespace(namespace, exports);
            if short {
                assert_eq!(result, Err(HeapError::HeapLimitExceeded { limit }));
                assert_eq!(heap.stats().managed_bytes, before);
                assert_eq!(
                    heap.module_namespace_export_cell(namespace, &"x".into()),
                    Ok(None)
                );
            } else {
                assert_eq!(result, Ok(()));
                assert_eq!(heap.stats().managed_bytes, limit);
                assert_eq!(
                    heap.module_namespace_export_cell(namespace, &"x".into()),
                    Ok(Some(cell))
                );
            }
            if shared {
                assert_eq!(heap.document_heap_bytes(), heap.stats().managed_bytes);
            }
        }
    }
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_invalid_completion_preserves_existing_payload() {
    let mut heap = Heap::default();
    let cell = heap.alloc_object(None).unwrap();
    heap.set(cell, "value", Value::Number(3.0)).unwrap();
    let namespace = heap.alloc_module_namespace(padded(cell), false).unwrap();
    heap.root(namespace).unwrap();
    let before = heap.stats();
    heap.next_major_bytes = 0;
    assert_eq!(
        heap.initialize_module_namespace(namespace, vec![("y".into(), cell)]),
        Err(HeapError::InvalidObject(namespace))
    );
    assert_eq!(heap.stats().managed_bytes, before.managed_bytes);
    assert_eq!(heap.stats().major_collections, before.major_collections);
    assert_eq!(heap.get(namespace, "x"), Ok(Value::Number(3.0)));
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_utf16_order_preserves_owned_name_buffers() {
    for complete in [false, true] {
        let mut heap = Heap::default();
        let cell = heap.alloc_object(None).unwrap();
        heap.root(cell).unwrap();
        let exports = vec![
            ("\u{e000}".into(), cell),
            ("😀".into(), cell),
            ("x".into(), cell),
        ];
        let pointers: Vec<_> = exports
            .iter()
            .map(|(name, _): &(JsString, ObjectId)| {
                (name.as_code_units()[0], name.as_code_units().as_ptr())
            })
            .collect();
        let expected = bytes(&exports);
        let namespace = if complete {
            let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
            let before = heap.stats().managed_bytes;
            heap.initialize_module_namespace(namespace, exports)
                .unwrap();
            assert_eq!(heap.stats().managed_bytes - before, expected);
            namespace
        } else {
            heap.alloc_module_namespace(exports, false).unwrap()
        };
        let ObjectKind::ModuleNamespace { exports } = &heap.object(namespace).unwrap().kind else {
            panic!("expected namespace");
        };
        assert_eq!(exports[0].0, JsString::from("x"));
        assert_eq!(exports[1].0, JsString::from("😀"));
        assert_eq!(exports[2].0, JsString::from("\u{e000}"));
        for (name, _) in exports {
            assert!(pointers.contains(&(name.as_code_units()[0], name.as_code_units().as_ptr())));
        }
    }
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_document_collection_refunds_completed_payload() {
    let mut owner = Heap::default();
    owner.begin_document_heap_budget();
    let mut child = Heap::default();
    child.inherit_document_heap_budget(&owner).unwrap();
    let before = owner.document_heap_bytes();
    let cell = child.alloc_object(None).unwrap();
    let namespace = child.alloc_module_namespace(Vec::new(), false).unwrap();
    child
        .initialize_module_namespace(namespace, padded(cell))
        .unwrap();
    assert!(owner.document_heap_bytes() > before);
    child.try_collect_major().unwrap();
    assert_eq!(child.stats().managed_bytes, 0);
    assert_eq!(owner.document_heap_bytes(), before);
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_heap_drop_refunds_shared_document_payload() {
    let mut owner = Heap::default();
    owner.begin_document_heap_budget();
    let before = owner.document_heap_bytes();
    let mut child = Heap::default();
    child.inherit_document_heap_budget(&owner).unwrap();
    let cell = child.alloc_object(None).unwrap();
    let namespace = child.alloc_module_namespace(Vec::new(), false).unwrap();
    child
        .initialize_module_namespace(namespace, padded(cell))
        .unwrap();
    assert_eq!(
        owner.document_heap_bytes(),
        before + child.stats().managed_bytes
    );
    drop(child);
    assert_eq!(owner.document_heap_bytes(), before);
}

#[cfg_attr(test, test)]
fn namespace_payload_storage_empty_reserved_lists_stay_charged_and_collectible() {
    let mut heap = Heap::default();
    let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
    let exports = Vec::with_capacity(8);
    let required = bytes(&exports);
    let before = heap.stats().managed_bytes;
    heap.initialize_module_namespace(namespace, exports)
        .unwrap();
    assert_eq!(heap.stats().managed_bytes - before, required);
    heap.try_collect_major().unwrap();
    assert_eq!(heap.stats().managed_bytes, 0);
}

impl crate::Heap {
    /// Runs finite namespace payload adoption, budget and collection contracts.
    #[doc(hidden)]
    pub fn verify_namespace_payload_storage_contracts() {
        namespace_payload_storage_allocation_charges_retained_capacity();
        namespace_payload_storage_completion_charges_retained_capacity();
        namespace_payload_storage_completed_namespace_collection_refunds_every_byte();
        namespace_payload_storage_completion_gc_protects_unrooted_input_cells();
        namespace_payload_storage_completion_refunds_replaced_empty_backing();
        namespace_payload_storage_minor_collection_refunds_completed_payload();
        namespace_payload_storage_old_namespaces_remember_new_binding_cells();
        namespace_payload_storage_completion_admits_exact_and_refuses_short_room();
        namespace_payload_storage_invalid_completion_preserves_existing_payload();
        namespace_payload_storage_utf16_order_preserves_owned_name_buffers();
        namespace_payload_storage_document_collection_refunds_completed_payload();
        namespace_payload_storage_heap_drop_refunds_shared_document_payload();
        namespace_payload_storage_empty_reserved_lists_stay_charged_and_collectible();
    }
}
