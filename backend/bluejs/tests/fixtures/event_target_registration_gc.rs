// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Borrowed event candidates keep pending edges through normal collection.

use super::*;

#[test]
fn event_registration_gc_borrowed_store_keeps_pending_edges_then_releases_them() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let transfer = heap
        .alloc_drag_store(DragStore::new(DragStoreMode::ReadWrite), prototype)
        .unwrap();
    let file = heap
        .alloc_web_blob(
            WebBlobData {
                bytes: vec![7],
                media_type: "x".into(),
                file: Some(("a".into(), 0)),
            },
            prototype,
        )
        .unwrap();
    let wrapper = heap.alloc_object(None).unwrap();
    let image = heap.alloc_object(None).unwrap();
    let types = heap.alloc_array(0, None).unwrap();
    let items = heap.alloc_object(None).unwrap();
    let files = heap.alloc_object(None).unwrap();
    let mut pending = DragStore::new(DragStoreMode::ReadWrite);
    pending.items.push(DragItem {
        id: 1,
        format: "x".into(),
        data: DragItemData::File(file),
        wrapper: Some(wrapper),
    });
    pending.image = Some((image, 1, 2));
    pending.types = Some(types);
    pending.item_list = Some(items);
    pending.file_list = Some(files);
    heap.major_gc(ProtectedRoots::DragStore(transfer, &pending))
        .unwrap();
    for id in [file, wrapper, image, types, items, files] {
        assert!(heap.contains(id));
    }
    assert!(heap.drag_store(transfer).unwrap().items.is_empty());
    heap.replace_drag_store(transfer, pending).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[transfer])).unwrap();
    for id in [file, wrapper, image, types, items, files] {
        assert!(heap.contains(id));
    }
    heap.replace_drag_store(transfer, DragStore::new(DragStoreMode::ReadWrite))
        .unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[transfer])).unwrap();
    for id in [file, wrapper, image, types, items, files] {
        assert!(!heap.contains(id));
    }
}

#[test]
fn event_registration_gc_disabled_store_releases_files_types_and_image_but_keeps_slots_paid() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let file = heap
        .alloc_web_blob(
            WebBlobData {
                bytes: vec![7],
                media_type: "text/plain".into(),
                file: Some(("one.txt".into(), 0)),
            },
            prototype,
        )
        .unwrap();
    let image = heap.alloc_object(None).unwrap();
    let types = heap.alloc_array(0, None).unwrap();
    let mut store = DragStore::new(DragStoreMode::ReadOnly);
    store.items = Vec::with_capacity(5);
    store.items.push(DragItem {
        id: 1,
        format: "text/plain".into(),
        data: DragItemData::File(file),
        wrapper: None,
    });
    store.types = Some(types);
    store.image = Some((image, 4, -3));
    let transfer = heap.alloc_drag_store(store, prototype).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[transfer])).unwrap();
    assert!(heap.contains(file) && heap.contains(image) && heap.contains(types));
    heap.disable_drag_store(transfer).unwrap();
    let store = heap.drag_store(transfer).unwrap();
    assert!(store.mode == DragStoreMode::Disabled && store.items.is_empty());
    assert!(store.types.is_none() && store.image.is_none() && store.items.capacity() >= 5);
    assert_eq!(
        store.bytes(),
        size_of::<DragStore>()
            + store.items.capacity() * size_of::<DragItem>()
            + store.drop_effect.owned_heap_payload_bytes().unwrap()
            + store.effect_allowed.owned_heap_payload_bytes().unwrap()
    );
    heap.major_gc(ProtectedRoots::Slice(&[transfer])).unwrap();
    assert!(!heap.contains(file) && !heap.contains(image) && !heap.contains(types));
}

#[test]
fn event_registration_gc_borrowed_node_path_keeps_ancestors_until_terminal_cleanup() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let mut state = DragEventState::new(0);
    state.set("target", Value::Null);
    state.set("isTrusted", Value::Bool(false));
    let event = heap.alloc_drag_event(state, prototype).unwrap();
    let target = heap.alloc_object(None).unwrap();
    let parent = heap.alloc_object(None).unwrap();
    let document = heap.alloc_object(None).unwrap();
    let mut path = Vec::with_capacity(5);
    path.extend([target, parent, document]);
    heap.major_gc(ProtectedRoots::EventPath(event, &path))
        .unwrap();
    heap.begin_node_event_dispatch(event, target, true, path)
        .unwrap();
    heap.set_event_phase(event, document, 1).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[event])).unwrap();
    assert!(heap.contains(target) && heap.contains(parent) && heap.contains(document));
    heap.finish_event_dispatch(event).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[event])).unwrap();
    assert!(heap.contains(target));
    assert!(!heap.contains(parent) && !heap.contains(document));
    let state = heap.drag_event(event).unwrap();
    assert_eq!(state.phase, 0);
    assert!(state.current_target.is_none() && state.path.is_empty() && state.path.capacity() >= 5);
}

#[test]
fn event_registration_gc_borrowed_event_keeps_every_pending_edge_then_releases_it() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let event = heap
        .alloc_drag_event(DragEventState::new(0), prototype)
        .unwrap();
    let detail = heap.alloc_object(None).unwrap();
    let path_target = heap.alloc_web_event_target(prototype).unwrap();
    let current_target = heap.alloc_web_event_target(prototype).unwrap();
    let mut pending = DragEventState::new(0);
    pending.set("detail", Value::Object(detail));
    pending.path.push(path_target);
    pending.current_target = Some(current_target);
    heap.major_gc(ProtectedRoots::Event(event, &pending))
        .unwrap();
    assert!(heap.contains(detail) && heap.contains(path_target) && heap.contains(current_target));
    heap.set_drag_event_state(event, pending).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[event])).unwrap();
    assert!(heap.contains(detail) && heap.contains(path_target) && heap.contains(current_target));
    heap.set_drag_event_state(event, DragEventState::new(0))
        .unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[event])).unwrap();
    assert!(
        !heap.contains(detail) && !heap.contains(path_target) && !heap.contains(current_target)
    );
}

#[test]
fn event_registration_gc_borrowed_target_keeps_pending_edges_then_releases_them() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let target = heap.alloc_web_event_target(prototype).unwrap();
    let callback = heap.alloc_object(None).unwrap();
    let signal = heap.alloc_abort_signal(prototype).unwrap();
    let discarded = heap.alloc_object(None).unwrap();
    let mut pending = WebTargetData::default();
    pending.listeners.push(WebListener {
        serial: 1,
        name: "x".into(),
        callback,
        signal: Some(signal),
        capture: false,
        once: false,
        passive: false,
        handler: false,
    });
    pending.next_serial = 1;
    heap.major_gc(ProtectedRoots::WebTarget(target, &pending))
        .unwrap();
    assert!(!heap.contains(discarded));
    assert!(heap.contains(callback) && heap.contains(signal));
    assert!(heap.web_event_target(target).unwrap().listeners.is_empty());
    heap.set_web_event_target(target, pending).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[target])).unwrap();
    assert!(heap.contains(callback) && heap.contains(signal));
    heap.remove_web_listener(target, 1).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[target])).unwrap();
    assert!(!heap.contains(callback) && !heap.contains(signal));
}

#[test]
fn event_registration_gc_borrowed_abort_keeps_pending_edges_then_releases_them() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let signal = heap.alloc_abort_signal(prototype).unwrap();
    let callback = heap.alloc_object(None).unwrap();
    let reason = heap.alloc_object(None).unwrap();
    let target = heap.alloc_web_event_target(prototype).unwrap();
    let mut pending = AbortData {
        reason: Value::Object(reason),
        ..AbortData::default()
    };
    pending.target.listeners.push(WebListener {
        serial: 1,
        name: "abort".into(),
        callback,
        signal: None,
        capture: false,
        once: false,
        passive: false,
        handler: true,
    });
    pending.hooks.push(AbortHook::Web { target, serial: 1 });
    heap.major_gc(ProtectedRoots::AbortSignal(signal, &pending))
        .unwrap();
    assert!(heap.contains(callback) && heap.contains(reason) && heap.contains(target));
    assert!(!heap.abort_signal(signal).unwrap().aborted());
    heap.set_abort_signal(signal, pending).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[signal])).unwrap();
    assert!(heap.contains(callback) && heap.contains(reason) && heap.contains(target));
    heap.set_abort_signal(signal, AbortData::default()).unwrap();
    heap.major_gc(ProtectedRoots::Slice(&[signal])).unwrap();
    assert!(!heap.contains(callback) && !heap.contains(reason) && !heap.contains(target));
}
