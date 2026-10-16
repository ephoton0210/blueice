// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite top-level await/debugger frames retain one immutable code snapshot.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.execute_script(&script("globalThis.gate=Promise.withResolvers();0;"))
        .unwrap();
    vm
}
fn suspend(vm: &mut Vm) {
    vm.set_module_loader_context(
        "entry",
        HashMap::from([(
            "entry".into(),
            module("await gate.promise;export const answer=42;"),
        )]),
    );
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
    assert_eq!(vm.module_continuations.len(), 1);
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_await_keeps_the_installed_code_buffer() {
    let mut vm = bounded_vm();
    suspend(&mut vm);
    let frame = vm.module_continuations.values().next().unwrap();
    assert_eq!(
        frame.code.code.as_ptr(),
        vm.module_registry["entry"].code.as_ptr()
    );
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_debugger_pause_keeps_the_installed_code_buffer() {
    let mut vm = bounded_vm();
    let code = module("export let answer=0;await gate.promise;answer=42;");
    let start = code.module_evaluate_entry.unwrap() as usize;
    let target = code
        .instructions()
        .find(|op| op.offset >= start)
        .unwrap()
        .offset as u32;
    let registry = HashMap::from([("entry".into(), code)]);
    assert!(matches!(
        vm.execute_module_graph_until_debugger_pause("entry", &registry, target)
            .unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    let frame = vm.debugger_module_continuation.as_ref().unwrap();
    assert_eq!(
        frame.code.code.as_ptr(),
        vm.module_registry["entry"].code.as_ptr()
    );
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_pending_frame_retains_the_replaced_registry_charge() {
    let mut vm = bounded_vm();
    suspend(&mut vm);
    let retained = vm.module_registry.retained_bytes();
    vm.set_module_loader_context("", HashMap::new());
    let shared = vm.host_scheduling_snapshot().payload_bytes
        - vm.own_scheduling_usage().payload_bytes
        - vm.module_record_storage_bytes()
        - vm.module_root_storage_bytes();
    assert!(
        shared >= retained,
        "pending frame released its backing code snapshot: {shared} < {retained}"
    );
}

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&script(source)).unwrap()
}
fn install(vm: &mut Vm, source: &str) {
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), module(source))]));
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
}
fn pause(vm: &mut Vm, source: &str) {
    let code = module(source);
    let target = code.module_evaluate_entry.unwrap();
    assert!(matches!(
        vm.execute_module_graph_until_debugger_pause(
            "entry",
            &HashMap::from([("entry".into(), code)]),
            target
        )
        .unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
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
fn module_continuation_ownership_views_select_distinct_code_at_full_scalar_budget() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context(
        "entry",
        HashMap::from([
            ("first".into(), module("1;")),
            ("second".into(), module("2;")),
        ]),
    );
    let peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    for name in ["first", "second"] {
        let code = vm.module_registry.code(name);
        let copy = code.clone();
        assert_eq!(copy.code.as_ptr(), vm.module_registry[name].code.as_ptr());
    }
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    drop(peer);
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_name_exact_budget_and_refusal_preserve_active_fields() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let active = vm.active_module_name.clone();
        let serial = vm.next_module_continuation;
        let _peer = leave_room(&mut vm, 5 - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = vm.prepare_module_frame_name("entry");
        if accepts {
            let (name, charge) = result.unwrap();
            assert_eq!(charge.bytes(), name.capacity());
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                before + name.capacity()
            );
            drop(charge);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        vm.reconcile_scheduling_ownership();
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(vm.active_module_name, active);
        assert_eq!(vm.next_module_continuation, serial);
        assert!(vm.module_continuations.is_empty());
    }
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_repeated_await_keeps_code_and_gc_edges() {
    let mut vm = bounded_vm();
    execute(&mut vm, "globalThis.other=Promise.withResolvers();0;");
    let seen = Rc::new(std::cell::Cell::new(0));
    let output = seen.clone();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    install(&mut vm, "const saved={answer:42};await gate.promise;observe();await other.promise;globalThis.answer=saved.answer;");
    let pointer = vm.module_registry["entry"].code.as_ptr();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    execute(&mut vm, "gate.resolve();0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert!(seen.get() >= 5);
    let frame = vm.module_continuations.values().next().unwrap();
    assert_eq!(frame.code.code.as_ptr(), pointer);
    assert_eq!(frame.module_charge.bytes(), frame.module.capacity());
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    execute(&mut vm, "other.resolve();0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(execute(&mut vm, "globalThis.answer;"), Value::Number(42.0));
    assert!(vm.module_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_loader_replacement_resumes_the_original_program() {
    let mut vm = bounded_vm();
    install(&mut vm, "await gate.promise;globalThis.answer=42;");
    vm.set_module_loader_context(
        "entry",
        HashMap::from([("entry".into(), module("globalThis.answer=99;"))]),
    );
    let latest = vm.module_registry.retained_bytes();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    };
    execute(&mut vm, "gate.resolve();0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
            + latest
    );
    assert_eq!(execute(&mut vm, "globalThis.answer;"), Value::Number(42.0));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
            + latest
    );
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_debugger_step_then_await_preserves_code_and_name() {
    let mut vm = bounded_vm();
    pause(&mut vm, "await gate.promise;globalThis.answer=42;");
    let pointer = vm.module_registry["entry"].code.as_ptr();
    assert!(matches!(
        vm.step_debugger_module_root_instruction().unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    assert_eq!(
        vm.debugger_module_continuation
            .as_ref()
            .unwrap()
            .code
            .code
            .as_ptr(),
        pointer
    );
    vm.resume_debugger_module_execution().unwrap();
    let frame = vm.module_continuations.values().next().unwrap();
    assert_eq!(frame.code.code.as_ptr(), pointer);
    assert_eq!(frame.module_charge.bytes(), frame.module.capacity());
    execute(&mut vm, "gate.resolve();0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(execute(&mut vm, "globalThis.answer;"), Value::Number(42.0));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_late_activation_attaches_a_displaced_snapshot() {
    let mut vm = Vm::default();
    execute(&mut vm, "globalThis.gate=Promise.withResolvers();0;");
    suspend(&mut vm);
    let retained = vm.module_registry.retained_bytes();
    vm.set_module_loader_context("", HashMap::new());
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
            + retained
    );
    execute(&mut vm, "gate.resolve();0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert!(vm.module_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_late_debugger_activation_retains_and_refunds_the_frame() {
    let mut vm = Vm::default();
    pause(&mut vm, "globalThis.answer=42;");
    let retained = vm.module_registry.retained_bytes();
    vm.set_module_loader_context("", HashMap::new());
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let frame = vm.debugger_module_continuation.as_ref().unwrap();
    assert!(
        vm.own_scheduling_usage().payload_bytes
            >= frame.module.capacity() + frame.execution.control_bytes()
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
            + retained
    );
    vm.resume_debugger_module_execution().unwrap();
    assert_eq!(execute(&mut vm, "globalThis.answer;"), Value::Number(42.0));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_rejected_await_refunds_the_old_snapshot() {
    let mut vm = bounded_vm();
    install(
        &mut vm,
        "try {await gate.promise;} catch(error) {globalThis.answer=error;}",
    );
    vm.set_module_loader_context("", HashMap::new());
    execute(&mut vm, "gate.reject(7);0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(execute(&mut vm, "globalThis.answer;"), Value::Number(7.0));
    assert!(vm.module_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_registration_refusal_releases_code_and_name_without_a_serial() {
    let mut vm = bounded_vm();
    suspend(&mut vm);
    vm.set_module_loader_context("", HashMap::new());
    let serial = *vm.module_continuations.keys().next().unwrap();
    let frame = vm.module_continuations.remove(&serial).unwrap();
    let next = vm.next_module_continuation;
    let promise = vm.new_promise().unwrap();
    let peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.suspend_module_await(frame, promise),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.next_module_continuation, next);
    assert!(vm.module_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(peer);
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_continuation_ownership_terminal_debugger_cleanup_refunds_the_old_snapshot() {
    let mut vm = bounded_vm();
    pause(&mut vm, "globalThis.answer=42;");
    vm.set_module_loader_context("", HashMap::new());
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.resume_debugger_module_execution(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.debugger_module_continuation.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
    );
}

impl crate::Vm {
    /// Runs finite module-continuation contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_module_continuation_ownership_contracts() {
        module_continuation_ownership_await_keeps_the_installed_code_buffer();
        module_continuation_ownership_debugger_pause_keeps_the_installed_code_buffer();
        module_continuation_ownership_pending_frame_retains_the_replaced_registry_charge();
        module_continuation_ownership_views_select_distinct_code_at_full_scalar_budget();
        module_continuation_ownership_name_exact_budget_and_refusal_preserve_active_fields();
        module_continuation_ownership_repeated_await_keeps_code_and_gc_edges();
        module_continuation_ownership_loader_replacement_resumes_the_original_program();
        module_continuation_ownership_debugger_step_then_await_preserves_code_and_name();
        module_continuation_ownership_late_activation_attaches_a_displaced_snapshot();
        module_continuation_ownership_late_debugger_activation_retains_and_refunds_the_frame();
        module_continuation_ownership_rejected_await_refunds_the_old_snapshot();
        module_continuation_ownership_registration_refusal_releases_code_and_name_without_a_serial(
        );
        module_continuation_ownership_terminal_debugger_cleanup_refunds_the_old_snapshot();
    }
}
