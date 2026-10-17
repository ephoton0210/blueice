// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny real module records and finite dispatches exercise reporting lifetime.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn fixture() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let modules = HashMap::from([
        ("first".into(), module("throw {marker:'first'};")),
        ("second".into(), module("throw {marker:'second'};")),
    ]);
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
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn discard_graph(vm: &mut Vm) {
    let mut graph = vm.module_graph.take().unwrap();
    vm.release_module_roots_from(&mut graph.roots, 0);
    drop(graph);
    // The embedding also releases the last public thrown result when it
    // discards this execution context; that lease is separate from the graph.
    if let Some(root) = vm.result_root.take() {
        vm.release_root(root);
    }
}

#[cfg_attr(test, test)]
fn module_error_reporting_admission_refusal_preserves_flags_and_skips_dispatch() {
    let mut vm = fixture();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
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
    assert_eq!(vm.heap.stats(), before);
}

#[cfg_attr(test, test)]
fn module_error_reporting_keeps_the_whole_batch_alive_after_graph_release() {
    let mut vm = fixture();
    let mut ids = Vec::new();
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|vm, error| {
        calls += 1;
        let id = error.value.object_id().unwrap();
        ids.push(id);
        if calls == 1 {
            discard_graph(vm);
            vm.with_roots(|heap| heap.try_collect_major())?;
        }
        assert!(
            vm.heap.contains(id),
            "the Rust-held reporting batch roots every error"
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(ids.iter().all(|id| !vm.heap.contains(*id)));
}

#[cfg_attr(test, test)]
fn module_error_reporting_partial_root_refusal_restores_the_pending_batch() {
    let mut vm = fixture();
    let roots = vm.module_graph.as_ref().unwrap().roots.snapshot();
    let mut calls = 0;
    vm.heap.allow_root_registrations(1);
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            Ok(())
        }),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(calls, 0);
    assert_eq!(pending(&vm), 2);
    assert_eq!(vm.module_graph.as_ref().unwrap().roots.snapshot(), roots);
    let ids = vm
        .module_graph
        .as_ref()
        .unwrap()
        .linked
        .values()
        .map(|record| record.error.as_ref().unwrap().value.object_id().unwrap())
        .collect::<Vec<_>>();
    discard_graph(&mut vm);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(ids.iter().all(|id| !vm.heap.contains(*id)));
}

fn prepared_bytes(vm: &mut Vm) -> usize {
    let mut batch = vm.prepare_pending_module_reports().unwrap().unwrap();
    let bytes = batch.retained_bytes() + batch.roots.retained_bytes();
    vm.release_module_roots_from(&mut batch.roots, 0);
    drop(batch);
    vm.reconcile_scheduling_ownership();
    bytes
}
fn install_reporter(vm: &mut Vm) {
    vm.host_error_reporting = None;
    for record in vm.module_graph.as_mut().unwrap().linked.values_mut() {
        record.host_error_pending = false;
    }
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_window_aliases().unwrap();
    vm.install_host_error_reporting().unwrap();
    let code = crate::compile(
        &crate::parse("var reported=0;onerror=function(){reported++;return true;};").unwrap(),
    )
    .unwrap();
    vm.execute_script(&code).unwrap();
    for record in vm.module_graph.as_mut().unwrap().linked.values_mut() {
        record.host_error_pending = true;
    }
}

#[cfg_attr(test, test)]
fn module_error_reporting_public_checkpoint_refusal_preserves_pending_flags() {
    let mut vm = fixture();
    install_reporter(&mut vm);
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_host_microtask_checkpoint(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(pending(&vm), 2);
    assert_eq!(
        vm.lookup_global_name("reported").unwrap(),
        Some(Value::Number(0.0))
    );
}

#[cfg_attr(test, test)]
fn module_error_reporting_exact_snapshot_allowance_is_admitted_jointly() {
    for accepts in [false, true] {
        let mut vm = fixture();
        let bytes = prepared_bytes(&mut vm);
        let room = bytes - usize::from(!accepts);
        let _peer = leave_room(&mut vm, room);
        let before = vm.heap.stats().root_registrations;
        let mut calls = 0;
        let result = vm.dispatch_pending_host_module_errors(|vm, _| {
            calls += 1;
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            Ok(())
        });
        if accepts {
            assert_eq!(result, Ok(()));
            assert_eq!(calls, 2);
            assert_eq!(pending(&vm), 0);
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(calls, 0);
            assert_eq!(pending(&vm), 2);
            assert_eq!(vm.heap.stats().root_registrations, before);
        }
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - room
        );
    }
}

#[cfg_attr(test, test)]
fn module_error_reporting_snapshot_storage_stays_charged_through_every_callback() {
    let mut vm = fixture();
    let bytes = prepared_bytes(&mut vm);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|vm, _| {
        calls += 1;
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
        assert_eq!(pending(vm), 0);
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_error_reporting_reentrant_delivery_does_not_repeat_the_outer_batch() {
    let mut vm = fixture();
    let bytes = prepared_bytes(&mut vm);
    let _peer = leave_room(&mut vm, bytes);
    let mut calls = 0;
    let mut nested = 0;
    vm.dispatch_pending_host_module_errors(|vm, _| {
        calls += 1;
        vm.dispatch_pending_host_module_errors(|_, _| {
            nested += 1;
            Ok(())
        })?;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(nested, 0);
    assert_eq!(pending(&vm), 0);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn module_error_reporting_interrupted_delivery_preserves_only_unattempted_records() {
    let mut vm = fixture();
    let mut attempted = None;
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|_, error| {
            attempted = Some(error.clone());
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        }),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(pending(&vm), 1);
    let attempted = attempted.unwrap();
    for record in vm.module_graph.as_ref().unwrap().linked.values() {
        assert_eq!(
            record.host_error_pending,
            !Rc::ptr_eq(record.error.as_ref().unwrap(), &attempted)
        );
    }
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|_, error| {
        assert!(!Rc::ptr_eq(error, &attempted));
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_error_reporting_replacement_error_is_not_rearmed_as_the_old_error() {
    let mut vm = fixture();
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|vm, error| {
            let replacement =
                vm.retain_module_error(&Value::Number(7.0), ThrowOrigin::default())?;
            let other = vm
                .module_graph
                .as_mut()
                .unwrap()
                .linked
                .values_mut()
                .find(|record| !Rc::ptr_eq(record.error.as_ref().unwrap(), error))
                .unwrap();
            other.error = Some(replacement);
            other.host_error_pending = false;
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        }),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_error_reporting_removed_record_is_not_resurrected_on_failure() {
    let mut vm = fixture();
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|vm, error| {
            let name = vm
                .module_graph
                .as_ref()
                .unwrap()
                .linked
                .iter()
                .find(|(_, record)| !Rc::ptr_eq(record.error.as_ref().unwrap(), error))
                .unwrap()
                .0
                .clone();
            vm.module_graph.as_mut().unwrap().linked.remove(&name);
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        }),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(vm.module_graph.as_ref().unwrap().linked.len(), 1);
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_error_reporting_new_pending_work_survives_the_older_snapshot() {
    let mut vm = fixture();
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|vm, error| {
        calls += 1;
        if calls == 1 {
            vm.module_graph
                .as_mut()
                .unwrap()
                .linked
                .values_mut()
                .find(|record| Rc::ptr_eq(record.error.as_ref().unwrap(), error))
                .unwrap()
                .host_error_pending = true;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(pending(&vm), 1);
    calls = 0;
    vm.dispatch_pending_host_module_errors(|_, _| {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_error_reporting_terminal_callback_releases_all_snapshot_owners() {
    let mut vm = fixture();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let mut calls = 0;
    assert_eq!(
        vm.dispatch_pending_host_module_errors(|vm, _| {
            calls += 1;
            Err(vm.host_scheduling.as_ref().unwrap().fail())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls, 1);
    assert_eq!(pending(&vm), 1);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_error_reporting_empty_graph_needs_no_native_capacity() {
    let mut vm = fixture();
    discard_graph(&mut vm);
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    vm.dispatch_pending_host_module_errors(|_, _| panic!("empty graph"))
        .unwrap();
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn module_error_reporting_disabled_owner_keeps_pending_work_at_full_quota() {
    let mut vm = fixture();
    vm.host_error_reporting = None;
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    vm.dispatch_pending_host_module_errors(|_, _| panic!("disabled reporter"))
        .unwrap();
    assert_eq!(pending(&vm), 2);
    assert_eq!(vm.heap.stats(), before);
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn module_error_reporting_successful_records_clear_flags_without_snapshot_capacity() {
    let mut vm = fixture();
    for record in vm.module_graph.as_mut().unwrap().linked.values_mut() {
        record.error = None;
    }
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    vm.dispatch_pending_host_module_errors(|_, _| panic!("successful module"))
        .unwrap();
    assert_eq!(pending(&vm), 0);
    assert_eq!(vm.heap.stats(), before);
}

#[cfg_attr(test, test)]
fn module_error_reporting_unevaluated_records_keep_their_pending_flag() {
    let mut vm = fixture();
    vm.module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut("first")
        .unwrap()
        .evaluated = false;
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|_, _| {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(pending(&vm), 1);
    vm.module_graph
        .as_mut()
        .unwrap()
        .linked
        .get_mut("first")
        .unwrap()
        .evaluated = true;
    calls = 0;
    vm.dispatch_pending_host_module_errors(|_, _| {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_error_reporting_shared_primitive_errors_need_no_object_registration() {
    let mut vm = fixture();
    let error = vm
        .retain_module_error(&Value::String("tiny".into()), ThrowOrigin::default())
        .unwrap();
    for record in vm.module_graph.as_mut().unwrap().linked.values_mut() {
        record.error = Some(error.clone());
    }
    vm.heap.allow_root_registrations(0);
    let before = vm.heap.stats().root_registrations;
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|_, observed| {
        calls += 1;
        assert!(Rc::ptr_eq(observed, &error));
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(vm.heap.stats().root_registrations, before);
}

#[cfg_attr(test, test)]
fn module_error_reporting_successful_batch_is_not_reported_again() {
    let mut vm = fixture();
    let mut calls = 0;
    vm.dispatch_pending_host_module_errors(|_, _| {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    let _peer = leave_room(&mut vm, 0);
    vm.dispatch_pending_host_module_errors(|_, _| panic!("already delivered"))
        .unwrap();
    assert_eq!(pending(&vm), 0);
}

#[cfg_attr(test, test)]
fn module_error_reporting_resource_failure_is_local_to_its_document() {
    let mut failing = fixture();
    let mut healthy = fixture();
    let _peer = leave_room(&mut failing, 0);
    assert_eq!(
        failing.dispatch_pending_host_module_errors(|_, _| Ok(())),
        Err(RuntimeError::SchedulingLimit)
    );
    let mut calls = 0;
    healthy
        .dispatch_pending_host_module_errors(|_, _| {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(pending(&failing), 2);
    assert_eq!(pending(&healthy), 0);
    assert!(healthy.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn module_error_reporting_public_error_events_deliver_each_record_once() {
    let mut vm = fixture();
    install_reporter(&mut vm);
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        vm.lookup_global_name("reported").unwrap(),
        Some(Value::Number(2.0))
    );
    assert_eq!(pending(&vm), 0);
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        vm.lookup_global_name("reported").unwrap(),
        Some(Value::Number(2.0))
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[cfg_attr(test, test)]
fn module_error_reporting_public_registration_refusal_does_not_consume_flags() {
    let mut vm = fixture();
    install_reporter(&mut vm);
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.run_host_microtask_checkpoint(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(pending(&vm), 2);
    assert_eq!(
        vm.lookup_global_name("reported").unwrap(),
        Some(Value::Number(0.0))
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_module_error_reporting_contracts() {
        module_error_reporting_admission_refusal_preserves_flags_and_skips_dispatch();
        module_error_reporting_keeps_the_whole_batch_alive_after_graph_release();
        module_error_reporting_partial_root_refusal_restores_the_pending_batch();
        module_error_reporting_public_checkpoint_refusal_preserves_pending_flags();
        module_error_reporting_exact_snapshot_allowance_is_admitted_jointly();
        module_error_reporting_snapshot_storage_stays_charged_through_every_callback();
        module_error_reporting_reentrant_delivery_does_not_repeat_the_outer_batch();
        module_error_reporting_interrupted_delivery_preserves_only_unattempted_records();
        module_error_reporting_replacement_error_is_not_rearmed_as_the_old_error();
        module_error_reporting_removed_record_is_not_resurrected_on_failure();
        module_error_reporting_new_pending_work_survives_the_older_snapshot();
        module_error_reporting_terminal_callback_releases_all_snapshot_owners();
        module_error_reporting_empty_graph_needs_no_native_capacity();
        module_error_reporting_disabled_owner_keeps_pending_work_at_full_quota();
        module_error_reporting_successful_records_clear_flags_without_snapshot_capacity();
        module_error_reporting_unevaluated_records_keep_their_pending_flag();
        module_error_reporting_shared_primitive_errors_need_no_object_registration();
        module_error_reporting_successful_batch_is_not_reported_again();
        module_error_reporting_resource_failure_is_local_to_its_document();
        module_error_reporting_public_error_events_deliver_each_record_once();
        module_error_reporting_public_registration_refusal_does_not_consume_flags();
    }
}
