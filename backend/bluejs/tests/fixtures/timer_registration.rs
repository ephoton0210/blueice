// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny registration batches and scalar allowances verify atomic admission.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn fixture() -> (Vm, Value) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_host_timer_functions().unwrap();
    vm.install_host_function("callback", 0, |_args: &[HostValue]| {
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    (vm, callback)
}
fn register(vm: &mut Vm, callback: Value, arguments: &[Value]) -> Result<Value, RuntimeError> {
    let mut args = vec![callback, Value::Number(1.0)];
    args.extend_from_slice(arguments);
    vm.host_timer_call(host_timers::HostTimerFunction::SetTimeout, &args, false)
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn serial(vm: &Vm) -> u64 {
    vm.host_scheduling.as_ref().unwrap().task_serial_state()
}
fn fill_roots(vm: &mut Vm, free: usize) {
    let (mut len, mut capacity, _) = vm.heap.root_storage_state();
    assert!(capacity <= 31);
    if capacity - len < free {
        for _ in 0..=capacity - len {
            vm.heap.root(vm.object_prototype).unwrap();
        }
        (len, capacity, _) = vm.heap.root_storage_state();
    }
    for _ in len..capacity - free {
        vm.heap.root(vm.object_prototype).unwrap();
    }
    vm.synchronize_scheduling_usage().unwrap();
}

#[cfg_attr(test, test)]
fn timer_registration_growth_refuses_a_net_capacity_only_allowance() {
    let (mut vm, callback) = fixture();
    for _ in 0..4 {
        register(&mut vm, callback.clone(), &[]).unwrap();
    }
    assert_eq!(vm.host_timers.len(), vm.host_timers.capacity());
    let roots = vm.heap.root_storage_state();
    let next_id = vm.host_timer_next_id;
    let next_serial = serial(&vm);
    let room = 4 * std::mem::size_of::<HostTimer>() + std::mem::size_of::<RootId>();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        register(&mut vm, callback, &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_timers.len(), 4);
    assert_eq!(vm.host_timers.capacity(), 4);
    assert_eq!(vm.host_timer_next_id, next_id);
    assert_eq!(serial(&vm), next_serial);
    assert_eq!(vm.heap.root_storage_state(), roots);
}

#[cfg_attr(test, test)]
fn timer_registration_root_refusal_does_not_consume_a_task_serial() {
    let (mut vm, callback) = fixture();
    vm.host_timers.reserve_exact(4);
    fill_roots(&mut vm, 0);
    let next_serial = serial(&vm);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, std::mem::size_of::<RootId>());
    assert_eq!(
        register(&mut vm, callback, &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(serial(&vm), next_serial);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.host_timers.is_empty());
    assert_eq!(vm.host_timer_next_id, 1);
}

#[cfg_attr(test, test)]
fn timer_registration_partial_root_refusal_preserves_root_ids_and_counters() {
    let (mut vm, callback) = fixture();
    vm.host_timers.reserve_exact(4);
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    fill_roots(&mut vm, 1);
    let roots = vm.heap.root_storage_state();
    let registrations = vm.heap.stats().root_registrations;
    let _peer = leave_room(
        &mut vm,
        std::mem::size_of::<Value>() + 2 * std::mem::size_of::<RootId>(),
    );
    assert_eq!(
        register(&mut vm, callback, &[Value::Object(object)]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.heap.stats().root_registrations, registrations);
    assert!(vm.host_timers.is_empty());
    assert_eq!(vm.host_timer_next_id, 1);
}

fn root_bytes(count: usize) -> usize {
    count * std::mem::size_of::<RootId>()
}
fn value_bytes(count: usize) -> usize {
    count * std::mem::size_of::<Value>()
}
fn table_bytes(count: usize) -> usize {
    count * std::mem::size_of::<HostTimer>()
}
fn clear(vm: &mut Vm, id: u32) -> Result<Value, RuntimeError> {
    vm.host_timer_call(
        host_timers::HostTimerFunction::Clear,
        &[Value::Number(f64::from(id))],
        false,
    )
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn prewarm(vm: &mut Vm, free: usize) {
    vm.host_timers.reserve_exact(4);
    fill_roots(vm, free);
}

#[cfg_attr(test, test)]
fn timer_registration_exact_initial_allowance_publishes_actual_storage_once() {
    let (mut vm, callback) = fixture();
    fill_roots(&mut vm, 1);
    let registrations = vm.heap.stats().root_registrations;
    let _peer = leave_room(&mut vm, table_bytes(4) + value_bytes(1) + root_bytes(1));
    assert_eq!(
        register(&mut vm, callback, &[Value::Number(7.0)]),
        Ok(Value::Number(1.0))
    );
    assert_eq!(vm.host_timers.capacity(), 4);
    assert_eq!(
        vm.host_timers[0].scheduling_payload_bytes(),
        value_bytes(1) + root_bytes(1)
    );
    assert_eq!(vm.heap.stats().root_registrations, registrations + 1);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn timer_registration_one_byte_short_leaves_all_original_storage_untouched() {
    let (mut vm, callback) = fixture();
    fill_roots(&mut vm, 1);
    let roots = vm.heap.root_storage_state();
    let stats = vm.heap.stats();
    let next_serial = serial(&vm);
    let _peer = leave_room(&mut vm, table_bytes(4) + root_bytes(1) - 1);
    assert_eq!(
        register(&mut vm, callback, &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.heap.stats(), stats);
    assert_eq!(vm.host_timers.capacity(), 0);
    assert_eq!(serial(&vm), next_serial);
    assert_eq!(vm.host_timer_next_id, 1);
}

#[cfg_attr(test, test)]
fn timer_registration_primitive_arguments_require_only_one_object_root_slot() {
    let (mut vm, callback) = fixture();
    prewarm(&mut vm, 1);
    let arguments = [
        Value::Number(7.0),
        Value::Bool(true),
        Value::String("tiny".into()),
    ];
    let _peer = leave_room(&mut vm, value_bytes(3) + root_bytes(1) + 8);
    register(&mut vm, callback, &arguments).unwrap();
    assert_eq!(
        vm.host_timers[0].scheduling_payload_bytes(),
        value_bytes(3) + root_bytes(1) + 8
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn timer_registration_full_replacement_allowance_refunds_the_old_table() {
    let (mut vm, callback) = fixture();
    for _ in 0..4 {
        register(&mut vm, callback.clone(), &[]).unwrap();
    }
    assert!(vm.heap.root_storage_state().1 > vm.heap.root_storage_state().0);
    let _peer = leave_room(&mut vm, table_bytes(8) + root_bytes(1));
    assert_eq!(register(&mut vm, callback, &[]), Ok(Value::Number(5.0)));
    assert_eq!(vm.host_timers.capacity(), 8);
    assert_eq!(vm.host_timers.len(), 5);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - table_bytes(4)
    );
}

#[cfg_attr(test, test)]
fn timer_registration_dropped_root_batch_refunds_the_detached_hash_allocation() {
    let (mut vm, _) = fixture();
    fill_roots(&mut vm, 0);
    let objects = [vm.object_prototype; 2];
    let roots = vm.heap.root_storage_state();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm.heap.prepare_root_batch(&objects).unwrap();
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.host_scheduling_snapshot().payload_bytes > used);
    drop(prepared);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
    assert_eq!(vm.heap.root_storage_state(), roots);
}

#[cfg_attr(test, test)]
fn timer_registration_root_batch_commits_unique_ids_and_counters_together() {
    let (mut vm, _) = fixture();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let objects = [vm.object_prototype, object];
    let before = vm.heap.root_storage_state();
    let registrations = vm.heap.stats().root_registrations;
    let prepared = vm.heap.prepare_root_batch(&objects).unwrap();
    let mut roots = Vec::with_capacity(2);
    vm.heap.commit_root_batch(prepared, &mut roots);
    assert_eq!(roots.len(), 2);
    assert_ne!(roots[0], roots[1]);
    assert_eq!(vm.heap.root_storage_state().0, before.0 + 2);
    assert_eq!(vm.heap.root_storage_state().2, before.2 + 2);
    assert_eq!(vm.heap.stats().root_registrations, registrations + 2);
    assert_eq!(vm.heap.unroot(roots[0]), Ok(objects[0]));
    assert_eq!(vm.heap.unroot(roots[1]), Ok(objects[1]));
}

#[cfg_attr(test, test)]
fn timer_registration_duplicate_object_batch_keeps_independent_roots() {
    let (mut vm, _) = fixture();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let objects = [object; 2];
    let prepared = vm.heap.prepare_root_batch(&objects).unwrap();
    let mut roots = Vec::with_capacity(2);
    vm.heap.commit_root_batch(prepared, &mut roots);
    vm.heap.unroot(roots[0]).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(object));
    vm.heap.unroot(roots[1]).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}

#[cfg_attr(test, test)]
fn timer_registration_empty_root_batch_needs_no_native_allowance_or_ids() {
    let (mut vm, _) = fixture();
    let before = vm.heap.root_storage_state();
    let registrations = vm.heap.stats().root_registrations;
    let _peer = leave_room(&mut vm, 0);
    let prepared = vm.heap.prepare_root_batch(&[]).unwrap();
    vm.heap.commit_root_batch(prepared, &mut Vec::new());
    assert_eq!(vm.heap.root_storage_state(), before);
    assert_eq!(vm.heap.stats().root_registrations, registrations);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn timer_registration_foreign_argument_refuses_before_any_root_batch_allocation() {
    let (mut vm, callback) = fixture();
    fill_roots(&mut vm, 0);
    let foreign = Heap::default().alloc_object(None).unwrap();
    let roots = vm.heap.root_storage_state();
    let stats = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        register(&mut vm, callback, &[Value::Object(foreign)]),
        Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.heap.stats(), stats);
    assert_eq!(serial(&vm), 0);
    assert_eq!(vm.host_timers.capacity(), 0);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn timer_registration_root_id_batch_overflow_precedes_storage_admission() {
    let (mut vm, callback) = fixture();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.set_root_counter_for_test(u64::MAX - 1);
    let before = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        register(&mut vm, callback, &[Value::Object(object)]),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(vm.heap.root_storage_state(), before);
    assert_eq!(serial(&vm), 0);
    assert_eq!(vm.host_timers.capacity(), 0);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn timer_registration_timer_id_boundary_preserves_roots_and_serials() {
    let (mut vm, callback) = fixture();
    vm.host_timer_next_id = i32::MAX as u32;
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        register(&mut vm, callback, &[]),
        Err(RuntimeError::RangeError(_))
    ));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), 0);
    assert_eq!(vm.host_timers.capacity(), 0);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn timer_registration_task_serial_failure_refunds_all_candidates_without_publication() {
    let (mut vm, callback) = fixture();
    fill_roots(&mut vm, 0);
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .set_task_serial_for_test(u64::MAX);
    let roots = vm.heap.root_storage_state();
    let stats = vm.heap.stats();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(
        register(&mut vm, callback, &[Value::String("tiny".into())]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.heap.stats(), stats);
    assert_eq!(vm.host_timer_next_id, 1);
    assert_eq!(vm.host_timers.capacity(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
}

#[cfg_attr(test, test)]
fn timer_registration_terminal_refusal_preserves_prior_timer_owners() {
    let (mut vm, callback) = fixture();
    register(&mut vm, callback.clone(), &[]).unwrap();
    let roots = vm.heap.root_storage_state();
    let next_serial = serial(&vm);
    let next_id = vm.host_timer_next_id;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        register(&mut vm, callback, &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_timers.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), next_serial);
    assert_eq!(vm.host_timer_next_id, next_id);
}

#[cfg_attr(test, test)]
fn timer_registration_spare_slot_payload_refusal_keeps_the_old_capacity() {
    let (mut vm, callback) = fixture();
    prewarm(&mut vm, 1);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, value_bytes(1) + root_bytes(1) + 7);
    assert_eq!(
        register(&mut vm, callback, &[Value::String("tiny".into())]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_timers.capacity(), 4);
    assert!(vm.host_timers.is_empty());
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), 0);
}

#[cfg_attr(test, test)]
fn timer_registration_rust_held_candidate_retains_its_document_charge_after_vm_drop() {
    let (mut vm, _) = fixture();
    let observed = observer(&vm);
    let prepared = vm
        .prepare_timer_registration(&[Value::String("tiny".into())], 1)
        .unwrap();
    let retained = table_bytes(4) + value_bytes(1) + root_bytes(1) + 8;
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, retained);
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

fn child_fixture(parent: &Vm) -> (Vm, Value) {
    let mut child = Vm {
        host_scheduling: Some(parent.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    child.use_bounded_host_microtask_checkpoints().unwrap();
    child.install_host_timer_functions().unwrap();
    child
        .install_host_function("callback", 0, |_args: &[HostValue]| {
            Ok(HostValue::Undefined)
        })
        .unwrap();
    let callback = child.lookup_global_name("callback").unwrap().unwrap();
    (child, callback)
}

#[cfg_attr(test, test)]
fn timer_registration_shared_document_refusal_preserves_child_and_parent_roots() {
    let (mut parent, callback) = fixture();
    register(&mut parent, callback, &[]).unwrap();
    let (mut child, callback) = child_fixture(&parent);
    prewarm(&mut child, 1);
    let parent_roots = parent.heap.root_storage_state();
    let child_roots = child.heap.root_storage_state();
    let next_serial = serial(&parent);
    let _peer = leave_room(&mut parent, root_bytes(1) - 1);
    assert_eq!(
        register(&mut child, callback, &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(parent.heap.root_storage_state(), parent_roots);
    assert_eq!(child.heap.root_storage_state(), child_roots);
    assert_eq!(serial(&parent), next_serial);
    assert_eq!(child.host_timer_next_id, 1);
    assert!(child.host_timers.is_empty());
    assert_eq!(parent.host_timers.len(), 1);
}

#[cfg_attr(test, test)]
fn timer_registration_canceled_input_storage_is_reusable_by_another_realm() {
    let (mut parent, callback) = fixture();
    prewarm(&mut parent, 2);
    register(&mut parent, callback.clone(), &[]).unwrap();
    let (mut child, child_callback) = child_fixture(&parent);
    prewarm(&mut child, 1);
    let retained = value_bytes(1) + root_bytes(1) + 8;
    let _peer = leave_room(&mut parent, retained);
    register(&mut parent, callback, &[Value::String("tiny".into())]).unwrap();
    assert_eq!(
        parent.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    clear(&mut parent, 2).unwrap();
    assert_eq!(parent.host_timers.len(), 1);
    assert_eq!(
        parent.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - retained
    );
    register(&mut child, child_callback, &[Value::String("tiny".into())]).unwrap();
    assert_eq!(
        parent.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn timer_registration_object_argument_roots_survive_gc_then_release_on_cancel() {
    let (mut vm, callback) = fixture();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    register(&mut vm, callback, &[Value::Object(object)]).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(object));
    clear(&mut vm, 1).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}

#[cfg_attr(test, test)]
fn timer_registration_scalar_count_refusal_precedes_any_root_or_vector_allocation() {
    let (mut vm, callback) = fixture();
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_timers: MAX_PENDING_HOST_TIMERS,
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap();
    let stats = vm.heap.stats();
    let roots = vm.heap.root_storage_state();
    assert_eq!(
        register(&mut vm, callback, &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), stats);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_timers.capacity(), 0);
    assert_eq!(vm.host_timer_next_id, 1);
    assert_eq!(serial(&vm), 0);
}

#[cfg_attr(test, test)]
fn timer_registration_terminal_cancellation_refunds_committed_storage() {
    let (mut vm, callback) = fixture();
    register(&mut vm, callback, &[Value::String("tiny".into())]).unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let released =
        table_bytes(vm.host_timers.capacity()) + vm.host_timers[0].scheduling_payload_bytes();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(clear(&mut vm, 1), Err(RuntimeError::SchedulingLimit));
    assert!(vm.host_timers.is_empty());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - released
    );
}

#[cfg_attr(test, test)]
fn timer_registration_argument_limit_uses_one_root_and_refuses_excess_before_staging() {
    let (mut vm, callback) = fixture();
    prewarm(&mut vm, 1);
    let arguments: [Value; 32] = std::array::from_fn(|_| Value::Number(7.0));
    let _peer = leave_room(&mut vm, value_bytes(32) + root_bytes(1));
    register(&mut vm, callback.clone(), &arguments).unwrap();
    assert_eq!(
        vm.host_timers[0].scheduling_payload_bytes(),
        value_bytes(32) + root_bytes(1)
    );
    let roots = vm.heap.root_storage_state();
    let next_serial = serial(&vm);
    let too_many: [Value; 33] = std::array::from_fn(|_| Value::Undefined);
    assert!(matches!(
        register(&mut vm, callback, &too_many),
        Err(RuntimeError::RangeError(_))
    ));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), next_serial);
    assert_eq!(vm.host_timer_next_id, 2);
    assert_eq!(vm.host_timers.len(), 1);
    assert!(vm.check_scheduling_failure().is_ok());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_timer_registration_contracts() {
        timer_registration_growth_refuses_a_net_capacity_only_allowance();
        timer_registration_root_refusal_does_not_consume_a_task_serial();
        timer_registration_partial_root_refusal_preserves_root_ids_and_counters();
        timer_registration_exact_initial_allowance_publishes_actual_storage_once();
        timer_registration_one_byte_short_leaves_all_original_storage_untouched();
        timer_registration_primitive_arguments_require_only_one_object_root_slot();
        timer_registration_full_replacement_allowance_refunds_the_old_table();
        timer_registration_dropped_root_batch_refunds_the_detached_hash_allocation();
        timer_registration_root_batch_commits_unique_ids_and_counters_together();
        timer_registration_duplicate_object_batch_keeps_independent_roots();
        timer_registration_empty_root_batch_needs_no_native_allowance_or_ids();
        timer_registration_foreign_argument_refuses_before_any_root_batch_allocation();
        timer_registration_root_id_batch_overflow_precedes_storage_admission();
        timer_registration_timer_id_boundary_preserves_roots_and_serials();
        timer_registration_task_serial_failure_refunds_all_candidates_without_publication();
        timer_registration_terminal_refusal_preserves_prior_timer_owners();
        timer_registration_spare_slot_payload_refusal_keeps_the_old_capacity();
        timer_registration_rust_held_candidate_retains_its_document_charge_after_vm_drop();
        timer_registration_shared_document_refusal_preserves_child_and_parent_roots();
        timer_registration_canceled_input_storage_is_reusable_by_another_realm();
        timer_registration_object_argument_roots_survive_gc_then_release_on_cancel();
        timer_registration_scalar_count_refusal_precedes_any_root_or_vector_allocation();
        timer_registration_terminal_cancellation_refunds_committed_storage();
        timer_registration_argument_limit_uses_one_root_and_refuses_excess_before_staging();
    }
}
