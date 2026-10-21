// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Existing job producers retain native data, pending counts and cancellation.

use super::*;
use crate::vm::promise_job_storage::{PromiseJobQueue, QueuedPromiseJob};

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
fn job() -> PromiseJob {
    PromiseJob::HostMicrotask {
        callback: Value::String("tiny".into()),
    }
}
fn bytes(queue: &PromiseJobQueue) -> usize {
    queue.capacity() * std::mem::size_of::<QueuedPromiseJob>()
        + queue
            .iter()
            .map(PromiseJob::scheduling_payload_bytes)
            .sum::<usize>()
}
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_scoped_queue_keeps_content_backing_and_pending_count() {
    let mut vm = page();
    vm.publish_promise_job(job()).unwrap();
    let view = observer(&vm);
    let retained = bytes(&vm.promise_jobs);
    let queue = std::mem::take(&mut vm.promise_jobs);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 1);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 0);
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_borrowed_queue_keeps_its_original_account() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| vm.publish_promise_job(job())).unwrap();
    let retained = bytes(&vm.promise_jobs);
    let queue = std::mem::take(&mut vm.promise_jobs);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 1);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 0);
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_popped_content_remains_charged_until_destroyed() {
    let mut vm = page();
    vm.publish_promise_job(job()).unwrap();
    let view = observer(&vm);
    let active = vm.promise_jobs.pop_front().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 8);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 0);
    drop(active);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_canceled_original_refuses_execution_before_pop() {
    let source = page();
    let mut vm = Vm::default();
    run(&mut vm, "var called=0;function callback(){called++;}0;");
    let callback = run(&mut vm, "callback;");
    loan(&mut vm, &source, |vm| {
        vm.publish_promise_job(PromiseJob::HostMicrotask { callback })
    })
    .unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.run_next_promise_job(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promise_jobs.len(), 1);
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_canceled_original_refuses_publication() {
    let source = page();
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| vm.publish_promise_job(job())).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.publish_promise_job(job()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promise_jobs.len(), 1);
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_policy_discovery_refuses_queue_account_reassignment() {
    let source = page();
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| vm.publish_promise_job(job())).unwrap();
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!vm.document_policy_ready);
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_neutral_finalization_producer_keeps_its_loan() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    run(
        &mut vm,
        "var registry=new FinalizationRegistry(value=>0);registry.register({},'tiny');0;",
    );
    loan(&mut vm, &source, |vm| {
        vm.with_roots(|heap| heap.try_collect_major())?;
        vm.enqueue_finalization_cleanup_jobs()
    })
    .unwrap();
    assert_eq!(vm.promise_jobs.len(), 1);
    let retained = bytes(&vm.promise_jobs);
    let queue = std::mem::take(&mut vm.promise_jobs);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 1);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 0);
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_settlement_keeps_the_copied_job_independent() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let target = vm.new_promise().unwrap();
    vm.perform_promise_then(
        promise,
        Value::String("yes".into()),
        Value::String("no".into()),
        ReactionTarget::Native(target),
    )
    .unwrap();
    vm.settle_promise(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    let view = observer(&vm);
    let retained = bytes(&vm.promise_jobs);
    let queue = std::mem::take(&mut vm.promise_jobs);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 1);
    drop(queue);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 0);
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_public_checkpoint_preserves_callback_order() {
    let mut vm = page();
    run(
        &mut vm,
        "var trace=[];Promise.resolve('tiny').then(value=>trace.push(value));trace.push('sync');0;",
    );
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        run(&mut vm, "trace.join(',');"),
        Value::String("sync,tiny".into())
    );
}

#[cfg_attr(test, test)]
fn promise_job_lifetime_empty_queue_gc_and_drop_refund_all_credit() {
    let mut vm = page();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_microtasks, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_job_lifetime_contracts() {
        promise_job_lifetime_scoped_queue_keeps_content_backing_and_pending_count();
        promise_job_lifetime_borrowed_queue_keeps_its_original_account();
        promise_job_lifetime_popped_content_remains_charged_until_destroyed();
        promise_job_lifetime_canceled_original_refuses_execution_before_pop();
        promise_job_lifetime_canceled_original_refuses_publication();
        promise_job_lifetime_policy_discovery_refuses_queue_account_reassignment();
        promise_job_lifetime_neutral_finalization_producer_keeps_its_loan();
        promise_job_lifetime_settlement_keeps_the_copied_job_independent();
        promise_job_lifetime_public_checkpoint_preserves_callback_order();
        promise_job_lifetime_empty_queue_gc_and_drop_refund_all_credit();
    }
}
