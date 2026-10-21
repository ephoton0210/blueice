// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny host-family identity, independent capacity and document contracts.

use super::*;

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

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn retained_bytes(vm: &Vm) -> usize {
    vm.host_object_families.capacity() * std::mem::size_of::<HostObjectFamilyState>()
        + vm.host_object_families
            .iter()
            .map(|family| {
                host_scheduling::map_slots(&family.wrappers)
                    + host_scheduling::map_slots(&family.keys_by_wrapper)
                    + family
                        .element_validator
                        .as_ref()
                        .map_or(0, |callback| std::mem::size_of_val(&**callback))
                    + family
                        .event_path
                        .as_ref()
                        .map_or(0, |callback| std::mem::size_of_val(&**callback))
            })
            .sum::<usize>()
}

fn loaned_family(vm: &mut Vm, source: &Vm) -> (HostObjectFamily, HostObjectKey, ObjectId) {
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let result = vm
        .with_realm_call_roots(roots, |vm| {
            let family = vm.create_host_object_family()?;
            let key = HostObjectKey::new(7, 4, 1);
            let object = vm.mint_host_family_wrapper(family, key)?;
            Ok((family, key, object))
        })
        .unwrap();
    assert!(vm.host_scheduling.is_none() && vm.realm_call_roots.is_none());
    result
}

#[cfg_attr(test, test)]
fn host_family_storage_registry_and_both_identity_maps_outlive_the_vm() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 1))
        .unwrap();
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    assert!(bytes > 0);
    let families = std::mem::take(&mut vm.host_object_families);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(families);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_family_storage_neutral_registry_adopts_nested_identity_backing() {
    let mut vm = Vm::default();
    let family = vm.create_host_object_family().unwrap();
    vm.mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 1))
        .unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    let families = std::mem::take(&mut vm.host_object_families);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(families);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_family_storage_original_termination_refuses_a_cached_identity() {
    let source = page();
    let mut vm = Vm::default();
    let (family, key, _) = loaned_family(&mut vm, &source);
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.mint_host_family_wrapper(family, key),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.host_object_families[family.index as usize]
            .wrappers
            .len(),
        1
    );
}

#[cfg_attr(test, test)]
fn host_family_storage_original_termination_precedes_wrapper_allocation() {
    let source = page();
    let mut vm = Vm::default();
    let (family, _, _) = loaned_family(&mut vm, &source);
    let before = vm.heap.stats();
    let before = before.nursery_objects + before.tenured_objects;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let result = vm.mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 2));
    let after = vm.heap.stats();
    assert_eq!(after.nursery_objects + after.tenured_objects, before);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.host_object_families[family.index as usize]
            .keys_by_wrapper
            .len(),
        1
    );
}

#[cfg_attr(test, test)]
fn host_family_storage_foreign_activation_preserves_original_registry_ownership() {
    let source = page();
    let mut vm = Vm::default();
    loaned_family(&mut vm, &source);
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!vm.document_policy_ready);
    assert!(source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
}

#[cfg_attr(test, test)]
fn host_family_storage_terminal_factory_result_cannot_publish_or_return_null() {
    for present in [false, true] {
        let source = page();
        let mut vm = Vm::default();
        let (family, _, _) = loaned_family(&mut vm, &source);
        let scope = source.host_scheduling.as_ref().unwrap().child();
        vm.install_host_object_factory("target", 0, family, move |_: &[HostValue]| {
            let _ = scope.fail();
            Ok(present.then_some(HostObjectKey::new(7, 4, 2)))
        })
        .unwrap();
        let owner = source.argument_list_owner().unwrap();
        let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
        let result = vm.with_realm_call_roots(roots, |vm| {
            vm.host_object_factory_call(0, Value::Undefined, &[], false)
        });
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert_eq!(
            vm.host_object_families[family.index as usize]
                .wrappers
                .len(),
            1
        );
        assert!(vm.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn host_family_storage_validator_and_path_boxes_keep_their_allocation_credit() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let flags = [7u64, 4, 1];
    vm.install_host_element_validator(family, move |key| {
        key.matches_owner(flags[0], flags[1]) && key.object() == flags[2]
    })
    .unwrap();
    let owner = [7u64, 4];
    vm.install_host_node_event_methods(family, move |key| {
        Ok(vec![HostObjectKey::new(owner[0], owner[1], key.object())])
    })
    .unwrap();
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    let families = std::mem::take(&mut vm.host_object_families);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(families);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_family_storage_terminal_loan_prevents_family_allocation_and_publication() {
    let source = page();
    let mut vm = Vm::default();
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let before = vm.heap.stats();
    let before = before.nursery_objects + before.tenured_objects;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let result = vm.with_realm_call_roots(roots, |vm| vm.create_host_object_family());
    let after = vm.heap.stats();
    assert_eq!(after.nursery_objects + after.tenured_objects, before);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(vm.host_object_families.is_empty());
}

#[cfg_attr(test, test)]
fn host_family_storage_public_cached_wrapper_survives_gc_with_stable_identity() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("target", 0, family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(7, 4, 1)))
    })
    .unwrap();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        "var wrapper=target(),weak=new WeakRef(wrapper);wrapper=null;0;",
    );
    vm.finish_host_checkpoint().unwrap();
    assert_eq!(
        run(
            &mut vm,
            "$262.gc();weak.deref()===target()&&target()===target();"
        ),
        Value::Bool(true)
    );
    assert_eq!(
        vm.host_object_families[family.index as usize]
            .wrappers
            .len(),
        1
    );
}

#[cfg_attr(test, test)]
fn host_family_storage_equal_keys_in_different_families_keep_separate_identity() {
    let mut vm = page();
    let first = vm.create_host_object_family().unwrap();
    let second = vm.create_host_object_family().unwrap();
    let key = HostObjectKey::new(7, 4, 1);
    let a = vm.mint_host_family_wrapper(first, key).unwrap();
    let b = vm.mint_host_family_wrapper(second, key).unwrap();
    assert_ne!(a, b);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.mint_host_family_wrapper(first, key), Ok(a));
    assert_eq!(vm.mint_host_family_wrapper(second, key), Ok(b));
}

#[cfg_attr(test, test)]
fn host_family_storage_terminal_method_result_cannot_report_success() {
    let source = page();
    let mut vm = Vm::default();
    let (family, _, object) = loaned_family(&mut vm, &source);
    let scope = source.host_scheduling.as_ref().unwrap().child();
    vm.install_host_object_method(
        family,
        "read",
        0,
        move |_: HostObjectKey, _: &[HostValue]| {
            let _ = scope.fail();
            Ok(HostValue::Number(7.0))
        },
    )
    .unwrap();
    assert_eq!(
        vm.host_object_method_call(0, Value::Object(object), &[], false),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn host_family_storage_terminal_pair_result_cannot_report_success() {
    let source = page();
    let mut vm = Vm::default();
    let (family, _, parent) = loaned_family(&mut vm, &source);
    let child = vm
        .mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 2))
        .unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    vm.install_host_object_pair_method(
        family,
        "append",
        1,
        move |_: HostObjectKey, _: HostObjectKey| {
            let _ = scope.fail();
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        vm.host_object_pair_method_call(0, Value::Object(parent), &[Value::Object(child)], false),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn host_family_storage_both_map_candidates_are_paid_before_wrapper_publication() {
    let mut vm = page();
    vm.create_host_object_family().unwrap();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let objects = vm.heap.stats();
    let owner = vm.argument_list_owner().unwrap();
    let forward = vm.host_object_families[0]
        .wrappers
        .prepare_insert_owned(owner.as_ref())
        .unwrap();
    let reverse = vm.host_object_families[0]
        .keys_by_wrapper
        .prepare_insert_owned(owner.as_ref())
        .unwrap();
    let bytes = forward.staging_bytes() + reverse.staging_bytes();
    assert!(bytes > 0);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    assert!(vm.host_object_families[0].wrappers.is_empty());
    assert!(vm.host_object_families[0].keys_by_wrapper.is_empty());
    let after = vm.heap.stats();
    assert_eq!(
        objects.nursery_objects + objects.tenured_objects,
        after.nursery_objects + after.tenured_objects
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop((forward, reverse));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_family_storage_prepared_registry_replacement_outlives_the_vm() {
    let mut vm = page();
    for _ in 0..4 {
        vm.create_host_object_family().unwrap();
    }
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let owner = vm.argument_list_owner().unwrap();
    let prepared = vm.host_object_families.prepare(owner.as_ref()).unwrap();
    let bytes = prepared.staging_bytes();
    assert!(bytes > 0);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    assert_eq!(vm.host_object_families.len(), 4);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_family_storage_neutral_callback_boxes_adopt_the_document() {
    let mut vm = Vm::default();
    let family = vm.create_host_object_family().unwrap();
    let flags = [7u64, 4, 1];
    vm.install_host_element_validator(family, move |key| {
        key.matches_owner(flags[0], flags[1]) && key.object() == flags[2]
    })
    .unwrap();
    let owner = [7u64, 4];
    vm.install_host_node_event_methods(family, move |key| {
        Ok(vec![HostObjectKey::new(owner[0], owner[1], key.object())])
    })
    .unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    let families = std::mem::take(&mut vm.host_object_families);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(families);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_family_storage_replacing_a_validator_returns_the_previous_box_credit() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let object = vm
        .mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 2))
        .unwrap();
    let flags = [7u64, 4, 1];
    vm.install_host_element_validator(family, move |key| {
        key.matches_owner(flags[0], flags[1]) && key.object() == flags[2]
    })
    .unwrap();
    assert!(!vm.is_host_element(object));
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let old = std::mem::size_of_val(
        &**vm.host_object_families[0]
            .element_validator
            .as_ref()
            .unwrap(),
    );
    let flag = [2u64];
    vm.install_host_element_validator(family, move |key| key.object() == flag[0])
        .unwrap();
    let new = std::mem::size_of_val(
        &**vm.host_object_families[0]
            .element_validator
            .as_ref()
            .unwrap(),
    );
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before - old + new
    );
    assert!(vm.is_host_element(object));
}

#[cfg_attr(test, test)]
fn host_family_storage_original_failure_preserves_a_healthy_callers_root_prefix() {
    let source = page();
    let other = page();
    let mut vm = Vm::default();
    let (family, key, _) = loaned_family(&mut vm, &source);
    let owner = other.argument_list_owner().unwrap();
    let outer = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let prefix = outer.clone();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    vm.with_realm_call_roots(outer, |vm| {
        let inner = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
        assert_eq!(
            vm.with_realm_call_roots(inner, |vm| vm.mint_host_family_wrapper(family, key)),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &prefix));
        Ok(())
    })
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.host_family_storage_can_join(source.host_scheduling.as_ref().unwrap()));
    assert!(!vm.host_family_storage_can_join(other.host_scheduling.as_ref().unwrap()));
    assert!(other
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
}

#[cfg_attr(test, test)]
fn host_family_storage_terminal_ancestry_callback_stops_before_event_progress() {
    let source = page();
    let mut vm = Vm::default();
    let (family, key, _) = loaned_family(&mut vm, &source);
    let scope = source.host_scheduling.as_ref().unwrap().child();
    vm.install_host_node_event_methods(family, move |key| {
        let _ = scope.fail();
        Ok(vec![key])
    })
    .unwrap();
    let before = vm.heap.stats();
    assert!(matches!(
        vm.host_node_event_path(family.index, key),
        Err(RuntimeError::SchedulingLimit)
    ));
    let after = vm.heap.stats();
    assert_eq!(
        before.nursery_objects + before.tenured_objects,
        after.nursery_objects + after.tenured_objects
    );
}

#[cfg_attr(test, test)]
fn host_family_storage_growing_both_maps_keeps_one_owner_per_allocation() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    for id in [1, 2, 3, 4] {
        vm.mint_host_family_wrapper(family, HostObjectKey::new(7, 4, id))
            .unwrap();
    }
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    let families = std::mem::take(&mut vm.host_object_families);
    assert_eq!(families[0].wrappers.len(), 4);
    assert_eq!(families[0].keys_by_wrapper.len(), 4);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(families);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_family_storage_manual_timer_and_promise_preserve_wrapper_identity_through_gc() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("target", 0, family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(7, 4, 1)))
    })
    .unwrap();
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(&mut vm,"var result=false;setTimeout(()=>{let wrapper=target();Promise.resolve(wrapper).then(value=>{$262.gc();result=value===target();});},1);0;");
    let turn = vm.run_due_host_timers(1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.uncaught_errors, 0);
    assert_eq!(vm.next_host_timer_due(), None);
    assert_eq!(run(&mut vm, "result;"), Value::Bool(true));
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_family_storage_healthy_host_callback_errors_keep_their_public_messages() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let parent = vm
        .mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 1))
        .unwrap();
    let child = vm
        .mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 2))
        .unwrap();
    vm.install_host_object_factory("target", 0, family, |_: &[HostValue]| {
        Err(HostFunctionError::new("factory marker"))
    })
    .unwrap();
    vm.install_host_object_method(family, "read", 0, |_: HostObjectKey, _: &[HostValue]| {
        Err(HostFunctionError::new("method marker"))
    })
    .unwrap();
    vm.install_host_object_pair_method(
        family,
        "append",
        1,
        |_: HostObjectKey, _: HostObjectKey| Err(HostFunctionError::new("pair marker")),
    )
    .unwrap();
    assert_eq!(
        vm.host_object_factory_call(0, Value::Undefined, &[], false),
        Err(RuntimeError::TypeError("factory marker".into()))
    );
    assert_eq!(
        vm.host_object_method_call(0, Value::Object(parent), &[], false),
        Err(RuntimeError::TypeError("method marker".into()))
    );
    assert_eq!(
        vm.host_object_pair_method_call(0, Value::Object(parent), &[Value::Object(child)], false),
        Err(RuntimeError::TypeError("pair marker".into()))
    );
    assert!(vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_family_storage_contracts() {
        host_family_storage_registry_and_both_identity_maps_outlive_the_vm();
        host_family_storage_neutral_registry_adopts_nested_identity_backing();
        host_family_storage_original_termination_refuses_a_cached_identity();
        host_family_storage_original_termination_precedes_wrapper_allocation();
        host_family_storage_foreign_activation_preserves_original_registry_ownership();
        host_family_storage_terminal_factory_result_cannot_publish_or_return_null();
        host_family_storage_validator_and_path_boxes_keep_their_allocation_credit();
        host_family_storage_terminal_loan_prevents_family_allocation_and_publication();
        host_family_storage_public_cached_wrapper_survives_gc_with_stable_identity();
        host_family_storage_equal_keys_in_different_families_keep_separate_identity();
        host_family_storage_terminal_method_result_cannot_report_success();
        host_family_storage_terminal_pair_result_cannot_report_success();
        host_family_storage_both_map_candidates_are_paid_before_wrapper_publication();
        host_family_storage_prepared_registry_replacement_outlives_the_vm();
        host_family_storage_neutral_callback_boxes_adopt_the_document();
        host_family_storage_replacing_a_validator_returns_the_previous_box_credit();
        host_family_storage_original_failure_preserves_a_healthy_callers_root_prefix();
        host_family_storage_terminal_ancestry_callback_stops_before_event_progress();
        host_family_storage_growing_both_maps_keeps_one_owner_per_allocation();
        host_family_storage_manual_timer_and_promise_preserve_wrapper_identity_through_gc();
        host_family_storage_healthy_host_callback_errors_keep_their_public_messages();
    }
}
