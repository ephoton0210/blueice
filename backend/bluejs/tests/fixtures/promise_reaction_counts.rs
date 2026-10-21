// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed pending registrations keep count credit through their lifecycle.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
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
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?;
    vm.with_realm_call_roots(roots, action)
}
fn pair(vm: &mut Vm) -> (ObjectId, ObjectId) {
    (vm.new_promise().unwrap(), vm.new_promise().unwrap())
}
fn reaction(target: ObjectId) -> PromiseReaction {
    PromiseReaction::Then(PromiseThenReaction {
        target: ReactionTarget::Native(target),
        on_fulfilled: Value::Undefined,
        on_rejected: Value::Undefined,
    })
}
fn register(vm: &mut Vm, promise: ObjectId, target: ObjectId) {
    vm.perform_promise_then(
        promise,
        Value::Undefined,
        Value::Undefined,
        ReactionTarget::Native(target),
    )
    .unwrap();
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_borrowed_registration_uses_its_original_account() {
    let source = page();
    let view = observer(&source);
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let (promise, target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    register(&mut vm, promise, target);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_scoped_records_keep_credit_after_vm_drop() {
    let mut vm = page();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    let view = observer(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_prepared_registration_keeps_its_reservation() {
    let source = page();
    let view = observer(&source);
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let (promise, target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    let prepared = AwaitRegistration::Reaction {
        promise,
        reaction: reaction(target),
    }
    .prepare(&mut vm, 0, None)
    .unwrap();
    assert!(vm.promises[&promise].reactions.is_empty());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_existing_registrations_adopt_with_the_record() {
    let mut vm = Vm::default();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.new_promise().unwrap();
    let view = observer(&vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_gc_releases_only_collected_registrations() {
    let source = page();
    let view = observer(&source);
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let (first, first_target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    let (second, second_target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    vm.heap.root(first).unwrap();
    register(&mut vm, first, first_target);
    register(&mut vm, second, second_target);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 2);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.promises.contains_key(&second));
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_another_caller_does_not_reassign_the_registration() {
    let source = page();
    let caller = page();
    let home = observer(&source);
    let other = observer(&caller);
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let (promise, target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    loan(&mut vm, &caller, |vm| {
        register(vm, promise, target);
        Ok(())
    })
    .unwrap();
    assert_eq!(home.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(other.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_settlement_transfers_pending_work_to_the_job() {
    let mut vm = page();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)))
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(vm.host_scheduling_snapshot().pending_microtasks, 1);
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_microtasks, 0);
    assert!(matches!(
        vm.promises[&target].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
}

#[cfg_attr(test, test)]
fn promise_reaction_counts_terminal_cleanup_releases_existing_work() {
    let mut vm = page();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    vm.promises.get_mut(&promise).unwrap().release_reactions();
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_reaction_counts_contracts() {
        promise_reaction_counts_borrowed_registration_uses_its_original_account();
        promise_reaction_counts_scoped_records_keep_credit_after_vm_drop();
        promise_reaction_counts_prepared_registration_keeps_its_reservation();
        promise_reaction_counts_existing_registrations_adopt_with_the_record();
        promise_reaction_counts_gc_releases_only_collected_registrations();
        promise_reaction_counts_another_caller_does_not_reassign_the_registration();
        promise_reaction_counts_settlement_transfers_pending_work_to_the_job();
        promise_reaction_counts_terminal_cleanup_releases_existing_work();
    }
}
