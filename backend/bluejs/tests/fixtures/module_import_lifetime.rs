// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed import waiter ownership and collection without resource pressure.

use super::*;

fn ready() -> (Vm, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let promise = vm.new_promise().unwrap();
    vm.enqueue_module_import_waiter("entry".into(), promise)
        .unwrap();
    (vm, promise)
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn retained(vm: &Vm) -> usize {
    vm.module_import_waiters.retained_bytes()
        + vm.module_import_waiters
            .iter()
            .map(|(name, waiters)| name.capacity() + host_scheduling::slots(waiters))
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn module_import_lifetime_table_keeps_count_keys_and_vectors_after_vm_drop() {
    let (mut vm, _) = ready();
    let bytes = retained(&vm);
    let view = observer(&vm);
    let table = std::mem::take(&mut vm.module_import_waiters);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(table);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_import_lifetime_drained_table_keeps_original_zero_byte_identity() {
    let (mut source, _) = ready();
    source.module_import_waiters.remove("entry");
    source.reclaim_import_waiter_storage();
    let mut target = Vm {
        module_import_waiters: std::mem::take(&mut source.module_import_waiters),
        ..Vm::default()
    };
    assert_eq!(
        target.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn module_import_lifetime_neutral_registration_uses_the_retained_original_account() {
    let (mut vm, promise) = ready();
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    vm.enqueue_module_import_waiter("entry".into(), promise)
        .unwrap();
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 2);
    let bytes = retained(&vm);
    let table = std::mem::take(&mut vm.module_import_waiters);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 2);
    drop(table);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn module_import_lifetime_failed_original_owner_refuses_neutral_publication() {
    let (mut vm, promise) = ready();
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.enqueue_module_import_waiter("entry".into(), promise),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_import_waiters["entry"].len(), 1);
}

#[cfg_attr(test, test)]
fn module_import_lifetime_duplicate_targets_survive_gc_until_waiters_are_removed() {
    let (mut vm, promise) = ready();
    vm.enqueue_module_import_waiter("entry".into(), promise)
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 2);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(promise));
    vm.module_import_waiters.remove("entry");
    vm.reclaim_import_waiter_storage();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise));
}

#[cfg_attr(test, test)]
fn module_import_lifetime_legacy_fields_transfer_without_a_duplicate_peak() {
    let (mut vm, _) = ready();
    {
        let mut old = std::mem::take(&mut vm.module_import_waiters);
        let (name, waiters) = old.remove_entry("entry").unwrap();
        vm.module_import_waiters.reserve(1);
        vm.module_import_waiters.insert(
            name,
            ModuleImportWaiterGroup {
                data: waiters.data,
                name_bytes: 0,
                storage: None,
            },
        );
    }
    vm.synchronize_scheduling_usage().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| vm.module_import_owner());
    drop(result.unwrap());
    assert_eq!(peak, 0);
    let view = observer(&vm);
    let bytes = retained(&vm);
    let table = std::mem::take(&mut vm.module_import_waiters);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(table);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_module_import_lifetime_contracts() {
        module_import_lifetime_legacy_fields_transfer_without_a_duplicate_peak();
        module_import_lifetime_table_keeps_count_keys_and_vectors_after_vm_drop();
        module_import_lifetime_drained_table_keeps_original_zero_byte_identity();
        module_import_lifetime_neutral_registration_uses_the_retained_original_account();
        module_import_lifetime_failed_original_owner_refuses_neutral_publication();
        module_import_lifetime_duplicate_targets_survive_gc_until_waiters_are_removed();
    }
}
