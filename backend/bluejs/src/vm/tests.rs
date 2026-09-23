// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn an_import_that_joins_a_running_graph_leaves_its_roots_with_that_graph() {
    // While module code runs, its graph's records are parked in
    // `evaluating_linked` and its root list lives in the evaluation's own
    // frame, out of an `import()`'s reach. `dep.js` is compiled and linked
    // only by such a nested import: every root it registers (its two binding
    // cells and its namespace) must be handed to the graph once the running
    // evaluation stores it, where a teardown would unroot them, not dropped.
    let mut vm = Vm::default();
    vm.set_dynamic_module_sources(HashMap::from([(
        "t/dep.js".to_string(),
        "export var x = 1; export var y = 2;".to_string(),
    )]));
    vm.evaluating_linked = Some(HashMap::new());
    vm.execute_module_graph_inner(
        "t/dep.js",
        &HashMap::new(),
        false,
        true,
        ImportPhase::Evaluation,
    )
    .unwrap();
    assert!(
        vm.nested_module_roots.len() >= 3,
        "the nested load's roots wait for the running graph: {}",
        vm.nested_module_roots.len()
    );
    let linked = vm.evaluating_linked.take().expect("still parked");
    assert!(linked.contains_key("t/dep.js"));
    vm.store_module_graph(
        ModuleGraphState {
            linked,
            roots: Vec::new(),
        },
        false,
    );
    assert!(vm.nested_module_roots.is_empty());
    assert!(vm.module_graph.as_ref().unwrap().roots.len() >= 3);
}

#[test]
fn interpreter_converts_catchable_errors_and_rejects_a_top_level_yield() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    let test262_error = vm.error_value(RuntimeError::Test262("failure".into()));
    assert!(
        matches!(test262_error, Ok(Value::Object(_))),
        "{test262_error:?}"
    );
    assert_eq!(
        vm.error_value(RuntimeError::InstructionLimit),
        Err(RuntimeError::InstructionLimit)
    );
    let mut code = Bytecode::empty();
    code.constants.push(Value::Undefined);
    code.code
        .extend([Opcode::Constant as u8, 0, 0, 0, 0, Opcode::Yield as u8]);
    vm.remaining_instructions = vm.config.instruction_budget;
    let yield_error = vm.execute(&code);
    assert!(
        matches!(yield_error, Err(RuntimeError::TypeError(ref message)) if message == "yield is not supported in this execution context"),
        "{yield_error:?}"
    );
    code.generator = true;
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(
        matches!(vm.execute(&code), Err(RuntimeError::TypeError(message)) if message == "yield requires a generator function")
    );
}

#[test]
fn with_lookup_uses_the_object_then_reports_an_unbound_name() {
    let mut vm = Vm::default();
    let object = vm.heap.alloc_object(None).unwrap();
    vm.heap.set(object, "value", Value::Number(7.0)).unwrap();
    vm.with_objects.push(Value::Object(object));
    assert_eq!(vm.with_get("value", None), Ok(Value::Number(7.0)));
    assert_eq!(
        vm.with_get("missing", None),
        Err(RuntimeError::ReferenceError("missing".into()))
    );
}

#[test]
fn class_definition_opcodes_assign_home_objects_to_closures() {
    let mut vm = Vm::default();
    let target = vm.heap.alloc_object(None).unwrap();
    let mut function_code = Bytecode::empty();
    function_code.code.push(Opcode::Halt as u8);
    let function = vm
        .heap
        .alloc_closure(
            std::rc::Rc::new(function_code),
            Vec::new(),
            Value::Undefined,
            vm.object_prototype,
        )
        .unwrap();
    // DefineMethod carries an operand (non-zero: object-literal method).
    let mut method_code = Bytecode::empty();
    method_code.code.push(Opcode::DefineMethod as u8);
    method_code.code.extend(0u32.to_le_bytes());
    method_code.code.push(Opcode::Halt as u8);
    let mut code = Bytecode::empty();
    code.code
        .extend([Opcode::DefineMethod as u8, Opcode::Halt as u8]);

    vm.stack = vec![
        Value::Object(target),
        Value::String("method".into()),
        Value::Object(function),
    ];
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(matches!(
        vm.interpret(&method_code, &mut Vec::new(), 0, None, None, None),
        Ok(InterpreterExit::Return(Value::Undefined))
    ));
    vm.stack = vec![
        Value::Object(target),
        Value::String("empty".into()),
        Value::Undefined,
    ];
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(matches!(
        vm.interpret(&method_code, &mut Vec::new(), 0, None, None, None),
        Ok(InterpreterExit::Return(Value::Undefined))
    ));

    code.code[0] = Opcode::CallClassStaticBlock as u8;
    vm.stack = vec![Value::Object(target), Value::Object(function)];
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(matches!(
        vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
        Ok(InterpreterExit::Return(Value::Undefined))
    ));
    vm.stack = vec![Value::Object(target), Value::Undefined];
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(matches!(
        vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn super_assignment_reports_a_non_extensible_receiver() {
    let mut vm = Vm::default();
    let base = vm.heap.alloc_object(None).unwrap();
    let home = vm.heap.alloc_object(Some(base)).unwrap();
    let receiver = vm.heap.alloc_object(None).unwrap();
    vm.heap.prevent_extensions(receiver).unwrap();
    vm.home_object = Some(home);
    vm.strict = true;
    let super_base = vm.super_base().unwrap();
    assert_eq!(super_base, Value::Object(base));
    assert_eq!(
        vm.super_set(
            &super_base,
            &Value::String("value".into()),
            &Value::Number(1.0),
            &Value::Object(receiver)
        ),
        Err(RuntimeError::TypeError(
            "super property cannot be assigned".into()
        ))
    );

    let base = vm.heap.alloc_object(None).unwrap();
    let home = vm.heap.alloc_object(Some(base)).unwrap();
    vm.heap.root(home).unwrap();
    let stale_receiver = vm.heap.alloc_object(None).unwrap();
    vm.heap.collect_major();
    vm.home_object = Some(home);
    let super_base = vm.super_base().unwrap();
    assert_eq!(
        vm.super_set(
            &super_base,
            &Value::String("value".into()),
            &Value::Number(1.0),
            &Value::Object(stale_receiver)
        ),
        Err(RuntimeError::Heap(HeapError::InvalidObject(stale_receiver)))
    );
}

#[test]
fn super_and_eval_context_errors_describe_missing_internal_context() {
    let mut vm = Vm::default();
    assert_eq!(
        vm.super_base(),
        Err(RuntimeError::TypeError(
            "super is not available in this function".into()
        ))
    );
    let home = vm.heap.alloc_object(None).unwrap();
    vm.home_object = Some(home);
    // A null super base is only an error once a property is read through it.
    assert_eq!(vm.super_base(), Ok(Value::Null));
    assert_eq!(
        vm.super_get(&Value::Null, &Value::String("x".into()), &Value::Undefined),
        Err(RuntimeError::TypeError(
            "cannot access a property through a null super base".into()
        ))
    );
    assert_eq!(
        vm.super_call(Value::Null, Vec::new()),
        Err(RuntimeError::TypeError(
            "super constructor is not a constructor".into()
        ))
    );
    vm.binding_metadata.push(Binding {
        name: "captured".into(),
        mutable: true,
        strict_immutable: false,
        lexical: true,
        catch_parameter: false,
        eval_var: false,
    });
    vm.cells.insert(0, home);
    assert_eq!(
        vm.eval_visible_bindings()
            .into_iter()
            .map(|(name, _, slot)| (name, slot))
            .collect::<Vec<_>>(),
        vec![("captured".into(), 0)]
    );
}

#[test]
fn compiler_owned_bytecode_invariants_fail_loudly() {
    let no_handler = Bytecode::empty();
    let mut vm = Vm::default();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.resolve_completion(
            &no_handler,
            &mut Vec::new(),
            &mut Vec::new(),
            Completion::Yield(Value::Undefined)
        )))
        .is_err()
    );
    let mut vm = Vm::default();
    vm.stack.push(Value::Number(0.0));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.set_class_heritage())).is_err()
    );
    let mut vm = Vm::default();
    vm.stack.push(Value::Number(0.0));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.set_class_home())).is_err()
    );
}

#[test]
fn host_object_methods_are_realm_local_callable_globals() {
    let mut vm = Vm::default();
    vm.install_host_function("double", 1, |args: &[HostValue]| {
        let Some(HostValue::Number(value)) = args.first() else {
            return Err(HostFunctionError::new("double requires a number"));
        };
        Ok(HostValue::Number(value * 2.0))
    })
    .unwrap();
    let code = crate::compile(&crate::parse("double(21);").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&code).unwrap(), Value::Number(42.0));

    let host = vm.install_host_object("blueice").unwrap();
    vm.install_host_method(host, "increment", 1, |args: &[HostValue]| {
        let Some(HostValue::Number(value)) = args.first() else {
            return Err(HostFunctionError::new("increment requires a number"));
        };
        Ok(HostValue::Number(value + 1.0))
    })
    .unwrap();
    let code = crate::compile(&crate::parse("blueice.increment(41);").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&code).unwrap(), Value::Number(42.0));

    let wrong_arity = crate::compile(&crate::parse("blueice.increment('x');").unwrap()).unwrap();
    assert!(matches!(
        vm.execute_script(&wrong_arity),
        Err(RuntimeError::TypeError(message)) if message == "increment requires a number"
    ));
    let object_argument =
        crate::compile(&crate::parse("blueice.increment({ value: 1 });").unwrap()).unwrap();
    assert!(matches!(
        vm.execute_script(&object_argument),
        Err(RuntimeError::TypeError(message)) if message == "host functions accept primitive values only"
    ));
}

#[test]
fn host_objects_and_methods_reject_collisions_and_construction() {
    let mut vm = Vm::default();
    let code = crate::compile(&crate::parse("globalThis.reserved = undefined;").unwrap()).unwrap();
    vm.execute_script(&code).unwrap();
    assert!(matches!(
        vm.install_host_object("reserved"),
        Err(RuntimeError::TypeError(_))
    ));
    let host = vm.install_host_object("blueice").unwrap();
    assert!(matches!(
        vm.install_host_object("blueice"),
        Err(RuntimeError::TypeError(_))
    ));
    vm.install_host_method(host, "value", 0, |_args: &[HostValue]| {
        Ok(HostValue::Number(1.0))
    })
    .unwrap();
    let code = crate::compile(&crate::parse("blueice.reserved = undefined;").unwrap()).unwrap();
    vm.execute_script(&code).unwrap();
    assert!(matches!(
        vm.install_host_method(host, "reserved", 0, |_args: &[HostValue]| Ok(
            HostValue::Number(2.0)
        )),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        vm.install_host_method(host, "value", 0, |_args: &[HostValue]| Ok(
            HostValue::Number(2.0)
        )),
        Err(RuntimeError::TypeError(_))
    ));
    let code = crate::compile(&crate::parse("new blueice.value();").unwrap()).unwrap();
    assert!(matches!(
        vm.execute_script(&code),
        Err(RuntimeError::TypeError(message)) if message == "value is not a constructor"
    ));
}

#[test]
fn a_call_is_refused_exactly_when_the_native_stack_falls_below_the_red_zone() {
    use super::completion::CALL_STACK_RED_ZONE;
    // The depth is irrelevant once the thread's stack can be measured: only
    // the bytes left decide, so a deep-but-cheap chain is not cut short and a
    // shallow-but-nearly-overflowing one is.
    for depth in [0, 1, 32, 500, usize::MAX] {
        assert!(!call_stack_exhausted(Some(CALL_STACK_RED_ZONE), depth));
        assert!(!call_stack_exhausted(Some(usize::MAX), depth));
        assert!(call_stack_exhausted(Some(CALL_STACK_RED_ZONE - 1), depth));
        assert!(call_stack_exhausted(Some(0), depth));
    }
}

#[test]
fn an_unmeasurable_stack_falls_back_to_the_conservative_frame_count() {
    use super::completion::UNMEASURED_STACK_MAX_CALL_DEPTH;
    assert!(!call_stack_exhausted(None, 0));
    assert!(!call_stack_exhausted(
        None,
        UNMEASURED_STACK_MAX_CALL_DEPTH - 1
    ));
    assert!(call_stack_exhausted(None, UNMEASURED_STACK_MAX_CALL_DEPTH));
    assert!(call_stack_exhausted(None, usize::MAX));
}

#[test]
fn host_values_convert_both_ways_for_every_primitive_and_refuse_objects() {
    let primitives = [
        (Value::Undefined, HostValue::Undefined),
        (Value::Null, HostValue::Null),
        (Value::Bool(true), HostValue::Bool(true)),
        (Value::Number(1.5), HostValue::Number(1.5)),
        (
            Value::String("host".into()),
            HostValue::String("host".into()),
        ),
    ];
    for (value, host) in primitives {
        assert_eq!(HostValue::try_from(&value).unwrap(), host);
        assert_eq!(Value::from(host), value);
    }
    let mut vm = Vm::default();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    assert_eq!(
        HostValue::try_from(&Value::Object(object))
            .unwrap_err()
            .to_string(),
        "host functions accept primitive values only"
    );
}

#[test]
fn every_runtime_error_renders_and_only_a_heap_error_has_a_source() {
    let rendered = [
        (
            RuntimeError::ReferenceError("x".into()),
            "ReferenceError: x is not defined",
        ),
        (RuntimeError::TypeError("t".into()), "TypeError: t"),
        (RuntimeError::RangeError("r".into()), "RangeError: r"),
        (RuntimeError::SyntaxError("s".into()), "SyntaxError: s"),
        (
            RuntimeError::Thrown(Value::Null),
            "uncaught JavaScript value: Null",
        ),
        (
            RuntimeError::Heap(HeapError::InvalidConfig),
            "invalid BlueJS heap configuration",
        ),
        (
            RuntimeError::InstructionLimit,
            "BlueJS instruction budget exhausted",
        ),
        (
            RuntimeError::StringLimit { limit: 7 },
            "BlueJS string exceeds 7 bytes",
        ),
        (RuntimeError::Test262("t".into()), "Test262Error: t"),
        (RuntimeError::RegexTimeout, "BlueJS regex deadline exceeded"),
        (
            RuntimeError::RegexWorker("w".into()),
            "BlueJS regex worker failed: w",
        ),
        (
            RuntimeError::ModuleResolution("no such module".into()),
            "module resolution error: no such module",
        ),
        (
            RuntimeError::Unsupported("a missing feature"),
            "BlueJS unsupported: a missing feature",
        ),
    ];
    for (error, text) in rendered {
        assert_eq!(error.to_string(), text);
        let source = std::error::Error::source(&error);
        assert_eq!(source.is_some(), matches!(error, RuntimeError::Heap(_)));
    }
}

#[test]
fn heap_errors_map_to_the_language_error_they_stand_for() {
    let object = ObjectId { heap: 0, serial: 0 };
    assert!(matches!(
        RuntimeError::from(HeapError::InvalidArrayLength),
        RuntimeError::RangeError(message) if message == "invalid array length"
    ));
    assert!(matches!(
        RuntimeError::from(HeapError::InvalidBufferRange),
        RuntimeError::RangeError(message) if message == "invalid ArrayBuffer view range"
    ));
    for error in [
        HeapError::DetachedArrayBuffer,
        HeapError::ImmutableArrayBuffer,
        HeapError::InvalidWeakTarget,
        HeapError::InvalidInternalSlot(object),
        HeapError::RevokedProxy,
    ] {
        let message = error.to_string();
        assert_eq!(RuntimeError::from(error), RuntimeError::TypeError(message));
    }
    assert!(matches!(
        RuntimeError::from(HeapError::UninitializedModuleExport),
        RuntimeError::ReferenceError(message) if message == "module export is uninitialized"
    ));
    // Every other heap error is carried through unchanged.
    assert_eq!(
        RuntimeError::from(HeapError::PrototypeCycle),
        RuntimeError::Heap(HeapError::PrototypeCycle)
    );
}

#[test]
fn host_function_installation_walks_every_branch_with_one_callback_type() {
    let mut vm = Vm::default();
    // One callback type for every call, so each installer's single
    // monomorphised instance takes every branch below.
    let noop = |_args: &[HostValue]| Ok(HostValue::Undefined);

    vm.install_host_function("taken", 0, noop).unwrap();
    for name in ["", "has space", "1leading", "taken"] {
        assert!(
            matches!(
                vm.install_host_function(name, 0, noop),
                Err(RuntimeError::TypeError(message))
                    if message == "host global name is invalid or already defined"
            ),
            "{name:?}"
        );
    }

    let own = vm.install_host_object("here").unwrap();
    let foreign = Vm::default().install_host_object("elsewhere").unwrap();
    vm.install_host_method(own, "method", 0, noop).unwrap();
    for (owner, name, reason) in [
        (foreign, "method", "host object or method name is invalid"),
        (own, "bad name", "host object or method name is invalid"),
        (own, "method", "host method is already defined"),
    ] {
        assert!(
            matches!(
                vm.install_host_method(owner, name, 0, noop),
                Err(RuntimeError::TypeError(message)) if message == reason
            ),
            "{name:?}: {reason}"
        );
    }
    assert!(matches!(
        vm.install_host_object("here"),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        vm.install_host_object("no good"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn a_host_function_reached_directly_refuses_construction_and_unknown_indexes() {
    let mut vm = Vm::default();
    vm.install_host_function("callable", 0, |_args: &[HostValue]| Ok(HostValue::Null))
        .unwrap();
    // JavaScript never gets past `IsConstructor` first, or past a valid index.
    assert!(matches!(
        vm.host_function_call(0, Value::Undefined, &[], true),
        Err(RuntimeError::TypeError(message)) if message == "host functions are not constructors"
    ));
    assert!(matches!(
        vm.host_function_call(99, Value::Undefined, &[], false),
        Err(RuntimeError::TypeError(message)) if message == "host function is unavailable"
    ));
    assert_eq!(
        vm.host_function_call(0, Value::Undefined, &[], false),
        Ok(Value::Null)
    );
}

#[test]
fn the_function_prototype_is_reported_missing_when_string_lost_its_prototype() {
    let mut vm = Vm::default();
    let script = crate::compile(&crate::parse("Object.setPrototypeOf(String, null);").unwrap());
    vm.execute_script(&script.unwrap()).unwrap();
    assert!(matches!(
        vm.function_prototype(),
        Err(RuntimeError::TypeError(message)) if message == "Function prototype is unavailable"
    ));
}

#[test]
fn the_lazy_object_prototype_methods_are_left_alone_when_a_script_defined_them() {
    let mut vm = Vm::default();
    // Touching either name already installs the built-in (and records that),
    // and only then does the assignment replace it.
    let script = crate::compile(
        &crate::parse(
            "Object.prototype.propertyIsEnumerable = 1; Object.prototype.hasOwnProperty = 2;",
        )
        .unwrap(),
    );
    vm.execute_script(&script.unwrap()).unwrap();
    // With the record cleared, the script's own values are what is found.
    vm.property_is_enumerable_installed = false;
    vm.has_own_property_installed = false;
    // The second call finds the work already recorded as done.
    for _ in 0..2 {
        vm.property_is_enumerable_intrinsic().unwrap();
        vm.has_own_property_intrinsic().unwrap();
    }
    let after = crate::compile(&crate::parse("Object.prototype.hasOwnProperty").unwrap());
    assert_eq!(vm.execute_script(&after.unwrap()), Ok(Value::Number(2.0)));
}

/// A VM, with the function prototype and `globalThis` already built, whose
/// heap ceiling leaves only `extra` bytes over what that took; `None` when
/// even that does not fit. Building those lazily is what an operation under
/// test must not be charged for, or every run would fail before reaching it.
fn vm_with_heap_headroom(extra: usize) -> Option<Vm> {
    fn warm_up(vm: &mut Vm) -> Result<(), RuntimeError> {
        vm.function_prototype()?;
        vm.global("globalThis")?;
        Ok(())
    }
    let mut probe = Vm::default();
    warm_up(&mut probe).unwrap();
    let limit = probe.heap().stats().managed_bytes + extra;
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            major_threshold_bytes: limit.min(HeapConfig::default().major_threshold_bytes),
            max_heap_bytes: limit,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .ok()?;
    warm_up(&mut vm).ok()?;
    Some(vm)
}

/// Runs `operation` on VMs with ever more heap headroom, so that each
/// allocation it makes fails in turn. Every outcome must be either success or
/// the heap-limit error, never a panic or some other error; returns how many
/// runs hit the limit.
fn heap_limit_failures(mut operation: impl FnMut(&mut Vm) -> Result<(), RuntimeError>) -> usize {
    let mut failures = 0;
    for extra in (0..6144).step_by(8) {
        let Some(mut vm) = vm_with_heap_headroom(extra) else {
            continue;
        };
        match operation(&mut vm) {
            Ok(()) => {}
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => failures += 1,
            Err(other) => panic!("headroom {extra}: {other:?}"),
        }
    }
    failures
}

#[test]
fn installing_a_host_function_survives_running_out_of_heap_at_each_step() {
    let failures = heap_limit_failures(|vm| {
        vm.install_host_function("probe", 2, |_args: &[HostValue]| Ok(HostValue::Undefined))
    });
    assert!(failures > 3, "only {failures} failing points were reached");
}

#[test]
fn installing_native_getters_and_accessors_survives_running_out_of_heap() {
    let object_method = || NativeFunction::ObjectMethod(native::ObjectMethod::HasOwn);
    for (label, failures) in [
        (
            "getter",
            heap_limit_failures(|vm| {
                let prototype = vm.function_prototype()?;
                let owner = vm.object_prototype;
                vm.install_native_getter(owner, prototype, "probe", object_method())
            }),
        ),
        (
            "symbol getter",
            heap_limit_failures(|vm| {
                let prototype = vm.function_prototype()?;
                let owner = vm.object_prototype;
                vm.install_symbol_native_getter(owner, prototype, "toStringTag", object_method())
            }),
        ),
        (
            "accessor",
            heap_limit_failures(|vm| {
                let prototype = vm.function_prototype()?;
                let owner = vm.object_prototype;
                vm.install_native_accessor(
                    owner,
                    prototype,
                    "probe",
                    object_method(),
                    object_method(),
                )
            }),
        ),
    ] {
        assert!(failures > 3, "{label}: only {failures} failing points");
    }
}

#[test]
fn installing_the_lazy_object_prototype_methods_survives_running_out_of_heap() {
    let failures = heap_limit_failures(|vm| vm.property_is_enumerable_intrinsic());
    assert!(failures > 0, "propertyIsEnumerable: {failures}");
    let failures = heap_limit_failures(|vm| vm.has_own_property_intrinsic());
    assert!(failures > 0, "hasOwnProperty: {failures}");
}

#[test]
fn a_native_accessor_cannot_replace_a_non_configurable_property() {
    let mut vm = Vm::default();
    let prototype = vm.function_prototype().unwrap();
    let owner = vm.object_prototype;
    vm.define_data(owner, "locked", Value::Number(1.0), false, false, false)
        .unwrap();
    let native = NativeFunction::ObjectMethod(native::ObjectMethod::HasOwn);
    assert!(matches!(
        vm.install_native_accessor(owner, prototype, "locked", native, native),
        Err(RuntimeError::TypeError(message)) if message == "cannot install native accessor"
    ));
}
