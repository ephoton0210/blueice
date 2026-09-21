// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
        vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
        Ok(InterpreterExit::Return(Value::Undefined))
    ));
    vm.stack = vec![
        Value::Object(target),
        Value::String("empty".into()),
        Value::Undefined,
    ];
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(matches!(
        vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
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

    code.code[0] = Opcode::DefineClassStaticField as u8;
    vm.stack = vec![
        Value::Undefined,
        Value::Object(target),
        Value::String("field".into()),
        Value::Object(function),
    ];
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(matches!(
        vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
        Ok(InterpreterExit::Return(Value::Undefined))
    ));
    vm.stack = vec![
        Value::Undefined,
        Value::Object(target),
        Value::String("emptyField".into()),
        Value::Undefined,
    ];
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
    vm.this = Value::Object(receiver);
    vm.strict = true;
    assert_eq!(
        vm.super_set(&"value".into(), &Value::Number(1.0)),
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
    vm.this = Value::Object(stale_receiver);
    assert_eq!(
        vm.super_set(&"value".into(), &Value::Number(1.0)),
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
    assert_eq!(
        vm.super_base(),
        Err(RuntimeError::TypeError("superclass is null".into()))
    );
    assert_eq!(
        vm.super_call(Vec::new()),
        Err(RuntimeError::TypeError(
            "super() is not available in this function".into()
        ))
    );
    let closure = vm
        .heap
        .alloc_closure(
            std::rc::Rc::new(Bytecode::empty()),
            Vec::new(),
            Value::Undefined,
            vm.object_prototype,
        )
        .unwrap();
    vm.class_constructor = Some(closure);
    assert_eq!(
        vm.super_call(Vec::new()),
        Err(RuntimeError::TypeError(
            "super() requires a derived constructor".into()
        ))
    );
    vm.heap.set_class_base(closure, Value::Null).unwrap();
    assert_eq!(
        vm.super_call(Vec::new()),
        Err(RuntimeError::TypeError("super constructor is null".into()))
    );
    vm.binding_metadata.push(Binding {
        name: "captured".into(),
        mutable: true,
        strict_immutable: false,
        lexical: true,
        catch_parameter: false,
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
