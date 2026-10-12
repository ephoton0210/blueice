// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Genuine object ingress and root exhaustion in both library instantiations.

use super::*;
use crate::{compile, parse};

#[cfg_attr(test, test)]
fn collection_and_host_ingress_reject_foreign_objects_without_losing_roots() {
    let mut owner = Vm::default();
    let id = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(id).unwrap();
    let foreign = Value::Object(id);
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(id)));
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    let source = vm.heap.alloc_object(None).unwrap();
    let source_root = vm.heap.root(source).unwrap();
    let source = Value::Object(source);
    let sentinel = Value::String("caller root".into());
    vm.stack.push(sentinel.clone());
    macro_rules! rejects_foreign {
        ($operation:expr) => {
            let result = $operation;
            assert_eq!(result, expected);
            assert_eq!(vm.stack, vec![sentinel.clone()]);
        };
    }
    for map in [true, false] {
        rejects_foreign!(vm.collection_for_each(id, map, std::slice::from_ref(&foreign)));
    }
    rejects_foreign!(vm.object_group_by_method(&source, &foreign));
    rejects_foreign!(vm.map_group_by_method(&source, &foreign));
    rejects_foreign!(vm.array_from_method(&Value::Undefined, &[source.clone(), foreign.clone()]));
    rejects_foreign!(vm.array_from_method(&foreign, std::slice::from_ref(&source)));
    rejects_foreign!(vm.array_of_method(&foreign, &[]));
    rejects_foreign!(vm.bind_function(foreign.clone(), &[]));
    rejects_foreign!(vm.test262_call("setTimeout", std::slice::from_ref(&foreign)));
    for width in [3.0, 4.0] {
        rejects_foreign!(vm.test262_decode_uri_exhaustive(&foreign, &Value::Number(width)));
    }
    rejects_foreign!(vm.test262_encode_uri_exhaustive(
        &foreign,
        &Value::Number(2048.0),
        &Value::Number(2048.0),
    ));
    assert_eq!(
        vm.has_instance(Value::Null, foreign.clone(), true),
        expected.clone().map(|_| false)
    );
    assert_eq!(vm.stack, vec![sentinel.clone()]);
    rejects_foreign!(vm
        .temporal_to_plain_date(&foreign, &Value::Undefined)
        .map(|_| Value::Undefined));
    rejects_foreign!(vm
        .temporal_to_plain_date_time(&foreign, &Value::Undefined)
        .map(|_| Value::Undefined));
    vm.stack.clear();
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(source_root).unwrap();
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn collection_iterator_root_exhaustion_preserves_cached_base_prototypes() {
    for map in [true, false] {
        let mut vm = Vm::default();
        let base = vm.base_iterator_prototype().unwrap();
        let function = vm.function_prototype().unwrap();
        vm.heap.allow_root_registrations(0);
        assert_eq!(
            vm.collection_iterator_prototype(map),
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        );
        assert_eq!(vm.iterator_base, Some(base));
        assert_eq!(vm.function_intrinsic_prototype, Some(function));
        assert!(vm.map_iterator_prototype.is_none());
        assert!(vm.set_iterator_prototype.is_none());
        assert!(vm.heap.contains(base));
        assert!(vm.heap.contains(function));
        assert!(vm.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn has_instance_reports_a_released_foreign_intrinsic_and_restores_operands() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let target = vm
        .execute_script(
            &compile(
                &parse(
                    r#"
        globalThis.instanceTarget = {
            [Symbol.hasInstance]: $262.createRealm().global.Function.prototype[Symbol.hasInstance]
        };
        instanceTarget
    "#,
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    vm.test262_realms.clear();
    let sentinel = Value::Number(17.0);
    vm.stack.push(sentinel.clone());
    assert_eq!(
        vm.has_instance(Value::Null, target, false),
        Err(RuntimeError::TypeError(
            "foreign Test262 realm is no longer available".into()
        ))
    );
    assert_eq!(vm.stack, vec![sentinel]);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_remaining_region_boundary_contracts() {
        collection_and_host_ingress_reject_foreign_objects_without_losing_roots();
        collection_iterator_root_exhaustion_preserves_cached_base_prototypes();
        has_instance_reports_a_released_foreign_intrinsic_and_restores_operands();
    }
}
