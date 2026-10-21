// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed task lifetime and publication cases, without resource-limit probes.

use super::*;
use std::cell::Cell;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn loan<T>(
    vm: &mut Vm,
    source: &Vm,
    action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let owner = source.argument_list_owner()?;
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?;
    vm.with_realm_call_roots(roots, action)
}
fn callback(vm: &mut Vm) -> ObjectId {
    vm.install_host_function("callback", 1, |_: &[HostValue]| Ok(HostValue::Undefined))
        .unwrap();
    vm.execute_script(&crate::compile(&crate::parse("callback;").unwrap()).unwrap())
        .unwrap()
        .object_id()
        .unwrap()
}
fn bytes(vm: &Vm) -> usize {
    let element = std::mem::size_of_val(&vm.browser_tasks.as_slices().0[0]);
    vm.browser_tasks.capacity() * element
        + vm.browser_tasks
            .iter()
            .map(|task| match task {
                BrowserTask::DragString { text, .. } => text.owned_heap_payload_bytes().unwrap(),
                BrowserTask::HostPromise { completion, .. } => completion.payload_bytes(),
                BrowserTask::PromiseRejections { entries, .. } => host_scheduling::slots(entries),
                _ => 0,
            })
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn browser_task_lifetime_scoped_queue_outlives_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let callback = callback(&mut vm);
    vm.queue_drag_string_task(callback, "tiny".into()).unwrap();
    let expected = bytes(&vm);
    let tasks = std::mem::take(&mut vm.browser_tasks);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(tasks);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_borrowed_queue_outlives_vm() {
    let source = page();
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    loan(&mut vm, &source, |vm| {
        vm.queue_drag_string_task(callback, "tiny".into())
    })
    .unwrap();
    let expected = bytes(&vm);
    let tasks = std::mem::take(&mut vm.browser_tasks);
    drop(vm);
    let view = observer(&source);
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(tasks);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_original_backing_refuses_publication() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    loan(&mut vm, &source, |vm| {
        vm.queue_drag_string_task(callback, "tiny".into())
    })
    .unwrap();
    let roots = vm.heap.root_storage_state().0;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        loan(&mut vm, &caller, |vm| vm
            .queue_drag_string_task(callback, "next".into())),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state().0, roots);
    assert!(caller.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_prepared_count_outlives_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let prepared = vm
        .prepare_browser_task(HostSchedulingUsage::default())
        .unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_legacy_attachment_preserves_queue_count() {
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    vm.queue_drag_string_task(callback, "tiny".into()).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let expected = bytes(&vm);
    let tasks = std::mem::take(&mut vm.browser_tasks);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(tasks);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_completion_uses_pending_document_without_ambient_caller() {
    let source = page();
    let mut vm = Vm::default();
    let token = Rc::new(Cell::new(None));
    let output = token.clone();
    loan(&mut vm, &source, |vm| {
        vm.install_host_async_function(
            "wait",
            0,
            HostTaskSource::UserInteraction,
            move |value: HostPromiseToken, _: &[HostValue]| {
                output.set(Some(value));
                Ok(())
            },
        )?;
        vm.execute_script(&crate::compile(&crate::parse("wait();").unwrap()).unwrap())
    })
    .unwrap();
    assert!(vm
        .queue_host_promise_completion(
            token.get().unwrap(),
            HostPromiseCompletion::Resolve(HostValue::String("tiny".into()))
        )
        .unwrap());
    let expected = bytes(&vm);
    let tasks = std::mem::take(&mut vm.browser_tasks);
    drop(vm);
    let view = observer(&source);
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(tasks);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_rejection_batch_keeps_count_after_vm_drop() {
    let mut vm = page();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_rejection_tracking().unwrap();
    vm.execute_script(
        &crate::compile(&crate::parse("Promise.reject('tiny');0;").unwrap()).unwrap(),
    )
    .unwrap();
    vm.notify_host_rejections().unwrap();
    let view = observer(&vm);
    let expected = bytes(&vm);
    let tasks = std::mem::take(&mut vm.browser_tasks);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(tasks);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_normal_delivery_releases_content_and_count() {
    let mut vm = page();
    let callback = callback(&mut vm);
    vm.queue_drag_string_task(callback, "tiny".into()).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 1);
    vm.run_browser_tasks_bounded(1).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert!(vm.browser_tasks.is_empty());
}
#[cfg_attr(test, test)]
fn browser_task_lifetime_terminal_caller_keeps_healthy_task_queued() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    loan(&mut vm, &source, |vm| {
        vm.queue_drag_string_task(callback, "tiny".into())
    })
    .unwrap();
    let _ = caller.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        loan(&mut vm, &caller, |vm| vm.run_browser_tasks_bounded(1)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.browser_tasks.len(), 1);
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn browser_task_lifetime_active_rejection_counts_are_not_charged_twice() {
    let mut vm = page();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_rejection_tracking().unwrap();
    let view = observer(&vm);
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
        seen.set(seen.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    vm.execute_script(
        &crate::compile(
            &crate::parse(
                "onunhandledrejection=e=>{observe();e.preventDefault();};Promise.reject(7);0;",
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    vm.notify_host_rejections().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    vm.run_browser_tasks_bounded(1).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 0);
}

#[cfg_attr(test, test)]
fn browser_task_lifetime_original_owner_stops_delivery_before_callback() {
    let source = page();
    let mut vm = Vm::default();
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    vm.install_host_function("callback", 1, move |_: &[HostValue]| {
        seen.set(seen.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm
        .execute_script(&crate::compile(&crate::parse("callback;").unwrap()).unwrap())
        .unwrap()
        .object_id()
        .unwrap();
    loan(&mut vm, &source, |vm| {
        vm.queue_drag_string_task(callback, "tiny".into())
    })
    .unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.run_browser_tasks_bounded(1),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(vm.browser_tasks.len(), 1);
    drop(vm);
    assert_eq!(source.host_scheduling_snapshot().pending_browser_tasks, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_browser_task_lifetime_contracts() {
        browser_task_lifetime_active_rejection_counts_are_not_charged_twice();
        browser_task_lifetime_original_owner_stops_delivery_before_callback();
        browser_task_lifetime_scoped_queue_outlives_vm();
        browser_task_lifetime_borrowed_queue_outlives_vm();
        browser_task_lifetime_original_backing_refuses_publication();
        browser_task_lifetime_prepared_count_outlives_vm();
        browser_task_lifetime_legacy_attachment_preserves_queue_count();
        browser_task_lifetime_completion_uses_pending_document_without_ambient_caller();
        browser_task_lifetime_rejection_batch_keeps_count_after_vm_drop();
        browser_task_lifetime_normal_delivery_releases_content_and_count();
        browser_task_lifetime_terminal_caller_keeps_healthy_task_queued();
    }
}
