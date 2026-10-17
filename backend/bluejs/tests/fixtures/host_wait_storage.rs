// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite in-memory callbacks and scalar native/managed admission controls.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

type Calls = Rc<RefCell<Vec<HostPromiseToken>>>;
fn fixture(count: usize) -> (Vm, Calls) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    // Direct host entry still needs finite fuel for rejection constructors.
    vm.remaining_instructions = 128;
    vm.global("Promise").unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        move |token, _args: &[HostValue]| {
            output.borrow_mut().push(token);
            Ok(())
        },
    )
    .unwrap();
    for _ in 0..count {
        call(&mut vm, &[]).unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    (vm, calls)
}
fn call(vm: &mut Vm, args: &[Value]) -> Result<Value, RuntimeError> {
    vm.host_async_function_call(0, Value::Undefined, args, false)
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn replacement_bytes<K, V>(values: &HashMap<K, V>) -> usize {
    host_scheduling::map_slots(&HashMap::<K, V>::with_capacity(values.len() + 1))
}

#[cfg_attr(test, test)]
fn host_wait_storage_refuses_net_only_joint_table_growth_before_publication() {
    let (mut vm, calls) = fixture(7);
    assert_eq!(vm.promises.len(), vm.promises.capacity());
    assert_eq!(
        vm.host_promises.pending.len(),
        vm.host_promises.pending.capacity()
    );
    let bytes = replacement_bytes(&vm.promises) - host_scheduling::map_slots(&vm.promises)
        + replacement_bytes(&vm.host_promises.pending)
        - host_scheduling::map_slots(&vm.host_promises.pending);
    let serial = vm.host_scheduling.as_ref().unwrap().task_serial_state();
    let roots = vm.heap.root_storage_state();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(call(&mut vm, &[]), Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.host_scheduling.as_ref().unwrap().task_serial_state(),
        serial
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.promises.len(), 7);
    assert_eq!(vm.promises.capacity(), 7);
    assert_eq!(vm.host_promises.pending.len(), 7);
    assert_eq!(vm.host_promises.pending.capacity(), 7);
    assert_eq!(calls.borrow().len(), 7);
}

#[cfg_attr(test, test)]
fn host_wait_storage_root_id_refusal_keeps_serial_tables_and_callbacks_untouched() {
    let (mut vm, calls) = fixture(0);
    vm.heap.allow_root_registrations(0);
    let serial = vm.host_scheduling.as_ref().unwrap().task_serial_state();
    let before = vm.heap.stats();
    assert_eq!(
        call(&mut vm, &[]),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(
        vm.host_scheduling.as_ref().unwrap().task_serial_state(),
        serial
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(vm.host_promises.pending.capacity(), 0);
    assert!(calls.borrow().is_empty());
}

#[cfg_attr(test, test)]
fn host_wait_storage_managed_refusal_does_not_publish_wait_table_capacity() {
    let (mut vm, calls) = fixture(0);
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        call(&mut vm, &[]),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.promises.len(), 0);
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(vm.host_promises.pending.len(), 0);
    assert_eq!(vm.host_promises.pending.capacity(), 0);
    assert_eq!(vm.host_scheduling.as_ref().unwrap().task_serial_state(), 0);
    assert!(calls.borrow().is_empty());
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn argument_bytes(count: usize) -> usize {
    count * std::mem::size_of::<HostValue>()
}
fn serial(vm: &Vm) -> u64 {
    vm.host_scheduling.as_ref().unwrap().task_serial_state()
}
fn root_candidate_bytes(vm: &Vm) -> usize {
    let (len, capacity, _) = vm.heap.root_storage_state();
    if len < capacity {
        0
    } else {
        host_scheduling::map_slots(&HashMap::<RootId, ObjectId>::with_capacity(
            (len + 1).max(len * 2),
        ))
    }
}
fn fill_root_slots(vm: &mut Vm) {
    let (len, capacity, _) = vm.heap.root_storage_state();
    assert!(capacity <= 31);
    for _ in len..capacity {
        vm.heap.root(vm.object_prototype).unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
}

#[cfg_attr(test, test)]
fn host_wait_storage_arguments_exact_allowance_owns_actual_copies_and_slots() {
    let (mut vm, _) = fixture(0);
    let observed = observer(&vm);
    let _peer = leave_room(&mut vm, argument_bytes(2) + 8);
    let prepared = vm
        .prepare_host_wait_arguments(&[Value::String("tiny".into()), Value::Number(7.0)])
        .unwrap();
    assert_eq!(
        prepared.values,
        [HostValue::String("tiny".into()), HostValue::Number(7.0)]
    );
    assert!(prepared.error.is_none());
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(prepared);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - argument_bytes(2) - 8
    );
}
#[cfg_attr(test, test)]
fn host_wait_storage_argument_refusal_precedes_callback_tables_and_managed_allocation() {
    let (mut vm, calls) = fixture(0);
    let before = vm.heap.stats();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, argument_bytes(1) + 7);
    assert_eq!(
        call(&mut vm, &[Value::String("tiny".into())]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(vm.host_promises.pending.capacity(), 0);
    assert_eq!(serial(&vm), 0);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn host_wait_storage_partial_conversion_refunds_buffers_and_retains_its_error_owner() {
    let (mut vm, _) = fixture(0);
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let observed = observer(&vm);
    let prepared = vm
        .prepare_host_wait_arguments(&[Value::String("tiny".into()), Value::Object(object)])
        .unwrap();
    assert!(prepared.values.is_empty());
    assert_eq!(prepared.values.capacity(), 0);
    let Some(RuntimeError::TypeError(message)) = &prepared.error else {
        panic!("conversion TypeError");
    };
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        used + message.capacity()
    );
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, used);
}
#[cfg_attr(test, test)]
fn host_wait_storage_rust_held_arguments_keep_their_document_lease_after_vm_drop() {
    let (mut vm, _) = fixture(0);
    let observed = observer(&vm);
    let prepared = vm
        .prepare_host_wait_arguments(&[Value::String("tiny".into())])
        .unwrap();
    drop(vm);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        argument_bytes(1) + 8
    );
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_wait_storage_root_storage_refusal_keeps_unpublished_managed_objects_unrooted() {
    let (mut vm, calls) = fixture(0);
    fill_root_slots(&mut vm);
    vm.promises.reserve(1);
    vm.host_promises.pending.reserve(1);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(call(&mut vm, &[]), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.promises.is_empty());
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.promises.capacity(), 3);
    assert_eq!(vm.host_promises.pending.capacity(), 3);
    assert_eq!(serial(&vm), 0);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn host_wait_storage_serial_refusal_refunds_every_candidate_without_publishing_roots() {
    let (mut vm, calls) = fixture(0);
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .set_task_serial_for_test(u64::MAX);
    let roots = vm.heap.root_storage_state();
    let observed = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(
        call(&mut vm, &[Value::String("tiny".into())]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(vm.host_promises.pending.capacity(), 0);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, before);
    assert!(!vm.preserve_promise_table_capacity);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn host_wait_storage_managed_refusal_refunds_argument_and_map_leases_for_other_realms() {
    let (mut vm, calls) = fixture(0);
    let observed = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.heap.allow_only(0);
    assert!(matches!(
        call(&mut vm, &[Value::String("tiny".into())]),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(serial(&vm), 0);
    assert!(vm.host_promises.pending.is_empty());
    assert!(vm.promises.is_empty());
    assert!(!vm.preserve_promise_table_capacity);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn host_wait_storage_scalar_count_refusal_precedes_native_or_managed_allocations() {
    let (mut vm, calls) = fixture(0);
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_reactions: MAX_PENDING_HOST_REACTIONS,
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap();
    let before = vm.heap.stats();
    let roots = vm.heap.root_storage_state();
    assert_eq!(call(&mut vm, &[]), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.promises.capacity(), 0);
    assert_eq!(vm.host_promises.pending.capacity(), 0);
    assert_eq!(serial(&vm), 0);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn host_wait_storage_one_commit_publishes_exactly_one_root_record_wait_and_token() {
    let (mut vm, calls) = fixture(0);
    let roots = vm.heap.root_storage_state();
    let registrations = vm.heap.stats().root_registrations;
    let promise = call(&mut vm, &[Value::Number(7.0)])
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(vm.promises.len(), 1);
    assert!(vm.promises.contains_key(&promise));
    assert_eq!(vm.host_promises.pending.len(), 1);
    assert_eq!(vm.heap.root_storage_state().0, roots.0 + 1);
    assert_eq!(vm.heap.root_storage_state().2, roots.2 + 1);
    assert_eq!(vm.heap.stats().root_registrations, registrations + 1);
    assert_eq!(serial(&vm), 1);
    assert_eq!(calls.borrow().len(), 1);
}
#[cfg_attr(test, test)]
fn host_wait_storage_callback_observes_committed_wait_and_its_argument_lease() {
    let (mut vm, _) = fixture(0);
    let observed = observer(&vm);
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    vm.install_host_async_function(
        "observe",
        2,
        HostTaskSource::UserInteraction,
        move |_token: HostPromiseToken, args: &[HostValue]| {
            let usage = observed.host_scheduling_snapshot();
            captured.borrow_mut().push((
                usage.payload_bytes,
                usage.pending_reactions,
                args.to_vec(),
            ));
            Ok(())
        },
    )
    .unwrap();
    vm.host_async_function_call(
        1,
        Value::Undefined,
        &[Value::String("tiny".into()), Value::Number(7.0)],
        false,
    )
    .unwrap();
    let after = vm.host_scheduling_usage().unwrap().payload_bytes;
    let output = output.borrow();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].0 - after, argument_bytes(2) + 8);
    assert_eq!(output[0].1, 1);
    assert_eq!(
        output[0].2,
        [HostValue::String("tiny".into()), HostValue::Number(7.0)]
    );
}
#[cfg_attr(test, test)]
fn host_wait_storage_callback_error_keeps_rejected_promise_semantics_and_releases_wait_root() {
    let (mut vm, _) = fixture(0);
    vm.global("TypeError").unwrap();
    vm.install_host_async_function(
        "fail",
        0,
        HostTaskSource::UserInteraction,
        |_token: HostPromiseToken, _args: &[HostValue]| Err(HostFunctionError::new("declined")),
    )
    .unwrap();
    let roots = vm.heap.root_storage_state().0;
    let promise = vm
        .host_async_function_call(1, Value::Undefined, &[], false)
        .unwrap()
        .object_id()
        .unwrap();
    let PromiseStatus::Rejected(Value::Object(error)) = vm.promises[&promise].status else {
        panic!("rejected TypeError Promise");
    };
    assert_eq!(
        vm.heap.get(error, "message").unwrap(),
        Value::String("declined".into())
    );
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    assert_eq!(serial(&vm), 1);
}
#[cfg_attr(test, test)]
fn host_wait_storage_unsupported_arguments_reject_without_invoking_the_callback() {
    let (mut vm, calls) = fixture(0);
    vm.global("TypeError").unwrap();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let roots = vm.heap.root_storage_state().0;
    let promise = call(
        &mut vm,
        &[Value::String("tiny".into()), Value::Object(object)],
    )
    .unwrap()
    .object_id()
    .unwrap();
    let PromiseStatus::Rejected(Value::Object(error)) = vm.promises[&promise].status else {
        panic!("rejected conversion Promise");
    };
    assert_eq!(
        vm.heap.get(error, "message").unwrap(),
        Value::String("host functions accept primitive values only".into())
    );
    assert!(calls.borrow().is_empty());
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
}
#[cfg_attr(test, test)]
fn host_wait_storage_receiver_error_precedes_deferred_argument_conversion_error() {
    let (mut vm, calls) = fixture(0);
    vm.global("TypeError").unwrap();
    let family = vm.create_host_object_family().unwrap();
    let output = calls.clone();
    vm.install_host_async_object_method(
        family,
        "wait",
        0,
        HostTaskSource::UserInteraction,
        move |token, _key: HostObjectKey, _args: &[HostValue]| {
            output.borrow_mut().push(token);
            Ok(())
        },
    )
    .unwrap();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let roots = vm.heap.root_storage_state().0;
    let promise = vm
        .host_async_function_call(1, Value::Undefined, &[Value::Object(object)], false)
        .unwrap()
        .object_id()
        .unwrap();
    let PromiseStatus::Rejected(Value::Object(error)) = vm.promises[&promise].status else {
        panic!("rejected receiver TypeError Promise");
    };
    assert_eq!(
        vm.heap.get(error, "message").unwrap(),
        Value::String("Invalid host object receiver".into())
    );
    assert!(calls.borrow().is_empty());
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    assert_eq!(serial(&vm), 1);
}
#[cfg_attr(test, test)]
fn host_wait_storage_terminal_refusal_preserves_already_pending_owners() {
    let (mut vm, calls) = fixture(1);
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(call(&mut vm, &[]), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(vm.host_promises.pending.len(), 1);
    assert_eq!(serial(&vm), next);
    assert_eq!(calls.borrow().len(), 1);
}
#[cfg_attr(test, test)]
fn host_wait_storage_pending_completion_roots_survive_gc_and_release_after_delivery() {
    let (mut vm, calls) = fixture(0);
    let promise = call(&mut vm, &[]).unwrap().object_id().unwrap();
    let token = calls.borrow()[0];
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(promise));
    assert!(vm
        .queue_host_promise_completion(
            token,
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    assert_eq!(vm.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert!(
        matches!(vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::Number(value)) if value == 7.0)
    );
    assert!(vm.host_promises.pending.is_empty());
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise));
}
#[cfg_attr(test, test)]
fn host_wait_storage_collection_during_registration_keeps_existing_waits_alive() {
    let (mut vm, calls) = fixture(3);
    let old: Vec<_> = vm.promises.keys().copied().collect();
    let garbage = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let collections = vm.heap.stats().major_collections;
    vm.heap.allow_only(0);
    let promise = call(&mut vm, &[]).unwrap().object_id().unwrap();
    assert!(vm.heap.stats().major_collections > collections);
    assert!(!vm.heap.contains(garbage));
    assert!(old.iter().all(|object| vm.heap.contains(*object)));
    assert!(vm.heap.contains(promise));
    assert_eq!(vm.promises.len(), 4);
    assert_eq!(vm.host_promises.pending.len(), 4);
    assert_eq!(calls.borrow().len(), 4);
}
#[cfg_attr(test, test)]
fn host_wait_storage_gc_prunes_unrooted_records_before_joint_commit() {
    let (mut vm, calls) = fixture(1);
    let active = *vm.promises.keys().next().unwrap();
    let mut dead = Vec::new();
    let mut roots = Vec::new();
    for _ in 0..2 {
        let object = vm.new_promise().unwrap();
        roots.push(vm.heap.root(object).unwrap());
        dead.push(object);
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    for root in roots {
        vm.heap.unroot(root).unwrap();
    }
    vm.heap.allow_only(0);
    let promise = call(&mut vm, &[]).unwrap().object_id().unwrap();
    assert!(dead.iter().all(|object| !vm.heap.contains(*object)));
    assert!(vm.heap.contains(active));
    assert!(vm.heap.contains(promise));
    assert_eq!(vm.promises.len(), 2);
    assert_eq!(vm.promises.capacity(), 3);
    assert_eq!(vm.host_promises.pending.len(), 2);
    assert_eq!(calls.borrow().len(), 2);
}
#[cfg_attr(test, test)]
fn host_wait_storage_exact_joint_allowance_refunds_old_table_and_root_allocations() {
    let (mut vm, calls) = fixture(7);
    let root_growth = root_candidate_bytes(&vm);
    let old = host_scheduling::map_slots(&vm.promises)
        + host_scheduling::map_slots(&vm.host_promises.pending)
        + if root_growth == 0 {
            0
        } else {
            vm.heap.root_storage_bytes()
        };
    let bytes = replacement_bytes(&vm.promises)
        + replacement_bytes(&vm.host_promises.pending)
        + root_growth;
    let _peer = leave_room(&mut vm, bytes);
    call(&mut vm, &[]).unwrap();
    assert_eq!(vm.promises.len(), 8);
    assert_eq!(vm.host_promises.pending.len(), 8);
    assert_eq!(calls.borrow().len(), 8);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}
#[cfg_attr(test, test)]
fn host_wait_storage_one_byte_short_joint_root_allowance_keeps_every_native_owner() {
    let (mut vm, calls) = fixture(7);
    let root_growth = root_candidate_bytes(&vm);
    assert!(root_growth > 0);
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    let bytes = replacement_bytes(&vm.promises)
        + replacement_bytes(&vm.host_promises.pending)
        + root_growth
        - 1;
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(call(&mut vm, &[]), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), next);
    assert_eq!(vm.promises.len(), 7);
    assert_eq!(vm.promises.capacity(), 7);
    assert_eq!(vm.host_promises.pending.len(), 7);
    assert_eq!(vm.host_promises.pending.capacity(), 7);
    assert_eq!(calls.borrow().len(), 7);
}
struct Cancelled {
    calls: Calls,
    cancellations: Rc<RefCell<Vec<HostPromiseToken>>>,
}
impl HostAsyncFunction for Cancelled {
    fn start(
        &mut self,
        token: HostPromiseToken,
        _args: &[HostValue],
    ) -> Result<(), HostFunctionError> {
        self.calls.borrow_mut().push(token);
        Ok(())
    }
    fn cancel(&mut self, token: HostPromiseToken) {
        self.cancellations.borrow_mut().push(token);
    }
}
#[cfg_attr(test, test)]
fn host_wait_storage_vm_drop_cancels_the_committed_token_once_and_refunds_ownership() {
    let (mut vm, calls) = fixture(0);
    let cancellations = Rc::new(RefCell::new(Vec::new()));
    vm.install_host_async_function(
        "cancelled",
        0,
        HostTaskSource::UserInteraction,
        Cancelled {
            calls: calls.clone(),
            cancellations: cancellations.clone(),
        },
    )
    .unwrap();
    let observed = observer(&vm);
    vm.host_async_function_call(1, Value::Undefined, &[], false)
        .unwrap();
    let token = calls.borrow()[0];
    drop(vm);
    assert_eq!(*cancellations.borrow(), [token]);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_wait_storage_dropped_prepared_record_has_no_native_record_or_gc_root() {
    let (mut vm, calls) = fixture(0);
    let roots = vm.heap.root_storage_state();
    let growth =
        promise_queue_storage::PreparedMap::for_insert(&vm.promises, vm.host_scheduling.as_ref())
            .unwrap();
    let prepared = vm
        .prepare_promise_record_with_capacity(growth, None)
        .unwrap();
    let promise = prepared.promise;
    assert!(!vm.promises.contains_key(&promise));
    drop(prepared);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.promises.is_empty());
    assert!(calls.borrow().is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_wait_storage_contracts() {
        host_wait_storage_refuses_net_only_joint_table_growth_before_publication();
        host_wait_storage_root_id_refusal_keeps_serial_tables_and_callbacks_untouched();
        host_wait_storage_managed_refusal_does_not_publish_wait_table_capacity();
        host_wait_storage_arguments_exact_allowance_owns_actual_copies_and_slots();
        host_wait_storage_argument_refusal_precedes_callback_tables_and_managed_allocation();
        host_wait_storage_partial_conversion_refunds_buffers_and_retains_its_error_owner();
        host_wait_storage_rust_held_arguments_keep_their_document_lease_after_vm_drop();
        host_wait_storage_root_storage_refusal_keeps_unpublished_managed_objects_unrooted();
        host_wait_storage_serial_refusal_refunds_every_candidate_without_publishing_roots();
        host_wait_storage_managed_refusal_refunds_argument_and_map_leases_for_other_realms();
        host_wait_storage_scalar_count_refusal_precedes_native_or_managed_allocations();
        host_wait_storage_one_commit_publishes_exactly_one_root_record_wait_and_token();
        host_wait_storage_callback_observes_committed_wait_and_its_argument_lease();
        host_wait_storage_callback_error_keeps_rejected_promise_semantics_and_releases_wait_root();
        host_wait_storage_unsupported_arguments_reject_without_invoking_the_callback();
        host_wait_storage_receiver_error_precedes_deferred_argument_conversion_error();
        host_wait_storage_terminal_refusal_preserves_already_pending_owners();
        host_wait_storage_pending_completion_roots_survive_gc_and_release_after_delivery();
        host_wait_storage_collection_during_registration_keeps_existing_waits_alive();
        host_wait_storage_gc_prunes_unrooted_records_before_joint_commit();
        host_wait_storage_exact_joint_allowance_refunds_old_table_and_root_allocations();
        host_wait_storage_one_byte_short_joint_root_allowance_keeps_every_native_owner();
        host_wait_storage_vm_drop_cancels_the_committed_token_once_and_refunds_ownership();
        host_wait_storage_dropped_prepared_record_has_no_native_record_or_gc_root();
    }
}
