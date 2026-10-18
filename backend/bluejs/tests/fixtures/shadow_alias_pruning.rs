// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small alias graphs retain live instances and one necessary work route.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    run(&mut vm, "var realm=new ShadowRealm();0;");
    vm
}
fn clear(vm: &mut Vm, name: &str) {
    if !vm.set_global_binding(name, Value::Null).unwrap() {
        let global = vm.global("globalThis").unwrap().object_id().unwrap();
        vm.heap.set(global, name, Value::Null).unwrap();
    }
}
fn alias(vm: &mut Vm, record: &ShadowRealmRecord) -> ObjectId {
    let prototype = vm.shadow_realm_prototype().unwrap();
    let instance = vm
        .with_roots(|heap| heap.alloc_object(Some(prototype)))
        .unwrap();
    vm.adopt_shadow_realm(instance, record.clone()).unwrap();
    instance
}
fn path(vm: &Vm, instance: ObjectId, heap: u64) -> HostRealm {
    let mut path = TaskVector::new(vm.host_scheduling.as_ref()).unwrap();
    path.push(Step {
        kind: Kind::Shared,
        instance,
        heap,
    })
    .unwrap();
    HostRealm { heap, path }
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_dead_aliases_leave_only_the_live_instance() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let first = alias(&mut vm, &record);
    let second = alias(&mut vm, &record);
    assert_eq!(vm.shadow_realms.len(), 3);
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert!(!vm.heap.contains(first) && !vm.heap.contains(second));
    assert_eq!(record.owners.len(), 1);
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_dead_alias_index_moves_to_the_surviving_instance() {
    let mut vm = page();
    let primary = *vm.shadow_realms.keys().next().unwrap();
    let record = vm.shadow_realms[&primary].clone();
    let discarded = alias(&mut vm, &record);
    assert_eq!(vm.shadow_realm_by_heap.get(&record.heap), Some(&discarded));
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realm_by_heap.get(&record.heap), Some(&primary));
    assert!(!vm.shadow_realms.contains_key(&discarded));
    assert_eq!(run(&mut vm, "realm.evaluate('42');"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_callable_keeps_one_fallback_record_then_releases_it() {
    let mut vm = page();
    run(&mut vm, "var callback=realm.evaluate('()=>42');0;");
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let weak = record.vm.downgrade();
    alias(&mut vm, &record);
    alias(&mut vm, &record);
    drop(record);
    clear(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert_eq!(run(&mut vm, "callback();"), Value::Number(42.0));
    run(&mut vm, "callback=null;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
    assert!(vm.shadow_realms.is_empty() && vm.shadow_realm_by_heap.is_empty());
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_captured_path_resolves_the_original_heap_after_replacement() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let instance = alias(&mut vm, &record);
    let root = vm.heap.root(instance).unwrap();
    let captured = path(&vm, instance, record.heap);
    let other = run(&mut vm, "var other=new ShadowRealm();other;")
        .object_id()
        .unwrap();
    let replacement = vm.shadow_realms[&other].clone();
    vm.adopt_shadow_realm(instance, replacement).unwrap();
    assert_eq!(
        captured
            .read(&vm, |child| Ok(child.object_prototype.heap))
            .unwrap(),
        Some(record.heap)
    );
    assert_eq!(
        captured
            .enter(&mut vm, |child| Ok(run(child, "41+1;")))
            .unwrap(),
        Some(Value::Number(42.0))
    );
    vm.release_root(root);
}

fn counter(vm: &mut Vm) -> Rc<std::cell::Cell<usize>> {
    let count = Rc::new(std::cell::Cell::new(0));
    let output = count.clone();
    vm.install_host_function("record", 0, move |_: &[HostValue]| {
        output.set(output.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    count
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_all_live_instances_survive_then_dead_ones_are_released() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let owners = record.owners.clone();
    let weak = record.vm.downgrade();
    let first = alias(&mut vm, &record);
    let second = alias(&mut vm, &record);
    let first_root = vm.heap.root(first).unwrap();
    let second_root = vm.heap.root(second).unwrap();
    drop(record);
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 3);
    assert_eq!(owners.len(), 3);
    vm.release_root(first_root);
    clear(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert!(vm.shadow_realms.contains_key(&second));
    assert_eq!(owners.len(), 1);
    assert_eq!(
        vm.shadow_realm_evaluate(Value::Object(second), &Value::String("42".into()))
            .unwrap(),
        Value::Number(42.0)
    );
    vm.release_root(second_root);
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(owners.len(), 0);
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_collection_snapshot_keeps_data_without_an_owner_identity() {
    let mut vm = page();
    let primary = *vm.shadow_realms.keys().next().unwrap();
    let plan = vm.prepare_realm_collection().unwrap().unwrap();
    let record = vm.shadow_realms.remove(&primary).unwrap();
    let owners = record.owners.clone();
    let weak = record.vm.downgrade();
    drop(record);
    owners.prune();
    assert_eq!(owners.len(), 0);
    assert!(weak.upgrade().is_some());
    drop(plan);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_timer_work_keeps_one_route_until_delivery() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let weak = record.vm.downgrade();
    let owners = record.owners.clone();
    let count;
    {
        let mut child = record.vm.borrow_mut();
        child.install_host_timer_functions().unwrap();
        count = counter(&mut child);
        run(&mut child, "setTimeout(()=>record(),1);0;");
    }
    alias(&mut vm, &record);
    alias(&mut vm, &record);
    drop(record);
    clear(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert_eq!(owners.len(), 1);
    assert_eq!(vm.next_host_task_wake(0).unwrap(), Some(1));
    assert_eq!(vm.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(count.get(), 1);
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(owners.len(), 0);
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_promise_work_keeps_one_route_until_checkpoint() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let weak = record.vm.downgrade();
    let count;
    {
        let mut child = record.vm.borrow_mut();
        count = counter(&mut child);
        run(&mut child, "Promise.resolve().then(()=>record());0;");
    }
    alias(&mut vm, &record);
    alias(&mut vm, &record);
    drop(record);
    clear(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert_eq!(vm.run_host_microtask_checkpoint().unwrap(), 0);
    assert_eq!(count.get(), 1);
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_kept_targets_keep_one_route_until_clear_kept_objects() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let weak = record.vm.downgrade();
    run(
        &mut record.vm.borrow_mut(),
        "var object={};var weak=new WeakRef(object);weak.deref();object=null;0;",
    );
    alias(&mut vm, &record);
    alias(&mut vm, &record);
    drop(record);
    clear(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert!(weak.upgrade().is_some());
    vm.run_host_microtask_checkpoint().unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_busy_child_defers_pruning_until_full_collection() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    alias(&mut vm, &record);
    alias(&mut vm, &record);
    let busy = record.vm.borrow_mut();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 3);
    drop(busy);
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert_eq!(record.owners.len(), 1);
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_captured_path_survives_pruning_and_does_not_revive_a_dead_vm() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let instance = alias(&mut vm, &record);
    let captured = path(&vm, instance, record.heap);
    let heap = record.heap;
    drop(record);
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.shadow_realms.contains_key(&instance));
    assert_eq!(
        captured
            .read(&vm, |child| Ok(child.object_prototype.heap))
            .unwrap(),
        Some(heap)
    );
    assert_eq!(
        captured
            .enter(&mut vm, |child| Ok(run(child, "42;")))
            .unwrap(),
        Some(Value::Number(42.0))
    );
    clear(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(captured.read(&vm, |_| Ok(())).unwrap(), None);
    assert_eq!(captured.enter(&mut vm, |_| Ok(())).unwrap(), None);
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_captured_path_uses_heap_identity_without_a_cached_index() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let captured = path(&vm, vm.object_prototype, record.heap);
    vm.shadow_realm_by_heap.remove(&record.heap);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) =
        scope.measure_payload_peak(|| captured.read(&vm, |child| Ok(child.object_prototype.heap)));
    assert_eq!(result.unwrap(), Some(record.heap));
    assert_eq!(peak, 0);
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_captured_path_marks_a_busy_surviving_route_for_retry() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let captured = path(&vm, vm.object_prototype, record.heap);
    let busy = record.vm.borrow_mut();
    let mut blocked = false;
    assert_eq!(
        captured
            .read_checkpoint(&vm, &mut blocked, |_| Ok(()))
            .unwrap(),
        None
    );
    assert!(blocked);
    drop(busy);
    assert_eq!(captured.enter(&mut vm, |_| Ok(42)).unwrap(), Some(42));
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_heap_fallback_still_excludes_another_document() {
    let mut vm = page();
    let mut foreign = page();
    let record = foreign.shadow_realms.values().next().unwrap().clone();
    alias(&mut vm, &record);
    let captured = path(&vm, vm.object_prototype, record.heap);
    assert_eq!(captured.read(&vm, |_| Ok(())).unwrap(), None);
    assert_eq!(captured.enter(&mut vm, |_| Ok(())).unwrap(), None);
    assert!(vm.check_scheduling_failure().is_ok());
    assert_eq!(
        run(&mut foreign, "realm.evaluate('42');"),
        Value::Number(42.0)
    );
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_noncanonical_parent_releases_its_dead_access_records() {
    let mut vm = page();
    let parent = vm.shadow_realms.values().next().unwrap().clone();
    run(&mut parent.vm.borrow_mut(), "var leaf=new ShadowRealm();0;");
    let leaf = parent
        .vm
        .borrow()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    let weak = leaf.vm.downgrade();
    let owners = leaf.owners.clone();
    let instance = alias(&mut vm, &leaf);
    let root = vm.heap.root(instance).unwrap();
    clear(&mut parent.vm.borrow_mut(), "leaf");
    drop(leaf);
    vm.collect_realm_garbage().unwrap();
    assert!(parent.vm.borrow().shadow_realms.is_empty());
    assert!(parent.vm.borrow().shadow_realm_by_heap.is_empty());
    assert_eq!(owners.len(), 1);
    vm.release_root(root);
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_terminal_warm_collection_releases_discarded_owners() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let first = alias(&mut vm, &record);
    let second = alias(&mut vm, &record);
    let first_root = vm.heap.root(first).unwrap();
    let second_root = vm.heap.root(second).unwrap();
    vm.collect_realm_garbage().unwrap();
    vm.release_root(first_root);
    vm.release_root(second_root);
    assert!(matches!(
        vm.host_scheduling.as_ref().unwrap().fail(),
        RuntimeError::SchedulingLimit
    ));
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert_eq!(record.owners.len(), 1);
    assert!(vm.check_scheduling_failure().is_err());
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_live_backing_stays_charged_until_the_last_route_is_gone() {
    let mut vm = page();
    let view = observer(&vm);
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let weak = record.vm.downgrade();
    alias(&mut vm, &record);
    alias(&mut vm, &record);
    drop(record);
    let bytes = vm.shadow_realms.retained_bytes();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.len(), 1);
    assert_eq!(vm.shadow_realms.retained_bytes(), bytes);
    clear(&mut vm, "realm");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.shadow_realms.retained_bytes(), 0);
    assert_eq!(vm.shadow_realm_by_heap.retained_bytes(), 0);
    assert!(weak.upgrade().is_none());
    drop(weak);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_alias_pruning_captured_path_changes_parents_after_its_old_edge_is_pruned() {
    let mut vm = page();
    let primary = *vm.shadow_realms.keys().next().unwrap();
    let parent = vm.shadow_realms[&primary].clone();
    run(&mut parent.vm.borrow_mut(), "var leaf=new ShadowRealm();0;");
    let leaf_instance = *parent.vm.borrow().shadow_realms.keys().next().unwrap();
    let leaf = parent.vm.borrow().shadow_realms[&leaf_instance].clone();
    let heap = leaf.heap;
    let base = path(&vm, primary, parent.heap);
    let captured = base
        .child(
            Step {
                kind: Kind::Shared,
                instance: leaf_instance,
                heap,
            },
            vm.host_scheduling.as_ref().unwrap(),
        )
        .unwrap();
    let instance = alias(&mut vm, &leaf);
    let root = vm.heap.root(instance).unwrap();
    clear(&mut parent.vm.borrow_mut(), "leaf");
    drop(leaf);
    vm.collect_realm_garbage().unwrap();
    assert!(parent.vm.borrow().shadow_realms.is_empty());
    let calls = std::cell::Cell::new(0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| {
        captured.read(&vm, |child| {
            calls.set(calls.get() + 1);
            assert!(child.host_scheduling_snapshot().payload_bytes > before);
            Ok(child.object_prototype.heap)
        })
    });
    assert_eq!(result.unwrap(), Some(heap));
    assert!(peak > 0);
    assert_eq!(calls.get(), 1);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        captured
            .enter(&mut vm, |child| {
                calls.set(calls.get() + 1);
                Ok(run(child, "42;"))
            })
            .unwrap(),
        Some(Value::Number(42.0))
    );
    assert_eq!(calls.get(), 2);
    let result: Result<Option<()>, _> = captured.enter(&mut vm, |_| {
        calls.set(calls.get() + 1);
        Err(RuntimeError::TypeError("once".into()))
    });
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert_eq!(calls.get(), 3);
    vm.release_root(root);
    vm.collect_realm_garbage().unwrap();
    assert_eq!(captured.read(&vm, |_| Ok(())).unwrap(), None);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_shadow_alias_pruning_contracts() {
        shadow_alias_pruning_dead_aliases_leave_only_the_live_instance();
        shadow_alias_pruning_dead_alias_index_moves_to_the_surviving_instance();
        shadow_alias_pruning_callable_keeps_one_fallback_record_then_releases_it();
        shadow_alias_pruning_captured_path_resolves_the_original_heap_after_replacement();
        shadow_alias_pruning_all_live_instances_survive_then_dead_ones_are_released();
        shadow_alias_pruning_collection_snapshot_keeps_data_without_an_owner_identity();
        shadow_alias_pruning_timer_work_keeps_one_route_until_delivery();
        shadow_alias_pruning_promise_work_keeps_one_route_until_checkpoint();
        shadow_alias_pruning_kept_targets_keep_one_route_until_clear_kept_objects();
        shadow_alias_pruning_busy_child_defers_pruning_until_full_collection();
        shadow_alias_pruning_captured_path_survives_pruning_and_does_not_revive_a_dead_vm();
        shadow_alias_pruning_captured_path_uses_heap_identity_without_a_cached_index();
        shadow_alias_pruning_captured_path_marks_a_busy_surviving_route_for_retry();
        shadow_alias_pruning_heap_fallback_still_excludes_another_document();
        shadow_alias_pruning_noncanonical_parent_releases_its_dead_access_records();
        shadow_alias_pruning_terminal_warm_collection_releases_discarded_owners();
        shadow_alias_pruning_live_backing_stays_charged_until_the_last_route_is_gone();
        shadow_alias_pruning_captured_path_changes_parents_after_its_old_edge_is_pruned();
    }
}
