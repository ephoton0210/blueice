// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Terminal cleanup releases only obsolete abort hooks and timeout roots.

use super::*;
use crate::vm::browser_tasks::BrowserTask;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    let key = HostObjectKey::new(7, 2, 11);
    vm.install_host_object_factory("node", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    vm.install_host_click_event_methods(family).unwrap();
    vm
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn loan<T>(
    vm: &mut Vm,
    caller: &Vm,
    action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let owner = caller.argument_list_owner()?;
    let roots = RealmCallRoots::new(0, owner.as_ref())?;
    vm.with_realm_call_roots(roots, action)
}

fn rooted(vm: &Vm, signal: ObjectId) -> bool {
    vm.browser_tasks
        .iter()
        .find_map(|task| match task {
            BrowserTask::AbortTimeout {
                signal: owner,
                root,
                ..
            } if *owner == signal => Some(root.is_some()),
            _ => None,
        })
        .expect("one queued deadline for the signal")
}

fn listening_timeout(vm: &mut Vm) -> (ObjectId, u64) {
    run(vm, "var signal=AbortSignal.timeout(1),target=node();target.addEventListener('click',function(){},{signal:signal});0;");
    let signal = run(vm, "signal").object_id().unwrap();
    assert!(rooted(vm, signal));
    (signal, vm.next_host_listener_serial)
}

#[cfg_attr(test, test)]
fn abort_cleanup_failed_owner_releases_removed_timeout_but_keeps_observed_peer() {
    let mut vm = page();
    let (signal, serial) = listening_timeout(&mut vm);
    run(
        &mut vm,
        "var peer=AbortSignal.timeout(2);peer.onabort=function(){};signal=null;target=null;0;",
    );
    let peer = run(&mut vm, "peer").object_id().unwrap();
    let registrations = vm.heap.stats().root_registrations;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.remove_host_listener_serial(serial),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.heap.abort_signal(signal).unwrap().hooks.is_empty());
    assert!(!rooted(&vm, signal));
    assert!(rooted(&vm, peer));
    assert_eq!(vm.heap.stats().root_registrations, registrations);
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(signal));
    assert!(vm.heap.contains(peer));
}

#[cfg_attr(test, test)]
fn abort_cleanup_failed_caller_refunds_original_timeout_without_failing_its_owner() {
    let mut vm = page();
    let (signal, serial) = listening_timeout(&mut vm);
    let original = observer(&vm);
    run(&mut vm, "signal=null;target=null;0;");
    drop(vm.host_scheduling.take());
    let mut caller = Vm::default();
    caller.use_bounded_host_microtask_checkpoints().unwrap();
    let result = loan(&mut vm, &caller, |vm| {
        let _ = caller.host_scheduling.as_ref().unwrap().fail();
        vm.remove_host_listener_serial(serial)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.heap.abort_signal(signal).unwrap().hooks.is_empty());
    assert!(!rooted(&vm, signal));
    assert!(original.check_scheduling_failure().is_ok());
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(signal));
}

#[cfg_attr(test, test)]
fn abort_cleanup_failed_owner_removes_the_web_hook_before_returning_failure() {
    let mut vm = page();
    run(&mut vm, "var controller=new AbortController(),target=new EventTarget();target.addEventListener('x',function(){},{signal:controller.signal});0;");
    let signal = run(&mut vm, "controller.signal").object_id().unwrap();
    let target = run(&mut vm, "target").object_id().unwrap();
    let hook = vm.heap.abort_signal(signal).unwrap().hooks[0];
    run(&mut vm, "target=null;0;");
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.detach_abort_hook(signal, hook),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.heap.abort_signal(signal).unwrap().hooks.is_empty());
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(target));
}

#[cfg_attr(test, test)]
fn abort_cleanup_callback_cancellation_still_allows_obsolete_timeout_root_release() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        "var signal=AbortSignal.timeout(1);signal.onabort=function(){cancel()};0;",
    );
    let signal = run(&mut vm, "signal").object_id().unwrap();
    assert_eq!(
        vm.signal_abort(signal, Value::Number(7.0)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.heap.abort_signal(signal).unwrap().aborted());
    assert!(rooted(&vm, signal));
    let registrations = vm.heap.stats().root_registrations;
    assert_eq!(
        vm.refresh_abort_timer_roots(signal),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!rooted(&vm, signal));
    assert_eq!(vm.heap.stats().root_registrations, registrations);
}

#[cfg_attr(test, test)]
fn abort_cleanup_healthy_listener_removal_preserves_ordinary_collection() {
    let mut vm = page();
    let (signal, serial) = listening_timeout(&mut vm);
    run(&mut vm, "signal=null;target=null;0;");
    vm.remove_host_listener_serial(serial).unwrap();
    assert!(vm.heap.abort_signal(signal).unwrap().hooks.is_empty());
    assert!(!rooted(&vm, signal));
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(signal));
}

#[cfg_attr(test, test)]
fn abort_cleanup_foreign_scope_cannot_modify_original_hooks_or_timeout_roots() {
    let mut vm = page();
    let (signal, serial) = listening_timeout(&mut vm);
    let original = observer(&vm);
    let mut foreign = Vm::default();
    foreign.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&foreign);
    drop(vm.host_scheduling.take());
    vm.host_scheduling = foreign.host_scheduling.take();
    assert_eq!(
        vm.detach_abort_hook(signal, AbortHook::Host { serial }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm
        .heap
        .abort_signal(signal)
        .unwrap()
        .hooks
        .contains(&AbortHook::Host { serial }));
    assert!(rooted(&vm, signal));
    assert!(original.check_scheduling_failure().is_ok());
    assert_eq!(
        view.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_abort_cleanup_contracts() {
        abort_cleanup_failed_owner_releases_removed_timeout_but_keeps_observed_peer();
        abort_cleanup_failed_caller_refunds_original_timeout_without_failing_its_owner();
        abort_cleanup_failed_owner_removes_the_web_hook_before_returning_failure();
        abort_cleanup_callback_cancellation_still_allows_obsolete_timeout_root_release();
        abort_cleanup_healthy_listener_removal_preserves_ordinary_collection();
        abort_cleanup_foreign_scope_cannot_modify_original_hooks_or_timeout_roots();
    }
}
