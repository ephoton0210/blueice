// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny debugger snapshots retain storage independently of their originating VM.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn paused_records(vm: &mut Vm) -> DebuggerContinuation {
    let code = crate::compile(
        &crate::parse("for (const item of [1]) {try {item;} finally {}} 0;").unwrap(),
    )
    .unwrap();
    let handler = code
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::PushHandler)
        .unwrap();
    let target = handler.offset + handler.opcode.width();
    assert!(matches!(
        vm.execute_script_until_debugger_pause(&code, target as u32)
            .unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    vm.debugger_continuation.take().unwrap()
}

fn minimum_root_storage(frame: &DebuggerContinuation) -> usize {
    assert!(frame.code.functions.is_empty());
    frame.code.retained_bytes()
        + frame.code.runtime_source_bytes()
        + std::mem::size_of::<DebuggerContinuation>()
        - std::mem::size_of_val(&frame.code)
        + host_scheduling::slots(&frame.iterators)
        + host_scheduling::values_bytes(&frame.iterators)
        + host_scheduling::slots(&frame.handlers)
        + Vm::diagnostic_handler_bytes(&frame.handlers)
}

#[cfg_attr(test, test)]
fn retained_frame_storage_debugger_records_outlive_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let frame = paused_records(&mut vm);
    let minimum = minimum_root_storage(&frame);
    assert!(!frame.iterators.is_empty() && !frame.handlers.is_empty());
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= minimum,
        "retained interpreter buffers and controller storage must keep their native credit"
    );
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_captured_execution_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    vm.stack = vec![Value::Bool(true), Value::Number(42.0)];
    let execution = vm.capture_suspension_execution(0).unwrap();
    let bytes = execution.control_bytes() + execution.payload_bytes();
    assert!(bytes > 0);
    vm.synchronize_scheduling_usage().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(execution);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_late_activation_attaches_paused_records() {
    let mut vm = Vm::default();
    let frame = paused_records(&mut vm);
    vm.debugger_continuation = Some(frame);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let frame = vm.debugger_continuation.take().unwrap();
    let minimum = minimum_root_storage(&frame);
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= minimum,
        "late activation must attach retained interpreter and controller storage"
    );
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_handler_origin_keeps_its_native_credit() {
    let mut vm = page();
    let view = observer(&vm);
    let mut frame = paused_records(&mut vm);
    frame.handlers.last_mut().unwrap().pending_origin.source =
        Some(crate::diagnostics::DiagnosticSourceLocation {
            filename: "retained-handler.js".into(),
            line: 1,
            column: 1,
        });
    frame.handlers.retain_native_storage(&mut vm).unwrap();
    let minimum = minimum_root_storage(&frame);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, minimum);
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn installed_script(source: &str) -> Bytecode {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            crate::BlueJsSourceIdentity::new("page:///retained-frame.js", "sha256:retained-frame")
                .unwrap(),
            &crate::BlueJsProgramV1::Script(crate::parse(source).unwrap()),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

fn execution_storage(execution: &SuspendedModuleExecution) -> usize {
    execution.control_bytes() + execution.payload_bytes()
}

fn records_storage(iterators: &IteratorRecords, handlers: &HandlerRecords) -> usize {
    host_scheduling::slots(iterators)
        + host_scheduling::values_bytes(iterators)
        + host_scheduling::slots(handlers)
        + Vm::diagnostic_handler_bytes(handlers)
}

fn assert_nested_storage(intermediate: bool) {
    assert_nested_storage_mode(intermediate, false, false);
}

fn assert_nested_storage_mode(intermediate: bool, step: bool, late: bool) {
    let mut vm = if late { Vm::default() } else { page() };
    let code = installed_script(if intermediate {
        "function inner(){for(const item of [1]){try{item;}finally{}}}function middle(){return inner();}middle();"
    } else {
        "function inner(){for(const item of [1]){try{item;}finally{}}}inner();"
    });
    let handler = code.functions[0]
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::PushHandler)
        .unwrap();
    let target = (handler.offset + handler.opcode.width()) as u32;
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&code, 1, target)
        .unwrap()
    else {
        panic!("selected child must pause");
    };
    if late {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    }
    if step {
        assert!(matches!(
            vm.step_debugger_nested_instruction(frame_serial).unwrap(),
            VmDebuggerNestedExecutionState::Paused { .. }
        ));
    }
    let view = observer(&vm);
    let child = vm.debugger_nested_continuation.take().unwrap();
    let middle = vm.debugger_nested_intermediate_continuation.take();
    assert_eq!(middle.is_some(), intermediate);
    let mut minimum = child.code.retained_bytes()
        + child.code.runtime_source_bytes()
        + std::mem::size_of::<NestedDebuggerContinuation>()
        - std::mem::size_of_val(&child.code)
        + execution_storage(&child.execution)
        + records_storage(&child.iterators, &child.handlers);
    if let Some(frame) = &middle {
        minimum += frame.code.retained_bytes()
            + std::mem::size_of::<IntermediateDebuggerContinuation>()
            - std::mem::size_of_val(&frame.code)
            + execution_storage(&frame.execution)
            + records_storage(&frame.iterators, &frame.handlers);
    }
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= minimum,
        "retained nested state lost its controller or interpreter allocation credit"
    );
    drop(child);
    drop(middle);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_nested_state_outlives_the_vm() {
    assert_nested_storage(false);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_intermediate_state_outlives_the_vm() {
    assert_nested_storage(true);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_module_state_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let code =
        crate::compile_module(&crate::parse_module("export const answer = 42;").unwrap()).unwrap();
    let target = code.module_evaluate_entry.unwrap();
    vm.execute_module_graph_until_debugger_pause(
        "entry",
        &HashMap::from([("entry".into(), code)]),
        target,
    )
    .unwrap();
    let frame = vm.debugger_module_continuation.take().unwrap();
    let minimum = frame.code.registry().retained_bytes()
        + frame.module.capacity()
        + std::mem::size_of::<ModuleDebuggerContinuation>()
        + execution_storage(&frame.execution)
        + records_storage(&frame.iterators, &frame.handlers);
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= minimum,
        "retained module state lost its identifier, controller or frame storage credit"
    );
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_moving_an_origin_has_no_duplicate_peak() {
    let mut vm = page();
    let mut frame = paused_records(&mut vm);
    vm.pending_diagnostic_origin = Some(crate::diagnostics::DiagnosticSourceLocation {
        filename: "moved-handler.js".into(),
        line: 1,
        column: 1,
    });
    vm.synchronize_scheduling_usage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let ((), peak) = scope.measure_payload_peak(|| {
        frame.handlers.last_mut().unwrap().pending_origin.source =
            vm.pending_diagnostic_origin.take();
        frame.handlers.retain_native_storage(&mut vm).unwrap();
        vm.reconcile_scheduling_ownership();
    });
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        peak, 0,
        "moving existing data must not acquire another charge"
    );
}

#[cfg_attr(test, test)]
fn retained_frame_storage_captured_payload_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    vm.stack = vec![Value::String(
        "retained execution payload across VM teardown".into(),
    )];
    vm.pending_completions
        .push(Completion::Return(Value::String(
            "retained pending completion payload".into(),
        )));
    let execution = vm.capture_suspension_execution(0).unwrap();
    assert!(execution.payload_bytes() > 0);
    let bytes = execution_storage(&execution);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(execution);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_capture_restoration_preserves_the_original_allocation() {
    for at in [0, 1] {
        let mut vm = page();
        vm.stack = vec![Value::Bool(true), Value::Number(42.0)];
        let pointer = vm.stack.as_ptr();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let execution = vm.capture_suspension_execution(at).unwrap();
        vm.restore_captured_execution(execution);
        assert_eq!(vm.stack.as_ptr(), pointer);
        assert_eq!(vm.stack, vec![Value::Bool(true), Value::Number(42.0)]);
        assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn retained_frame_storage_nested_steps_keep_durable_state() {
    assert_nested_storage_mode(false, true, false);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_late_activation_attaches_nested_and_intermediate_state() {
    for intermediate in [false, true] {
        assert_nested_storage_mode(intermediate, false, true);
    }
}

fn minimum_module_storage(frame: &ModuleDebuggerContinuation) -> usize {
    frame.code.registry().retained_bytes()
        + frame.module.capacity()
        + std::mem::size_of::<ModuleDebuggerContinuation>()
        + execution_storage(&frame.execution)
        + records_storage(&frame.iterators, &frame.handlers)
}

fn assert_module_mode(step: bool, late: bool) {
    let mut vm = if late { Vm::default() } else { page() };
    let code =
        crate::compile_module(&crate::parse_module("export const answer = 42;").unwrap()).unwrap();
    let target = code.module_evaluate_entry.unwrap();
    vm.execute_module_graph_until_debugger_pause(
        "entry",
        &HashMap::from([("entry".into(), code)]),
        target,
    )
    .unwrap();
    if late {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    }
    if step {
        assert!(matches!(
            vm.step_debugger_module_root_instruction().unwrap(),
            VmDebuggerExecutionState::Paused { .. }
        ));
    }
    let view = observer(&vm);
    let frame = vm.debugger_module_continuation.take().unwrap();
    let minimum = minimum_module_storage(&frame);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= minimum);
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_module_steps_keep_durable_state() {
    assert_module_mode(true, false);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_late_activation_attaches_module_state() {
    assert_module_mode(false, true);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_nested_module_throw_keeps_the_waiting_root() {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            crate::BlueJsSourceIdentity::new("page:///retained-throw.mjs", "sha256:retained-throw")
                .unwrap(),
            &crate::BlueJsProgramV1::Module(
                crate::parse_module(
                    "function inner(){throw 7;}try{inner();}catch(error){globalThis.caught=error;}export const answer=42;",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let code = registry.get(handle).unwrap().bytecode().clone();
    let mut vm = page();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_module_graph_until_nested_debugger_pause(
            "entry",
            &HashMap::from([("entry".into(), code)]),
            1,
            0,
        )
        .unwrap()
    else {
        panic!("throwing child must pause");
    };
    assert!(matches!(
        vm.resume_debugger_nested_execution(frame_serial).unwrap(),
        VmDebuggerNestedExecutionState::FrameReturned { .. }
    ));
    let view = observer(&vm);
    let frame = vm.debugger_module_continuation.take().unwrap();
    let minimum = minimum_module_storage(&frame);
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= minimum,
        "a child throw must retain its parked module caller's storage"
    );
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn retained_frame_storage_foreign_attachment_preserves_the_source_owner() {
    let mut vm = page();
    let view = observer(&vm);
    vm.stack = vec![Value::Bool(true)];
    let mut execution = vm.capture_suspension_execution(0).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let pointer = execution.stack.as_ptr();
    let mut other = page();
    assert_eq!(
        execution.retain_native_storage(&mut other),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(execution.stack.as_ptr(), pointer);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn retained_frame_storage_terminal_cleanup_releases_durable_state() {
    let mut vm = page();
    let view = observer(&vm);
    let frame = paused_records(&mut vm);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes > 0);
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(
        view.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_retained_frame_storage_contracts() {
        retained_frame_storage_debugger_records_outlive_the_vm();
        retained_frame_storage_captured_execution_outlives_the_vm();
        retained_frame_storage_late_activation_attaches_paused_records();
        retained_frame_storage_handler_origin_keeps_its_native_credit();
        retained_frame_storage_nested_state_outlives_the_vm();
        retained_frame_storage_intermediate_state_outlives_the_vm();
        retained_frame_storage_module_state_outlives_the_vm();
        retained_frame_storage_moving_an_origin_has_no_duplicate_peak();
        retained_frame_storage_captured_payload_outlives_the_vm();
        retained_frame_storage_capture_restoration_preserves_the_original_allocation();
        retained_frame_storage_nested_steps_keep_durable_state();
        retained_frame_storage_late_activation_attaches_nested_and_intermediate_state();
        retained_frame_storage_module_steps_keep_durable_state();
        retained_frame_storage_late_activation_attaches_module_state();
        retained_frame_storage_nested_module_throw_keeps_the_waiting_root();
        retained_frame_storage_foreign_attachment_preserves_the_source_owner();
        retained_frame_storage_terminal_cleanup_releases_durable_state();
        retained_frame_storage_equal_sized_origin_transfer_updates_peer_usage();
    }
}

#[cfg_attr(test, test)]
fn retained_frame_storage_equal_sized_origin_transfer_updates_peer_usage() {
    let mut vm = page();
    let view = observer(&vm);
    let mut frame = paused_records(&mut vm);
    let location = crate::diagnostics::DiagnosticSourceLocation {
        filename: "same-handler.js".into(),
        line: 1,
        column: 1,
    };
    frame.handlers.last_mut().unwrap().pending_origin.source = Some(location.clone());
    frame.handlers.retain_native_storage(&mut vm).unwrap();
    let released = location.filename.capacity();
    vm.pending_diagnostic_origin = Some(location);
    vm.synchronize_scheduling_usage().unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    frame.handlers.last_mut().unwrap().pending_origin.source = vm.pending_diagnostic_origin.take();
    frame.handlers.retain_native_storage(&mut vm).unwrap();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before - released
    );
}
