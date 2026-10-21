// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed root-context ownership and caller-cancellation regressions.

use super::*;
use crate::vm::argument_list_storage::ArgumentValues;
use crate::vm::native_value_storage::NativeValue;
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

fn input(source: &Vm) -> NativeValue {
    NativeValue::adopt(
        Value::String("tiny".into()),
        source.argument_list_owner().unwrap().as_ref(),
    )
    .unwrap()
}

#[cfg_attr(test, test)]
fn realm_root_affinity_native_loan_charges_nested_arguments_to_its_root_owner() {
    let source = page();
    let source_view = observer(&source);
    let mut caller = page();
    let caller_view = observer(&caller);
    let mut neutral = Vm::default();
    let value = input(&source);
    let active = realm_reentrancy::register_active(&mut caller).unwrap();
    let (source_delta, caller_delta, expected) = value
        .with_owned_context(&mut neutral, |vm, _| {
            let before_source = source_view.host_scheduling_snapshot().payload_bytes;
            let before_caller = caller_view.host_scheduling_snapshot().payload_bytes;
            let owner = vm.argument_list_owner()?;
            let arguments =
                ArgumentValues::single(vm, Value::String("copy".into()), owner.as_ref())?;
            let expected = host_scheduling::slots(&arguments.values)
                + host_scheduling::values_bytes(&arguments.values);
            Ok((
                source_view.host_scheduling_snapshot().payload_bytes - before_source,
                caller_view.host_scheduling_snapshot().payload_bytes - before_caller,
                expected,
            ))
        })
        .unwrap();
    drop(active);
    assert_eq!(source_delta, expected);
    assert_eq!(caller_delta, 0);
    assert!(neutral.realm_call_roots.is_none());
    assert!(neutral.host_scheduling.is_none());
}

fn registration_sample(ambient: bool) -> (usize, usize) {
    let source = page();
    let source_view = observer(&source);
    let mut caller = page();
    let caller_view = observer(&caller);
    let mut neutral = Vm::default();
    let value = input(&source);
    let active = ambient.then(|| realm_reentrancy::register_active(&mut caller).unwrap());
    let result = value
        .with_owned_context(&mut neutral, |vm, _| {
            let before_source = source_view.host_scheduling_snapshot().payload_bytes;
            let before_caller = caller_view.host_scheduling_snapshot().payload_bytes;
            let prepared = realm_reentrancy::prepare_active_registration(vm, None)?;
            let result = (
                source_view.host_scheduling_snapshot().payload_bytes - before_source,
                caller_view.host_scheduling_snapshot().payload_bytes - before_caller,
            );
            drop(prepared);
            Ok(result)
        })
        .unwrap();
    drop(active);
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    result
}

#[cfg_attr(test, test)]
fn realm_root_affinity_neutral_registration_preserves_root_owner_over_ambient_account() {
    let (source, caller) = registration_sample(true);
    assert!(source > 0);
    assert_eq!(caller, 0);
}

#[cfg_attr(test, test)]
fn realm_root_affinity_neutral_registration_uses_retained_owner_without_ambient_account() {
    let (source, caller) = registration_sample(false);
    assert!(source > 0);
    assert_eq!(caller, 0);
}

#[cfg_attr(test, test)]
fn realm_root_affinity_peer_fallback_does_not_replace_an_explicit_native_loan() {
    let source = page();
    let owner = source.argument_list_owner().unwrap().unwrap();
    let peer = page();
    let mut neutral = Vm::default();
    let retained = input(&source)
        .with_owned_context(&mut neutral, |vm, _| {
            Ok(vm
                .realm_call_native_owner(&peer)?
                .unwrap()
                .same_owner(&owner))
        })
        .unwrap();
    assert!(retained);
}

fn cancelled_caller(language_error: bool) {
    let source = page();
    let mut caller = page();
    let scope = caller.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    let value = input(&source);
    let active = realm_reentrancy::register_active(&mut caller).unwrap();
    let result = value.with_owned_context(&mut neutral, |_, _| {
        let _ = scope.fail();
        if language_error {
            Err(RuntimeError::TypeError("tiny".into()))
        } else {
            Ok(7)
        }
    });
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(source.check_scheduling_failure().is_ok());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_last_callback_success_still_checks_the_calling_account() {
    cancelled_caller(false);
}

#[cfg_attr(test, test)]
fn realm_root_affinity_last_callback_error_still_checks_the_calling_account() {
    cancelled_caller(true);
}

#[cfg_attr(test, test)]
fn realm_root_affinity_failed_root_owner_cannot_prepare_from_a_healthy_ambient_account() {
    let source = page();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let mut caller = page();
    let mut neutral = Vm::default();
    let prepared = Cell::new(false);
    let value = input(&source);
    let active = realm_reentrancy::register_active(&mut caller).unwrap();
    let result = value.with_owned_context(&mut neutral, |vm, _| {
        let _ = scope.fail();
        let owner = vm.argument_list_owner()?;
        let _arguments = ArgumentValues::single(vm, Value::Number(7.0), owner.as_ref())?;
        prepared.set(true);
        Ok(())
    });
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!prepared.get());
    assert!(caller.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_failed_prepared_frame_is_refused_before_guest_entry() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let roots = RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let mut neutral = Vm::default();
    let entered = Cell::new(false);
    let result = neutral.with_realm_call_roots(roots, |_| {
        entered.set(true);
        Ok(())
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!entered.get());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_owned_document_ignores_an_unrelated_failed_ambient_ancestor() {
    let mut vm = page();
    let mut unrelated = page();
    let value = input(&vm);
    let active = realm_reentrancy::register_active(&mut unrelated).unwrap();
    let _ = unrelated.host_scheduling.as_ref().unwrap().fail();
    let result = value.with_owned_context(&mut vm, |vm, _| {
        vm.argument_list_owner()?;
        Ok(7)
    });
    drop(active);
    assert_eq!(result, Ok(7));
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_neutral_calls_without_retained_context_use_the_ambient_owner() {
    let mut source = page();
    let owner = source.argument_list_owner().unwrap().unwrap();
    let neutral = Vm::default();
    let active = realm_reentrancy::register_active(&mut source).unwrap();
    let same = neutral
        .argument_list_owner()
        .unwrap()
        .unwrap()
        .same_owner(&owner);
    drop(active);
    assert!(same);
}

#[cfg_attr(test, test)]
fn realm_root_affinity_legacy_frame_adopts_credit_before_guest_and_keeps_it_until_drop() {
    let mut source = page();
    let view = observer(&source);
    let roots = RealmCallRoots::new(0, None).unwrap();
    let expected = roots.retained_bytes();
    let mut neutral = Vm::default();
    let active = realm_reentrancy::register_active(&mut source).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let during = neutral
        .with_realm_call_roots(roots.clone(), |_| {
            Ok(view.host_scheduling_snapshot().payload_bytes)
        })
        .unwrap();
    let after = view.host_scheduling_snapshot().payload_bytes;
    drop(roots);
    let released = view.host_scheduling_snapshot().payload_bytes;
    drop(active);
    assert_eq!(during - before, expected);
    assert_eq!(after - before, expected);
    assert_eq!(released, before);
}

#[cfg_attr(test, test)]
fn realm_root_affinity_foreign_object_is_refused_before_the_consumer_runs() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let mut foreign = Vm::default();
    let object = foreign.heap.alloc_object(None).unwrap();
    let mut neutral = Vm::default();
    let entered = Cell::new(false);
    let result = NativeValue::adopt(Value::Object(object), owner.as_ref())
        .unwrap()
        .with_owned_context(&mut neutral, |_, _| {
            entered.set(true);
            Ok(())
        });
    assert_eq!(
        result,
        Err(RuntimeError::Heap(HeapError::InvalidObject(object)))
    );
    assert!(!entered.get());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_normal_gc_keeps_input_only_until_its_consumer_finishes() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = neutral.heap.alloc_object(None).unwrap();
    let alive = NativeValue::adopt(Value::Object(object), owner.as_ref())
        .unwrap()
        .with_owned_context(&mut neutral, |vm, _| {
            vm.collect_realm_garbage()?;
            Ok(vm.heap.contains(object))
        })
        .unwrap();
    assert!(alive);
    neutral.collect_realm_garbage().unwrap();
    assert!(!neutral.heap.contains(object));
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_collected_serial_is_refused_before_the_consumer_runs() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = neutral.heap.alloc_object(None).unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    neutral.collect_realm_garbage().unwrap();
    assert!(!neutral.heap.contains(object));
    let replacement = neutral.heap.alloc_object(None).unwrap();
    assert_ne!(replacement, object);
    let entered = Cell::new(false);
    let result = neutral.with_realm_call_roots(roots, |_| {
        entered.set(true);
        Ok(())
    });
    assert_eq!(
        result,
        Err(RuntimeError::Heap(HeapError::InvalidObject(object)))
    );
    assert!(!entered.get());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_sequential_frame_reuse_restores_context_after_error() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let roots = RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let mut neutral = Vm::default();
    let failed: Result<(), RuntimeError> = neutral.with_realm_call_roots(roots.clone(), |_| {
        Err(RuntimeError::TypeError("tiny".into()))
    });
    assert_eq!(failed, Err(RuntimeError::TypeError("tiny".into())));
    assert!(neutral.realm_call_roots.is_none());
    let same = neutral
        .with_realm_call_roots(roots, |vm| {
            Ok(vm
                .argument_list_owner()?
                .unwrap()
                .same_owner(owner.as_ref().unwrap()))
        })
        .unwrap();
    assert!(same);
    assert!(neutral.realm_call_roots.is_none());
}

fn nested_registration_cancellation(peer: bool) {
    let source = page();
    let mut caller = page();
    let scope = caller.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    let mut other = Vm::default();
    let prepared = Cell::new(false);
    let value = input(&source);
    let active = realm_reentrancy::register_active(&mut caller).unwrap();
    let result = value.with_owned_context(&mut neutral, |vm, _| {
        let registration = realm_reentrancy::prepare_active_registration(vm, None)?;
        let nested = registration.publish(vm).unwrap();
        let _ = scope.fail();
        let outcome = (|| {
            let target = if peer { &mut other } else { vm };
            let owner = target.argument_list_owner()?;
            let _arguments = ArgumentValues::single(target, Value::Number(7.0), owner.as_ref())?;
            prepared.set(true);
            Ok(())
        })();
        drop(nested);
        outcome
    });
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!prepared.get());
    assert!(source.check_scheduling_failure().is_ok());
    assert!(neutral.realm_call_roots.is_none());
    assert!(other.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn realm_root_affinity_nested_registration_keeps_the_original_calling_guard() {
    nested_registration_cancellation(false);
}

#[cfg_attr(test, test)]
fn realm_root_affinity_nested_neutral_peer_keeps_the_original_calling_guard() {
    nested_registration_cancellation(true);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_realm_root_affinity_contracts() {
        realm_root_affinity_native_loan_charges_nested_arguments_to_its_root_owner();
        realm_root_affinity_neutral_registration_preserves_root_owner_over_ambient_account();
        realm_root_affinity_neutral_registration_uses_retained_owner_without_ambient_account();
        realm_root_affinity_peer_fallback_does_not_replace_an_explicit_native_loan();
        realm_root_affinity_last_callback_success_still_checks_the_calling_account();
        realm_root_affinity_last_callback_error_still_checks_the_calling_account();
        realm_root_affinity_failed_root_owner_cannot_prepare_from_a_healthy_ambient_account();
        realm_root_affinity_failed_prepared_frame_is_refused_before_guest_entry();
        realm_root_affinity_owned_document_ignores_an_unrelated_failed_ambient_ancestor();
        realm_root_affinity_neutral_calls_without_retained_context_use_the_ambient_owner();
        realm_root_affinity_legacy_frame_adopts_credit_before_guest_and_keeps_it_until_drop();
        realm_root_affinity_foreign_object_is_refused_before_the_consumer_runs();
        realm_root_affinity_normal_gc_keeps_input_only_until_its_consumer_finishes();
        realm_root_affinity_collected_serial_is_refused_before_the_consumer_runs();
        realm_root_affinity_sequential_frame_reuse_restores_context_after_error();
        realm_root_affinity_nested_registration_keeps_the_original_calling_guard();
        realm_root_affinity_nested_neutral_peer_keeps_the_original_calling_guard();
    }
}
