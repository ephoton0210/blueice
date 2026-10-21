// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny default-configuration abort storage, refusal and GC regressions.

use super::*;
use crate::heap::{AbortData, WebTargetData};
use std::cell::Cell;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    run(&mut vm, "var controller=new AbortController();0;");
    vm
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn signal(vm: &mut Vm) -> ObjectId {
    run(vm, "controller.signal").object_id().unwrap()
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

fn input_sample(capacity: usize, neutral: bool) -> (usize, usize) {
    let mut vm = page();
    let source = observer(&vm);
    let view = observer(&vm);
    let signal = signal(&mut vm);
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        assert!(output.get().is_none());
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        "controller.signal.onabort=function(){observe()};0;",
    );
    let mut units = Vec::with_capacity(capacity);
    units.push(u16::from(b'x'));
    let reason = Value::String(JsString::from_code_units(units));
    let actual = host_scheduling::value_payload_bytes(&reason);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    if neutral {
        loan(&mut vm, &source, |vm| vm.signal_abort(signal, reason)).unwrap();
    } else {
        vm.signal_abort(signal, reason).unwrap();
    }
    (actual, sample.get().unwrap())
}

#[cfg_attr(test, test)]
fn abort_storage_scoped_native_reason_capacity_stays_paid_during_callback() {
    let small = input_sample(128, false);
    let large = input_sample(256, false);
    assert_eq!(large.1 - small.1, large.0 - small.0);
}

#[cfg_attr(test, test)]
fn abort_storage_neutral_native_reason_capacity_stays_paid_during_callback() {
    let small = input_sample(128, true);
    let large = input_sample(256, true);
    assert_eq!(large.1 - small.1, large.0 - small.0);
}

#[cfg_attr(test, test)]
fn abort_storage_managed_signal_counts_all_retained_container_capacity() {
    let mut vm = page();
    let signal = signal(&mut vm);
    let state = AbortData {
        target: WebTargetData {
            listeners: Vec::with_capacity(3),
            next_serial: 0,
        },
        sources: Vec::with_capacity(4),
        dependents: Vec::with_capacity(5),
        hooks: Vec::with_capacity(6),
        ..AbortData::default()
    };
    let expected = host_scheduling::slots(&state.target.listeners)
        + host_scheduling::slots(&state.sources)
        + host_scheduling::slots(&state.dependents)
        + host_scheduling::slots(&state.hooks);
    let before = vm.heap.stats().managed_bytes;
    vm.heap.set_abort_signal(signal, state).unwrap();
    assert_eq!(vm.heap.stats().managed_bytes - before, expected);
}

#[cfg_attr(test, test)]
fn abort_storage_managed_reason_retains_spare_utf16_capacity() {
    let mut vm = page();
    let signal = signal(&mut vm);
    let mut units = Vec::with_capacity(16);
    units.push(u16::from(b'x'));
    let reason = Value::String(JsString::from_code_units(units));
    let expected = host_scheduling::value_payload_bytes(&reason);
    let before = vm.heap.stats().managed_bytes;
    vm.heap
        .set_abort_signal(
            signal,
            AbortData {
                reason,
                ..AbortData::default()
            },
        )
        .unwrap();
    assert_eq!(vm.heap.stats().managed_bytes - before, expected);
}

#[cfg_attr(test, test)]
fn abort_storage_managed_reason_counts_bigint_storage() {
    let mut vm = page();
    let signal = signal(&mut vm);
    let reason = Value::BigInt(42.into());
    let expected = host_scheduling::value_payload_bytes(&reason);
    let before = vm.heap.stats().managed_bytes;
    vm.heap
        .set_abort_signal(
            signal,
            AbortData {
                reason,
                ..AbortData::default()
            },
        )
        .unwrap();
    assert_eq!(vm.heap.stats().managed_bytes - before, expected);
}

#[cfg_attr(test, test)]
fn abort_storage_gc_does_not_refund_weak_edge_capacity_still_allocated() {
    let mut vm = page();
    let signal = signal(&mut vm);
    let dead = vm.heap.alloc_object(None).unwrap();
    let mut dependents = Vec::with_capacity(4);
    dependents.push(dead);
    vm.heap
        .set_abort_signal(
            signal,
            AbortData {
                dependents,
                ..AbortData::default()
            },
        )
        .unwrap();
    let before = vm.heap.abort_signal(signal).unwrap().bytes();
    let capacity = vm.heap.abort_signal(signal).unwrap().dependents.capacity();
    vm.collect_realm_garbage().unwrap();
    let state = vm.heap.abort_signal(signal).unwrap();
    assert!(state.dependents.is_empty());
    assert_eq!(state.dependents.capacity(), capacity);
    assert_eq!(state.bytes(), before);
    assert!(!vm.heap.contains(dead));
}

#[cfg_attr(test, test)]
fn abort_storage_failed_original_account_cannot_publish_from_another_caller() {
    let mut vm = page();
    let signal = signal(&mut vm);
    let original = observer(&vm);
    drop(vm.host_scheduling.take());
    let mut caller = Vm::default();
    caller.use_bounded_host_microtask_checkpoints().unwrap();
    let _ = original.host_scheduling.as_ref().unwrap().fail();
    let result = loan(&mut vm, &caller, |vm| {
        vm.signal_abort(signal, Value::Number(7.0))
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!vm.heap.abort_signal(signal).unwrap().aborted());
    assert!(caller.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn abort_storage_foreign_scoped_caller_is_refused_before_heap_reconfiguration() {
    let mut vm = page();
    let signal = signal(&mut vm);
    let original = observer(&vm);
    let mut caller = Vm::default();
    caller.use_bounded_host_microtask_checkpoints().unwrap();
    let foreign = observer(&caller);
    drop(vm.host_scheduling.take());
    vm.host_scheduling = caller.host_scheduling.take();
    assert_eq!(
        vm.signal_abort(signal, Value::Number(7.0)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!vm.heap.abort_signal(signal).unwrap().aborted());
    assert!(original.check_scheduling_failure().is_ok());
    assert_eq!(
        foreign.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn abort_storage_failed_calling_account_cannot_publish_a_reason() {
    let mut vm = page();
    let signal = signal(&mut vm);
    let original = observer(&vm);
    drop(vm.host_scheduling.take());
    let mut caller = Vm::default();
    caller.use_bounded_host_microtask_checkpoints().unwrap();
    let result = loan(&mut vm, &caller, |vm| {
        let _ = caller.host_scheduling.as_ref().unwrap().fail();
        vm.signal_abort(signal, Value::Number(7.0))
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!vm.heap.abort_signal(signal).unwrap().aborted());
    assert!(original.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn abort_storage_reason_and_pending_dependent_survive_normal_gc_then_release() {
    let mut vm = page();
    run(&mut vm, "var reason={marker:7}, combined=AbortSignal.any([controller.signal]), seen=0; combined.onabort=function(){seen=combined.reason.marker}; controller.abort(reason);0;");
    let reason = run(&mut vm, "reason").object_id().unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(reason));
    assert_eq!(run(&mut vm, "seen"), Value::Number(7.0));
    run(&mut vm, "controller=null;combined=null;reason=null;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(reason));
}

#[cfg_attr(test, test)]
fn abort_storage_listener_removal_releases_its_signal_root_when_detach_fails() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let key = HostObjectKey::new(7, 2, 11);
    vm.install_host_object_factory("node", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    vm.install_host_click_event_methods(family).unwrap();
    run(&mut vm, "var target=node();target.addEventListener('click',function(){},{signal:controller.signal});0;");
    let signal = signal(&mut vm);
    let serial = vm.next_host_listener_serial;
    run(&mut vm, "controller=null;0;");
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.remove_host_listener_serial(serial),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.host_click_listeners.is_empty());
    vm.collect_realm_garbage().unwrap();
    assert!(
        !vm.heap.contains(signal),
        "detached listener left a live persistent signal root"
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_abort_storage_contracts() {
        abort_storage_scoped_native_reason_capacity_stays_paid_during_callback();
        abort_storage_neutral_native_reason_capacity_stays_paid_during_callback();
        abort_storage_managed_signal_counts_all_retained_container_capacity();
        abort_storage_managed_reason_retains_spare_utf16_capacity();
        abort_storage_managed_reason_counts_bigint_storage();
        abort_storage_gc_does_not_refund_weak_edge_capacity_still_allocated();
        abort_storage_failed_original_account_cannot_publish_from_another_caller();
        abort_storage_foreign_scoped_caller_is_refused_before_heap_reconfiguration();
        abort_storage_failed_calling_account_cannot_publish_a_reason();
        abort_storage_reason_and_pending_dependent_survive_normal_gc_then_release();
        abort_storage_listener_removal_releases_its_signal_root_when_detach_fails();
    }
}
