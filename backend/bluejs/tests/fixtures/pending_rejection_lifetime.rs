// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed rejection-notification owners and transfer cases without pressure.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_rejection_tracking().unwrap();
    vm.execute_script(
        &crate::compile(
            &crate::parse("var seen=0;onunhandledrejection=e=>{seen++;e.preventDefault();};0;")
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn ready() -> (Vm, ObjectId) {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.settle_promise(
        promise,
        PromiseStatus::Rejected(Value::String("tiny".into())),
    )
    .unwrap();
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 1);
    (vm, promise)
}

#[cfg_attr(test, test)]
fn pending_rejection_lifetime_state_keeps_count_and_backing_after_vm_drop() {
    let (mut vm, _) = ready();
    let bytes = host_scheduling::slots(&vm.host_rejections.as_ref().unwrap().pending);
    let view = observer(&vm);
    let state = vm.host_rejections.take().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(state);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn pending_rejection_lifetime_handled_state_keeps_actual_empty_capacity() {
    let (mut vm, promise) = ready();
    vm.mark_promise_handled(promise);
    let state = vm.host_rejections.take().unwrap();
    assert!(state.pending.is_empty());
    let bytes = host_scheduling::slots(&state.pending);
    assert!(bytes > 0);
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(state);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn pending_rejection_lifetime_transferred_state_keeps_original_zero_byte_identity() {
    let (mut source, _) = ready();
    source.notify_host_rejections().unwrap();
    assert!(source.run_browser_task_turn(1).unwrap());
    let state = source.host_rejections.take();
    assert_eq!(state.as_ref().unwrap().pending.capacity(), 0);
    let mut target = Vm {
        host_rejections: state,
        ..Vm::default()
    };
    assert_eq!(
        target.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn pending_rejection_lifetime_normal_transfer_and_delivery_preserve_one_count() {
    let (mut vm, _) = ready();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    vm.notify_host_rejections().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert!(vm.run_browser_task_turn(1).unwrap());
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 0);
    let seen = vm
        .execute_script(&crate::compile(&crate::parse("seen;").unwrap()).unwrap())
        .unwrap();
    assert_eq!(seen, Value::Number(1.0));
}
#[cfg_attr(test, test)]
fn pending_rejection_lifetime_terminal_handling_releases_count_and_root() {
    let (mut vm, promise) = ready();
    let roots = vm.heap.root_storage_state().0;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    vm.mark_promise_handled(promise);
    assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    assert_eq!(
        vm.argument_list_owner().err(),
        Some(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn pending_rejection_lifetime_legacy_field_credit_transfers_without_a_duplicate_peak() {
    let (mut vm, _) = ready();
    {
        let state = vm.host_rejections.take().unwrap();
        vm.host_rejections = Some(HostRejections {
            pending: state.pending,
            ..Default::default()
        });
    }
    vm.synchronize_scheduling_usage().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| vm.pending_rejection_owner());
    drop(result.unwrap());
    assert_eq!(peak, 0);
    let view = observer(&vm);
    let state = vm.host_rejections.take().unwrap();
    let bytes = host_scheduling::slots(&state.pending);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(state);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn pending_rejection_lifetime_browser_publication_charges_only_new_queue_backing() {
    let (mut vm, _) = ready();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| vm.notify_host_rejections());
    result.unwrap();
    let backing = vm.browser_tasks.backing_bytes();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + backing
    );
    assert_eq!(peak, backing);
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_pending_rejection_lifetime_contracts() {
        pending_rejection_lifetime_legacy_field_credit_transfers_without_a_duplicate_peak();
        pending_rejection_lifetime_browser_publication_charges_only_new_queue_backing();
        pending_rejection_lifetime_state_keeps_count_and_backing_after_vm_drop();
        pending_rejection_lifetime_handled_state_keeps_actual_empty_capacity();
        pending_rejection_lifetime_transferred_state_keeps_original_zero_byte_identity();
        pending_rejection_lifetime_normal_transfer_and_delivery_preserve_one_count();
        pending_rejection_lifetime_terminal_handling_releases_count_and_root();
    }
}
