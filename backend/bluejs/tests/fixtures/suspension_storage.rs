// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny native snapshot boundaries check admission and detached ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn suspension_storage_refuses_before_allocating_or_moving_the_frame() {
    let mut vm = bounded_vm();
    vm.stack = vec![Value::Number(1.0), Value::String("tiny".into())];
    vm.pending_completions = vec![Completion::Return(Value::Number(42.0))];
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.prepare_suspension_storage(1, true),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.stack.len(), 2);
    assert_eq!(vm.pending_completions.len(), 1);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn suspension_storage_detached_buffers_remain_charged_before_adoption() {
    let mut vm = bounded_vm();
    vm.stack = vec![Value::Number(1.0), Value::String("tiny".into())];
    vm.pending_completions = vec![Completion::Return(Value::String("more".into()))];
    let mut prepared = vm.prepare_suspension_storage(1, true).unwrap();
    let stack = prepared.take_stack(&mut vm);
    let pending = prepared.take_pending(&mut vm);
    let expected = host_scheduling::slots(&stack)
        + host_scheduling::values_bytes(&stack)
        + host_scheduling::slots(&pending)
        + 2 * "more".len();
    assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
    assert_eq!(vm.stack.len(), 1);
    assert!(vm.pending_completions.is_empty());
    drop((stack, pending, prepared));
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn suspension_storage_exact_joint_capacity_and_one_byte_refusal() {
    for pending in [false, true] {
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            vm.stack = vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)];
            vm.pending_completions = vec![Completion::Return(Value::Number(42.0))];
            let bytes = 2 * std::mem::size_of::<Value>()
                + usize::from(pending) * std::mem::size_of::<GeneratorPendingCompletion>();
            let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
            let result = vm.prepare_suspension_storage(1, pending);
            if accepts {
                drop(result.unwrap());
                vm.reconcile_scheduling_ownership();
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert_eq!(vm.stack.len(), 3);
            assert_eq!(vm.pending_completions.len(), 1);
            assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        }
    }
}

#[cfg_attr(test, test)]
fn suspension_storage_whole_stack_moves_without_an_extra_allocation() {
    let mut vm = bounded_vm();
    vm.stack = Vec::with_capacity(16);
    vm.stack.push(Value::String("tiny".into()));
    let pointer = vm.stack.as_ptr();
    let capacity = vm.stack.capacity();
    let _peer = leave_room(&mut vm, 0);
    let mut storage = vm.prepare_suspension_storage(0, false).unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    let stack = storage.take_stack(&mut vm);
    assert_eq!(stack.as_ptr(), pointer);
    assert_eq!(stack.capacity(), capacity);
    assert_eq!(vm.stack.capacity(), 0);
    assert_eq!(
        vm.displaced_execution_control_bytes.get(),
        capacity * std::mem::size_of::<Value>() + 8
    );
    vm.synchronize_scheduling_usage().unwrap();
    vm.stack = stack;
    drop(storage);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn suspension_storage_empty_conversion_releases_unused_source_capacity() {
    let mut vm = bounded_vm();
    vm.pending_completions = Vec::with_capacity(8);
    let _peer = leave_room(&mut vm, 0);
    let mut storage = vm.prepare_suspension_storage(0, true).unwrap();
    let stack = storage.take_stack(&mut vm);
    let pending = storage.take_pending(&mut vm);
    assert_eq!(stack.capacity(), 0);
    assert_eq!(pending.capacity(), 0);
    assert_eq!(vm.pending_completions.capacity(), 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(storage);
    vm.reconcile_scheduling_ownership();
}

#[cfg_attr(test, test)]
fn suspension_storage_error_capacity_survives_native_and_heap_handoffs() {
    let mut vm = bounded_vm();
    let mut message = String::with_capacity(32);
    message.push_str("tiny");
    let bytes = message.capacity();
    vm.pending_completions = vec![Completion::Throw(RuntimeError::TypeError(message))];
    let mut storage = vm.prepare_suspension_storage(0, true).unwrap();
    let stack = storage.take_stack(&mut vm);
    let pending = storage.take_pending(&mut vm);
    assert_eq!(
        vm.displaced_execution_control_bytes.get(),
        host_scheduling::slots(&pending) + bytes
    );
    let state = small_state(&vm, stack, pending);
    let native = state.owned_buffer_bytes();
    let before = vm.heap.stats().managed_bytes;
    let generator = vm
        .heap
        .alloc_generator(GeneratorState::Running, vm.object_prototype)
        .unwrap();
    let root = vm.heap.root(generator).unwrap();
    let baseline = vm.heap.stats().managed_bytes;
    let owned = storage.into_generator(&vm, state);
    assert_eq!(vm.displaced_execution_control_bytes.get(), native);
    owned.store(&mut vm, generator).unwrap();
    assert_eq!(vm.heap.stats().managed_bytes, baseline + native);
    assert!(baseline >= before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.heap.unroot(root).unwrap();
}

fn small_state(
    vm: &Vm,
    stack: Vec<Value>,
    pending: Vec<GeneratorPendingCompletion>,
) -> GeneratorState {
    GeneratorState::Suspended {
        code: SharedBytecode::new(Bytecode::empty()),
        pc: 0,
        stack,
        bindings: Vec::new(),
        cells: HashMap::new(),
        this: Value::Undefined,
        args: Vec::new(),
        completion: Value::Undefined,
        completion_empty: true,
        active_scopes: Vec::new(),
        disposables: Vec::new(),
        dispose_marks: Vec::new(),
        iterators: Vec::new(),
        handlers: Vec::new(),
        pending_completions: pending,
        completion_saves: Vec::new(),
        async_delegate: None,
        delegate: None,
        dynamic_bindings: HashMap::new(),
        home: None,
        callee: Value::Object(vm.object_prototype),
        with_objects: Vec::new(),
    }
}

#[cfg_attr(test, test)]
fn suspension_storage_generator_handoff_retains_object_edges_through_collection() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.stack = vec![Value::Object(object)];
    vm.pending_completions = vec![Completion::Return(Value::Object(object))];
    let prototype = vm.object_prototype;
    let generator = vm
        .with_roots(|heap| heap.alloc_generator(GeneratorState::Running, prototype))
        .unwrap();
    let root = vm.heap.root(generator).unwrap();
    let mut storage = vm.prepare_suspension_storage(0, true).unwrap();
    let stack = storage.take_stack(&mut vm);
    let pending = storage.take_pending(&mut vm);
    let state = small_state(&vm, stack, pending);
    storage
        .into_generator(&vm, state)
        .store(&mut vm, generator)
        .unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(vm.heap.get_own(object, "retained").is_ok());
    vm.heap.unroot(root).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.heap.get_own(object, "retained"),
        Err(HeapError::InvalidObject(object))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn suspension_storage_execution_handoff_keeps_the_parent_prefix_active() {
    let mut vm = bounded_vm();
    vm.stack = vec![
        Value::String("parent".into()),
        Value::String("frame".into()),
    ];
    vm.bindings = vec![Some(Value::Number(42.0))];
    vm.pending_completions = vec![Completion::Return(Value::Number(7.0))];
    let pointer = vm.stack.as_ptr();
    let capacity = vm.stack.capacity();
    let execution = vm.capture_suspension_execution(1).unwrap();
    assert_eq!(vm.stack, vec![Value::String("parent".into())]);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack.capacity(), capacity);
    assert_eq!(execution.stack, vec![Value::String("frame".into())]);
    assert_eq!(execution.bindings, vec![Some(Value::Number(42.0))]);
    let expected = execution.control_bytes() + execution.payload_bytes();
    assert_eq!(
        vm.displaced_execution_control_bytes.get() + vm.displaced_execution_bytes.get(),
        expected
    );
    drop(execution);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

fn install_budget_filler(vm: &mut Vm) -> Rc<RefCell<Option<SharedSchedulingCharge>>> {
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let held = Rc::new(RefCell::new(None));
    let output = held.clone();
    vm.install_host_function("fill", 0, move |_: &[HostValue]| {
        let used = observer.host_scheduling_snapshot().payload_bytes;
        let charge = observer
            .host_scheduling
            .as_ref()
            .unwrap()
            .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
            .unwrap();
        *output.borrow_mut() = Some(charge);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    held
}

#[cfg_attr(test, test)]
fn suspension_storage_real_generator_refusal_restores_the_caller() {
    let mut vm = bounded_vm();
    let held = install_budget_filler(&mut vm);
    // Fixed ordinary operands keep a small suffix live at the yield boundary.
    let operands = (0..32)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("(function*(){{return ((...xs)=>0)({operands},fill(),yield 1);}})();");
    let code = crate::compile(&crate::parse(&source).unwrap()).unwrap();
    let generator = vm.execute_script(&code).unwrap();
    let id = generator.object_id().unwrap();
    assert_eq!(
        vm.generator_next(&generator, None, None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(held.borrow().is_some());
    assert!(vm.heap.generator_state_is_done(id).unwrap());
    assert!(vm.stack.is_empty() && vm.pending_completions.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    drop(held.borrow_mut().take());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn suspension_storage_real_async_refusal_publishes_no_continuation() {
    let mut vm = bounded_vm();
    let setup =
        crate::compile(&crate::parse("globalThis.gate=Promise.resolve(1);").unwrap()).unwrap();
    vm.execute_script(&setup).unwrap();
    let held = install_budget_filler(&mut vm);
    let serial = vm.next_async_continuation;
    let operands = (0..32)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let source =
        format!("(async function(){{return ((...xs)=>0)({operands},fill(),await gate);}})();");
    let code = crate::compile(&crate::parse(&source).unwrap()).unwrap();
    assert_eq!(vm.execute_script(&code), Err(RuntimeError::SchedulingLimit));
    assert!(held.borrow().is_some());
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.next_async_continuation, serial);
    assert!(vm.stack.is_empty() && vm.pending_completions.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    drop(held.borrow_mut().take());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn suspension_storage_refusal_refunds_temporary_buffers_to_a_peer() {
    let mut vm = bounded_vm();
    vm.stack = vec![Value::Number(1.0), Value::Number(2.0)];
    vm.pending_completions = vec![Completion::Return(Value::Number(3.0))];
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let owner = leave_room(
        &mut vm,
        std::mem::size_of::<Value>() + std::mem::size_of::<GeneratorPendingCompletion>() - 1,
    );
    assert!(matches!(
        vm.prepare_suspension_storage(1, true),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.stack.len(), 2);
    assert_eq!(vm.pending_completions.len(), 1);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn suspension_storage_empty_suffix_needs_no_new_buffer() {
    let mut vm = bounded_vm();
    vm.stack = vec![Value::Number(1.0)];
    let _peer = leave_room(&mut vm, 0);
    let mut storage = vm.prepare_suspension_storage(1, false).unwrap();
    let suffix = storage.take_stack(&mut vm);
    assert_eq!(suffix.capacity(), 0);
    assert_eq!(vm.stack, vec![Value::Number(1.0)]);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn suspension_storage_debugger_refusal_clears_nested_and_intermediate_frames() {
    for intermediate in [false, true] {
        let mut vm = bounded_vm();
        let held = install_budget_filler(&mut vm);
        let operands = (0..32)
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let source = if intermediate {
            format!("function f(){{fill();let marker=99;return marker;}} function parent(){{((...xs)=>0)({operands},f(),99);return 0;}} parent();")
        } else {
            format!("function f(){{((...xs)=>0)({operands},fill(),99);return 0;}} f();")
        };
        let mut registry = crate::BlueJsProgramRegistry::default();
        let code = crate::compile(&crate::parse(&source).unwrap()).unwrap();
        let handle = registry
            .install_precompiled(
                crate::BlueJsSourceIdentity::new("page:///snapshot.js", "sha256:snapshot").unwrap(),
                code,
            )
            .unwrap();
        let code = registry.get(handle).unwrap().bytecode().clone();
        let child = &code.functions[0];
        let target = child
            .instructions()
            .find(|instruction| {
                instruction.opcode == Opcode::Constant
                    && instruction
                        .operand
                        .is_some_and(|index| child.constants[index as usize] == Value::Number(99.0))
            })
            .unwrap()
            .offset as u32;
        let serial = vm.next_debugger_frame_serial;
        let result = vm.execute_script_until_nested_debugger_pause(&code, 1, target);
        assert!(
            matches!(result, Err(RuntimeError::SchedulingLimit)),
            "intermediate={intermediate}: {result:?}"
        );
        assert!(held.borrow().is_some());
        assert!(
            vm.debugger_nested_continuation.is_none(),
            "intermediate={intermediate}"
        );
        assert!(vm.debugger_nested_intermediate_continuation.is_none());
        assert!(vm.debugger_nested_parent_execution.is_none());
        assert!(vm.debugger_continuation.is_none());
        if !intermediate {
            assert_eq!(vm.next_debugger_frame_serial, serial);
        }
        assert!(vm.stack.is_empty());
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
        drop(held.borrow_mut().take());
        assert_eq!(
            vm.check_scheduling_failure(),
            Err(RuntimeError::SchedulingLimit)
        );
    }
}

#[cfg_attr(test, test)]
fn suspension_storage_terminal_debugger_cleanup_releases_completion_roots() {
    let mut vm = bounded_vm();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.result_root = Some(vm.heap.root(object).unwrap());
    vm.debugger_nested_parent_execution = Some(vm.suspend_module_execution());
    vm.finish_root_execution(Ok(Value::Undefined)).unwrap();
    assert!(vm.debugger_nested_parent_execution.is_none());
    assert_eq!(
        vm.heap.get_own(object, "retained"),
        Err(HeapError::InvalidObject(object))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

impl crate::Vm {
    /// Runs finite outgoing suspension-storage contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_suspension_storage_contracts() {
        suspension_storage_refuses_before_allocating_or_moving_the_frame();
        suspension_storage_detached_buffers_remain_charged_before_adoption();
        suspension_storage_exact_joint_capacity_and_one_byte_refusal();
        suspension_storage_whole_stack_moves_without_an_extra_allocation();
        suspension_storage_empty_conversion_releases_unused_source_capacity();
        suspension_storage_error_capacity_survives_native_and_heap_handoffs();
        suspension_storage_generator_handoff_retains_object_edges_through_collection();
        suspension_storage_execution_handoff_keeps_the_parent_prefix_active();
        suspension_storage_real_generator_refusal_restores_the_caller();
        suspension_storage_real_async_refusal_publishes_no_continuation();
        suspension_storage_refusal_refunds_temporary_buffers_to_a_peer();
        suspension_storage_empty_suffix_needs_no_new_buffer();
        suspension_storage_debugger_refusal_clears_nested_and_intermediate_frames();
        suspension_storage_terminal_debugger_cleanup_releases_completion_roots();
    }
}
