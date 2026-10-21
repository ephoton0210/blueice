// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed admission-to-publication cancellation and ownership regressions.

use super::*;
use crate::vm::native_value_storage::NativeValue;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn attempt(prepared: ActiveRegistration, vm: &mut Vm) -> bool {
    let guard = prepared.publish(vm);
    let published = resolve_active(vm.object_prototype.heap).is_some();
    assert_eq!(guard.is_ok(), published);
    drop(guard);
    published
}

#[cfg_attr(test, test)]
fn active_realm_publication_cancelled_owner_keeps_outer_registration() {
    let mut outer = page();
    let outer_tag = outer.object_prototype.heap;
    let active = register_active(&mut outer).unwrap();
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let prepared = prepare_active_registration(&mut vm, None).unwrap();
    let _ = scope.fail();
    let published = attempt(prepared, &mut vm);
    let outer_preserved = resolve_active(outer_tag) == Some(&mut outer as *mut Vm);
    drop(active);
    assert!(
        !published,
        "cancelled metadata must not publish a VM pointer"
    );
    assert!(outer_preserved);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(outer.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_publication_cancelled_peer_hint_is_not_replaced_by_ambient_credit() {
    let owner = page();
    let scope = owner.host_scheduling.as_ref().unwrap();
    let before = owner.host_scheduling_snapshot().payload_bytes;
    let mut neutral = Vm::default();
    let prepared = prepare_active_registration(&mut neutral, Some(scope)).unwrap();
    let mut ambient = page();
    let active = register_active(&mut ambient).unwrap();
    let _ = scope.fail();
    let published = attempt(prepared, &mut neutral);
    drop(active);
    assert!(!published);
    assert_eq!(owner.host_scheduling_snapshot().payload_bytes, before);
    assert!(ambient.check_scheduling_failure().is_ok());
    assert!(neutral.host_scheduling.is_none());
}

fn retained_context(cancel_owner: bool, cancel_caller: bool) {
    let source = page();
    let source_scope = source.host_scheduling.as_ref().unwrap().child();
    let mut caller = page();
    let caller_scope = caller.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    let source_before = source.host_scheduling_snapshot().payload_bytes;
    let value = NativeValue::adopt(
        Value::String("tiny".into()),
        source.argument_list_owner().unwrap().as_ref(),
    )
    .unwrap();
    let caller_before = caller.host_scheduling_snapshot().payload_bytes;
    let active = register_active(&mut caller).unwrap();
    let prepared = value
        .with_owned_context(&mut neutral, |vm, _| prepare_active_registration(vm, None))
        .unwrap();
    drop(active);
    assert!(neutral.realm_call_roots.is_none());
    assert!(source.host_scheduling_snapshot().payload_bytes > source_before);
    if cancel_owner {
        let _ = source_scope.fail();
    }
    if cancel_caller {
        let _ = caller_scope.fail();
    }
    let published = attempt(prepared, &mut neutral);
    assert_eq!(published, !(cancel_owner || cancel_caller));
    assert_eq!(
        source.host_scheduling_snapshot().payload_bytes,
        source_before
    );
    assert_eq!(
        caller.host_scheduling_snapshot().payload_bytes,
        caller_before
    );
    assert!(neutral.host_scheduling.is_none());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_publication_cancelled_retained_root_owner_is_still_required() {
    retained_context(true, false);
}

#[cfg_attr(test, test)]
fn active_realm_publication_cancelled_retained_caller_is_still_required() {
    retained_context(false, true);
}

#[cfg_attr(test, test)]
fn active_realm_publication_healthy_retained_context_releases_its_original_credit() {
    retained_context(false, false);
}

#[cfg_attr(test, test)]
fn active_realm_publication_current_root_is_checked_before_pointer_publication() {
    let source = page();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let value = NativeValue::adopt(
        Value::String("tiny".into()),
        source.argument_list_owner().unwrap().as_ref(),
    )
    .unwrap();
    let mut neutral = Vm::default();
    let prepared = prepare_active_registration(&mut neutral, None).unwrap();
    let published = Cell::new(false);
    let result = value.with_owned_context(&mut neutral, |vm, _| {
        let _ = scope.fail();
        published.set(attempt(prepared, vm));
        Ok(())
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!published.get());
    assert!(neutral.realm_call_roots.is_none());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_publication_changed_document_policy_requires_new_preparation() {
    let mut vm = Vm::default();
    let prepared = prepare_active_registration(&mut vm, None).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert!(!attempt(prepared, &mut vm));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn active_realm_publication_moving_an_inactive_vm_preserves_its_identity() {
    let mut original = Vm::default();
    let prepared = prepare_active_registration(&mut original, None).unwrap();
    let mut moved = original;
    assert!(attempt(prepared, &mut moved));
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_publication_owned_document_ignores_unrelated_failed_ambient_work() {
    let mut vm = page();
    let prepared = prepare_active_registration(&mut vm, None).unwrap();
    let mut unrelated = page();
    let active = register_active(&mut unrelated).unwrap();
    let _ = unrelated.host_scheduling.as_ref().unwrap().fail();
    let published = attempt(prepared, &mut vm);
    drop(active);
    assert!(published);
    assert!(vm.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_publication_foreign_heap_is_refused_without_publishing_either_tag() {
    let mut vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let prepared = prepare_active_registration(&mut vm, None).unwrap();
    let mut other = page();
    let result = prepared.publish(&mut other);
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert!(resolve_active(vm.object_prototype.heap).is_none());
    assert!(resolve_active(other.object_prototype.heap).is_none());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.check_scheduling_failure().is_ok());
    assert!(other.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_active_realm_publication_contracts() {
        active_realm_publication_cancelled_owner_keeps_outer_registration();
        active_realm_publication_cancelled_peer_hint_is_not_replaced_by_ambient_credit();
        active_realm_publication_cancelled_retained_root_owner_is_still_required();
        active_realm_publication_cancelled_retained_caller_is_still_required();
        active_realm_publication_healthy_retained_context_releases_its_original_credit();
        active_realm_publication_current_root_is_checked_before_pointer_publication();
        active_realm_publication_changed_document_policy_requires_new_preparation();
        active_realm_publication_moving_an_inactive_vm_preserves_its_identity();
        active_realm_publication_owned_document_ignores_unrelated_failed_ambient_work();
        active_realm_publication_foreign_heap_is_refused_without_publishing_either_tag();
    }
}
