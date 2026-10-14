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
        slice_callbacks_preserve_source_detachment_and_cross_realm_copy_values();
        foreign_species_results_validate_the_brand_and_preserve_vm_reuse();
        foreign_species_validation_uses_internal_length_without_getters();
        typed_snapshots_check_overflow_bounds_and_foreign_ownership();
        typed_callback_and_constructor_ingress_reject_foreign_handles();
        cold_typed_copy_builders_preserve_backings_across_collection_and_refusal();
        cold_species_initialization_and_numeric_join_preserve_resource_errors();
    }
}

#[cfg_attr(test, test)]
fn foreign_species_validation_uses_internal_length_without_getters() {
    for nursery_capacity in [1, HeapConfig::default().nursery_capacity] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        vm.install_test262_harness().unwrap();
        let source = r#"
            var child = $262.createRealm().global;
            var source = new Uint8Array([7,42]);
            child.eval(`
                globalThis.reads = 0;
                globalThis.Valid = class extends Uint8Array {
                    get length() {reads++; throw 'observable length';}
                };
                globalThis.Short = function(n) {
                    var result = new Uint8Array(0);
                    Object.defineProperty(result,'length',{value:100});
                    return result;
                };
                globalThis.Immutable = function(n) {
                    return new Uint8Array(new ArrayBuffer(n).transferToImmutable());
                };
            `);
            source.constructor = {[Symbol.species]:child.Valid};
            var copied = source.slice();
            if (copied[0] !== 7 || copied[1] !== 42 || child.reads !== 0) throw 'length getter ran';
            for (var constructor of [child.Short, child.Immutable]) {
                source.constructor = {[Symbol.species]:constructor};
                var rejected = false;
                try {source.slice();} catch(error) {rejected = error instanceof TypeError;}
                if (!rejected) throw 'invalid foreign result accepted';
            }
            // A foreign constructor can hand back an imported object from
            // this realm. Its original immutable buffer must survive the
            // membrane round trip and fail the local validation boundary.
            child.immutableLocal = new Uint8Array(new ArrayBuffer(2).transferToImmutable());
            child.eval('globalThis.ReturnLocal = function(n) {return immutableLocal;};');
            source.constructor = {[Symbol.species]:child.ReturnLocal};
            for (var create of [
                () => source.slice(),
                () => Uint8Array.of.call(child.ReturnLocal, 7, 42),
                () => Uint8Array.from.call(child.ReturnLocal, [7,42])
            ]) {
                var rejected = false;
                try {create();} catch(error) {rejected = error instanceof TypeError;}
                if (!rejected) throw 'immutable local species accepted';
            }
            source.constructor = Uint8Array;
            child.eval(`
                Object.defineProperty(Uint8Array.prototype,'length', {
                    configurable:true,
                    get() {reads++; $262.detachArrayBuffer(this.buffer); return 100;}
                });
            `);
            var reverse = child.Uint8Array.prototype.slice.call(source);
            if (reverse[0] !== 7 || reverse[1] !== 42 || child.reads !== 0) throw 'reverse length getter ran';
            delete child.Uint8Array.prototype.length;
            42;
        "#;
        assert_eq!(vm.execute_script(&script(source)), Ok(Value::Number(42.0)));
        assert!(vm.stack.is_empty());
        assert_eq!(
            vm.execute_script(&script("21 + 21")),
            Ok(Value::Number(42.0))
        );
    }
}

#[cfg_attr(test, test)]
fn slice_callbacks_preserve_source_detachment_and_cross_realm_copy_values() {
    for nursery_capacity in [1, HeapConfig::default().nursery_capacity] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        vm.install_test262_harness().unwrap();
        let source = r#"
            var child = $262.createRealm().global;
            globalThis.source = new Uint8Array([7,42]);
            child.detachSource = function() {$262.detachArrayBuffer(source.buffer);};
            child.eval('globalThis.Foreign = class extends Uint8Array {get buffer() {detachSource(); return super.buffer;}};');
            source.constructor = {[Symbol.species]:child.Foreign};
            var rejected = false;
            try {source.slice(0);} catch (error) {rejected = error instanceof TypeError;}
            if (!rejected || source.buffer.byteLength !== 0) throw 'detached source was accepted';
            var first = new Uint8Array([7,42]);
            first.constructor = {[Symbol.species]:child.Float64Array};
            var converted = first.slice();
            child.collectParent = function() {$262.gc();};
            child.eval('globalThis.Collected = class extends Float64Array {get set() {collectParent(); return super.set;}}; globalThis.Refused = class extends Float64Array {get set() {throw 7;}};');
            first.constructor = {[Symbol.species]:child.Collected};
            var collected = first.slice();
            first.constructor = {[Symbol.species]:child.Refused};
            var caught = false;
            try {first.slice();} catch (error) {caught = error === 7;}
            var second = new Uint8Array([7,42]);
            var reverse = child.Uint8Array.prototype.slice.call(second);
            converted[0] === 7 && converted[1] === 42 && collected[0] === 7 && collected[1] === 42 && caught && reverse[0] === 7 && reverse[1] === 42 ? 42 : 0;
        "#;
        assert_eq!(vm.execute_script(&script(source)), Ok(Value::Number(42.0)));
        assert!(vm.stack.is_empty());
        assert_eq!(
            vm.execute_script(&script("21 + 21")),
            Ok(Value::Number(42.0))
        );
    }
}
