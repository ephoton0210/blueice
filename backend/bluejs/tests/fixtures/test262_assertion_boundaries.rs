// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assertion ingress and allocation contracts, without corrupting heap state.

use super::*;
use crate::{compile, parse};

#[cfg_attr(test, test)]
fn assertion_dispatch_rejects_foreign_handles_and_restores_its_roots() {
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let foreign = Value::Object(object);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let sentinel = Value::String("caller root".into());
    vm.stack.push(sentinel.clone());
    assert_eq!(
        vm.test262_assertion_call("another native", std::slice::from_ref(&foreign)),
        None
    );
    assert_eq!(vm.stack, vec![sentinel.clone()]);
    for (name, args) in [
        ("assert", vec![Value::Bool(false), foreign.clone()]),
        ("isPrimitive", vec![foreign.clone()]),
        ("formatIdentityFreeValue", vec![foreign.clone()]),
        ("formatSimpleValue", vec![foreign.clone()]),
        ("throws", vec![Value::Undefined, foreign.clone()]),
        (
            "compareArray",
            vec![Value::Undefined, Value::Undefined, foreign.clone()],
        ),
        ("compareArray", vec![foreign.clone(), Value::Undefined]),
    ] {
        assert_eq!(
            vm.test262_assertion_call(name, &args),
            Some(Err(RuntimeError::Heap(HeapError::InvalidObject(object)))),
            "{name}"
        );
        assert_eq!(vm.stack, vec![sentinel.clone()], "{name}");
    }
    vm.stack.clear();
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn a_cold_assertion_preserves_constructor_allocation_failure_and_allows_retry() {
    let mut vm = Vm::default();
    vm.remaining_instructions = vm.config.instruction_budget;
    let limit = vm.heap.allow_only(0);
    let args = [Value::Bool(false), Value::Number(7.0)];
    assert_eq!(
        vm.test262_assertion_call("assert", &args),
        Some(Err(RuntimeError::Heap(HeapError::HeapLimitExceeded {
            limit
        })))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    let result = vm.test262_assertion_call("assert", &args);
    let Some(Err(RuntimeError::Thrown(error))) = result else {
        panic!("retry must construct the original Test262Error: {result:?}");
    };
    assert_eq!(
        vm.get_property(&error, &"message".into()),
        Ok(Value::Number(7.0))
    );
    assert!(vm.stack.is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_test262_assertion_boundary_contracts() {
        assertion_dispatch_rejects_foreign_handles_and_restores_its_roots();
        a_cold_assertion_preserves_constructor_allocation_failure_and_allows_retry();
    }
}
