// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny linked tables and scalar peers exercise module cell ownership.

use super::*;
use crate::vm::host_scheduling::{map_slots, SharedSchedulingCharge};

fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
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
fn install(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), module(source))]));
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
}

#[cfg_attr(test, test)]
fn module_cell_ownership_completed_record_retains_its_table_charge() {
    let mut vm = bounded_vm();
    install(&mut vm, "export let answer=42;").unwrap();
    let record = vm.linked_record("entry").unwrap();
    let bytes = map_slots(&record.cells);
    assert!(bytes > 0);
    assert_eq!(vm.linked_module_cell_bytes.get(), bytes);
}

#[cfg_attr(test, test)]
fn module_cell_ownership_pending_record_and_frame_have_independent_capacity() {
    let mut vm = bounded_vm();
    vm.execute_script(&script("globalThis.gate=Promise.withResolvers();0;"))
        .unwrap();
    install(&mut vm, "export let answer=1;await gate.promise;answer=42;").unwrap();
    let record = vm.linked_record("entry").unwrap();
    let frame = vm.module_continuations.values().next().unwrap();
    let minimum =
        map_slots(&record.cells) + frame.execution.control_bytes() + frame.module.capacity();
    let charged = vm.displaced_execution_control_bytes.get() + vm.linked_module_cell_bytes.get();
    assert!(charged >= minimum, "{charged} < {minimum}");
}

#[cfg_attr(test, test)]
fn module_cell_ownership_table_refusal_precedes_managed_cell_allocation() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context(
        "entry",
        HashMap::from([("entry".into(), module("export let answer=42;"))]),
    );
    let snapshot = vm.module_registry.clone();
    let _peer = leave_room(&mut vm, 0);
    vm.heap.allow_only(0);
    assert_eq!(
        vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation),
        Err(RuntimeError::SchedulingLimit)
    );
}

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&script(source)).unwrap()
}
fn retained_cells(vm: &Vm) -> usize {
    vm.module_graph
        .as_ref()
        .unwrap()
        .linked
        .values()
        .map(|record| map_slots(&record.cells))
        .sum()
}
fn pause(vm: &mut Vm) {
    let code = module("export let answer=1;answer=42;");
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

#[cfg_attr(test, test)]
fn module_cell_ownership_preparation_checks_actual_slots_and_refunds_on_drop() {
    let mut probe = HashMap::<usize, ObjectId>::new();
    probe.try_reserve(1).unwrap();
    let bytes = map_slots(&probe);
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = vm.prepare_linked_module_cells(1);
        if accepts {
            let prepared = result.unwrap();
            assert_eq!(vm.linked_module_cell_bytes.get(), bytes);
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
            drop(prepared);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        vm.reconcile_scheduling_ownership();
        assert_eq!(vm.linked_module_cell_bytes.get(), 0);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_cell_ownership_active_entry_transfers_the_table_without_a_second_owner() {
    let mut vm = bounded_vm();
    let seen = Rc::new(std::cell::Cell::new(usize::MAX));
    let output = seen.clone();
    let counter = vm.linked_module_cell_bytes.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    install(&mut vm, "export let answer=1;observe();answer=42;").unwrap();
    assert_eq!(
        seen.get(),
        0,
        "active module cells were counted as a second linked table"
    );
    assert_eq!(vm.linked_module_cell_bytes.get(), retained_cells(&vm));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_cell_ownership_rust_held_graph_keeps_its_charge() {
    let mut vm = bounded_vm();
    install(&mut vm, "export let answer=42;").unwrap();
    let expected = vm.host_scheduling_snapshot().payload_bytes;
    let bytes = retained_cells(&vm);
    let graph = vm.module_graph.take().unwrap();
    assert_eq!(vm.linked_module_cell_bytes.get(), bytes);
    assert!(!graph.linked["entry"].cells.is_empty());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, expected);
    vm.module_graph = Some(graph);
}

#[cfg_attr(test, test)]
fn module_cell_ownership_two_awaits_preserve_import_aliases_through_gc() {
    let mut vm = bounded_vm();
    execute(
        &mut vm,
        "globalThis.gate=Promise.withResolvers();globalThis.other=Promise.withResolvers();0;",
    );
    let modules = HashMap::from([
        (
            "entry".into(),
            module("import {answer} from './dep';export {answer};"),
        ),
        (
            "dep".into(),
            module(
                "export let answer=1;await gate.promise;answer=2;await other.promise;answer=42;",
            ),
        ),
    ]);
    vm.execute_module_graph("entry", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Ok(Value::Number(1.0))
    );
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    execute(&mut vm, "gate.resolve();0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Ok(Value::Number(2.0))
    );
    assert_eq!(vm.linked_module_cell_bytes.get(), retained_cells(&vm));
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    execute(&mut vm, "other.resolve();0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Ok(Value::Number(42.0))
    );
    assert!(vm.module_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.linked_module_cell_bytes.get(), retained_cells(&vm));
}

#[cfg_attr(test, test)]
fn module_cell_ownership_late_activation_attaches_completed_and_pending_tables() {
    for pending in [false, true] {
        let mut vm = Vm::default();
        execute(&mut vm, "globalThis.gate=Promise.withResolvers();0;");
        install(
            &mut vm,
            if pending {
                "export let answer=1;await gate.promise;answer=42;"
            } else {
                "export let answer=42;"
            },
        )
        .unwrap();
        let bytes = retained_cells(&vm);
        let pointer = vm
            .linked_record("entry")
            .unwrap()
            .cells
            .values()
            .next()
            .unwrap() as *const ObjectId;
        assert_eq!(vm.linked_module_cell_bytes.get(), 0);
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        assert_eq!(vm.linked_module_cell_bytes.get(), bytes);
        assert_eq!(
            vm.linked_record("entry")
                .unwrap()
                .cells
                .values()
                .next()
                .unwrap() as *const ObjectId,
            pointer
        );
        if pending {
            execute(&mut vm, "gate.resolve();0;");
            vm.run_promise_jobs_bounded(16).unwrap();
        }
        assert_eq!(vm.linked_module_cell_bytes.get(), retained_cells(&vm));
    }
}

#[cfg_attr(test, test)]
fn module_cell_ownership_activation_refusal_preserves_the_graph_and_terminal_account() {
    let mut vm = Vm::default();
    install(&mut vm, "export let answer=42;").unwrap();
    let bytes = retained_cells(&vm);
    vm.active_scheduling_bytes = MAX_HOST_SCHEDULING_BYTES;
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.linked_module_cell_bytes.get(), bytes);
    assert_eq!(retained_cells(&vm), bytes);
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_cell_ownership_link_failure_releases_only_new_table_owners() {
    let mut vm = bounded_vm();
    install(&mut vm, "export let answer=42;").unwrap();
    let bytes = retained_cells(&vm);
    let roots = vm.module_graph.as_ref().unwrap().roots.clone();
    let snapshot = ModuleRegistry::new(
        HashMap::from([
            ("entry".into(), module("export let answer=42;")),
            (
                "bad".into(),
                module("import {missing} from './entry';export let extra=1;"),
            ),
        ]),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    assert!(matches!(
        vm.execute_module_registry_inner("bad", &snapshot, false, false, ImportPhase::Evaluation),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(vm.linked_module_cell_bytes.get(), bytes);
    assert_eq!(vm.module_graph.as_ref().unwrap().roots, roots);
    assert!(vm.linked_record("bad").is_none());
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    };
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_registry.retained_bytes()
            + snapshot.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_cell_ownership_realm_drop_returns_linked_storage_to_a_peer() {
    let parent = bounded_vm();
    let before = parent.host_scheduling_snapshot().payload_bytes;
    let mut child = Vm {
        host_scheduling: Some(parent.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    };
    child.use_bounded_host_microtask_checkpoints().unwrap();
    install(&mut child, "export let answer=42;").unwrap();
    assert!(parent.host_scheduling_snapshot().payload_bytes >= before + retained_cells(&child));
    drop(child);
    assert_eq!(parent.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_cell_ownership_snapshot_refusal_preserves_active_cells_and_serial() {
    let mut vm = bounded_vm();
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.cells.insert(0, cell);
    let pointer = vm.cells.get(&0).unwrap() as *const ObjectId;
    let promise = vm.new_promise().unwrap();
    let handlers = vm.handler_records(Vec::new());
    let serial = vm.next_module_continuation;
    let _peer = leave_room(&mut vm, 0);
    let (outcome, copy) = vm.stage_module_exit_cells(Ok(InterpreterExit::Await {
        promise,
        pc: 0,
        handlers,
    }));
    assert!(matches!(outcome, Err(RuntimeError::SchedulingLimit)));
    assert!(copy.is_none());
    assert_eq!(vm.cells.get(&0).unwrap() as *const ObjectId, pointer);
    assert_eq!(vm.next_module_continuation, serial);
    assert!(vm.module_continuations.is_empty());
    assert_eq!(vm.linked_module_cell_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_cell_ownership_debugger_copies_remain_owned_across_step_and_completion() {
    let mut vm = bounded_vm();
    pause(&mut vm);
    assert_eq!(vm.linked_module_cell_bytes.get(), retained_cells(&vm));
    assert!(vm.linked_module_cell_bytes.get() > 0);
    vm.step_debugger_module_root_instruction().unwrap();
    assert_eq!(vm.linked_module_cell_bytes.get(), retained_cells(&vm));
    vm.resume_debugger_module_execution().unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Ok(Value::Number(42.0))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.linked_module_cell_bytes.get(), retained_cells(&vm));
}

impl crate::Vm {
    /// Runs finite module-cell contracts in unit and ordinary coverage builds.
    #[doc(hidden)]
    pub fn verify_module_cell_ownership_contracts() {
        module_cell_ownership_completed_record_retains_its_table_charge();
        module_cell_ownership_pending_record_and_frame_have_independent_capacity();
        module_cell_ownership_table_refusal_precedes_managed_cell_allocation();
        module_cell_ownership_preparation_checks_actual_slots_and_refunds_on_drop();
        module_cell_ownership_active_entry_transfers_the_table_without_a_second_owner();
        module_cell_ownership_rust_held_graph_keeps_its_charge();
        module_cell_ownership_two_awaits_preserve_import_aliases_through_gc();
        module_cell_ownership_late_activation_attaches_completed_and_pending_tables();
        module_cell_ownership_activation_refusal_preserves_the_graph_and_terminal_account();
        module_cell_ownership_link_failure_releases_only_new_table_owners();
        module_cell_ownership_realm_drop_returns_linked_storage_to_a_peer();
        module_cell_ownership_snapshot_refusal_preserves_active_cells_and_serial();
        module_cell_ownership_debugger_copies_remain_owned_across_step_and_completion();
    }
}
