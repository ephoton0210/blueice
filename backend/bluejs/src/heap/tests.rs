// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn closure_metadata_validates_its_receiver_and_is_reclaimed_with_a_young_closure() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 16,
        ..HeapConfig::default()
    })
    .unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    let ordinary = heap.alloc_object(None).unwrap();
    let closure = heap
        .alloc_closure(
            Rc::new(Bytecode::empty()),
            Vec::new(),
            Value::Undefined,
            prototype,
        )
        .unwrap();
    assert_eq!(
        heap.set_closure_home(ordinary, prototype),
        Err(HeapError::InvalidObject(ordinary))
    );
    assert_eq!(
        heap.set_closure_new_target(ordinary, Value::Null),
        Err(HeapError::InvalidObject(ordinary))
    );
    assert_eq!(
        heap.closure_new_target(ordinary),
        Err(HeapError::InvalidObject(ordinary))
    );
    heap.set_closure_new_target(closure, Value::Null).unwrap();
    assert_eq!(heap.closure_new_target(closure).unwrap(), Value::Null);
    assert_eq!(
        heap.set_class_fields(ordinary, closure),
        Err(HeapError::InvalidObject(ordinary))
    );
    assert_eq!(
        heap.class_fields(ordinary),
        Err(HeapError::InvalidObject(ordinary))
    );
    assert_eq!(heap.class_fields(closure).unwrap(), None);
    heap.set_class_fields(closure, closure).unwrap();
    assert_eq!(heap.class_fields(closure).unwrap(), Some(closure));
    heap.collect_minor();
    assert!(!heap.contains(closure));
}

#[test]
fn typed_array_backing_buffer_survives_minor_and_major_collection() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 1,
        major_threshold_bytes: 256,
        max_heap_bytes: 8192,
    })
    .unwrap();
    let buffer = heap.alloc_array_buffer(4, None).unwrap();
    let view = heap
        .alloc_typed_array(buffer, 0, 4, false, TypedArrayKind::Uint8, None)
        .unwrap();
    let root = heap.root(view).unwrap();
    heap.collect_minor();
    heap.collect_major();
    assert_eq!(
        heap.typed_array_set_index(view, 0, &Value::Number(9.0)),
        Ok(true)
    );
    assert_eq!(
        heap.typed_array_index_value(view, 0),
        Ok(Some(Value::Number(9.0)))
    );
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert!(!heap.contains(buffer));
}

#[test]
fn immutable_array_buffer_holds_its_bytes_and_rejects_every_heap_mutation() {
    let mut heap = Heap::default();
    let buffer = heap
        .alloc_immutable_array_buffer(vec![1, 2, 3, 4], None)
        .unwrap();
    let view = heap
        .alloc_typed_array(buffer, 0, 4, false, TypedArrayKind::Uint8, None)
        .unwrap();
    assert_eq!(heap.buffer_is_immutable(buffer), Ok(true));
    assert_eq!(heap.typed_array_is_immutable(view), Ok(true));
    // Immutable buffers are fixed-length, non-detached, unshared ArrayBuffers
    // whose maximum length is their length.
    assert_eq!(heap.is_array_buffer(buffer), Ok(true));
    assert_eq!(heap.buffer_is_shared(buffer), Ok(false));
    assert_eq!(heap.buffer_is_detached(buffer), Ok(false));
    assert_eq!(heap.buffer_resizable(buffer), Ok(false));
    assert_eq!(heap.buffer_byte_length(buffer), Ok(4));
    assert_eq!(heap.buffer_max_byte_length(buffer), Ok(4));
    assert_eq!(heap.array_buffer_copy(buffer, 1, 2), Ok(vec![2, 3]));
    assert_eq!(
        heap.typed_array_index_value(view, 3),
        Ok(Some(Value::Number(4.0)))
    );

    let refused = HeapError::ImmutableArrayBuffer;
    assert_eq!(heap.array_buffer_write(buffer, 0, &[9]), Err(refused));
    assert_eq!(
        heap.typed_array_set_index(view, 0, &Value::Number(9.0)),
        Err(refused)
    );
    assert_eq!(
        heap.typed_array_atomic_modify(view, 0, |old| (Some(Value::Number(9.0)), old)),
        Err(refused)
    );
    assert_eq!(heap.detach_array_buffer(buffer), Err(refused));
    assert_eq!(heap.resize_array_buffer(buffer, 2), Err(refused));
    // A read-only Atomics operation (no replacement) is still allowed.
    assert_eq!(
        heap.typed_array_atomic_modify(view, 1, |old| (None, old)),
        Ok(Value::Number(2.0))
    );
    // Every refusal left the contents and the buffer's state untouched.
    assert_eq!(heap.array_buffer_copy(buffer, 0, 4), Ok(vec![1, 2, 3, 4]));
    assert_eq!(heap.buffer_is_detached(buffer), Ok(false));
    assert_eq!(heap.buffer_byte_length(buffer), Ok(4));
}

#[test]
fn ordinary_and_shared_buffers_are_not_immutable() {
    let mut heap = Heap::default();
    let ordinary = heap.alloc_array_buffer(4, None).unwrap();
    let resizable = heap.alloc_resizable_array_buffer(2, 8, None).unwrap();
    let shared = heap.alloc_shared_array_buffer(4, Some(8), None).unwrap();
    let plain = heap.alloc_object(None).unwrap();
    for buffer in [ordinary, resizable, shared] {
        assert_eq!(heap.buffer_is_immutable(buffer), Ok(false));
    }
    let view = heap
        .alloc_typed_array(ordinary, 0, 4, false, TypedArrayKind::Uint8, None)
        .unwrap();
    assert_eq!(heap.typed_array_is_immutable(view), Ok(false));
    // Only buffers and views carry the slot.
    assert_eq!(
        heap.buffer_is_immutable(plain),
        Err(HeapError::InvalidInternalSlot(plain))
    );
    assert_eq!(
        heap.typed_array_is_immutable(ordinary),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    // A mutable buffer still accepts every write the immutable one refuses.
    assert_eq!(heap.array_buffer_write(ordinary, 0, &[7]), Ok(()));
    assert_eq!(heap.resize_array_buffer(resizable, 4), Ok(()));
    assert_eq!(heap.detach_array_buffer(ordinary), Ok(()));
}

#[test]
fn immutable_array_buffer_is_accounted_bounded_and_survives_collection() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 1,
        major_threshold_bytes: 256,
        max_heap_bytes: 8192,
    })
    .unwrap();
    let before = heap.stats().managed_bytes;
    let buffer = heap
        .alloc_immutable_array_buffer(vec![5; 64], None)
        .unwrap();
    let view = heap
        .alloc_typed_array(buffer, 0, 64, false, TypedArrayKind::Uint8, None)
        .unwrap();
    assert!(heap.stats().managed_bytes >= before + 64);
    let root = heap.root(view).unwrap();
    heap.collect_minor();
    heap.collect_major();
    // The flag and the bytes both live through promotion and collection.
    assert_eq!(heap.buffer_is_immutable(buffer), Ok(true));
    assert_eq!(heap.array_buffer_copy(buffer, 0, 64), Ok(vec![5; 64]));
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert!(!heap.contains(buffer));
    assert!(heap.stats().managed_bytes < before + 64);
    // A buffer larger than the heap can hold is refused, not allocated.
    assert_eq!(
        heap.alloc_immutable_array_buffer(vec![0; 16384], None),
        Err(HeapError::InvalidBufferRange)
    );
}

#[test]
fn immutable_typed_array_index_descriptors_are_neither_writable_nor_configurable() {
    let mut heap = Heap::default();
    let immutable = heap.alloc_immutable_array_buffer(vec![1, 2], None).unwrap();
    let ordinary = heap.alloc_array_buffer(2, None).unwrap();
    let frozen = heap
        .alloc_typed_array(immutable, 0, 2, false, TypedArrayKind::Uint8, None)
        .unwrap();
    let mutable = heap
        .alloc_typed_array(ordinary, 0, 2, false, TypedArrayKind::Uint8, None)
        .unwrap();
    let descriptor = |heap: &Heap, view, key| {
        let d = heap
            .get_own_property_descriptor(view, key)
            .unwrap()
            .unwrap();
        (d.value, d.writable, d.enumerable, d.configurable)
    };
    assert_eq!(
        descriptor(&heap, frozen, "1"),
        (
            Some(Value::Number(2.0)),
            Some(false),
            Some(true),
            Some(false)
        )
    );
    assert_eq!(
        descriptor(&heap, mutable, "1"),
        (Some(Value::Number(0.0)), Some(true), Some(true), Some(true))
    );
    // Out-of-range indices stay absent either way.
    assert!(heap
        .get_own_property_descriptor(frozen, "2")
        .unwrap()
        .is_none());
}

#[test]
fn weak_collection_values_follow_live_keys_to_an_ephemeron_fixed_point() {
    let mut heap = Heap::default();
    let first = heap.alloc_weak_collection(true, None).unwrap();
    let first_root = heap.root(first).unwrap();
    let second = heap.alloc_weak_collection(true, None).unwrap();
    let second_root = heap.root(second).unwrap();
    let first_key = heap.alloc_object(None).unwrap();
    let first_key_root = heap.root(first_key).unwrap();
    let second_key = heap.alloc_object(None).unwrap();
    let value = heap.alloc_object(None).unwrap();

    // `first_key` makes `second_key` live through the first table. The
    // second table then makes `value` live, so one ephemeron scan is not
    // enough to retain the entire chain.
    heap.weak_collection_set(first, Value::Object(first_key), Value::Object(second_key))
        .unwrap();
    heap.weak_collection_set(second, Value::Object(second_key), Value::Object(value))
        .unwrap();
    heap.collect_minor();
    heap.collect_major();
    assert!(heap.contains(second_key));
    assert!(heap.contains(value));
    assert_eq!(
        heap.weak_collection_get(second, &Value::Object(second_key))
            .unwrap(),
        Some(Value::Object(value))
    );

    heap.unroot(first_key_root).unwrap();
    heap.collect_major();
    assert!(!heap.contains(second_key));
    assert!(!heap.contains(value));
    assert_eq!(
        heap.weak_collection_get(second, &Value::Object(second_key))
            .unwrap(),
        None
    );
    heap.unroot(second_root).unwrap();
    heap.unroot(first_root).unwrap();
}

/// A heap whose collections only ever run when a test asks for one, so a test
/// can build a large structure without unrooted objects being reclaimed.
fn manual_collection_heap() -> Heap {
    Heap::new(HeapConfig {
        nursery_capacity: usize::MAX,
        major_threshold_bytes: usize::MAX,
        max_heap_bytes: usize::MAX,
    })
    .unwrap()
}

/// A chain in which each key's only reference is the previous key's table
/// value is the worst case for an ephemeron scan that restarts from the
/// beginning after every newly live key. Marking must cost time proportional
/// to the number of entries, not to their square: this fixture
/// (`staging/sm/regress/regress-1507322-deep-weakmap.js`) builds a hundred
/// thousand links and collects repeatedly while doing so.
#[test]
fn a_deep_weak_map_chain_is_marked_in_linear_time() {
    const LINKS: usize = 5_000;
    let mut heap = manual_collection_heap();
    let table = heap.alloc_weak_collection(true, None).unwrap();
    let table_root = heap.root(table).unwrap();
    let head = heap.alloc_object(None).unwrap();
    let head_root = heap.root(head).unwrap();
    let mut keys = vec![head];
    for _ in 0..LINKS {
        let next = heap.alloc_object(None).unwrap();
        heap.weak_collection_set(
            table,
            Value::Object(*keys.last().unwrap()),
            Value::Object(next),
        )
        .unwrap();
        keys.push(next);
    }

    let started = std::time::Instant::now();
    heap.collect_minor();
    heap.collect_major();
    let elapsed = started.elapsed();
    assert!(
        keys.iter().all(|key| heap.contains(*key)),
        "every link is reachable from the rooted head through the table"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "collecting a {LINKS}-link ephemeron chain took {elapsed:?}"
    );

    // Dropping the head releases the whole chain at once.
    heap.unroot(head_root).unwrap();
    heap.collect_major();
    assert!(keys.iter().all(|key| !heap.contains(*key)));
    heap.unroot(table_root).unwrap();
}

/// A table that is itself reachable only as an ephemeron value becomes live
/// mid-scan; its entries must then take part in the same fixed point, whatever
/// order the marker meets the table and its keys in. `promote_first` runs a
/// minor collection before the inner structure exists, so the outer table and
/// its key are old while everything the inner table holds is young.
#[test]
fn an_ephemeron_table_reached_only_through_another_table_still_retains_its_values() {
    for promote_first in [false, true] {
        let mut heap = manual_collection_heap();
        let outer = heap.alloc_weak_collection(true, None).unwrap();
        let outer_root = heap.root(outer).unwrap();
        let key = heap.alloc_object(None).unwrap();
        let key_root = heap.root(key).unwrap();
        if promote_first {
            heap.collect_minor();
        }
        let inner = heap.alloc_weak_collection(true, None).unwrap();
        let derived_key = heap.alloc_object(None).unwrap();
        let leaf = heap.alloc_object(None).unwrap();
        let symbol_value = heap.alloc_object(None).unwrap();
        let symbol = JsSymbol::new(Some("weak key".into()));

        // The inner table is live only through `outer[key]`; `derived_key` is
        // live only through `inner[key]`; `leaf` only through
        // `inner[derived_key]`. Each step needs the one before it.
        heap.weak_collection_set(outer, Value::Object(key), Value::Object(inner))
            .unwrap();
        heap.weak_collection_set(inner, Value::Object(derived_key), Value::Object(leaf))
            .unwrap();
        heap.weak_collection_set(inner, Value::Object(key), Value::Object(derived_key))
            .unwrap();
        heap.weak_collection_set(inner, Value::Symbol(symbol), Value::Object(symbol_value))
            .unwrap();

        heap.collect_minor();
        for object in [inner, derived_key, leaf, symbol_value] {
            assert!(heap.contains(object), "promote_first={promote_first}");
        }
        heap.collect_major();
        for object in [inner, derived_key, leaf, symbol_value] {
            assert!(heap.contains(object), "promote_first={promote_first}");
        }
        assert_eq!(
            heap.weak_collection_get(inner, &Value::Object(derived_key))
                .unwrap(),
            Some(Value::Object(leaf))
        );

        heap.unroot(key_root).unwrap();
        heap.collect_major();
        for object in [inner, derived_key, leaf, symbol_value] {
            assert!(!heap.contains(object), "promote_first={promote_first}");
        }
        heap.unroot(outer_root).unwrap();
    }
}

#[test]
fn weak_ref_does_not_trace_its_target_and_clears_after_collection() {
    let mut heap = Heap::default();
    let target = heap.alloc_object(None).unwrap();
    let target_root = heap.root(target).unwrap();
    let weak_ref = heap.alloc_weak_ref(Value::Object(target), None).unwrap();
    let weak_ref_root = heap.root(weak_ref).unwrap();

    heap.collect_minor();
    assert_eq!(
        heap.weak_ref_target(weak_ref).unwrap(),
        Some(Value::Object(target))
    );
    heap.unroot(target_root).unwrap();
    heap.collect_major();
    assert!(!heap.contains(target));
    assert_eq!(heap.weak_ref_target(weak_ref).unwrap(), None);
    heap.unroot(weak_ref_root).unwrap();
}

#[test]
fn suspended_generator_references_keep_every_saved_object_visible_to_gc() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let stack = heap.alloc_object(None).unwrap();
    let binding = heap.alloc_object(None).unwrap();
    let this = heap.alloc_object(None).unwrap();
    let argument = heap.alloc_object(None).unwrap();
    let completion = heap.alloc_object(None).unwrap();
    let cell = heap.alloc_object(None).unwrap();
    let dynamic = heap.alloc_object(None).unwrap();
    let home = heap.alloc_object(None).unwrap();
    let iterator = heap.alloc_object(None).unwrap();
    let pending = heap.alloc_object(None).unwrap();
    let saved = heap.alloc_object(None).unwrap();
    let delegate = heap.alloc_object(None).unwrap();
    let with_object = heap.alloc_object(None).unwrap();
    let state = GeneratorState::Suspended {
        code: Rc::new(Bytecode::empty()),
        pc: 0,
        stack: vec![Value::Object(stack)],
        bindings: vec![Some(Value::Object(binding))],
        cells: vec![(0, cell)],
        this: Value::Object(this),
        args: vec![Value::Object(argument)],
        completion: Value::Object(completion),
        completion_empty: true,
        active_scopes: Vec::new(),
        dynamic_bindings: vec![("dynamic".into(), dynamic, Vec::new())],
        iterators: vec![Value::Object(iterator)],
        handlers: Vec::new(),
        pending_completions: vec![GeneratorPendingCompletion::Throw(Value::Object(pending))],
        completion_saves: vec![(Value::Object(saved), false)],
        async_delegate: Some(AsyncGeneratorDelegate {
            record: Value::Object(delegate),
            exit_pc: 0,
        }),
        delegate: Some(GeneratorDelegate {
            record: Value::Object(delegate),
            exit_pc: 0,
        }),
        home: Some(home),
        callee: Value::Undefined,
        with_objects: vec![Value::Object(with_object)],
    };
    let references = state.references();
    for object in [
        stack,
        binding,
        this,
        argument,
        completion,
        cell,
        dynamic,
        home,
        iterator,
        pending,
        saved,
        delegate,
        with_object,
    ] {
        assert!(references.contains(&object));
    }
    assert!(heap.contains(prototype));
}

#[test]
fn start_and_completed_generator_states_expose_their_gc_edges() {
    let mut heap = Heap::default();
    let capture = heap.alloc_object(None).unwrap();
    let callee = heap.alloc_object(None).unwrap();
    let receiver = heap.alloc_object(None).unwrap();
    let argument = heap.alloc_object(None).unwrap();
    let home = heap.alloc_object(None).unwrap();
    let state = GeneratorState::Start {
        code: Rc::new(Bytecode::empty()),
        captures: vec![capture],
        callee: Value::Object(callee),
        receiver: Value::Object(receiver),
        args: vec![Value::Object(argument)],
        home: Some(home),
        with_objects: Vec::new(),
    };
    let references = state.references();
    for object in [capture, callee, receiver, argument, home] {
        assert!(references.contains(&object));
    }
    assert!(GeneratorState::Done.references().is_empty());
}

#[test]
fn restoring_generator_state_updates_its_managed_byte_charge() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let generator = heap
        .alloc_generator(GeneratorState::Done, prototype)
        .unwrap();
    let saved = heap.alloc_object(None).unwrap();
    let baseline = heap.stats().managed_bytes;

    heap.set_generator_state(
        generator,
        GeneratorState::Start {
            code: Rc::new(Bytecode::empty()),
            captures: vec![saved],
            callee: Value::Undefined,
            receiver: Value::Undefined,
            args: Vec::new(),
            home: None,
            with_objects: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(heap.stats().managed_bytes, baseline + size_of::<ObjectId>());

    heap.set_generator_state(generator, GeneratorState::Done)
        .unwrap();
    assert_eq!(heap.stats().managed_bytes, baseline);
}

#[test]
fn async_generator_queue_keeps_request_targets_and_values_alive() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let generator = heap
        .alloc_generator(GeneratorState::Done, prototype)
        .unwrap();
    heap.enable_async_generator(generator).unwrap();
    let generator_root = heap.root(generator).unwrap();
    let target = heap.alloc_object(None).unwrap();
    let value = heap.alloc_object(None).unwrap();

    let mut control = heap.async_generator_control(generator).unwrap().unwrap();
    control.requests.push_back(AsyncGeneratorRequest {
        id: 0,
        completion: AsyncGeneratorCompletion::Next(Value::Object(value)),
        target,
    });
    control.next_request_id = 1;
    control.status = AsyncGeneratorStatus::Awaiting;
    heap.set_async_generator_control(generator, control)
        .unwrap();

    heap.collect_major();
    assert!(heap.contains(target));
    assert!(heap.contains(value));

    heap.unroot(generator_root).unwrap();
    heap.collect_major();
    assert!(!heap.contains(target));
    assert!(!heap.contains(value));
}

#[test]
fn private_sidecars_trace_brands_slots_and_private_elements() {
    let mut heap = Heap::default();
    let owner = heap.alloc_object(None).unwrap();
    let owner_root = heap.root(owner).unwrap();
    heap.define_private_field(owner, "field".into()).unwrap();

    let receiver = heap.alloc_object(None).unwrap();
    let receiver_root = heap.root(receiver).unwrap();
    heap.add_private_brand(receiver, owner).unwrap();
    let slot_value = heap.alloc_object(None).unwrap();
    heap.set_private_slot(receiver, owner, "field".into(), Value::Object(slot_value))
        .unwrap();

    let method = heap.alloc_object(None).unwrap();
    heap.define_private_method(owner, "method".into(), Value::Object(method))
        .unwrap();

    heap.collect_major();
    assert!(heap.contains(slot_value));
    assert!(heap.contains(method));
    assert_eq!(
        heap.private_slot(receiver, owner, &"field".into()),
        Ok(Some(Value::Object(slot_value)))
    );
    assert_eq!(
        heap.private_element(owner, &"method".into())
            .unwrap()
            .and_then(|element| match element {
                PrivateElement::Method(value) => Some(value),
                _ => None,
            }),
        Some(Value::Object(method))
    );
    heap.unroot(receiver_root).unwrap();
    heap.unroot(owner_root).unwrap();
}

#[test]
fn restoring_generator_state_respects_the_heap_limit() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 16,
        major_threshold_bytes: 1_024,
        max_heap_bytes: 4_096,
    })
    .unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    let generator = heap
        .alloc_generator(GeneratorState::Done, prototype)
        .unwrap();
    let saved = heap.alloc_object(None).unwrap();
    let baseline = heap.stats().managed_bytes;

    assert_eq!(
        heap.set_generator_state(
            generator,
            GeneratorState::Start {
                code: Rc::new(Bytecode::empty()),
                captures: vec![saved; 1_024],
                callee: Value::Undefined,
                receiver: Value::Undefined,
                args: Vec::new(),
                home: None,
                with_objects: Vec::new(),
            },
        ),
        Err(HeapError::HeapLimitExceeded { limit: 4_096 })
    );
    assert_eq!(heap.stats().managed_bytes, baseline);
    assert!(matches!(
        heap.take_generator_state(generator),
        Ok(GeneratorState::Done)
    ));
}

#[test]
fn array_length_updates_and_failed_truncation_leave_specified_lengths() {
    let mut heap = Heap::default();
    let array = heap.alloc_array(0, None).unwrap();
    heap.set(array, "0", Value::Number(1.0)).unwrap();
    heap.set(array, "1", Value::Number(2.0)).unwrap();
    heap.set(array, "length", Value::Number(1.0)).unwrap();
    assert_eq!(heap.get(array, "length"), Ok(Value::Number(1.0)));

    heap.define_own_property(
        array,
        "1",
        PropertyDescriptor::data(Value::Number(2.0), true, true, false),
    )
    .unwrap();
    assert_eq!(
        heap.set(array, "length", Value::Number(0.0)),
        Err(HeapError::ReadOnlyProperty)
    );
    assert_eq!(heap.get(array, "length"), Ok(Value::Number(2.0)));
}

#[test]
fn canonical_numeric_index_strings_use_ecmascript_number_formatting() {
    assert_eq!(binary_data::ecmascript_number_string(0.0000001), "1e-7");
    assert_eq!(binary_data::ecmascript_number_string(0.000001), "0.000001");
    assert_eq!(binary_data::ecmascript_number_string(1e21), "1e+21");
    assert!(binary_data::typed_array_numeric_key(&"1e-7".into()).is_some());
    assert!(binary_data::typed_array_numeric_key(&"0.0000001".into()).is_none());
    assert!(binary_data::typed_array_numeric_key(&"1e21".into()).is_none());
    assert!(binary_data::typed_array_numeric_key(&"1e+21".into()).is_some());
}

#[test]
fn temporal_payload_is_accounted() {
    let mut heap = Heap::new(HeapConfig::default()).unwrap();
    let before = heap.stats().managed_bytes;
    let value = TemporalValue {
        kind: TemporalKind::PlainDateTime,
        duration: None,
        year: 2024,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        millisecond: 0,
        microsecond: 0,
        nanosecond: 0,
        epoch_nanoseconds: BigInt::from(0),
        calendar: "iso8601".repeat(50),
        time_zone: "UTC".repeat(50),
    };
    let expected = std::mem::size_of::<Object>() + value.bytes();
    let bytes_added = heap
        .alloc(ObjectKind::Temporal(Box::new(value)), None)
        .map(|_| heap.stats().managed_bytes - before)
        .unwrap();
    assert_eq!(bytes_added, expected);
}

/// Exact IEEE 754 binary16 bit patterns pinned from Test262's
/// `built-ins/DataView/prototype/{get,set}Float16` fixtures (read 2026-09-18):
/// `set-values-little-endian-order.js` (42 <-> 0x5140, 2.158203125 <->
/// 0x4051 -- the latter is what writing 42 little-endian and reading it back
/// big-endian observes) and `return-values.js` (3.078125 <-> 0x4228).
#[test]
fn float16_bits_round_trip_pinned_test262_vectors() {
    assert_eq!(binary_data::f16_bits_to_f64(0x5140), 42.0);
    assert_eq!(binary_data::f64_to_f16_bits(42.0), 0x5140);
    assert_eq!(binary_data::f16_bits_to_f64(0x4051), 2.158203125);
    assert_eq!(binary_data::f64_to_f16_bits(2.158203125), 0x4051);
    assert_eq!(binary_data::f16_bits_to_f64(0x4228), 3.078125);
    assert_eq!(binary_data::f64_to_f16_bits(3.078125), 0x4228);
}

#[test]
fn float16_bits_handle_signed_zero_infinity_and_nan() {
    assert_eq!(binary_data::f64_to_f16_bits(0.0), 0x0000);
    assert_eq!(binary_data::f64_to_f16_bits(-0.0), 0x8000);
    assert_eq!(
        binary_data::f16_bits_to_f64(0x0000).to_bits(),
        0.0f64.to_bits()
    );
    assert_eq!(
        binary_data::f16_bits_to_f64(0x8000).to_bits(),
        (-0.0f64).to_bits()
    );

    assert_eq!(binary_data::f64_to_f16_bits(f64::INFINITY), 0x7c00);
    assert_eq!(binary_data::f64_to_f16_bits(f64::NEG_INFINITY), 0xfc00);
    assert_eq!(binary_data::f16_bits_to_f64(0x7c00), f64::INFINITY);
    assert_eq!(binary_data::f16_bits_to_f64(0xfc00), f64::NEG_INFINITY);

    // Any input that overflows binary16's finite range rounds to infinity.
    assert_eq!(binary_data::f64_to_f16_bits(1.0e10), 0x7c00);

    assert!(binary_data::f16_bits_to_f64(0x7e00).is_nan());
    assert!(binary_data::f64_to_f16_bits(f64::NAN) == 0x7e00);
}

#[test]
fn float16_bits_round_trip_subnormals_and_min_normal() {
    // Smallest subnormal: 2^-24.
    let smallest_subnormal = 2f64.powi(-24);
    assert_eq!(binary_data::f16_bits_to_f64(0x0001), smallest_subnormal);
    assert_eq!(binary_data::f64_to_f16_bits(smallest_subnormal), 0x0001);

    // Largest subnormal (mantissa 0x3ff, exponent field 0) rounds up to the
    // smallest normal (exponent field 1, mantissa 0) exactly at the boundary.
    let smallest_normal = 2f64.powi(-14);
    assert_eq!(binary_data::f16_bits_to_f64(0x0400), smallest_normal);
    assert_eq!(binary_data::f64_to_f16_bits(smallest_normal), 0x0400);

    // A value strictly below the halfway point to the smallest subnormal
    // flushes to zero; a value strictly above it rounds up to that
    // subnormal.
    let halfway = 2f64.powi(-25);
    assert_eq!(binary_data::f64_to_f16_bits(halfway * 0.5), 0x0000);
    assert_eq!(binary_data::f64_to_f16_bits(halfway * 1.5), 0x0001);
}

#[test]
fn a_collection_iterator_traces_its_collection_and_releases_it_once_exhausted() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    // Neither the set nor its member is rooted: only the iterator reaches them.
    let set = heap.alloc_set(None).unwrap();
    let member = heap.alloc_object(None).unwrap();
    heap.set_add(set, Value::Object(member)).unwrap();
    let iterator = heap
        .alloc_collection_iterator(set, false, ArrayIteratorKind::Values, prototype)
        .unwrap();
    let iterator_root = heap.root(iterator).unwrap();

    heap.collect_minor();
    heap.collect_major();
    assert!(
        heap.contains(set),
        "the iterator keeps its collection alive"
    );
    assert!(heap.contains(member), "and, through it, the entries");

    // A Map iterator's `next` (and any non-iterator) is not this iterator's.
    assert_eq!(heap.collection_iterator_next(iterator, true), Ok(None));
    assert_eq!(heap.collection_iterator_next(set, false), Ok(None));

    let (key, value, kind) = heap
        .collection_iterator_next(iterator, false)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(key, Value::Object(member));
    assert_eq!(
        value,
        Value::Undefined,
        "a Set stores no value beside its key"
    );
    assert_eq!(kind, ArrayIteratorKind::Values);

    // Exhaustion is final and drops the reference to the collection.
    assert_eq!(
        heap.collection_iterator_next(iterator, false),
        Ok(Some(None))
    );
    heap.set_add(set, Value::Number(1.0)).unwrap();
    assert_eq!(
        heap.collection_iterator_next(iterator, false),
        Ok(Some(None))
    );
    heap.collect_major();
    assert!(!heap.contains(set));
    assert!(!heap.contains(member));
    heap.unroot(iterator_root).unwrap();
}

#[test]
fn collection_clear_releases_entry_bytes_and_keeps_positions_valid_for_iterators() {
    let mut heap = Heap::default();
    let map = heap.alloc_map(None).unwrap();
    let root = heap.root(map).unwrap();
    let empty_bytes = heap.managed_bytes;
    heap.map_set(map, Value::Number(1.0), Value::Number(2.0))
        .unwrap();
    heap.map_set(map, Value::Number(3.0), Value::Number(4.0))
        .unwrap();
    assert!(heap.managed_bytes > empty_bytes);

    heap.map_delete(map, &Value::Number(1.0)).unwrap();
    assert!(matches!(
        heap.collection_entry_at(map, 0),
        Ok(CollectionEntry::Deleted)
    ));
    assert!(matches!(
        heap.collection_entry_at(map, 1),
        Ok(CollectionEntry::Present(..))
    ));
    assert!(matches!(
        heap.collection_entry_at(map, 2),
        Ok(CollectionEntry::End)
    ));

    heap.collection_clear(map).unwrap();
    assert_eq!(
        heap.managed_bytes, empty_bytes,
        "every entry's bytes are released"
    );
    assert_eq!(heap.map_size(map), Ok(0));
    // The list keeps its length: an iterator positioned inside it stays valid.
    assert!(matches!(
        heap.collection_entry_at(map, 1),
        Ok(CollectionEntry::Deleted)
    ));
    assert!(matches!(
        heap.collection_entry_at(map, 2),
        Ok(CollectionEntry::End)
    ));
    // ... and an entry added afterwards lands past every old position.
    heap.map_set(map, Value::Number(5.0), Value::Number(6.0))
        .unwrap();
    assert!(matches!(
        heap.collection_entry_at(map, 2),
        Ok(CollectionEntry::Present(..))
    ));

    let ordinary = heap.alloc_object(None).unwrap();
    assert_eq!(
        heap.collection_clear(ordinary),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert!(matches!(
        heap.collection_entry_at(ordinary, 0),
        Err(HeapError::InvalidInternalSlot(_))
    ));
    heap.unroot(root).unwrap();
}

#[test]
fn own_integer_keys_lists_only_canonical_indices_in_ascending_order() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    for key in ["10", "2", "01", "x", "9007199254740990", "-1", "1.5", "0"] {
        heap.set(object, key, Value::Number(1.0)).unwrap();
    }
    assert_eq!(
        heap.own_integer_keys(object),
        Ok(Some(vec![0, 2, 10, 9_007_199_254_740_990]))
    );
    let array = heap.alloc_array(100, None).unwrap();
    heap.set(array, "50", Value::Null).unwrap();
    assert_eq!(heap.own_integer_keys(array), Ok(Some(vec![50])));
}

#[test]
fn own_integer_keys_declines_objects_whose_indices_are_not_all_stored() {
    let mut heap = Heap::default();
    let target = heap.alloc_object(None).unwrap();
    let proxy = heap
        .alloc_proxy(target, target, None, false, false)
        .unwrap();
    let string = heap.alloc_string("abc".into(), None).unwrap();
    let buffer = heap.alloc_array_buffer(4, None).unwrap();
    let view = heap
        .alloc_typed_array(buffer, 0, 4, false, TypedArrayKind::Uint8, None)
        .unwrap();
    for object in [proxy, string, view] {
        assert_eq!(heap.own_integer_keys(object), Ok(None));
    }
}

#[test]
fn structure_epoch_advances_only_when_the_set_of_findable_keys_changes() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    let data = |value: Value, writable: bool| PropertyDescriptor::data(value, writable, true, true);
    let mut epoch = heap.structure_epoch();
    let mut advanced = |heap: &Heap| {
        let now = heap.structure_epoch();
        let moved = now != epoch;
        epoch = now;
        moved
    };

    heap.set(object, "a", Value::Number(1.0)).unwrap();
    assert!(advanced(&heap), "a new key");
    heap.set(object, "a", Value::Number(2.0)).unwrap();
    assert!(!advanced(&heap), "overwriting a value");
    heap.define_own_property(object, "b", data(Value::Null, true))
        .unwrap();
    assert!(advanced(&heap), "a new defined key");
    heap.define_own_property(object, "b", data(Value::Null, false))
        .unwrap();
    assert!(!advanced(&heap), "redefining an existing key");
    heap.delete(object, "missing").unwrap();
    assert!(!advanced(&heap), "deleting an absent key");
    heap.delete(object, "a").unwrap();
    assert!(advanced(&heap), "deleting a key");
    heap.set_prototype(object, Some(prototype)).unwrap();
    assert!(advanced(&heap), "a new prototype");
}

#[test]
fn completion_records_retain_object_references_and_charge_owned_payloads() {
    let mut heap = Heap::default();
    let first = heap.alloc_object(None).unwrap();
    let second = heap.alloc_object(None).unwrap();
    let values = vec![
        Value::Object(first),
        Value::String("payload".into()),
        Value::Object(second),
    ];
    let completion = GeneratorPendingCompletion::TailRecur(values.clone());
    assert_eq!(completion.references(), vec![first, second]);
    assert_eq!(
        completion.managed_bytes(),
        values.len() * size_of::<Value>() + values.iter().map(Value::payload_bytes).sum::<usize>()
    );

    for completion in [
        GeneratorPendingCompletion::Throw(Value::Object(first)),
        GeneratorPendingCompletion::Return(Value::Object(first)),
    ] {
        assert_eq!(completion.references(), vec![first]);
        assert_eq!(completion.managed_bytes(), 0);
    }
    for completion in [
        GeneratorPendingCompletion::ReferenceError("message".into()),
        GeneratorPendingCompletion::TypeError("message".into()),
        GeneratorPendingCompletion::RangeError("message".into()),
        GeneratorPendingCompletion::SyntaxError("message".into()),
        GeneratorPendingCompletion::Test262("message".into()),
    ] {
        assert!(completion.references().is_empty());
        assert_eq!(completion.managed_bytes(), "message".len());
    }

    let mut queue = AsyncGeneratorControl::default();
    queue.requests.push_back(AsyncGeneratorRequest {
        id: 0,
        completion: AsyncGeneratorCompletion::ResumeThrow(Value::Object(first)),
        target: second,
    });
    assert_eq!(queue.references(), vec![second, first]);
    assert_eq!(queue.managed_bytes(), size_of::<AsyncGeneratorRequest>());
}

#[test]
fn shared_buffer_rejects_overflow_and_notifies_only_matching_waiters() {
    let buffer = SharedBuffer::new(4);
    assert_eq!(buffer.copy(usize::MAX, 2), None);
    assert!(!buffer.write(usize::MAX, &[1, 2]));
    assert!(!buffer.write(4, &[1]));
    assert_eq!(buffer.modify(usize::MAX, 2, |_| true), None);
    assert_eq!(buffer.copy(0, 4), Some(vec![0; 4]));

    let waiter = buffer.register_waiter(0);
    assert_eq!(buffer.notify(4, 1), 0);
    assert_eq!(buffer.notify(0, 0), 0);
    assert_eq!(buffer.notify(0, 1), 1);
    assert_eq!(
        buffer.wait_for(waiter, Some(Duration::ZERO)),
        SharedWaitResult::Ok
    );
}

#[test]
fn collection_index_keeps_other_entries_when_a_hash_bucket_is_shared() {
    let first = Value::String("first".into());
    let second = first.clone();
    let mut collection = OrderedCollection {
        entries: vec![
            Some((first.clone(), Value::Number(1.0))),
            Some((second.clone(), Value::Number(2.0))),
        ],
        indexes: HashMap::from([(same_value_zero_hash(&first), vec![0, 1])]),
        len: 2,
    };
    // Two slots with one indexed key exercise bucket removal independently
    // of a chance 64-bit hash collision.
    assert_eq!(
        collection.delete(&first),
        Some((first.clone(), Value::Number(1.0)))
    );
    assert_eq!(collection.len(), 1);
    assert_eq!(collection.indexes.values().next(), Some(&vec![1]));
    assert_eq!(collection.get(&first), Some(&Value::Number(2.0)));
    assert_eq!(
        collection.delete(&first),
        Some((second, Value::Number(2.0)))
    );
    assert_eq!(collection.delete(&first), None);
}

#[test]
fn finalization_registry_allocation_traces_holdings_but_not_weak_targets() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let callback = heap.alloc_object(None).unwrap();
    let target = heap.alloc_object(None).unwrap();
    let holdings = heap.alloc_object(None).unwrap();
    let registry = ObjectKind::FinalizationRegistry {
        cleanup_callback: Value::Object(callback),
        cells: vec![FinalizationCell {
            target: Some(WeakCollectionKey::Object(target)),
            holdings: Value::Object(holdings),
            unregister_token: None,
        }],
    };
    assert_eq!(
        allocation_references(&registry, Some(prototype)),
        vec![prototype, callback, holdings]
    );
    assert!(WeakCollectionKey::from_value(&Value::Bool(false)).is_none());
}

#[test]
fn same_value_zero_index_hashes_every_key_category_consistently() {
    let symbol = JsSymbol::new(Some("identity".into()));
    let keys = [
        Value::Undefined,
        Value::Null,
        Value::Bool(true),
        Value::BigInt(BigInt::from(123)),
        Value::Symbol(symbol),
    ];
    let mut collection = OrderedCollection::default();
    for (index, key) in keys.iter().enumerate() {
        collection.set(key.clone(), Value::Number(index as f64));
        assert_eq!(collection.get(key), Some(&Value::Number(index as f64)));
    }
    collection.set(Value::Number(0.0), Value::Bool(true));
    collection.set(Value::Number(f64::NAN), Value::Bool(false));
    assert_eq!(
        collection.get(&Value::Number(-0.0)),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        collection.get(&Value::Number(-f64::NAN)),
        Some(&Value::Bool(false))
    );
}

#[test]
fn heap_errors_identify_invalid_buffer_ranges_and_uninitialized_exports() {
    assert_eq!(
        HeapError::InvalidBufferRange.to_string(),
        "invalid ArrayBuffer view range"
    );
    assert_eq!(
        HeapError::UninitializedModuleExport.to_string(),
        "module namespace export is uninitialized"
    );
}

#[test]
fn intl_host_records_only_trace_their_javascript_prototype() {
    use blueice_ecma402 as ecma402;

    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let display_names = ecma402::DisplayNames::try_new(
        &[],
        ecma402::DisplayNamesOptions {
            locale_matcher: Default::default(),
            display_type: ecma402::DisplayNamesType::Language,
            style: Default::default(),
            fallback: Default::default(),
            language_display: Default::default(),
        },
    )
    .unwrap();
    let duration_format = ecma402::DurationFormat::try_new(&[], Default::default()).unwrap();
    let list_format = ecma402::ListFormat::try_new(&[], Default::default()).unwrap();
    let plural_rules = crate::intl::PluralRules {
        data: ecma402::PluralRules::try_new(&[], Default::default()).unwrap(),
        rule_type: Default::default(),
        notation: "standard".into(),
        compact_display: None,
        minimum_integer_digits: 1,
        minimum_fraction_digits: 0,
        maximum_fraction_digits: 3,
        minimum_significant_digits: None,
        maximum_significant_digits: None,
        rounding_increment: 1,
        rounding_mode: "halfExpand".into(),
        rounding_priority: "auto".into(),
        trailing_zero_display: "auto".into(),
    };
    let relative_time_format =
        ecma402::RelativeTimeFormat::try_new(&[], Default::default()).unwrap();
    let segments = Rc::new(crate::intl::Segments {
        input: "abc".into(),
        records: vec![],
    });
    for kind in [
        ObjectKind::DisplayNames(Rc::new(display_names)),
        ObjectKind::DurationFormat(Rc::new(duration_format)),
        ObjectKind::ListFormat(Rc::new(list_format)),
        ObjectKind::PluralRules(Rc::new(plural_rules)),
        ObjectKind::RelativeTimeFormat(Rc::new(relative_time_format)),
        ObjectKind::SegmentIterator {
            data: segments,
            next: 0,
        },
    ] {
        assert_eq!(
            allocation_references(&kind, Some(prototype)),
            vec![prototype]
        );
    }
}

#[test]
#[should_panic(expected = "a duration has no date-time fields")]
fn duration_cannot_be_interpreted_as_a_plain_date_time() {
    let duration = TemporalValue {
        kind: TemporalKind::Duration,
        duration: None,
        year: 0,
        month: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        millisecond: 0,
        microsecond: 0,
        nanosecond: 0,
        epoch_nanoseconds: BigInt::from(0),
        calendar: String::new(),
        time_zone: String::new(),
    };
    duration.plain_epoch_milliseconds();
}

#[test]
fn buffer_allocation_rejects_limits_before_installing_a_shared_backing() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 32,
        major_threshold_bytes: OBJECT_BYTES,
        max_heap_bytes: OBJECT_BYTES + 32,
    })
    .unwrap();
    let capacity = heap.max_array_buffer_byte_length();
    assert_eq!(capacity, 32);
    assert_eq!(
        heap.alloc_array_buffer(capacity + 1, None),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.alloc_resizable_array_buffer(2, 1, None),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.alloc_shared_array_buffer(2, Some(capacity + 1), None),
        Err(HeapError::InvalidBufferRange)
    );

    let large = std::sync::Arc::new(SharedBuffer::new(capacity + 1));
    assert_eq!(
        heap.alloc_shared_array_buffer_backing(large, None, None),
        Err(HeapError::InvalidBufferRange)
    );
    let backing = std::sync::Arc::new(SharedBuffer::new(2));
    for maximum in [1, capacity + 1] {
        assert_eq!(
            heap.alloc_shared_array_buffer_backing(backing.clone(), Some(maximum), None),
            Err(HeapError::InvalidBufferRange)
        );
    }
    let shared = heap
        .alloc_shared_array_buffer_backing(backing.clone(), Some(capacity), None)
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &heap.shared_buffer_backing(shared).unwrap(),
        &backing
    ));
    assert_eq!(heap.buffer_byte_length(shared), Ok(2));
}

#[test]
fn binary_data_accessors_reject_other_kinds_and_preserve_detached_view_slots() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    let buffer = heap.alloc_resizable_array_buffer(8, 16, None).unwrap();
    let shared = heap.alloc_shared_array_buffer(2, Some(8), None).unwrap();
    let view = heap.alloc_data_view(buffer, 4, 4, false, None).unwrap();
    let tracking = heap.alloc_data_view(buffer, 2, 0, true, None).unwrap();
    let typed = heap
        .alloc_typed_array(buffer, 4, 4, false, TypedArrayKind::Uint8, None)
        .unwrap();

    assert_eq!(heap.is_data_view(view), Ok(true));
    assert_eq!(heap.is_data_view(object), Ok(false));
    assert_eq!(heap.data_view_buffer(view), Ok(buffer));
    assert_eq!(heap.data_view_current_info(view), Ok((buffer, 4, 4)));
    assert_eq!(heap.array_buffer_is_resizable(buffer), Ok(true));
    assert_eq!(heap.buffer_growable(shared), Ok(true));
    assert_eq!(heap.buffer_growable(buffer), Ok(false));
    assert_eq!(heap.typed_array_is_length_tracking(typed), Ok(false));

    macro_rules! invalid_slot {
        () => {
            Err(HeapError::InvalidInternalSlot(object))
        };
    }
    assert_eq!(heap.array_buffer_byte_length(object), invalid_slot!());
    assert_eq!(heap.buffer_byte_length(object), invalid_slot!());
    assert_eq!(heap.array_buffer_is_detached(object), invalid_slot!());
    assert_eq!(heap.buffer_is_shared(object), invalid_slot!());
    assert_eq!(heap.buffer_max_byte_length(object), invalid_slot!());
    assert_eq!(heap.buffer_resizable(object), invalid_slot!());
    assert_eq!(heap.array_buffer_is_resizable(object), invalid_slot!());
    assert_eq!(heap.buffer_growable(object), invalid_slot!());
    assert_eq!(heap.data_view_raw_info(object), invalid_slot!());
    assert_eq!(heap.data_view_current_info(object), invalid_slot!());
    assert_eq!(heap.data_view_buffer(object), invalid_slot!());
    assert_eq!(heap.typed_array_info(object), invalid_slot!());
    assert_eq!(heap.typed_array_is_out_of_bounds(object), invalid_slot!());
    assert_eq!(heap.typed_array_is_length_tracking(object), invalid_slot!());
    assert_eq!(
        heap.typed_array_prevent_extensions_allowed(object),
        invalid_slot!()
    );
    assert_eq!(heap.detach_array_buffer(object), invalid_slot!());
    assert_eq!(heap.array_buffer_copy(object, 0, 1), invalid_slot!());
    assert_eq!(
        heap.array_buffer_write(object, 0, &[1]),
        Err(HeapError::InvalidObject(object))
    );
    assert_eq!(
        heap.array_buffer_byte_length(shared),
        Err(HeapError::InvalidInternalSlot(shared))
    );
    assert_eq!(
        heap.array_buffer_is_detached(shared),
        Err(HeapError::InvalidInternalSlot(shared))
    );
    assert_eq!(
        heap.detach_array_buffer(shared),
        Err(HeapError::InvalidInternalSlot(shared))
    );

    assert_eq!(
        heap.alloc_data_view(buffer, usize::MAX, 1, false, None),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.alloc_data_view(buffer, 7, 2, false, None),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.alloc_typed_array(buffer, 0, usize::MAX, false, TypedArrayKind::Uint16, None),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.alloc_typed_array(buffer, usize::MAX, 1, false, TypedArrayKind::Uint8, None),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.alloc_typed_array(buffer, 7, 2, false, TypedArrayKind::Uint8, None),
        Err(HeapError::InvalidBufferRange)
    );

    heap.resize_array_buffer(buffer, 3).unwrap();
    assert_eq!(heap.data_view_raw_info(view), Ok((buffer, 4, 4)));
    assert_eq!(
        heap.data_view_current_info(view),
        Err(HeapError::InvalidInternalSlot(view))
    );
    assert_eq!(heap.data_view_current_info(tracking), Ok((buffer, 2, 1)));
    assert_eq!(
        heap.typed_array_info(typed),
        Ok((buffer, 4, 0, TypedArrayKind::Uint8))
    );
    assert_eq!(heap.typed_array_is_out_of_bounds(typed), Ok(true));
    assert_eq!(heap.typed_array_index_value(typed, 0), Ok(None));
    heap.resize_array_buffer(buffer, 1).unwrap();
    assert_eq!(
        heap.data_view_current_info(tracking),
        Err(HeapError::InvalidInternalSlot(tracking))
    );
    assert_eq!(heap.typed_array_is_out_of_bounds(typed), Ok(true));

    heap.detach_array_buffer(buffer).unwrap();
    assert_eq!(heap.data_view_buffer(view), Ok(buffer));
    assert_eq!(
        heap.data_view_current_info(view),
        Err(HeapError::DetachedArrayBuffer)
    );
    assert_eq!(heap.typed_array_index_value(typed, 0), Ok(None));
    assert_eq!(
        heap.array_buffer_copy(buffer, 0, 0),
        Err(HeapError::DetachedArrayBuffer)
    );
    assert_eq!(
        heap.array_buffer_write(buffer, 0, &[]),
        Err(HeapError::DetachedArrayBuffer)
    );
    assert_eq!(
        heap.detach_array_buffer(buffer),
        Err(HeapError::DetachedArrayBuffer)
    );
}

#[test]
fn growable_and_fixed_buffers_reject_wrong_or_excessive_resize_operations() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    let ordinary = heap.alloc_array_buffer(2, None).unwrap();
    let resizable = heap.alloc_resizable_array_buffer(2, 4, None).unwrap();
    let shared = heap.alloc_shared_array_buffer(2, Some(4), None).unwrap();
    let fixed_shared = heap.alloc_shared_array_buffer(2, None, None).unwrap();
    assert_eq!(
        heap.resize_array_buffer(object, 3),
        Err(HeapError::InvalidInternalSlot(object))
    );
    assert_eq!(
        heap.resize_array_buffer(ordinary, 3),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.grow_shared_array_buffer(fixed_shared, 3),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.resize_array_buffer(resizable, 5),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.grow_shared_array_buffer(shared, 1),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.grow_shared_array_buffer(shared, 5),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.resize_array_buffer(shared, 3),
        Err(HeapError::InvalidInternalSlot(shared))
    );
    assert_eq!(
        heap.grow_shared_array_buffer(resizable, 3),
        Err(HeapError::InvalidInternalSlot(resizable))
    );
    assert_eq!(heap.grow_shared_array_buffer(shared, 4), Ok(()));
    assert_eq!(heap.buffer_byte_length(shared), Ok(4));
}

#[test]
fn numeric_key_and_float16_boundary_helpers_preserve_canonical_values() {
    use binary_data::{f64_to_f16_bits, fixed_to_scientific, scientific_to_fixed};

    assert_eq!(
        binary_data::typed_array_numeric_key(&"4294967295".into()),
        Some(TypedArrayNumericKey::Index(4_294_967_295))
    );
    assert_eq!(scientific_to_fixed("-1.25e3"), Some("-1250".into()));
    assert_eq!(scientific_to_fixed("1e-7"), Some("0.0000001".into()));
    assert_eq!(scientific_to_fixed("1.2e-2"), Some("0.012".into()));
    assert_eq!(scientific_to_fixed("1.25e0"), Some("1.25".into()));
    assert_eq!(scientific_to_fixed("1.25e2"), Some("125".into()));
    assert_eq!(scientific_to_fixed("1.25eX"), None);
    assert_eq!(scientific_to_fixed("1.25"), None);
    assert_eq!(
        fixed_to_scientific("-1200000000000000000000"),
        Some("-1.2e+21".into())
    );
    assert_eq!(fixed_to_scientific("0.0000001"), Some("1e-7".into()));
    assert_eq!(fixed_to_scientific("0"), None);

    assert_eq!(f64_to_f16_bits(f64::from_bits(1)), 0x0000);
    assert_eq!(f64_to_f16_bits(2.0 - 2f64.powi(-11)), 0x4000);
    assert_eq!(f64_to_f16_bits(65520.0), 0x7c00);
}

#[test]
fn buffer_growth_observes_the_heap_budget_without_changing_existing_bytes() {
    let limit = OBJECT_BYTES * 3 + 16;
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 32,
        major_threshold_bytes: limit,
        max_heap_bytes: limit,
    })
    .unwrap();
    let buffer = heap.alloc_resizable_array_buffer(1, 17, None).unwrap();
    heap.array_buffer_write(buffer, 0, &[9]).unwrap();
    let _first = heap.alloc_object(None).unwrap();
    let _second = heap.alloc_object(None).unwrap();
    assert_eq!(
        heap.resize_array_buffer(buffer, 17),
        Err(HeapError::HeapLimitExceeded { limit })
    );
    assert_eq!(heap.array_buffer_copy(buffer, 0, 1), Ok(vec![9]));
    assert_eq!(heap.buffer_byte_length(buffer), Ok(1));
}

#[test]
fn shared_and_plain_typed_array_atomic_bounds_are_checked_before_modification() {
    let mut heap = Heap::default();
    let plain = heap.alloc_array_buffer(2, None).unwrap();
    let shared = heap.alloc_shared_array_buffer(2, None, None).unwrap();
    let plain_view = heap
        .alloc_typed_array(plain, 0, 2, false, TypedArrayKind::Uint8, None)
        .unwrap();
    let shared_view = heap
        .alloc_typed_array(shared, 0, 2, false, TypedArrayKind::Uint8, None)
        .unwrap();
    assert_eq!(
        heap.typed_array_atomic_modify(plain_view, 2, |_| (Some(Value::Number(9.0)), ())),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.shared_typed_array_atomic_modify(shared_view, 2, |_| {
            (Some(Value::Number(9.0)), ())
        }),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.shared_typed_array_atomic_modify(shared_view, 0, |value| {
            (Some(Value::Number(7.0)), value)
        }),
        Ok(Value::Number(0.0))
    );
    assert_eq!(
        heap.shared_typed_array_atomic_modify(shared_view, 0, |value| (None, value)),
        Ok(Value::Number(7.0))
    );
    assert_eq!(
        heap.typed_array_index_value(shared_view, 0),
        Ok(Some(Value::Number(7.0)))
    );
    assert_eq!(heap.buffer_max_byte_length(shared), Ok(2));
}

#[test]
fn uint8_clamping_obeys_range_and_ties_to_even() {
    let heap = Heap::default();
    for (input, expected) in [
        (f64::NAN, 0.0),
        (-1.0, 0.0),
        (300.0, 255.0),
        (1.5, 2.0),
        (2.5, 2.0),
        (2.6, 3.0),
    ] {
        assert_eq!(
            heap.typed_array_normalize_value(TypedArrayKind::Uint8Clamped, &Value::Number(input)),
            Value::Number(expected)
        );
    }
}

#[test]
#[should_panic(expected = "BigInt typed arrays receive a BigInt element value")]
fn bigint_typed_write_rejects_a_number_before_mutating_bytes() {
    binary_data::typed_write(TypedArrayKind::BigInt64, &mut [0; 8], &Value::Number(1.0));
}

#[test]
#[should_panic(expected = "numeric typed arrays receive a Number element value")]
fn numeric_typed_write_rejects_a_bigint_before_mutating_bytes() {
    binary_data::typed_write(
        TypedArrayKind::Uint8,
        &mut [0],
        &Value::BigInt(BigInt::from(1)),
    );
}

#[test]
fn binary_data_rejects_foreign_heap_ids_through_each_accessor_family() {
    let mut heap = Heap::default();
    let mut other = Heap::default();
    let foreign = other.alloc_object(None).unwrap();
    macro_rules! missing {
        () => {
            Err(HeapError::InvalidObject(foreign))
        };
    }
    assert_eq!(heap.is_array_buffer(foreign), missing!());
    assert_eq!(heap.is_shared_array_buffer(foreign), missing!());
    assert_eq!(heap.is_buffer(foreign), missing!());
    assert_eq!(heap.is_data_view(foreign), missing!());
    assert_eq!(heap.is_typed_array(foreign), missing!());
    assert_eq!(heap.array_buffer_byte_length(foreign), missing!());
    assert_eq!(heap.buffer_byte_length(foreign), missing!());
    assert_eq!(heap.shared_buffer_backing(foreign).map(|_| ()), missing!());
    assert_eq!(heap.array_buffer_is_detached(foreign), missing!());
    assert_eq!(heap.buffer_is_detached(foreign), missing!());
    assert_eq!(heap.buffer_is_shared(foreign), missing!());
    assert_eq!(heap.buffer_is_immutable(foreign), missing!());
    assert_eq!(heap.typed_array_is_immutable(foreign), missing!());
    assert_eq!(heap.buffer_max_byte_length(foreign), missing!());
    assert_eq!(heap.buffer_resizable(foreign), missing!());
    assert_eq!(heap.array_buffer_is_resizable(foreign), missing!());
    assert_eq!(heap.buffer_growable(foreign), missing!());
    assert_eq!(heap.alloc_data_view(foreign, 0, 0, false, None), missing!());
    assert_eq!(
        heap.alloc_typed_array(foreign, 0, 0, false, TypedArrayKind::Uint8, None),
        missing!()
    );
    assert_eq!(heap.detach_array_buffer(foreign), missing!());
    assert_eq!(heap.resize_array_buffer(foreign, 0), missing!());
    assert_eq!(heap.array_buffer_copy(foreign, 0, 0), missing!());
    assert_eq!(heap.array_buffer_write(foreign, 0, &[]), missing!());
    assert_eq!(heap.data_view_raw_info(foreign), missing!());
    assert_eq!(heap.data_view_current_info(foreign), missing!());
    assert_eq!(heap.data_view_buffer(foreign), missing!());
    assert_eq!(heap.typed_array_info(foreign), missing!());
    assert_eq!(heap.typed_array_is_out_of_bounds(foreign), missing!());
    assert_eq!(heap.typed_array_is_length_tracking(foreign), missing!());
    assert_eq!(
        heap.typed_array_prevent_extensions_allowed(foreign),
        missing!()
    );
    assert_eq!(
        heap.typed_array_numeric_key(foreign, &"0".into()),
        missing!()
    );
    assert_eq!(heap.typed_array_index_value(foreign, 0), missing!());
    assert_eq!(
        heap.typed_array_set_index(foreign, 0, &Value::Number(1.0)),
        missing!()
    );
    assert_eq!(
        heap.shared_typed_array_atomic_modify(foreign, 0, |value| (None, value)),
        missing!()
    );
    assert_eq!(
        heap.typed_array_atomic_modify(foreign, 0, |value| (None, value)),
        missing!()
    );
}

#[test]
fn malformed_view_backings_fail_without_reading_ordinary_object_storage() {
    let mut heap = Heap::default();
    let ordinary = heap.alloc_object(None).unwrap();
    let view = heap
        .alloc(
            ObjectKind::DataView {
                buffer: ordinary,
                byte_offset: 0,
                byte_length: 1,
                length_tracking: false,
            },
            None,
        )
        .unwrap();
    let typed = heap
        .alloc(
            ObjectKind::TypedArray {
                buffer: ordinary,
                byte_offset: 0,
                length: 1,
                length_tracking: false,
                kind: TypedArrayKind::Uint8,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        heap.data_view_current_info(view),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(
        heap.typed_array_info(typed),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(
        heap.typed_array_is_out_of_bounds(typed),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(
        heap.typed_array_prevent_extensions_allowed(typed),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(
        heap.typed_array_index_value(typed, 0),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(
        heap.typed_array_set_index(typed, 0, &Value::Number(1.0)),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(
        heap.shared_typed_array_atomic_modify(typed, 0, |value| (None, value)),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
    assert_eq!(
        heap.typed_array_atomic_modify(typed, 0, |value| (None, value)),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
}

#[test]
fn buffer_view_construction_and_byte_ranges_reject_detachment_and_overflow() {
    let mut heap = Heap::default();
    let buffer = heap.alloc_array_buffer(4, None).unwrap();
    assert_eq!(
        heap.array_buffer_copy(buffer, usize::MAX, 1),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.array_buffer_copy(buffer, 3, 2),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.array_buffer_write(buffer, usize::MAX, &[1]),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(
        heap.array_buffer_write(buffer, 3, &[1, 2]),
        Err(HeapError::InvalidBufferRange)
    );
    assert_eq!(heap.array_buffer_copy(buffer, 0, 4), Ok(vec![0; 4]));
    heap.detach_array_buffer(buffer).unwrap();
    assert_eq!(
        heap.alloc_data_view(buffer, 0, 0, false, None),
        Err(HeapError::DetachedArrayBuffer)
    );
    assert_eq!(
        heap.alloc_typed_array(buffer, 0, 0, false, TypedArrayKind::Uint8, None),
        Err(HeapError::DetachedArrayBuffer)
    );
}

#[test]
fn only_a_length_tracking_view_of_a_growable_shared_buffer_can_gain_indices() {
    let mut heap = Heap::default();
    let shared = heap.alloc_shared_array_buffer(2, Some(4), None).unwrap();
    let fixed = heap
        .alloc_typed_array(shared, 0, 2, false, TypedArrayKind::Uint8, None)
        .unwrap();
    let tracking = heap
        .alloc_typed_array(shared, 0, 0, true, TypedArrayKind::Uint8, None)
        .unwrap();
    assert_eq!(heap.typed_array_prevent_extensions_allowed(fixed), Ok(true));
    assert_eq!(
        heap.typed_array_prevent_extensions_allowed(tracking),
        Ok(false)
    );
    let ordinary = heap.alloc_array_buffer(2, None).unwrap();
    let plain_view = heap
        .alloc_typed_array(ordinary, 0, 2, false, TypedArrayKind::Uint8, None)
        .unwrap();
    assert_eq!(
        heap.shared_typed_array_atomic_modify(plain_view, 0, |value| (None, value)),
        Err(HeapError::InvalidInternalSlot(ordinary))
    );
}

#[test]
fn numeric_index_keys_reject_ill_formed_utf16_and_noncanonical_spellings() {
    let ill_formed = PropertyName::String(JsString::from_code_units(vec![0xd800]));
    assert_eq!(binary_data::typed_array_numeric_key(&ill_formed), None);
    assert_eq!(
        binary_data::typed_array_numeric_key(&"9007199254740991".into()),
        Some(TypedArrayNumericKey::Index(9_007_199_254_740_991))
    );
    assert_eq!(binary_data::typed_array_numeric_key(&"01".into()), None);
    assert_eq!(binary_data::ecmascript_number_string(0.0), "0");
    assert_eq!(
        binary_data::normalize_scientific_rendering("1e21".into()),
        "1e+21"
    );
    assert_eq!(
        binary_data::normalize_scientific_rendering("-1.5e-7".into()),
        "-1.5e-7"
    );
    assert_eq!(binary_data::normalize_scientific_rendering("0".into()), "0");
    assert_eq!(binary_data::scientific_to_fixed("1.0e2147483647"), None);
    assert_eq!(binary_data::f64_to_f16_bits(1.0006), 0x3c01);
    assert_eq!(binary_data::f64_to_f16_bits(1.5 * 2f64.powi(-24)), 0x0002);
}
