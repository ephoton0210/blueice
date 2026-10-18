// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny mixed-realm adoption cases; no quota or exhaustion probes.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn shadow(vm: &mut Vm) -> ShadowRealmRecord {
    let value = run(vm, "new ShadowRealm();");
    let instance = value.object_id().unwrap();
    vm.heap.root(instance).unwrap();
    vm.shadow_realms[&instance].clone()
}
fn tree() -> (Vm, ShadowRealmRecord, ShadowRealmRecord) {
    let mut root = Vm::default();
    let child = shadow(&mut root);
    let grandchild = shadow(&mut child.vm.borrow_mut());
    (root, child, grandchild)
}

#[cfg_attr(test, test)]
fn document_policy_late_activation_adopts_existing_shadow_descendants() {
    let (mut root, child, grandchild) = tree();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    let scope = root.host_scheduling.as_ref().unwrap();
    for record in [&child, &grandchild] {
        let vm = record.vm.borrow();
        assert!(
            vm.host_scheduling
                .as_ref()
                .is_some_and(|child| scope.same_document(child)),
            "every existing same-document realm must join the document account"
        );
        assert!(vm.bounded_host_microtask_checkpoints);
    }
}

#[cfg_attr(test, test)]
fn document_policy_late_activation_counts_every_managed_heap_once() {
    let (mut root, child, grandchild) = tree();
    let expected = root.heap.stats().managed_bytes
        + child.vm.borrow().heap.stats().managed_bytes
        + grandchild.vm.borrow().heap.stats().managed_bytes;
    root.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(
        root.heap.document_heap_budget().unwrap().0,
        expected,
        "the document owns all existing member heaps"
    );
    assert_eq!(
        child.vm.borrow().heap.document_heap_budget(),
        root.heap.document_heap_budget()
    );
    assert_eq!(
        grandchild.vm.borrow().heap.document_heap_budget(),
        root.heap.document_heap_budget()
    );
}

#[cfg_attr(test, test)]
fn document_policy_late_activation_includes_pending_child_work() {
    let mut root = Vm::default();
    let child = shadow(&mut root);
    {
        let mut vm = child.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm,"var seen=0;setTimeout(()=>{seen+=1;},1);var pending=Promise.withResolvers();pending.promise.then(()=>{seen+=2;});0;");
    }
    root.use_bounded_host_microtask_checkpoints().unwrap();
    let usage = root.host_scheduling_usage().unwrap();
    assert_eq!(
        usage.pending_timers, 1,
        "existing child timers must enter the shared document account"
    );
    assert!(usage.pending_reactions > 0);
}

#[cfg_attr(test, test)]
fn document_policy_fresh_test262_realms_inherit_the_document() {
    let mut root = Vm::default();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    root.install_test262_harness().unwrap();
    run(&mut root, "var ownedRealm=$262.createRealm();0;");
    let child = &root.test262_realms.values().next().unwrap().vm.borrow_mut();
    assert!(
        child.host_scheduling.as_ref().is_some_and(|child| root
            .host_scheduling
            .as_ref()
            .unwrap()
            .same_document(child)),
        "fresh Test262 children must not become independent neutral budgets"
    );
    assert_eq!(
        child.heap.document_heap_budget(),
        root.heap.document_heap_budget()
    );
}

#[cfg_attr(test, test)]
fn document_policy_fresh_shadow_realms_attach_before_publication() {
    let mut root = Vm::default();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    let child = shadow(&mut root);
    let vm = child.vm.borrow();
    assert!(vm.document_policy_ready);
    assert!(vm.bounded_host_microtask_checkpoints);
    assert!(vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(root.host_scheduling.as_ref().unwrap()));
    assert_eq!(
        vm.heap.document_heap_budget(),
        root.heap.document_heap_budget()
    );
}

#[cfg_attr(test, test)]
fn document_policy_mixed_owned_and_shared_descendants_join_once() {
    let mut root = Vm::default();
    root.install_test262_harness().unwrap();
    run(&mut root, "var ownedRealm=$262.createRealm();0;");
    let owned = root.test262_realms.values_mut().next().unwrap();
    let middle = shadow(&mut owned.vm.borrow_mut());
    let leaf = {
        let mut vm = middle.vm.borrow_mut();
        vm.install_test262_harness().unwrap();
        run(&mut vm, "var ownedRealm=$262.createRealm();0;");
        let leaf = shadow(
            &mut vm
                .test262_realms
                .values_mut()
                .next()
                .unwrap()
                .vm
                .borrow_mut(),
        );
        leaf
    };
    root.use_bounded_host_microtask_checkpoints().unwrap();
    let expected = root.heap.stats().managed_bytes
        + root
            .test262_realms
            .values()
            .next()
            .unwrap()
            .vm
            .borrow_mut()
            .heap
            .stats()
            .managed_bytes
        + middle.vm.borrow().heap.stats().managed_bytes
        + middle
            .vm
            .borrow()
            .test262_realms
            .values()
            .next()
            .unwrap()
            .vm
            .borrow_mut()
            .heap
            .stats()
            .managed_bytes
        + leaf.vm.borrow().heap.stats().managed_bytes;
    assert_eq!(root.heap.document_heap_budget().unwrap().0, expected);
    let scope = root.host_scheduling.as_ref().unwrap();
    assert!(root
        .test262_realms
        .values()
        .next()
        .unwrap()
        .vm
        .borrow_mut()
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(scope));
    assert!(middle
        .vm
        .borrow()
        .test262_realms
        .values()
        .next()
        .unwrap()
        .vm
        .borrow_mut()
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(scope));
    assert!(leaf
        .vm
        .borrow()
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(scope));
}

#[cfg_attr(test, test)]
fn document_policy_shared_aliases_and_cycles_do_not_duplicate_heap_charges() {
    let mut root = Vm::default();
    let first = shadow(&mut root);
    let second = shadow(&mut root);
    let expected = root.heap.stats().managed_bytes
        + first.vm.borrow().heap.stats().managed_bytes
        + second.vm.borrow().heap.stats().managed_bytes;
    root.adopt_shadow_realm(root.object_prototype, first.clone());
    {
        let mut vm = first.vm.borrow_mut();
        let id = vm.object_prototype;
        vm.adopt_shadow_realm(id, second.clone());
    }
    {
        let mut vm = second.vm.borrow_mut();
        let id = vm.object_prototype;
        vm.adopt_shadow_realm(id, first.clone());
    }
    root.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(root.heap.document_heap_budget().unwrap().0, expected);
    first.vm.borrow_mut().shadow_realms.clear();
    second.vm.borrow_mut().shadow_realms.clear();
}

#[cfg_attr(test, test)]
fn document_policy_busy_member_refuses_without_publishing_then_retries() {
    let (mut root, child, _) = tree();
    let borrowed = child.vm.borrow_mut();
    assert!(matches!(
        root.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::Unsupported(_))
    ));
    assert!(root.host_scheduling.is_none());
    assert!(borrowed.host_scheduling.is_none());
    assert!(root.heap.document_heap_budget().is_none());
    drop(borrowed);
    root.use_bounded_host_microtask_checkpoints().unwrap();
    assert!(child.vm.borrow().document_policy_ready);
}

#[cfg_attr(test, test)]
fn document_policy_active_neutral_root_refuses_without_changing_policy() {
    let mut root = Vm::default();
    let guard = realm_reentrancy::register_active(&mut root).unwrap();
    assert!(matches!(
        root.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::Unsupported(_))
    ));
    assert!(root.host_scheduling.is_none());
    drop(guard);
    root.use_bounded_host_microtask_checkpoints().unwrap();
}

#[cfg_attr(test, test)]
fn document_policy_foreign_shared_document_keeps_its_account() {
    let mut foreign = Vm::default();
    foreign.use_bounded_host_microtask_checkpoints().unwrap();
    let record = shadow(&mut foreign);
    let mut root = Vm::default();
    root.adopt_shadow_realm(root.object_prototype, record.clone());
    let expected = root.heap.stats().managed_bytes;
    root.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(root.heap.document_heap_budget().unwrap().0, expected);
    assert!(record
        .vm
        .borrow()
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(foreign.host_scheduling.as_ref().unwrap()));
    assert!(!root
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(foreign.host_scheduling.as_ref().unwrap()));
    foreign.host_scheduling.as_ref().unwrap().fail();
    root.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn document_policy_foreign_owned_document_is_refused_without_rebinding() {
    let mut root = Vm::default();
    root.install_test262_harness().unwrap();
    run(&mut root, "var ownedRealm=$262.createRealm();0;");
    root.test262_realms
        .values_mut()
        .next()
        .unwrap()
        .vm
        .borrow_mut()
        .use_bounded_host_microtask_checkpoints()
        .unwrap();
    assert!(matches!(
        root.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(matches!(
        root.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    ));
    root.test262_realms
        .values()
        .next()
        .unwrap()
        .vm
        .borrow_mut()
        .check_scheduling_failure()
        .unwrap();
}

#[cfg_attr(test, test)]
fn document_policy_repeated_activation_reuses_all_document_ownership() {
    let (mut root, child, _) = tree();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    let usage = root.host_scheduling_usage().unwrap();
    let managed = root.heap.document_heap_budget();
    for _ in 0..3 {
        root.use_bounded_host_microtask_checkpoints().unwrap();
    }
    assert_eq!(root.host_scheduling_usage().unwrap(), usage);
    assert_eq!(root.heap.document_heap_budget(), managed);
    assert_eq!(child.vm.borrow().heap.document_heap_budget(), managed);
}

#[cfg_attr(test, test)]
fn document_policy_pending_timers_keep_handles_deadlines_and_unique_serials() {
    let mut root = Vm::default();
    root.install_host_timer_functions().unwrap();
    let child = shadow(&mut root);
    run(&mut root, "setTimeout(()=>0,9);setTimeout(()=>0,11);0;");
    {
        let mut vm = child.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm, "setTimeout(()=>0,5);0;");
        vm.host_timer_clock_ms = 3;
    }
    let root_id = root.host_timers[0].id;
    let child_id = child.vm.borrow().host_timers[0].id;
    root.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(root.host_timers[0].id, root_id);
    assert_eq!(root.host_timers[0].due_ms, 9);
    let vm = child.vm.borrow();
    assert_eq!(vm.host_timers[0].id, child_id);
    assert_eq!(vm.host_timers[0].due_ms, 5);
    assert!(root.host_timers[0].serial < root.host_timers[1].serial);
    assert_ne!(root.host_timers[0].serial, vm.host_timers[0].serial);
    assert_ne!(root.host_timers[1].serial, vm.host_timers[0].serial);
    let last = root
        .host_scheduling
        .as_ref()
        .unwrap()
        .next_task_serial()
        .unwrap();
    assert_eq!(last, 3);
    assert_eq!(root.host_scheduling.as_ref().unwrap().advance_clock(0), 3);
}

#[cfg_attr(test, test)]
fn document_policy_adopted_timer_and_promise_complete_with_gc() {
    let mut root = Vm::default();
    let child = shadow(&mut root);
    {
        let mut vm = child.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm,"var seen=0;var pending=Promise.withResolvers();pending.promise.then(()=>{seen+=2;});setTimeout(()=>{seen+=1;pending.resolve(0);},1);0;");
    }
    root.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut child.vm.borrow_mut(), "seen;"), Value::Number(3.0));
    run(&mut child.vm.borrow_mut(), "pending=null;0;");
    root.collect_realm_garbage().unwrap();
    let usage = root.host_scheduling_usage().unwrap();
    assert_eq!(usage.pending_timers, 0);
    assert_eq!(usage.pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn document_policy_managed_terminal_state_stops_detached_native_owners() {
    let (mut root, child, _) = tree();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    let scope = root.host_scheduling.as_ref().unwrap();
    let mut charge = scope.shared_charge(8).unwrap();
    root.heap
        .document_heap_budget_handle()
        .unwrap()
        .fail_for_test();
    assert!(matches!(
        charge.reserve_peer(1),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(matches!(
        root.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(matches!(
        child.vm.borrow().check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    ));
    charge.release_to(0);
    let mut healthy = Vm::default();
    healthy.use_bounded_host_microtask_checkpoints().unwrap();
    healthy.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn document_policy_managed_account_rejects_a_different_native_identity() {
    let mut first = Vm::default();
    first.use_bounded_host_microtask_checkpoints().unwrap();
    let mut second = Vm::default();
    second.use_bounded_host_microtask_checkpoints().unwrap();
    let budget = first.heap.document_heap_budget_handle().unwrap();
    assert!(matches!(
        budget.bind_native(
            &second
                .host_scheduling
                .as_ref()
                .unwrap()
                .heap_native_budget()
        ),
        Err(crate::HeapError::InvalidConfig)
    ));
    first.check_scheduling_failure().unwrap();
    second.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn document_policy_immutably_borrowed_member_refuses_before_any_publication() {
    let (mut root, child, _) = tree();
    let borrowed = child.vm.borrow();
    assert!(matches!(
        root.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::Unsupported(_))
    ));
    assert!(root.host_scheduling.is_none());
    assert!(root.heap.document_heap_budget().is_none());
    assert!(!borrowed.document_policy_ready);
    drop(borrowed);
    root.use_bounded_host_microtask_checkpoints().unwrap();
}

#[cfg_attr(test, test)]
fn document_policy_existing_scope_joins_its_associated_managed_account() {
    let mut owner = Vm::default();
    owner.use_bounded_host_microtask_checkpoints().unwrap();
    let mut member = Vm {
        host_scheduling: owner.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    };
    let expected = owner.heap.stats().managed_bytes + member.heap.stats().managed_bytes;
    member.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(owner.heap.document_heap_budget().unwrap().0, expected);
    assert_eq!(
        owner.heap.document_heap_budget(),
        member.heap.document_heap_budget()
    );
    member.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(owner.heap.document_heap_budget().unwrap().0, expected);
}

#[cfg_attr(test, test)]
fn document_policy_active_foreign_record_is_excluded_without_changing_it() {
    let mut foreign = Vm::default();
    foreign.use_bounded_host_microtask_checkpoints().unwrap();
    let record = shadow(&mut foreign);
    let mut root = Vm::default();
    root.adopt_shadow_realm(root.object_prototype, record.clone());
    let mut borrowed = record.vm.borrow_mut();
    let guard = realm_reentrancy::register_active(&mut borrowed).unwrap();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    assert!(!root
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(borrowed.host_scheduling.as_ref().unwrap()));
    borrowed.check_scheduling_failure().unwrap();
    drop(guard);
    drop(borrowed);
    foreign.check_scheduling_failure().unwrap();
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_document_policy_contracts() {
        document_policy_late_activation_adopts_existing_shadow_descendants();
        document_policy_late_activation_counts_every_managed_heap_once();
        document_policy_late_activation_includes_pending_child_work();
        document_policy_fresh_test262_realms_inherit_the_document();
        document_policy_fresh_shadow_realms_attach_before_publication();
        document_policy_mixed_owned_and_shared_descendants_join_once();
        document_policy_shared_aliases_and_cycles_do_not_duplicate_heap_charges();
        document_policy_busy_member_refuses_without_publishing_then_retries();
        document_policy_active_neutral_root_refuses_without_changing_policy();
        document_policy_foreign_shared_document_keeps_its_account();
        document_policy_foreign_owned_document_is_refused_without_rebinding();
        document_policy_repeated_activation_reuses_all_document_ownership();
        document_policy_pending_timers_keep_handles_deadlines_and_unique_serials();
        document_policy_adopted_timer_and_promise_complete_with_gc();
        document_policy_managed_terminal_state_stops_detached_native_owners();
        document_policy_managed_account_rejects_a_different_native_identity();
        document_policy_immutably_borrowed_member_refuses_before_any_publication();
        document_policy_existing_scope_joins_its_associated_managed_account();
        document_policy_active_foreign_record_is_excluded_without_changing_it();
    }
}
