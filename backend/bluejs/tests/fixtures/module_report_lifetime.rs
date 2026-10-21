// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Two finite module failures retain reporting storage and original ownership.

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

fn fixture(objects: bool) -> Vm {
    fixture_with_policy(objects, true)
}

fn fixture_with_policy(objects: bool, scoped: bool) -> Vm {
    let mut vm = if scoped { page() } else { Vm::default() };
    let source = if objects {
        "throw {marker:7};"
    } else {
        "throw 'tiny';"
    };
    let code = crate::compile_module(&crate::parse_module(source).unwrap()).unwrap();
    let modules = HashMap::from([("first".into(), code.clone()), ("second".into(), code)]);
    for entry in ["first", "second"] {
        assert!(matches!(
            vm.execute_module_graph(entry, &modules),
            Err(RuntimeError::Thrown(_))
        ));
    }
    for record in vm.module_graph.as_mut().unwrap().linked.values_mut() {
        record.host_error_pending = true;
    }
    vm.host_error_reporting = Some(false);
    vm
}

fn pending(vm: &Vm) -> usize {
    vm.module_graph.as_ref().map_or(0, |graph| {
        graph
            .linked
            .values()
            .filter(|record| record.host_error_pending)
            .count()
    })
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

fn detach_graph(vm: &mut Vm) {
    let mut graph = vm.module_graph.take().unwrap();
    vm.release_module_roots_from(&mut graph.roots, 0);
    drop(graph);
    if let Some(root) = vm.result_root.take() {
        vm.release_root(root);
    }
}

fn detached_batch(neutral: bool) {
    let mut vm = fixture(true);
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let batch = vm.prepare_pending_module_reports().unwrap().unwrap();
    let bytes = batch.retained_bytes()
        + batch.roots.retained_bytes()
        + batch
            .entries
            .iter()
            .map(|entry| entry.error.retained_bytes())
            .sum::<usize>();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(batch);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_scoped_batch_outlives_the_vm_with_its_actual_storage() {
    detached_batch(false);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_neutral_batch_outlives_the_vm_with_its_actual_storage() {
    detached_batch(true);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_neutral_preparation_accounts_for_backing_and_roots() {
    let mut vm = fixture(true);
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut batch = vm.prepare_pending_module_reports().unwrap().unwrap();
    let bytes = batch.retained_bytes() + batch.roots.retained_bytes();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    vm.release_module_roots_from(&mut batch.roots, 0);
    drop(batch);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(pending(&vm), 2);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_failed_original_owner_preserves_pending_reports() {
    let mut vm = fixture(false);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    drop(vm.host_scheduling.take());
    let _ = scope.fail();
    let mut calls = 0;
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls, 0);
    assert_eq!(pending(&vm), 2);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_another_caller_does_not_revive_failed_input() {
    let mut vm = fixture(false);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    drop(vm.host_scheduling.take());
    let caller = page();
    let _ = scope.fail();
    let mut calls = 0;
    let result = loan(&mut vm, &caller, |vm| {
        vm.dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            Ok(())
        })
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(calls, 0);
    assert_eq!(pending(&vm), 2);
    assert!(caller.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn module_report_lifetime_borrowed_callback_uses_the_original_record_account() {
    let mut vm = fixture(false);
    let original = vm.host_scheduling.as_ref().unwrap().child();
    drop(vm.host_scheduling.take());
    let caller = page();
    let mut calls = 0;
    loan(&mut vm, &caller, |vm| {
        vm.dispatch_pending_host_module_errors(|vm, _| {
            calls += 1;
            assert!(vm.argument_list_owner()?.unwrap().belongs_to(&original));
            Ok(())
        })
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(pending(&vm), 0);
    assert!(vm.current_realm_root_owner().unwrap().is_none());
}

#[cfg_attr(test, test)]
fn module_report_lifetime_foreign_scoped_caller_cannot_consume_the_reports() {
    let mut vm = fixture(false);
    let view = observer(&vm);
    let caller = page();
    drop(vm.host_scheduling.take());
    vm.host_scheduling = caller.host_scheduling.as_ref().map(SchedulingScope::child);
    let mut calls = 0;
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls, 0);
    assert_eq!(pending(&vm), 2);
    assert!(view.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn module_report_lifetime_cancellation_after_successful_callback_keeps_the_suffix() {
    for neutral in [false, true] {
        let mut vm = fixture(false);
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        if neutral {
            drop(vm.host_scheduling.take());
        }
        let mut calls = 0;
        assert_eq!(
            vm.dispatch_pending_host_module_errors(|_, _| {
                calls += 1;
                let _ = scope.fail();
                Ok(())
            }),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(calls, 1);
        assert_eq!(pending(&vm), 1);
    }
}

#[cfg_attr(test, test)]
fn module_report_lifetime_cancellation_overrides_a_callback_language_error() {
    for neutral in [false, true] {
        let mut vm = fixture(false);
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        if neutral {
            drop(vm.host_scheduling.take());
        }
        let mut calls = 0;
        assert_eq!(
            vm.dispatch_pending_host_module_errors(|_, _| {
                calls += 1;
                let _ = scope.fail();
                Err(RuntimeError::Thrown(Value::Number(7.0)))
            }),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(calls, 1);
        assert_eq!(pending(&vm), 1);
    }
}

#[cfg_attr(test, test)]
fn module_report_lifetime_disabled_and_empty_reporting_needs_no_admission() {
    for disabled in [false, true] {
        let mut vm = fixture(false);
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        if disabled {
            vm.host_error_reporting = None;
        } else {
            for record in vm.module_graph.as_mut().unwrap().linked.values_mut() {
                record.host_error_pending = false;
            }
        }
        drop(vm.host_scheduling.take());
        let _ = scope.fail();
        vm.dispatch_pending_host_module_errors(|_, _| panic!("no pending dispatch"))
            .unwrap();
        assert_eq!(pending(&vm), if disabled { 2 } else { 0 });
    }
}

#[cfg_attr(test, test)]
fn module_report_lifetime_normal_gc_keeps_only_the_active_batch_objects() {
    let mut vm = fixture(true);
    drop(vm.host_scheduling.take());
    let mut ids = Vec::new();
    vm.dispatch_pending_host_module_errors(|vm, error| {
        let object = error.value.object_id().unwrap();
        ids.push(object);
        if ids.len() == 1 {
            detach_graph(vm);
            vm.collect_realm_garbage()?;
        }
        assert!(vm.heap.contains(object));
        Ok(())
    })
    .unwrap();
    assert_eq!(ids.len(), 2);
    vm.collect_realm_garbage().unwrap();
    assert!(ids.into_iter().all(|id| !vm.heap.contains(id)));
}

#[cfg_attr(test, test)]
fn module_report_lifetime_callback_error_restores_only_unattempted_entries() {
    let mut vm = fixture(false);
    let mut calls = 0;
    let mut nested = 0;
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|vm, _| {
            calls += 1;
            vm.dispatch_pending_host_module_errors(|_, _| {
                nested += 1;
                Ok(())
            })?;
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        }),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(calls, 1);
    assert_eq!(nested, 0);
    assert_eq!(pending(&vm), 1);
    vm.dispatch_pending_host_module_errors(|_, _| {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_legacy_adoption_reaches_a_preexisting_shared_error() {
    let mut vm = fixture_with_policy(false, false);
    let caller = page();
    let view = observer(&caller);
    let held = vm
        .module_graph
        .as_ref()
        .unwrap()
        .linked
        .values()
        .next()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .clone();
    let batch = loan(&mut vm, &caller, |vm| vm.prepare_pending_module_reports())
        .unwrap()
        .unwrap();
    let bytes = batch.retained_bytes()
        + batch.roots.retained_bytes()
        + batch
            .entries
            .iter()
            .map(|entry| entry.error.retained_bytes())
            .sum::<usize>();
    drop(vm);
    drop(caller);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(batch);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        held.retained_bytes()
    );
    drop(held);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_mixed_neutral_entries_keep_their_own_accounts() {
    let mut vm = fixture(false);
    let first = vm.host_scheduling.as_ref().unwrap().child();
    let other = page();
    let second = other.host_scheduling.as_ref().unwrap().child();
    let error = {
        let mut other = other;
        other
            .retain_module_error(&Value::String("other".into()), ThrowOrigin::default())
            .unwrap()
    };
    vm.module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut("second")
        .unwrap()
        .error = Some(error);
    drop(vm.host_scheduling.take());
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|vm, error| {
        calls += 1;
        let expected = if error.value == Value::String("other".into()) {
            &second
        } else {
            &first
        };
        assert!(vm.argument_list_owner()?.unwrap().belongs_to(expected));
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_failed_entry_account_is_not_replaced_by_batch_ownership() {
    let mut vm = fixture(false);
    let view = observer(&vm);
    let mut other = page();
    let error = other
        .retain_module_error(&Value::String("other".into()), ThrowOrigin::default())
        .unwrap();
    vm.module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut("second")
        .unwrap()
        .error = Some(error);
    drop(vm.host_scheduling.take());
    let _ = other.host_scheduling.as_ref().unwrap().fail();
    let mut calls = 0;
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls, 0);
    assert_eq!(pending(&vm), 2);
    assert!(view.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn module_report_lifetime_last_callback_cancellation_does_not_return_success() {
    let mut vm = fixture(false);
    vm.module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut("second")
        .unwrap()
        .host_error_pending = false;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    drop(vm.host_scheduling.take());
    let mut calls = 0;
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            let _ = scope.fail();
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls, 1);
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_report_lifetime_wrong_heap_input_unwinds_prepared_roots_before_publication() {
    let mut vm = fixture(true);
    let names = {
        let mut names = vm.module_graph.as_ref().unwrap().linked.keys();
        [names.next().unwrap().clone(), names.next().unwrap().clone()]
    };
    let fresh = vm.heap.alloc_object(None).unwrap();
    let first = vm
        .retain_module_error(&Value::Object(fresh), ThrowOrigin::default())
        .unwrap();
    let mut other = Vm::default();
    let foreign = other.heap.alloc_object(None).unwrap();
    let second = vm
        .retain_module_error(&Value::Object(foreign), ThrowOrigin::default())
        .unwrap();
    let first_old = vm
        .module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut(&names[0])
        .unwrap()
        .error
        .replace(first);
    let second_old = vm
        .module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut(&names[1])
        .unwrap()
        .error
        .replace(second);
    let prefix = vm.module_graph.as_ref().unwrap().roots.snapshot();
    let registrations = vm.heap.stats().root_registrations;
    let mut calls = 0;
    assert!(matches!(
        vm.dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            Ok(())
        }),
        Err(RuntimeError::Heap(_))
    ));
    assert_eq!(calls, 0);
    assert_eq!(pending(&vm), 2);
    assert!(vm.heap.stats().root_registrations > registrations);
    assert_eq!(vm.module_graph.as_ref().unwrap().roots.snapshot(), prefix);
    vm.module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut(&names[0])
        .unwrap()
        .error = first_old;
    vm.module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut(&names[1])
        .unwrap()
        .error = second_old;
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(fresh));
    assert!(other.heap.contains(foreign));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_module_report_lifetime_contracts() {
        module_report_lifetime_scoped_batch_outlives_the_vm_with_its_actual_storage();
        module_report_lifetime_neutral_batch_outlives_the_vm_with_its_actual_storage();
        module_report_lifetime_neutral_preparation_accounts_for_backing_and_roots();
        module_report_lifetime_failed_original_owner_preserves_pending_reports();
        module_report_lifetime_another_caller_does_not_revive_failed_input();
        module_report_lifetime_borrowed_callback_uses_the_original_record_account();
        module_report_lifetime_foreign_scoped_caller_cannot_consume_the_reports();
        module_report_lifetime_cancellation_after_successful_callback_keeps_the_suffix();
        module_report_lifetime_cancellation_overrides_a_callback_language_error();
        module_report_lifetime_disabled_and_empty_reporting_needs_no_admission();
        module_report_lifetime_normal_gc_keeps_only_the_active_batch_objects();
        module_report_lifetime_callback_error_restores_only_unattempted_entries();
        module_report_lifetime_legacy_adoption_reaches_a_preexisting_shared_error();
        module_report_lifetime_mixed_neutral_entries_keep_their_own_accounts();
        module_report_lifetime_failed_entry_account_is_not_replaced_by_batch_ownership();
        module_report_lifetime_last_callback_cancellation_does_not_return_success();
        module_report_lifetime_wrong_heap_input_unwinds_prepared_roots_before_publication();
    }
}
