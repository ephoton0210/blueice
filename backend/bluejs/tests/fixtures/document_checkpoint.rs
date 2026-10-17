// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny deterministic GC/checkpoint cases without threads or resource probes.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_test262_harness().unwrap();
    vm
}
fn child(vm: &mut Vm) -> ObjectId {
    run(vm, "var realm=$262.createRealm();0;");
    *vm.test262_realms.keys().next().unwrap()
}
fn browser_timers(vm: &mut Vm) {
    run(vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
}

#[cfg_attr(test, test)]
fn document_checkpoint_script_keeps_weak_target_until_its_promise_runs() {
    let mut root = page();
    run(&mut root,"var seen=false;var reference=new WeakRef({});Promise.resolve().then(()=>{$262.gc();seen=reference.deref()!==undefined;});0;");
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(run(&mut root, "seen;"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn document_checkpoint_root_target_survives_later_child_job() {
    let mut root = page();
    child(&mut root);
    run(&mut root,"var seen=false,reference;realm.global.probe=()=>{$262.gc();seen=reference.deref()!==undefined;return 0;};realm.evalScript('globalThis.queueProbe=()=>{Promise.resolve().then(()=>probe());return 0;};');Promise.resolve().then(()=>{reference=new WeakRef({});realm.global.queueProbe();});0;");
    root.run_host_task_turn(0, 0).unwrap();
    assert_eq!(run(&mut root, "seen;"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn document_checkpoint_child_target_survives_later_ancestor_job() {
    let mut root = page();
    child(&mut root);
    run(&mut root,"var seen=false;realm.global.queueParent=()=>{Promise.resolve().then(()=>seen=realm.global.probe());return 0;};realm.evalScript('var reference;globalThis.probe=()=>{$262.gc();return reference.deref()!==undefined;};globalThis.queueProbe=()=>{Promise.resolve().then(()=>{reference=new WeakRef({});queueParent();});return 0;};');Promise.resolve().then(()=>realm.global.queueProbe());0;");
    root.run_host_task_turn(0, 0).unwrap();
    assert_eq!(run(&mut root, "seen;"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn document_checkpoint_timer_keeps_target_through_ancestor_jobs() {
    let mut root = page();
    let id = child(&mut root);
    browser_timers(&mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    run(&mut root,"var seen=false;realm.global.queueParent=()=>{Promise.resolve().then(()=>seen=realm.global.probe());return 0;};realm.evalScript('var reference;globalThis.probe=()=>{$262.gc();return reference.deref()!==undefined;};setTimeout(()=>{reference=new WeakRef({});queueParent();},1);0;');0;");
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut root, "seen;"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn document_checkpoint_late_child_handler_avoids_a_spurious_rejection_task() {
    let mut root = page();
    child(&mut root);
    root.install_web_file_api().unwrap();
    root.install_host_console().unwrap();
    root.install_host_error_reporting().unwrap();
    root.install_host_rejection_tracking().unwrap();
    run(&mut root,"var pending;realm.global.handle=()=>{pending.catch(()=>0);return 0;};realm.evalScript('globalThis.queueHandler=()=>{Promise.resolve().then(()=>handle());return 0;};');Promise.resolve().then(()=>{pending=Promise.reject('handled in the same checkpoint');realm.global.queueHandler();});0;");
    let turn = root.run_host_task_turn(0, 0).unwrap();
    assert_eq!(
        turn.next_wake_in_ms, None,
        "a handler attached before document quiescence needs no rejection notification task"
    );
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn rejection_bindings(vm: &mut Vm) {
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_rejection_tracking().unwrap();
}
fn shadow(vm: &mut Vm) -> ShadowRealmRecord {
    run(vm, "var shadow=new ShadowRealm();0;");
    vm.shadow_realms.values().next().unwrap().clone()
}

#[cfg_attr(test, test)]
fn document_checkpoint_completed_targets_and_backing_are_released() {
    let mut root = page();
    run(&mut root, "var reference=new WeakRef({});0;");
    assert!(root.kept_weak_objects.retained_bytes() > 0);
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(root.kept_weak_objects.retained_bytes(), 0);
    assert_eq!(
        run(&mut root, "$262.gc();reference.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn document_checkpoint_duplicate_keep_reuses_the_same_native_owner() {
    let mut root = page();
    run(
        &mut root,
        "var target={};var reference=new WeakRef(target);0;",
    );
    let scope = root.host_scheduling.as_ref().unwrap().child();
    let target = root.kept_weak_objects[0];
    let before = root.host_scheduling_snapshot().payload_bytes;
    let pointer = root.kept_weak_objects.as_ptr();
    root.kept_weak_objects.keep(target, Some(&scope)).unwrap();
    assert_eq!(root.kept_weak_objects.len(), 1);
    assert_eq!(root.kept_weak_objects.as_ptr(), pointer);
    assert_eq!(root.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn document_checkpoint_displaced_kept_buffer_outlives_its_vm() {
    let mut root = page();
    run(&mut root, "var reference=new WeakRef({});0;");
    let view = observer(&root);
    let kept = std::mem::take(&mut root.kept_weak_objects);
    let bytes = kept.retained_bytes();
    assert!(bytes > 0);
    drop(root);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(kept);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn document_checkpoint_terminal_keep_refuses_publication_and_allows_release() {
    let mut root = page();
    run(&mut root, "var reference=new WeakRef({});0;");
    let next = root.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let scope = root.host_scheduling.as_ref().unwrap().child();
    let bytes = root.kept_weak_objects.retained_bytes();
    let before = root.host_scheduling_snapshot().payload_bytes;
    scope.fail();
    assert!(matches!(
        root.kept_weak_objects.keep(next, Some(&scope)),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(root.kept_weak_objects.len(), 1);
    assert_eq!(root.kept_weak_objects.retained_bytes(), bytes);
    root.kept_weak_objects.clear();
    assert_eq!(
        root.host_scheduling_snapshot().payload_bytes,
        before - bytes
    );
}

#[cfg_attr(test, test)]
fn document_checkpoint_late_activation_attaches_existing_kept_storage() {
    let mut root = Vm::default();
    let target = root.with_roots(|heap| heap.alloc_object(None)).unwrap();
    root.kept_weak_objects.keep(target, None).unwrap();
    let bytes = root.kept_weak_objects.retained_bytes();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&root);
    let kept = std::mem::take(&mut root.kept_weak_objects);
    drop(root);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(kept);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn document_checkpoint_busy_member_keeps_targets_and_requests_retry() {
    let mut root = page();
    let record = shadow(&mut root);
    run(
        &mut root,
        "var reference;Promise.resolve().then(()=>{reference=new WeakRef({});});0;",
    );
    let borrowed = record.vm.borrow();
    let turn = root.run_host_task_turn(0, 0).unwrap();
    assert_eq!(turn.next_wake_in_ms, Some(0));
    assert!(root.checkpoint_completion_pending());
    let target = root.kept_weak_objects[0];
    root.collect_realm_garbage().unwrap();
    assert!(root.heap.contains(target));
    drop(borrowed);
    root.run_host_microtask_checkpoint().unwrap();
    assert!(!root.checkpoint_completion_pending());
    assert!(root.kept_weak_objects.is_empty());
    assert_eq!(
        run(&mut root, "$262.gc();reference.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn document_checkpoint_busy_member_defers_rejection_notification() {
    let mut root = page();
    rejection_bindings(&mut root);
    let record = shadow(&mut root);
    run(&mut root,"var notifications=0;onunhandledrejection=()=>notifications++;Promise.resolve().then(()=>{Promise.reject('pending');});0;");
    let borrowed = record.vm.borrow_mut();
    let turn = root.run_host_task_turn(0, 0).unwrap();
    assert_eq!(turn.next_wake_in_ms, Some(0));
    assert!(root.browser_tasks.is_empty());
    drop(borrowed);
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(root.browser_tasks.len(), 1);
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut root, "notifications;"), Value::Number(1.0));
}

#[cfg_attr(test, test)]
fn document_checkpoint_foreign_active_document_does_not_defer_completion() {
    let mut root = page();
    let mut foreign = page();
    run(&mut root, "var reference=new WeakRef({});0;");
    run(&mut foreign, "var reference=new WeakRef({});0;");
    let guard = realm_reentrancy::register_active(&mut foreign).unwrap();
    root.run_host_microtask_checkpoint().unwrap();
    assert!(root.kept_weak_objects.is_empty());
    assert_eq!(foreign.kept_weak_objects.len(), 1);
    assert!(foreign.checkpoint_completion_pending());
    drop(guard);
    foreign.run_host_microtask_checkpoint().unwrap();
    assert!(foreign.kept_weak_objects.is_empty());
}

#[cfg_attr(test, test)]
fn document_checkpoint_counts_catchable_errors_across_members_and_cleans_up() {
    let mut root = page();
    let id = child(&mut root);
    browser_timers(&mut root);
    browser_timers(&mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    run(&mut root,"realm.evalScript('globalThis.queueError=()=>{queueMicrotask(()=>{new WeakRef({});throw 2;});return 0;};');queueMicrotask(()=>{new WeakRef({});throw 1;});realm.global.queueError();0;");
    assert_eq!(root.run_host_microtask_checkpoint().unwrap(), 2);
    assert!(root.kept_weak_objects.is_empty());
    assert!(root.test262_realms[&id]
        .vm
        .borrow_mut()
        .kept_weak_objects
        .is_empty());
    assert!(!root.checkpoint_completion_pending());
}

#[cfg_attr(test, test)]
fn document_checkpoint_neutral_job_keeps_its_local_lifetime() {
    let mut root = Vm::default();
    root.install_test262_harness().unwrap();
    run(&mut root, "var reference=new WeakRef({});0;");
    assert!(root.kept_weak_objects.is_empty());
    assert_eq!(
        run(&mut root, "$262.gc();reference.deref()===undefined;"),
        Value::Bool(true)
    );
    assert!(root.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn document_checkpoint_reentrant_shadow_evaluation_preserves_inner_and_outer_targets() {
    let mut root = page();
    let record = shadow(&mut root);
    record.vm.borrow_mut().install_test262_harness().unwrap();
    assert_eq!(run(&mut root,"var call=shadow.evaluate('(callback)=>{globalThis.before=new WeakRef({});callback();$262.gc();return before.deref()!==undefined&&inner.deref()!==undefined;}');call(()=>shadow.evaluate('globalThis.inner=new WeakRef({});0;'));"),Value::Bool(true));
    assert_eq!(record.vm.borrow().kept_weak_objects.len(), 2);
    root.run_host_microtask_checkpoint().unwrap();
    assert!(record.vm.borrow().kept_weak_objects.is_empty());
    assert_eq!(
        run(
            &mut record.vm.borrow_mut(),
            "$262.gc();before.deref()===undefined&&inner.deref()===undefined;"
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn document_checkpoint_nested_deferrals_release_even_after_terminal_failure() {
    let root = page();
    let scope = root.host_scheduling.as_ref().unwrap().child();
    let first = scope.defer_checkpoint().unwrap();
    let second = scope.defer_checkpoint().unwrap();
    drop(first);
    assert!(scope.checkpoint_deferred());
    scope.fail();
    drop(second);
    assert!(!scope.checkpoint_deferred());
    let mut healthy = page();
    assert_eq!(healthy.run_host_microtask_checkpoint().unwrap(), 0);
}

#[cfg_attr(test, test)]
fn document_checkpoint_unhandled_notification_publishes_only_once() {
    let mut root = page();
    rejection_bindings(&mut root);
    run(&mut root,"var notifications=0;onunhandledrejection=()=>notifications++;Promise.resolve().then(()=>{Promise.reject('one');});0;");
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(root.browser_tasks.len(), 1);
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(root.browser_tasks.len(), 1);
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut root, "notifications;"), Value::Number(1.0));
}

#[cfg_attr(test, test)]
fn document_checkpoint_later_handler_preserves_rejectionhandled_delivery() {
    let mut root = page();
    child(&mut root);
    rejection_bindings(&mut root);
    run(&mut root,"var events=[],pending;onunhandledrejection=()=>events.push('unhandled');onrejectionhandled=()=>events.push('handled');Promise.resolve().then(()=>{pending=Promise.reject('later');});0;");
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    run(&mut root,"realm.global.handle=()=>{pending.catch(()=>0);return 0;};realm.evalScript('globalThis.queueHandler=()=>{Promise.resolve().then(()=>handle());return 0;};');Promise.resolve().then(()=>realm.global.queueHandler());0;");
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(&mut root, "events.join(',');"),
        Value::String("unhandled,handled".into())
    );
}

#[cfg_attr(test, test)]
fn document_checkpoint_fifth_kept_target_admits_full_replacement_storage() {
    let mut root = page();
    let scope = root.host_scheduling.as_ref().unwrap().child();
    for _ in 0..4 {
        let target = root.with_roots(|heap| heap.alloc_object(None)).unwrap();
        root.kept_weak_objects.keep(target, Some(&scope)).unwrap();
    }
    let old = root.kept_weak_objects.retained_bytes();
    let target = root.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let before = root.host_scheduling_snapshot().payload_bytes;
    let (result, peak) =
        scope.measure_payload_peak(|| root.kept_weak_objects.keep(target, Some(&scope)));
    result.unwrap();
    let new = root.kept_weak_objects.retained_bytes();
    assert!(new > old);
    assert_eq!(peak, new);
    assert_eq!(
        root.host_scheduling_snapshot().payload_bytes,
        before - old + new
    );
    assert_eq!(root.kept_weak_objects.len(), 5);
}

#[cfg_attr(test, test)]
fn document_checkpoint_kept_owner_rejects_foreign_rebinding_without_poisoning_source() {
    let mut root = page();
    let foreign = page();
    run(&mut root, "var reference=new WeakRef({});0;");
    let scope = root.host_scheduling.as_ref().unwrap().child();
    let foreign_scope = foreign.host_scheduling.as_ref().unwrap().child();
    let bytes = root.kept_weak_objects.retained_bytes();
    let before = root.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(
        root.kept_weak_objects.attach(&foreign_scope),
        Err(RuntimeError::SchedulingLimit)
    ));
    scope.shared_charge(0).unwrap();
    assert_eq!(root.kept_weak_objects.retained_bytes(), bytes);
    assert_eq!(root.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn document_checkpoint_direct_timer_entry_keeps_targets_and_orders_earlier_jobs() {
    let mut root = page();
    child(&mut root);
    browser_timers(&mut root);
    run(&mut root,"var seen=false,reference,events=[];realm.global.probe=()=>{$262.gc();seen=reference.deref()!==undefined;events.push('after');return 0;};realm.evalScript('globalThis.queueProbe=()=>{Promise.resolve().then(()=>probe());return 0;};');Promise.resolve().then(()=>events.push('before'));setTimeout(()=>{events.push('timer');reference=new WeakRef({});realm.global.queueProbe();},1);0;");
    let timer = root.run_due_host_timers(1).unwrap();
    assert_eq!(timer.tasks_run, 1);
    assert_eq!(timer.uncaught_errors, 0);
    assert_eq!(run(&mut root, "seen;"), Value::Bool(true));
    assert_eq!(
        run(&mut root, "events.join(',');"),
        Value::String("before,timer,after".into())
    );
    assert_eq!(
        run(&mut root, "$262.gc();reference.deref()===undefined;"),
        Value::Bool(true)
    );
}

fn throw_script(vm: &mut Vm) {
    let code = crate::compile(&crate::parse("throw 1;").unwrap()).unwrap();
    assert!(matches!(
        vm.execute_script(&code),
        Err(RuntimeError::Thrown(_))
    ));
}

#[cfg_attr(test, test)]
fn document_checkpoint_error_handler_kept_target_requests_cleanup_wake() {
    let mut root = page();
    rejection_bindings(&mut root);
    run(
        &mut root,
        "var reference;onerror=()=>{reference=new WeakRef({});return true;};0;",
    );
    throw_script(&mut root);
    assert_eq!(root.next_host_task_wake(0).unwrap(), Some(0));
    root.run_host_microtask_checkpoint().unwrap();
    assert!(root.kept_weak_objects.is_empty());
    assert_eq!(
        run(&mut root, "$262.gc();reference.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn document_checkpoint_error_handler_job_requests_cleanup_wake() {
    let mut root = page();
    rejection_bindings(&mut root);
    browser_timers(&mut root);
    run(
        &mut root,
        "var seen=0;onerror=()=>{queueMicrotask(()=>seen=1);return true;};0;",
    );
    throw_script(&mut root);
    assert_eq!(root.next_host_task_wake(0).unwrap(), Some(0));
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(run(&mut root, "seen;"), Value::Number(1.0));
}

#[cfg_attr(test, test)]
fn document_checkpoint_error_handler_rejection_requests_cleanup_wake() {
    let mut root = page();
    rejection_bindings(&mut root);
    run(
        &mut root,
        "onerror=()=>{Promise.reject('handler');return true;};0;",
    );
    throw_script(&mut root);
    assert_eq!(root.next_host_task_wake(0).unwrap(), Some(0));
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(root.browser_tasks.len(), 1);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_document_checkpoint_contracts() {
        document_checkpoint_error_handler_kept_target_requests_cleanup_wake();
        document_checkpoint_error_handler_job_requests_cleanup_wake();
        document_checkpoint_error_handler_rejection_requests_cleanup_wake();
        document_checkpoint_direct_timer_entry_keeps_targets_and_orders_earlier_jobs();
        document_checkpoint_script_keeps_weak_target_until_its_promise_runs();
        document_checkpoint_root_target_survives_later_child_job();
        document_checkpoint_child_target_survives_later_ancestor_job();
        document_checkpoint_timer_keeps_target_through_ancestor_jobs();
        document_checkpoint_late_child_handler_avoids_a_spurious_rejection_task();
        document_checkpoint_completed_targets_and_backing_are_released();
        document_checkpoint_duplicate_keep_reuses_the_same_native_owner();
        document_checkpoint_displaced_kept_buffer_outlives_its_vm();
        document_checkpoint_terminal_keep_refuses_publication_and_allows_release();
        document_checkpoint_late_activation_attaches_existing_kept_storage();
        document_checkpoint_busy_member_keeps_targets_and_requests_retry();
        document_checkpoint_busy_member_defers_rejection_notification();
        document_checkpoint_foreign_active_document_does_not_defer_completion();
        document_checkpoint_counts_catchable_errors_across_members_and_cleans_up();
        document_checkpoint_neutral_job_keeps_its_local_lifetime();
        document_checkpoint_reentrant_shadow_evaluation_preserves_inner_and_outer_targets();
        document_checkpoint_nested_deferrals_release_even_after_terminal_failure();
        document_checkpoint_unhandled_notification_publishes_only_once();
        document_checkpoint_later_handler_preserves_rejectionhandled_delivery();
        document_checkpoint_fifth_kept_target_admits_full_replacement_storage();
        document_checkpoint_kept_owner_rejects_foreign_rebinding_without_poisoning_source();
    }
}
