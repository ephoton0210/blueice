// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed timer ownership, cancellation and ordinary GC cases without pressure.

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
    vm.with_realm_call_roots(
        realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?,
        action,
    )
}
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn callback(vm: &mut Vm) -> Value {
    vm.install_host_timer_functions().unwrap();
    vm.install_host_function("callback", 1, |_: &[HostValue]| Ok(HostValue::Undefined))
        .unwrap();
    run(vm, "callback;")
}
fn register(vm: &mut Vm, callback: Value, repeat: bool) -> Result<Value, RuntimeError> {
    vm.host_timer_call(
        if repeat {
            host_timers::HostTimerFunction::SetInterval
        } else {
            host_timers::HostTimerFunction::SetTimeout
        },
        &[callback, Value::Number(1.0), Value::String("tiny".into())],
        false,
    )
}
fn clear(vm: &mut Vm) -> Result<Value, RuntimeError> {
    vm.host_timer_call(
        host_timers::HostTimerFunction::Clear,
        &[Value::Number(1.0)],
        false,
    )
}
fn bytes(vm: &Vm) -> usize {
    host_scheduling::slots(&vm.host_timers)
        + vm.host_timers
            .iter()
            .map(HostTimer::scheduling_payload_bytes)
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn timer_lifetime_scoped_table_and_record_outlive_vm() {
    let mut vm = page();
    let callback = callback(&mut vm);
    register(&mut vm, callback, false).unwrap();
    let expected = bytes(&vm);
    let view = observer(&vm);
    let timers = std::mem::take(&mut vm.host_timers);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(timers);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn timer_lifetime_borrowed_table_and_record_keep_original_document() {
    let source = page();
    let view = observer(&source);
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    loan(&mut vm, &source, |vm| register(vm, callback, false)).unwrap();
    let expected = bytes(&vm);
    let timers = std::mem::take(&mut vm.host_timers);
    drop(vm);
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(timers);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn timer_lifetime_prepared_registration_keeps_count_after_vm_drop() {
    let mut vm = page();
    let prepared = vm
        .prepare_timer_registration(&[Value::String("tiny".into())], 1)
        .unwrap();
    let expected = host_scheduling::slots(&prepared.arguments)
        + host_scheduling::values_bytes(&prepared.arguments)
        + host_scheduling::slots(&prepared.roots)
        + prepared
            .replacement
            .as_ref()
            .map_or(0, host_scheduling::slots);
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn timer_lifetime_prepared_interval_copy_outlives_vm() {
    let mut vm = page();
    let callback = callback(&mut vm);
    register(&mut vm, callback, true).unwrap();
    let prepared = vm.prepare_timer_entry(0).unwrap();
    let expected = host_scheduling::slots(&prepared.arguments)
        + host_scheduling::values_bytes(&prepared.arguments);
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn timer_lifetime_original_backing_refuses_healthy_callers_publication() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    loan(&mut vm, &source, |vm| register(vm, callback.clone(), false)).unwrap();
    let roots = vm.heap.root_storage_state();
    let next = vm.host_timer_next_id;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        loan(&mut vm, &caller, |vm| register(vm, callback, false)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_timers.len(), 1);
    assert_eq!(vm.host_timer_next_id, next);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(caller.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn timer_lifetime_original_owner_refuses_delivery_before_native_callback() {
    let source = page();
    let mut vm = Vm::default();
    vm.install_host_timer_functions().unwrap();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    vm.install_host_function("callback", 1, move |_: &[HostValue]| {
        output.set(output.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = run(&mut vm, "callback;");
    loan(&mut vm, &source, |vm| register(vm, callback, false)).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.run_due_host_timers(1),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(vm.host_timers.len(), 1);
}
#[cfg_attr(test, test)]
fn timer_lifetime_legacy_attachment_preserves_detached_record_count() {
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    register(&mut vm, callback, false).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let expected = bytes(&vm);
    let view = observer(&vm);
    let timers = std::mem::take(&mut vm.host_timers);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(timers);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn timer_lifetime_cleared_table_keeps_original_zero_byte_identity() {
    let source = page();
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    loan(&mut vm, &source, |vm| register(vm, callback, false)).unwrap();
    clear(&mut vm).unwrap();
    assert_eq!(vm.host_timers.capacity(), 0);
    assert_eq!(source.host_scheduling_snapshot().pending_timers, 0);
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn timer_lifetime_normal_one_shot_releases_count_and_roots() {
    let mut vm = page();
    let callback = callback(&mut vm);
    register(&mut vm, callback, false).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_timers, 1);
    assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    assert_eq!(vm.host_scheduling_snapshot().pending_timers, 0);
    assert_eq!(vm.host_timers.capacity(), 0);
}
#[cfg_attr(test, test)]
fn timer_lifetime_interval_self_clear_and_promise_gc_preserve_argument() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(&mut vm, "var seen;setInterval(value=>{clearInterval(1);Promise.resolve(value).then(result=>{$262.gc();seen=result;});},1,'tiny');0;");
    assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut vm, "seen;"), Value::String("tiny".into()));
    assert_eq!(vm.host_scheduling_snapshot().pending_timers, 0);
    assert!(vm.host_timers.is_empty());
}

#[cfg_attr(test, test)]
fn timer_lifetime_due_snapshot_keeps_original_credit_after_vm_drop() {
    let source = page();
    let view = observer(&source);
    let mut vm = Vm::default();
    let callback = callback(&mut vm);
    loan(&mut vm, &source, |vm| register(vm, callback, false)).unwrap();
    let snapshot = vm.snapshot_due_host_timers(1).unwrap();
    let expected = snapshot.retained_bytes();
    assert_eq!(snapshot.len(), 1);
    drop(vm);
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().pending_timers, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn timer_lifetime_borrowed_interval_protects_argument_through_self_clear_and_gc() {
    let source = page();
    let mut vm = Vm::default();
    vm.install_host_timer_functions().unwrap();
    let callback = run(
        &mut vm,
        "var seen; (arg)=>{clearInterval(1);Promise.resolve(arg).then(value=>seen=value.marker);};",
    );
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.set(object, "marker", Value::Number(7.0)).unwrap();
    loan(&mut vm, &source, |vm| {
        vm.host_timer_call(
            host_timers::HostTimerFunction::SetInterval,
            &[callback, Value::Number(1.0), Value::Object(object)],
            false,
        )
    })
    .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(object));
    assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut vm, "seen;"), Value::Number(7.0));
    assert_eq!(source.host_scheduling_snapshot().pending_timers, 0);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
    assert!(vm.host_timers.is_empty() && vm.realm_call_roots.is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_timer_lifetime_contracts() {
        timer_lifetime_due_snapshot_keeps_original_credit_after_vm_drop();
        timer_lifetime_borrowed_interval_protects_argument_through_self_clear_and_gc();
        timer_lifetime_scoped_table_and_record_outlive_vm();
        timer_lifetime_borrowed_table_and_record_keep_original_document();
        timer_lifetime_prepared_registration_keeps_count_after_vm_drop();
        timer_lifetime_prepared_interval_copy_outlives_vm();
        timer_lifetime_original_backing_refuses_healthy_callers_publication();
        timer_lifetime_original_owner_refuses_delivery_before_native_callback();
        timer_lifetime_legacy_attachment_preserves_detached_record_count();
        timer_lifetime_cleared_table_keeps_original_zero_byte_identity();
        timer_lifetime_normal_one_shot_releases_count_and_roots();
        timer_lifetime_interval_self_clear_and_promise_gc_preserve_argument();
    }
}
