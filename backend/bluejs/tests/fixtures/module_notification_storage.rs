// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite asynchronous module notifications own admitted work storage.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn code(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn record(vm: &Vm) -> LinkedModule {
    LinkedModule {
        cells: HashMap::new(),
        cells_charge: vm.charge_module_cells(0),
        namespace: None,
        deferred_namespace: None,
        namespace_epochs: [0; 2],
        evaluated: false,
        evaluating: true,
        suspended: true,
        completion: None,
        error: None,
        host_error_pending: false,
    }
}
fn fixture() -> (Vm, ModuleRegistry, ModuleRecords) {
    let mut vm = page();
    vm.register_async_module_dependencies("parent", vec!["child".into()])
        .unwrap();
    let modules = ModuleRegistry::new(
        HashMap::from([("parent".into(), code("")), ("child".into(), code(""))]),
        None,
    )
    .unwrap();
    let mut linked = ModuleRecords::default();
    for name in ["parent", "child"] {
        linked
            .insert_new(name, record(&vm), vm.host_scheduling.as_ref())
            .unwrap();
    }
    (vm, modules, linked)
}
fn rejection_fixture() -> (Vm, ModuleEvaluationError, ModuleRecords) {
    let mut vm = page();
    vm.register_async_module_dependencies("middle", vec!["leaf".into()])
        .unwrap();
    vm.register_async_module_dependencies("top", vec!["middle".into()])
        .unwrap();
    let error = vm
        .retain_module_error(&Value::Null, ThrowOrigin::default())
        .unwrap();
    let mut linked = ModuleRecords::default();
    for name in ["middle", "leaf", "top"] {
        linked
            .insert_new(name, record(&vm), vm.host_scheduling.as_ref())
            .unwrap();
    }
    (vm, error, linked)
}

#[cfg(test)]
#[test]
fn module_notification_storage_completion_refusal_precedes_queue_allocation() {
    let (mut vm, modules, mut linked) = fixture();
    let child = "child".to_owned();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| vm.settle_module_parents(&child, &modules, &mut linked));
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn module_notification_storage_completion_refusal_keeps_reverse_edges() {
    let (mut vm, modules, mut linked) = fixture();
    let _peer = leave_room(&mut vm, 0);
    let result = vm.settle_module_parents("child", &modules, &mut linked);
    assert_eq!(
        vm.module_async_parents.get("child").map(Vec::as_slice),
        Some(["parent".to_owned()].as_slice())
    );
    assert!(linked["parent"].suspended && linked["parent"].evaluating);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
}

#[cfg(test)]
#[test]
fn module_notification_storage_rejection_refusal_precedes_queue_allocation() {
    let (mut vm, error, mut linked) = rejection_fixture();
    let module = "middle".to_owned();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| vm.reject_module_and_parents(&module, error, &mut linked));
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn module_notification_storage_rejection_refusal_keeps_dependency_edges() {
    let (mut vm, error, mut linked) = rejection_fixture();
    let _peer = leave_room(&mut vm, 0);
    let result = vm.reject_module_and_parents("middle", error, &mut linked);
    assert!(vm
        .module_pending_dependencies
        .get("middle")
        .is_some_and(|deps| deps.contains("leaf")));
    assert_eq!(
        vm.module_async_parents.get("leaf").map(Vec::as_slice),
        Some(["middle".to_owned()].as_slice())
    );
    assert!(linked["top"].suspended && linked["top"].evaluating && linked["top"].error.is_none());
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
}

use crate::vm::module_notification_storage::NotificationQueue;
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn many_parents() -> (Vm, ModuleRegistry, ModuleRecords) {
    let mut vm = page();
    let mut data = HashMap::from([("child".into(), code(""))]);
    let mut linked = ModuleRecords::default();
    for name in ["p0", "p1", "p2", "p3", "p4"] {
        vm.register_async_module_dependencies(name, vec!["child".into()])
            .unwrap();
        data.insert(name.into(), code(""));
        linked
            .insert_new(name, record(&vm), vm.host_scheduling.as_ref())
            .unwrap();
    }
    (vm, ModuleRegistry::new(data, None).unwrap(), linked)
}

#[cfg_attr(test, test)]
fn module_notification_storage_queue_retains_backing_and_name_capacity() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let queue = NotificationQueue::singleton_copy("child", vm.host_scheduling.as_ref()).unwrap();
    assert!(queue.bytes() > "child".len());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        queue.bytes()
    );
    drop(queue);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_notification_storage_queue_outlives_the_vm() {
    let vm = page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let queue = NotificationQueue::singleton_copy("child", vm.host_scheduling.as_ref()).unwrap();
    let bytes = queue.bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(queue);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_notification_storage_singleton_exact_and_one_short_room() {
    let bytes = NotificationQueue::singleton_copy("child", None)
        .unwrap()
        .bytes();
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let result = NotificationQueue::singleton_copy("child", vm.host_scheduling.as_ref());
        if accepts {
            let queue = result.unwrap();
            assert_eq!(queue.bytes(), bytes);
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(queue);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_notification_storage_wrapped_queue_keeps_fifo_order() {
    let vm = page();
    let mut queue = NotificationQueue::default();
    queue.reserve(4, vm.host_scheduling.as_ref()).unwrap();
    for name in ["a", "b", "c", "d"] {
        queue
            .push_prepared(
                ModuleName::copy(name, vm.host_scheduling.as_ref()).unwrap(),
                vm.host_scheduling.as_ref(),
            )
            .unwrap();
    }
    assert_eq!(queue.pop_front().unwrap().as_str(), "a");
    assert_eq!(queue.pop_front().unwrap().as_str(), "b");
    for name in ["e", "f"] {
        queue
            .push_prepared(
                ModuleName::copy(name, vm.host_scheduling.as_ref()).unwrap(),
                vm.host_scheduling.as_ref(),
            )
            .unwrap();
    }
    for name in ["c", "d", "e", "f"] {
        assert_eq!(queue.pop_front().unwrap().as_str(), name);
    }
    assert!(queue.pop_front().is_none());
}

#[cfg_attr(test, test)]
fn module_notification_storage_popped_name_keeps_its_independent_owner() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let mut queue =
        NotificationQueue::singleton_copy("child", vm.host_scheduling.as_ref()).unwrap();
    let name = queue.pop_front().unwrap();
    drop(queue);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        name.bytes()
    );
    drop(name);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_notification_storage_growth_admits_the_complete_coexisting_buffer() {
    for accepts in [false, true] {
        let mut vm = page();
        let mut queue = NotificationQueue::default();
        queue.reserve(4, vm.host_scheduling.as_ref()).unwrap();
        for name in ["a", "b", "c", "d"] {
            queue
                .push_prepared(
                    ModuleName::copy(name, vm.host_scheduling.as_ref()).unwrap(),
                    vm.host_scheduling.as_ref(),
                )
                .unwrap();
        }
        let candidate = 8 * std::mem::size_of::<ModuleName>();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, candidate - usize::from(!accepts));
        let result = queue.reserve(1, vm.host_scheduling.as_ref());
        if accepts {
            result.unwrap();
            assert_eq!(queue.bytes(), 8 * std::mem::size_of::<ModuleName>() + 4);
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(queue.bytes(), 4 * std::mem::size_of::<ModuleName>() + 4);
        }
        drop(peer);
        for name in ["a", "b", "c", "d"] {
            assert_eq!(queue.pop_front().unwrap().as_str(), name);
        }
        drop(queue);
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            before - (4 * std::mem::size_of::<ModuleName>() + 4)
        );
    }
}

#[cfg_attr(test, test)]
fn module_notification_storage_prepared_publication_succeeds_at_full_quota() {
    let mut vm = page();
    let mut queue = NotificationQueue::default();
    queue.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    let name = ModuleName::copy("child", vm.host_scheduling.as_ref()).unwrap();
    let pointer = name.as_str().as_ptr();
    let _peer = leave_room(&mut vm, 0);
    queue
        .push_prepared(name, vm.host_scheduling.as_ref())
        .unwrap();
    let name = queue.pop_front().unwrap();
    assert_eq!(name.as_str().as_ptr(), pointer);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn module_notification_storage_foreign_and_unbounded_queue_handoff_is_rejected() {
    let owner = page();
    let other = page();
    let mut queue =
        NotificationQueue::singleton_copy("child", owner.host_scheduling.as_ref()).unwrap();
    let bytes = queue.bytes();
    assert_eq!(
        queue.reserve(0, other.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(queue.bytes(), bytes);
    assert_eq!(queue.reserve(0, None), Err(RuntimeError::SchedulingLimit));
    assert_eq!(queue.bytes(), bytes);
}

#[cfg_attr(test, test)]
fn module_notification_storage_unbounded_queue_attaches_existing_storage() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let mut queue = NotificationQueue::singleton_copy("child", None).unwrap();
    let bytes = queue.bytes();
    queue.reserve(0, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes - before, bytes);
    drop(queue);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_notification_storage_parent_transfer_needs_no_duplicate_headroom() {
    let mut vm = page();
    vm.register_async_module_dependencies("parent", vec!["child".into()])
        .unwrap();
    let key_bytes = vm
        .module_async_parents
        .get_key_value("child")
        .unwrap()
        .0
        .capacity();
    let _peer = leave_room(&mut vm, 0);
    let mut parents = vm.take_notification_parents("child").unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - key_bytes
    );
    assert_eq!(parents.next().unwrap().as_str(), "parent");
}

#[cfg_attr(test, test)]
fn module_notification_storage_detached_parent_iterator_survives_vm_drop() {
    let mut vm = page();
    vm.register_async_module_dependencies("parent", vec!["child".into()])
        .unwrap();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let bytes = module_dependencies::string_vector_bytes(&vm.module_async_parents["child"]);
    let mut parents = vm.take_notification_parents("child").unwrap();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    let name = parents.next().unwrap();
    drop(parents);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        name.bytes()
    );
    drop(name);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_notification_storage_parent_transfer_preserves_padded_buffers() {
    let mut vm = page();
    vm.register_async_module_dependencies("parent", vec!["child".into()])
        .unwrap();
    let values = vm.module_async_parents.get_mut("child").unwrap();
    values.reserve(8);
    values[0].reserve(64);
    let pointer = values[0].as_ptr();
    let capacity = values[0].capacity();
    vm.host_scheduling_usage().unwrap();
    let mut parents = vm.take_notification_parents("child").unwrap();
    let name = parents.next().unwrap();
    assert_eq!(name.as_str().as_ptr(), pointer);
    assert_eq!(name.bytes(), capacity);
}

#[cfg_attr(test, test)]
fn module_notification_storage_completion_batch_refusal_precedes_row_detachment() {
    let (mut vm, modules, mut linked) = many_parents();
    let room = NotificationQueue::singleton_copy("child", None)
        .unwrap()
        .bytes();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        vm.settle_module_parents("child", &modules, &mut linked),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_async_parents["child"].len(), 5);
    assert!(linked
        .values()
        .all(|r| r.evaluating && r.suspended && !r.evaluated));
}

#[cfg_attr(test, test)]
fn module_notification_storage_rejection_batch_refusal_precedes_graph_mutation() {
    let (mut vm, _, mut linked) = many_parents();
    let error = vm
        .retain_module_error(&Value::Null, ThrowOrigin::default())
        .unwrap();
    let room = NotificationQueue::singleton_copy("child", None)
        .unwrap()
        .bytes();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        vm.reject_module_and_parents("child", error, &mut linked),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_async_parents["child"].len(), 5);
    assert_eq!(vm.module_pending_dependencies.len(), 5);
    assert!(linked.values().all(|r| r.error.is_none() && r.suspended));
}

#[cfg_attr(test, test)]
fn module_notification_storage_public_completion_keeps_breadth_first_order() {
    let mut vm = page();
    execute(&mut vm, "var gate=Promise.withResolvers();var order=[];");
    let modules = HashMap::from([
        (
            "root".into(),
            code("import 'p0';import 'p1';order.push('root');"),
        ),
        ("p0".into(), code("import 'leaf';order.push('p0');")),
        ("p1".into(), code("import 'leaf';order.push('p1');")),
        ("leaf".into(), code("await gate.promise;")),
    ]);
    vm.execute_module_graph("root", &modules).unwrap();
    execute(&mut vm, "gate.resolve();");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(
        execute(&mut vm, "order.join()"),
        Value::String("p0,p1,root".into())
    );
    assert!(vm.module_pending_dependencies.is_empty() && vm.module_async_parents.is_empty());
}

#[cfg_attr(test, test)]
fn module_notification_storage_failed_ready_parent_does_not_stop_its_sibling() {
    let mut vm = page();
    execute(&mut vm, "var gate=Promise.withResolvers();var order=[];");
    let modules = HashMap::from([
        (
            "root".into(),
            code("import 'p0';import 'p1';order.push('root');"),
        ),
        ("p0".into(), code("import 'leaf';throw 7;")),
        ("p1".into(), code("import 'leaf';order.push('p1');")),
        ("leaf".into(), code("await gate.promise;")),
    ]);
    vm.execute_module_graph("root", &modules).unwrap();
    execute(&mut vm, "gate.resolve();");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(execute(&mut vm, "order.join()"), Value::String("p1".into()));
    assert!(vm.linked_record("root").unwrap().error.is_some());
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_notification_storage_public_rejection_shares_identity_through_gc() {
    let mut vm = page();
    execute(
        &mut vm,
        "var gate=Promise.withResolvers();var reason={marker:7};",
    );
    let modules = HashMap::from([
        ("root".into(), code("import 'parent';")),
        ("parent".into(), code("import 'leaf';")),
        ("leaf".into(), code("await gate.promise;")),
    ]);
    vm.execute_module_graph("root", &modules).unwrap();
    let reason = execute(&mut vm, "reason");
    execute(&mut vm, "gate.reject(reason);");
    vm.run_promise_jobs_bounded(16).unwrap();
    vm.collect_realm_garbage().unwrap();
    for name in ["root", "parent", "leaf"] {
        let record = vm.linked_record(name).unwrap();
        assert!(record.evaluated && !record.suspended && !record.evaluating);
        assert_eq!(record.error.as_ref().unwrap().value, reason);
    }
}

#[cfg_attr(test, test)]
fn module_notification_storage_error_root_refusal_preserves_parent_flags() {
    let (mut vm, _, mut linked) = rejection_fixture();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.root(object).unwrap();
    let error = vm
        .retain_module_error(&Value::Object(object), ThrowOrigin::default())
        .unwrap();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.reject_module_and_parents("middle", error, &mut linked),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(
        linked["top"].evaluating
            && linked["top"].suspended
            && !linked["top"].evaluated
            && linked["top"].error.is_none()
    );
}

#[cfg_attr(test, test)]
fn module_notification_storage_missing_parent_row_has_no_detached_payload() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut parents = vm.take_notification_parents("missing").unwrap();
    assert!(parents.next().is_none());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_notification_storage_host_neutral_completion_and_rejection_remain_valid() {
    let mut vm = Vm::default();
    vm.register_async_module_dependencies("parent", vec!["child".into()])
        .unwrap();
    let modules = ModuleRegistry::new(HashMap::from([("parent".into(), code(""))]), None).unwrap();
    let mut linked = ModuleRecords::default();
    linked.insert_new("parent", record(&vm), None).unwrap();
    vm.settle_module_parents("child", &modules, &mut linked)
        .unwrap();
    assert!(linked["parent"].evaluated);
    assert!(vm.module_async_parents.is_empty() && vm.module_pending_dependencies.is_empty());
    vm.register_async_module_dependencies("parent", vec!["child".into()])
        .unwrap();
    let error = vm
        .retain_module_error(&Value::Null, ThrowOrigin::default())
        .unwrap();
    vm.reject_module_and_parents("child", error, &mut linked)
        .unwrap();
    assert!(linked["parent"].error.is_some());
    assert!(vm.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn module_notification_storage_terminal_document_keeps_the_reverse_row() {
    let (mut vm, _, _) = fixture();
    vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        vm.take_notification_parents("child"),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(vm.module_async_parents.contains_key("child"));
}

#[cfg_attr(test, test)]
fn module_notification_storage_owned_seed_moves_without_copying() {
    let vm = page();
    let name = ModuleName::copy("seed", vm.host_scheduling.as_ref()).unwrap();
    let pointer = name.as_str().as_ptr();
    let mut queue = NotificationQueue::singleton_owned(name, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(queue.pop_front().unwrap().as_str().as_ptr(), pointer);
}

impl crate::Vm {
    /// Runs finite module completion/rejection notification storage contracts.
    #[doc(hidden)]
    pub fn verify_module_notification_storage_contracts() {
        module_notification_storage_completion_refusal_keeps_reverse_edges();
        module_notification_storage_rejection_refusal_keeps_dependency_edges();
        module_notification_storage_queue_retains_backing_and_name_capacity();
        module_notification_storage_queue_outlives_the_vm();
        module_notification_storage_singleton_exact_and_one_short_room();
        module_notification_storage_wrapped_queue_keeps_fifo_order();
        module_notification_storage_popped_name_keeps_its_independent_owner();
        module_notification_storage_growth_admits_the_complete_coexisting_buffer();
        module_notification_storage_prepared_publication_succeeds_at_full_quota();
        module_notification_storage_foreign_and_unbounded_queue_handoff_is_rejected();
        module_notification_storage_unbounded_queue_attaches_existing_storage();
        module_notification_storage_parent_transfer_needs_no_duplicate_headroom();
        module_notification_storage_detached_parent_iterator_survives_vm_drop();
        module_notification_storage_parent_transfer_preserves_padded_buffers();
        module_notification_storage_completion_batch_refusal_precedes_row_detachment();
        module_notification_storage_rejection_batch_refusal_precedes_graph_mutation();
        module_notification_storage_public_completion_keeps_breadth_first_order();
        module_notification_storage_failed_ready_parent_does_not_stop_its_sibling();
        module_notification_storage_public_rejection_shares_identity_through_gc();
        module_notification_storage_error_root_refusal_preserves_parent_flags();
        module_notification_storage_missing_parent_row_has_no_detached_payload();
        module_notification_storage_host_neutral_completion_and_rejection_remain_valid();
        module_notification_storage_terminal_document_keeps_the_reverse_row();
        module_notification_storage_owned_seed_moves_without_copying();
    }
}
