// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite render callbacks and scalar native allowances check entry ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

type Calls = Rc<RefCell<Vec<HostValue>>>;
fn fixture(count: usize) -> (Vm, Value, Calls) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    vm.install_host_function("callback", 1, move |args: &[HostValue]| {
        output.borrow_mut().push(args[0].clone());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    for _ in 0..count {
        register(&mut vm, callback.clone()).unwrap();
    }
    vm.remaining_instructions = 73;
    (vm, callback, calls)
}
fn register(vm: &mut Vm, callback: Value) -> Result<Value, RuntimeError> {
    vm.host_animation_frame_call(
        host_animation_frames::HostAnimationFrameFunction::Request,
        &[callback],
        false,
    )
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn snapshot_bytes() -> usize {
    4 * std::mem::size_of::<u32>()
}

#[cfg_attr(test, test)]
fn animation_frame_entry_zero_starting_fuel_preserves_registration_and_context() {
    let (mut vm, _, calls) = fixture(1);
    vm.config.instruction_budget = 0;
    let roots = vm.heap.root_storage_state();
    let timestamp = vm.host_animation_frame_timestamp_ms;
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_animation_frame_timestamp_ms, timestamp);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn animation_frame_entry_snapshot_refusal_keeps_callbacks_roots_and_timestamp() {
    let (mut vm, _, calls) = fixture(1);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 0.0);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn animation_frame_entry_argument_refusal_keeps_the_unattempted_callback() {
    let (mut vm, _, calls) = fixture(1);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, snapshot_bytes());
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 0.0);
    assert!(calls.borrow().is_empty());
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}
fn cancel(vm: &mut Vm, handle: u32) {
    vm.host_animation_frame_call(
        host_animation_frames::HostAnimationFrameFunction::Cancel,
        &[Value::Number(f64::from(handle))],
        false,
    )
    .unwrap();
}
fn argument_bytes() -> usize {
    std::mem::size_of::<Value>()
}

#[cfg_attr(test, test)]
fn animation_frame_entry_zero_fuel_precedes_native_snapshot_admission() {
    let (mut vm, _, calls) = fixture(1);
    vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn animation_frame_entry_zero_fuel_can_retry_the_same_registration() {
    let (mut vm, _, calls) = fixture(1);
    let budget = vm.config.instruction_budget;
    vm.config.instruction_budget = 0;
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::InstructionLimit)
    );
    vm.config.instruction_budget = budget;
    let run = vm.run_host_animation_frame(8.25).unwrap();
    assert_eq!(run.callbacks_run, 1);
    assert_eq!(*calls.borrow(), [HostValue::Number(8.25)]);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_snapshot_exact_allowance_keeps_fixed_order_after_registry_changes() {
    let (mut vm, callback, _) = fixture(3);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, snapshot_bytes());
    let snapshot = vm.prepare_animation_frame_snapshot().unwrap();
    assert_eq!(&*snapshot, &[1, 2, 3]);
    assert_eq!(snapshot.retained_bytes(), snapshot_bytes());
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    cancel(&mut vm, 2);
    register(&mut vm, callback).unwrap();
    assert_eq!(&*snapshot, &[1, 2, 3]);
    drop(snapshot);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - snapshot_bytes()
    );
}
#[cfg_attr(test, test)]
fn animation_frame_entry_snapshot_one_byte_short_preserves_all_registrations() {
    let (mut vm, _, _) = fixture(4);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, snapshot_bytes() - 1);
    assert!(matches!(
        vm.prepare_animation_frame_snapshot(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.host_animation_frames.len(), 4);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 0.0);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_empty_snapshot_requires_no_native_backing() {
    let (mut vm, _, _) = fixture(0);
    let _peer = leave_room(&mut vm, 0);
    let snapshot = vm.prepare_animation_frame_snapshot().unwrap();
    assert!(snapshot.is_empty());
    assert_eq!(snapshot.retained_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn animation_frame_entry_rust_held_snapshot_keeps_its_document_lease_after_vm_drop() {
    let (mut vm, _, _) = fixture(3);
    let observed = observer(&vm);
    let snapshot = vm.prepare_animation_frame_snapshot().unwrap();
    drop(vm);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        snapshot_bytes()
    );
    assert_eq!(&*snapshot, &[1, 2, 3]);
    drop(snapshot);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_argument_exact_allowance_owns_the_timestamp_vector() {
    let (mut vm, _, _) = fixture(0);
    let _peer = leave_room(&mut vm, argument_bytes());
    let prepared = vm.prepare_animation_frame_arguments(9.25).unwrap();
    assert_eq!(prepared.arguments, [Value::Number(9.25)]);
    assert_eq!(
        host_scheduling::slots(&prepared.arguments),
        argument_bytes()
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(prepared);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - argument_bytes()
    );
}
#[cfg_attr(test, test)]
fn animation_frame_entry_argument_one_byte_short_publishes_no_preparation() {
    let (mut vm, _, _) = fixture(1);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, argument_bytes() - 1);
    assert!(matches!(
        vm.prepare_animation_frame_arguments(7.0),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 0.0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - argument_bytes() + 1
    );
}
#[cfg_attr(test, test)]
fn animation_frame_entry_rust_held_arguments_keep_their_document_lease_after_vm_drop() {
    let (mut vm, _, _) = fixture(0);
    let observed = observer(&vm);
    let prepared = vm.prepare_animation_frame_arguments(7.0).unwrap();
    drop(vm);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        argument_bytes()
    );
    assert_eq!(prepared.arguments, [Value::Number(7.0)]);
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_arguments_transfer_once_into_native_dispatch() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let native = observer(&vm);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    vm.install_host_function("callback", 1, move |_args: &[HostValue]| {
        output
            .borrow_mut()
            .push(native.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    register(&mut vm, callback.clone()).unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    for _ in 0..2 {
        vm.call_native(
            callback.clone(),
            Value::Undefined,
            vec![Value::Number(7.0)],
            false,
        )
        .unwrap();
    }
    vm.run_host_animation_frame(7.0).unwrap();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[2] - seen[1], snapshot_bytes());
}
#[cfg_attr(test, test)]
fn animation_frame_entry_argument_slots_do_not_survive_into_the_checkpoint() {
    let (mut vm, _, _) = fixture(1);
    let native = observer(&vm);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    vm.install_host_function("job", 0, move |_args: &[HostValue]| {
        output
            .borrow_mut()
            .push(native.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let job = vm.lookup_global_name("job").unwrap().unwrap();
    for _ in 0..2 {
        vm.enqueue_promise_job(PromiseJob::HostMicrotask {
            callback: job.clone(),
        });
        vm.run_host_microtask_checkpoint().unwrap();
    }
    vm.enqueue_promise_job(PromiseJob::HostMicrotask { callback: job });
    vm.run_host_animation_frame(7.0).unwrap();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[2] - seen[1], snapshot_bytes());
}
#[cfg_attr(test, test)]
fn animation_frame_entry_committed_dispatch_failure_consumes_only_one_callback() {
    let (mut vm, _, calls) = fixture(2);
    let roots = vm.heap.root_storage_state().0;
    let budget = vm.config.instruction_budget;
    vm.config.instruction_budget = 1;
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 7.0);
    assert!(calls.borrow().is_empty());
    vm.config.instruction_budget = budget;
    assert_eq!(vm.run_host_animation_frame(8.0).unwrap().callbacks_run, 1);
    assert_eq!(*calls.borrow(), [HostValue::Number(8.0)]);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_page_exception_releases_its_root_and_keeps_later_callbacks() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let count = Rc::new(std::cell::Cell::new(0));
    let output = count.clone();
    vm.install_host_function("callback", 1, move |_args: &[HostValue]| {
        output.set(output.get() + 1);
        if output.get() == 1 {
            Err(HostFunctionError::new("declined"))
        } else {
            Ok(HostValue::Undefined)
        }
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    for _ in 0..2 {
        register(&mut vm, callback.clone()).unwrap();
    }
    let roots = vm.heap.root_storage_state().0;
    let run = vm.run_host_animation_frame(7.0).unwrap();
    assert_eq!((run.callbacks_run, run.uncaught_errors), (2, 1));
    assert_eq!(count.get(), 2);
    assert_eq!(vm.heap.root_storage_state().0, roots - 2);
    assert_eq!(vm.host_animation_frames.capacity(), 0);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_callback_abort_releases_only_the_committed_root() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let count = Rc::new(std::cell::Cell::new(0));
    let output = count.clone();
    vm.install_host_function("callback", 1, move |_args: &[HostValue]| {
        output.set(output.get() + 1);
        scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    for _ in 0..2 {
        register(&mut vm, callback.clone()).unwrap();
    }
    let roots = vm.heap.root_storage_state().0;
    let observed = observer(&vm);
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(count.get(), 1);
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_reporting_refusal_still_releases_the_committed_root() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("callback", 1, move |_args: &[HostValue]| {
        scope.fail();
        Err(HostFunctionError::new("declined"))
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    register(&mut vm, callback).unwrap();
    let roots = vm.heap.root_storage_state().0;
    let observed = observer(&vm);
    assert_eq!(
        vm.run_host_animation_frame(7.0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    assert_eq!(vm.host_animation_frames.capacity(), 0);
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_invalid_timestamp_precedes_fuel_and_native_preparation() {
    let (mut vm, _, calls) = fixture(1);
    vm.config.instruction_budget = 0;
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    for timestamp in [-1.0, f64::INFINITY, f64::NAN] {
        assert!(matches!(
            vm.run_host_animation_frame(timestamp),
            Err(RuntimeError::RangeError(_))
        ));
    }
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 0.0);
    assert!(calls.borrow().is_empty());
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn animation_frame_entry_paused_render_needs_no_snapshot_arguments_or_starting_fuel() {
    let (mut vm, _, calls) = fixture(1);
    vm.execute_script_until_debugger_pause(
        &crate::compile(&crate::parse("0;").unwrap()).unwrap(),
        0,
    )
    .unwrap();
    vm.config.instruction_budget = 0;
    let fuel = vm.remaining_instructions;
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_host_animation_frame(7.0).unwrap(),
        BlueJsAnimationFrameRun::default()
    );
    assert_eq!(vm.remaining_instructions, fuel);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_animation_frames.len(), 1);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 0.0);
    assert!(calls.borrow().is_empty());
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn animation_frame_entry_canceled_identity_needs_no_arguments_or_instruction() {
    let (mut vm, _, calls) = fixture(1);
    cancel(&mut vm, 1);
    vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut vm, 0);
    let mut run = BlueJsAnimationFrameRun::default();
    vm.run_animation_frame_entry(1, 7.0, &mut run).unwrap();
    assert_eq!(run, BlueJsAnimationFrameRun::default());
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.host_animation_frame_timestamp_ms, 0.0);
    assert!(calls.borrow().is_empty());
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn animation_frame_entry_success_keeps_one_monotonic_fractional_timestamp() {
    let (mut vm, callback, calls) = fixture(1);
    vm.run_host_animation_frame(9.25).unwrap();
    register(&mut vm, callback).unwrap();
    vm.run_host_animation_frame(3.0).unwrap();
    assert_eq!(
        *calls.borrow(),
        [HostValue::Number(9.25), HostValue::Number(9.25)]
    );
    assert_eq!(vm.host_animation_frame_timestamp_ms, 9.25);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_callback_root_survives_collection_and_releases_after_dispatch() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    execute(
        &mut vm,
        "var marker=null;globalThis.cb=function(){marker={};};0;",
    );
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let callback = vm.heap.get(global, "cb").unwrap();
    let object = callback.object_id().unwrap();
    register(&mut vm, callback).unwrap();
    vm.heap.set(global, "cb", Value::Undefined).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let garbage = [
        vm.heap.alloc_object(None).unwrap(),
        vm.heap.alloc_object(None).unwrap(),
    ];
    vm.heap.allow_only(0);
    let collections = vm.heap.stats().major_collections;
    assert_eq!(vm.run_host_animation_frame(7.0).unwrap().callbacks_run, 1);
    assert!(vm.heap.stats().major_collections > collections);
    assert!(garbage.iter().all(|object| !vm.heap.contains(*object)));
    assert!(matches!(
        vm.lookup_global_name("marker").unwrap().unwrap(),
        Value::Object(_)
    ));
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}
#[cfg_attr(test, test)]
fn animation_frame_entry_snapshot_drop_refunds_after_terminal_failure() {
    let (mut vm, _, _) = fixture(2);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let snapshot = vm.prepare_animation_frame_snapshot().unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + snapshot_bytes()
    );
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn animation_frame_entry_empty_render_needs_no_starting_fuel_or_argument_storage() {
    let (mut vm, _, _) = fixture(0);
    vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_host_animation_frame(7.0).unwrap(),
        BlueJsAnimationFrameRun::default()
    );
    assert_eq!(vm.host_animation_frame_timestamp_ms, 7.0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_animation_frame_entry_contracts() {
        animation_frame_entry_zero_starting_fuel_preserves_registration_and_context();
        animation_frame_entry_snapshot_refusal_keeps_callbacks_roots_and_timestamp();
        animation_frame_entry_argument_refusal_keeps_the_unattempted_callback();
        animation_frame_entry_zero_fuel_precedes_native_snapshot_admission();
        animation_frame_entry_zero_fuel_can_retry_the_same_registration();
        animation_frame_entry_snapshot_exact_allowance_keeps_fixed_order_after_registry_changes();
        animation_frame_entry_snapshot_one_byte_short_preserves_all_registrations();
        animation_frame_entry_empty_snapshot_requires_no_native_backing();
        animation_frame_entry_rust_held_snapshot_keeps_its_document_lease_after_vm_drop();
        animation_frame_entry_argument_exact_allowance_owns_the_timestamp_vector();
        animation_frame_entry_argument_one_byte_short_publishes_no_preparation();
        animation_frame_entry_rust_held_arguments_keep_their_document_lease_after_vm_drop();
        animation_frame_entry_arguments_transfer_once_into_native_dispatch();
        animation_frame_entry_argument_slots_do_not_survive_into_the_checkpoint();
        animation_frame_entry_committed_dispatch_failure_consumes_only_one_callback();
        animation_frame_entry_page_exception_releases_its_root_and_keeps_later_callbacks();
        animation_frame_entry_callback_abort_releases_only_the_committed_root();
        animation_frame_entry_reporting_refusal_still_releases_the_committed_root();
        animation_frame_entry_invalid_timestamp_precedes_fuel_and_native_preparation();
        animation_frame_entry_paused_render_needs_no_snapshot_arguments_or_starting_fuel();
        animation_frame_entry_canceled_identity_needs_no_arguments_or_instruction();
        animation_frame_entry_success_keeps_one_monotonic_fractional_timestamp();
        animation_frame_entry_callback_root_survives_collection_and_releases_after_dispatch();
        animation_frame_entry_snapshot_drop_refunds_after_terminal_failure();
        animation_frame_entry_empty_render_needs_no_starting_fuel_or_argument_storage();
    }
}
