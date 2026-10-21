// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed deferred waiter records, dependency refunds and normal collection.

use super::*;

fn ready() -> (Vm, ObjectId, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    let promise = vm.new_promise().unwrap();
    let root = vm.heap.root(promise).unwrap();
    let namespace = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.enqueue_deferred_import_waiter(promise, namespace, vec!["entry".into(), "later".into()])
        .unwrap();
    vm.heap.unroot(root).unwrap();
    (vm, promise, namespace)
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn retained(vm: &Vm) -> usize {
    host_scheduling::slots(&vm.deferred_import_waiters)
        + vm.deferred_import_waiters
            .iter()
            .map(|waiter| waiter.pending.retained_bytes())
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_queue_keeps_records_backing_and_count_after_vm_drop() {
    let (mut vm, _, _) = ready();
    let bytes = retained(&vm);
    let view = observer(&vm);
    let queue = std::mem::take(&mut vm.deferred_import_waiters);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_removed_waiter_keeps_dependency_bytes_and_count() {
    let (mut vm, _, _) = ready();
    let view = observer(&vm);
    let waiter = vm.deferred_import_waiters.remove(0);
    let bytes = waiter.pending.retained_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(waiter);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_drained_queue_keeps_original_zero_byte_identity() {
    let (mut source, _, _) = ready();
    source.deferred_import_waiters.clear();
    source.reclaim_import_waiter_storage();
    let mut target = Vm {
        deferred_import_waiters: std::mem::take(&mut source.deferred_import_waiters),
        ..Vm::default()
    };
    assert_eq!(
        target.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_neutral_registration_retains_the_original_account() {
    let (mut vm, promise, namespace) = ready();
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    vm.enqueue_deferred_import_waiter(promise, namespace, vec!["third".into()])
        .unwrap();
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 2);
    let bytes = retained(&vm);
    let queue = std::mem::take(&mut vm.deferred_import_waiters);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 2);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_failed_original_owner_refuses_neutral_publication() {
    let (mut vm, promise, namespace) = ready();
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.enqueue_deferred_import_waiter(promise, namespace, vec!["third".into()]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.deferred_import_waiters.len(), 1);
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_dependency_removal_refunds_only_the_removed_name() {
    let (mut vm, _, _) = ready();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let bytes = vm.deferred_import_waiters[0]
        .pending
        .iter()
        .find(|name| name.as_str() == "entry")
        .unwrap()
        .capacity();
    vm.deferred_import_waiters[0].pending.remove("entry");
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before - bytes
    );
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(vm.deferred_import_waiters[0].pending.len(), 1);
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_partial_and_complete_delivery_keep_normal_gc() {
    let (mut vm, promise, namespace) = ready();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(promise) && vm.heap.contains(namespace));
    vm.settle_dynamic_import_waiters("entry", &mut HashMap::new())
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    assert!(vm.deferred_import_waiters[0].pending.contains("later"));
    vm.settle_dynamic_import_waiters("later", &mut HashMap::new())
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert!(vm.deferred_import_waiters.is_empty());
    assert!(
        matches!(vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == namespace)
    );
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise) && !vm.heap.contains(namespace));
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_legacy_fields_transfer_without_a_duplicate_peak() {
    let (mut vm, _, _) = ready();
    vm.deferred_import_waiters.storage = None;
    vm.deferred_import_waiters[0].pending.storage = None;
    vm.synchronize_scheduling_usage().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| vm.deferred_import_owner(None));
    drop(result.unwrap());
    assert_eq!(peak, 0);
    let view = observer(&vm);
    let bytes = retained(&vm);
    let queue = std::mem::take(&mut vm.deferred_import_waiters);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn deferred_import_lifetime_compaction_preserves_live_dependency_and_count_credit() {
    let (mut vm, promise, namespace) = ready();
    for _ in 0..7 {
        vm.enqueue_deferred_import_waiter(promise, namespace, vec!["entry".into()])
            .unwrap();
    }
    vm.deferred_import_waiters.truncate(1);
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.deferred_import_waiters.capacity(), 1);
    assert_eq!(vm.deferred_import_waiters[0].promise, promise);
    assert!(vm.deferred_import_waiters[0].pending.contains("later"));
    let view = observer(&vm);
    let bytes = retained(&vm);
    let queue = std::mem::take(&mut vm.deferred_import_waiters);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_deferred_import_lifetime_contracts() {
        deferred_import_lifetime_legacy_fields_transfer_without_a_duplicate_peak();
        deferred_import_lifetime_compaction_preserves_live_dependency_and_count_credit();
        deferred_import_lifetime_queue_keeps_records_backing_and_count_after_vm_drop();
        deferred_import_lifetime_removed_waiter_keeps_dependency_bytes_and_count();
        deferred_import_lifetime_drained_queue_keeps_original_zero_byte_identity();
        deferred_import_lifetime_neutral_registration_retains_the_original_account();
        deferred_import_lifetime_failed_original_owner_refuses_neutral_publication();
        deferred_import_lifetime_dependency_removal_refunds_only_the_removed_name();
        deferred_import_lifetime_partial_and_complete_delivery_keep_normal_gc();
    }
}
