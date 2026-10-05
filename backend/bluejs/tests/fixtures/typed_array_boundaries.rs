// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! TypedArray ownership, copying, and real resource boundaries.

use super::*;
use crate::{compile, parse};

fn script(source: &str) -> Bytecode {
    compile(&parse(source).unwrap()).unwrap()
}

fn rooted_view(vm: &mut Vm, kind: TypedArrayKind) -> (ObjectId, RootId) {
    let buffer = vm
        .heap
        .alloc_array_buffer(2 * kind.byte_width(), None)
        .unwrap();
    let buffer_root = vm.heap.root(buffer).unwrap();
    let view = vm
        .heap
        .alloc_typed_array(buffer, 0, 2, false, kind, None)
        .unwrap();
    let root = vm.heap.root(view).unwrap();
    vm.heap.unroot(buffer_root).unwrap();
    (view, root)
}

#[cfg_attr(test, test)]
fn typed_snapshots_check_overflow_bounds_and_foreign_ownership() {
    let mut vm = Vm::default();
    let (view, root) = rooted_view(&mut vm, TypedArrayKind::Uint8);
    assert_eq!(
        vm.typed_array_read_values(view, usize::MAX, 1),
        Err(RuntimeError::RangeError(
            "TypedArray range is too large".into()
        ))
    );
    assert_eq!(
        vm.typed_array_read_values(view, 1, 2),
        Err(RuntimeError::TypeError(
            "TypedArray is out of bounds".into()
        ))
    );
    assert_eq!(
        vm.typed_array_read_values(view, 0, 2),
        Ok(vec![Value::Number(0.0), Value::Number(0.0)])
    );
    let mut owner = Vm::default();
    let (foreign, foreign_root) = rooted_view(&mut owner, TypedArrayKind::Uint8);
    assert_eq!(
        vm.typed_array_read_values(foreign, 0, 1),
        Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
    );
    assert_eq!(
        vm.typed_array_write_values(foreign, TypedArrayKind::Uint8, 0, &[Value::Number(7.0)]),
        Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
    );
    owner.heap.unroot(foreign_root).unwrap();
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn typed_callback_and_constructor_ingress_reject_foreign_handles() {
    let mut owner = Vm::default();
    let callback = owner.execute_script(&script("(() => 1)")).unwrap();
    let foreign = callback.object_id().unwrap();
    let foreign_root = owner.heap.root(foreign).unwrap();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)));
    let mut vm = Vm::default();
    let (view, root) = rooted_view(&mut vm, TypedArrayKind::Uint8);
    for method in [
        TypedArrayMethod::Every,
        TypedArrayMethod::ForEach,
        TypedArrayMethod::Some,
        TypedArrayMethod::Find,
        TypedArrayMethod::FindIndex,
        TypedArrayMethod::FindLast,
        TypedArrayMethod::FindLastIndex,
        TypedArrayMethod::Map,
        TypedArrayMethod::Filter,
        TypedArrayMethod::Reduce,
        TypedArrayMethod::ReduceRight,
        TypedArrayMethod::Sort,
        TypedArrayMethod::ToSorted,
    ] {
        assert_eq!(
            vm.typed_array_method(
                &Value::Object(view),
                std::slice::from_ref(&callback),
                method
            ),
            expected
        );
        assert!(vm.stack.is_empty());
    }
    assert_eq!(
        vm.typed_array_create(callback.clone(), 2)
            .map(|_| Value::Undefined),
        expected
    );
    assert_eq!(
        vm.typed_array_method(&callback, &[], TypedArrayMethod::ToLocaleString),
        expected
    );
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(root).unwrap();
    owner.heap.unroot(foreign_root).unwrap();
}

#[cfg_attr(test, test)]
fn cold_typed_copy_builders_preserve_backings_across_collection_and_refusal() {
    for kind in [
        TypedArrayKind::Uint8,
        TypedArrayKind::Float64,
        TypedArrayKind::BigInt64,
    ] {
        for nursery in [VmConfig::default().heap.nursery_capacity, 1] {
            let mut config = VmConfig::default();
            config.heap.nursery_capacity = nursery;
            if nursery == 1 {
                config.heap.major_threshold_bytes = 1;
            }
            let mut vm = Vm::new(config).unwrap();
            let view = vm.typed_array_new_same_kind(2, kind).unwrap();
            assert_eq!(vm.heap.typed_array_info(view).unwrap().2, 2);
            assert!(vm.stack.is_empty());
        }
        // ArrayBuffer is ready, while this concrete constructor remains cold.
        let mut completed = false;
        let mut refused = 0;
        for remaining in (0..=65_536).step_by(128) {
            let mut vm = Vm::default();
            vm.buffer_prototype("ArrayBuffer").unwrap();
            vm.heap.allow_only(remaining);
            match vm.typed_array_new_same_kind(2, kind) {
                Ok(view) => {
                    assert_eq!(vm.heap.typed_array_info(view).unwrap().2, 2);
                    completed = true;
                }
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => refused += 1,
                Err(error) => panic!("{kind:?}, {remaining}: {error:?}"),
            }
            assert!(vm.stack.is_empty());
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(
                vm.execute_script(&script("21 + 21")),
                Ok(Value::Number(42.0))
            );
            if completed {
                break;
            }
        }
        assert!(completed && refused > 0);
    }
}

#[cfg_attr(test, test)]
fn cold_species_initialization_and_numeric_join_preserve_resource_errors() {
    for slice in [false, true] {
        let mut vm = Vm::default();
        vm.string_intrinsics().unwrap();
        let (view, root) = rooted_view(&mut vm, TypedArrayKind::Uint8);
        let limit = vm.heap.allow_only(0);
        let result = if slice {
            vm.typed_array_slice(&Value::Object(view), &[]).map(|_| ())
        } else {
            vm.typed_array_species_create(&Value::Object(view), 2, TypedArrayKind::Uint8)
                .map(|_| ())
        };
        assert_eq!(
            result,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.execute_script(&script("21 + 21")),
            Ok(Value::Number(42.0))
        );
        vm.heap.unroot(root).unwrap();
    }
    let mut vm = Vm::default();
    let (view, root) = rooted_view(&mut vm, TypedArrayKind::Float64);
    vm.typed_array_write_values(
        view,
        TypedArrayKind::Float64,
        0,
        &[Value::Number(123_456.0)],
    )
    .unwrap();
    vm.config.max_string_bytes = 2;
    vm.remaining_instructions = vm.config.instruction_budget;
    assert_eq!(
        vm.typed_array_join(&Value::Object(view), &Value::String("".into())),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn foreign_species_results_validate_the_brand_and_preserve_vm_reuse() {
    for nursery_capacity in [VmConfig::default().heap.nursery_capacity, 1] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        let mut vm = Vm::new(config).unwrap();
        vm.install_test262_harness().unwrap();
        vm.execute_script(&script(
            "var source = new Uint8Array([1,2,3]); var child = $262.createRealm().global;\
             child.eval('globalThis.Plain = function(n) {return {length:100};};\
             globalThis.Detached = class extends Uint8Array {constructor(n) {super(n);\
             $262.detachArrayBuffer(this.buffer);}};');",
        ))
        .unwrap();
        for operation in [
            "source.constructor = {[Symbol.species]: child.Plain};\
             try {source.slice(1); 0;} catch(error) {error instanceof TypeError ? 42 : 0;}",
            "source.constructor = {[Symbol.species]: child.Detached};\
             try {source.slice(1,1); 0;} catch(error) {error instanceof TypeError ? 42 : 0;}",
        ] {
            assert_eq!(
                vm.execute_script(&script(operation)),
                Ok(Value::Number(42.0)),
                "{operation}"
            );
            assert!(vm.stack.is_empty());
            assert_eq!(
                vm.execute_script(&script("21 + 21")),
                Ok(Value::Number(42.0))
            );
        }
    }
}

impl Vm {
    /// Runs TypedArray boundary contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_typed_array_boundary_contracts() {
        foreign_species_results_validate_the_brand_and_preserve_vm_reuse();
        typed_snapshots_check_overflow_bounds_and_foreign_ownership();
        typed_callback_and_constructor_ingress_reject_foreign_handles();
        cold_typed_copy_builders_preserve_backings_across_collection_and_refusal();
        cold_species_initialization_and_numeric_join_preserve_resource_errors();
    }
}
