// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny borrowed roots and owned debugger snapshots follow native lifetime.

use super::*;
use crate::vm::host_scheduling::{owned_code_bytes, SharedSchedulingCharge};

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 10_000;
    vm
}
fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
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
fn root_code_storage_borrowed_buffers_stay_charged_during_guest_calls() {
    let mut vm = page();
    let view = observer(&vm);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output
            .borrow_mut()
            .push(view.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let code = script("observe();42;");
    let mut padded = code.clone();
    padded.code.reserve_exact(1024);
    let expected = owned_code_bytes(&padded) - owned_code_bytes(&code);
    vm.execute_script(&code).unwrap();
    seen.borrow_mut().clear();
    vm.execute_script(&code).unwrap();
    vm.execute_script(&padded).unwrap();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1] - seen[0], expected);
}

#[cfg_attr(test, test)]
fn root_code_storage_paused_code_outlives_its_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let code = script("1;");
    vm.execute_script_until_debugger_pause(&code, 0).unwrap();
    let frame = vm.debugger_continuation.take().unwrap();
    let bytes = owned_code_bytes(&frame.code);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= bytes);
    drop(frame);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn root_code_storage_snapshot_refuses_before_cloning_or_publication() {
    let mut vm = page();
    let code = script("0;");
    vm.prepare_root_execution(&code, false).unwrap();
    let _peer = leave_room(&mut vm, 0);
    let outcome = vm.run_debugger_script(&code, Some(InterpreterSuspensionPoint::Offset(0)));
    assert!(matches!(
        outcome,
        DebuggerRunOutcome::Completed(Err(RuntimeError::SchedulingLimit))
    ));
    assert!(vm.debugger_continuation.is_none());
}

fn installed(source: &str) -> Bytecode {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            crate::BlueJsSourceIdentity::new("page:///root-code.js", "sha256:root-code").unwrap(),
            &crate::BlueJsProgramV1::Script(crate::parse(source).unwrap()),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

#[cfg_attr(test, test)]
fn root_code_storage_borrowed_lease_has_exact_and_one_short_admission() {
    let code = script("42;");
    let bytes = owned_code_bytes(&code);
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let result = BorrowedRootCode::new(&code, vm.host_scheduling.as_ref());
        if accepts {
            drop(result.unwrap());
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn root_code_storage_borrowed_lease_can_outlive_its_vm() {
    let vm = page();
    let view = observer(&vm);
    let code = script("42;");
    let loan = BorrowedRootCode::new(&code, vm.host_scheduling.as_ref()).unwrap();
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        owned_code_bytes(&code)
    );
    drop(loan);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn root_code_storage_return_and_throw_release_the_borrowed_lease() {
    let mut vm = page();
    let success = script("42;");
    let thrown = script("throw 3;");
    vm.execute_script(&success).unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(
        vm.execute_script(&thrown),
        Err(RuntimeError::Thrown(Value::Number(3.0)))
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    vm.execute_script(&success).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn root_code_storage_snapshot_peak_has_exact_and_one_short_admission() {
    let mut vm = page();
    let code = script("42;");
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (sample, peak) = scope.measure_payload_peak(|| SavedRootCode::copy(&code, &mut vm));
    drop(sample.unwrap());
    for accepts in [false, true] {
        let mut vm = page();
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = SavedRootCode::copy(&code, &mut vm);
        if accepts {
            drop(result.unwrap());
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
    }
}

#[cfg_attr(test, test)]
fn root_code_storage_snapshot_owns_its_children_and_source_after_vm_drop() {
    let mut vm = page();
    let view = observer(&vm);
    let original = script("function kept(){return 1;}42;");
    let saved = SavedRootCode::copy(&original, &mut vm).unwrap();
    assert!(saved.functions[0].is_runtime());
    let bytes = saved.retained_bytes()
        + saved.functions[0].retained_bytes()
        + saved.functions[0].runtime_source_bytes();
    assert!(saved.functions[0].runtime_source_bytes() > 0);
    drop(original);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(saved);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn root_code_storage_late_activation_attaches_paused_code_once() {
    let mut vm = Vm::default();
    let code = script("function kept(){return 1;}42;");
    vm.execute_script_until_debugger_pause(&code, 0).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.debugger_continuation.as_ref().unwrap().code.functions[0].is_runtime());
    assert_eq!(
        vm.resume_debugger_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
}

#[cfg_attr(test, test)]
fn root_code_storage_foreign_snapshot_attachment_refuses_without_rebinding() {
    let mut owner = page();
    let code = script("42;");
    let mut saved = SavedRootCode::copy(&code, &mut owner).unwrap();
    let mut other = page();
    let budget = other.host_scheduling.as_ref().unwrap().heap_native_budget();
    let pointer = saved.code.as_ptr();
    assert_eq!(
        saved.attach(&budget, &mut other.heap),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(saved.code.as_ptr(), pointer);
    assert!(owner.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn root_code_storage_terminal_snapshot_copy_refuses_before_allocation() {
    let mut vm = page();
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        SavedRootCode::copy(&script("42;"), &mut vm).err(),
        Some(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn root_code_storage_root_refusal_prevents_guest_side_effects() {
    let mut vm = page();
    let seen = Rc::new(std::cell::Cell::new(0));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(output.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let code = script("observe();42;");
    vm.execute_script(&code).unwrap();
    seen.set(0);
    let _peer = leave_room(&mut vm, owned_code_bytes(&code) - 1);
    assert_eq!(vm.execute_script(&code), Err(RuntimeError::SchedulingLimit));
    assert_eq!(seen.get(), 0);
}

#[cfg_attr(test, test)]
fn root_code_storage_prepared_code_uses_its_existing_root_charge() {
    let mut vm = page();
    let view = observer(&vm);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output
            .borrow_mut()
            .push(view.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let prepared = crate::vm::eval_compilation_storage::ParsedEvalInput::parse_script(
        "observe();42;",
        vm.host_scheduling.as_ref(),
    )
    .unwrap()
    .compile_with(vm.host_scheduling.as_ref(), crate::compile)
    .unwrap();
    let host = script("observe();42;");
    assert_eq!(owned_code_bytes(&prepared), owned_code_bytes(&host));
    vm.execute_prepared_script(&prepared).unwrap();
    seen.borrow_mut().clear();
    vm.execute_prepared_script(&prepared).unwrap();
    vm.execute_script(&host).unwrap();
    let seen = seen.borrow();
    assert_eq!(seen[1] - seen[0], owned_code_bytes(&host));
}

#[cfg_attr(test, test)]
fn root_code_storage_step_and_resume_move_the_same_saved_buffers() {
    let mut vm = page();
    let code = script("1+2;");
    vm.execute_script_until_debugger_pause(&code, 0).unwrap();
    let pointer = vm
        .debugger_continuation
        .as_ref()
        .unwrap()
        .code
        .code
        .as_ptr();
    assert!(matches!(
        vm.step_debugger_root_instruction().unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    assert_eq!(
        vm.debugger_continuation
            .as_ref()
            .unwrap()
            .code
            .code
            .as_ptr(),
        pointer
    );
    assert_eq!(
        vm.resume_debugger_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
    assert!(vm.debugger_continuation.is_none());
}

#[cfg_attr(test, test)]
fn root_code_storage_resume_throw_releases_every_saved_code_owner() {
    let mut vm = page();
    let code = script("throw 3;");
    vm.execute_script_until_debugger_pause(&code, 0).unwrap();
    assert_eq!(
        vm.resume_debugger_execution(),
        Err(RuntimeError::Thrown(Value::Number(3.0)))
    );
    assert!(vm.debugger_continuation.is_none() && vm.debugger_nested_continuation.is_none());
    assert_eq!(
        vm.execute_script(&script("42;")).unwrap(),
        Value::Number(42.0)
    );
}

#[cfg_attr(test, test)]
fn root_code_storage_nested_and_intermediate_code_outlive_vm_teardown() {
    for intermediate in [false, true] {
        let mut vm = page();
        let view = observer(&vm);
        let code = installed(if intermediate {
            "function inner(){return 1;}function middle(){return inner();}middle();"
        } else {
            "function inner(){return 1;}inner();"
        });
        assert!(matches!(
            vm.execute_script_until_nested_debugger_pause(&code, 1, 0)
                .unwrap(),
            VmDebuggerNestedExecutionState::Paused { .. }
        ));
        let root = vm.debugger_continuation.take().unwrap();
        let child = vm.debugger_nested_continuation.take().unwrap();
        let middle = vm.debugger_nested_intermediate_continuation.take();
        assert_eq!(middle.is_some(), intermediate);
        let minimum = root.code.retained_bytes()
            + child.code.retained_bytes()
            + middle
                .as_ref()
                .map_or(0, |frame| frame.code.retained_bytes());
        drop(vm);
        assert!(view.host_scheduling_snapshot().payload_bytes >= minimum);
        drop(root);
        drop(child);
        drop(middle);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    }
}

#[cfg_attr(test, test)]
fn root_code_storage_nested_steps_preserve_the_saved_code_allocation() {
    let mut vm = page();
    let code = installed("function inner(){return 42;}inner();");
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&code, 1, 0)
        .unwrap()
    else {
        panic!("child pause");
    };
    let pointer = vm
        .debugger_nested_continuation
        .as_ref()
        .unwrap()
        .code
        .code
        .as_ptr();
    assert!(matches!(
        vm.step_debugger_nested_instruction(frame_serial).unwrap(),
        VmDebuggerNestedExecutionState::Paused { .. }
    ));
    assert_eq!(
        vm.debugger_nested_continuation
            .as_ref()
            .unwrap()
            .code
            .code
            .as_ptr(),
        pointer
    );
    vm.resume_debugger_nested_execution(frame_serial).unwrap();
    vm.resume_debugger_execution().unwrap();
}

#[cfg_attr(test, test)]
fn root_code_storage_nested_refusal_cleans_requests_and_snapshots() {
    let mut vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let held = Rc::new(RefCell::new(None));
    let output = held.clone();
    vm.install_host_function("cap", 0, move |_: &[HostValue]| {
        let used = view.host_scheduling_snapshot().payload_bytes;
        *output.borrow_mut() = Some(
            scope
                .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - 512)
                .unwrap(),
        );
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let code = installed("function inner(){return 42;}cap();inner();");
    assert_eq!(
        vm.execute_script_until_nested_debugger_pause(&code, 1, 0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(
        vm.debugger_continuation.is_none()
            && vm.debugger_nested_continuation.is_none()
            && vm.debugger_nested_intermediate_continuation.is_none()
    );
    assert!(vm.debugger_nested_pause_request.is_none());
    held.borrow_mut().take();
}

#[cfg_attr(test, test)]
fn root_code_storage_execute_and_module_entries_charge_their_borrowed_buffers() {
    for module in [false, true] {
        let mut vm = page();
        let view = observer(&vm);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        vm.install_host_function("observe", 0, move |_: &[HostValue]| {
            output
                .borrow_mut()
                .push(view.host_scheduling_snapshot().payload_bytes);
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let code = if module {
            crate::compile_module(&crate::parse_module("observe();42;").unwrap()).unwrap()
        } else {
            script("observe();42;")
        };
        let mut padded = code.clone();
        padded.code.reserve_exact(1024);
        let expected = owned_code_bytes(&padded) - owned_code_bytes(&code);
        let run = |vm: &mut Vm, code: &Bytecode| {
            if module {
                vm.execute_module(code)
            } else {
                vm.execute(code)
            }
        };
        run(&mut vm, &code).unwrap();
        seen.borrow_mut().clear();
        run(&mut vm, &code).unwrap();
        run(&mut vm, &padded).unwrap();
        let seen = seen.borrow();
        assert_eq!(seen[1] - seen[0], expected);
    }
}

#[cfg_attr(test, test)]
fn root_code_storage_paused_entry_refusal_keeps_existing_code_and_pc() {
    let mut vm = page();
    let code = script("42;");
    vm.execute_script_until_debugger_pause(&code, 0).unwrap();
    let pointer = vm
        .debugger_continuation
        .as_ref()
        .unwrap()
        .code
        .code
        .as_ptr();
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.execute_script(&code),
        Err(RuntimeError::Unsupported(_))
    ));
    assert_eq!(
        vm.debugger_continuation
            .as_ref()
            .unwrap()
            .code
            .code
            .as_ptr(),
        pointer
    );
    assert_eq!(vm.debugger_continuation.as_ref().unwrap().pc, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_root_code_storage_contracts() {
        root_code_storage_borrowed_buffers_stay_charged_during_guest_calls();
        root_code_storage_paused_code_outlives_its_vm();
        root_code_storage_snapshot_refuses_before_cloning_or_publication();
        root_code_storage_borrowed_lease_has_exact_and_one_short_admission();
        root_code_storage_borrowed_lease_can_outlive_its_vm();
        root_code_storage_return_and_throw_release_the_borrowed_lease();
        root_code_storage_snapshot_peak_has_exact_and_one_short_admission();
        root_code_storage_snapshot_owns_its_children_and_source_after_vm_drop();
        root_code_storage_late_activation_attaches_paused_code_once();
        root_code_storage_foreign_snapshot_attachment_refuses_without_rebinding();
        root_code_storage_terminal_snapshot_copy_refuses_before_allocation();
        root_code_storage_root_refusal_prevents_guest_side_effects();
        root_code_storage_prepared_code_uses_its_existing_root_charge();
        root_code_storage_step_and_resume_move_the_same_saved_buffers();
        root_code_storage_resume_throw_releases_every_saved_code_owner();
        root_code_storage_nested_and_intermediate_code_outlive_vm_teardown();
        root_code_storage_nested_steps_preserve_the_saved_code_allocation();
        root_code_storage_nested_refusal_cleans_requests_and_snapshots();
        root_code_storage_execute_and_module_entries_charge_their_borrowed_buffers();
        root_code_storage_paused_entry_refusal_keeps_existing_code_and_pc();
    }
}
