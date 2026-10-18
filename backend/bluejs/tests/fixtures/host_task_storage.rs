// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite timer/realm snapshots and scalar native allowances check ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn with_child() -> Vm {
    let mut vm = bounded_vm();
    vm.execute_script(&script("globalThis.child=new ShadowRealm();0;"))
        .unwrap();
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

#[cfg_attr(test, test)]
fn host_task_storage_realm_discovery_has_a_native_owner() {
    let vm = with_child();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = vm.host_task_realms().unwrap();
    assert_eq!(snapshot.len(), 1);
    assert!(vm.host_scheduling_snapshot().payload_bytes > before);
}

#[cfg_attr(test, test)]
fn host_task_storage_public_wake_refuses_unadmitted_realm_discovery() {
    let mut vm = with_child();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.next_host_task_wake(0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
}

#[cfg_attr(test, test)]
fn host_task_storage_due_timer_snapshot_has_a_native_owner() {
    let mut vm = bounded_vm();
    vm.install_host_timer_functions().unwrap();
    vm.execute_script(&script("setTimeout(()=>0,0);0;"))
        .unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = vm.snapshot_due_host_timers(0).unwrap();
    assert_eq!(snapshot.len(), 1);
    assert!(vm.host_scheduling_snapshot().payload_bytes > before);
}

fn timer_vm() -> Vm {
    let mut vm = bounded_vm();
    vm.install_host_timer_functions().unwrap();
    vm.execute_script(&script("setTimeout(()=>0,0);0;"))
        .unwrap();
    vm
}
fn abort_vm() -> Vm {
    let mut vm = bounded_vm();
    vm.install_web_file_api().unwrap();
    vm.execute_script(&script("globalThis.signal=AbortSignal.timeout(0);0;"))
        .unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    }
}

#[cfg_attr(test, test)]
fn host_task_storage_vector_refusal_preserves_prefix_and_allocation() {
    let mut vm = bounded_vm();
    let mut values = TaskVector::new(vm.host_scheduling.as_ref()).unwrap();
    for value in 0u64..4 {
        values.push(value).unwrap();
    }
    let pointer = values.as_ptr();
    let bytes = values.retained_bytes();
    let _peer = leave_room(&mut vm, bytes);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(values.push(4), Err(RuntimeError::SchedulingLimit));
    assert_eq!(&*values, &[0, 1, 2, 3]);
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!(values.retained_bytes(), bytes);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_task_storage_spare_slots_need_no_new_charge() {
    let mut vm = bounded_vm();
    let mut values = TaskVector::new(vm.host_scheduling.as_ref()).unwrap();
    values.push(1u64).unwrap();
    let bytes = values.retained_bytes();
    let _peer = leave_room(&mut vm, 0);
    values.push(2).unwrap();
    assert_eq!(values.retained_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn host_task_storage_exact_replacement_allowance_refunds_old_slots() {
    let mut vm = bounded_vm();
    let mut values = TaskVector::new(vm.host_scheduling.as_ref()).unwrap();
    for value in 0u64..4 {
        values.push(value).unwrap();
    }
    let old = values.retained_bytes();
    let _peer = leave_room(&mut vm, 2 * old);
    values.push(4).unwrap();
    assert_eq!(&*values, &[0, 1, 2, 3, 4]);
    assert_eq!(values.retained_bytes(), 2 * old);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}

#[cfg_attr(test, test)]
fn host_task_storage_rust_held_vector_outlives_the_vm() {
    let vm = bounded_vm();
    let observed = observer(&vm);
    let mut values = TaskVector::new(vm.host_scheduling.as_ref()).unwrap();
    values.push(7u64).unwrap();
    let bytes = values.retained_bytes();
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(values);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_task_storage_snapshot_drop_refunds_after_terminal_failure() {
    let mut vm = timer_vm();
    let snapshot = vm.snapshot_due_host_timers(0).unwrap();
    let bytes = snapshot.retained_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(snapshot);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
    assert_eq!(vm.host_timers.len(), 1);
}

#[cfg_attr(test, test)]
fn host_task_storage_discovery_snapshot_does_not_keep_realms_alive() {
    let mut vm = with_child();
    let weak = vm.shadow_realms.values().next().unwrap().vm.downgrade();
    let snapshot = vm.host_task_realms().unwrap();
    assert_eq!(weak.strong_count(), 1);
    let bytes = snapshot.retained_bytes()
        + snapshot
            .iter()
            .map(|realm| realm.retained_bytes())
            .sum::<usize>();
    vm.shadow_realms.clear();
    assert!(weak.upgrade().is_none());
    assert!(snapshot[0].read(&vm, |_| Ok(())).unwrap().is_none());
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
}

fn store_shadow(vm: &mut Vm, key: ObjectId, record: ShadowRealmRecord) {
    let prepared = vm
        .shadow_realms
        .prepare_set(&key, vm.host_scheduling.as_ref())
        .unwrap();
    vm.shadow_realms.commit(prepared);
    vm.shadow_realms.set(key, record);
}

fn attached_child(owner: &Vm) -> Vm {
    Vm {
        host_scheduling: owner.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Default::default()
    }
}
#[cfg_attr(test, test)]
fn host_task_storage_discovery_filters_documents_deduplicates_cycles_and_sorts() {
    let mut vm = bounded_vm();
    let child = ShadowRealmRecord::new(attached_child(&vm)).unwrap();
    let grandchild = ShadowRealmRecord::new(attached_child(&vm)).unwrap();
    let foreign = ShadowRealmRecord::new(bounded_vm()).unwrap();
    let object = vm.object_prototype;
    let array = vm.array_prototype;
    store_shadow(&mut child.vm.borrow_mut(), object, grandchild.clone());
    store_shadow(&mut grandchild.vm.borrow_mut(), object, child.clone());
    store_shadow(&mut vm, object, child.clone());
    store_shadow(&mut vm, array, child.clone());
    let alias = vm.heap.alloc_object(None).unwrap();
    store_shadow(&mut vm, alias, foreign.clone());
    let references = child.vm.strong_count();
    let snapshot = vm.host_task_realms().unwrap();
    let ids: Vec<_> = snapshot.iter().map(|record| record.heap).collect();
    assert_eq!(ids, [child.heap, grandchild.heap]);
    assert_eq!(child.vm.strong_count(), references);
    grandchild.vm.borrow_mut().shadow_realms.clear();
    child.vm.borrow_mut().shadow_realms.clear();
}

#[cfg_attr(test, test)]
fn host_task_storage_borrowed_descendant_is_skipped_and_scratch_is_refunded() {
    let vm = with_child();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let _borrow = record.vm.borrow_mut();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = vm.host_task_realms().unwrap();
    assert!(snapshot.is_empty());
    assert_eq!(snapshot.retained_bytes(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_task_storage_partial_discovery_refunds_queue_and_seen_storage() {
    let mut vm = with_child();
    let room = HashSet::<u64>::with_capacity(1).capacity() * std::mem::size_of::<u64>()
        + 4 * std::mem::size_of::<std::rc::Weak<RefCell<Vm>>>();
    let before = vm.heap.stats();
    let references = vm.shadow_realms.values().next().unwrap().vm.strong_count();
    let _peer = leave_room(&mut vm, room);
    assert!(matches!(
        vm.host_task_realms(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(
        vm.shadow_realms.values().next().unwrap().vm.strong_count(),
        references
    );
}

#[cfg_attr(test, test)]
fn host_task_storage_public_timer_refusal_preserves_registration_and_fuel() {
    let mut vm = timer_vm();
    let roots = vm.heap.stats();
    let fuel = vm.remaining_instructions;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_due_host_timers(0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_timers.len(), 1);
    assert_eq!(vm.heap.stats(), roots);
    assert_eq!(vm.remaining_instructions, fuel);
}

#[cfg_attr(test, test)]
fn host_task_storage_cancellation_and_new_timers_preserve_turn_snapshots() {
    let mut vm = bounded_vm();
    vm.install_host_timer_functions().unwrap();
    vm.execute_script(&script("var log=[];setTimeout(()=>{log.push(1);clearTimeout(second);setTimeout(()=>log.push(3),0);},0);var second=setTimeout(()=>log.push(2),0);0;")).unwrap();
    assert_eq!(vm.run_due_host_timers(0).unwrap().tasks_run, 1);
    assert_eq!(
        vm.execute_script(&script("log.join(',');")),
        Ok(Value::String("1".into()))
    );
    assert_eq!(vm.run_due_host_timers(0).unwrap().tasks_run, 1);
    assert_eq!(
        vm.execute_script(&script("log.join(',');")),
        Ok(Value::String("1,3".into()))
    );
    assert!(vm.host_timers.is_empty());
}

#[cfg_attr(test, test)]
fn host_task_storage_intervals_run_once_and_standalone_policy_is_preserved() {
    let mut vm = Vm::default();
    vm.install_host_timer_functions().unwrap();
    vm.execute_script(&script(
        "var count=0;var interval=setInterval(()=>count++,1);0;",
    ))
    .unwrap();
    assert_eq!(vm.run_due_host_timers(100).unwrap().tasks_run, 1);
    assert_eq!(vm.run_due_host_timers(100).unwrap().tasks_run, 0);
    assert_eq!(vm.run_due_host_timers(101).unwrap().tasks_run, 1);
    vm.execute_script(&script("clearInterval(interval);0;"))
        .unwrap();
    assert_eq!(vm.execute_script(&script("count;")), Ok(Value::Number(2.0)));
    assert!(vm.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn host_task_storage_abort_snapshot_retains_and_refunds_its_slots() {
    let mut vm = abort_vm();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = vm.snapshot_task_timers(false).unwrap();
    assert_eq!(snapshot.aborts.len(), 1);
    assert!(snapshot.callbacks.is_empty());
    let bytes = snapshot.aborts.retained_bytes();
    assert!(bytes > 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_task_storage_abort_refusal_preserves_queued_work() {
    let mut vm = abort_vm();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.snapshot_task_timers(false),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn host_task_storage_realm_timer_plan_retains_outer_and_nested_storage() {
    let mut vm = timer_vm();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = vm.snapshot_task_timers(true).unwrap();
    let nested = snapshot.callbacks.retained_bytes();
    let mut plan = TimerSnapshots::new(vm.host_scheduling.as_ref()).unwrap();
    plan.insert(vm.object_prototype.heap, snapshot).unwrap();
    plan.sort();
    assert_eq!(
        plan.get(&vm.object_prototype.heap).unwrap().callbacks.len(),
        1
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + nested + plan.entries.retained_bytes()
    );
    drop(plan);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_task_storage_outer_plan_refusal_refunds_the_incoming_snapshot() {
    let mut vm = timer_vm();
    let snapshot = vm.snapshot_task_timers(true).unwrap();
    let nested = snapshot.callbacks.retained_bytes();
    let mut plan = TimerSnapshots::new(vm.host_scheduling.as_ref()).unwrap();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        plan.insert(vm.object_prototype.heap, snapshot),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(plan.entries.is_empty());
    assert_eq!(vm.host_timers.len(), 1);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - nested
    );
}

#[cfg_attr(test, test)]
fn host_task_storage_nested_plan_outlives_the_document_vm() {
    let mut vm = timer_vm();
    let observed = observer(&vm);
    let snapshot = vm.snapshot_task_timers(true).unwrap();
    let nested = snapshot.callbacks.retained_bytes();
    let mut plan = TimerSnapshots::new(vm.host_scheduling.as_ref()).unwrap();
    plan.insert(vm.object_prototype.heap, snapshot).unwrap();
    let bytes = nested + plan.entries.retained_bytes();
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(plan);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_task_storage_empty_snapshots_need_no_capacity_at_full_scalar_quota() {
    let mut vm = bounded_vm();
    let _peer = leave_room(&mut vm, 0);
    assert!(vm.host_task_realms().unwrap().is_empty());
    assert!(vm.snapshot_due_host_timers(0).unwrap().is_empty());
    let snapshot = vm.snapshot_task_timers(true).unwrap();
    let mut plan = TimerSnapshots::new(vm.host_scheduling.as_ref()).unwrap();
    plan.insert(vm.object_prototype.heap, snapshot).unwrap();
    assert!(plan.entries.is_empty());
    assert_eq!(vm.next_host_task_wake(0), Ok(None));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn host_task_storage_browser_only_snapshot_excludes_callback_timers() {
    let mut vm = abort_vm();
    vm.install_host_timer_functions().unwrap();
    vm.execute_script(&script("setTimeout(()=>0,0);0;"))
        .unwrap();
    let snapshot = vm.snapshot_task_timers(false).unwrap();
    assert!(snapshot.callbacks.is_empty());
    assert_eq!(snapshot.aborts.len(), 1);
    drop(snapshot);
    let snapshot = vm.snapshot_task_timers(true).unwrap();
    assert_eq!(snapshot.callbacks.len(), 1);
    assert_eq!(snapshot.aborts.len(), 1);
    assert_eq!(vm.host_timers.len(), 1);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_task_storage_contracts() {
        host_task_storage_realm_discovery_has_a_native_owner();
        host_task_storage_public_wake_refuses_unadmitted_realm_discovery();
        host_task_storage_due_timer_snapshot_has_a_native_owner();
        host_task_storage_vector_refusal_preserves_prefix_and_allocation();
        host_task_storage_spare_slots_need_no_new_charge();
        host_task_storage_exact_replacement_allowance_refunds_old_slots();
        host_task_storage_rust_held_vector_outlives_the_vm();
        host_task_storage_snapshot_drop_refunds_after_terminal_failure();
        host_task_storage_discovery_snapshot_does_not_keep_realms_alive();
        host_task_storage_discovery_filters_documents_deduplicates_cycles_and_sorts();
        host_task_storage_borrowed_descendant_is_skipped_and_scratch_is_refunded();
        host_task_storage_partial_discovery_refunds_queue_and_seen_storage();
        host_task_storage_public_timer_refusal_preserves_registration_and_fuel();
        host_task_storage_cancellation_and_new_timers_preserve_turn_snapshots();
        host_task_storage_intervals_run_once_and_standalone_policy_is_preserved();
        host_task_storage_abort_snapshot_retains_and_refunds_its_slots();
        host_task_storage_abort_refusal_preserves_queued_work();
        host_task_storage_realm_timer_plan_retains_outer_and_nested_storage();
        host_task_storage_outer_plan_refusal_refunds_the_incoming_snapshot();
        host_task_storage_nested_plan_outlives_the_document_vm();
        host_task_storage_empty_snapshots_need_no_capacity_at_full_scalar_quota();
        host_task_storage_browser_only_snapshot_excludes_callback_timers();
    }
}
