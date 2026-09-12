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
        .alloc_typed_array(buffer, 0, 4, TypedArrayKind::Uint8, None)
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
