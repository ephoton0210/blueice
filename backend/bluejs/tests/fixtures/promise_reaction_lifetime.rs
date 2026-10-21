// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny reaction buffers and await copies retain their original account.

use super::*;
use crate::vm::host_scheduling::{reaction_bytes, slots, value_payload_bytes};

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
fn register(vm: &mut Vm, promise: ObjectId, target: ObjectId) {
    vm.perform_promise_then(
        promise,
        Value::String("yes".into()),
        Value::String("no".into()),
        ReactionTarget::Native(target),
    )
    .unwrap();
}
fn record_bytes(record: &PromiseRecord) -> usize {
    let status = match &record.status {
        PromiseStatus::Pending => 0,
        PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => {
            value_payload_bytes(value)
        }
    };
    status
        + record.rejection_origin.payload_bytes()
        + slots(&record.reactions)
        + record.reactions.iter().map(reaction_bytes).sum::<usize>()
}
fn bytes(vm: &Vm) -> usize {
    vm.promises.retained_bytes() + vm.promises.values().map(record_bytes).sum::<usize>()
}
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_scoped_pending_records_outlive_the_vm() {
    let mut vm = page();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_borrowed_pending_records_keep_their_account() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let (promise, target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    register(&mut vm, promise, target);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_detached_record_keeps_backing_and_handlers() {
    let mut vm = page();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    let view = observer(&vm);
    let record = vm.promises.remove(&promise).unwrap();
    let retained = record_bytes(&record);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(record);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_failed_pending_owner_refuses_registration() {
    let source = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let (promise, target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert!(vm.promises[&promise].reactions.is_empty());
    assert!(vm.promise_jobs.is_empty());
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_failed_settled_owner_refuses_registration() {
    let source = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let (promise, target) = loan(&mut vm, &source, |vm| Ok(pair(vm))).unwrap();
    vm.settle_promise(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert!(vm.promise_jobs.is_empty());
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_borrowed_await_copy_outlives_the_vm() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    vm.settle_promise(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    let copy = vm.prepare_promise_await_status(promise).unwrap();
    drop(vm);
    assert!(
        matches!(&copy.0, PromiseAwaitStatus::Fulfilled(Value::String(text)) if text == "tiny")
    );
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before + 8);
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_failed_owner_refuses_an_await_copy() {
    let source = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    vm.settle_promise(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        vm.prepare_promise_await_status(promise),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(
        vm.await_value(Value::Object(promise)),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_existing_pending_handlers_adopt_with_the_table() {
    let mut vm = Vm::default();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.new_promise().unwrap();
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_settlement_retires_the_old_reaction_buffer() {
    let mut vm = page();
    let (promise, target) = pair(&mut vm);
    register(&mut vm, promise, target);
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)))
        .unwrap();
    assert_eq!(vm.promises[&promise].reactions.capacity(), 0);
    assert_eq!(vm.promise_jobs.len(), 1);
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_reaction_lifetime_public_pending_callback_survives_manual_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm,"var resolve;var result='';var p=new Promise(r=>resolve=r);var q=p.then(value=>result=value);$262.gc();resolve('tiny');0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(run(&mut vm, "result;"), Value::String("tiny".into()));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_reaction_lifetime_contracts() {
        promise_reaction_lifetime_scoped_pending_records_outlive_the_vm();
        promise_reaction_lifetime_borrowed_pending_records_keep_their_account();
        promise_reaction_lifetime_detached_record_keeps_backing_and_handlers();
        promise_reaction_lifetime_failed_pending_owner_refuses_registration();
        promise_reaction_lifetime_failed_settled_owner_refuses_registration();
        promise_reaction_lifetime_borrowed_await_copy_outlives_the_vm();
        promise_reaction_lifetime_failed_owner_refuses_an_await_copy();
        promise_reaction_lifetime_existing_pending_handlers_adopt_with_the_table();
        promise_reaction_lifetime_settlement_retires_the_old_reaction_buffer();
        promise_reaction_lifetime_public_pending_callback_survives_manual_gc();
    }
}
