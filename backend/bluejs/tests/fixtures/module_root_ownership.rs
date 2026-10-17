// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite module graphs, scalar quotas and tiny root-registration failures.

use super::*;
use crate::vm::host_scheduling::{map_slots, SharedSchedulingCharge};

fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
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
fn module_root_ownership_retained_root_slots_have_native_ownership() {
    let mut vm = bounded_vm();
    install(&mut vm, "export let answer=42;").unwrap();
    let roots = &vm.module_graph.as_ref().unwrap().roots;
    assert!(!roots.is_empty());
    let extra = vm.host_scheduling_snapshot().payload_bytes
        - vm.own_scheduling_usage().payload_bytes
        - vm.module_registry.retained_bytes()
        - vm.module_record_storage_bytes();
    assert!(
        extra >= roots.len() * std::mem::size_of::<RootId>(),
        "missing native root storage: {extra}"
    );
}

#[cfg_attr(test, test)]
fn module_root_ownership_linking_root_registration_failure_is_a_runtime_error() {
    let mut vm = bounded_vm();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        install(&mut vm, "export let answer=42;"),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
}

#[cfg_attr(test, test)]
fn module_root_ownership_slot_refusal_precedes_managed_cell_allocation() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context(
        "entry",
        HashMap::from([("entry".into(), module("export let answer=42;"))]),
    );
    let snapshot = vm.module_registry.clone();
    let mut records = HashMap::<String, LinkedModule>::new();
    records.try_reserve(1).unwrap();
    let mut cells = HashMap::<usize, ObjectId>::new();
    cells.try_reserve(1).unwrap();
    let room =
        map_slots(&records) + "entry".len() + std::mem::size_of::<&str>() + map_slots(&cells);
    let _peer = leave_room(&mut vm, room);
    vm.heap.allow_only(0);
    assert_eq!(
        vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_root_ownership_cached_completion_survives_a_later_script_collection() {
    let mut vm = bounded_vm();
    let value = install(&mut vm, "({answer:42});").unwrap();
    let id = value
        .object_id()
        .expect("the module has an object completion");
    vm.execute_script(&script("0;")).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(
        vm.heap.contains(id),
        "the cached module completion lost its root"
    );
    let snapshot = vm.module_registry.clone();
    assert_eq!(
        vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation),
        Ok(value)
    );
}

fn object(vm: &mut Vm) -> ObjectId {
    vm.with_roots(|heap| heap.alloc_object(None)).unwrap()
}
fn segment_bytes(capacity: usize) -> usize {
    std::mem::size_of::<RootSegment>() + capacity * std::mem::size_of::<RootId>()
}

#[cfg_attr(test, test)]
fn module_root_ownership_reservation_exact_budget_refusal_and_drop_are_accounted() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let roots = ModuleRoots::default();
        let _peer = leave_room(&mut vm, segment_bytes(4) - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = roots.prepare_slot(vm.host_scheduling.as_ref());
        if accepts {
            let slot = result.unwrap();
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                before + segment_bytes(4)
            );
            drop(slot);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert!(roots.is_empty());
        assert_eq!(roots.retained_bytes(), 0);
    }
}

#[cfg_attr(test, test)]
fn module_root_ownership_spare_slots_need_no_extra_capacity_at_full_scalar_quota() {
    let mut vm = bounded_vm();
    let id = object(&mut vm);
    let mut roots = ModuleRoots::default();
    roots
        .root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id)
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    roots
        .root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id)
        .unwrap();
    assert_eq!(roots.len(), 2);
    assert_eq!(roots.retained_bytes(), segment_bytes(4));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    vm.release_module_roots_from(&mut roots, 0);
}

#[cfg_attr(test, test)]
fn module_root_ownership_terminal_merge_moves_segments_and_refunds_without_admission() {
    let mut vm = bounded_vm();
    let id = object(&mut vm);
    let mut first = ModuleRoots::default();
    let mut second = ModuleRoots::default();
    first
        .root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id)
        .unwrap();
    second
        .root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id)
        .unwrap();
    let a = first.iter().next().unwrap() as *const RootId;
    let b = second.iter().next().unwrap() as *const RootId;
    let bytes = first.retained_bytes() + second.retained_bytes();
    let _peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.host_scheduling.as_ref().unwrap().fail();
    first.merge(second);
    assert_eq!(first.len(), 2);
    assert_eq!(first.retained_bytes(), bytes);
    assert!(first.iter().any(|root| std::ptr::eq(root, a)));
    assert!(first.iter().any(|root| std::ptr::eq(root, b)));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(matches!(
        first.prepare_slot(vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    ));
    let foreign = bounded_vm();
    assert!(matches!(
        first.attach(foreign.host_scheduling.as_ref().unwrap()),
        Err(RuntimeError::TypeError(_))
    ));
    vm.release_module_roots_from(&mut first, 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
    assert!(matches!(
        first.prepare_slot(None),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_root_ownership_rollback_crosses_segments_and_keeps_the_existing_prefix() {
    let mut vm = bounded_vm();
    let id = object(&mut vm);
    let mut roots = ModuleRoots::default();
    for _ in 0..3 {
        roots
            .root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id)
            .unwrap();
    }
    let prefix = roots.snapshot();
    let bytes = roots.retained_bytes();
    for _ in 0..6 {
        roots
            .root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id)
            .unwrap();
    }
    assert!(roots.retained_bytes() > bytes);
    vm.release_module_roots_from(&mut roots, prefix.len());
    assert_eq!(roots.snapshot(), prefix);
    assert_eq!(roots.retained_bytes(), bytes);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(vm.heap.contains(id));
    vm.release_module_roots_from(&mut roots, 0);
    assert_eq!(roots.retained_bytes(), 0);
}

#[cfg_attr(test, test)]
fn module_root_ownership_registration_failure_discards_only_the_staged_segment() {
    let mut vm = bounded_vm();
    let id = object(&mut vm);
    let mut roots = ModuleRoots::default();
    for _ in 0..4 {
        roots
            .root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id)
            .unwrap();
    }
    let prefix = roots.snapshot();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        roots.root_object(&mut vm.heap, vm.host_scheduling.as_ref(), id),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(roots.snapshot(), prefix);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    vm.release_module_roots_from(&mut roots, 0);
}

#[cfg_attr(test, test)]
fn module_root_ownership_late_activation_attaches_root_segments_once() {
    let mut vm = Vm::default();
    install(&mut vm, "({answer:42});").unwrap();
    let prefix = vm.module_graph.as_ref().unwrap().roots.snapshot();
    let bytes = vm.module_root_storage_bytes();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        before,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_value_storage_bytes()
            + vm.heap.collector_storage_bytes()
            + vm.module_registry.retained_bytes()
            + vm.module_record_storage_bytes()
            + bytes
    );
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.module_graph.as_ref().unwrap().roots.snapshot(), prefix);
}

#[cfg_attr(test, test)]
fn module_root_ownership_debugger_and_await_completions_remain_alive_through_gc() {
    for debugger in [false, true] {
        let mut vm = bounded_vm();
        if debugger {
            let code = module("({answer:42});");
            let target = code.module_evaluate_entry.unwrap();
            vm.execute_module_graph_until_debugger_pause(
                "entry",
                &HashMap::from([("entry".into(), code)]),
                target,
            )
            .unwrap();
            vm.resume_debugger_module_execution().unwrap();
        } else {
            vm.execute_script(&script("globalThis.gate=Promise.withResolvers();0;"))
                .unwrap();
            install(&mut vm, "await gate.promise;({answer:42});").unwrap();
            vm.execute_script(&script("gate.resolve();0;")).unwrap();
            vm.run_promise_jobs_bounded(16).unwrap();
        }
        let value = vm
            .linked_record("entry")
            .unwrap()
            .completion
            .clone()
            .unwrap();
        let id = value.value.object_id().unwrap();
        vm.execute_script(&script("0;")).unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert!(vm.heap.contains(id));
        assert_eq!(
            vm.get_property(&value.value, &"answer".into()),
            Ok(Value::Number(42.0))
        );
    }
}

#[cfg_attr(test, test)]
fn module_root_ownership_failed_await_root_does_not_publish_a_completion() {
    let mut vm = bounded_vm();
    vm.execute_script(&script("globalThis.gate=Promise.withResolvers();0;"))
        .unwrap();
    install(&mut vm, "await gate.promise;({answer:42});").unwrap();
    vm.execute_script(&script("gate.resolve();0;")).unwrap();
    assert_eq!(vm.module_continuations.len(), 1);
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.run_promise_jobs_bounded(16),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.linked_record("entry").unwrap().completion.is_none());
}

impl crate::Vm {
    /// Runs finite module root-storage contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_module_root_ownership_contracts() {
        module_root_ownership_retained_root_slots_have_native_ownership();
        module_root_ownership_linking_root_registration_failure_is_a_runtime_error();
        module_root_ownership_slot_refusal_precedes_managed_cell_allocation();
        module_root_ownership_cached_completion_survives_a_later_script_collection();
        module_root_ownership_reservation_exact_budget_refusal_and_drop_are_accounted();
        module_root_ownership_spare_slots_need_no_extra_capacity_at_full_scalar_quota();
        module_root_ownership_terminal_merge_moves_segments_and_refunds_without_admission();
        module_root_ownership_rollback_crosses_segments_and_keeps_the_existing_prefix();
        module_root_ownership_registration_failure_discards_only_the_staged_segment();
        module_root_ownership_late_activation_attaches_root_segments_once();
        module_root_ownership_debugger_and_await_completions_remain_alive_through_gc();
        module_root_ownership_failed_await_root_does_not_publish_a_completion();
        Self::verify_module_root_synthetic_contracts();
    }
}
