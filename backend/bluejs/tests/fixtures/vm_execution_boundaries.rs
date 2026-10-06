// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Internal execution boundary contracts, using valid rooted heap objects.

use super::*;
use crate::heap::{AsyncGeneratorCompletion, AsyncGeneratorRequest, AsyncGeneratorStatus};
use crate::{compile, parse, BlueJsProgramRegistry, BlueJsProgramV1, BlueJsSourceIdentity};

fn installed(source: &str) -> Bytecode {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            BlueJsSourceIdentity::new("page:///serial-boundary.js", "sha256:serial-boundary")
                .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

#[cfg_attr(test, test)]
fn host_callback_tags_validate_the_integer_boundary_without_allocating_a_registry() {
    for index in [0, 1, u32::MAX as usize] {
        assert_eq!(host_function_index(index), Ok(index as u32));
    }
    if let Some(index) = (u32::MAX as usize).checked_add(1) {
        assert_eq!(
            host_function_index(index),
            Err(RuntimeError::RangeError("too many host functions".into()))
        );
    }
}

#[cfg_attr(test, test)]
fn a_registered_host_timer_wakes_an_idle_vm_and_releases_its_callback_root() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    let callback = vm
        .execute_script(&compile(&parse("(() => $DONE())").unwrap()).unwrap())
        .unwrap()
        .object_id()
        .unwrap();
    // Register after all setup and compilation, then enter the idle host
    // event loop immediately. A delayed event exercises its blocking wakeup.
    vm.schedule_test262_timer(callback, std::time::Duration::from_millis(250))
        .unwrap();
    assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    assert!(
        !vm.heap.contains(callback),
        "the processed timer retained its callback root"
    );
}

#[cfg_attr(test, test)]
fn template_cache_root_refusal_restores_operands_and_publishes_no_template() {
    let code = compile(&parse("tag`answer${42}tail`").unwrap()).unwrap();
    let site = &code.templates[0];
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    vm.stack.push(Value::Number(7.0));
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.template_object(site),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert!(vm.templates.is_empty());
    vm.stack.clear();
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn iterator_callbacks_reject_foreign_heap_handles_before_observing_or_closing_the_receiver() {
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let foreign_root = owner.heap.root(foreign).unwrap();
    let callback = Value::Object(foreign);
    let mut vm = Vm::default();
    let receiver = vm.execute_script(&compile(&parse("globalThis.observed = 0; ({get next() {observed++; throw 7;}, return() {observed++; throw 9;}})").unwrap()).unwrap()).unwrap();
    let root = vm.heap.root(receiver.object_id().unwrap()).unwrap();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)));
    for result in [
        vm.iterator_zip_count(foreign).map(|_| ()),
        vm.callback_iterator_record(&receiver, &callback)
            .map(|_| ()),
        vm.iterator_helper_create(&receiver, &callback, crate::heap::IteratorHelperKind::Map)
            .map(|_| ()),
        vm.iterator_callback(&callback, Value::Number(42.0), 0)
            .map(|_| ()),
        vm.iterator_every(&receiver, &callback).map(|_| ()),
        vm.iterator_some(&receiver, &callback).map(|_| ()),
        vm.iterator_find(&receiver, &callback).map(|_| ()),
        vm.iterator_reduce(&receiver, &[callback, Value::Number(0.0)])
            .map(|_| ()),
    ] {
        assert_eq!(result, expected);
    }
    assert_eq!(
        vm.lookup_global_name("observed").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(root).unwrap();
    owner.heap.unroot(foreign_root).unwrap();
}

#[cfg_attr(test, test)]
fn failed_global_declaration_preparation_clears_transient_execution_state() {
    let mut vm = Vm::default();
    vm.execute_script(&compile(&parse("Object.defineProperty(globalThis, 'blocked', {value:7, writable:false, enumerable:true, configurable:false});").unwrap()).unwrap()).unwrap();
    let code = compile(&parse("function blocked() {return 42;} var shouldNotRun = true;").unwrap())
        .unwrap();
    assert!(matches!(
        vm.execute_script(&code),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(vm.bindings.is_empty() && vm.cells.is_empty() && vm.binding_metadata.is_empty());
    assert!(vm.stack.is_empty() && vm.active_scopes.is_empty() && vm.active_scope_slots.is_empty());
    assert!(vm.script_global_slots.is_empty());
    assert_eq!(
        vm.lookup_global_name("blocked").unwrap(),
        Some(Value::Number(7.0))
    );
    assert_eq!(vm.lookup_global_name("shouldNotRun").unwrap(), None);
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn regexp_resource_boundaries_restore_roots_and_preserve_the_existing_matcher() {
    use crate::native::RegExpMethod;
    for method in [
        RegExpMethod::Exec,
        RegExpMethod::Match,
        RegExpMethod::Replace,
        RegExpMethod::Search,
        RegExpMethod::Split,
        RegExpMethod::MatchAll,
    ] {
        let mut vm = Vm::default();
        let regexp = vm
            .execute_script(&compile(&parse("/a/g").unwrap()).unwrap())
            .unwrap();
        let root = vm.heap.root(regexp.object_id().unwrap()).unwrap();
        let before = vm.stack.len();
        vm.remaining_instructions = 0;
        assert_eq!(
            vm.regexp_method(
                method,
                &regexp,
                &[Value::String("aa".into()), Value::String("x".into())]
            ),
            Err(RuntimeError::InstructionLimit)
        );
        assert_eq!(vm.stack.len(), before);
        assert_eq!(
            vm.heap
                .regexp(regexp.object_id().unwrap())
                .unwrap()
                .unwrap()
                .source,
            JsString::from("a")
        );
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        vm.heap.unroot(root).unwrap();
    }
    let mut vm = Vm::default();
    let pattern = vm
        .execute_script(&compile(&parse("/abcd/").unwrap()).unwrap())
        .unwrap();
    let pattern_root = vm.heap.root(pattern.object_id().unwrap()).unwrap();
    let receiver = vm
        .execute_script(&compile(&parse("/a/").unwrap()).unwrap())
        .unwrap();
    let receiver_root = vm.heap.root(receiver.object_id().unwrap()).unwrap();
    vm.config.max_string_bytes = 2;
    assert_eq!(
        vm.regexp_method(
            RegExpMethod::Compile,
            &receiver,
            std::slice::from_ref(&pattern)
        ),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert_eq!(
        vm.heap
            .regexp(receiver.object_id().unwrap())
            .unwrap()
            .unwrap()
            .source,
        JsString::from("a")
    );
    assert_eq!(
        vm.regexp_getter("source", &pattern),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert_eq!(
        vm.regexp_method(RegExpMethod::ToString, &receiver, &[]),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert_eq!(
        vm.regexp_method(
            RegExpMethod::Replace,
            &receiver,
            &[Value::String("aa".into()), Value::String("bb".into())]
        ),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert!(vm.stack.is_empty());
    vm.config.max_string_bytes = VmConfig::default().max_string_bytes;
    assert_eq!(
        vm.regexp_method(
            RegExpMethod::Compile,
            &receiver,
            &[Value::String("b".into())]
        ),
        Ok(receiver.clone())
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(receiver_root).unwrap();
    vm.heap.unroot(pattern_root).unwrap();
}

#[cfg_attr(test, test)]
fn synchronous_root_exit_protocol_preserves_value_and_rejects_suspension() {
    use super::execution::root_interpreter_exit;
    assert_eq!(
        root_interpreter_exit(InterpreterExit::Return(Value::Number(42.0))),
        Ok(Value::Number(42.0))
    );
    assert_eq!(
        root_interpreter_exit(InterpreterExit::Yield {
            value: Value::Undefined,
            pc: 0,
            iterators: Vec::new(),
            handlers: Vec::new()
        }),
        Err(RuntimeError::TypeError(
            "yield requires a generator function".into()
        ))
    );
    assert!(
        std::panic::catch_unwind(|| root_interpreter_exit(InterpreterExit::Suspend {
            pc: 0,
            iterators: Vec::new(),
            handlers: Vec::new()
        }))
        .is_err()
    );
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    assert!(
        std::panic::catch_unwind(|| root_interpreter_exit(InterpreterExit::Await {
            promise,
            pc: 0,
            handlers: Vec::new()
        }))
        .is_err()
    );
}

#[cfg_attr(test, test)]
fn private_element_binding_and_receiver_checks_preserve_object_identity_without_coercion() {
    let code = compile(&parse("let owner;").unwrap()).unwrap();
    let slot = code
        .bindings
        .iter()
        .position(|binding| binding.name == "owner")
        .unwrap();
    let mut vm = Vm::default();
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    let error =
        RuntimeError::TypeError("private elements are not available in this function".into());
    assert_eq!(vm.private_element_owner(slot), Err(error.clone()));
    vm.run(&code).unwrap();
    assert_eq!(vm.private_element_owner(slot), Err(error.clone()));
    for value in [
        Value::Undefined,
        Value::Null,
        Value::Bool(true),
        Value::Number(42.0),
        Value::String("class".into()),
        Value::BigInt(42.into()),
        Value::Symbol(JsSymbol::well_known("iterator")),
    ] {
        vm.store_binding(slot, value.clone()).unwrap();
        assert_eq!(vm.private_element_owner(slot), Err(error.clone()));
        assert_eq!(
            Vm::private_element_receiver(value),
            Err(RuntimeError::TypeError(
                "private fields require an object receiver".into()
            ))
        );
    }
    let object = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(object).unwrap();
    vm.store_binding(slot, Value::Object(object)).unwrap();
    assert_eq!(vm.private_element_owner(slot), Ok(object));
    assert_eq!(
        Vm::private_element_receiver(Value::Object(object)),
        Ok(object)
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn representation_accessors_do_not_coerce_values() {
    let mut vm = Vm::default();
    let object = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(object).unwrap();
    let text = JsString::from("42");
    let number = Value::Number(42.0);
    let bigint = Value::BigInt(42.into());
    let string = Value::String(text.clone());
    assert_eq!(number.as_number(), Some(42.0));
    assert_eq!(bigint.as_bigint(), Some(&BigInt::from(42)));
    assert_eq!(string.as_string(), Some(&text));
    for other in [
        Value::Undefined,
        Value::Null,
        Value::Bool(true),
        Value::Symbol(JsSymbol::well_known("iterator")),
        Value::Object(object),
    ] {
        assert_eq!(other.as_number(), None);
        assert_eq!(other.as_bigint(), None);
        assert_eq!(other.as_string(), None);
    }
    assert_eq!(number.as_bigint(), None);
    assert_eq!(number.as_string(), None);
    assert_eq!(bigint.as_number(), None);
    assert_eq!(bigint.as_string(), None);
    assert_eq!(string.as_number(), None);
    assert_eq!(string.as_bigint(), None);
    assert!(Value::Number(f64::NAN).as_number().unwrap().is_nan());
    assert_eq!(PropertyName::String(text.clone()).into_string(), Some(text));
    assert_eq!(
        PropertyName::Symbol(JsSymbol::well_known("iterator")).into_string(),
        None
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn legacy_with_assignment_bytecode_writes_existing_bindings_and_rejects_absent_ones() {
    // WithSet remains a supported opcode even though the current compiler
    // emits resolved references. Exercise its established operand contract.
    let mut code = Bytecode::empty();
    code.constants = vec![Value::Number(7.0), Value::String("legacyValue".into())];
    code.code.push(Opcode::Constant as u8);
    code.code.extend_from_slice(&0_u32.to_le_bytes());
    code.code.push(Opcode::WithSet as u8);
    code.code.extend_from_slice(&1_u32.to_le_bytes());
    code.code
        .extend([Opcode::SetCompletion as u8, Opcode::Halt as u8]);
    for own in [false, true] {
        let mut vm = Vm::default();
        let object = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(object).unwrap();
        if own {
            vm.heap
                .set(object, "legacyValue", Value::Number(1.0))
                .unwrap();
        }
        vm.with_objects.push(Value::Object(object));
        vm.remaining_instructions = vm.config.instruction_budget;
        if own {
            assert_eq!(vm.run(&code), Ok(Value::Number(7.0)));
            assert_eq!(
                vm.heap.get_own(object, "legacyValue").unwrap(),
                Some(Value::Number(7.0))
            );
            assert_eq!(vm.lookup_global_name("legacyValue").unwrap(), None);
        } else {
            assert_eq!(
                vm.run(&code),
                Err(RuntimeError::ReferenceError("legacyValue".into()))
            );
            assert_eq!(vm.heap.get_own(object, "legacyValue").unwrap(), None);
            assert_eq!(vm.lookup_global_name("legacyValue").unwrap(), None);
        }
        vm.with_objects.clear();
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn queued_finalization_callbacks_keep_object_holdings_alive_across_collection() {
    let mut vm = Vm::default();
    vm.execute_script(
        &compile(
            &parse(
                r#"
        globalThis.finalized = 0;
        globalThis.registry = new FinalizationRegistry(holdings => {finalized = holdings.marker;});
        registry.register({}, {marker: 42}); 0
    "#,
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(vm
        .promise_jobs
        .iter()
        .any(|job| matches!(job, PromiseJob::FinalizationCleanup { .. })));
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        vm.lookup_global_name("finalized").unwrap(),
        Some(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn temporal_receiver_boundaries_reject_a_handle_owned_by_another_heap() {
    let mut other = Vm::default();
    let value = other
        .execute_script(&compile(&parse("Temporal.Duration.from('P1D')").unwrap()).unwrap())
        .unwrap();
    let object = value.object_id().unwrap();
    let mut vm = Vm::default();
    let expected = RuntimeError::Heap(HeapError::InvalidObject(object));
    assert_eq!(vm.temporal_duration_receiver(&value), Err(expected.clone()));
    assert!(matches!(vm.temporal_duration_relative_to(&value), Err(error) if error == expected));
}

#[cfg_attr(test, test)]
fn cold_temporal_allocation_propagates_intrinsic_initialization_failure() {
    for extra in [0, 16 * 1024 * 1024] {
        let mut vm = Vm::default();
        let value = vm
            .temporal_value_from_args(
                crate::heap::TemporalKind::Instant,
                &[Value::BigInt(0.into())],
            )
            .unwrap();
        let limit = vm.heap.allow_only(extra);
        let result = vm.alloc_temporal_value(value, false);
        if extra == 0 {
            assert_eq!(
                result,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
            );
        } else {
            assert!(result.unwrap().object_id().is_some());
        }
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        assert_eq!(
            vm.execute_script(
                &compile(&parse("Temporal.Duration.from('P1D').days").unwrap()).unwrap()
            ),
            Ok(Value::Number(1.0))
        );
    }
}

#[cfg_attr(test, test)]
fn temporal_initialization_publishes_its_cache_atomically_and_can_retry() {
    for materialize_global in [false, true] {
        let prepare = || {
            let mut vm = Vm::default();
            vm.string_intrinsics().unwrap();
            if materialize_global {
                vm.global("globalThis").unwrap();
            }
            vm
        };
        let mut measured = prepare();
        let before = measured.heap.stats().managed_bytes;
        measured.temporal_global().unwrap();
        let required = measured.heap.stats().managed_bytes - before;
        let reuse = compile(&parse("Temporal.Instant.fromEpochNanoseconds(0n).epochNanoseconds === 0n && Temporal.Duration.from('P1D').days === 1").unwrap()).unwrap();
        let mut failures = 0;
        for extra in (0..=required + 8).step_by(8) {
            let mut vm = prepare();
            let limit = vm.heap.allow_only(extra);
            let result = vm.temporal_global();
            let completed = result.is_ok();
            if !completed {
                assert_eq!(
                    result,
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
                );
                assert!(vm
                    .globals
                    .keys()
                    .all(|key| key != "Temporal" && !key.starts_with("%Temporal.")));
                assert!(vm.stack.is_empty());
                if materialize_global {
                    let global = vm.globals["globalThis"];
                    assert!(vm.heap.get_own(global, "Temporal").unwrap().is_none());
                }
                failures += 1;
            }
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(
                vm.execute_script(&reuse),
                Ok(Value::Bool(true)),
                "headroom {extra}, globalThis {materialize_global}"
            );
            if completed {
                break;
            }
        }
        assert!(failures > 0);
    }
}

#[cfg_attr(test, test)]
fn object_internal_methods_preserve_heap_identity_and_ordinary_fallbacks() {
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let foreign_root = owner.heap.root(foreign).unwrap();
    let mut vm = Vm::default();
    let key = PropertyName::from("value");
    let receiver = Value::Object(foreign);
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)));
    for result in [
        vm.object_get_own_property(foreign, &key).map(|_| ()),
        vm.object_define_own_property(
            foreign,
            key.clone(),
            PropertyDescriptor::data(Value::Number(7.0), true, true, true),
        )
        .map(|_| ()),
        vm.object_delete(foreign, &key).map(|_| ()),
        vm.object_own_property_keys(foreign).map(|_| ()),
        vm.object_is_extensible(foreign).map(|_| ()),
        vm.object_get_prototype(foreign).map(|_| ()),
        vm.object_set_prototype(foreign, None).map(|_| ()),
        vm.object_prevent_extensions(foreign).map(|_| ()),
        vm.proxy_get(foreign, &receiver, &key).map(|_| ()),
        vm.proxy_has(foreign, &key).map(|_| ()),
        vm.proxy_set(foreign, &receiver, &key, &Value::Number(7.0))
            .map(|_| ()),
        vm.proxy_delete(foreign, &key).map(|_| ()),
        vm.proxy_own_keys(foreign).map(|_| ()),
        vm.proxy_get_own_property(foreign, &key).map(|_| ()),
        vm.proxy_define_own_property(
            foreign,
            key.clone(),
            PropertyDescriptor::data(Value::Number(7.0), true, true, true),
        )
        .map(|_| ()),
        vm.proxy_get_prototype(foreign).map(|_| ()),
        vm.proxy_set_prototype(foreign, None).map(|_| ()),
        vm.proxy_is_extensible(foreign).map(|_| ()),
        vm.proxy_prevent_extensions(foreign).map(|_| ()),
    ] {
        assert_eq!(result, expected);
    }
    owner.heap.unroot(foreign_root).unwrap();

    let object = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(object).unwrap();
    vm.heap
        .define_own_property(
            object,
            key.clone(),
            PropertyDescriptor::data(Value::Number(7.0), false, true, false),
        )
        .unwrap();
    let receiver = Value::Object(object);
    assert_eq!(
        vm.proxy_get(object, &receiver, &key),
        Ok(Value::Number(7.0))
    );
    assert_eq!(vm.proxy_has(object, &key), Ok(true));
    assert_eq!(
        vm.proxy_set(object, &receiver, &key, &Value::Number(8.0)),
        Ok(false)
    );
    assert_eq!(vm.proxy_delete(object, &key), Ok(false));
    assert_eq!(vm.proxy_own_keys(object), Ok(vec![key.clone()]));
    assert_eq!(
        vm.proxy_get_own_property(object, &key)
            .unwrap()
            .unwrap()
            .value,
        Some(Value::Number(7.0))
    );
    assert_eq!(
        vm.proxy_define_own_property(
            object,
            key,
            PropertyDescriptor::data(Value::Number(8.0), false, true, false)
        ),
        Ok(false)
    );
    assert_eq!(vm.proxy_get_prototype(object), Ok(None));
    assert_eq!(vm.proxy_set_prototype(object, Some(object)), Ok(false));
    assert_eq!(vm.proxy_is_extensible(object), Ok(true));
    assert_eq!(vm.proxy_prevent_extensions(object), Ok(true));
    assert_eq!(vm.proxy_is_extensible(object), Ok(false));
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn frame_serial_exhaustion_restores_the_caller() {
    let code = installed("function child(value) { return value; } child(7);");
    let child = code.child_code_units().next().unwrap();
    let offset = child.instructions().next().unwrap().offset as u32;
    let mut vm = Vm {
        next_debugger_frame_serial: u64::MAX,
        ..Vm::default()
    };
    assert_eq!(
        vm.execute_script_until_nested_debugger_pause(&code, 1, offset),
        Err(RuntimeError::Unsupported(
            "nested debugger frame serial exhausted"
        ))
    );
    assert!(vm.call_stack.is_empty());
    assert!(vm.stack.is_empty());
    assert!(vm.dynamic_eval_outer_bindings.is_empty());
    assert!(vm.debugger_nested_continuation.is_none());
    assert!(vm.arguments.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.next_debugger_frame_serial = 1;
    assert!(matches!(
        vm.execute_script_until_nested_debugger_pause(&code, 1, offset),
        Ok(VmDebuggerNestedExecutionState::Paused {
            frame_serial: 1,
            ..
        })
    ));
    assert!(matches!(
        vm.resume_debugger_nested_execution(1),
        Ok(VmDebuggerNestedExecutionState::FrameReturned { .. })
    ));
    assert_eq!(
        vm.resume_debugger_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
}

#[cfg_attr(test, test)]
fn iterator_and_regexp_internal_receivers_preserve_heap_identity() {
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let receiver = Value::Object(object);
    let mut vm = Vm::default();
    let callback = vm
        .execute_script(&compile(&parse("(value => value)").unwrap()).unwrap())
        .unwrap();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(object)));
    for result in [
        vm.enter_call(
            receiver.clone(),
            Value::Undefined,
            Vec::new(),
            false,
            Value::Undefined,
        )
        .map(|_| ()),
        vm.dispatch_call(receiver.clone(), Value::Undefined, Vec::new(), false)
            .map(|_| ()),
        vm.iterator_from(&receiver).map(|_| ()),
        vm.iterator_dispose(&receiver).map(|_| ()),
        vm.iterator_map(&receiver, &callback).map(|_| ()),
        vm.iterator_filter(&receiver, &callback).map(|_| ()),
        vm.iterator_flat_map(&receiver, &callback).map(|_| ()),
        vm.iterator_helper_next(&receiver).map(|_| ()),
        vm.iterator_helper_return(&receiver).map(|_| ()),
        vm.iterator_to_array(&receiver).map(|_| ()),
        vm.iterator_for_each(&receiver, &callback).map(|_| ()),
        vm.iterator_every(&receiver, &callback).map(|_| ()),
        vm.iterator_some(&receiver, &callback).map(|_| ()),
        vm.iterator_find(&receiver, &callback).map(|_| ()),
        vm.iterator_reduce(&receiver, std::slice::from_ref(&callback))
            .map(|_| ()),
        vm.iterator_to_string_tag_setter(&receiver, &Value::Number(7.0))
            .map(|_| ()),
        vm.iterator_constructor_setter(&receiver, &Value::Number(7.0))
            .map(|_| ()),
        vm.regexp_constructor(&receiver, &Value::Undefined, false)
            .map(|_| ()),
        vm.regexp_create(&receiver, &Value::Undefined).map(|_| ()),
        vm.regexp_getter("source", &receiver).map(|_| ()),
        vm.regexp_method(
            crate::native::RegExpMethod::Exec,
            &receiver,
            &[Value::String("value".into())],
        )
        .map(|_| ()),
        vm.regexp_iterator_next(&receiver).map(|_| ()),
        vm.is_constructor(&receiver).map(|_| ()),
    ] {
        assert_eq!(result, expected);
    }
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn host_installation_failures_leave_the_callback_registry_and_vm_reusable() {
    // These public entries initialize globalThis themselves on a cold VM.
    // Retain no callback or temporary root when that initialization fails.
    for install_function in [false, true] {
        let mut vm = Vm::default();
        let limit = vm.heap.allow_only(0);
        let result = if install_function {
            vm.install_host_function("callback", 0, |_: &[crate::HostValue]| {
                Ok(crate::HostValue::Number(42.0))
            })
        } else {
            vm.install_host_object("host").map(|_| ())
        };
        assert_eq!(
            result,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.stack.is_empty() && vm.host_functions.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
    }
    // A pre-materialized realm global must also retain its initialized
    // Function prototype when the following callable allocation fails.
    let mut vm = Vm::default();
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.install_host_function("callback", 0, |_: &[crate::HostValue]| {
            Ok(crate::HostValue::Number(42.0))
        }),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty() && vm.host_functions.is_empty());
    assert_eq!(vm.heap.get_own(global, "callback").unwrap(), None);
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.install_host_function("callback", 0, |_: &[crate::HostValue]| {
        Ok(crate::HostValue::Number(42.0))
    })
    .unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("callback()").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    for operation in 0..3 {
        let mut failures = 0;
        let mut completed = false;
        for extra in (0..32_768).step_by(8) {
            let mut vm = Vm::default();
            let owner = vm.install_host_object("host").unwrap();
            let retained = vm.host_functions.len();
            let limit = vm.heap.allow_only(extra);
            let result = match operation {
                0 => vm.install_host_function("callback", 1, |_: &[crate::HostValue]| {
                    Ok(crate::HostValue::Number(42.0))
                }),
                1 => vm.install_host_method(owner, "callback", 1, |_: &[crate::HostValue]| {
                    Ok(crate::HostValue::Number(42.0))
                }),
                _ => vm.install_host_object("second").map(|_| ()),
            };
            match result {
                Ok(()) => completed = true,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: observed })) => {
                    assert_eq!(observed, limit);
                    assert_eq!(vm.host_functions.len(), retained);
                    failures += 1;
                }
                Err(error) => panic!("host operation {operation}, headroom {extra}: {error:?}"),
            }
            vm.heap.allow_only(16 * 1024 * 1024);
            let code = compile(&parse("21 + 21").unwrap()).unwrap();
            assert_eq!(vm.execute_script(&code), Ok(Value::Number(42.0)));
            if completed {
                break;
            }
        }
        assert!(completed && failures > 0);
    }
}

#[cfg_attr(test, test)]
fn all_completion_variants_keep_their_object_edges_alive() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    let mut objects = Vec::new();
    for index in 0..6 {
        let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
        vm.stack.push(Value::Object(object));
        vm.with_roots(|heap| heap.set(object, "marker", Value::Number(index as f64)))
            .unwrap();
        objects.push(object);
    }
    vm.pending_completions = vec![
        Completion::Return(Value::Object(objects[0])),
        Completion::Yield(Value::Object(objects[1])),
        Completion::Throw(RuntimeError::Thrown(Value::Object(objects[2]))),
        Completion::TailRecur(vec![Value::Object(objects[3])]),
        Completion::TailCall(vec![Value::Object(objects[4])]),
        Completion::Throw(RuntimeError::TypeError("boundary".into())),
        Completion::Jump {
            cleanup: 0,
            target: 0,
        },
        Completion::Resume(0),
        Completion::Halt(Value::Undefined),
    ];
    vm.pending_tail_call = Some(vec![Value::Object(objects[5])]);
    vm.stack.clear();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    for (index, object) in objects.iter().enumerate() {
        assert_eq!(
            vm.heap.get_own(*object, "marker").unwrap(),
            Some(Value::Number(index as f64))
        );
    }
    vm.pending_completions.clear();
    vm.pending_tail_call = None;
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    for object in objects {
        assert!(vm.heap.get_own(object, "marker").is_err());
    }
}

#[cfg_attr(test, test)]
fn deletion_removes_current_and_captured_dynamic_eval_cells() {
    let mut vm = Vm::default();
    for outer in [false, true] {
        let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
        let root = vm.heap.root(cell).unwrap();
        vm.heap.set(cell, "value", Value::Number(7.0)).unwrap();
        let binding = DynamicEvalBinding {
            cell,
            shadowed_cells: Vec::new(),
        };
        if outer {
            vm.dynamic_eval_outer_bindings
                .push(HashMap::from([("dynamic".into(), binding)]));
        } else {
            vm.dynamic_eval_bindings.insert("dynamic".into(), binding);
        }
        assert_eq!(vm.delete_unbound_name("dynamic"), Ok(true));
        assert_eq!(vm.heap.get_own(cell, "value").unwrap(), None);
        assert!(vm.dynamic_eval_bindings.is_empty());
        assert!(vm.dynamic_eval_outer_bindings.iter().all(HashMap::is_empty));
        vm.dynamic_eval_outer_bindings.clear();
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn iterator_metadata_and_callback_boundaries_preserve_safe_integer_limits() {
    let mut vm = Vm::default();
    let metadata = vm.heap.alloc_object(None).unwrap();
    let metadata_root = vm.heap.root(metadata).unwrap();
    let state_error = RuntimeError::TypeError("invalid Iterator.zip state".into());
    assert_eq!(vm.iterator_zip_count(metadata), Err(state_error.clone()));
    for value in [
        Value::Undefined,
        Value::Null,
        Value::Bool(false),
        Value::String("2".into()),
        Value::Number(2.0),
    ] {
        vm.heap.set(metadata, "zipCount", value.clone()).unwrap();
        assert_eq!(
            vm.iterator_zip_count(metadata),
            if matches!(value, Value::Number(_)) {
                Ok(2)
            } else {
                Err(state_error.clone())
            }
        );
    }
    assert_eq!(
        vm.iterator_callback(&Value::Undefined, Value::Number(7.0), 0),
        Err(RuntimeError::TypeError(
            "Iterator helper callback must be callable".into()
        ))
    );
    assert_eq!(
        vm.regexp_exec(&Value::Undefined, &JsString::default(), true),
        Err(RuntimeError::TypeError(
            "RegExp exec requires a RegExp".into()
        ))
    );
    let helper = vm
        .execute_script(
            &compile(&parse("Iterator.from([7]).map(value => value + 1)").unwrap()).unwrap(),
        )
        .unwrap()
        .object_id()
        .unwrap();
    let state = vm.heap.iterator_helper(helper).unwrap().unwrap();
    let prototype = vm.heap.prototype(helper).unwrap().unwrap();
    let max = 9_007_199_254_740_991;
    let boundary = vm
        .heap
        .alloc_iterator_helper(state.record, state.callback, state.kind, max - 1, prototype)
        .unwrap();
    let boundary_root = vm.heap.root(boundary).unwrap();
    let base = vm.stack.len();
    assert_eq!(
        vm.iterator_helper_callback(boundary, &Value::Number(7.0)),
        Ok(Value::Number(8.0))
    );
    vm.stack.truncate(base);
    assert_eq!(
        vm.heap.iterator_helper(boundary).unwrap().unwrap().index,
        max
    );
    assert_eq!(
        vm.iterator_helper_callback(boundary, &Value::Number(7.0)),
        Err(RuntimeError::RangeError(
            "Iterator helper index exceeds the safe integer range".into()
        ))
    );
    assert_eq!(
        vm.heap.iterator_helper(boundary).unwrap().unwrap().index,
        max
    );
    vm.heap.unroot(boundary_root).unwrap();
    vm.heap.unroot(metadata_root).unwrap();
}

#[cfg_attr(test, test)]
fn async_generator_queue_rejects_invalid_brand_serial_and_completion_order() {
    let mut empty = Vm::default();
    assert_eq!(
        empty.async_generator_request(&Value::Undefined, Value::Undefined, NativeFunction::Empty),
        Err(RuntimeError::Unsupported("not an async generator request"))
    );
    let mut vm = Vm::default();
    let sync = vm
        .execute_script(&compile(&parse("(function* () {})()").unwrap()).unwrap())
        .unwrap()
        .object_id()
        .unwrap();
    let sync_root = vm.heap.root(sync).unwrap();
    let promise = vm.new_promise().unwrap();
    let promise_root = vm.heap.root(promise).unwrap();
    let brand_error = RuntimeError::TypeError("Async generator receiver required".into());
    assert_eq!(
        vm.set_async_generator_status(sync, AsyncGeneratorStatus::Completed),
        Err(brand_error.clone())
    );
    assert_eq!(
        vm.complete_async_generator_request(
            sync,
            promise,
            PromiseStatus::Fulfilled(Value::Undefined)
        ),
        Err(brand_error.clone())
    );
    assert_eq!(vm.resume_async_generator_next(sync), Err(brand_error));

    let generator = vm
        .execute_script(&compile(&parse("(async function* () {yield 1;})()").unwrap()).unwrap())
        .unwrap()
        .object_id()
        .unwrap();
    let root = vm.heap.root(generator).unwrap();
    assert_eq!(
        vm.complete_async_generator_request(
            generator,
            promise,
            PromiseStatus::Fulfilled(Value::Undefined)
        ),
        Err(RuntimeError::Unsupported("missing async generator request"))
    );
    let mut control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    control.next_request_id = u64::MAX;
    vm.heap
        .set_async_generator_control(generator, control)
        .unwrap();
    assert_eq!(
        vm.async_generator_request(
            &Value::Object(generator),
            Value::Undefined,
            NativeFunction::AsyncGeneratorNext
        ),
        Err(RuntimeError::RangeError(
            "async generator request identifiers exhausted".into()
        ))
    );
    assert!(vm
        .heap
        .async_generator_control(generator)
        .unwrap()
        .unwrap()
        .requests
        .is_empty());
    let mut control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    control.next_request_id = 2;
    control.requests.push_back(AsyncGeneratorRequest {
        id: 1,
        completion: AsyncGeneratorCompletion::Next(Value::Undefined),
        target: promise,
    });
    vm.heap
        .set_async_generator_control(generator, control)
        .unwrap();
    let wrong_target = vm.new_promise().unwrap();
    let wrong_root = vm.heap.root(wrong_target).unwrap();
    assert_eq!(
        vm.complete_async_generator_request(
            generator,
            wrong_target,
            PromiseStatus::Fulfilled(Value::Undefined)
        ),
        Err(RuntimeError::Unsupported(
            "async generator request completed out of order"
        ))
    );
    assert_eq!(
        vm.heap
            .async_generator_control(generator)
            .unwrap()
            .unwrap()
            .requests
            .len(),
        1
    );
    // A heap queue supplied at the scheduler boundary must retain its head
    // when later request IDs do not strictly follow it.
    let mut control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    control.requests.push_back(AsyncGeneratorRequest {
        id: 1,
        completion: AsyncGeneratorCompletion::Next(Value::Undefined),
        target: wrong_target,
    });
    vm.heap
        .set_async_generator_control(generator, control)
        .unwrap();
    assert_eq!(
        vm.complete_async_generator_request(
            generator,
            promise,
            PromiseStatus::Fulfilled(Value::Undefined)
        ),
        Err(RuntimeError::Unsupported(
            "async generator queue identifiers are not FIFO"
        ))
    );
    let mut control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    assert_eq!(control.requests.len(), 2);
    assert_eq!(control.requests.front().unwrap().target, promise);
    control.requests.pop_back();
    vm.heap
        .set_async_generator_control(generator, control)
        .unwrap();
    vm.resume_async_generator_next(generator).unwrap();
    vm.run_promise_jobs().unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(_)
    ));
    vm.heap.unroot(wrong_root).unwrap();
    vm.heap.unroot(root).unwrap();
    vm.heap.unroot(promise_root).unwrap();
    vm.heap.unroot(sync_root).unwrap();
}

#[cfg_attr(test, test)]
fn foreign_facades_reject_a_released_realm_without_accessing_its_heap() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let wrapper = vm
        .execute_script(
            &compile(&parse("$262.createRealm().global.eval('new Date(0)')").unwrap()).unwrap(),
        )
        .unwrap()
        .object_id()
        .unwrap();
    let (realm, target, _, _) = vm.test262_foreign_reference(wrapper).unwrap();
    vm.test262_realms.clear();
    let error = RuntimeError::TypeError("foreign Test262 realm is no longer available".into());
    assert_eq!(
        vm.test262_foreign_native_function(wrapper),
        Err(error.clone())
    );
    assert_eq!(vm.test262_foreign_regexp_data(wrapper), Err(error.clone()));
    vm.assert_foreign_regexp_slot_failure(wrapper, error.clone());
    assert_eq!(vm.test262_foreign_date_value(wrapper), Err(error.clone()));
    assert_eq!(
        vm.test262_foreign_boxed_primitive(wrapper),
        Err(error.clone())
    );
    assert_eq!(
        vm.test262_import_foreign_value(realm, Value::Object(target)),
        Err(error)
    );
}

#[cfg_attr(test, test)]
fn iterator_getter_failures_restore_the_temporary_stack_and_finish_the_record() {
    for asynchronous in [false, true] {
        let mut vm = Vm::default();
        let source = "({[Symbol.iterator]() {return this;}, [Symbol.asyncIterator]() {return this;}, get next() {throw 7;}})";
        let iterator = vm
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        let symbol = if asynchronous {
            "asyncIterator"
        } else {
            "iterator"
        };
        let method = vm
            .get_method(&iterator, &JsSymbol::well_known(symbol).into())
            .unwrap();
        let before = vm.stack.len();
        let error = if asynchronous {
            vm.async_iterator_record_from_method(&iterator, method)
        } else {
            vm.get_iterator_from_method(&iterator, method)
        };
        assert_eq!(error, Err(RuntimeError::Thrown(Value::Number(7.0))));
        assert_eq!(vm.stack.len(), before);
    }
    for source in [
        "({[Symbol.iterator]() {return this;}, next() {return {get done() {throw 7;}};}})",
        "({[Symbol.iterator]() {return this;}, next() {return {done:false, get value() {throw 7;}};}})",
    ] {
        let mut vm = Vm::default();
        let iterator = vm.execute_script(&compile(&parse(source).unwrap()).unwrap()).unwrap();
        let record = vm.get_iterator(&iterator).unwrap();
        let root = vm.heap.root(record.object_id().unwrap()).unwrap();
        let before = vm.stack.len();
        assert_eq!(vm.iterator_step(&record, true), Err(RuntimeError::Thrown(Value::Number(7.0))));
        assert_eq!(vm.stack.len(), before);
        assert_eq!(vm.iterator_step(&record, true), Ok(None));
        assert_eq!(vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()), Ok(Value::Number(42.0)));
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn iterator_helper_abrupt_paths_restore_roots_and_complete_the_helper() {
    for source in [
        "globalThis.closed = 0; Iterator.from({next() {return {done:false, value:1};}, return() {closed++; return {};}}).flatMap(() => ({get next() {throw 7;}}))",
        "globalThis.closed = 0; Iterator.from({next() {return {done:false, value:1};}, return() {closed++; return {};}}).filter(() => {throw 7;})",
        "Iterator.concat({[Symbol.iterator]() {throw 7;}})",
        "Iterator.zip([{next() {throw 7;}}])",
        "Iterator.from({next() {throw 7;}}).chunks(2)",
        "Iterator.from({next() {throw 7;}}).windows(2)",
    ] {
        let mut vm = Vm::default();
        let helper = vm.execute_script(&compile(&parse(source).unwrap()).unwrap()).unwrap();
        let root = vm.heap.root(helper.object_id().unwrap()).unwrap();
        let before = vm.stack.len();
        assert_eq!(vm.iterator_helper_next(&helper), Err(RuntimeError::Thrown(Value::Number(7.0))), "{source}");
        assert_eq!(vm.stack.len(), before, "{source}");
        let state = vm.heap.iterator_helper(helper.object_id().unwrap()).unwrap().unwrap();
        assert!(state.done && !state.executing, "{source}");
        if source.starts_with("globalThis.closed") {
            assert_eq!(vm.lookup_global_name("closed").unwrap(), Some(Value::Number(1.0)));
        }
        let completed = vm.iterator_helper_next(&helper).unwrap().object_id().unwrap();
        assert_eq!(vm.heap.get_own(completed, "done").unwrap(), Some(Value::Bool(true)));
        assert_eq!(vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()), Ok(Value::Number(42.0)));
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn zip_padding_failures_restore_temporary_roots_and_preserve_heap_errors() {
    for source in [
        "({get [Symbol.iterator]() {throw 7;}})",
        "({[Symbol.iterator]() {throw 7;}})",
        "({[Symbol.iterator]() {return {get next() {throw 7;}};}})",
        "({[Symbol.iterator]() {return {next() {throw 7;}};}})",
        "({[Symbol.iterator]() {return {next() {return {get done() {throw 7;}};}};}})",
        "({[Symbol.iterator]() {return {next() {return {done:false, get value() {throw 7;}};}};}})",
        "({[Symbol.iterator]() {return {next() {return {done:false, value:1};}, return() {throw 7;}};}})",
    ] {
        let mut vm = Vm::default();
        let padding = vm.execute_script(&compile(&parse(source).unwrap()).unwrap()).unwrap();
        let padding_root = vm.heap.root(padding.object_id().unwrap()).unwrap();
        let metadata = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(metadata).unwrap();
        let before = vm.stack.len();
        assert_eq!(vm.iterator_zip_collect_padding(metadata, 2, &padding), Err(RuntimeError::Thrown(Value::Number(7.0))), "{source}");
        assert_eq!(vm.stack.len(), before, "{source}");
        assert_eq!(vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()), Ok(Value::Number(42.0)));
        vm.heap.unroot(root).unwrap();
        vm.heap.unroot(padding_root).unwrap();
    }
    for keyed in [false, true] {
        let mut vm = Vm::default();
        let metadata = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(metadata).unwrap();
        if keyed {
            vm.heap
                .set(metadata, "zipKey0", Value::String("first".into()))
                .unwrap();
        }
        let before = vm.stack.len();
        let limit = vm.heap.allow_only(0);
        let outcome = if keyed {
            vm.iterator_zip_keyed_collect_padding(metadata, 1, &Value::Undefined)
        } else {
            vm.iterator_zip_collect_padding(metadata, 1, &Value::Undefined)
        };
        assert_eq!(
            outcome,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert_eq!(vm.stack.len(), before);
        assert_eq!(vm.heap.get_own(metadata, "zipPadding0").unwrap(), None);
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.iterator_zip_collect_padding(metadata, 1, &Value::Undefined),
            Ok(())
        );
        assert_eq!(
            vm.heap.get_own(metadata, "zipPadding0").unwrap(),
            Some(Value::Undefined)
        );
        vm.heap.unroot(root).unwrap();
    }
    // Exercise the value-growth refusal after iterator_step, rather than
    // the earlier allocation of a new padding property or iterator record.
    let mut vm = Vm::default();
    let padding = vm
        .execute_script(&compile(&parse("['x'.repeat(512)]").unwrap()).unwrap())
        .unwrap();
    let padding_root = vm.heap.root(padding.object_id().unwrap()).unwrap();
    let metadata = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(metadata).unwrap();
    vm.heap
        .set(metadata, "zipPadding0", Value::Undefined)
        .unwrap();
    vm.base_iterator_prototype().unwrap();
    vm.array_iterator_prototype().unwrap();
    let mut next_extra = 0;
    let mut completed = false;
    while next_extra <= 16 * 1024 * 1024 {
        let extra = next_extra;
        let limit = vm.heap.allow_only(extra);
        match vm.iterator_zip_collect_padding(metadata, 1, &padding) {
            Ok(()) => {
                completed = true;
                break;
            }
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                assert_eq!(actual, limit)
            }
            Err(error) => panic!("padding growth, headroom {extra}: {error:?}"),
        }
        assert!(vm.stack.is_empty());
        next_extra = vm.heap.next_allocation_headroom(extra);
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    }
    assert!(completed);
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(
        vm.heap.get_own(metadata, "zipPadding0").unwrap(),
        Some(Value::String("x".repeat(512).into()))
    );
    vm.heap.unroot(root).unwrap();
    vm.heap.unroot(padding_root).unwrap();

    // An Array iterator's temporary result can be collected to fund copying
    // its value into metadata. Retain a real result object in the realm so
    // a refusal after next() returned reaches the padding value write itself.
    let setup = compile(
        &parse(
            r#"
        globalThis.paddingCalls = 0;
        globalThis.paddingStep = {done:false, value:'x'.repeat(4096)};
        globalThis.paddingClosed = {done:true};
        globalThis.retainedPadding = {
            [Symbol.iterator]() {return this;},
            next() {paddingCalls++; return paddingStep;},
            return() {return paddingClosed;}
        };
    "#,
        )
        .unwrap(),
    )
    .unwrap();
    let mut next_extra = 0;
    let mut refused_growth = false;
    let mut completed = false;
    while next_extra <= 16 * 1024 * 1024 {
        let extra = next_extra;
        let mut vm = Vm::default();
        vm.execute_script(&setup).unwrap();
        let global = vm.global("globalThis").unwrap().object_id().unwrap();
        let padding = vm.heap.get_own(global, "retainedPadding").unwrap().unwrap();
        let metadata = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(metadata).unwrap();
        vm.heap
            .set(metadata, "zipPadding0", Value::Undefined)
            .unwrap();
        vm.base_iterator_prototype().unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let limit = vm.heap.allow_only(extra);
        match vm.iterator_zip_collect_padding(metadata, 1, &padding) {
            Ok(()) => {
                completed = true;
                assert_eq!(
                    vm.heap.get_own(metadata, "zipPadding0").unwrap(),
                    Some(Value::String("x".repeat(4096).into()))
                );
            }
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                assert_eq!(actual, limit);
                if vm.heap.get_own(global, "paddingCalls").unwrap() == Some(Value::Number(1.0))
                    && vm.heap.get_own(metadata, "zipPadding0").unwrap() == Some(Value::Undefined)
                {
                    refused_growth = true;
                }
                next_extra = vm.heap.next_allocation_headroom(extra);
            }
            Err(error) => panic!("retained padding result, headroom {extra}: {error:?}"),
        }
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        vm.heap.unroot(root).unwrap();
        if completed {
            break;
        }
    }
    assert!(
        completed && refused_growth,
        "retained result did not reach the padding value-growth refusal"
    );
}

#[cfg_attr(test, test)]
fn lazy_intrinsic_root_refusals_preserve_existing_globals_and_roots() {
    let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
    for name in [
        "Error",
        "TypeError",
        "AggregateError",
        "SuppressedError",
        "Intl",
        "Function",
        "Array",
        "Symbol",
        "Date",
        "ArrayBuffer",
        "SharedArrayBuffer",
        "DataView",
        "Int8Array",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "WeakRef",
        "FinalizationRegistry",
        "DisposableStack",
        "AsyncDisposableStack",
        "Promise",
        "Reflect",
        "Math",
        "JSON",
        "Atomics",
        "Iterator",
        "ShadowRealm",
        "Proxy",
        "globalThis",
        "eval",
    ] {
        let mut completed = false;
        for remaining in 0..64 {
            let mut vm = Vm::default();
            vm.string_intrinsics().unwrap();
            let before = vm.heap.stats().root_registrations;
            vm.heap.allow_root_registrations(remaining);
            match vm.global(name) {
                Ok(value) => {
                    assert!(vm.heap.contains(value.object_id().unwrap()));
                    completed = true;
                }
                Err(RuntimeError::Heap(HeapError::IdExhausted)) => {
                    assert!(
                        !vm.globals.contains_key(name),
                        "refused {name} published a cache"
                    );
                }
                Err(error) => panic!("{name}, remaining roots {remaining}: {error:?}"),
            }
            assert!(vm.stack.is_empty(), "{name}, remaining roots {remaining}");
            assert!(vm.heap.stats().root_registrations >= before);
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            assert!(vm.heap.contains(vm.object_prototype));
            assert!(vm.heap.contains(vm.array_prototype));
            for object in vm.globals.values() {
                assert!(vm.heap.contains(*object));
            }
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if completed {
                break;
            }
        }
        assert!(
            completed,
            "{name} did not reach successful root registration"
        );
    }
    type ObjectBuilder = fn(&mut Vm) -> Result<ObjectId, RuntimeError>;
    let builders: &[(&str, ObjectBuilder)] = &[
        ("ArrayIterator", Vm::array_iterator_prototype),
        ("IteratorWrapper", Vm::iterator_wrapper_prototype),
        ("IteratorHelper", Vm::iterator_helper_prototype),
        ("GeneratorFunction", Vm::generator_function_prototype),
        ("Generator", Vm::generator_prototype),
        ("AsyncIterator", Vm::async_iterator_prototype),
        ("AsyncGenerator", Vm::async_generator_prototype),
        (
            "AsyncGeneratorFunction",
            Vm::async_generator_function_prototype,
        ),
        ("AsyncFunction", Vm::async_function_prototype),
        ("ThrowTypeError", Vm::throw_type_error),
    ];
    for &(name, builder) in builders {
        let mut completed = false;
        for remaining in 0..64 {
            let mut vm = Vm::default();
            vm.string_intrinsics().unwrap();
            vm.heap.allow_root_registrations(remaining);
            match builder(&mut vm) {
                Ok(object) => {
                    assert!(vm.heap.contains(object));
                    completed = true;
                }
                Err(RuntimeError::Heap(HeapError::IdExhausted)) => {}
                Err(error) => panic!("{name}, remaining roots {remaining}: {error:?}"),
            }
            assert!(vm.stack.is_empty());
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            for object in vm.globals.values() {
                assert!(vm.heap.contains(*object));
            }
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if completed {
                break;
            }
        }
        assert!(completed, "{name} never completed");
    }
    for remaining in 0..=2 {
        let mut vm = Vm::default();
        let prototype = vm.function_prototype().unwrap();
        let function = vm
            .with_roots(|heap| {
                heap.alloc_native_function(NativeFunction::Empty, "ordinary", prototype)
            })
            .unwrap();
        let root = vm.heap.root(function).unwrap();
        vm.heap.allow_root_registrations(remaining);
        let result = vm.install_legacy_function_properties(function);
        if remaining < 2 {
            assert_eq!(result, Err(RuntimeError::Heap(HeapError::IdExhausted)));
            assert!(vm.legacy_function_getters.is_none());
        } else {
            result.unwrap();
        }
        assert!(vm.stack.is_empty());
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert!(vm.heap.contains(function));
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
        vm.heap.unroot(root).unwrap();
    }
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    vm.base_iterator_prototype().unwrap();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.string_iterator_prototype(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.iterator_prototype.is_none());
    assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
}

#[cfg_attr(test, test)]
fn iterator_cleanup_keeps_pending_records_and_the_first_throw_alive_across_collection() {
    for nursery_capacity in [1, VmConfig::default().heap.nursery_capacity] {
        for failure in ["return {done:true};", "throw 7;", "throw {marker:42};"] {
            let mut config = VmConfig::default();
            config.heap.nursery_capacity = nursery_capacity;
            let mut vm = Vm::new(config).unwrap();
            let source = format!(
                "globalThis.closed = ''; [0,1,2].map(index => ({{[Symbol.iterator]() {{return this;}}, next() {{return {{done:false,value:index}};}}, return() {{closed += index; var allocations = Array.from({{length:96}}, () => ({{}})); if (index === 2) {{{failure}}} throw 'outer';}}}}));"
            );
            let sources = vm
                .execute_script(&compile(&parse(&source).unwrap()).unwrap())
                .unwrap();
            let values = vm.array_like_values(&sources).unwrap();
            let mut records = Vec::new();
            let mut roots = Vec::new();
            for value in values {
                let record = vm.get_iterator(&value).unwrap();
                roots.push(vm.heap.root(record.object_id().unwrap()).unwrap());
                records.push(record);
            }
            vm.execute_script(&compile(&parse("0").unwrap()).unwrap())
                .unwrap();
            for root in roots {
                vm.heap.unroot(root).unwrap();
            }
            let base = vm.stack.len();
            let result = vm.close_iterators_for_return(&mut records, 0);
            assert!(records.is_empty());
            assert_eq!(vm.stack.len(), base);
            let error = result.unwrap_err();
            match failure {
                "throw 7;" => assert_eq!(error, RuntimeError::Thrown(Value::Number(7.0))),
                "throw {marker:42};" => {
                    let RuntimeError::Thrown(Value::Object(object)) = error else {
                        panic!("lost object throw: {error:?}");
                    };
                    assert_eq!(
                        vm.heap.get_own(object, "marker").unwrap(),
                        Some(Value::Number(42.0))
                    );
                }
                _ => assert_eq!(error, RuntimeError::Thrown(Value::String("outer".into()))),
            }
            assert_eq!(
                vm.lookup_global_name("closed").unwrap(),
                Some(Value::String("210".into()))
            );
            assert_eq!(
                vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
            assert_eq!(vm.close_iterators_for_return(&mut records, 0), Ok(()));
        }
    }
}

#[cfg_attr(test, test)]
fn root_identifier_exhaustion_propagates_without_invalidating_existing_heaps() {
    for remaining in [0, 1] {
        let mut vm = Vm::default();
        vm.heap.allow_root_registrations(remaining);
        assert_eq!(
            vm.string_intrinsics(),
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        );
        assert!(vm.string_intrinsics.is_none() && vm.function_intrinsic_prototype.is_none());
        assert!(vm.stack.is_empty());
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        for owner in [vm.object_prototype, vm.array_prototype] {
            // Bootstrap can already have installed methods on permanent
            // prototypes before the final root registration fails. Every
            // published edge must remain valid after collection.
            assert!(vm.heap.contains(owner));
            for key in vm.heap.own_property_keys(owner).unwrap() {
                let descriptor = vm
                    .heap
                    .get_own_property_descriptor(owner, &key)
                    .unwrap()
                    .unwrap();
                for value in descriptor
                    .value
                    .iter()
                    .chain(descriptor.get.iter())
                    .chain(descriptor.set.iter())
                {
                    if let Some(object) = value.object_id() {
                        assert!(vm.heap.contains(object));
                    }
                }
            }
        }
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
    }
    let expected = RuntimeError::Heap(HeapError::IdExhausted);
    let code = compile(&parse("let local = {answer:42}; local;").unwrap()).unwrap();
    let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
    for thrown in [false, true] {
        let mut vm = Vm::default();
        vm.prepare_root_execution(&code, false).unwrap();
        vm.prepare_global_declarations(&code).unwrap();
        let value = vm.run(&code).unwrap();
        let object = value.object_id().unwrap();
        vm.heap.allow_root_registrations(0);
        let completion = if thrown {
            Err(RuntimeError::Thrown(value))
        } else {
            Ok(value)
        };
        assert_eq!(vm.finish_root_execution(completion), Err(expected.clone()));
        assert!(vm.stack.is_empty() && vm.bindings.is_empty() && vm.cells.is_empty());
        assert!(vm.binding_metadata.is_empty() && vm.pending_completions.is_empty());
        assert!(vm.result_root.is_none());
        assert!(vm.heap.contains(object));
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }

    // The foundational intrinsics are already complete. Temporal has its
    // own fallible namespace registration and must publish nothing on failure.
    let mut temporal = Vm::default();
    temporal
        .execute_script(&compile(&parse("String; globalThis;").unwrap()).unwrap())
        .unwrap();
    temporal.heap.allow_root_registrations(0);
    assert_eq!(temporal.temporal_global(), Err(expected.clone()));
    assert!(!temporal.globals.contains_key("Temporal"));
    assert_eq!(temporal.execute_script(&reuse), Ok(Value::Number(42.0)));

    for exhaust_child in [false, true] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.execute_script(&compile(&parse("$262.createRealm();").unwrap()).unwrap())
            .unwrap();
        let realm = *vm.test262_realms.keys().next().unwrap();
        let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
        let target = child.heap.alloc_object(None).unwrap();
        let root = child.heap.root(target).unwrap();
        if exhaust_child {
            child.heap.allow_root_registrations(0);
        } else {
            vm.heap.allow_root_registrations(0);
        }
        assert_eq!(
            vm.test262_import_foreign_value(realm, Value::Object(target)),
            Err(expected.clone())
        );
        assert!(!vm.test262_realms[&realm].wrappers.contains_key(&target));
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
        vm.test262_realms
            .get_mut(&realm)
            .unwrap()
            .vm
            .heap
            .unroot(root)
            .unwrap();
    }

    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.execute_script(
        &compile(&parse("$262.createRealm(); globalThis.retained = {answer:42};").unwrap())
            .unwrap(),
    )
    .unwrap();
    let realm = *vm.test262_realms.keys().next().unwrap();
    let value = vm.lookup_global_name("retained").unwrap().unwrap();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.test262_export_foreign_value(realm, &value),
        Err(expected)
    );
    assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
}

#[cfg_attr(test, test)]
fn failed_reverse_facade_root_registration_releases_every_unpublished_owner() {
    for fail_in_child in [false, true] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        let realm = vm
            .execute_script(&compile(&parse("$262.createRealm().global").unwrap()).unwrap())
            .unwrap()
            .object_id()
            .unwrap();
        {
            let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
            child
                .with_roots(|heap| {
                    heap.collect_major();
                    Ok(())
                })
                .unwrap();
        }
        let before = vm.test262_realms[&realm].vm.heap.stats();
        let source = vm
            .execute_script(&compile(&parse("({answer:42})").unwrap()).unwrap())
            .unwrap();
        let source_id = source.object_id().unwrap();
        if fail_in_child {
            vm.test262_realms
                .get_mut(&realm)
                .unwrap()
                .vm
                .heap
                .allow_root_registrations(1);
        } else {
            vm.heap.allow_root_registrations(1);
        }
        assert_eq!(
            vm.test262_export_foreign_value(realm, &source),
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        );
        assert!(!vm.test262_realms[&realm]
            .imported_sources
            .contains_key(&source_id));
        assert!(vm.test262_realms[&realm]
            .vm
            .test262_reverse_values
            .is_empty());
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        assert!(
            !vm.heap.contains(source_id),
            "failed reverse construction retained its parent target root"
        );
        let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
        child
            .with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
        let after = child.heap.stats();
        assert_eq!(
            after.nursery_objects + after.tenured_objects,
            before.nursery_objects + before.tenured_objects,
            "failed reverse construction retained its child facade root"
        );
    }
}

#[cfg_attr(test, test)]
fn failed_realm_creation_releases_parent_facades_and_publishes_no_child() {
    let mut parent = Vm::default();
    parent.install_test262_harness().unwrap();
    let config = parent.config;
    parent.config.heap.max_heap_bytes = 0;
    assert!(matches!(
        parent.test262_create_realm(),
        Err(RuntimeError::Heap(HeapError::InvalidConfig))
    ));
    assert!(parent.test262_realms.is_empty() && parent.test262_foreign_values.is_empty());
    parent.config = config;
    assert!(parent.test262_create_realm().is_ok());
    for remaining_roots in [0, 1] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let before = vm.heap.stats();
        vm.heap.allow_root_registrations(remaining_roots);
        assert_eq!(
            vm.test262_create_realm(),
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        );
        assert!(vm.test262_realms.is_empty());
        assert!(vm.test262_foreign_values.is_empty());
        assert!(vm.stack.is_empty());
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        let after = vm.heap.stats();
        assert_eq!(
            after.nursery_objects + after.tenured_objects,
            before.nursery_objects + before.tenured_objects,
            "failed realm publication retained a parent facade"
        );
    }
    for extra in (0..16_384).step_by(8) {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.heap.allow_only(extra);
        let result = vm.test262_create_realm();
        let completed = result.is_ok();
        if !completed {
            assert!(
                matches!(
                    result,
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
                ),
                "{extra}: {result:?}"
            );
            assert!(
                vm.test262_realms.is_empty() && vm.test262_foreign_values.is_empty(),
                "{extra}: incomplete realm publication"
            );
        }
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        if completed {
            return;
        }
    }
    panic!("the realm creation allocation boundary did not reach success");
}

#[cfg_attr(test, test)]
fn shadow_child_initialization_failure_is_opaque_and_publishes_no_realm() {
    let mut vm = Vm::default();
    let construction = compile(&parse("new ShadowRealm()").unwrap()).unwrap();
    vm.global("ShadowRealm").unwrap();
    let config = vm.config;
    // The already valid parent heap is unchanged. Exercise validation of the
    // separate child configuration at the constructor boundary.
    vm.config.heap.max_heap_bytes = 0;
    assert_eq!(
        vm.execute_script(&construction),
        Err(RuntimeError::RangeError(
            "could not create a ShadowRealm".into()
        ))
    );
    assert!(vm.shadow_realms.is_empty());
    assert!(vm.shadow_realm_by_heap.is_empty());
    vm.config = config;
    assert!(matches!(
        vm.execute_script(&construction),
        Ok(Value::Object(_))
    ));
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn shadow_wrapped_functions_reject_construction_and_busy_realms_reject_imports() {
    let mut vm = Vm::default();
    let realm = vm
        .execute_script(&compile(&parse("new ShadowRealm()").unwrap()).unwrap())
        .unwrap();
    let realm_id = realm.object_id().unwrap();
    let root = vm.heap.root(realm_id).unwrap();
    let function = vm
        .shadow_realm_evaluate(
            realm.clone(),
            Value::String("(function() {return 42;})".into()),
        )
        .unwrap();
    let wrapper = function.object_id().unwrap();
    let wrapper_root = vm.heap.root(wrapper).unwrap();
    assert_eq!(
        vm.shadow_call_wrapped(wrapper, Value::Undefined, Vec::new(), true),
        Err(RuntimeError::TypeError(
            "a ShadowRealm wrapped function has no [[Construct]]".into()
        ))
    );
    let record = vm.shadow_realms[&realm_id].clone();
    let busy = record.vm.borrow_mut();
    let promise = vm
        .shadow_realm_import_value(
            realm.clone(),
            Value::String("unused".into()),
            Value::String("value".into()),
        )
        .unwrap()
        .object_id()
        .unwrap();
    drop(busy);
    vm.run_promise_jobs().unwrap();
    let PromiseStatus::Rejected(error) = &vm.promises[&promise].status else {
        panic!("a busy ShadowRealm import did not reject");
    };
    let error = error.clone();
    assert_eq!(
        vm.get_property(&error, &"name".into()).unwrap(),
        Value::String("TypeError".into())
    );
    assert_eq!(
        vm.shadow_call_wrapped(wrapper, Value::Undefined, Vec::new(), false),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(wrapper_root).unwrap();
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn shadow_ancestor_resolution_rejects_absent_and_self_contexts_before_reborrowing() {
    use super::realm_reentrancy::register_active;
    let mut child = Vm::default();
    let mut parent = Vm::default();
    let parent_heap = parent.object_prototype.heap;
    let unavailable = RuntimeError::TypeError(
        "the ShadowRealm this function belongs to is no longer reachable".into(),
    );
    assert_eq!(
        child.resolve_shadow_ancestor(None),
        Err(unavailable.clone())
    );
    assert_eq!(
        child.resolve_shadow_ancestor(Some(parent_heap)),
        Err(unavailable)
    );
    let expected = &mut parent as *mut Vm;
    {
        let guard = register_active(&mut parent);
        assert_eq!(
            child.resolve_shadow_ancestor(Some(parent_heap)),
            Ok(expected)
        );
        drop(guard);
    }
    let child_heap = child.object_prototype.heap;
    let guard = register_active(&mut child);
    assert_eq!(
        child.resolve_shadow_ancestor(Some(child_heap)),
        Err(RuntimeError::TypeError(
            "a ShadowRealm boundary cannot resolve to itself".into()
        ))
    );
    drop(guard);
}

#[cfg_attr(test, test)]
fn captured_binding_reads_reject_foreign_cells_and_preserve_the_original_cell() {
    let metadata = compile(&parse("let captured = 42; captured;").unwrap()).unwrap();
    let slot = metadata
        .bindings
        .iter()
        .position(|binding| binding.name == "captured")
        .unwrap();
    let mut vm = Vm::default();
    vm.prepare_root_execution(&metadata, false).unwrap();
    assert_eq!(vm.run(&metadata), Ok(Value::Number(42.0)));
    let original = vm.capture(slot).unwrap();
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(foreign).unwrap();
    owner
        .heap
        .set(foreign, "value", Value::Number(7.0))
        .unwrap();
    let visible = [("captured".to_string(), metadata.bindings[slot].clone(), 0)];
    for source in [
        "captured",
        "typeof captured",
        "captured = 7",
        "captured += 1",
        "captured++",
    ] {
        let code = crate::compiler::compile_eval(
            &parse(source).unwrap(),
            &visible,
            &[],
            &[],
            crate::compiler::EvalContext::default(),
            crate::CompileLimits::default(),
        )
        .unwrap();
        assert_eq!(
            vm.execute_eval(&code, vec![foreign], false),
            Err(RuntimeError::Heap(HeapError::InvalidObject(foreign))),
            "{source}"
        );
        assert_eq!(vm.cells.get(&slot), Some(&original));
        assert_eq!(vm.binding_value(slot), Ok(Some(Value::Number(42.0))));
        assert!(vm.stack.is_empty());
    }
    assert_eq!(
        owner.heap.get_own(foreign, "value").unwrap(),
        Some(Value::Number(7.0))
    );
    vm.finish_root_execution(Ok(Value::Number(42.0))).unwrap();
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn reverse_definition_and_deletion_reject_an_inactive_parent() {
    let mut parent = Vm::default();
    parent.install_test262_harness().unwrap();
    let global = parent
        .execute_script(
            &compile(
                &parse("globalThis.retainedParent = {answer: 42}; $262.createRealm().global")
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
        .object_id()
        .unwrap();
    let realm = parent.test262_foreign_reference(global).unwrap().0;
    let original = parent
        .lookup_global_name("retainedParent")
        .unwrap()
        .unwrap();
    let wrapper = parent
        .test262_export_foreign_value(realm, &original)
        .unwrap()
        .object_id()
        .unwrap();
    let child = &mut parent.test262_realms.get_mut(&realm).unwrap().vm;
    assert_eq!(
        child.test262_reverse_reference(wrapper).unwrap().1,
        original.object_id().unwrap()
    );
    let unavailable = RuntimeError::TypeError(
        "the Test262 realm this value belongs to is no longer reachable".into(),
    );
    assert_eq!(
        child.test262_reverse_define_own_property(
            wrapper,
            "answer".into(),
            PropertyDescriptor {
                value: Some(Value::Number(0.0)),
                ..PropertyDescriptor::default()
            },
        ),
        Err(unavailable.clone())
    );
    assert_eq!(
        child.test262_reverse_delete(wrapper, &"answer".into()),
        Err(unavailable)
    );
    assert_eq!(
        parent.execute_script(&compile(&parse("retainedParent.answer").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn failed_shadow_wrapper_does_not_retain_its_target() {
    for property in ["length", "name"] {
        let mut vm = Vm::default();
        let realm = vm
            .execute_script(&compile(&parse("new ShadowRealm()").unwrap()).unwrap())
            .unwrap();
        let realm_id = realm.object_id().unwrap();
        let realm_root = vm.heap.root(realm_id).unwrap();
        let source = format!("globalThis.target = function() {{return 42;}}; Object.defineProperty(target, '{property}', {{get() {{throw 'private';}}, configurable: true}}); target");
        assert!(matches!(
            vm.shadow_realm_evaluate(realm.clone(), Value::String(source.into())),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(vm.shadow_wrapped_functions.is_empty());
        let record = vm.shadow_realms[&realm_id].clone();
        let mut child = record.vm.borrow_mut();
        let target = child
            .lookup_global_name("target")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        child
            .execute_script(&compile(&parse("delete globalThis.target; 0").unwrap()).unwrap())
            .unwrap();
        child
            .with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
        assert!(
            child.heap.get_own(target, property).is_err(),
            "failed {property} copy retained the target"
        );
        drop(child);
        assert_eq!(
            vm.shadow_realm_evaluate(realm, Value::String("21 + 21".into())),
            Ok(Value::Number(42.0))
        );
        vm.heap.unroot(realm_root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn allocation_failures_preserve_vm_reusability() {
    // The existing heap test hook reduces a valid heap's budget without
    // invalidating any handle. Each refusal gives the budget needed to reach
    // the next allocating step, including normal collection. Sources are
    // compiled once per scenario, outside the sweep.
    for (setup, operation) in [
        ("globalThis.make = Uint8Array;", "new make([1, 2, 3])"),
        ("globalThis.make = BigInt64Array;", "new make([1n, 2n])"),
        ("globalThis.make = Uint8Array;", "make.from({length: 2, 0: 1, 1: 2})"),
        ("globalThis.make = Uint8Array;", "make.of(1, 2)"),
        ("Atomics; globalThis.view = new Int32Array(new SharedArrayBuffer(4));", "Atomics.waitAsync(view, 0, 0, 0)"),
        ("globalThis.buffer = new ArrayBuffer(8, {maxByteLength: 16});", "buffer.slice(1, 6)"),
        ("globalThis.buffer = new ArrayBuffer(8, {maxByteLength: 16});", "buffer.transfer(12)"),
        ("globalThis.view = new Uint8Array(4);", "view.subarray(1, 3)"),
        ("globalThis.regexp = /old/g;", "regexp.compile('(new|alternative){1,10}', 'di')"),
        ("globalThis.G = function* (value = eval('var extra = 7')) { yield extra; }; G();", "G()"),
        ("globalThis.G = function* () { yield {value: 1}; }; G();", "globalThis.iterator = G(); iterator.next()"),
        ("globalThis.G = function* () { yield 1; }; globalThis.iterator = G(); iterator.next();", "iterator.return(7)"),
        ("Temporal.PlainDate;", "Temporal.PlainDate.from('2020-02-29')"),
        ("globalThis.date = Temporal.PlainDate.from('2020-02-29');", "date.add({years: 1})"),
        ("globalThis.date = Temporal.PlainDateTime.from('2020-02-29T12:00');", "date.until('2021-03-01T13:00')"),
        ("globalThis.date = Temporal.ZonedDateTime.from('2020-02-29T12:00[UTC]');", "date.until('2021-03-01T13:00[UTC]', {largestUnit:'year'})"),
        ("Object.defineProperty;", "Object.create(null, {value: {value: 7}})"),
        ("globalThis.object = Object.defineProperty({}, 'value', {get:Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get});", "try {object.value; throw 'accepted getter receiver';} catch (error) {if (!(error instanceof TypeError)) throw error;} 42"),
        ("globalThis.object = Object.defineProperty({}, 'value', {set:Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get});", "try {object.value = 7; throw 'accepted setter receiver';} catch (error) {if (!(error instanceof TypeError)) throw error;} 42"),
        ("globalThis.child = $262.createRealm().global; globalThis.object = Object.defineProperty({exec() {return null;}}, 'flags', {get:Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get});", "try {child.RegExp.prototype[Symbol.match].call(object, ''); throw 'accepted foreign getter';} catch (error) {if (!(error instanceof TypeError)) throw error;} 42"),
        ("globalThis.child = $262.createRealm().global; globalThis.object = Object.defineProperty({flags:'g', exec() {return null;}}, 'lastIndex', {set:Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get});", "try {child.RegExp.prototype[Symbol.match].call(object, ''); throw 'accepted foreign setter';} catch (error) {if (!(error instanceof TypeError)) throw error;} 42"),
        ("var constructor = 7; globalThis.payload = 'x'.repeat(512); globalThis.setter = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor').set;", "setter.call(globalThis, payload); if (constructor !== payload || globalThis.constructor !== payload) throw 'global setter desynchronized its binding'; 42"),
        ("var constructor = 7; globalThis.payload = 'x'.repeat(512); Reflect.set;", "Reflect.set(globalThis, 'constructor', payload); if (constructor !== payload || globalThis.constructor !== payload) throw 'Reflect.set desynchronized its binding'; 42"),
        ("var constructor = 7; globalThis.payload = 'x'.repeat(512); globalThis.child = $262.createRealm().global; globalThis.setter = Object.getOwnPropertyDescriptor(child.Iterator.prototype, 'constructor').set;", "setter.call(globalThis, payload); if (constructor !== payload || globalThis.constructor !== payload) throw 'foreign setter desynchronized its binding'; 42"),
        ("globalThis.object = {};", "Object.defineProperties(object, {one: {value: 1}, two: {value: 2}})"),
        ("Iterator; globalThis.source = [1,2,3];", "Iterator.from(source).map(value => value + 1)"),
        ("Iterator; globalThis.source = [1,2,3];", "Iterator.from(source).take(2)"),
        ("Iterator; globalThis.source = [1,2,3];", "Iterator.from(source).drop(1)"),
        ("Iterator.from; String.prototype[Symbol.iterator] = undefined;", "Iterator.from('abc')"),
        ("Iterator; globalThis.source = [1,2,3];", "Iterator.from(source).filter(value => value > 1).toArray()"),
        ("Iterator; globalThis.source = [1,2,3];", "Iterator.from(source).flatMap(value => [value, value]).toArray()"),
        ("Iterator.zip; globalThis.source = [[1,2], [3,4]];", "Iterator.zip(source, {mode:'longest', padding:[7,8]}).next()"),
        ("Iterator.zip; globalThis.source = [[1], [2]]; globalThis.padding = ['x'.repeat(256), 'y'.repeat(256)];", "Iterator.zip(source, {mode:'longest', padding}).next()"),
        ("Iterator.zipKeyed; globalThis.source = {one:[1,2], two:[3,4]};", "Iterator.zipKeyed(source).next()"),
        ("Iterator.concat; globalThis.source = [1,2,3];", "Iterator.concat(source, source).next()"),
        ("Iterator; globalThis.source = [1,2,3];", "Iterator.from(source).chunks(2).next()"),
        ("Iterator; globalThis.source = [1,2,3];", "Iterator.from(source).windows(2).next()"),
        ("globalThis.Base = class {};", "class Child extends Base { #value = 7; get value() {return this.#value;} } new Child().value"),
        ("Iterator; [1,2][Symbol.iterator]();", "Iterator.from([1,2]).map(value => value).next()"),
        ("RegExp;", "RegExp('(?<name>a)', 'dg').exec('a')"),
        ("globalThis.regexp = /a/g;", "regexp[Symbol.matchAll]('aa').next()"),
        ("globalThis.dispose = Object.getPrototypeOf(Object.getPrototypeOf((async function*() {}).prototype))[Symbol.asyncDispose]; globalThis.returned = Promise.resolve(42); globalThis.receiver = {return() {return returned;}};", "dispose.call(receiver)"),
        ("ShadowRealm;", "new ShadowRealm()"),
        ("$262;", "$262.createRealm()"),
        ("globalThis.child = $262.createRealm();", "child.evalScript('({value:7})')"),
        ("globalThis.child = $262.createRealm();", "child.evalScript('new Uint8Array([1,2,3])').slice()"),
        ("globalThis.child = $262.createRealm(); globalThis.target = {}; child.global.target = target; globalThis.define = child.evalScript('() => Object.defineProperty(target, \"value\", {get() {return 42;}, configurable:true})');", "define()"),
        ("String;", "new (function Outer() {this.arrow = () => new.target;})()"),
        ("String;", "({method() {return () => super.value;}})"),
        ("String; globalThis.object = {value:7};", "with (object) { (function() {return value;}) }"),
        ("String;", "class C {static #value = 7; static #method() {return this.#value;} static get #accessor() {return this.#method();} static run() {return this.#accessor;}} C.run()"),
        ("String;", "class C {#method() {return 7;} get #accessor() {return this.#method();} run() {return this.#accessor;}} new C().run()"),
        ("String;", "var closures = []; for (let i = 0; i < 3; i++) {closures.push(() => i);} closures[2]()"),
        ("String;", "function f(value = eval('var parameterVar = 7')) {eval('var parameterVar = (delete parameterVar, 42)'); return parameterVar;} f()"),
        ("eval('var recreated = 7');", "eval(\"var recreated = (delete recreated, 'x'.repeat(64)); recreated\")"),
        ("String;", "function f() {eval('var deleted = 7; delete deleted; var deleted = 42'); return deleted;} f()"),
        ("String;", "function f() {return eval('var ArrayBuffer = 7; delete ArrayBuffer; ArrayBuffer');} f()"),
        ("Iterator; globalThis.values = [1,2,3];", "var [first, ...rest] = values; rest"),
        ("Iterator; globalThis.values = [1,2,3];", "var copied = [...values]; copied"),
        ("String; globalThis.source = {one:1, two:2};", "var {one, ...rest} = source; rest"),
        ("String; globalThis.source = {one:1}; globalThis.destination = {};", "({one:destination.value} = source)"),
        ("String;", "(function f() {'use strict'; return arguments;})(7)"),
        ("String;", "Object.getOwnPropertyDescriptor((function named() {}), 'caller')"),
        ("Uint8Array; String.prototype[Symbol.iterator] = undefined;", "Uint8Array.from('42')"),
        ("globalThis.child = $262.createRealm().global; Uint8Array; child.Uint8Array;", "Uint8Array.from.call(child.Uint8Array, {length:2,0:7,1:42}, x => x)"),
        ("globalThis.child = $262.createRealm().global; Uint8Array; child.Uint8Array;", "Uint8Array.of.call(child.Uint8Array, 7,42)"),
        ("String;", "class C {static #value = 'x'.repeat(128); static read() {return this.#value;}} C.read()"),
        ("String;", "function f(...rest) {return rest;} f(7,13,42)"),
        ("String;", "function f(...rest) {return eval(...rest);} f('var answer = 42; answer')"),
        ("String;", "var object = {}; ({answer:object[{toString() {return 'answer';}}]} = {answer:42}); object.answer"),
        ("String;", "class C {#value = 7; update() {return this.#value++;}} new C().update()"),
        ("String;", "var object = {value:7, update() {return super.value++;}}; Object.setPrototypeOf(object, {value:42}); object.update()"),
        ("globalThis;", "Object.getOwnPropertyDescriptor(globalThis, 'Temporal')"),
        ("globalThis;", "Reflect.deleteProperty(globalThis, 'Temporal')"),
        ("globalThis;", "Reflect.ownKeys(globalThis)"),
        ("globalThis;", "Reflect.preventExtensions(globalThis)"),
        ("Object;", "Object.getOwnPropertyDescriptors({a:7, b:42})"),
        ("Object;", "Object.defineProperties({}, {a:{value:'x'.repeat(128)}, b:{get() {return 42;}}})"),
        ("globalThis.receiver = function() {};", "Proxy.revocable(receiver, {})"),
        ("globalThis.realm = new ShadowRealm();", "realm.importValue('missing', 'value')"),
        ("String;", "new Error('message', {cause:7})"),
        ("String;", "Function('return 42')()"),
        ("String;", "'abc'[Symbol.iterator]().next()"),
        ("String;", "[7][Symbol.iterator]().next()"),
        ("String;", "new Intl.Collator('en').compare('a', 'b')"),
        ("Intl.Collator;", "new Intl.Collator('en').compare('a', 'b')"),
        ("Intl.NumberFormat;", "new Intl.NumberFormat('en').format(42)"),
        ("Intl.DateTimeFormat;", "new Intl.DateTimeFormat('en', {timeZone:'UTC'}).format(0)"),
        ("globalThis.formatter = new Intl.DateTimeFormat('en', {timeZone:'UTC', dateStyle:'full', timeStyle:'long'});", "formatter.resolvedOptions()"),
        ("globalThis.formatter = new Intl.DateTimeFormat('en', {timeZone:'UTC', weekday:'long', era:'narrow', year:'2-digit', month:'long', day:'numeric', dayPeriod:'narrow', hour:'numeric', minute:'2-digit', second:'2-digit', fractionalSecondDigits:3, timeZoneName:'long'});", "formatter.resolvedOptions()"),
        ("globalThis.formatter = new Intl.DateTimeFormat('en', {timeZone:'UTC'});", "formatter.formatToParts(0)"),
        ("globalThis.formatter = new Intl.DateTimeFormat('en', {timeZone:'UTC'});", "formatter.format"),
        ("globalThis.formatter = new Intl.DateTimeFormat('en', {timeZone:'UTC'});", "formatter.formatRangeToParts(0, 86400000)"),
        ("Intl.Segmenter;", "new Intl.Segmenter('en').segment('abc')[Symbol.iterator]().next()"),
    ] {
        let setup_code = compile(&parse(setup).unwrap()).unwrap();
        let operation_code = compile(&parse(operation).unwrap()).unwrap();
        let reuse_code = compile(&parse("21 + 21").unwrap()).unwrap();
        let mut failures = 0;
        let mut completed = false;
        let mut next_extra = 0;
        while next_extra <= 16 * 1024 * 1024 {
            let extra = next_extra;
            let mut vm = Vm::default();
            if setup.contains("$262") {vm.install_test262_harness().unwrap();}
            vm.execute_script(&setup_code).unwrap();
            vm.execute_script(&reuse_code).unwrap();
            vm.with_roots(|heap| {heap.collect_major(); Ok(())}).unwrap();
            let limit = vm.heap.allow_only(extra);
            match vm.execute_script(&operation_code) {
                Ok(_) => completed = true,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded {limit: observed})) => {
                    assert_eq!(observed, limit, "{operation}");
                    failures += 1;
                }
                Err(error) => panic!("{operation}, headroom {extra}: {error:?}"),
            }
            if !completed { next_extra = vm.heap.next_allocation_headroom(extra); }
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(vm.execute_script(&reuse_code), Ok(Value::Number(42.0)), "{operation}, headroom {extra}");
            if completed {break;}
        }
        assert!(completed && failures > 0, "allocation boundary was not exercised: {operation}; completed={completed}, failures={failures}");
    }
}

#[cfg_attr(test, test)]
fn unpublished_collection_promise_and_disposal_prototypes_are_strong_vm_edges() {
    type Builder = fn(&mut Vm) -> Result<ObjectId, RuntimeError>;
    let builders: &[(&str, Builder)] = &[
        ("Map", |vm| vm.collection_prototype(true)),
        ("Set", |vm| vm.collection_prototype(false)),
        ("WeakMap", |vm| vm.weak_collection_prototype(true)),
        ("WeakSet", |vm| vm.weak_collection_prototype(false)),
        ("WeakRef", Vm::weak_ref_prototype),
        ("FinalizationRegistry", Vm::finalization_registry_prototype),
        ("Promise", Vm::promise_prototype),
        ("DisposableStack", |vm| vm.disposable_stack_prototype(false)),
        ("AsyncDisposableStack", |vm| {
            vm.disposable_stack_prototype(true)
        }),
    ];
    for nursery_capacity in [1, crate::heap::HeapConfig::default().nursery_capacity] {
        for &(name, builder) in builders {
            let mut config = VmConfig::default();
            config.heap.nursery_capacity = nursery_capacity;
            let mut vm = Vm::new(config).unwrap();
            vm.string_intrinsics().unwrap();
            let prototype = builder(&mut vm).unwrap();
            assert!(!vm.globals.contains_key(name));
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            assert!(
                vm.heap.contains(prototype),
                "unpublished {name} prototype was collected"
            );
            assert_eq!(builder(&mut vm), Ok(prototype));
            let constructor = vm.global(name).unwrap().object_id().unwrap();
            assert_eq!(
                vm.heap.get(constructor, "prototype").unwrap(),
                Value::Object(prototype),
                "{name} changed intrinsic identity"
            );
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            assert_eq!(
                vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
        }
    }
}

#[cfg_attr(test, test)]
fn cached_shadow_prototypes_survive_collection_and_refused_roots_publish_nothing() {
    let code = compile(
        &parse("Object.getPrototypeOf(new ShadowRealm()) === ShadowRealm.prototype").unwrap(),
    )
    .unwrap();
    for nursery_capacity in [1, crate::heap::HeapConfig::default().nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        let mut vm = Vm::new(config).unwrap();
        let prototype = vm.shadow_realm_prototype().unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert!(vm.heap.contains(prototype));
        assert_eq!(vm.shadow_realm_prototype(), Ok(prototype));
        assert_eq!(vm.execute_script(&code), Ok(Value::Bool(true)));
    }
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.shadow_realm_prototype(),
        Err(RuntimeError::Heap(crate::heap::HeapError::IdExhausted))
    );
    assert!(vm.shadow_realm_prototype.is_none() && vm.stack.is_empty());
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn intrinsic_builders_restore_roots_and_retry_after_cold_and_warm_allocation_failures() {
    type Builder = fn(&mut Vm) -> Result<ObjectId, RuntimeError>;
    let builders: &[(&str, Builder)] = &[
        ("Function prototype", Vm::function_prototype),
        ("Promise prototype", Vm::promise_prototype),
        ("Map prototype", |vm| vm.collection_prototype(true)),
        ("Set prototype", |vm| vm.collection_prototype(false)),
        ("WeakMap prototype", |vm| vm.weak_collection_prototype(true)),
        ("WeakSet prototype", |vm| {
            vm.weak_collection_prototype(false)
        }),
        ("WeakRef prototype", Vm::weak_ref_prototype),
        (
            "FinalizationRegistry prototype",
            Vm::finalization_registry_prototype,
        ),
        ("Map native construction", |vm| {
            vm.new_target = vm.global("Object")?;
            vm.collection_constructor(true, &[], true)
                .map(|value| value.object_id().unwrap())
        }),
        ("Set native construction", |vm| {
            vm.new_target = vm.global("Object")?;
            vm.collection_constructor(false, &[], true)
                .map(|value| value.object_id().unwrap())
        }),
        ("WeakMap native construction", |vm| {
            vm.new_target = vm.global("Object")?;
            vm.weak_collection_constructor(true, &[], true)
                .map(|value| value.object_id().unwrap())
        }),
        ("WeakSet native construction", |vm| {
            vm.new_target = vm.global("Object")?;
            vm.weak_collection_constructor(false, &[], true)
                .map(|value| value.object_id().unwrap())
        }),
        ("WeakRef native construction", |vm| {
            vm.new_target = vm.global("Object")?;
            vm.weak_ref_constructor(Value::Symbol(JsSymbol::new(None)), true)
                .map(|value| value.object_id().unwrap())
        }),
        ("FinalizationRegistry native construction", |vm| {
            vm.new_target = vm.global("Object")?;
            let prototype = vm.function_prototype()?;
            let callback = vm.with_roots(|heap| {
                heap.alloc_native_function(NativeFunction::Empty, "cleanup", prototype)
            })?;
            vm.finalization_registry_constructor(Value::Object(callback), true)
                .map(|value| value.object_id().unwrap())
        }),
        ("ShadowRealm prototype", Vm::shadow_realm_prototype),
        ("Iterator prototype", Vm::base_iterator_prototype),
        ("Iterator wrapper prototype", Vm::iterator_wrapper_prototype),
        ("Iterator helper prototype", Vm::iterator_helper_prototype),
        ("Array iterator prototype", Vm::array_iterator_prototype),
        ("String iterator prototype", Vm::string_iterator_prototype),
        (
            "Generator function prototype",
            Vm::generator_function_prototype,
        ),
        ("Generator prototype", Vm::generator_prototype),
        ("Async iterator prototype", Vm::async_iterator_prototype),
        ("Async generator prototype", Vm::async_generator_prototype),
        (
            "Async generator function prototype",
            Vm::async_generator_function_prototype,
        ),
        ("Async function prototype", Vm::async_function_prototype),
        ("RegExp", |vm| {
            vm.regexp_global().map(|value| value.object_id().unwrap())
        }),
        ("Temporal", |vm| {
            vm.temporal_global().map(|value| value.object_id().unwrap())
        }),
        ("Intl", |vm| {
            vm.intl_global().map(|value| value.object_id().unwrap())
        }),
        ("TypeError", |vm| {
            vm.error_global("TypeError")
                .map(|value| value.object_id().unwrap())
        }),
        ("async generator brand rejection", |vm| {
            vm.remaining_instructions = vm.config.instruction_budget;
            vm.async_generator_request(
                &Value::Undefined,
                Value::Undefined,
                NativeFunction::AsyncGeneratorNext,
            )
            .map(|value| value.object_id().unwrap())
        }),
        ("ArrayBuffer", |vm| {
            vm.global("ArrayBuffer")
                .map(|value| value.object_id().unwrap())
        }),
        ("globalThis", |vm| {
            vm.global("globalThis")
                .map(|value| value.object_id().unwrap())
        }),
        ("ShadowRealm instance", |vm| {
            let code = compile(&parse("new ShadowRealm()").unwrap()).unwrap();
            // Script entry establishes fuel and constructor context together.
            vm.execute_script(&code)
                .map(|value| value.object_id().unwrap())
        }),
    ];
    let mut failures = Vec::new();
    for &(name, builder) in builders {
        if std::panic::catch_unwind(|| Vm::verify_intrinsic_allocation_boundary(name, builder))
            .is_err()
        {
            failures.push(format!("intrinsic {name}"));
        }
    }
    for name in [
        "String",
        "Error",
        "TypeError",
        "RangeError",
        "SyntaxError",
        "ReferenceError",
        "EvalError",
        "URIError",
        "AggregateError",
        "SuppressedError",
        "Object",
        "Function",
        "Array",
        "Number",
        "Boolean",
        "BigInt",
        "Symbol",
        "Date",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "WeakRef",
        "FinalizationRegistry",
        "Promise",
        "Reflect",
        "Math",
        "JSON",
        "Atomics",
        "DataView",
        "DisposableStack",
        "AsyncDisposableStack",
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "BigInt64Array",
        "BigUint64Array",
        "SharedArrayBuffer",
        "Iterator",
        "ShadowRealm",
        "Proxy",
        "eval",
        "isNaN",
        "isFinite",
        "parseInt",
        "parseFloat",
        "encodeURI",
        "encodeURIComponent",
        "decodeURI",
        "decodeURIComponent",
        "escape",
        "unescape",
    ] {
        if std::panic::catch_unwind(|| Vm::verify_global_allocation_boundary(name)).is_err() {
            failures.push(format!("global {name}"));
        }
    }
    assert!(
        failures.is_empty(),
        "initialization matrix failures: {}",
        failures.join(", ")
    );
}

#[cfg_attr(test, test)]
fn ordinary_call_return_checks_existing_strings_after_the_budget_changes() {
    let mut vm = Vm::default();
    vm.execute_script(
        &compile(
            &parse(
                "globalThis.cached = 'oversized'; globalThis.read = function() {return cached;};",
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let config = vm.config;
    vm.config.max_string_bytes = 2;
    assert_eq!(
        vm.execute_script(&compile(&parse("read()").unwrap()).unwrap()),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert!(vm.stack.is_empty());
    vm.config = config;
    assert_eq!(
        vm.execute_script(&compile(&parse("read()").unwrap()).unwrap()),
        Ok(Value::String("oversized".into()))
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn segmenter_prototype_root_failures_release_the_first_prototype_without_publishing() {
    let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
    for remaining in [0, 1] {
        let mut vm = Vm::default();
        vm.intl_global().unwrap();
        vm.base_iterator_prototype().unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let before = vm.heap.stats();
        vm.heap.allow_root_registrations(remaining);
        assert_eq!(
            vm.segmenter_internal_prototypes(),
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        );
        assert!(
            !vm.globals.contains_key("%Intl.SegmentsPrototype%")
                && !vm.globals.contains_key("%Intl.SegmentIteratorPrototype%")
        );
        assert!(vm.stack.is_empty());
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
        let after = vm.heap.stats();
        assert_eq!(
            after.nursery_objects + after.tenured_objects,
            before.nursery_objects + before.tenured_objects
        );
    }
}

#[cfg_attr(test, test)]
fn failed_shadow_wrapping_releases_child_targets_and_preserves_opaque_metadata_errors() {
    let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
    let setup = compile(&parse("new ShadowRealm()").unwrap()).unwrap();
    let mut completed = false;
    let mut next_extra = 0;
    while next_extra <= 16 * 1024 * 1024 {
        let extra = next_extra;
        let mut vm = Vm::default();
        let realm = vm.execute_script(&setup).unwrap();
        let realm_id = realm.object_id().unwrap();
        let root = vm.heap.root(realm_id).unwrap();
        vm.heap.allow_only(extra);
        let result = vm.shadow_realm_evaluate(
            realm.clone(),
            Value::String("(function named(value) {return value + 1;})".into()),
        );
        if result.is_err() {
            next_extra = vm.heap.next_allocation_headroom(extra);
        }
        vm.heap.allow_only(16 * 1024 * 1024);
        match result {
            Ok(wrapper) => {
                assert_eq!(
                    vm.call_native(wrapper, Value::Undefined, vec![Value::Number(41.0)], false),
                    Ok(Value::Number(42.0))
                );
                completed = true;
            }
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {}
            Err(RuntimeError::TypeError(message)) => {
                assert_eq!(message, "could not wrap this function across a ShadowRealm")
            }
            Err(error) => panic!("shadow wrapping, {extra}: {error:?}"),
        }
        assert!(vm.stack.is_empty());
        if !completed {
            assert!(vm.shadow_wrapped_functions.is_empty());
        }
        let record = vm.shadow_realms[&realm_id].clone();
        let mut child = record.vm.borrow_mut();
        assert_eq!(child.execute_script(&reuse), Ok(Value::Number(42.0)));
        assert!(child.stack.is_empty());
        drop(child);
        if !completed {
            assert!(matches!(
                vm.shadow_realm_evaluate(
                    realm,
                    Value::String("(function(value) {return value + 1;})".into())
                ),
                Ok(Value::Object(_))
            ));
        }
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
        vm.heap.unroot(root).unwrap();
        if completed {
            break;
        }
    }
    assert!(completed);
}

#[cfg_attr(test, test)]
fn foreign_snapshot_allocation_failures_preserve_both_realms_and_source_identity() {
    let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
    for (setup, source) in [
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.make = () => ({answer:42})');", "child.make().answer"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.make = () => new Proxy({answer:42}, {})');", "child.make().answer"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.make = () => ({get answer() {return 42;}})');", "Object.getOwnPropertyDescriptor(child.make(), 'answer').get()"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.make = () => ({set answer(v) {globalThis.answer = v;}})');", "Object.getOwnPropertyDescriptor(child.make(), 'answer').set(42); child.answer"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.make = () => {throw {answer:42}}');", "try {child.make();} catch (error) {error.answer;}"),
        ("globalThis.child = $262.createRealm().global; child.eval('Uint8Array; globalThis.make = () => new Uint8Array([7,42])');", "child.make().slice()[1]"),
        ("globalThis.child = $262.createRealm().global; child.eval('ArrayBuffer; Uint8Array; globalThis.make = () => {var a = new ArrayBuffer(8); new Uint8Array(a)[0] = 42; return a.transferToImmutable()}');", "new Uint8Array(child.make().slice())[0]"),
        ("globalThis.child = $262.createRealm().global; child.eval('Uint8Array; globalThis.read = values => values.length === 3 && values[2] === 3 ? 42 : 0'); globalThis.values = [1,2,3];", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('Uint8Array; globalThis.read = values => values.length === 3 && values[2] === 3 ? 42 : 0'); globalThis.values = new Uint8Array([1,2,3]);", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('ArrayBuffer; Uint8Array; globalThis.read = buffer => buffer.byteLength === 8 && new Uint8Array(buffer)[0] === 42 ? 42 : 0'); globalThis.values = new ArrayBuffer(8, {maxByteLength:16}); new Uint8Array(values)[0] = 42;", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('SharedArrayBuffer; Uint8Array; globalThis.read = buffer => buffer.byteLength === 8 && new Uint8Array(buffer)[0] === 42 ? 42 : 0'); globalThis.values = new SharedArrayBuffer(8, {maxByteLength:16}); new Uint8Array(values)[0] = 42;", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('ArrayBuffer; globalThis.read = buffer => buffer.byteLength === 0 ? 42 : 0'); globalThis.values = new ArrayBuffer(8); $262.detachArrayBuffer(values);", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('ArrayBuffer; Uint8Array; globalThis.read = buffer => buffer.immutable && new Uint8Array(buffer)[0] === 42 ? 42 : 0'); globalThis.buffer = new ArrayBuffer(8); new Uint8Array(buffer)[0] = 42; globalThis.values = buffer.transferToImmutable();", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('Uint8Array; globalThis.values = new Uint8Array([1,2,3])');", "child.values.slice().length === 3 ? 42 : 0"),
        ("globalThis.child = $262.createRealm().global; child.eval('Object; globalThis.target = {}; globalThis.read = value => Object.defineProperty(target, \\\"answer\\\", {value, configurable:true}).answer'); globalThis.values = 42;", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.read = obj => (\"absent\" in obj) ? 0 : 42;'); globalThis.values = Object.create({});", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.proto = {}; globalThis.object = Object.create(proto);');", "('absent' in child.object) ? 0 : 42"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.object = {answer:{value:42}};');", "Object.getOwnPropertyDescriptor(child.object, 'answer').value.value"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.object = {answer:{value:42}};');", "('answer' in child.object) ? 42 : 0"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.b = new ArrayBuffer(8,{maxByteLength:16}); globalThis.ta = new Uint8Array(b); globalThis.write = () => {b.resize(16); ta[0] = 42; return 42;};'); globalThis.mirror = new Uint8Array(child.ta.buffer);", "child.write(); mirror[0]"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.b = new ArrayBuffer(8,{maxByteLength:16}); globalThis.ta = new Uint8Array(b);'); globalThis.mirror = new Uint8Array(child.ta.buffer); globalThis.value = {valueOf(){child.b.resize(16); return 42;}};", "child.ta.fill(value); mirror[0]"),
        ("Reflect; globalThis.child = $262.createRealm().global; child.eval('globalThis.b = new ArrayBuffer(8,{maxByteLength:16}); globalThis.ta = new Uint8Array(b);'); globalThis.mirror = new Uint8Array(child.ta.buffer); globalThis.value = {valueOf(){child.b.resize(16); return 42;}};", "Reflect.defineProperty(child.ta, '0', {value}); mirror[0]"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.b = new ArrayBuffer(8,{maxByteLength:16}); globalThis.ta = new Uint8Array(b);'); globalThis.mirror = new Uint8Array(child.ta.buffer); globalThis.value = {valueOf(){child.b.resize(16); return 42;}};", "child.ta[0] = value; mirror[0]"),
        ("Reflect; globalThis.child = $262.createRealm().global; child.ArrayBuffer; globalThis.Target = function() {}; Target.prototype = 7;", "Reflect.construct(child.ArrayBuffer, [8], Target).byteLength === 8 ? 42 : 0"),
        ("Reflect; globalThis.child = $262.createRealm().global; child.ArrayBuffer; globalThis.Target = new Proxy(function(){}, {get(target,key){if(key === 'prototype') return {answer:42}; return Reflect.get(target,key);}});", "Object.getPrototypeOf(Reflect.construct(child.ArrayBuffer, [8], Target)).answer"),
        ("globalThis.child = $262.createRealm().global; child.eval('globalThis.read = values => values[0].answer'); globalThis.values = [{answer:42}];", "child.read(values)"),
        ("globalThis.child = $262.createRealm().global; globalThis.Base = child.eval('(class {constructor() {this.answer = 42;}})'); globalThis.Derived = class extends Base {constructor() {globalThis.readThis = () => this; super();}};", "new Derived().answer"),
        ("Reflect; globalThis.child = $262.createRealm().global; child.eval('globalThis.target = new Uint8Array(8);'); child.TypeError;", "try {Reflect.defineProperty(child.target, '0', {value:Symbol(7)}); 0;} catch(error) {error instanceof child.TypeError ? 42 : 0;}"),
    ] {
        let setup_source = setup;
        let setup = compile(&parse(setup_source).unwrap()).unwrap();
        let operation = compile(&parse(source).unwrap()).unwrap();
        for limit_parent in [false, true] {
        let mut completed = false;
        let mut failures = 0;
        let mut next_extra = 0;
        while next_extra <= 16 * 1024 * 1024 {
            let extra = next_extra;
            let mut vm = Vm::default();
            vm.install_test262_harness().unwrap();
            vm.execute_script(&setup).unwrap();
            let realm = *vm.test262_realms.keys().next().unwrap();
            let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
            child.with_roots(|heap| {heap.collect_major(); Ok(())}).unwrap();
            let limit = if limit_parent {
                vm.with_roots(|heap| {heap.collect_major(); Ok(())}).unwrap();
                vm.heap.allow_only(extra)
            } else {
                child.heap.allow_only(extra)
            };
            match vm.execute_script(&operation) {
                Ok(answer) => {assert_eq!(answer, Value::Number(42.0), "{source}"); completed = true;}
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded {limit: actual})) => {assert_eq!(actual, limit); failures += 1;}
                Err(error) => panic!("{setup_source}; {source}, parent={limit_parent}, headroom {extra}: {error:?}"),
            }
            assert!(vm.stack.is_empty());
            let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
            assert!(child.stack.is_empty());
            if !completed && !limit_parent { next_extra = child.heap.next_allocation_headroom(extra); }
            child.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(child.execute_script(&reuse), Ok(Value::Number(42.0)));
            if !completed && limit_parent { next_extra = vm.heap.next_allocation_headroom(extra); }
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if completed {break;}
        }
        assert!(completed, "{setup_source}; {source}: parent={limit_parent} did not complete; failures={failures}");
        // Some calls return only primitives and allocate nothing in the caller.
        // Pure child metadata queries allocate only their parent-side facades.
        let pure_child_query = matches!(source, "('absent' in child.object) ? 0 : 42" | "Object.getOwnPropertyDescriptor(child.object, 'answer').value.value" | "('answer' in child.object) ? 42 : 0");
        if !limit_parent && !pure_child_query { assert!(failures > 0, "{setup_source}; {source}: no child allocation boundary was exercised"); }
        }
    }
}

#[cfg_attr(test, test)]
fn global_declaration_helpers_validate_the_heap_and_restore_failed_creation_roots() {
    let declarations = compile(&parse("var answer;").unwrap()).unwrap();
    let binding = &declarations.bindings[0];
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(foreign).unwrap();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)));
    let mut vm = Vm::default();
    for result in [
        vm.can_declare_global_var(foreign, "answer").map(|_| ()),
        vm.can_declare_global_function(foreign, "answer")
            .map(|_| ()),
        vm.can_declare_global_var(foreign, "undefined").map(|_| ()),
        vm.can_declare_global_function(foreign, "undefined")
            .map(|_| ()),
        vm.materialize_lexical_global(foreign, "undefined"),
        vm.create_global_binding(foreign, binding, false, true),
        vm.store_global_cell(foreign, Value::Number(42.0)),
    ] {
        assert_eq!(result, expected);
    }
    assert!(vm.stack.is_empty() && vm.global_bindings.is_empty());
    assert_eq!(owner.heap.get_own(foreign, "value").unwrap(), None);
    owner.heap.unroot(root).unwrap();

    assert_eq!(vm.global_binding_value("absent"), Ok(None));
    assert_eq!(vm.dynamic_eval_binding_value("absent"), Ok(None));
    assert_eq!(
        vm.assign_unbound_name("absent", Value::Number(42.0), true),
        Err(RuntimeError::ReferenceError("absent".into()))
    );
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let invalid_utf16 = PropertyName::String(JsString::from_code_units(vec![0xd800]));
    assert_eq!(
        vm.materialize_global_object_property(global, &invalid_utf16),
        Ok(())
    );
    assert_eq!(vm.global_property_cell(global, &invalid_utf16), None);
    let eval = crate::compiler::compile_eval(
        &parse("var recreated = 7;").unwrap(),
        &[],
        &[],
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap();
    vm.prepare_root_execution(&eval, false).unwrap();
    let slot = eval
        .bindings
        .iter()
        .position(|binding| binding.name == "recreated")
        .unwrap();
    assert!(!vm.eval_var_deleted(slot));
    let env = vm.new_parameter_eval_env().unwrap();
    let env_root = vm.heap.root(env).unwrap();
    assert_eq!(vm.delete_eval_env_var(env, "absent"), Ok(true));
    vm.heap.unroot(env_root).unwrap();
    vm.finish_root_execution(Ok(Value::Undefined)).unwrap();

    let mut root_refusal = Vm::default();
    root_refusal.string_intrinsics().unwrap();
    let root_global = root_refusal
        .global("globalThis")
        .unwrap()
        .object_id()
        .unwrap();
    root_refusal.heap.allow_root_registrations(0);
    assert_eq!(
        root_refusal.create_global_binding(root_global, binding, false, true),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(root_refusal.global_bindings.is_empty() && root_refusal.stack.is_empty());

    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap.prevent_extensions(global).unwrap();
    assert_eq!(
        vm.create_global_binding(global, binding, false, true),
        Err(RuntimeError::TypeError(
            "cannot create global binding".into()
        ))
    );
    assert!(vm.global_bindings.is_empty() && vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

impl Vm {
    fn verify_global_allocation_boundary(name: &str) {
        let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
        let mut cold = Vm::default();
        let limit = cold.heap.allow_only(0);
        assert_eq!(
            cold.global(name),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })),
            "cold {name}"
        );
        assert!(cold.stack.is_empty());
        for with_global in [false, true] {
            let mut completed = false;
            let mut next_extra = 0;
            while next_extra <= 16 * 1024 * 1024 {
                let extra = next_extra;
                let mut vm = Vm::default();
                vm.string_intrinsics().unwrap();
                if with_global {
                    vm.global("globalThis").unwrap();
                }
                vm.with_roots(|heap| {
                    heap.collect_major();
                    Ok(())
                })
                .unwrap();
                let limit = vm.heap.allow_only(extra);
                match vm.global(name) {
                    Ok(value) => {
                        assert!(value.object_id().is_some());
                        completed = true;
                    }
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                        assert_eq!(actual, limit)
                    }
                    Err(error) => panic!("{name}, {extra}: {error:?}"),
                }
                assert!(
                    vm.stack.is_empty(),
                    "{name}, {extra}: global initialization retained temporary roots"
                );
                if !completed {
                    next_extra = vm.heap.next_allocation_headroom(extra);
                }
                vm.heap.allow_only(16 * 1024 * 1024);
                if name == "Date" && !completed {
                    assert!(vm.date_prototype.is_none());
                }
                vm.with_roots(|heap| {
                    heap.collect_major();
                    Ok(())
                })
                .unwrap();
                let id = vm.global(name).unwrap_or_else(|error| panic!("retry global {name}, headroom {extra}, with global={with_global}: {error:?}")).object_id().unwrap();
                assert!(vm.heap.contains(id));
                assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
                if completed {
                    break;
                }
            }
            assert!(
                completed,
                "{name} never reached successful global initialization"
            );
        }
    }
    pub(in crate::vm) fn verify_intrinsic_allocation_boundary(
        name: &str,
        builder: fn(&mut Vm) -> Result<ObjectId, RuntimeError>,
    ) {
        let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
        let mut cold = Vm::default();
        let limit = cold.heap.allow_only(0);
        assert_eq!(
            builder(&mut cold),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })),
            "cold {name}"
        );
        assert!(
            cold.stack.is_empty(),
            "cold {name} retained temporary roots"
        );
        cold.heap.allow_only(16 * 1024 * 1024);
        let built =
            builder(&mut cold).unwrap_or_else(|error| panic!("cold retry {name}: {error:?}"));
        assert!(cold.heap.contains(built));
        assert!(cold.stack.is_empty());
        assert_eq!(cold.execute_script(&reuse), Ok(Value::Number(42.0)));

        for warm_string in [false, true] {
            let mut completed = false;
            let mut next_extra = 0;
            while next_extra <= 16 * 1024 * 1024 {
                let extra = next_extra;
                let mut vm = Vm::default();
                if warm_string {
                    vm.string_intrinsics().unwrap();
                }
                let before = vm.stack.len();
                let limit = vm.heap.allow_only(extra);
                match builder(&mut vm) {
                    Ok(id) => {
                        assert!(vm.heap.contains(id));
                        completed = true;
                    }
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                        assert_eq!(actual, limit, "String warm={warm_string}, {name}, {extra}")
                    }
                    Err(error) => panic!("String warm={warm_string}, {name}, {extra}: {error:?}"),
                }
                assert_eq!(
                    vm.stack.len(),
                    before,
                    "String warm={warm_string}, {name}, {extra}: temporary roots leaked"
                );
                if name == "Intl" && !completed {
                    assert!(
                        !vm.globals
                            .keys()
                            .any(|name| name == "Intl" || name.starts_with("%Intl.")),
                        "failed Intl construction published a partial cache"
                    );
                }
                if !completed {
                    next_extra = vm.heap.next_allocation_headroom(extra);
                }
                vm.heap.allow_only(16 * 1024 * 1024);
                vm.with_roots(|heap| {
                    heap.collect_major();
                    Ok(())
                })
                .unwrap();
                let id = builder(&mut vm)
                    .unwrap_or_else(|error| panic!("retry {name}, headroom {extra}: {error:?}"));
                assert!(vm.heap.contains(id));
                assert_eq!(vm.stack.len(), before);
                assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
                if completed {
                    break;
                }
            }
            assert!(completed, "{name} never reached successful construction");
        }
    }

    /// Runs internal boundary contracts only in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_execution_boundary_contracts() {
        paused_rest_instructions_preserve_real_array_refusals();
        compiled_eval_with_references_use_real_dynamic_environments();
        compiled_eval_deletion_keeps_cold_global_refusals();
        compiled_eval_captures_use_owned_dynamic_cells_and_preserve_outer_storage();
        staged_interpreter_name_and_reference_errors_preserve_vm_reuse();
        with_references_preserve_the_resolved_global_environment();
        compiled_source_keys_are_coerced_once_before_target_evaluation();
        cold_regexp_constructor_identity_preserves_initialization_refusal();
        existing_string_values_and_cold_this_preserve_interpreter_limits();
        collected_weak_targets_and_queued_job_refusals_preserve_vm_state();
        late_global_publication_and_eval_recreation_preserve_owned_state();
        late_name_lookup_and_assignment_preserve_real_environment_errors();
        interpreter_instruction_refusals_preserve_spread_and_rest_cleanup();
        idle_test262_agents_can_be_shutdown_after_reporting_readiness();
        collection_native_ingress_rejects_live_handles_from_another_heap();
        bounded_job_runs_preserve_cleanup_callback_errors_and_empty_queues();
        script_resource_refusals_cover_lazy_declarations_and_native_data_growth();
        cold_name_boundaries_preserve_global_allocation_refusals();
        eval_global_declaration_entry_keeps_cold_global_and_intrinsic_refusals();
        nested_script_entry_refusal_keeps_the_callers_frame_unchanged();
        lazy_intrinsic_root_refusals_preserve_existing_globals_and_roots();
        foreign_proxy_import_root_refusals_preserve_both_heaps_and_partial_facades();
        cold_iterator_entry_and_setter_refusals_leave_receivers_usable();
        template_cache_root_refusal_restores_operands_and_publishes_no_template();
        unpublished_collection_promise_and_disposal_prototypes_are_strong_vm_edges();
        cached_shadow_prototypes_survive_collection_and_refused_roots_publish_nothing();
        segmenter_prototype_root_failures_release_the_first_prototype_without_publishing();
        ordinary_call_return_checks_existing_strings_after_the_budget_changes();
        failed_shadow_wrapping_releases_child_targets_and_preserves_opaque_metadata_errors();
        global_declaration_helpers_validate_the_heap_and_restore_failed_creation_roots();
        foreign_snapshot_allocation_failures_preserve_both_realms_and_source_identity();
        intrinsic_builders_restore_roots_and_retry_after_cold_and_warm_allocation_failures();
        Self::verify_regexp_builder_boundary_contracts();
        host_callback_tags_validate_the_integer_boundary_without_allocating_a_registry();
        a_registered_host_timer_wakes_an_idle_vm_and_releases_its_callback_root();
        iterator_callbacks_reject_foreign_heap_handles_before_observing_or_closing_the_receiver();
        failed_global_declaration_preparation_clears_transient_execution_state();
        regexp_resource_boundaries_restore_roots_and_preserve_the_existing_matcher();
        failed_reverse_facade_root_registration_releases_every_unpublished_owner();
        failed_realm_creation_releases_parent_facades_and_publishes_no_child();
        iterator_helper_abrupt_paths_restore_roots_and_complete_the_helper();
        zip_padding_failures_restore_temporary_roots_and_preserve_heap_errors();
        let contracts: &[(&str, fn())] = &[
            (
                "private_element_binding_and_receiver_checks_preserve_object_identity_without_coercion",
                private_element_binding_and_receiver_checks_preserve_object_identity_without_coercion,
            ),
            (
                "iterator_getter_failures_restore_the_temporary_stack_and_finish_the_record",
                iterator_getter_failures_restore_the_temporary_stack_and_finish_the_record,
            ),
            (
                "iterator_cleanup_keeps_pending_records_and_the_first_throw_alive_across_collection",
                iterator_cleanup_keeps_pending_records_and_the_first_throw_alive_across_collection,
            ),
            (
                "iterator_metadata_and_callback_boundaries_preserve_safe_integer_limits",
                iterator_metadata_and_callback_boundaries_preserve_safe_integer_limits,
            ),
            (
                "synchronous_root_exit_protocol_preserves_value_and_rejects_suspension",
                synchronous_root_exit_protocol_preserves_value_and_rejects_suspension,
            ),
            (
                "temporal_initialization_publishes_its_cache_atomically_and_can_retry",
                temporal_initialization_publishes_its_cache_atomically_and_can_retry,
            ),
            (
                "cold_temporal_allocation_propagates_intrinsic_initialization_failure",
                cold_temporal_allocation_propagates_intrinsic_initialization_failure,
            ),
            (
                "representation_accessors_do_not_coerce_values",
                representation_accessors_do_not_coerce_values,
            ),
            (
                "legacy_with_assignment_bytecode_writes_existing_bindings_and_rejects_absent_ones",
                legacy_with_assignment_bytecode_writes_existing_bindings_and_rejects_absent_ones,
            ),
            (
                "queued_finalization_callbacks_keep_object_holdings_alive_across_collection",
                queued_finalization_callbacks_keep_object_holdings_alive_across_collection,
            ),
            (
                "temporal_receiver_boundaries_reject_a_handle_owned_by_another_heap",
                temporal_receiver_boundaries_reject_a_handle_owned_by_another_heap,
            ),
            (
                "object_internal_methods_preserve_heap_identity_and_ordinary_fallbacks",
                object_internal_methods_preserve_heap_identity_and_ordinary_fallbacks,
            ),
            (
                "iterator_and_regexp_internal_receivers_preserve_heap_identity",
                iterator_and_regexp_internal_receivers_preserve_heap_identity,
            ),
            (
                "host_installation_failures_leave_the_callback_registry_and_vm_reusable",
                host_installation_failures_leave_the_callback_registry_and_vm_reusable,
            ),
            (
                "frame_serial_exhaustion_restores_the_caller",
                frame_serial_exhaustion_restores_the_caller,
            ),
            (
                "all_completion_variants_keep_their_object_edges_alive",
                all_completion_variants_keep_their_object_edges_alive,
            ),
            (
                "deletion_removes_current_and_captured_dynamic_eval_cells",
                deletion_removes_current_and_captured_dynamic_eval_cells,
            ),
            (
                "async_generator_queue_rejects_invalid_brand_serial_and_completion_order",
                async_generator_queue_rejects_invalid_brand_serial_and_completion_order,
            ),
            (
                "foreign_facades_reject_a_released_realm_without_accessing_its_heap",
                foreign_facades_reject_a_released_realm_without_accessing_its_heap,
            ),
            (
                "captured_binding_reads_reject_foreign_cells_and_preserve_the_original_cell",
                captured_binding_reads_reject_foreign_cells_and_preserve_the_original_cell,
            ),
            (
                "shadow_ancestor_resolution_rejects_absent_and_self_contexts_before_reborrowing",
                shadow_ancestor_resolution_rejects_absent_and_self_contexts_before_reborrowing,
            ),
            (
                "shadow_wrapped_functions_reject_construction_and_busy_realms_reject_imports",
                shadow_wrapped_functions_reject_construction_and_busy_realms_reject_imports,
            ),
            (
                "shadow_child_initialization_failure_is_opaque_and_publishes_no_realm",
                shadow_child_initialization_failure_is_opaque_and_publishes_no_realm,
            ),
            (
                "root_identifier_exhaustion_propagates_without_invalidating_existing_heaps",
                root_identifier_exhaustion_propagates_without_invalidating_existing_heaps,
            ),
            (
                "reverse_definition_and_deletion_reject_an_inactive_parent",
                reverse_definition_and_deletion_reject_an_inactive_parent,
            ),
            (
                "failed_shadow_wrapper_does_not_retain_its_target",
                failed_shadow_wrapper_does_not_retain_its_target,
            ),
            (
                "allocation_failures_preserve_vm_reusability",
                allocation_failures_preserve_vm_reusability,
            ),
        ];
        let mut failures = Vec::new();
        for (name, contract) in contracts {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(contract)).is_err() {
                failures.push(*name);
            }
        }
        assert!(
            failures.is_empty(),
            "boundary contract failures: {}",
            failures.join(", ")
        );
    }
}

#[cfg_attr(test, test)]
fn compiled_eval_deletion_keeps_cold_global_refusals() {
    let parent = compile(&parse("42").unwrap()).unwrap();
    let operation = crate::compiler::compile_eval(
        &parse("var undefined = 7; delete undefined; delete undefined; 42").unwrap(),
        &[],
        &[],
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap();
    let mut extra = 0;
    let mut completed = false;
    let mut refusals = 0;
    while extra <= 16 * 1024 * 1024 {
        let mut vm = Vm::default();
        vm.prepare_root_execution(&parent, false).unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let limit = vm.heap.allow_only(extra);
        let result = vm.execute_eval(&operation, Vec::new(), false);
        match &result {
            Ok(value) => {
                assert_eq!(*value, Value::Number(42.0));
                completed = true;
            }
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                assert_eq!(*actual, limit);
                refusals += 1;
                extra = vm.heap.next_allocation_headroom(extra);
            }
            result => panic!("headroom {extra}: {result:?}"),
        }
        let _ = vm.finish_root_execution(result);
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(vm.execute_script(&parent), Ok(Value::Number(42.0)));
        if completed {
            break;
        }
    }
    assert!(completed && refusals > 0);
}

#[cfg_attr(test, test)]
fn paused_rest_instructions_preserve_real_array_refusals() {
    use crate::bytecode::Opcode;
    use crate::vm::debugger::VmDebuggerExecutionState;
    for (source, opcode) in [
        ("var [...rest] = values; rest.length === 2 && rest[1] === payload ? 42 : 0", Opcode::IteratorRest),
        ("[target.first,...target.rest] = values; target.first === payload && target.rest[0] === payload ? 42 : 0", Opcode::IteratorRestReference),
    ] {
        // Retain each next result before the budget is imposed. Collecting
        // an ephemeral iterator result must not fund the later array write
        // and hide the refusal at IteratorRest's array_push boundary.
        let setup = compile(&parse("globalThis.payload = 'x'.repeat(8192); globalThis.results = [{done:false,value:payload},{done:false,value:payload},{done:true}]; globalThis.cursor = 0; globalThis.values = {[Symbol.iterator]() {return this;}, next() {return results[cursor++];}}; globalThis.target = {};").unwrap()).unwrap();
        let operation = compile(&parse(source).unwrap()).unwrap();
        let offset = operation.instructions().find(|instruction| instruction.opcode == opcode).unwrap().offset as u32;
        let check = compile(&parse(if opcode == Opcode::IteratorRest {
            "rest.length === 2 && rest[0] === payload && rest[1] === payload ? 42 : 0"
        } else {
            "target.first === payload && target.rest.length === 1 && target.rest[0] === payload ? 42 : 0"
        }).unwrap()).unwrap();
        let mut extra = 0;
        let mut completed = false;
        let mut refusals = 0;
        while extra <= 16 * 1024 * 1024 {
            let mut vm = Vm::default();
            vm.execute_script(&setup).unwrap();
            assert_eq!(vm.execute_script_until_debugger_pause(&operation, offset), Ok(VmDebuggerExecutionState::Paused {bytecode_offset:offset}));
            vm.with_roots(|heap| {heap.collect_major(); Ok(())}).unwrap();
            let limit = vm.heap.allow_only(extra);
            match vm.resume_debugger_execution() {
                Ok(VmDebuggerExecutionState::Completed) => completed = true,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded {limit:actual})) => {assert_eq!(actual, limit); refusals += 1; extra = vm.heap.next_allocation_headroom(extra);}
                result => panic!("{source}, headroom {extra}: {result:?}"),
            }
            assert!(vm.stack.is_empty());
            vm.heap.allow_only(16 * 1024 * 1024);
            if completed {assert_eq!(vm.execute_script(&check), Ok(Value::Number(42.0)));}
            assert_eq!(vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()), Ok(Value::Number(42.0)));
            if completed {break;}
        }
        assert!(completed && refusals > 0, "{source}");
    }
}

#[cfg_attr(test, test)]
fn compiled_eval_with_references_use_real_dynamic_environments() {
    use crate::bytecode::Opcode;
    let eval_code = crate::compiler::compile_eval(
        &parse("var dynamicValue = 40;").unwrap(),
        &[],
        &[],
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap();
    for nursery_capacity in [1, HeapConfig::default().nursery_capacity] {
        for source in [
            "with ({}) {dynamicValue += 2;}",
            "function update() {with ({}) {dynamicValue += 2;} return 42;} update();",
        ] {
            let parent = compile(&parse(source).unwrap()).unwrap();
            let mut vm = Vm::new(VmConfig {
                heap: HeapConfig {
                    nursery_capacity,
                    ..HeapConfig::default()
                },
                ..VmConfig::default()
            })
            .unwrap();
            vm.prepare_root_execution(&parent, false).unwrap();
            vm.prepare_global_declarations(&parent).unwrap();
            // The compiled-eval ingress creates an actual owned dynamic cell
            // that is absent from the previously compiled parent's slots.
            vm.execute_eval(&eval_code, Vec::new(), false).unwrap();
            assert_eq!(
                vm.dynamic_eval_binding_value("dynamicValue"),
                Ok(Some(Value::Number(40.0)))
            );
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            let result = vm.run(&parent);
            assert_eq!(result, Ok(Value::Number(42.0)), "{source}");
            assert_eq!(
                vm.dynamic_eval_binding_value("dynamicValue"),
                Ok(Some(Value::Number(42.0)))
            );
            vm.finish_root_execution(result).unwrap();
            assert!(vm.stack.is_empty());
        }

        // Suspend at a real compiled GetValue boundary and consume the
        // producer's Reference pair, without constructing bytecode or cells.
        let parent = compile(&parse("with ({}) {dynamicValue += 2;}").unwrap()).unwrap();
        let offset = parent
            .instructions()
            .find(|instruction| instruction.opcode == Opcode::LoadWithReference)
            .unwrap()
            .offset;
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        vm.prepare_root_execution(&parent, false).unwrap();
        vm.prepare_global_declarations(&parent).unwrap();
        vm.execute_eval(&eval_code, Vec::new(), false).unwrap();
        let mut iterators = Vec::new();
        let InterpreterExit::Suspend {
            pc,
            mut iterators,
            handlers,
        } = vm
            .interpret(
                &parent,
                &mut iterators,
                0,
                None,
                Some(InterpreterSuspensionPoint::Offset(offset)),
                None,
            )
            .unwrap()
        else {
            panic!("compiled GetValue boundary did not suspend")
        };
        assert_eq!(pc, offset);
        let target = vm.stack[vm.stack.len() - 2].clone();
        let marker = vm.stack[vm.stack.len() - 1].clone();
        assert_eq!(target, Value::Bool(false));
        assert_eq!(
            vm.load_with_reference(&parent, &target, &marker),
            Ok(Value::Number(40.0))
        );
        let before = vm.stack.len();
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.store_with_reference(
                &parent,
                target.clone(),
                marker.clone(),
                &Value::String("x".repeat(8192).into())
            ),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert_eq!(vm.stack.len(), before);
        assert_eq!(
            vm.dynamic_eval_binding_value("dynamicValue"),
            Ok(Some(Value::Number(40.0)))
        );
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(vm.delete_dynamic_eval_binding("dynamicValue"), Ok(true));
        assert_eq!(
            vm.load_with_reference(&parent, &target, &marker),
            Err(RuntimeError::ReferenceError("dynamicValue".into()))
        );
        let before = vm.stack.len();
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.store_with_reference(
                &parent,
                target.clone(),
                marker.clone(),
                &Value::String("x".repeat(8192).into())
            ),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert_eq!(vm.stack.len(), before);
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.store_with_reference(&parent, target, marker, &Value::Number(42.0))
            .unwrap();
        let error = match vm.interpret(&parent, &mut iterators, pc, None, None, Some((handlers, 0)))
        {
            Err(error) => error,
            Ok(_) => panic!("the deleted compiled Reference unexpectedly resumed normally"),
        };
        assert_eq!(error, RuntimeError::ReferenceError("dynamicValue".into()));
        vm.finish_root_execution(Err(error)).unwrap_err();
        assert!(vm.stack.is_empty());
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
    }
}

#[cfg_attr(test, test)]
fn compiled_eval_captures_use_owned_dynamic_cells_and_preserve_outer_storage() {
    let parent = compile(&parse("var captured; if (globalThis.phase === 1) {captured += 2; var before = captured++; captured--; before === 15 && captured === 15 ? 42 : 0;} else {captured = 7; captured;}").unwrap()).unwrap();
    let slot = parent
        .bindings
        .iter()
        .position(|binding| binding.name == "captured")
        .unwrap();
    let mut vm = Vm::default();
    vm.prepare_root_execution(&parent, false).unwrap();
    vm.prepare_global_declarations(&parent).unwrap();
    vm.run(&parent).unwrap();
    let original = vm.capture(slot).unwrap();
    let original_root = vm.heap.root(original).unwrap();
    let visible = [(
        "captured".to_string(),
        parent.bindings[slot].clone(),
        slot as u32,
    )];
    // The internal compiled-eval boundary receives actual owned captures.
    // A fresh variable environment owns its new cell; the supplied capture
    // remains the original environment's storage throughout reference updates.
    let code = crate::compiler::compile_eval(
        &parse("var captured = 11; captured += 2; var previous = captured++; captured--; previous === 13 && captured === 13 ? 42 : 0").unwrap(),
        &visible, &[], &[], crate::compiler::EvalContext::default(), crate::CompileLimits::default(),
    ).unwrap();
    assert_eq!(
        vm.execute_eval(&code, vec![original], false),
        Ok(Value::Number(42.0))
    );
    let recreated = crate::compiler::compile_eval(
        &parse("var recreated = (delete recreated, 42); recreated").unwrap(),
        &[],
        &[],
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap();
    assert!(vm.parameter_eval_env.is_none());
    assert_eq!(
        vm.execute_eval(&recreated, Vec::new(), false),
        Ok(Value::Number(42.0))
    );
    assert_eq!(
        vm.heap.get_own(original, "value").unwrap(),
        Some(Value::Number(7.0))
    );
    assert_eq!(
        vm.eval_aware_binding_value(slot, "captured"),
        Ok(Some(Value::Number(13.0)))
    );
    // These are the real captures and aliases prepared by execute_eval above.
    // Resolve subsequent assignments through the new cell, while references
    // resolved before eval continue to retain the original cell.
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.assign_unbound_name("captured", Value::String("x".repeat(4096).into()), false),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    for (source, captures, exposed) in [
        (
            "captured += 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; 42",
            vec![original],
            visible.to_vec(),
        ),
        (
            "captured = 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; 42",
            Vec::new(),
            Vec::new(),
        ),
    ] {
        let operation = crate::compiler::compile_eval(
            &parse(source).unwrap(),
            &exposed,
            &[],
            &[],
            crate::compiler::EvalContext::default(),
            crate::CompileLimits::default(),
        )
        .unwrap();
        assert_eq!(
            vm.execute_eval(&operation, captures, false),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })),
            "{source}"
        );
        assert_eq!(
            vm.heap.get_own(original, "value").unwrap(),
            Some(Value::Number(7.0))
        );
        assert!(vm.stack.is_empty());
    }
    assert_eq!(
        vm.assign_binding_slot(&parent, slot, Value::String("x".repeat(4096).into()), false),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(
        vm.eval_aware_binding_value(slot, "captured"),
        Ok(Some(Value::Number(13.0)))
    );
    assert_eq!(
        vm.heap.get_own(original, "value").unwrap(),
        Some(Value::Number(7.0))
    );
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.assign_binding_slot(&parent, slot, Value::Number(13.0), false)
        .unwrap();
    let global = vm.global("globalThis").unwrap();
    vm.set_property(&global, &"phase".into(), &Value::Number(1.0))
        .unwrap();
    assert_eq!(vm.run(&parent), Ok(Value::Number(42.0)));
    assert_eq!(
        vm.eval_aware_binding_value(slot, "captured"),
        Ok(Some(Value::Number(15.0)))
    );
    assert_eq!(
        vm.heap.get_own(original, "value").unwrap(),
        Some(Value::Number(7.0))
    );
    assert!(vm.stack.is_empty());
    vm.finish_root_execution(Ok(Value::Undefined)).unwrap();
    assert_eq!(
        vm.heap.get_own(original, "value").unwrap(),
        Some(Value::Number(7.0))
    );
    vm.heap.unroot(original_root).unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn existing_string_values_and_cold_this_preserve_interpreter_limits() {
    let script = |source| compile(&parse(source).unwrap()).unwrap();
    let mut vm = Vm::default();
    vm.execute_script(&script("globalThis.p = 'oversized'; globalThis.o = {p}; globalThis.f = () => p; globalThis.e = () => eval('p'); class B {} B.prototype.p = p; class C extends B {f(){return super.p;}} globalThis.c = new C();")).unwrap();
    vm.config.max_string_bytes = 2;
    for source in ["o.p", "f()", "c.f()", "eval('p')", "e()"] {
        assert_eq!(
            vm.execute_script(&script(source)),
            Err(RuntimeError::StringLimit { limit: 2 }),
            "{source}"
        );
        assert!(vm.stack.is_empty());
    }
    vm.config.max_string_bytes = VmConfig::default().max_string_bytes;
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );

    let mut vm = Vm::default();
    let function = vm
        .execute_script(&script("(function(){return this;})"))
        .unwrap();
    let root = vm.heap.root(function.object_id().unwrap()).unwrap();
    assert!(!vm.globals.contains_key("globalThis"));
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.call_native(function.clone(), Value::Undefined, vec![], false),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    assert!(vm
        .call_native(function, Value::Undefined, vec![], false)
        .unwrap()
        .object_id()
        .is_some());
    vm.heap.unroot(root).unwrap();
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn collection_native_ingress_rejects_live_handles_from_another_heap() {
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let receiver = Value::Object(object);
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(object)));
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    for result in [
        vm.map_method(native::MapMethod::Get, &receiver, &[]),
        vm.set_method(native::SetMethod::Has, &receiver, &[]),
        vm.weak_collection_method(true, native::WeakCollectionMethod::Get, &receiver, &[]),
        vm.finalization_registry_constructor(receiver.clone(), true),
        vm.finalization_registry_register(&receiver, &[]),
        vm.finalization_registry_unregister(&receiver, Value::Undefined),
        vm.proxy_constructor(
            &[receiver.clone(), Value::Object(vm.object_prototype)],
            true,
        ),
        vm.proxy_revocable(&[receiver.clone(), Value::Object(vm.object_prototype)]),
    ] {
        assert_eq!(result, expected);
    }
    vm.new_target = vm.global("Object").unwrap();
    let map = vm.collection_constructor(true, &[], true).unwrap();
    let map_root = vm.heap.root(map.object_id().unwrap()).unwrap();
    assert_eq!(
        vm.map_method(
            native::MapMethod::GetOrInsertComputed,
            &map,
            &[Value::Number(7.0), receiver.clone()]
        ),
        expected
    );
    let weak_map = vm.weak_collection_constructor(true, &[], true).unwrap();
    let weak_root = vm.heap.root(weak_map.object_id().unwrap()).unwrap();
    let key = vm.heap.alloc_object(None).unwrap();
    let key_root = vm.heap.root(key).unwrap();
    assert_eq!(
        vm.weak_collection_method(
            true,
            native::WeakCollectionMethod::GetOrInsertComputed,
            &weak_map,
            &[Value::Object(key), receiver]
        ),
        expected
    );
    assert!(vm.stack.is_empty());
    owner.heap.unroot(root).unwrap();
    vm.heap.unroot(key_root).unwrap();
    vm.heap.unroot(weak_root).unwrap();
    vm.heap.unroot(map_root).unwrap();
}

#[cfg_attr(test, test)]
fn collected_weak_targets_and_queued_job_refusals_preserve_vm_state() {
    let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
    let mut vm = Vm::default();
    let weak = vm
        .execute_script(
            &compile(
                &parse("globalThis.weakTarget = {answer:42}; new WeakRef(weakTarget)").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let root = vm.heap.root(weak.object_id().unwrap()).unwrap();
    assert!(vm.weak_ref_deref(&weak).unwrap().object_id().is_some());
    assert_eq!(
        vm.execute_script(&compile(&parse("delete globalThis.weakTarget; 42").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(vm.weak_ref_deref(&weak), Ok(Value::Undefined));
    vm.heap.unroot(root).unwrap();

    for source in [
        "globalThis.next = Promise.resolve(7).then(() => next);",
        "Promise.resolve({then(resolve,reject){throw 7;}});",
        "class Capability {constructor(executor){executor(()=>{},()=>{new Array(100);});}} var promise = Promise.resolve(7); promise.constructor = {[Symbol.species]:Capability}; promise.then(()=>{throw 7;});",
    ] {
        let setup = compile(&parse(source).unwrap()).unwrap();
        let mut extra = 0;
        let mut complete = false;
        let mut refused = false;
        while extra <= 16 * 1024 * 1024 {
            let mut vm = Vm::default();
            vm.execute_script(&setup).unwrap();
            let limit = vm.heap.allow_only(extra);
            match vm.run_promise_jobs() {
                Ok(()) => complete = true,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded {limit: actual})) => {
                    assert_eq!(actual, limit, "{source}, {extra}");
                    refused = true;
                    extra = vm.heap.next_allocation_headroom(extra);
                }
                result => panic!("{source}, {extra}: {result:?}"),
            }
            assert!(vm.stack.is_empty(), "{source}, {extra}");
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if complete {break;}
        }
        assert!(complete && refused, "{source}");
    }
    let setup =
        compile(&parse("Promise.resolve({then(resolve,reject){throw 7;}});").unwrap()).unwrap();
    for instructions in 0..=32 {
        let mut vm = Vm::default();
        vm.execute_script(&setup).unwrap();
        vm.config.instruction_budget = instructions;
        assert!(matches!(
            vm.run_promise_jobs(),
            Ok(()) | Err(RuntimeError::InstructionLimit)
        ));
        assert!(vm.stack.is_empty());
        vm.config.instruction_budget = VmConfig::default().instruction_budget;
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn bounded_job_runs_preserve_cleanup_callback_errors_and_empty_queues() {
    let mut vm = Vm::default();
    assert_eq!(vm.run_promise_jobs_bounded(0), Ok(()));
    assert_eq!(vm.run_promise_jobs_bounded(1), Ok(()));
    vm.execute_script(&compile(&parse(
        "globalThis.registry = new FinalizationRegistry(() => {throw 7;}); registry.register({}, 42);"
    ).unwrap()).unwrap()).unwrap();
    assert!(vm
        .promise_jobs
        .iter()
        .any(|job| matches!(job, PromiseJob::FinalizationCleanup { .. })));
    // Collect while the actual cleanup job is still queued. Its callable is
    // retained, and its primitive holdings must not be treated as an object.
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.run_promise_jobs_bounded(1),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(vm.stack.is_empty());
    assert_eq!(vm.run_promise_jobs_bounded(1), Ok(()));
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn script_resource_refusals_cover_lazy_declarations_and_native_data_growth() {
    for (setup, source) in [
        ("String; globalThis;", "let Array = 42; Array"),
        ("String; globalThis;", "function Array() {return 42;} Array()"),
        ("String; globalThis;", "var Array; 42"),
        ("String; globalThis;", "delete Iterator; 42"),
        ("String; globalThis;", "(function(){return this;})() === globalThis ? 42 : 0"),
        ("String; globalThis;", "with({}) { typeof Iterator; } 42"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "eval('var item = 7; with({}) {item = payload;} item.length === 4096 ? 42 : 0')"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "(function(){eval('var item = 7'); with({}) {item = payload;} return item.length === 4096 ? 42 : 0;})()"),
        ("Object; globalThis.target = {answer:42};", "Object.getOwnPropertyDescriptors(target).answer.value"),
        ("Object; globalThis.target = {}; globalThis.descriptors = {answer:{value:42}};", "Object.defineProperties(target, descriptors).answer"),
        ("Object; globalThis.descriptors = {answer:{value:42}};", "Object.create(null, descriptors).answer"),
        ("Object; globalThis.source = {answer:42};", "Object.entries(source)[0][1]"),
        ("SuppressedError; globalThis.message = 'x'.repeat(4096);", "new SuppressedError(7, 9, message).message.length === 4096 ? 42 : 0"),
        ("AggregateError; globalThis.errors = [7, 9];", "new AggregateError(errors).errors[1] === 9 ? 42 : 0"),
        ("AggregateError; globalThis.payload = 'x'.repeat(4096); globalThis.errors = [payload];", "new AggregateError(errors).errors[0].length === 4096 ? 42 : 0"),
        ("Map; globalThis.map = new Map(); globalThis.payload = 'x'.repeat(4096);", "map.set('answer', payload).get('answer').length === 4096 ? 42 : 0"),
        ("Map; globalThis.map = new Map(); globalThis.payload = 'x'.repeat(4096);", "map.getOrInsert('answer', payload).length === 4096 ? 42 : 0"),
        ("Map; globalThis.map = new Map(); globalThis.payload = 'x'.repeat(4096);", "map.getOrInsertComputed('answer', () => payload).length === 4096 ? 42 : 0"),
        ("Set; globalThis.set = new Set(); globalThis.payload = 'x'.repeat(4096);", "set.add(payload).size === 1 ? 42 : 0"),
        ("WeakMap; globalThis.map = new WeakMap(); globalThis.key = {}; globalThis.payload = 'x'.repeat(4096);", "map.set(key, payload).get(key).length === 4096 ? 42 : 0"),
        ("WeakMap; globalThis.map = new WeakMap(); globalThis.key = {}; globalThis.payload = 'x'.repeat(4096);", "map.getOrInsert(key, payload).length === 4096 ? 42 : 0"),
        ("WeakMap; globalThis.map = new WeakMap(); globalThis.key = {}; globalThis.payload = 'x'.repeat(4096);", "map.getOrInsertComputed(key, () => payload).length === 4096 ? 42 : 0"),
        ("FinalizationRegistry; globalThis.registry = new FinalizationRegistry(() => {}); globalThis.target = {}; globalThis.token = {}; globalThis.holdings = 'x'.repeat(4096);", "registry.register(target, holdings, token); registry.unregister(token) ? 42 : 0"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "eval('var transient = 1; transient = payload; transient.length === 4096 ? 42 : 0')"),
        ("globalThis.payload = 'x'.repeat(4096); globalThis.value = 7;", "value = payload; value.length === 4096 ? 42 : 0"),
        ("globalThis.payload = 'x'.repeat(4096); var value = 7;", "value = payload; value.length === 4096 ? 42 : 0"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "(function() {eval('var dynamicOnly = 7'); dynamicOnly = payload; return dynamicOnly.length === 4096 ? 42 : 0;})()"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "(function() {var captured = 7; function inner() {eval('var captured = 11'); captured = payload; return captured.length === 4096 ? 42 : 0;} return inner();})()"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "eval('var recreated = (delete recreated, payload); recreated.length === 4096 ? 42 : 0')"),
        ("Object; globalThis; eval;", "eval('var replaced = (delete replaced, Object.defineProperty(globalThis, \"replaced\", {value:7, writable:false, configurable:false}), 42); replaced === 7 ? 42 : 0')"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "(function() {return eval('var recreated = (delete recreated, payload); recreated.length === 4096 ? 42 : 0');})()"),
        ("globalThis; eval; globalThis.payload = 'x'.repeat(4096);", "(function(value = eval('var dynamic = 1')) {eval('var dynamic = (delete dynamic, payload)'); return dynamic.length === 4096 ? 42 : 0;})()"),
        ("globalThis;", "(function() {for (let value = 0; value < 1; value++) {(() => value)();} return 42;})()"),
        ("globalThis.payload = 'x'.repeat(4096);", "(function() {for (let value = payload, i = 0; i < 1; i++) {(() => value)();} return 42;})()"),
        ("globalThis.values = [7,42];", "var [first,...rest] = values; rest[0]"),
        ("globalThis.values = [7,42]; globalThis.target = {};", "[target.first,...target.rest] = values; target.rest[0]"),
        ("globalThis.payload = 'x'.repeat(4096); globalThis.values = [7,payload];", "var [first,...rest] = values; rest[0].length === 4096 ? 42 : 0"),
        ("globalThis.payload = 'x'.repeat(4096); globalThis.values = [7,payload]; globalThis.target = {};", "[target.first,...target.rest] = values; target.rest[0].length === 4096 ? 42 : 0"),
        ("Reflect; globalThis.payload = 'x'.repeat(4096); globalThis.proxy = new Proxy({}, {defineProperty(target, key, descriptor) {return descriptor.value.length === 4096;}});", "Reflect.defineProperty(proxy, 'answer', {value:payload}) ? 42 : 0"),
        ("globalThis.payload = 'x'.repeat(4096); globalThis.proxy = new Proxy(function(){}, {apply(target, receiver, args) {return args[0].length === 4096 ? 42 : 0;}});", "proxy(payload)"),
        ("globalThis.payload = 'x'.repeat(4096); globalThis.proxy = new Proxy(function(){}, {construct(target, args) {return {answer:args[0].length === 4096 ? 42 : 0};}});", "new proxy(payload).answer"),
        ("Function; Object; globalThis.payload = 'x'.repeat(4096);", "new (class {#value = payload; value() {return this.#value;}})().value().length === 4096 ? 42 : 0"),
        ("Function; Object;", "class C {static #method() {return 42;} static value() {return this.#method();}} C.value()"),
        ("Function; Object;", "class C {static get #value() {return 42;} static read() {return this.#value;}} C.read()"),
        ("Function; Object;", "class C {static set #value(value) {} static write() {this.#value = 42; return 42;}} C.write()"),
        ("Function; Object;", "({get answer() {return 42;}}).answer"),
        ("Function; Object; globalThis.holder = {value() {return () => 42;}};", "holder.value()()"),
    ] {
        Vm::verify_script_allocation_boundary(setup, source);
    }
}

#[cfg_attr(test, test)]
fn cold_name_boundaries_preserve_global_allocation_refusals() {
    type Operation = fn(&mut Vm) -> Result<(), RuntimeError>;
    for operation in [
        (|vm: &mut Vm| vm.assign_unbound_name("absent", Value::Number(42.0), false)) as Operation,
        |vm| vm.unbound_name_resolves("absent").map(|_| ()),
        |vm| vm.delete_unbound_name("absent").map(|_| ()),
    ] {
        let mut vm = Vm::default();
        vm.string_intrinsics().unwrap();
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            operation(&mut vm),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
    }
}

#[cfg_attr(test, test)]
fn eval_global_declaration_entry_keeps_cold_global_and_intrinsic_refusals() {
    for source in ["var Array;", "function Array() {}"] {
        let eval = crate::compiler::compile_eval(
            &parse(source).unwrap(),
            &[],
            &[],
            &[],
            crate::compiler::EvalContext::default(),
            crate::CompileLimits::default(),
        )
        .unwrap();
        for warm_global in [false, true] {
            let mut vm = Vm::default();
            vm.string_intrinsics().unwrap();
            if warm_global {
                vm.global("globalThis").unwrap();
            }
            vm.prepare_root_execution(&eval, false).unwrap();
            let limit = vm.heap.allow_only(0);
            assert_eq!(
                vm.prepare_eval_global_var_declarations(&eval),
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })),
                "{source}, warm={warm_global}"
            );
            assert!(vm.stack.is_empty());
            vm.heap.allow_only(16 * 1024 * 1024);
            vm.finish_root_execution(Ok(Value::Undefined)).unwrap();
            assert_eq!(
                vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
        }
    }
}

impl Vm {
    pub(in crate::vm) fn verify_script_allocation_boundary(setup: &str, source: &str) {
        let setup = compile(&parse(setup).unwrap()).unwrap();
        let operation = compile(&parse(source).unwrap()).unwrap();
        let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
        let mut next_extra = 0;
        let mut completed = false;
        let mut refused = 0;
        while next_extra <= 16 * 1024 * 1024 {
            let extra = next_extra;
            let mut vm = Vm::default();
            vm.execute_script(&setup).unwrap();
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            let limit = vm.heap.allow_only(extra);
            match vm.execute_script(&operation) {
                Ok(value) => {
                    assert_eq!(value, Value::Number(42.0), "{source}, headroom {extra}");
                    completed = true;
                }
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                    assert_eq!(actual, limit);
                    refused += 1;
                }
                Err(error) => panic!("{source}, headroom {extra}: {error:?}"),
            }
            assert!(vm.stack.is_empty(), "{source}, headroom {extra}");
            if !completed {
                next_extra = vm.heap.next_allocation_headroom(extra);
            }
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if completed {
                break;
            }
        }
        assert!(
            completed && refused > 0,
            "{source} did not exercise a refused allocation"
        );
    }
}

#[cfg_attr(test, test)]
fn nested_script_entry_refusal_keeps_the_callers_frame_unchanged() {
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    let code = compile(&parse("21 + 21").unwrap()).unwrap();
    let object = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(object).unwrap();
    vm.stack.push(Value::Object(object));
    vm.bindings = vec![Some(Value::Number(7.0))];
    vm.completion = Value::Number(13.0);
    vm.this = Value::Object(object);
    vm.arguments = vec![Value::Number(9.0)];
    vm.strict = true;
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.execute_nested_script(&code),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.stack, [Value::Object(object)]);
    assert_eq!(vm.bindings, [Some(Value::Number(7.0))]);
    assert_eq!(vm.completion, Value::Number(13.0));
    assert_eq!(vm.this, Value::Object(object));
    assert_eq!(vm.arguments, [Value::Number(9.0)]);
    assert!(vm.strict);
    vm.stack.clear();
    vm.bindings.clear();
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(vm.execute_script(&code), Ok(Value::Number(42.0)));
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn foreign_proxy_import_root_refusals_preserve_both_heaps_and_partial_facades() {
    let script = |source| compile(&parse(source).unwrap()).unwrap();
    let reuse = script("21 + 21");
    for proxy in [false, true] {
        for limit_parent in [false, true] {
            let mut completed = false;
            let mut failures = 0;
            for registrations in 0..16 {
                let mut vm = Vm::default();
                vm.install_test262_harness().unwrap();
                let global = vm
                    .execute_script(&script("$262.createRealm().global"))
                    .unwrap()
                    .object_id()
                    .unwrap();
                let realm = vm.test262_foreign_reference(global).unwrap().0;
                let source = if proxy {
                    "globalThis.retained = new Proxy({answer:42}, {}); retained"
                } else {
                    "globalThis.retained = {answer:42}; retained"
                };
                let target = vm
                    .test262_realms
                    .get_mut(&realm)
                    .unwrap()
                    .vm
                    .execute_script(&script(source))
                    .unwrap();
                if limit_parent {
                    vm.heap.allow_root_registrations(registrations);
                } else {
                    vm.test262_realms
                        .get_mut(&realm)
                        .unwrap()
                        .vm
                        .heap
                        .allow_root_registrations(registrations);
                }
                match vm.test262_import_foreign_value(realm, target.clone()) {
                    Ok(imported) => {
                        assert!(imported.object_id().is_some());
                        completed = true;
                    }
                    Err(RuntimeError::Heap(HeapError::IdExhausted)) => failures += 1,
                    Err(error) => panic!(
                        "proxy={proxy}, parent={limit_parent}, roots={registrations}: {error:?}"
                    ),
                }
                assert!(vm.stack.is_empty());
                let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
                assert!(child.stack.is_empty() && child.heap.contains(target.object_id().unwrap()));
                assert_eq!(child.execute_script(&reuse), Ok(Value::Number(42.0)));
                assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
                if completed {
                    break;
                }
            }
            assert!(
                completed && failures > 0,
                "proxy={proxy}, parent={limit_parent}"
            );
        }
    }
}

#[cfg_attr(test, test)]
fn cold_iterator_entry_and_setter_refusals_leave_receivers_usable() {
    type Operation = fn(&mut Vm, &Value) -> Result<Value, RuntimeError>;
    let operations: &[Operation] = &[
        |vm, receiver| vm.iterator_from(receiver),
        |vm, receiver| vm.iterator_to_string_tag_setter(receiver, &Value::String("custom".into())),
        |vm, receiver| vm.iterator_constructor_setter(receiver, &Value::Number(42.0)),
    ];
    for operation in operations {
        let mut vm = Vm::default();
        let receiver = vm
            .execute_script(&compile(&parse("({next() {return {done:true};}})").unwrap()).unwrap())
            .unwrap();
        let root = vm.heap.root(receiver.object_id().unwrap()).unwrap();
        assert!(vm.iterator_base.is_none());
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            operation(&mut vm, &receiver),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert!(operation(&mut vm, &receiver).is_ok());
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        vm.heap.unroot(root).unwrap();
    }
    let mut vm = Vm::default();
    let setter = vm
        .execute_script(
            &compile(
                &parse(
                    "Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag).set",
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let receiver = vm.execute_script(&compile(&parse("Object.defineProperty({}, Symbol.toStringTag, {value:'locked', writable:false})").unwrap()).unwrap()).unwrap();
    let receiver_root = vm.heap.root(receiver.object_id().unwrap()).unwrap();
    assert!(matches!(
        vm.enter_call(
            setter,
            receiver,
            vec![Value::String("custom".into())],
            false,
            Value::Undefined
        ),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(
        !vm.strict,
        "a failed native setter restores sloppy execution"
    );
    vm.heap.unroot(receiver_root).unwrap();
    let receiver = vm
        .execute_script(
            &compile(
                &parse("Object.defineProperty({}, 'constructor', {value:7, writable:false})")
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let receiver_root = vm.heap.root(receiver.object_id().unwrap()).unwrap();
    for strict in [false, true] {
        vm.strict = strict;
        assert!(matches!(
            vm.iterator_constructor_setter(&receiver, &Value::Number(42.0)),
            Err(RuntimeError::TypeError(_))
        ));
        assert_eq!(vm.strict, strict);
        assert!(matches!(
            vm.get_property(&receiver, &"constructor".into()),
            Ok(Value::Number(7.0))
        ));
    }
    vm.heap.unroot(receiver_root).unwrap();
}

#[cfg_attr(test, test)]
fn interpreter_instruction_refusals_preserve_spread_and_rest_cleanup() {
    for (setup, source) in [
        ("globalThis.values = [7,42]; globalThis.call = (...args) => args[1];", "call(...values)"),
        ("eval; globalThis.args = ['21 + 21'];", "eval(...args)"),
        ("globalThis.values = [7,42];", "var [first,...rest] = values; rest[0]"),
        ("globalThis.values = [7,42]; globalThis.target = {};", "[target.first,...target.rest] = values; target.rest[0]"),
        ("globalThis.values = [7,42]; class Base {constructor(first,last) {this.answer = last;}} globalThis.Derived = class extends Base {constructor(...args) {super(...args);}};", "new Derived(...values).answer"),
        ("globalThis.iterable = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}, return() {for (let i = 0; i < 8; i++) {} return {done:true};}}; globalThis.run = function() {for (const value of iterable) {return 42;}};", "run()"),
        ("globalThis.iterable = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}, return() {for (let i = 0; i < 8; i++) {} return {done:true};}}; globalThis.run = function() {try {for (const value of iterable) {return 42;}} finally {}};", "run()"),
        ("globalThis.iterable = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}, return() {throw 9;}}; globalThis.run = function() {try {for (const value of iterable) {throw 7;}} finally {}};", "try {run();} catch (error) {error === 7 ? 42 : 0;}"),
    ] {
        let setup = compile(&parse(setup).unwrap()).unwrap();
        let operation = compile(&parse(source).unwrap()).unwrap();
        let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
        let mut completed = false;
        for budget in 0..=4096 {
            let mut vm = Vm::default();
            vm.execute_script(&setup).unwrap();
            vm.config.instruction_budget = budget;
            match vm.execute_script(&operation) {
                Ok(value) => {assert_eq!(value, Value::Number(42.0)); completed = true;},
                Err(RuntimeError::InstructionLimit) => {},
                Err(error) => panic!("{source}, instruction budget {budget}: {error:?}"),
            }
            assert!(vm.stack.is_empty());
            vm.config.instruction_budget = VmConfig::default().instruction_budget;
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if completed {break;}
        }
        assert!(completed, "{source} did not finish within the fixture's instruction bound");
    }
}

#[cfg_attr(test, test)]
fn idle_test262_agents_can_be_shutdown_after_reporting_readiness() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.execute_script(
        &compile(&parse("$262.agent.start(\"$262.agent.report('ready')\");").unwrap()).unwrap(),
    )
    .unwrap();
    let report = compile(&parse("$262.agent.getReport()").unwrap()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match vm.execute_script(&report).unwrap() {
            Value::Null => {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            value => {
                assert_eq!(value, Value::String("ready".into()));
                break;
            }
        }
    }
    // Readiness precedes the idle loop; allow the independent agent to yield
    // to the scheduler before shutting down the host and joining its thread.
    std::thread::sleep(std::time::Duration::from_millis(50));
    vm.shutdown_test262_agents().unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn late_global_publication_and_eval_recreation_preserve_owned_state() {
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    assert!(!vm.globals.contains_key("String"));
    vm.heap
        .define_own_property(
            global,
            "String",
            PropertyDescriptor::data(Value::Number(7.0), false, false, false),
        )
        .unwrap();
    assert!(matches!(
        vm.global("String"),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(!vm.globals.contains_key("String"));
    assert_eq!(
        vm.heap.get_own(global, "String").unwrap(),
        Some(Value::Number(7.0))
    );
    let eval = crate::compiler::compile_eval(
        &parse("var recreated = 7;").unwrap(),
        &[],
        &[],
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap();
    let mut vm = Vm::default();
    vm.prepare_root_execution(&eval, false).unwrap();
    vm.prepare_eval_global_var_declarations(&eval).unwrap();
    assert_eq!(vm.run(&eval), Ok(Value::Undefined));
    let slot = eval
        .bindings
        .iter()
        .position(|binding| binding.name == "recreated")
        .unwrap();
    assert_eq!(vm.delete_eval_var(slot), Ok(true));
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.assign_binding_slot(&eval, slot, Value::Number(42.0), true),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(!vm.global_bindings.contains_key("recreated"));
    assert!(vm.stack.is_empty());
    vm.finish_root_execution(Ok(Value::Undefined)).unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn late_name_lookup_and_assignment_preserve_real_environment_errors() {
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let foreign_root = owner.heap.root(foreign).unwrap();
    let mut vm = Vm::default();
    assert_eq!(
        vm.delete_eval_env_var(foreign, "absent"),
        Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
    );
    owner.heap.unroot(foreign_root).unwrap();
    for operation in 0..3 {
        let mut vm = Vm::default();
        vm.global("globalThis").unwrap();
        let limit = vm.heap.allow_only(0);
        let expected = Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }));
        let result = match operation {
            0 => vm.assign_unbound_name("undefined", Value::Number(42.0), true),
            1 => vm.delete_unbound_name("undefined").map(|_| ()),
            _ => vm.unbound_name_resolves("undefined").map(|_| ()),
        };
        assert_eq!(result, expected);
        assert!(vm.stack.is_empty());
    }
    let mut vm = Vm::default();
    vm.execute_script(
        &compile(
            &parse("Object.setPrototypeOf(globalThis, new Proxy({}, {has(){throw 7;}}));").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.assign_unbound_name("missing", Value::Number(42.0), true),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    let code = compile(&parse("const pending = 42;").unwrap()).unwrap();
    let mut vm = Vm::default();
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    assert_eq!(
        vm.assign_unbound_name("pending", Value::Number(7.0), true),
        Err(RuntimeError::ReferenceError("pending".into()))
    );
    vm.finish_root_execution(Ok(Value::Undefined)).unwrap();
}

#[cfg_attr(test, test)]
fn staged_interpreter_name_and_reference_errors_preserve_vm_reuse() {
    let compile_script = |source: &str| compile(&parse(source).unwrap()).unwrap();
    let mut failures = Vec::new();
    fn record_case(failures: &mut Vec<String>, label: &str, operation: impl FnOnce()) {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("non-string assertion payload");
            failures.push(format!("{label}: {message}"));
        }
    }
    let code = compile_script("const pending = 42;");
    let mut vm = Vm::default();
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    let slot = code
        .bindings
        .iter()
        .position(|binding| binding.name == "pending")
        .unwrap();
    assert_eq!(
        vm.assign_binding_slot(&code, slot, Value::Number(7.0), false),
        Err(RuntimeError::ReferenceError("pending".into()))
    );
    vm.finish_root_execution(Ok(Value::Undefined)).unwrap();
    assert!(vm.stack.is_empty());
    for (source, warm_global) in [
        ("this", false),
        ("missing", false),
        ("missing = 42", false),
        ("NaN = 42", true),
        ("'use strict'; undefined = 42", true),
        ("with (env) {Array = 42;}", true),
    ] {
        record_case(&mut failures, source, || {
            let mut vm = Vm::default();
            vm.string_intrinsics().unwrap();
            if warm_global {
                vm.global("globalThis").unwrap();
                vm.execute_script(&compile_script("globalThis.env = {};"))
                    .unwrap();
            }
            let code = compile_script(source);
            vm.prepare_root_execution(&code, false).unwrap();
            let limit = vm.heap.allow_only(0);
            let result = vm.run(&code);
            assert_eq!(
                result,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })),
                "{source}"
            );
            vm.finish_root_execution(result).unwrap_err();
            assert!(vm.stack.is_empty());
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(
                vm.execute_script(&compile_script("21 + 21")),
                Ok(Value::Number(42.0))
            );
        });
    }
    for (setup, source) in [
        ("Object.setPrototypeOf(globalThis, new Proxy({}, {has() {throw 7;}}));", "missing = 42"),
        ("Object.setPrototypeOf(globalThis, new Proxy({}, {has() {throw 7;}}));", "'use strict'; missing = 42"),
        ("globalThis.known = 1; Object.setPrototypeOf(globalThis, new Proxy({}, {has() {throw 7;}}));", "'use strict'; known = (delete globalThis.known, 42)"),
        ("Object.setPrototypeOf(globalThis, new Proxy({}, {has() {throw 7;}}));", "with ({}) {missing = 42;}"),
        ("globalThis.known = 1; Object.setPrototypeOf(globalThis, new Proxy({}, {has() {throw 7;}}));", "with ({}) {(() => {'use strict'; known = (delete globalThis.known, 42);})()}"),
        ("Reflect; globalThis.reads = 0; Object.setPrototypeOf(globalThis, new Proxy({probe:7}, {has(target,key) {if (key === 'probe' && ++reads === 2) throw 7; return Reflect.has(target,key);}}));", "with ({}) {probe += 35;}"),
        ("eval;", "eval('var gone = 7; (() => {gone = (delete globalThis.gone, Object.defineProperty(globalThis, \"gone\", {set(value) {throw 7;}}), 42);})()')"),
        ("globalThis.iterable = {[Symbol.iterator]() {return this;}, next() {return {value:7,done:false};}, return() {throw 7;}};", "var [value] = iterable;"),
        ("globalThis.key = {[Symbol.toPrimitive]() {throw 7;}}; globalThis.target = {};", "[target[key]] = [42]"),
        ("globalThis.key = {[Symbol.toPrimitive]() {throw 7;}}; globalThis.target = {};", "({answer:target[key]} = {answer:42})"),
        ("globalThis.thrower = new Proxy({}, {deleteProperty() {throw 7;}});", "with (thrower) {thrower.present = 1; delete present;}"),
        ("", "with ({set value(v) {throw 7;}}) {value = 42;}"),
        ("", "with ({get value() {throw 7;}}) {value += 42;}"),
        ("", "with ({get value() {throw 7;}}) {value++;}"),
        ("", "with ({set value(v) {throw 7;}}) {for (value of [42]) {}}"),
        ("", "with ({set value(v) {throw 7;}}) {for (value in {answer:42}) {}}"),
        ("", "with ({set value(v) {throw 7;}}) {[value] = [42];}"),
        ("", "with ({set value(v) {throw 7;}}) {({answer:value} = {answer:42});}"),
        ("", "with ({set value(v) {throw 7;}}) {var value = 42;}"),
        ("globalThis.iterable = {[Symbol.iterator]() {return this;}, next() {return {value:7,done:false};}, return() {throw 7;}}; globalThis.recur = function recur(n) {if (n === 0) return 42; for (const value of iterable) {return recur(n - 1);}};", "recur(1)"),
        ("globalThis.iterable = {[Symbol.iterator]() {return this;}, next() {return {value:7,done:false};}, return() {throw 7;}}; globalThis.recur = function recur(n) {'use strict'; if (n === 0) return 42; for (const value of iterable) {return recur(n - 1);}};", "recur(1)"),
    ] {
        record_case(&mut failures, source, || {
        let mut vm = Vm::default();
        vm.execute_script(&compile_script(setup)).unwrap();
        assert_eq!(vm.execute_script(&compile_script(source)), Err(RuntimeError::Thrown(Value::Number(7.0))), "{source}");
        assert!(vm.stack.is_empty());
        assert_eq!(vm.execute_script(&compile_script("21 + 21")), Ok(Value::Number(42.0)));
        });
    }
    for source in [
        "var target = {set value(v) {throw 7;}}; with (target) {let value; for (value of [42]) {} if (value !== 42) throw 'lexical shadow';} 42",
        "var value = 0; with ({}) {for (value of [42]) {}} value",
        "eval('var recreated = (delete recreated, Object.defineProperty(globalThis, \"recreated\", {value:7, writable:false, configurable:false}), 42)'); recreated === 7 ? 42 : 0",
        "var read = eval('var gone = 1; (() => gone)'); eval('delete gone'); Object.defineProperty(globalThis, 'gone', {get() {throw 7;}}); try {read();} catch (error) {error === 7 ? 42 : 0;}",
        "var read = eval('var gone = 1; (() => {with ({}) {return typeof gone;}})'); eval('delete gone'); Object.defineProperty(globalThis, 'gone', {get() {throw 7;}}); try {read();} catch (error) {error === 7 ? 42 : 0;}",
        "var read = eval('var gone = 1; (() => typeof gone)'); eval('delete gone'); Object.defineProperty(globalThis, 'gone', {get() {throw 7;}}); try {read();} catch (error) {error === 7 ? 42 : 0;}",
        "var read = eval('var gone = 1; (() => {with ({}) {return gone;}})'); eval('delete gone'); Object.defineProperty(globalThis, 'gone', {get() {throw 7;}}); try {read();} catch (error) {error === 7 ? 42 : 0;}",
        "var read = eval('var gone = 1; (() => {with ({}) {return gone();}})'); eval('delete gone'); Object.defineProperty(globalThis, 'gone', {get() {throw 7;}}); try {read();} catch (error) {error === 7 ? 42 : 0;}",
        "var read = function self() {({answer:self} = {answer:7}); return typeof self === 'function' ? 42 : 0;}; read()",
        "Array; delete Array; try {Array;} catch (error) {error instanceof ReferenceError ? 42 : 0;}",
    ] {
        record_case(&mut failures, source, || {
        let mut vm = Vm::default();
        assert_eq!(vm.execute_script(&compile_script(source)), Ok(Value::Number(42.0)), "{source}");
        assert!(vm.stack.is_empty());
        });
    }
    for (setup, source) in [
        ("globalThis.values = [7,42];", "var [first,...rest] = values; rest[0]"),
        ("globalThis.values = [7,42]; globalThis.target = {};", "[target.first,...target.rest] = values; target.rest[0]"),
        ("Function; Object; globalThis.Base = class {constructor() {return {};}}; globalThis.Derived = class extends Base {constructor() {globalThis.readThis = () => this; super();}};", "new Derived(); 42"),
        ("AggregateError; globalThis.values = [7,42];", "new AggregateError(values).errors[1]"),
        ("RegExp;", "/a/; 42"),
        ("globalThis.payload = 'x'.repeat(8192); globalThis.values = [payload,payload];", "var [first,...rest] = values; first === payload && rest[0] === payload ? 42 : 0"),
        ("globalThis.payload = 'x'.repeat(8192); globalThis.values = [payload,payload]; globalThis.target = {};", "[target.first,...target.rest] = values; target.first === payload && target.rest[0] === payload ? 42 : 0"),
        ("eval; globalThis.payload = 'x'.repeat(8192);", "eval('var gone = 7; (() => {gone = (delete globalThis.gone, payload);})()'); gone === payload ? 42 : 0"),
        ("eval; globalThis.payload = 'x'.repeat(8192);", "(function() {eval('var shadow = 7'); ({answer:shadow} = {answer:payload}); return shadow === payload ? 42 : 0;})()"),
        ("eval; globalThis.outer = function() {eval('var Array = 1; delete Array; delete Array;'); return 42;};", "outer()"),
        ("eval; globalThis.payload = 'x'.repeat(4096); globalThis.outer = function() {eval('var shadow = 7'); shadow = payload; return 42;};", "outer()"),
    ] {record_case(&mut failures, source, || Vm::verify_script_allocation_boundary(setup, source));}
    record_case(&mut failures, "direct eval string result limit", || {
        let mut vm = Vm::default();
        vm.execute_script(&compile_script(
            "eval; globalThis.s = 'oversized'; globalThis.read = function() {'use strict'; return eval('s');};",
        ))
        .unwrap();
        let config = vm.config;
        vm.config.max_string_bytes = 2;
        assert_eq!(
            vm.execute_script(&compile_script("read()")),
            Err(RuntimeError::StringLimit { limit: 2 })
        );
        assert!(vm.stack.is_empty());
        vm.config = config;
        assert_eq!(
            vm.execute_script(&compile_script("read()")),
            Ok(Value::String("oversized".into()))
        );
    });
    assert!(
        failures.is_empty(),
        "independent interpreter boundary failures:\n{}",
        failures.join("\n")
    );
}

#[cfg_attr(test, test)]
fn compiled_source_keys_are_coerced_once_before_target_evaluation() {
    for nursery_capacity in [1, HeapConfig::default().nursery_capacity] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        let source = r#"
            var observations = [];
            var sourceKey = {[Symbol.toPrimitive]() {observations.push('key'); return 'answer';}};
            var source = {get answer() {observations.push('get'); return 42;}};
            var target = {};
            function destination() {observations.push('target'); return target;}
            ({[sourceKey]:destination().value} = source);
            var first = observations.join(',') === 'key,target,get' && target.value === 42;
            observations.length = 0;
            var {[sourceKey]: answer} = source;
            first && observations.join(',') === 'key,get' && answer === 42 ? 42 : 0;
        "#;
        assert_eq!(
            vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        assert!(vm.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn cold_regexp_constructor_identity_preserves_initialization_refusal() {
    let mut vm = Vm::default();
    let pattern = vm
        .execute_script(
            &compile(&parse("({[Symbol.match]:true, constructor:undefined})").unwrap()).unwrap(),
        )
        .unwrap();
    let root = vm.heap.root(pattern.object_id().unwrap()).unwrap();
    assert!(vm.is_regexp(&pattern).unwrap());
    assert_eq!(
        vm.get_property(&pattern, &"constructor".into()),
        Ok(Value::Undefined)
    );
    assert!(!vm.globals.contains_key("RegExp"));
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.regexp_constructor(&pattern, &Value::Undefined, false),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn with_references_preserve_the_resolved_global_environment() {
    let cases = [
        ("globalThis;", "function localEval() {eval('var hidden = 40'); with ({}) {hidden += 2;} return hidden;} localEval()"),
        ("globalThis;", "function capturedEval() {eval('var hidden = 40'); return function() {with ({}) {hidden += 2;} return hidden;};} capturedEval()()"),
        ("globalThis.readTdz = function() {with ({}) {return tdzGlobal += 1;}};", "var answer = 0; try {readTdz();} catch(error) {if (error instanceof ReferenceError) answer = 42;} let tdzGlobal = 7; answer"),
        ("globalThis.savedGlobal = 40;", "with ({}) { (() => {'use strict'; savedGlobal += 2;})() } globalThis.savedGlobal"),
        ("var savedGlobal = 40;", "with ({}) { savedGlobal++; ++savedGlobal; } savedGlobal"),
        ("let savedGlobal = 40;", "with ({}) { savedGlobal += 2; } savedGlobal"),
        ("const savedGlobal = 7;", "with ({}) { try { savedGlobal += 1; } catch (e) { if (e instanceof TypeError) 42; } }"),
        ("globalThis.savedGlobal = 7;", "with ({}) { (() => {'use strict'; try { savedGlobal = (delete globalThis.savedGlobal, 42); } catch (e) { if (e instanceof ReferenceError) return 42; }})() }"),
        ("globalThis.savedGlobal = 7;", "function resolveFirst(p = eval('0')) { savedGlobal = eval('var savedGlobal = 9; 42'); return savedGlobal === 9 && globalThis.savedGlobal === 42 ? 42 : 0; } resolveFirst()"),
        ("globalThis;", "function resolveMissingFirst(p = eval('0')) { newGlobal = eval('var newGlobal = 9; 42'); return newGlobal === 9 && globalThis.newGlobal === 42 ? 42 : 0; } resolveMissingFirst()"),
        ("globalThis;", "with ({}) { (() => {'use strict'; try { absentGlobal = 42; } catch (e) { if (e instanceof ReferenceError) return 42; }})() }"),
        ("globalThis;", "with ({}) { (() => {'use strict'; try { absentGlobal++; } catch (e) { if (e instanceof ReferenceError) return 42; }})() }"),
        ("globalThis;", "function resolveTdz(p = eval('0')) { try { tdzGlobal += 1; } catch (e) { if (e instanceof ReferenceError) return 42; } } resolveTdz(); let tdzGlobal = 7;"),
        ("globalThis.savedGlobal = 7;", "with ({}) { savedGlobal = (delete globalThis.savedGlobal, 42); } globalThis.savedGlobal"),
        ("globalThis.savedGlobal = 7;", "with ({}) { savedGlobal += (delete globalThis.savedGlobal, 35); } globalThis.savedGlobal"),
        ("globalThis;", "function destructure(p = eval('0')) { class C { constructor() { [savedGlobal = 42] = []; } } new C(); } globalThis.savedGlobal = 7; destructure(); globalThis.savedGlobal"),
    ];
    for nursery in [None, Some(1)] {
        for (setup, source) in cases {
            let mut config = VmConfig::default();
            if let Some(nursery) = nursery {
                config.heap.nursery_capacity = nursery;
            }
            let mut vm = Vm::new(config).unwrap();
            vm.execute_script(&compile(&parse(setup).unwrap()).unwrap())
                .unwrap();
            assert_eq!(
                vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
                Ok(Value::Number(42.0)),
                "{source}, nursery {nursery:?}",
            );
            assert!(vm.stack.is_empty());
            assert_eq!(
                vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
        }
    }
}
