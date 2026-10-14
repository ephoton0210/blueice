// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private RegExp builder ownership at real allocation and root boundaries.

use super::*;

#[cfg_attr(test, test)]
fn regexp_iterator_builders_clean_up_on_failure_and_retry() {
    Vm::verify_intrinsic_allocation_boundary(
        "RegExp iterator prototype",
        Vm::regexp_iterator_prototype,
    );
}

#[cfg_attr(test, test)]
fn regexp_builders_propagate_root_exhaustion_without_publishing_a_cache() {
    for iterator in [false, true] {
        let mut vm = Vm::default();
        vm.function_prototype().unwrap();
        if iterator {
            vm.base_iterator_prototype().unwrap();
        }
        vm.heap.allow_root_registrations(0);
        let result = if iterator {
            vm.regexp_iterator_prototype().map(Value::Object)
        } else {
            vm.regexp_global()
        };
        assert_eq!(result, Err(RuntimeError::Heap(HeapError::IdExhausted)));
        assert!(vm.regexp_iterator_prototype.is_none());
        assert!(!vm.globals.contains_key("RegExp"));
        assert!(vm.stack.is_empty());
        assert_eq!(
            vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
    }
}

#[cfg_attr(test, test)]
fn regexp_loop_fuel_and_output_limits_preserve_roots_and_matcher_identity() {
    for method in [
        RegExpMethod::Match,
        RegExpMethod::Split,
        RegExpMethod::Replace,
    ] {
        let mut completed = false;
        let mut refusals = 0;
        for fuel in 0..4096 {
            let mut vm = Vm::default();
            let regexp = vm
                .execute_script(&crate::compile(&crate::parse("/(a)(a)/g").unwrap()).unwrap())
                .unwrap();
            let id = regexp.object_id().unwrap();
            let root = vm.heap.root(id).unwrap();
            vm.remaining_instructions = fuel;
            let replacement = if method == RegExpMethod::Split {
                Value::Undefined
            } else {
                Value::String("$2$1$$$&".into())
            };
            let result = vm.regexp_method(
                method,
                &regexp,
                &[Value::String("aaaa".into()), replacement],
            );
            match result {
                Ok(_) => completed = true,
                Err(RuntimeError::InstructionLimit) => refusals += 1,
                Err(error) => panic!("{method:?}, fuel {fuel}: {error:?}"),
            }
            assert!(vm.stack.is_empty());
            assert_eq!(
                vm.heap.regexp(id).unwrap().unwrap().source,
                JsString::from("(a)(a)")
            );
            assert_eq!(
                vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
            vm.heap.unroot(root).unwrap();
            if completed {
                break;
            }
        }
        assert!(
            completed && refusals > 0,
            "{method:?} did not reach its loop fuel boundary"
        );
    }

    let mut prefix = Vm::default();
    let receiver = prefix
        .execute_script(
            &crate::compile(
                &crate::parse("({exec() {return {0:'b', index:5, length:1};}})").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let root = prefix.heap.root(receiver.object_id().unwrap()).unwrap();
    prefix.config.max_string_bytes = 2;
    assert_eq!(
        prefix.regexp_replace(&receiver, &"aaaaab".into(), &Value::String("".into())),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert!(prefix.stack.is_empty());
    prefix.heap.unroot(root).unwrap();
    for (pattern, input, replacement) in [
        ("/b/", "aaaaab", ""), // Prefix append.
        ("/a/", "a", "xxxx"),  // Replacement append.
        ("/a/g", "aaa", "x"),  // Individually valid replacements exceed the total output limit.
        ("/a/", "abbbbb", ""), // Trailing append.
    ] {
        let mut vm = Vm::default();
        let regexp = vm
            .execute_script(&crate::compile(&crate::parse(pattern).unwrap()).unwrap())
            .unwrap();
        let root = vm.heap.root(regexp.object_id().unwrap()).unwrap();
        vm.config.max_string_bytes = 2;
        vm.remaining_instructions = vm.config.instruction_budget;
        assert_eq!(
            vm.regexp_replace(&regexp, &input.into(), &Value::String(replacement.into())),
            Err(RuntimeError::StringLimit { limit: 2 }),
            "{pattern}, {input}"
        );
        assert!(
            vm.stack.is_empty(),
            "replacement must release each retained match"
        );
        vm.config.max_string_bytes = VmConfig::default().max_string_bytes;
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn regexp_private_ingress_preserves_foreign_heap_and_cold_resource_errors() {
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(foreign).unwrap();
    let mut vm = Vm::default();
    let value = Value::Object(foreign);
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)));
    assert_eq!(
        vm.regexp_compile(foreign, &Value::Undefined, &Value::Undefined),
        expected
    );
    assert_eq!(vm.regexp_exec(&value, &"a".into(), true), expected);
    let replacement = vm
        .execute_script(&crate::compile(&crate::parse("/a/").unwrap()).unwrap())
        .unwrap();
    assert_eq!(
        vm.regexp_replace(&replacement, &"a".into(), &value),
        expected
    );
    assert_eq!(
        vm.regexp_compile(replacement.object_id().unwrap(), &value, &Value::Undefined),
        expected
    );
    assert_eq!(
        vm.regexp_allocate(&value, false, &Value::Undefined, false),
        expected
    );
    owner.heap.unroot(root).unwrap();
    let mut cold = Vm::default();
    cold.string_intrinsics().unwrap();
    let pattern = cold.heap.alloc_object(None).unwrap();
    let root = cold.heap.root(pattern).unwrap();
    cold.heap
        .set(pattern, JsSymbol::well_known("match"), Value::Bool(true))
        .unwrap();
    cold.heap
        .set(pattern, "constructor", Value::Undefined)
        .unwrap();
    cold.heap.allow_root_registrations(0);
    assert_eq!(
        cold.regexp_constructor(&Value::Object(pattern), &Value::Undefined, false),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(cold.stack.is_empty() && !cold.globals.contains_key("RegExp"));
    cold.heap.unroot(root).unwrap();
    for setter in [false, true] {
        let mut vm = Vm::default();
        let limit = vm.heap.allow_only(0);
        let result = if setter {
            vm.regexp_legacy_set(
                LegacyRegExpStatic::Input,
                &Value::Undefined,
                &Value::Undefined,
            )
        } else {
            vm.regexp_legacy_get(LegacyRegExpStatic::Input, &Value::Undefined)
        };
        assert_eq!(
            result,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.stack.is_empty());
    }
    for regexp_like in [false, true] {
        let mut vm = Vm::default();
        let pattern = if regexp_like {
            vm.string_intrinsics().unwrap();
            let object = vm.heap.alloc_object(None).unwrap();
            vm.heap
                .define_own_property(
                    object,
                    JsSymbol::well_known("match"),
                    PropertyDescriptor::data(Value::Bool(true), true, true, true),
                )
                .unwrap();
            vm.heap
                .define_own_property(
                    object,
                    "constructor",
                    PropertyDescriptor::data(Value::Number(7.0), true, true, true),
                )
                .unwrap();
            vm.stack.push(Value::Object(object));
            assert!(!vm.globals.contains_key("RegExp"));
            Value::Object(object)
        } else {
            Value::Undefined
        };
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.regexp_constructor(&pattern, &Value::Undefined, false),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        if regexp_like {
            assert_eq!(vm.stack.pop(), Some(pattern));
        }
        assert!(vm.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn regexp_own_last_index_preserves_observable_coercion_and_skips_prototype_traps() {
    let source = r#"
        var regexp = /a/g, coerced = 0, caught = false;
        Object.setPrototypeOf(regexp, new Proxy(RegExp.prototype, {
            get(target, key, receiver) {
                if (key === 'lastIndex') throw 'prototype lastIndex was observed';
                return Reflect.get(target, key, receiver);
            }
        }));
        regexp.lastIndex = {valueOf() {coerced++; return 0;}};
        var match = RegExp.prototype.exec.call(regexp, 'a');
        var matched = match[0] === 'a' && regexp.lastIndex === 1;
        regexp.lastIndex = {[Symbol.toPrimitive]() {throw 7;}};
        try {RegExp.prototype.exec.call(regexp, 'a');}
        catch (error) {caught = error === 7;}
        regexp.lastIndex = 3;
        var absent = RegExp.prototype.exec.call(regexp, 'a');
        matched && coerced === 1 && caught && absent === null && regexp.lastIndex === 0 &&
            Object.getOwnPropertyDescriptor(regexp, 'lastIndex').configurable === false
    "#;
    let program = crate::compile(&crate::parse(source).unwrap()).unwrap();
    let reuse = crate::compile(&crate::parse("21 + 21").unwrap()).unwrap();
    for nursery in [VmConfig::default().heap.nursery_capacity, 1] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        assert_eq!(vm.execute_script(&program), Ok(Value::Bool(true)));
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }
}

impl Vm {
    pub(in crate::vm) fn assert_foreign_regexp_slot_failure(
        &self,
        wrapper: ObjectId,
        error: RuntimeError,
    ) {
        assert_eq!(self.regexp_slots(&Value::Object(wrapper)), Err(error));
    }

    pub(in crate::vm) fn verify_regexp_builder_boundary_contracts() {
        regexp_iterator_builders_clean_up_on_failure_and_retry();
        regexp_builders_propagate_root_exhaustion_without_publishing_a_cache();
        regexp_loop_fuel_and_output_limits_preserve_roots_and_matcher_identity();
        regexp_private_ingress_preserves_foreign_heap_and_cold_resource_errors();
        regexp_own_last_index_preserves_observable_coercion_and_skips_prototype_traps();
    }
}
