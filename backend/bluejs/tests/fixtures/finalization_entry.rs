// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny cleanup cells, finite callbacks and scalar allowances check entry.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use std::cell::Cell;

fn fixture(holdings: Value) -> (Vm, ObjectId, Rc<Cell<usize>>) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    vm.install_host_function("recordCleanup", 1, move |_args: &[HostValue]| {
        observed.set(observed.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("recordCleanup").unwrap().unwrap();
    let registry = queued_registry(&mut vm, callback, holdings);
    (vm, registry, calls)
}
fn queued_registry(vm: &mut Vm, callback: Value, holdings: Value) -> ObjectId {
    let registry = vm.heap.alloc_finalization_registry(callback, None).unwrap();
    vm.heap.root(registry).unwrap();
    for _ in 0..2 {
        let target = vm.heap.alloc_object(None).unwrap();
        vm.heap
            .finalization_registry_register(registry, Value::Object(target), holdings.clone(), None)
            .unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.enqueue_host_finalization_tasks().unwrap();
    vm.remaining_instructions = 73;
    registry
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
fn finalization_entry_root_refusal_keeps_the_cell_and_task() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.async_frame_roots = Vec::new();
    vm.synchronize_scheduling_usage().unwrap();
    let cells = vm.heap.finalization_cell_storage_state(registry);
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), cells);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn finalization_entry_payload_refusal_keeps_the_cell_and_task() {
    let (mut vm, registry, calls) = fixture(Value::String("tiny".into()));
    vm.prepare_async_frame_root_slots(2).unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let cells = vm.heap.finalization_cell_storage_state(registry);
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), cells);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn finalization_entry_zero_starting_fuel_keeps_the_cell_and_task() {
    let (mut vm, registry, calls) = fixture(Value::Number(1.0));
    vm.prepare_async_frame_root_slots(2).unwrap();
    vm.config.instruction_budget = 0;
    let cells = vm.heap.finalization_cell_storage_state(registry);
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), cells);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(calls.get(), 0);
}

fn argument_bytes() -> usize {
    std::mem::size_of::<Value>()
}
fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    }
}
fn primitive_counters(vm: &Vm) -> (usize, usize) {
    (
        vm.displaced_execution_bytes.get(),
        vm.displaced_execution_control_bytes.get(),
    )
}

#[cfg_attr(test, test)]
fn finalization_entry_argument_refusal_preserves_task_roots_and_cell() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.prepare_async_frame_root_slots(2).unwrap();
    let roots = vm.heap.root_storage_state();
    let cells = vm.heap.finalization_cell_storage_state(registry);
    let _peer = leave_room(&mut vm, argument_bytes() - 1);
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), cells);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(primitive_counters(&vm), (0, 0));
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn finalization_entry_root_replacement_refusal_preserves_the_ambient_prefix() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.extend_async_frame_roots([vm.object_prototype; 4].into_iter())
        .unwrap();
    let pointer = vm.async_frame_roots.as_ptr();
    let cells = vm.heap.finalization_cell_storage_state(registry);
    let _peer = leave_room(
        &mut vm,
        argument_bytes() + 4 * std::mem::size_of::<ObjectId>(),
    );
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.async_frame_roots, [vm.object_prototype; 4]);
    assert_eq!(vm.async_frame_roots.as_ptr(), pointer);
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), cells);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn finalization_entry_exact_preparation_allowance_keeps_the_original_owners() {
    let (mut vm, registry, calls) = fixture(Value::String("tiny".into()));
    vm.prepare_async_frame_root_slots(2).unwrap();
    let cells = vm.heap.finalization_cell_storage_state(registry);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 8 + argument_bytes());
    let prepared = vm.prepare_finalization_entry(registry).unwrap().unwrap();
    assert!(prepared.arguments.is_empty());
    assert_eq!(
        host_scheduling::slots(&prepared.arguments),
        argument_bytes()
    );
    assert_eq!(primitive_counters(&vm), (0, 0));
    assert_eq!(
        (prepared.payload.bytes(), prepared.control.bytes()),
        (8, argument_bytes())
    );
    assert!(prepared.payload.is_retained() && prepared.control.is_retained());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), cells);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(primitive_counters(&vm), (0, 0));
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn finalization_entry_partial_preparation_refunds_argument_and_payload_leases() {
    let (mut vm, registry, _) = fixture(Value::String("tiny".into()));
    let observed = observer(&vm);
    vm.async_frame_roots = Vec::new();
    vm.synchronize_scheduling_usage().unwrap();
    let room = 8 + argument_bytes();
    let _peer = leave_room(&mut vm, room);
    assert!(matches!(
        vm.prepare_finalization_entry(registry),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(primitive_counters(&vm), (0, 0));
    assert_eq!(vm.async_frame_roots.capacity(), 0);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 2);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
}

#[cfg_attr(test, test)]
fn finalization_entry_zero_fuel_refuses_before_allocating_at_full_scalar_quota() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.async_frame_roots = Vec::new();
    vm.config.instruction_budget = 0;
    vm.synchronize_scheduling_usage().unwrap();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.async_frame_roots.capacity(), 0);
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 2);
    assert_eq!(primitive_counters(&vm), (0, 0));
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(calls.get(), 0);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn finalization_entry_zero_fuel_retry_delivers_the_retained_attempt_once() {
    let (mut vm, registry, calls) = fixture(Value::Number(1.0));
    let budget = vm.config.instruction_budget;
    vm.config.instruction_budget = 0;
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::InstructionLimit)
    );
    vm.config.instruction_budget = budget;
    vm.run_next_host_finalization_task().unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 1);
}

#[cfg_attr(test, test)]
fn finalization_entry_existing_document_failure_precedes_fuel_validation() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.config.instruction_budget = 0;
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 2);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn finalization_entry_refusal_preserves_active_registry_context() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.active_finalization_registry = Some(vm.object_prototype);
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.active_finalization_registry, Some(vm.object_prototype));
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 2);
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn finalization_entry_success_consumes_one_cell_and_invokes_one_callback() {
    let (mut vm, registry, calls) = fixture(Value::Number(1.0));
    vm.run_next_host_finalization_task().unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 1);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert!(vm.active_finalization_registry.is_none());
    assert!(vm.async_frame_roots.is_empty());
    assert_eq!(primitive_counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn finalization_entry_repeated_delivery_never_repeats_a_consumed_cell() {
    let (mut vm, registry, calls) = fixture(Value::Number(1.0));
    vm.run_next_host_finalization_task().unwrap();
    vm.run_next_host_finalization_task().unwrap();
    vm.run_next_host_finalization_task().unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), (0, 0));
    assert!(vm.host_finalization_tasks.is_empty());
    assert_eq!(primitive_counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn finalization_entry_argument_ownership_is_handed_to_dispatch_once() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let payload = vm.displaced_execution_bytes.clone();
    let control = vm.displaced_execution_control_bytes.clone();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let output = recorded.clone();
    vm.install_host_function("capture", 1, move |_args: &[HostValue]| {
        output.borrow_mut().push((payload.get(), control.get()));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("capture").unwrap().unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    vm.call_native(
        callback.clone(),
        Value::Undefined,
        vec![Value::String("tiny".into())],
        false,
    )
    .unwrap();
    queued_registry(&mut vm, callback, Value::String("tiny".into()));
    vm.run_next_host_finalization_task().unwrap();
    let recorded = recorded.borrow();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].0, 8);
    assert!(recorded[0].1 >= argument_bytes());
    assert_eq!(
        recorded[1], recorded[0],
        "cleanup keeps ordinary single-call ownership"
    );
    assert_eq!(primitive_counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn finalization_entry_success_restores_root_prefix_and_registry_context() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.extend_async_frame_roots([vm.object_prototype; 3].into_iter())
        .unwrap();
    vm.active_finalization_registry = Some(vm.object_prototype);
    vm.run_next_host_finalization_task().unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(vm.async_frame_roots, [vm.object_prototype; 3]);
    assert_eq!(vm.active_finalization_registry, Some(vm.object_prototype));
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 1);
}

fn object_fixture(source: &str) -> (Vm, ObjectId, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let callback = vm.execute_script(&script(source)).unwrap();
    let holding = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.set(holding, "marker", Value::Number(42.0)).unwrap();
    vm.stack.push(Value::Object(holding));
    let registry = queued_registry(&mut vm, callback, Value::Object(holding));
    vm.stack.pop();
    (vm, registry, holding)
}

#[cfg_attr(test, test)]
fn finalization_entry_object_holdings_survive_callback_collection_then_release() {
    let (mut vm, registry, holding) =
        object_fixture("(holding)=>{globalThis.observed=holding.marker;};");
    let before = vm.heap.stats().major_collections;
    vm.next_host_native_gc_bytes = 0;
    vm.run_next_host_finalization_task().unwrap();
    assert!(vm.heap.stats().major_collections > before);
    assert_eq!(
        vm.lookup_global_name("observed").unwrap(),
        Some(Value::Number(42.0))
    );
    assert!(vm.heap.contains(holding));
    vm.run_next_host_finalization_task().unwrap();
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 0);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(holding));
}

#[cfg_attr(test, test)]
fn finalization_entry_ignored_object_return_does_not_keep_holdings_alive() {
    let (mut vm, _, holding) = object_fixture("(holding)=>holding;");
    vm.run_next_host_finalization_task().unwrap();
    vm.run_next_host_finalization_task().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(holding));
    assert!(vm.async_frame_roots.is_empty());
}

#[cfg_attr(test, test)]
fn finalization_entry_attempted_dispatch_failure_consumes_only_its_cell() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.config.instruction_budget = 1;
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 1);
    assert!(vm.host_finalization_tasks.is_empty());
    assert!(vm.async_frame_roots.is_empty());
    assert!(vm.active_finalization_registry.is_none());
    assert_eq!(primitive_counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn finalization_entry_empty_task_needs_no_argument_storage_or_instruction() {
    let (mut vm, registry, calls) = fixture(Value::Undefined);
    vm.heap
        .take_finalization_registry_cleanup_cell(registry)
        .unwrap()
        .unwrap();
    vm.heap
        .take_finalization_registry_cleanup_cell(registry)
        .unwrap()
        .unwrap();
    vm.async_frame_roots = Vec::new();
    vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut vm, 0);
    vm.run_next_host_finalization_task().unwrap();
    assert!(vm.host_finalization_tasks.is_empty());
    assert_eq!(vm.async_frame_roots.capacity(), 0);
    assert_eq!(calls.get(), 0);
    assert_eq!(primitive_counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn finalization_entry_borrowed_peek_validates_the_registry_and_keeps_data_owned() {
    let (mut vm, registry, _) = fixture(Value::String("tiny".into()));
    let cells = vm.heap.finalization_cell_storage_state(registry);
    let (callback, holdings) = vm
        .heap
        .peek_finalization_registry_cleanup_cell(registry)
        .unwrap()
        .unwrap();
    assert!(callback.object_id().is_some());
    assert_eq!(holdings, &Value::String("tiny".into()));
    assert_eq!(vm.heap.finalization_cell_storage_state(registry), cells);
    let object = vm.heap.alloc_object(None).unwrap();
    assert!(
        matches!(vm.heap.peek_finalization_registry_cleanup_cell(object), Err(HeapError::InvalidInternalSlot(id)) if id == object)
    );
    let foreign = Heap::default().alloc_object(None).unwrap();
    assert!(
        matches!(vm.heap.peek_finalization_registry_cleanup_cell(foreign), Err(HeapError::InvalidObject(id)) if id == foreign)
    );
}

#[cfg_attr(test, test)]
fn finalization_entry_prepared_drop_refunds_native_owners_after_terminal_failure() {
    let (mut vm, registry, _) = fixture(Value::String("tiny".into()));
    let observed = observer(&vm);
    let prepared = vm.prepare_finalization_entry(registry).unwrap().unwrap();
    let bytes = 8 + host_scheduling::slots(&prepared.arguments);
    let _peer = leave_room(&mut vm, 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
    assert_eq!(primitive_counters(&vm), (0, 0));
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 2);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
}

#[cfg_attr(test, test)]
fn finalization_entry_exact_root_preparation_keeps_reusable_capacity_owned() {
    let (mut vm, registry, _) = fixture(Value::Undefined);
    vm.async_frame_roots = Vec::new();
    vm.synchronize_scheduling_usage().unwrap();
    let roots = 4 * std::mem::size_of::<ObjectId>();
    let _peer = leave_room(&mut vm, argument_bytes() + roots);
    let prepared = vm.prepare_finalization_entry(registry).unwrap().unwrap();
    assert_eq!(vm.async_frame_roots.capacity(), 4);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - argument_bytes()
    );
    assert_eq!(vm.heap.finalization_cell_storage_state(registry).0, 2);
    assert_eq!(vm.host_finalization_tasks.len(), 1);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_finalization_entry_contracts() {
        finalization_entry_root_refusal_keeps_the_cell_and_task();
        finalization_entry_payload_refusal_keeps_the_cell_and_task();
        finalization_entry_zero_starting_fuel_keeps_the_cell_and_task();
        finalization_entry_argument_refusal_preserves_task_roots_and_cell();
        finalization_entry_root_replacement_refusal_preserves_the_ambient_prefix();
        finalization_entry_exact_preparation_allowance_keeps_the_original_owners();
        finalization_entry_partial_preparation_refunds_argument_and_payload_leases();
        finalization_entry_zero_fuel_refuses_before_allocating_at_full_scalar_quota();
        finalization_entry_zero_fuel_retry_delivers_the_retained_attempt_once();
        finalization_entry_existing_document_failure_precedes_fuel_validation();
        finalization_entry_refusal_preserves_active_registry_context();
        finalization_entry_success_consumes_one_cell_and_invokes_one_callback();
        finalization_entry_repeated_delivery_never_repeats_a_consumed_cell();
        finalization_entry_argument_ownership_is_handed_to_dispatch_once();
        finalization_entry_success_restores_root_prefix_and_registry_context();
        finalization_entry_object_holdings_survive_callback_collection_then_release();
        finalization_entry_ignored_object_return_does_not_keep_holdings_alive();
        finalization_entry_attempted_dispatch_failure_consumes_only_its_cell();
        finalization_entry_empty_task_needs_no_argument_storage_or_instruction();
        finalization_entry_borrowed_peek_validates_the_registry_and_keeps_data_owned();
        finalization_entry_prepared_drop_refunds_native_owners_after_terminal_failure();
        finalization_entry_exact_root_preparation_keeps_reusable_capacity_owned();
    }
}
