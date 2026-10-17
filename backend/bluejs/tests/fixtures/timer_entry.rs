// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite timer callbacks and scalar allowances verify entry ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn fixture(repeat: bool) -> (Vm, u32, Value, Rc<RefCell<Vec<usize>>>) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_host_timer_functions().unwrap();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let observed = Rc::new(RefCell::new(Vec::new()));
    let output = observed.clone();
    vm.install_host_function("recordTimer", 1, move |_args: &[HostValue]| {
        output
            .borrow_mut()
            .push(observer.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("recordTimer").unwrap().unwrap();
    let function = if repeat {
        host_timers::HostTimerFunction::SetInterval
    } else {
        host_timers::HostTimerFunction::SetTimeout
    };
    vm.host_timer_call(
        function,
        &[callback.clone(), Value::Number(1.0), Value::Number(7.0)],
        false,
    )
    .unwrap();
    vm.remaining_instructions = 73;
    (vm, 1, callback, observed)
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
fn timer_entry_zero_starting_fuel_keeps_one_shot_and_ambient_context() {
    let (mut vm, id, _, observed) = fixture(false);
    let roots = vm.heap.root_storage_state();
    vm.config.instruction_budget = 0;
    vm.host_timer_nesting = 4;
    let mut run = BlueJsTimerRun::default();
    assert_eq!(
        vm.run_host_timer_entry(id, &mut run),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.host_timers.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_timer_nesting, 4);
    assert_eq!(run, BlueJsTimerRun::default());
    assert!(observed.borrow().is_empty());
}

#[cfg_attr(test, test)]
fn timer_entry_zero_starting_fuel_precedes_interval_snapshot_admission() {
    let (mut vm, id, _, observed) = fixture(true);
    vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut vm, 0);
    let mut run = BlueJsTimerRun::default();
    assert_eq!(
        vm.run_host_timer_entry(id, &mut run),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.host_timers.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(run, BlueJsTimerRun::default());
    assert!(observed.borrow().is_empty());
}

#[cfg_attr(test, test)]
fn timer_entry_argument_capacity_transfers_once_into_native_dispatch() {
    let (mut vm, id, callback, observed) = fixture(false);
    vm.call_native(
        callback.clone(),
        Value::Undefined,
        vec![Value::Number(7.0)],
        false,
    )
    .unwrap();
    observed.borrow_mut().clear();
    vm.call_native(callback, Value::Undefined, vec![Value::Number(7.0)], false)
        .unwrap();
    let reference = observed.borrow()[0];
    let queued = host_scheduling::slots(&vm.host_timers) + std::mem::size_of::<Value>();
    let mut run = BlueJsTimerRun::default();
    vm.run_host_timer_entry(id, &mut run).unwrap();
    assert_eq!(observed.borrow()[1], reference - queued);
    assert_eq!(vm.active_scheduling_bytes, 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

fn counters(vm: &Vm) -> (usize, usize) {
    (
        vm.displaced_execution_bytes.get(),
        vm.displaced_execution_control_bytes.get(),
    )
}
fn argument_bytes() -> usize {
    std::mem::size_of::<Value>()
}
fn root_bytes(count: usize) -> usize {
    count * std::mem::size_of::<ObjectId>()
}
fn registration(vm: &Vm) -> (u32, u64, u64, usize) {
    let timer = &vm.host_timers[0];
    (timer.id, timer.due_ms, timer.serial, timer.arguments.len())
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn clear(vm: &mut Vm, id: u32) {
    vm.host_timer_call(
        host_timers::HostTimerFunction::Clear,
        &[Value::Number(f64::from(id))],
        false,
    )
    .unwrap();
}

#[cfg_attr(test, test)]
fn timer_entry_zero_fuel_can_retry_without_losing_its_registration() {
    for repeat in [false, true] {
        let (mut vm, id, _, observed) = fixture(repeat);
        let original = registration(&vm);
        let fuel = vm.config.instruction_budget;
        vm.config.instruction_budget = 0;
        assert_eq!(
            vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default()),
            Err(RuntimeError::InstructionLimit)
        );
        assert_eq!(registration(&vm), original);
        assert!(vm.check_scheduling_failure().is_ok());
        vm.config.instruction_budget = fuel;
        let mut run = BlueJsTimerRun::default();
        vm.run_host_timer_entry(id, &mut run).unwrap();
        assert_eq!(run.tasks_run, 1);
        assert_eq!(observed.borrow().len(), 1);
        clear(&mut vm, id);
    }
}

#[cfg_attr(test, test)]
fn timer_entry_interval_argument_refusal_preserves_the_original_owner() {
    let (mut vm, id, _, observed) = fixture(true);
    let original = registration(&vm);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, argument_bytes() - 1);
    assert_eq!(
        vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(registration(&vm), original);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(counters(&vm), (0, 0));
    assert!(observed.borrow().is_empty());
}

#[cfg_attr(test, test)]
fn timer_entry_interval_payload_refusal_precedes_copy_publication() {
    let (mut vm, _, _, _) = fixture(true);
    vm.host_timers[0].arguments[0] = Value::String("tiny".into());
    vm.synchronize_scheduling_usage().unwrap();
    let _peer = leave_room(&mut vm, argument_bytes() + 7);
    assert!(matches!(
        vm.prepare_timer_entry(0),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.host_timers[0].arguments, [Value::String("tiny".into())]);
    assert_eq!(vm.async_frame_roots.capacity(), 0);
    assert_eq!(counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn timer_entry_interval_exact_preparation_admits_slots_and_object_protection() {
    let (mut vm, _, _, _) = fixture(true);
    let roots = vm.heap.root_storage_state();
    let original = registration(&vm);
    let _peer = leave_room(&mut vm, argument_bytes() + root_bytes(4));
    let prepared = vm.prepare_timer_entry(0).unwrap();
    assert_eq!(prepared.arguments, [Value::Number(7.0)]);
    assert_eq!(counters(&vm), (0, argument_bytes()));
    assert_eq!(vm.async_frame_roots.capacity(), 4);
    assert!(vm.async_frame_roots.is_empty());
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(registration(&vm), original);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(counters(&vm), (0, 0));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - argument_bytes()
    );
}

#[cfg_attr(test, test)]
fn timer_entry_interval_exact_primitive_copy_has_a_separate_owner() {
    let (mut vm, _, _, _) = fixture(true);
    vm.host_timers[0].arguments[0] = Value::String("tiny".into());
    vm.prepare_async_frame_root_slots(1).unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let _peer = leave_room(&mut vm, argument_bytes() + 8);
    let prepared = vm.prepare_timer_entry(0).unwrap();
    assert_eq!(prepared.arguments, [Value::String("tiny".into())]);
    assert_eq!(counters(&vm), (8, argument_bytes()));
    assert_eq!(vm.host_timers[0].arguments, prepared.arguments);
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn timer_entry_interval_root_replacement_refusal_preserves_the_prefix() {
    let (mut vm, id, _, _) = fixture(true);
    vm.extend_async_frame_roots([vm.object_prototype; 4].into_iter())
        .unwrap();
    let pointer = vm.async_frame_roots.as_ptr();
    let original = registration(&vm);
    let _peer = leave_room(&mut vm, argument_bytes() + root_bytes(4));
    assert_eq!(
        vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.async_frame_roots, [vm.object_prototype; 4]);
    assert_eq!(vm.async_frame_roots.as_ptr(), pointer);
    assert_eq!(registration(&vm), original);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn timer_entry_interval_partial_preparation_refunds_candidate_ownership() {
    let (mut vm, _, _, _) = fixture(true);
    let room = argument_bytes();
    let _peer = leave_room(&mut vm, room);
    assert!(matches!(
        vm.prepare_timer_entry(0),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(counters(&vm), (0, 0));
    assert_eq!(vm.async_frame_roots.capacity(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
    assert_eq!(vm.host_timers.len(), 1);
}

#[cfg_attr(test, test)]
fn timer_entry_one_shot_preparation_transfers_existing_storage_without_extra_fees() {
    let (mut vm, _, _, _) = fixture(false);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    let prepared = vm.prepare_timer_entry(0).unwrap();
    assert!(prepared.arguments.is_empty());
    assert_eq!(counters(&vm), (0, 0));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.async_frame_roots.capacity(), 0);
    assert_eq!(vm.host_timers.len(), 1);
}

#[cfg_attr(test, test)]
fn timer_entry_repeated_intervals_use_no_new_persistent_root_registrations() {
    let (mut vm, id, _, observed) = fixture(true);
    let roots = vm.heap.root_storage_state();
    for _ in 0..3 {
        vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default())
            .unwrap();
        assert_eq!(vm.heap.root_storage_state(), roots);
        assert!(vm.async_frame_roots.is_empty());
        assert_eq!(vm.active_scheduling_bytes, 0);
        assert_eq!(counters(&vm), (0, 0));
    }
    assert_eq!(observed.borrow().len(), 3);
    clear(&mut vm, id);
}

#[cfg_attr(test, test)]
fn timer_entry_canceled_identity_requires_no_starting_instruction() {
    let (mut vm, id, _, observed) = fixture(false);
    clear(&mut vm, id);
    vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut vm, 0);
    let mut run = BlueJsTimerRun::default();
    vm.run_host_timer_entry(id, &mut run).unwrap();
    assert_eq!(run, BlueJsTimerRun::default());
    assert_eq!(vm.remaining_instructions, 73);
    assert!(observed.borrow().is_empty());
}

#[cfg_attr(test, test)]
fn timer_entry_success_and_attempt_failure_restore_ambient_root_prefix_and_nesting() {
    for failure in [false, true] {
        let (mut vm, id, _, _) = fixture(true);
        vm.extend_async_frame_roots([vm.object_prototype; 3].into_iter())
            .unwrap();
        vm.host_timer_nesting = 5;
        if failure {
            vm.config.instruction_budget = 1;
        }
        let result = vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default());
        if failure {
            assert_eq!(result, Err(RuntimeError::InstructionLimit));
        } else {
            result.unwrap();
        }
        assert_eq!(vm.async_frame_roots, [vm.object_prototype; 3]);
        assert_eq!(vm.host_timer_nesting, 5);
        assert_eq!(vm.active_scheduling_bytes, 0);
        assert_eq!(counters(&vm), (0, 0));
    }
}

#[cfg_attr(test, test)]
fn timer_entry_attempted_one_shot_failure_consumes_only_that_timer() {
    let (mut vm, id, callback, observed) = fixture(false);
    vm.host_timer_call(
        host_timers::HostTimerFunction::SetTimeout,
        &[callback, Value::Number(2.0)],
        false,
    )
    .unwrap();
    vm.config.instruction_budget = 1;
    let mut run = BlueJsTimerRun::default();
    assert_eq!(
        vm.run_host_timer_entry(id, &mut run),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(run.tasks_run, 1);
    assert_eq!(vm.host_timers.len(), 1);
    assert_eq!(vm.host_timers[0].id, 2);
    assert!(observed.borrow().is_empty());
    assert_eq!(counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn timer_entry_attempted_interval_failure_preserves_its_deadline_and_serial() {
    let (mut vm, id, _, _) = fixture(true);
    let original = registration(&vm);
    vm.config.instruction_budget = 1;
    assert_eq!(
        vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default()),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(registration(&vm), original);
    assert!(vm.async_frame_roots.is_empty());
    assert_eq!(vm.active_scheduling_bytes, 0);
}

#[cfg_attr(test, test)]
fn timer_entry_terminal_document_refusal_keeps_both_timer_kinds() {
    for repeat in [false, true] {
        let (mut vm, id, _, observed) = fixture(repeat);
        let original = registration(&vm);
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
        assert_eq!(
            vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default()),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(registration(&vm), original);
        assert_eq!(vm.remaining_instructions, 73);
        assert_eq!(counters(&vm), (0, 0));
        assert!(observed.borrow().is_empty());
    }
}

fn object_fixture(repeat: bool) -> (Vm, u32, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_host_timer_functions().unwrap();
    let callback = execute(
        &mut vm,
        "(value)=>{ clearInterval(1); globalThis.observed=value.marker; return value; };",
    );
    let held = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.set(held, "marker", Value::Number(42.0)).unwrap();
    let function = if repeat {
        host_timers::HostTimerFunction::SetInterval
    } else {
        host_timers::HostTimerFunction::SetTimeout
    };
    vm.host_timer_call(
        function,
        &[callback, Value::Number(1.0), Value::Object(held)],
        false,
    )
    .unwrap();
    (vm, 1, held)
}

#[cfg_attr(test, test)]
fn timer_entry_prepared_interval_edges_survive_registration_cancellation_and_gc() {
    let (mut vm, id, held) = object_fixture(true);
    let prepared = vm.prepare_timer_entry(0).unwrap();
    let base = vm.async_frame_roots.len();
    vm.async_frame_roots
        .extend(prepared.references.into_iter().flatten());
    clear(&mut vm, id);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(held));
    drop(prepared);
    vm.async_frame_roots.truncate(base);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(held));
    assert_eq!(counters(&vm), (0, 0));
}

#[cfg_attr(test, test)]
fn timer_entry_object_arguments_survive_collection_and_release_after_self_cancellation() {
    for repeat in [false, true] {
        let (mut vm, id, held) = object_fixture(repeat);
        let collections = vm.heap.stats().major_collections;
        vm.next_host_native_gc_bytes = 0;
        vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default())
            .unwrap();
        assert!(vm.heap.stats().major_collections > collections);
        assert_eq!(
            vm.lookup_global_name("observed").unwrap(),
            Some(Value::Number(42.0))
        );
        assert!(vm.host_timers.is_empty());
        vm.with_roots(|heap| heap.try_collect_major()).unwrap();
        assert!(!vm.heap.contains(held));
        assert!(vm.async_frame_roots.is_empty());
    }
}

#[cfg_attr(test, test)]
fn timer_entry_argument_slots_do_not_survive_into_the_promise_checkpoint() {
    let mut observations = Vec::new();
    for argument in ["", ", 7"] {
        let (mut vm, id, _, observed) = fixture(false);
        clear(&mut vm, id);
        execute(&mut vm, &format!("function cb() {{ Promise.resolve().then(recordTimer); }} setTimeout(cb, 1{argument}); 0;"));
        vm.run_host_timer_entry(2, &mut BlueJsTimerRun::default())
            .unwrap();
        observations.push(observed.borrow()[0]);
    }
    // Both root vectors contain only the callback. Primitive arguments add
    // no object-root slots, and consumed argument slots leave the checkpoint.
    assert_eq!(observations[1], observations[0]);
}

#[cfg_attr(test, test)]
fn timer_entry_interval_dispatch_has_ordinary_call_ownership_after_preparation() {
    let (mut vm, id, callback, observed) = fixture(true);
    drop(vm.prepare_timer_entry(0).unwrap());
    vm.reconcile_scheduling_ownership();
    vm.call_native(
        callback.clone(),
        Value::Undefined,
        vec![Value::Number(7.0)],
        false,
    )
    .unwrap();
    observed.borrow_mut().clear();
    vm.call_native(callback, Value::Undefined, vec![Value::Number(7.0)], false)
        .unwrap();
    vm.run_host_timer_entry(id, &mut BlueJsTimerRun::default())
        .unwrap();
    assert_eq!(observed.borrow()[0], observed.borrow()[1]);
    assert_eq!(counters(&vm), (0, 0));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_timer_entry_contracts() {
        timer_entry_zero_starting_fuel_keeps_one_shot_and_ambient_context();
        timer_entry_zero_starting_fuel_precedes_interval_snapshot_admission();
        timer_entry_argument_capacity_transfers_once_into_native_dispatch();
        timer_entry_zero_fuel_can_retry_without_losing_its_registration();
        timer_entry_interval_argument_refusal_preserves_the_original_owner();
        timer_entry_interval_payload_refusal_precedes_copy_publication();
        timer_entry_interval_exact_preparation_admits_slots_and_object_protection();
        timer_entry_interval_exact_primitive_copy_has_a_separate_owner();
        timer_entry_interval_root_replacement_refusal_preserves_the_prefix();
        timer_entry_interval_partial_preparation_refunds_candidate_ownership();
        timer_entry_one_shot_preparation_transfers_existing_storage_without_extra_fees();
        timer_entry_repeated_intervals_use_no_new_persistent_root_registrations();
        timer_entry_canceled_identity_requires_no_starting_instruction();
        timer_entry_success_and_attempt_failure_restore_ambient_root_prefix_and_nesting();
        timer_entry_attempted_one_shot_failure_consumes_only_that_timer();
        timer_entry_attempted_interval_failure_preserves_its_deadline_and_serial();
        timer_entry_terminal_document_refusal_keeps_both_timer_kinds();
        timer_entry_prepared_interval_edges_survive_registration_cancellation_and_gc();
        timer_entry_object_arguments_survive_collection_and_release_after_self_cancellation();
        timer_entry_argument_slots_do_not_survive_into_the_promise_checkpoint();
        timer_entry_interval_dispatch_has_ordinary_call_ownership_after_preparation();
    }
}
