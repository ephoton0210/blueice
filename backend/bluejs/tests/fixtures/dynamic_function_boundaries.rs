// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native dynamic-function builders at real cold and warm allocation boundaries.

use super::*;

#[cfg_attr(test, test)]
fn every_dynamic_function_kind_cleans_up_and_retries_after_allocation_refusal() {
    type Builder = fn(&mut Vm) -> Result<ObjectId, RuntimeError>;
    let builders: &[(&str, Builder)] = &[
        ("dynamic Function", |vm| {
            vm.dynamic_function_constructor(
                &[Value::String("return 42".into())],
                DynamicFunctionKind::Normal,
            )
            .map(|value| value.object_id().unwrap())
        }),
        ("dynamic AsyncFunction", |vm| {
            vm.dynamic_function_constructor(
                &[Value::String("return 42".into())],
                DynamicFunctionKind::Async,
            )
            .map(|value| value.object_id().unwrap())
        }),
        ("dynamic GeneratorFunction", |vm| {
            vm.dynamic_function_constructor(
                &[Value::String("yield 42".into())],
                DynamicFunctionKind::Generator,
            )
            .map(|value| value.object_id().unwrap())
        }),
        ("dynamic AsyncGeneratorFunction", |vm| {
            vm.dynamic_function_constructor(
                &[Value::String("yield 42".into())],
                DynamicFunctionKind::AsyncGenerator,
            )
            .map(|value| value.object_id().unwrap())
        }),
    ];
    for &(name, builder) in builders {
        Vm::verify_intrinsic_allocation_boundary(name, builder);
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_dynamic_function_boundary_contracts() {
        every_dynamic_function_kind_cleans_up_and_retries_after_allocation_refusal();
        object_source_boxing_and_pattern_fallbacks_preserve_real_errors();
    }
}

#[cfg_attr(test, test)]
fn object_source_boxing_and_pattern_fallbacks_preserve_real_errors() {
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    let target = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(target).unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.object_method(
            ObjectMethod::Assign,
            &Value::Undefined,
            &[Value::Object(target), Value::Number(7.0)]
        ),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.heap.unroot(root).unwrap();
    let receiver = vm
        .execute_script(
            &crate::compile(&crate::parse("({[Symbol.toPrimitive]() {throw 7;}})").unwrap())
                .unwrap(),
        )
        .unwrap();
    for method in [
        PatternMethod::Match,
        PatternMethod::MatchAll,
        PatternMethod::Search,
    ] {
        assert_eq!(
            vm.string_pattern(method, &receiver, &[]),
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        );
    }
    vm.execute_script(&crate::compile(&crate::parse(
        "Object.defineProperty(RegExp.prototype, Symbol.match, {get() {throw 7;}, configurable:true});")
        .unwrap()).unwrap()).unwrap();
    assert_eq!(
        vm.string_pattern(PatternMethod::Match, &Value::String("a".into()), &[]),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(vm.stack.is_empty());
}
