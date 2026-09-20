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
        heap.set_class_base(ordinary, Value::Null),
        Err(HeapError::InvalidObject(ordinary))
    );
    assert_eq!(
        heap.class_base(ordinary),
        Err(HeapError::InvalidObject(ordinary))
    );
    heap.set_class_base(closure, Value::Null).unwrap();
    assert_eq!(heap.class_base(closure).unwrap(), Some(Value::Null));
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
    };
    let references = state.references();
    for object in [
        stack, binding, this, argument, completion, cell, dynamic, home, iterator, pending, saved,
        delegate,
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
