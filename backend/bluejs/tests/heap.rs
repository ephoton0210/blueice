// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime-storage acceptance tests through BlueJS's real public API.
//! Copying an ObjectId is not a root; tests retain live objects through
//! Heap::root or through properties on an already-rooted object.

use blueice_bluejs::{Heap, HeapConfig, HeapError, PropertyDescriptor, Value};

#[test]
fn ordinary_properties_distinguish_missing_from_undefined_and_preserve_values() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    assert_eq!(heap.get_own(object, "absent").unwrap(), None);
    assert_eq!(heap.get(object, "absent").unwrap(), Value::Undefined);
    for (key, value) in [
        ("undefined", Value::Undefined),
        ("null", Value::Null),
        ("bool", Value::Bool(true)),
        ("number", Value::Number(42.5)),
        ("string", Value::String("BlueIce 冰".into())),
        ("self", Value::Object(object)),
    ] {
        heap.set(object, key, value.clone()).unwrap();
        assert_eq!(heap.get_own(object, key).unwrap(), Some(value.clone()));
        assert_eq!(heap.get(object, key).unwrap(), value);
    }
    heap.set(object, "number", Value::Number(7.0)).unwrap();
    assert_eq!(heap.get(object, "number").unwrap(), Value::Number(7.0));
    assert!(heap.delete(object, "number").unwrap());
    assert!(heap.delete(object, "missing").unwrap());
    assert_eq!(heap.get_own(object, "number").unwrap(), None);
}

#[test]
fn array_length_shrinks_and_restores_the_first_non_configurable_index() {
    let mut heap = Heap::default();
    let array = heap.alloc_array(0, None).unwrap();
    heap.set(array, "0", Value::Number(1.0)).unwrap();
    heap.set(array, "1", Value::Number(2.0)).unwrap();
    heap.set(array, "length", Value::Number(1.0)).unwrap();
    assert_eq!(heap.get(array, "length"), Ok(Value::Number(1.0)));

    heap.define_own_property(array, "1", PropertyDescriptor::data(Value::Number(2.0), true, true, false)).unwrap();
    assert_eq!(heap.set(array, "length", Value::Number(0.0)), Err(HeapError::ReadOnlyProperty));
    assert_eq!(heap.get(array, "length"), Ok(Value::Number(2.0)));
}

#[test]
fn boxed_string_own_keys_enumerate_utf16_indices_before_length() {
    let mut heap = Heap::default();
    let string = heap.alloc_string("A😀".into(), None).unwrap();
    assert_eq!(heap.own_keys(string).unwrap(), ["0", "1", "2", "length"]);
}

#[test]
fn prototype_lookup_shadows_locally_and_rejects_cycles_without_mutating() {
    let mut heap = Heap::default();
    let parent = heap.alloc_object(None).unwrap();
    heap.set(parent, "name", Value::String("parent".into())).unwrap();
    let child = heap.alloc_object(Some(parent)).unwrap();
    assert_eq!(heap.prototype(child).unwrap(), Some(parent));
    assert_eq!(heap.get_own(child, "name").unwrap(), None);
    assert_eq!(heap.get(child, "name").unwrap(), Value::String("parent".into()));
    heap.set(child, "name", Value::Undefined).unwrap();
    assert_eq!(heap.get(child, "name").unwrap(), Value::Undefined);
    assert_eq!(heap.get(parent, "name").unwrap(), Value::String("parent".into()));
    heap.delete(child, "name").unwrap();
    assert_eq!(heap.get(child, "name").unwrap(), Value::String("parent".into()));
    assert_eq!(heap.set_prototype(parent, Some(child)), Err(HeapError::PrototypeCycle));
    assert_eq!(heap.set_prototype(child, Some(child)), Err(HeapError::PrototypeCycle));
    assert_eq!(heap.prototype(parent).unwrap(), None);
    heap.set_prototype(child, None).unwrap();
    assert_eq!(heap.get(child, "name").unwrap(), Value::Undefined);
}

#[test]
fn own_keys_sort_array_indices_then_keep_string_insertion_order() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    for key in ["b", "10", "2", "01", "4294967295", "0", "4294967294", "a", "-0", "+1", "1.0", ""] {
        heap.set(object, key, Value::Null).unwrap();
    }
    heap.set(object, "b", Value::Bool(false)).unwrap();
    heap.delete(object, "01").unwrap();
    heap.set(object, "01", Value::Null).unwrap();
    assert_eq!(heap.own_keys(object).unwrap(), ["0", "2", "10", "4294967294", "b", "4294967295", "a", "-0", "+1", "1.0", "", "01"]);
}

#[test]
fn handles_are_never_reused_and_cannot_alias_an_object_in_another_heap() {
    let mut heap = Heap::default();
    let stale = heap.alloc_object(None).unwrap();
    heap.collect_major();
    let live = heap.alloc_object(None).unwrap();
    assert_ne!(stale, live);
    assert!(!heap.contains(stale));
    assert_eq!(heap.get(stale, "x"), Err(HeapError::InvalidObject(stale)));
    let mut other = Heap::default();
    let foreign = other.alloc_object(None).unwrap();
    assert_ne!(live, foreign);
    assert_eq!(heap.set(live, "x", Value::Object(foreign)), Err(HeapError::InvalidObject(foreign)));
    assert_eq!(heap.get_own(live, "x").unwrap(), None);
    assert_eq!(heap.alloc_object(Some(stale)), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.root(foreign), Err(HeapError::InvalidObject(foreign)));
}

#[test]
fn roots_preserve_transitive_cycles_until_the_last_root_is_released() {
    let mut heap = Heap::default();
    let a = heap.alloc_object(None).unwrap();
    let b = heap.alloc_object(None).unwrap();
    heap.set(a, "b", Value::Object(b)).unwrap();
    heap.set(b, "a", Value::Object(a)).unwrap();
    let first = heap.root(a).unwrap();
    let second = heap.root(a).unwrap();
    heap.collect_minor();
    assert_eq!(heap.stats().nursery_objects, 0);
    assert_eq!(heap.stats().tenured_objects, 2);
    assert_eq!(heap.get(a, "b").unwrap(), Value::Object(b));
    heap.unroot(first).unwrap();
    heap.collect_major();
    assert!(heap.contains(a) && heap.contains(b));
    heap.unroot(second).unwrap();
    heap.collect_major();
    assert!(!heap.contains(a) && !heap.contains(b));
    assert_eq!(heap.stats().managed_bytes, 0);
    assert_eq!(heap.unroot(second), Err(HeapError::InvalidRoot(second)));
}

#[test]
fn minor_gc_reclaims_unrooted_young_cycles() {
    let mut heap = Heap::default();
    let a = heap.alloc_object(None).unwrap();
    let b = heap.alloc_object(None).unwrap();
    heap.set(a, "b", Value::Object(b)).unwrap();
    heap.set(b, "a", Value::Object(a)).unwrap();
    heap.collect_minor();
    assert!(!heap.contains(a) && !heap.contains(b));
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn old_to_young_property_and_prototype_edges_survive_minor_gc() {
    let mut heap = Heap::default();
    let old = heap.alloc_object(None).unwrap();
    heap.root(old).unwrap();
    heap.collect_minor();
    let young = heap.alloc_object(None).unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    let grandchild = heap.alloc_object(None).unwrap();
    heap.set(old, "young", Value::Object(young)).unwrap();
    heap.set(young, "child", Value::Object(grandchild)).unwrap();
    heap.set(prototype, "inherited", Value::Number(3.0)).unwrap();
    heap.set_prototype(old, Some(prototype)).unwrap();
    heap.collect_minor();
    assert_eq!(heap.stats().tenured_objects, 4);
    assert_eq!(heap.get(old, "inherited").unwrap(), Value::Number(3.0));
    assert_eq!(heap.get(young, "child").unwrap(), Value::Object(grandchild));
    heap.collect_major();
    assert!(heap.contains(grandchild));
}

#[test]
fn nursery_capacity_triggers_collection_and_protects_the_allocation_prototype() {
    let mut heap = Heap::new(HeapConfig { nursery_capacity: 1, ..HeapConfig::default() }).unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    heap.set(prototype, "x", Value::Bool(true)).unwrap();
    let child = heap.alloc_object(Some(prototype)).unwrap();
    assert_eq!(heap.stats().minor_collections, 1);
    assert!(heap.contains(prototype));
    assert_eq!(heap.get(child, "x").unwrap(), Value::Bool(true));
    let root = heap.root(child).unwrap();
    heap.alloc_object(None).unwrap();
    assert!(heap.contains(child));
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert!(!heap.contains(child) && !heap.contains(prototype));
}

#[test]
fn replacing_and_deleting_remembered_edges_does_not_keep_the_old_targets_alive() {
    let mut heap = Heap::default();
    let old = heap.alloc_object(None).unwrap();
    heap.root(old).unwrap();
    heap.collect_minor();
    let replaced = heap.alloc_object(None).unwrap();
    let deleted = heap.alloc_object(None).unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    heap.set(old, "replace", Value::Object(replaced)).unwrap();
    heap.set(old, "delete", Value::Object(deleted)).unwrap();
    heap.set_prototype(old, Some(prototype)).unwrap();
    heap.set(old, "replace", Value::Null).unwrap();
    heap.delete(old, "delete").unwrap();
    heap.set_prototype(old, None).unwrap();
    heap.collect_minor();
    assert!(!heap.contains(replaced) && !heap.contains(deleted) && !heap.contains(prototype));
    assert_eq!(heap.stats().tenured_objects, 1);
}

#[test]
fn a_minor_gc_conservatively_keeps_young_edges_from_unrooted_old_objects_until_major_gc() {
    let mut heap = Heap::default();
    let old = heap.alloc_object(None).unwrap();
    let root = heap.root(old).unwrap();
    heap.collect_minor();
    heap.unroot(root).unwrap();
    let young = heap.alloc_object(None).unwrap();
    heap.set(old, "young", Value::Object(young)).unwrap();
    heap.collect_minor();
    assert!(heap.contains(old) && heap.contains(young));
    heap.collect_major();
    assert!(!heap.contains(old) && !heap.contains(young));
}

#[test]
fn managed_byte_pressure_collects_before_growing_a_property_and_preserves_its_receiver() {
    let mut heap = Heap::new(HeapConfig { major_threshold_bytes: 1024, max_heap_bytes: 4096, ..HeapConfig::default() }).unwrap();
    let garbage = heap.alloc_object(None).unwrap();
    let receiver = heap.alloc_object(None).unwrap();
    // No persistent root: the receiver is protected for this store itself.
    heap.set(receiver, "text", Value::String("x".repeat(1500).into())).unwrap();
    assert!(heap.stats().major_collections > 0);
    assert!(!heap.contains(garbage));
    assert_eq!(heap.get(receiver, "text").unwrap(), Value::String("x".repeat(1500).into()));
    let before = heap.stats().managed_bytes;
    heap.set(receiver, "text", Value::Null).unwrap();
    assert!(heap.stats().managed_bytes < before);
    heap.delete(receiver, "text").unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn exceeding_the_managed_byte_limit_returns_an_error_without_applying_the_store() {
    let mut heap = Heap::new(HeapConfig { major_threshold_bytes: 1024, max_heap_bytes: 2048, ..HeapConfig::default() }).unwrap();
    let object = heap.alloc_object(None).unwrap();
    heap.set(object, "keep", Value::Number(7.0)).unwrap();
    let before = heap.stats().managed_bytes;
    let error = Err(HeapError::HeapLimitExceeded { limit: 2048 });
    assert_eq!(heap.set(object, "keep", Value::String("x".repeat(4096).into())), error);
    assert_eq!(heap.set(object, "new", Value::String("x".repeat(4096).into())), error);
    assert_eq!(heap.stats().managed_bytes, before);
    assert_eq!(heap.get(object, "keep").unwrap(), Value::Number(7.0));
    assert_eq!(heap.own_keys(object).unwrap(), ["keep"]);
    assert!(heap.stats().major_collections >= 2);
}

#[test]
fn a_pressure_collection_protects_the_new_property_value_and_its_descendants() {
    let mut heap = Heap::new(HeapConfig { major_threshold_bytes: 1024, max_heap_bytes: 8192, ..HeapConfig::default() }).unwrap();
    let object = heap.alloc_object(None).unwrap();
    let root = heap.root(object).unwrap();
    let child = heap.alloc_object(None).unwrap();
    let grandchild = heap.alloc_object(None).unwrap();
    heap.set(child, "child", Value::Object(grandchild)).unwrap();
    let key = "k".repeat(1000);
    heap.set(object, &key, Value::Object(child)).unwrap();
    assert!(heap.stats().major_collections > 0);
    assert_eq!(heap.get(child, "child").unwrap(), Value::Object(grandchild));
    heap.collect_minor();
    heap.collect_major();
    assert!(heap.contains(child) && heap.contains(grandchild));
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().tenured_objects, 0);
}

#[test]
fn allocation_at_the_limit_collects_garbage_but_cannot_evict_rooted_objects() {
    let mut heap = Heap::new(HeapConfig { nursery_capacity: 2, major_threshold_bytes: 512, max_heap_bytes: 1024 }).unwrap();
    let mut live = Vec::new();
    loop {
        match heap.alloc_object(None) {
            Ok(object) => {
                live.push((object, heap.root(object).unwrap()));
                assert!(live.len() < 100, "the managed heap must be bounded");
            }
            Err(error) => {
                assert_eq!(error, HeapError::HeapLimitExceeded { limit: 1024 });
                break;
            }
        }
    }
    assert!(heap.stats().managed_bytes <= 1024);
    assert!(live.iter().all(|(id, _)| heap.contains(*id)));
    for (_, root) in &live {
        heap.unroot(*root).unwrap();
    }
    let replacement = heap.alloc_object(None).unwrap();
    assert!(heap.contains(replacement));
    assert!(live.iter().all(|(id, _)| !heap.contains(*id)));
}

#[test]
fn invalid_heap_configuration_is_reported_before_allocation() {
    for config in [
        HeapConfig { nursery_capacity: 0, ..HeapConfig::default() },
        HeapConfig { major_threshold_bytes: 0, ..HeapConfig::default() },
        HeapConfig { max_heap_bytes: 1, major_threshold_bytes: 1, ..HeapConfig::default() },
        HeapConfig { max_heap_bytes: 1024, major_threshold_bytes: 2048, ..HeapConfig::default() },
    ] {
        assert!(matches!(Heap::new(config), Err(HeapError::InvalidConfig)));
    }
}

#[test]
fn every_object_entry_point_rejects_a_collected_receiver() {
    let mut heap = Heap::default();
    let stale = heap.alloc_object(None).unwrap();
    heap.collect_major();
    let live = heap.alloc_object(None).unwrap();
    assert_eq!(heap.get_own(stale, "x"), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.set(stale, "x", Value::Null), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.delete(stale, "x"), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.own_keys(stale), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.prototype(stale), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.set_prototype(stale, None), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.set_prototype(live, Some(stale)), Err(HeapError::InvalidObject(stale)));
    assert_eq!(heap.prototype(live).unwrap(), None);
}

#[test]
fn stale_and_foreign_root_registrations_cannot_release_a_live_root() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    let stale = heap.root(object).unwrap();
    assert_eq!(heap.unroot(stale).unwrap(), object);
    let root = heap.root(object).unwrap();
    assert_ne!(root, stale);
    let mut other = Heap::default();
    let foreign_object = other.alloc_object(None).unwrap();
    let foreign_root = other.root(foreign_object).unwrap();
    assert_eq!(heap.unroot(stale), Err(HeapError::InvalidRoot(stale)));
    assert_eq!(heap.unroot(foreign_root), Err(HeapError::InvalidRoot(foreign_root)));
    heap.collect_major();
    assert!(heap.contains(object));
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert!(!heap.contains(object));
}

#[test]
fn allocation_protects_its_prototype_through_minor_then_major_gc() {
    // Derive the platform-dependent empty-object charge through the public
    // counter; the test concerns exact capacity boundaries, not struct layout.
    let mut probe = Heap::default();
    probe.alloc_object(None).unwrap();
    let object_bytes = probe.stats().managed_bytes;
    let mut heap = Heap::new(HeapConfig { nursery_capacity: 1, major_threshold_bytes: 2 * object_bytes, max_heap_bytes: 8 * object_bytes }).unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    let child = heap.alloc_object(Some(prototype)).unwrap();
    assert_eq!(heap.stats().minor_collections, 1);
    assert_eq!(heap.stats().major_collections, 1);
    assert_eq!(heap.prototype(child).unwrap(), Some(prototype));
    assert!(heap.contains(prototype));
    assert_eq!(heap.stats().nursery_objects, 1);
    assert_eq!(heap.stats().tenured_objects, 1);
}

#[test]
fn major_gc_traces_a_prototype_only_chain_and_resets_the_growth_threshold() {
    let mut heap = Heap::new(HeapConfig { major_threshold_bytes: 1024, max_heap_bytes: 8192, ..HeapConfig::default() }).unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    let root = heap.root(prototype).unwrap();
    heap.set(prototype, "payload", Value::String("x".repeat(1500).into())).unwrap();
    let child = heap.alloc_object(Some(prototype)).unwrap();
    let child_root = heap.root(child).unwrap();
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert!(heap.contains(prototype));
    assert_eq!(heap.stats().next_major_bytes, 2 * heap.stats().managed_bytes);
    assert_eq!(heap.get(child, "payload").unwrap(), Value::String("x".repeat(1500).into()));
    heap.unroot(child_root).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().next_major_bytes, 1024);
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn write_barriers_work_again_after_each_promotion_and_after_major_gc() {
    let mut heap = Heap::default();
    let parent = heap.alloc_object(None).unwrap();
    let root = heap.root(parent).unwrap();
    heap.collect_major();
    for _ in 0..4 {
        let property = heap.alloc_object(None).unwrap();
        let prototype = heap.alloc_object(None).unwrap();
        heap.set(parent, "child", Value::Object(property)).unwrap();
        heap.set_prototype(parent, Some(prototype)).unwrap();
        heap.collect_minor();
        assert!(heap.contains(property) && heap.contains(prototype));
        heap.delete(parent, "child").unwrap();
        heap.set_prototype(parent, None).unwrap();
        heap.collect_major();
        assert!(!heap.contains(property) && !heap.contains(prototype));
    }
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn a_deep_graph_survives_automatic_collections_and_is_marked_without_recursion() {
    let mut heap = Heap::default();
    let head = heap.alloc_object(None).unwrap();
    let root = heap.root(head).unwrap();
    let mut tail = head;
    for _ in 0..10_000 {
        let next = heap.alloc_object(None).unwrap();
        heap.set(tail, "next", Value::Object(next)).unwrap();
        tail = next;
    }
    heap.set(tail, "next", Value::Object(head)).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().tenured_objects, 10_001);
    assert!(heap.stats().minor_collections > 1 && heap.stats().major_collections > 1);
    assert_eq!(heap.get(tail, "next").unwrap(), Value::Object(head));
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn gc_reachability_matches_an_independent_graph_model() {
    // Model reachability by repeated boolean propagation, independently of
    // the collector's handle table, remembered set and worklist traversal.
    for nursery_capacity in [1, 7, 256] {
        let mut heap = Heap::new(HeapConfig { nursery_capacity, ..HeapConfig::default() }).unwrap();
        let mut ids = Vec::new();
        let mut roots = Vec::new();
        for _ in 0..40 {
            let id = heap.alloc_object(None).unwrap();
            roots.push(heap.root(id).unwrap());
            ids.push(id);
        }
        let mut edges = vec![Vec::new(); ids.len()];
        for i in 0..ids.len() {
            // Four disconnected components, with cycles and multiple paths
            // to each object; only the first two components will stay rooted.
            let component = i / 10 * 10;
            for target in [component + (i + 1) % 10, component + (i * 3 + 2) % 10] {
                heap.set(ids[i], target.to_string(), Value::Object(ids[target])).unwrap();
                edges[i].push(target);
            }
        }
        let mut reachable = vec![false; ids.len()];
        reachable[0] = true;
        reachable[10] = true;
        for (i, root) in roots.iter().enumerate() {
            if !reachable[i] {
                heap.unroot(*root).unwrap();
            }
        }
        for _ in 0..ids.len() {
            for (from, targets) in edges.iter().enumerate() {
                if reachable[from] {
                    for &target in targets {
                        reachable[target] = true;
                    }
                }
            }
        }
        heap.collect_minor();
        heap.collect_major();
        for (id, expected) in ids.iter().zip(reachable.iter()) {
            assert_eq!(heap.contains(*id), *expected, "nursery capacity {nursery_capacity}, object {id:?}");
        }
        heap.unroot(roots[0]).unwrap();
        heap.unroot(roots[10]).unwrap();
        heap.collect_major();
        assert_eq!(heap.stats().managed_bytes, 0);
    }
}

#[test]
fn heap_errors_have_usable_diagnostics() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    let root = heap.root(object).unwrap();
    for (error, detail) in [
        (HeapError::InvalidConfig, "configuration".to_string()),
        (HeapError::InvalidObject(object), format!("object: {object:?}")),
        (HeapError::InvalidRoot(root), format!("root: {root:?}")),
        (HeapError::PrototypeCycle, "prototype chain cannot contain a cycle".to_string()),
        (HeapError::HeapLimitExceeded { limit: 1024 }, "1024 bytes".to_string()),
        (HeapError::IdExhausted, "identity counter exhausted".to_string()),
    ] {
        let error: &dyn std::error::Error = &error;
        assert!(error.to_string().contains(&detail));
        assert!(error.source().is_none());
    }
}
