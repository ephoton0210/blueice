// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny Then registrations and numerical peers exercise native ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    vm
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
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
fn promise_reaction_storage_settled_payload_refusal_preserves_unhandled_state() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let target = vm.new_promise().unwrap();
    vm.settle_promise(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    let _peer = leave_room(&mut vm, 0);
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_pending_growth_refusal_preserves_unhandled_state() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let target = vm.new_promise().unwrap();
    let _peer = leave_room(&mut vm, 0);
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert!(vm.promises[&promise].reactions.is_empty());
    assert_eq!(vm.promises[&promise].reactions.capacity(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_refusal_retains_pending_rejection_and_root() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let target = vm.new_promise().unwrap();
    vm.host_rejections = Some(host_rejections::HostRejections::default());
    vm.settle_promise(promise, PromiseStatus::Rejected(Value::Number(7.0)))
        .unwrap();
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 1);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

fn pair() -> (Vm, ObjectId, ObjectId) {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let target = vm.new_promise().unwrap();
    (vm, promise, target)
}
fn origin() -> ThrowOrigin {
    ThrowOrigin {
        source: Some(crate::diagnostics::DiagnosticSourceLocation {
            filename: "tiny.js".into(),
            line: 3,
            column: 5,
        }),
        epoch: 7,
        ..Default::default()
    }
}
fn settled(rejected: bool, tracking: bool) -> (Vm, ObjectId, ObjectId) {
    let (mut vm, promise, target) = pair();
    if tracking {
        vm.host_rejections = Some(host_rejections::HostRejections::default());
    }
    vm.promise_origin_override = Some(origin());
    let value = Value::String("tiny".into());
    vm.settle_promise(
        promise,
        if rejected {
            PromiseStatus::Rejected(value)
        } else {
            PromiseStatus::Fulfilled(value)
        },
    )
    .unwrap();
    (vm, promise, target)
}
fn job_slot(vm: &mut Vm) {
    vm.enqueue_promise_job(PromiseJob::HostMicrotask {
        callback: Value::Undefined,
    });
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn promise_reaction_storage_generic_pending_refusal_does_not_mark_handled() {
    let (mut vm, promise, _) = pair();
    let _peer = leave_room(&mut vm, 0);
    vm.enqueue_promise_reaction(promise, PromiseReaction::AsyncAwait { continuation: 0 });
    assert!(!vm.promises[&promise].handled);
    assert!(vm.promises[&promise].reactions.is_empty());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_exact_settled_spare_slot_charges_only_its_copy() {
    let (mut vm, promise, target) = settled(false, false);
    job_slot(&mut vm);
    vm.remaining_instructions = 0;
    let _peer = leave_room(&mut vm, 8);
    register(&mut vm, promise, target);
    assert!(vm.promises[&promise].handled);
    assert_eq!(vm.promise_jobs.len(), 2);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert_eq!(vm.promise_jobs[1].rejection_origin().unwrap().epoch, 0);
    assert_eq!(vm.remaining_instructions, 0);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_exact_rejection_copy_preserves_its_record_source() {
    let (mut vm, promise, target) = settled(true, false);
    job_slot(&mut vm);
    let _peer = leave_room(&mut vm, 8 + "tiny.js".len());
    vm.perform_promise_then(
        promise,
        Value::String("unused".into()),
        Value::Undefined,
        ReactionTarget::Native(target),
    )
    .unwrap();
    assert!(vm.promises[&promise].handled);
    let copied = vm.promise_jobs[1].rejection_origin().unwrap();
    assert_eq!(copied.epoch, 7);
    assert_eq!(copied.source.as_ref().unwrap().filename, "tiny.js");
    assert_eq!(vm.promises[&promise].rejection_origin.epoch, 7);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_short_source_allowance_refuses_without_queue_mutation() {
    let (mut vm, promise, target) = settled(true, false);
    job_slot(&mut vm);
    let _peer = leave_room(&mut vm, 8 + "tiny.js".len() - 1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert_eq!(vm.promise_jobs.len(), 1);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.promises[&promise].rejection_origin.epoch, 7);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_exact_pending_registration_adopts_both_handlers() {
    let (mut vm, promise, target) = pair();
    let on_fulfilled = Value::String("yes".into());
    let on_rejected = Value::String("no".into());
    let payload = host_scheduling::value_payload_bytes(&on_fulfilled)
        + host_scheduling::value_payload_bytes(&on_rejected);
    let _peer = leave_room(
        &mut vm,
        4 * std::mem::size_of::<PromiseReaction>() + payload,
    );
    vm.perform_promise_then(
        promise,
        on_fulfilled,
        on_rejected,
        ReactionTarget::Native(target),
    )
    .unwrap();
    assert!(vm.promises[&promise].handled);
    assert_eq!(vm.promises[&promise].reactions.len(), 1);
    assert_eq!(vm.promises[&promise].reactions.capacity(), 4);
    assert!(
        matches!(&vm.promises[&promise].reactions[0], PromiseReaction::Then(PromiseThenReaction { on_fulfilled: Value::String(a), on_rejected: Value::String(b), .. }) if a.to_utf8().unwrap() == "yes" && b.to_utf8().unwrap() == "no")
    );
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_pending_spare_slot_accepts_zero_payload_at_full_allowance() {
    let (mut vm, promise, target) = pair();
    vm.publish_promise_reaction(promise, PromiseReaction::AsyncAwait { continuation: 0 })
        .unwrap();
    assert!(!vm.promises[&promise].handled);
    let _peer = leave_room(&mut vm, 0);
    register(&mut vm, promise, target);
    assert!(vm.promises[&promise].handled);
    assert_eq!(vm.promises[&promise].reactions.len(), 2);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_pending_replacement_requires_complete_coexisting_backing() {
    let (mut vm, promise, target) = pair();
    for continuation in 0..4 {
        vm.publish_promise_reaction(promise, PromiseReaction::AsyncAwait { continuation })
            .unwrap();
    }
    let _peer = leave_room(&mut vm, 4 * std::mem::size_of::<PromiseReaction>());
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert_eq!(vm.promises[&promise].reactions.len(), 4);
    assert_eq!(vm.promises[&promise].reactions.capacity(), 4);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_complete_job_replacement_refunds_old_backing() {
    let (mut vm, promise, target) = settled(false, false);
    for _ in 0..4 {
        job_slot(&mut vm);
    }
    let _peer = leave_room(&mut vm, 8 * std::mem::size_of::<PromiseJob>() + 8);
    register(&mut vm, promise, target);
    assert!(vm.promises[&promise].handled);
    assert_eq!(vm.promise_jobs.len(), 5);
    assert_eq!(vm.promise_jobs.capacity(), 8);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - 4 * std::mem::size_of::<PromiseJob>()
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_abandoned_preparation_refunds_without_handling_rejection() {
    let (mut vm, promise, target) = settled(true, true);
    let before = vm.host_scheduling_usage().unwrap();
    let roots = vm.heap.root_storage_state();
    let prepared = vm
        .prepare_then_registration(
            promise,
            Value::Undefined,
            Value::Undefined,
            ReactionTarget::Native(target),
        )
        .unwrap();
    assert!(!vm.promises[&promise].handled);
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 1);
    drop(prepared);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_scheduling_usage().unwrap(), before);
    assert_eq!(vm.promise_jobs.capacity(), 0);
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_rust_held_preparation_keeps_its_lease_after_vm_drop() {
    let (mut vm, promise, target) = settled(true, false);
    let view = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm
        .prepare_then_registration(
            promise,
            Value::Undefined,
            Value::Undefined,
            ReactionTarget::Native(target),
        )
        .unwrap();
    let extra = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, extra);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_scalar_job_quota_preserves_notification_ownership() {
    let (mut vm, promise, target) = settled(true, true);
    let roots = vm.heap.root_storage_state();
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_microtasks: MAX_PENDING_HOST_MICROTASKS,
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap();
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_scalar_reaction_quota_preserves_pending_registration() {
    let (mut vm, promise, target) = pair();
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_reactions: MAX_PENDING_HOST_REACTIONS,
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap();
    register(&mut vm, promise, target);
    assert!(!vm.promises[&promise].handled);
    assert_eq!(vm.promises[&promise].reactions.capacity(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
fn notified() -> (Vm, ObjectId, ObjectId) {
    let (mut vm, promise, target) = pair();
    vm.settle_promise(promise, PromiseStatus::Rejected(Value::Number(7.0)))
        .unwrap();
    vm.host_rejections = Some(host_rejections::HostRejections::default());
    vm.promises.get_mut(&promise).unwrap().rejection_notified = true;
    (vm, promise, target)
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_late_notification_refusal_keeps_committed_job_and_bit() {
    let (mut vm, promise, target) = notified();
    vm.heap.allow_root_registrations(0);
    let roots = vm.heap.root_storage_state();
    register(&mut vm, promise, target);
    assert!(vm.promises[&promise].handled);
    assert!(vm.promises[&promise].rejection_notified);
    assert_eq!(vm.promise_jobs.len(), 1);
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_successful_registration_releases_pending_notification_root() {
    let (mut vm, promise, target) = settled(true, true);
    let roots = vm.heap.root_storage_state();
    register(&mut vm, promise, target);
    assert!(vm.promises[&promise].handled);
    assert!(!vm.promises[&promise].rejection_notified);
    assert_eq!(vm.promise_jobs.len(), 1);
    assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots.0 - 1);
    assert_eq!(vm.heap.root_storage_state().2, roots.2);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_successful_late_handling_publishes_followup_after_job() {
    let (mut vm, promise, target) = notified();
    register(&mut vm, promise, target);
    assert!(vm.promises[&promise].handled);
    assert!(!vm.promises[&promise].rejection_notified);
    assert_eq!(vm.promise_jobs.len(), 1);
    assert_eq!(vm.browser_tasks.len(), 1);
    assert!(
        matches!(vm.browser_tasks.front().unwrap(), browser_tasks::BrowserTask::PromiseRejections { handled: true, entries, .. } if entries.len() == 1 && entries[0].promise == promise)
    );
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_already_handled_sources_still_register_each_job() {
    let (mut vm, promise, target) = pair();
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)))
        .unwrap();
    vm.mark_promise_handled(promise);
    job_slot(&mut vm);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    register(&mut vm, promise, target);
    register(&mut vm, promise, target);
    assert_eq!(vm.promise_jobs.len(), 3);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_invalid_receiver_retains_brand_error_without_admission() {
    let (mut vm, _, target) = pair();
    let ordinary = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    assert_eq!(
        vm.perform_promise_then(
            ordinary,
            Value::Undefined,
            Value::Undefined,
            ReactionTarget::Native(target)
        ),
        Err(RuntimeError::TypeError("invalid Promise receiver".into()))
    );
    assert!(vm.promise_jobs.is_empty());
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_published_job_roots_survive_notification_release_then_collect() {
    let (mut vm, promise, target) = pair();
    let reason = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.host_rejections = Some(host_rejections::HostRejections::default());
    vm.settle_promise(promise, PromiseStatus::Rejected(Value::Object(reason)))
        .unwrap();
    register(&mut vm, promise, target);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(promise));
    assert!(vm.heap.contains(reason) && vm.heap.contains(target));
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(
        matches!(vm.promises[&target].status, PromiseStatus::Rejected(Value::Object(id)) if id == reason)
    );
    vm.mark_promise_handled(target);
    // The checkpoint moved the new rejection's root into its browser task.
    // Handling suppresses delivery, but that queued owner must still be drained.
    assert_eq!(vm.browser_tasks.len(), 1);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(reason) && vm.heap.contains(target));
    vm.run_browser_task_turn(1).unwrap();
    assert!(vm.browser_tasks.is_empty());
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(reason) && !vm.heap.contains(target));
}
#[cfg_attr(test, test)]
fn promise_reaction_storage_public_settled_species_and_catch_keep_callback_order() {
    let mut vm = page();
    execute(&mut vm, "globalThis.trace=[];class Child extends Promise{};globalThis.out=Child.resolve(3).then(v=>{trace.push('a'+v);return v+1});out.then(v=>trace.push('b'+v));globalThis.speciesOK=out instanceof Child;trace.push('sync');0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("sync,a3,b4".into())
    );
    assert_eq!(execute(&mut vm, "speciesOK"), Value::Bool(true));
    execute(&mut vm, "trace=[];Promise.reject('x').catch(r=>trace.push('r'+r));Promise.resolve('y').then(v=>trace.push('f'+v));trace.push('sync');0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("sync,rx,fy".into())
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_reaction_storage_contracts() {
        promise_reaction_storage_settled_payload_refusal_preserves_unhandled_state();
        promise_reaction_storage_pending_growth_refusal_preserves_unhandled_state();
        promise_reaction_storage_refusal_retains_pending_rejection_and_root();
        promise_reaction_storage_generic_pending_refusal_does_not_mark_handled();
        promise_reaction_storage_exact_settled_spare_slot_charges_only_its_copy();
        promise_reaction_storage_exact_rejection_copy_preserves_its_record_source();
        promise_reaction_storage_short_source_allowance_refuses_without_queue_mutation();
        promise_reaction_storage_exact_pending_registration_adopts_both_handlers();
        promise_reaction_storage_pending_spare_slot_accepts_zero_payload_at_full_allowance();
        promise_reaction_storage_pending_replacement_requires_complete_coexisting_backing();
        promise_reaction_storage_complete_job_replacement_refunds_old_backing();
        promise_reaction_storage_abandoned_preparation_refunds_without_handling_rejection();
        promise_reaction_storage_rust_held_preparation_keeps_its_lease_after_vm_drop();
        promise_reaction_storage_scalar_job_quota_preserves_notification_ownership();
        promise_reaction_storage_scalar_reaction_quota_preserves_pending_registration();
        promise_reaction_storage_late_notification_refusal_keeps_committed_job_and_bit();
        promise_reaction_storage_successful_registration_releases_pending_notification_root();
        promise_reaction_storage_successful_late_handling_publishes_followup_after_job();
        promise_reaction_storage_already_handled_sources_still_register_each_job();
        promise_reaction_storage_invalid_receiver_retains_brand_error_without_admission();
        promise_reaction_storage_published_job_roots_survive_notification_release_then_collect();
        promise_reaction_storage_public_settled_species_and_catch_keep_callback_order();
    }
}
