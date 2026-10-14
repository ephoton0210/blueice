// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::{compile, parse};

fn number(value: f64) -> Value {
    Value::Number(value)
}

fn bigint(value: i64) -> Value {
    Value::BigInt(value.into())
}

#[cfg_attr(test, test)]
fn numeric_atomic_operations_wrap_at_the_element_width() {
    let apply = |operation| {
        Vm::atomics_binary_value(
            TypedArrayKind::Int32,
            &number(12.0),
            &number(10.0),
            operation,
        )
    };
    assert_eq!(apply(AtomicBinaryOperation::Add), number(22.0));
    assert_eq!(apply(AtomicBinaryOperation::And), number(8.0));
    assert_eq!(apply(AtomicBinaryOperation::Or), number(14.0));
    assert_eq!(apply(AtomicBinaryOperation::Sub), number(2.0));
    assert_eq!(apply(AtomicBinaryOperation::Xor), number(6.0));
}

#[cfg_attr(test, test)]
fn bigint_atomic_operations_are_exact() {
    let apply = |operation| {
        Vm::atomics_binary_value(
            TypedArrayKind::BigInt64,
            &bigint(12),
            &bigint(10),
            operation,
        )
    };
    assert_eq!(apply(AtomicBinaryOperation::Add), bigint(22));
    assert_eq!(apply(AtomicBinaryOperation::And), bigint(8));
    assert_eq!(apply(AtomicBinaryOperation::Or), bigint(14));
    assert_eq!(apply(AtomicBinaryOperation::Sub), bigint(2));
    assert_eq!(apply(AtomicBinaryOperation::Xor), bigint(6));
}

#[cfg(test)]
#[test]
#[should_panic(expected = "BigInt atomic operations have BigInt operands")]
fn a_bigint_element_never_meets_a_number_operand() {
    Vm::atomics_binary_value(
        TypedArrayKind::BigInt64,
        &bigint(1),
        &number(1.0),
        AtomicBinaryOperation::Add,
    );
}

#[cfg(test)]
#[test]
#[should_panic(expected = "numeric atomic operations have Number operands")]
fn a_numeric_element_never_meets_a_bigint_operand() {
    Vm::atomics_binary_value(
        TypedArrayKind::Int32,
        &number(1.0),
        &bigint(1),
        AtomicBinaryOperation::Add,
    );
}

#[cfg_attr(test, test)]
fn atomic_reads_reject_wrong_heap_handles_and_out_of_bounds_indices() {
    let mut vm = Vm::default();
    let value = vm
        .execute_script(&compile(&parse("new Int32Array(1)").unwrap()).unwrap())
        .unwrap();
    let object = value.object_id().unwrap();
    let root = vm.heap.root(object).unwrap();
    assert_eq!(vm.atomics_read(object, 0), Ok(Value::Number(0.0)));
    assert_eq!(
        vm.atomics_read(object, 1),
        Err(RuntimeError::TypeError(
            "TypedArray is out of bounds".into()
        ))
    );
    assert_eq!(
        vm.atomics_revalidate(object, 1),
        Err(RuntimeError::RangeError(
            "Atomics index is outside TypedArray".into()
        ))
    );
    let mut other = Vm::default();
    let foreign = other.heap.alloc_object(None).unwrap();
    let foreign_root = other.heap.root(foreign).unwrap();
    let expected = RuntimeError::Heap(HeapError::InvalidObject(foreign));
    assert_eq!(vm.atomics_read(foreign, 0), Err(expected.clone()));
    assert_eq!(vm.atomics_revalidate(foreign, 0), Err(expected));
    assert_eq!(
        vm.buffer_prototype("globalThis"),
        Err(RuntimeError::TypeError(
            "buffer prototype is unavailable".into()
        ))
    );
    other.heap.unroot(foreign_root).unwrap();
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn binary_data_operations_reject_foreign_heap_handles_before_accessing_storage() {
    type Operation = fn(&mut Vm, &Value) -> Result<(), RuntimeError>;
    let operations: &[(&str, Operation)] = &[
        ("ArrayBuffer receiver", |vm, value| {
            vm.array_buffer_receiver(value).map(|_| ())
        }),
        ("SharedArrayBuffer receiver", |vm, value| {
            vm.shared_array_buffer_receiver(value).map(|_| ())
        }),
        ("ArrayBuffer resize", |vm, value| {
            vm.buffer_resize(value, &Value::Number(0.0)).map(|_| ())
        }),
        ("SharedArrayBuffer grow", |vm, value| {
            vm.shared_buffer_grow(value, &Value::Number(0.0))
                .map(|_| ())
        }),
        ("ArrayBuffer transfer", |vm, value| {
            vm.array_buffer_transfer(value, &[], false).map(|_| ())
        }),
        ("ArrayBuffer slice", |vm, value| {
            vm.array_buffer_slice(value, &[]).map(|_| ())
        }),
        ("SharedArrayBuffer slice", |vm, value| {
            vm.shared_array_buffer_slice(value, &[]).map(|_| ())
        }),
        ("ArrayBuffer species", |vm, value| {
            vm.array_buffer_species_constructor(value).map(|_| ())
        }),
        ("SharedArrayBuffer species", |vm, value| {
            vm.shared_array_buffer_species_constructor(value)
                .map(|_| ())
        }),
        ("ArrayBuffer species result", |vm, value| {
            vm.species_result_buffer(value).map(|_| ())
        }),
        ("SharedArrayBuffer species result", |vm, value| {
            vm.shared_species_result_buffer(value).map(|_| ())
        }),
        ("DataView construction", |vm, value| {
            vm.data_view_constructor(std::slice::from_ref(value), true)
                .map(|_| ())
        }),
        ("TypedArray construction", |vm, value| {
            vm.typed_array_constructor(std::slice::from_ref(value), true, TypedArrayKind::Int32)
                .map(|_| ())
        }),
        ("TypedArray set", |vm, value| {
            vm.typed_array_set(value, &[]).map(|_| ())
        }),
        ("TypedArray subarray", |vm, value| {
            vm.typed_array_subarray(value, &[]).map(|_| ())
        }),
        ("TypedArray values", |vm, value| {
            vm.typed_array_values(value.object_id().unwrap(), 1)
                .map(|_| ())
        }),
        ("TypedArray from", |vm, value| {
            vm.typed_array_from(value, &[]).map(|_| ())
        }),
        ("TypedArray of", |vm, value| {
            vm.typed_array_of(value, &[]).map(|_| ())
        }),
        ("Atomics access", |vm, value| {
            vm.atomics_access(std::slice::from_ref(value), false)
                .map(|_| ())
        }),
        ("Atomics wait access", |vm, value| {
            vm.atomics_wait_access(std::slice::from_ref(value))
                .map(|_| ())
        }),
        ("Atomics wait", |vm, value| {
            vm.atomics_wait_status(std::slice::from_ref(value))
                .map(|_| ())
        }),
        ("Atomics waitAsync", |vm, value| {
            vm.atomics_wait_async(std::slice::from_ref(value))
                .map(|_| ())
        }),
        ("Atomics notify", |vm, value| {
            vm.atomics_notify(std::slice::from_ref(value)).map(|_| ())
        }),
    ];
    let mut vm = Vm::default();
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let receiver = Value::Object(object);
    for (name, operation) in operations {
        let depth = vm.stack.len();
        let new_target = vm.new_target.clone();
        if *name == "DataView construction" {
            vm.new_target = vm.global("DataView").unwrap();
        } else if *name == "TypedArray construction" {
            vm.new_target = vm.global("Int32Array").unwrap();
        }
        let expected = if *name == "TypedArray subarray" {
            RuntimeError::TypeError("TypedArray method requires a TypedArray receiver".into())
        } else {
            RuntimeError::Heap(HeapError::InvalidObject(object))
        };
        assert_eq!(operation(&mut vm, &receiver), Err(expected), "{name}");
        vm.new_target = new_target;
        assert_eq!(vm.stack.len(), depth, "{name} left a temporary operand");
    }
    let expected = RuntimeError::TypeError("DataView method requires a DataView receiver".into());
    assert_eq!(
        vm.data_view_get(&receiver, &[], 1, false, false, false),
        Err(expected.clone())
    );
    assert_eq!(
        vm.data_view_set(&receiver, &[], 1, false, false, false),
        Err(expected)
    );
    assert!(owner.heap.contains(object));
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn binary_data_argument_and_species_guards_preserve_exact_error_classes() {
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(foreign).unwrap();
    let mut vm = Vm::default();
    let view = vm
        .execute_script(&compile(&parse("new DataView(new ArrayBuffer(8))").unwrap()).unwrap())
        .unwrap();
    let view_root = vm.heap.root(view.object_id().unwrap()).unwrap();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)));
    assert_eq!(
        vm.data_view_get(
            &view,
            &[Value::Number(0.0), Value::Object(foreign)],
            1,
            false,
            false,
            false
        ),
        expected
    );
    assert_eq!(
        vm.data_view_set(
            &view,
            &[
                Value::Number(0.0),
                Value::Number(7.0),
                Value::Object(foreign)
            ],
            1,
            false,
            false,
            false
        ),
        expected
    );
    assert_eq!(
        vm.data_view_get(&view, &[Value::Number(0.0)], 1, false, false, false),
        Ok(Value::Number(0.0))
    );
    let constructor = vm.global("Uint8Array").unwrap();
    assert_eq!(
        vm.typed_array_from(&constructor, &[Value::Undefined, Value::Object(foreign)]),
        expected
    );
    for value in [
        Value::Undefined,
        Value::Null,
        Value::Bool(false),
        Value::Number(7.0),
        Value::String("buffer".into()),
    ] {
        assert_eq!(
            vm.species_result_buffer(&value),
            Err(RuntimeError::TypeError(
                "ArrayBuffer method requires an ArrayBuffer receiver".into()
            ))
        );
        assert_eq!(
            vm.shared_species_result_buffer(&value),
            Err(RuntimeError::TypeError(
                "SharedArrayBuffer method requires a SharedArrayBuffer receiver".into()
            ))
        );
    }
    let ordinary = vm
        .execute_script(&compile(&parse("({get 0() {throw 7;}})").unwrap()).unwrap())
        .unwrap();
    assert_eq!(
        vm.typed_array_values(ordinary.object_id().unwrap(), 1),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    vm.execute_script(
        &compile(
            &parse("Object.defineProperty(globalThis, 'prototype', {get() {throw 7;}})").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.buffer_prototype("globalThis"),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(view_root).unwrap();
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn iterable_typed_array_sources_stop_at_the_configured_byte_limit() {
    let mut vm = Vm::default();
    vm.execute_script(
        &compile(
            &parse(
                r#"
        globalThis.result = {done:true, value:1};
        globalThis.iterable = {[Symbol.iterator]() {return this;}, next() {return result;}};
        new Float64Array(iterable); result.done = false; 0
    "#,
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    vm.heap.allow_only(16_384);
    assert_eq!(
        vm.execute_script(&compile(&parse("new Float64Array(iterable)").unwrap()).unwrap()),
        Err(RuntimeError::RangeError(
            "TypedArray length is too large".into()
        ))
    );
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(
        vm.execute_script(&compile(&parse("new Float64Array([1, 2]).length").unwrap()).unwrap()),
        Ok(Value::Number(2.0))
    );
}

impl Vm {
    /// Runs binary-data contracts only in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_binary_data_boundary_contracts() {
        typed_intrinsic_builders_cover_cold_and_warm_allocation_refusal();
        cold_buffer_fallback_and_typed_intrinsic_root_refusals_preserve_receivers();
        numeric_atomic_operations_wrap_at_the_element_width();
        bigint_atomic_operations_are_exact();
        atomic_reads_reject_wrong_heap_handles_and_out_of_bounds_indices();
        binary_data_operations_reject_foreign_heap_handles_before_accessing_storage();
        binary_data_argument_and_species_guards_preserve_exact_error_classes();
        iterable_typed_array_sources_stop_at_the_configured_byte_limit();
    }
}

#[cfg_attr(test, test)]
fn typed_intrinsic_builders_cover_cold_and_warm_allocation_refusal() {
    Vm::verify_intrinsic_allocation_boundary("TypedArray prototype", |vm| {
        vm.typed_array_intrinsics().map(|(_, prototype)| prototype)
    });
}

#[cfg_attr(test, test)]
fn cold_buffer_fallback_and_typed_intrinsic_root_refusals_preserve_receivers() {
    let mut cold = Vm::default();
    cold.string_intrinsics().unwrap();
    let backing = cold.heap.alloc_array_buffer(8, None).unwrap();
    let backing_root = cold.heap.root(backing).unwrap();
    let view = cold
        .heap
        .alloc_typed_array(backing, 0, 8, false, TypedArrayKind::Uint8, None)
        .unwrap();
    let view_root = cold.heap.root(view).unwrap();
    let limit = cold.heap.allow_only(0);
    assert_eq!(
        cold.typed_array_subarray(&Value::Object(view), &[]),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(cold.heap.buffer_byte_length(backing), Ok(8));
    assert!(cold.stack.is_empty());
    cold.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(
        cold.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    cold.heap.unroot(view_root).unwrap();
    cold.heap.unroot(backing_root).unwrap();
    let mut vm = Vm::default();
    assert!(matches!(
        vm.constructed_buffer_prototype("globalThis"),
        Err(RuntimeError::TypeError(_))
    ));
    let mut vm = Vm::default();
    let buffer = vm
        .heap
        .alloc_resizable_array_buffer(8, 16, Some(vm.object_prototype))
        .unwrap();
    let root = vm.heap.root(buffer).unwrap();
    vm.string_intrinsics().unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.array_buffer_transfer(&Value::Object(buffer), &[Value::Number(8.0)], false),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.heap.buffer_byte_length(buffer), Ok(8));
    assert!(!vm.heap.buffer_is_detached(buffer).unwrap());
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.heap.unroot(root).unwrap();

    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.typed_array_intrinsics(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.typed_array_intrinsics.is_none() && vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}
