// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny Promise fan-outs and scalar peers verify native settlement ownership.

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
fn unchanged(vm: &Vm, promise: ObjectId, reactions: usize, capacity: usize) {
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert_eq!(vm.promises[&promise].reactions.len(), reactions);
    assert_eq!(vm.promises[&promise].reactions.capacity(), capacity);
    assert!(vm.promise_jobs.is_empty());
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

#[cfg_attr(test, test)]
fn promise_settlement_payload_refusal_keeps_the_pending_record() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let _peer = leave_room(&mut vm, 0);
    vm.settle_promise(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    unchanged(&vm, promise, 0, 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn promise_settlement_complete_fanout_refusal_keeps_all_reactions() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    for continuation in 0..2 {
        vm.enqueue_promise_reaction(promise, PromiseReaction::AsyncAwait { continuation });
    }
    let capacity = vm.promises[&promise].reactions.capacity();
    let _peer = leave_room(&mut vm, 4 * std::mem::size_of::<PromiseJob>());
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)))
        .unwrap();
    unchanged(&vm, promise, 2, capacity);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn promise_settlement_origin_refusal_does_not_publish_rejection() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.promise_origin_override = Some(origin());
    let _peer = leave_room(&mut vm, 0);
    vm.settle_promise(promise, PromiseStatus::Rejected(Value::Number(7.0)))
        .unwrap();
    unchanged(&vm, promise, 0, 0);
    assert!(vm.promises[&promise].rejection_origin.source.is_none());
    assert_eq!(vm.promise_origin_override.as_ref().unwrap().epoch, 7);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn fanout(count: usize) -> (Vm, ObjectId) {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    for continuation in 0..count as u64 {
        vm.enqueue_promise_reaction(promise, PromiseReaction::AsyncAwait { continuation });
    }
    (vm, promise)
}
fn rejected() -> PromiseStatus {
    PromiseStatus::Rejected(Value::String("tiny".into()))
}
fn preview(vm: &mut Vm, promise: ObjectId, status: PromiseStatus) -> usize {
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm
        .prepare_promise_settlement(promise, status, None)
        .unwrap()
        .unwrap();
    let extra = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(prepared);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    extra
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn promise_settlement_exact_value_only_admission_has_no_redundant_copy() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    let _peer = leave_room(&mut vm, 8);
    vm.settle_promise(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
    )
    .unwrap();
    assert!(
        matches!(&vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::String(text)) if text.to_utf8().unwrap() == "tiny")
    );
    assert_eq!(vm.promise_jobs.capacity(), 0);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_settlement_zero_payload_fulfillment_ignores_rejection_override() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.promise_origin_override = Some(origin());
    vm.remaining_instructions = 0;
    let _peer = leave_room(&mut vm, 0);
    vm.settle_tracked_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)));
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
    assert!(vm.promises[&promise].rejection_origin.source.is_none());
    assert_eq!(vm.promise_origin_override.as_ref().unwrap().epoch, 7);
    assert_eq!(vm.remaining_instructions, 0);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_settlement_exact_batch_admission_refunds_all_staging() {
    let (mut vm, promise) = fanout(2);
    vm.promise_origin_override = Some(origin());
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let before_owned = vm.own_scheduling_usage().payload_bytes;
    let extra = preview(&mut vm, promise, rejected());
    let view = observer(&vm);
    let peer = leave_room(&mut vm, extra);
    vm.settle_promise(promise, rejected()).unwrap();
    assert_eq!(vm.promise_jobs.len(), 2);
    assert_eq!(vm.promises[&promise].reactions.capacity(), 0);
    assert_eq!(vm.promises[&promise].rejection_origin.epoch, 7);
    let retained = vm.own_scheduling_usage().payload_bytes;
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - extra - before_owned + retained
    );
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - extra - before
    );
    drop(peer);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_settlement_one_byte_short_final_queue_refunds_prepared_copies() {
    let (mut vm, promise) = fanout(2);
    vm.promise_origin_override = Some(origin());
    let extra = preview(&mut vm, promise, rejected());
    let capacity = vm.promises[&promise].reactions.capacity();
    let _peer = leave_room(&mut vm, extra - 1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.settle_promise(promise, rejected()).unwrap();
    unchanged(&vm, promise, 2, capacity);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_settlement_preleased_value_transfers_at_a_full_document_allowance() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let input = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(8)
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    vm.settle_promise_with_input_charge(
        promise,
        PromiseStatus::Fulfilled(Value::String("tiny".into())),
        Some(input),
    )
    .unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(_)
    ));
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_settlement_foreign_document_lease_refuses_without_publication() {
    let mut vm = page();
    let mut other = page();
    let promise = vm.new_promise().unwrap();
    let before = other.host_scheduling_usage().unwrap().payload_bytes;
    let input = other
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(8)
        .unwrap();
    vm.settle_promise_with_input_charge(promise, rejected(), Some(input))
        .unwrap();
    unchanged(&vm, promise, 0, 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(other.host_scheduling_usage().unwrap().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn promise_settlement_abandoned_preparation_changes_no_live_owner() {
    let (mut vm, promise) = fanout(2);
    vm.promise_origin_override = Some(origin());
    let roots = vm.heap.root_storage_state();
    let capacity = vm.promises[&promise].reactions.capacity();
    let before = vm.host_scheduling_usage().unwrap();
    let prepared = vm
        .prepare_promise_settlement(promise, rejected(), None)
        .unwrap()
        .unwrap();
    unchanged(&vm, promise, 2, capacity);
    drop(prepared);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_scheduling_usage().unwrap(), before);
}
#[cfg_attr(test, test)]
fn promise_settlement_rust_held_batch_keeps_its_native_lease_after_vm_drop() {
    let (mut vm, promise) = fanout(2);
    vm.promise_origin_override = Some(origin());
    let view = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm
        .prepare_promise_settlement(promise, rejected(), None)
        .unwrap()
        .unwrap();
    let extra = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, extra);
    assert_eq!(prepared.jobs.len(), 2);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_settlement_spare_queue_slots_need_only_the_native_snapshot() {
    let (mut vm, promise) = fanout(2);
    vm.enqueue_promise_job(PromiseJob::HostMicrotask {
        callback: Value::Undefined,
    });
    assert_eq!(vm.promise_jobs.capacity(), 4);
    let extra = preview(
        &mut vm,
        promise,
        PromiseStatus::Fulfilled(Value::Number(7.0)),
    );
    assert_eq!(extra, 2 * std::mem::size_of::<PromiseJob>());
    let _peer = leave_room(&mut vm, extra);
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)))
        .unwrap();
    assert_eq!(vm.promise_jobs.len(), 3);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_settlement_wrapped_queue_replacement_keeps_fifo_order() {
    let (mut vm, promise) = fanout(2);
    for continuation in 2..6 {
        vm.enqueue_promise_job(PromiseJob::AsyncAwait {
            continuation,
            value: Value::Undefined,
            fulfilled: true,
            origin: ThrowOrigin::default(),
        });
    }
    for continuation in 6..8 {
        vm.promise_jobs.pop_front();
        vm.enqueue_promise_job(PromiseJob::AsyncAwait {
            continuation,
            value: Value::Undefined,
            fulfilled: true,
            origin: ThrowOrigin::default(),
        });
    }
    assert_eq!(vm.promise_jobs.capacity(), 4);
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)))
        .unwrap();
    let order: Vec<_> = vm
        .promise_jobs
        .iter()
        .map(|job| match job {
            PromiseJob::AsyncAwait { continuation, .. } => *continuation,
            _ => panic!("expected await job"),
        })
        .collect();
    assert_eq!(order, [4, 5, 6, 7, 0, 1]);
}
#[cfg_attr(test, test)]
fn promise_settlement_scalar_job_quota_admits_the_whole_batch_or_none() {
    for spare in [1, 2] {
        let (mut vm, promise) = fanout(2);
        let capacity = vm.promises[&promise].reactions.capacity();
        let mut peer = vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            HostSchedulingUsage {
                pending_microtasks: MAX_PENDING_HOST_MICROTASKS - spare,
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap();
        vm.settle_tracked_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)));
        if spare == 1 {
            unchanged(&vm, promise, 2, capacity);
            assert_eq!(
                vm.check_scheduling_failure(),
                Err(RuntimeError::SchedulingLimit)
            );
        } else {
            assert_eq!(vm.promise_jobs.len(), 2);
            assert!(vm.check_scheduling_failure().is_ok());
        }
    }
}
#[cfg_attr(test, test)]
fn promise_settlement_repeated_settlement_is_a_noop_at_full_allowance() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.settle_tracked_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)));
    vm.promise_origin_override = Some(origin());
    let _peer = leave_room(&mut vm, 0);
    vm.settle_promise(promise, rejected()).unwrap();
    vm.settle_promise(promise, PromiseStatus::Pending).unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
    assert!(vm.promises[&promise].rejection_origin.source.is_none());
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_settlement_invalid_receiver_and_pending_status_keep_type_errors() {
    let (mut vm, promise) = fanout(2);
    let invalid = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    assert!(matches!(
        vm.settle_promise(invalid, rejected()),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        vm.settle_promise(promise, PromiseStatus::Pending),
        Err(RuntimeError::TypeError(_))
    ));
    unchanged(&vm, promise, 2, vm.promises[&promise].reactions.capacity());
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_settlement_all_reaction_variants_preserve_values_and_origins() {
    for fulfilled in [false, true] {
        let (mut vm, promise) = fanout(0);
        let target = vm.new_promise().unwrap();
        for reaction in [
            PromiseReaction::Then(PromiseThenReaction {
                target: ReactionTarget::Native(target),
                on_fulfilled: Value::String("yes".into()),
                on_rejected: Value::String("no".into()),
            }),
            PromiseReaction::ModuleAwait { continuation: 3 },
            PromiseReaction::AsyncAwait { continuation: 5 },
            PromiseReaction::AsyncGeneratorYield {
                generator: target,
                target,
                result: target,
            },
            PromiseReaction::AsyncGeneratorDelegate {
                generator: target,
                target,
                kind: AsyncGeneratorDelegateKind::Return,
            },
        ] {
            vm.enqueue_promise_reaction(promise, reaction);
        }
        vm.promise_origin_override = Some(origin());
        let value = Value::String("tiny".into());
        vm.settle_promise(
            promise,
            if fulfilled {
                PromiseStatus::Fulfilled(value)
            } else {
                PromiseStatus::Rejected(value)
            },
        )
        .unwrap();
        assert_eq!(vm.promise_jobs.len(), 5);
        assert_eq!(vm.promise_jobs.capacity(), 8);
        assert!(
            matches!(&vm.promise_jobs[0], PromiseJob::Reaction { handler: Value::String(text), .. } if text.to_utf8().unwrap() == if fulfilled { "yes" } else { "no" })
        );
        assert!(matches!(
            vm.promise_jobs[1],
            PromiseJob::ModuleAwait {
                continuation: 3,
                ..
            }
        ));
        assert!(matches!(
            vm.promise_jobs[2],
            PromiseJob::AsyncAwait {
                continuation: 5,
                ..
            }
        ));
        assert!(matches!(
            vm.promise_jobs[3],
            PromiseJob::AsyncGeneratorYield { .. }
        ));
        assert!(matches!(
            vm.promise_jobs[4],
            PromiseJob::AsyncGeneratorDelegate {
                kind: AsyncGeneratorDelegateKind::Return,
                ..
            }
        ));
        for job in &vm.promise_jobs {
            let (value, was_fulfilled) = match job {
                PromiseJob::Reaction {
                    value, fulfilled, ..
                }
                | PromiseJob::ModuleAwait {
                    value, fulfilled, ..
                }
                | PromiseJob::AsyncAwait {
                    value, fulfilled, ..
                }
                | PromiseJob::AsyncGeneratorYield {
                    value, fulfilled, ..
                }
                | PromiseJob::AsyncGeneratorDelegate {
                    value, fulfilled, ..
                } => (value, *fulfilled),
                _ => panic!("expected settlement job"),
            };
            assert_eq!(value, &Value::String("tiny".into()));
            assert_eq!(was_fulfilled, fulfilled);
            assert_eq!(
                job.rejection_origin().unwrap().epoch,
                if fulfilled { 0 } else { 7 }
            );
        }
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn promise_settlement_native_frame_source_and_unknown_override_keep_precedence() {
    for unknown in [false, true] {
        let mut vm = page();
        let promise = vm.new_promise().unwrap();
        vm.active_diagnostic_frame = Some(crate::diagnostics::ActiveDiagnosticFrame::Native(
            origin().source,
        ));
        if unknown {
            vm.promise_origin_override = Some(ThrowOrigin::default());
        }
        let extra = preview(
            &mut vm,
            promise,
            PromiseStatus::Rejected(Value::Number(7.0)),
        );
        assert_eq!(extra, if unknown { 0 } else { "tiny.js".len() });
        let _peer = leave_room(&mut vm, extra);
        vm.settle_promise(promise, PromiseStatus::Rejected(Value::Number(7.0)))
            .unwrap();
        assert_eq!(
            vm.promises[&promise].rejection_origin.source.is_none(),
            unknown
        );
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn promise_settlement_published_jobs_keep_targets_and_value_alive_until_delivery() {
    let mut vm = page();
    execute(&mut vm, "globalThis.input=new Promise(()=>{});globalThis.result=0;input.then(v=>{result=v.answer});0;");
    let promise = vm
        .lookup_global_name("input")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let value = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.set(value, "answer", Value::Number(42.0)).unwrap();
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Object(value)))
        .unwrap();
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap.set(global, "input", Value::Null).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(value));
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(execute(&mut vm, "result"), Value::Number(42.0));
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(value));
}
#[cfg_attr(test, test)]
fn promise_settlement_tracked_refusal_never_panics_or_consumes_ambient_fuel() {
    let (mut vm, promise) = fanout(2);
    let capacity = vm.promises[&promise].reactions.capacity();
    vm.remaining_instructions = 0;
    let _peer = leave_room(&mut vm, 0);
    vm.settle_tracked_promise(promise, rejected());
    unchanged(&vm, promise, 2, capacity);
    assert_eq!(vm.remaining_instructions, 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_settlement_rejection_notification_root_refusal_is_a_committed_attempt() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.host_rejections = Some(host_rejections::HostRejections::default());
    vm.heap.allow_root_registrations(0);
    vm.settle_tracked_promise(promise, PromiseStatus::Rejected(Value::Number(7.0)));
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Rejected(Value::Number(7.0))
    ));
    assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_settlement_public_fanout_callbacks_and_async_await_keep_fifo_order() {
    let mut vm = page();
    execute(&mut vm, "globalThis.trace=[];globalThis.resolve=null;const input=new Promise(r=>{resolve=r});input.then(v=>trace.push('a'+v));input.then(v=>trace.push('b'+v));(async()=>{trace.push('wait');trace.push('c'+await input)})();resolve('x');trace.push('sync');0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("wait,sync,ax,bx,cx".into())
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_settlement_contracts() {
        promise_settlement_payload_refusal_keeps_the_pending_record();
        promise_settlement_complete_fanout_refusal_keeps_all_reactions();
        promise_settlement_origin_refusal_does_not_publish_rejection();
        promise_settlement_exact_value_only_admission_has_no_redundant_copy();
        promise_settlement_zero_payload_fulfillment_ignores_rejection_override();
        promise_settlement_exact_batch_admission_refunds_all_staging();
        promise_settlement_one_byte_short_final_queue_refunds_prepared_copies();
        promise_settlement_preleased_value_transfers_at_a_full_document_allowance();
        promise_settlement_foreign_document_lease_refuses_without_publication();
        promise_settlement_abandoned_preparation_changes_no_live_owner();
        promise_settlement_rust_held_batch_keeps_its_native_lease_after_vm_drop();
        promise_settlement_spare_queue_slots_need_only_the_native_snapshot();
        promise_settlement_wrapped_queue_replacement_keeps_fifo_order();
        promise_settlement_scalar_job_quota_admits_the_whole_batch_or_none();
        promise_settlement_repeated_settlement_is_a_noop_at_full_allowance();
        promise_settlement_invalid_receiver_and_pending_status_keep_type_errors();
        promise_settlement_all_reaction_variants_preserve_values_and_origins();
        promise_settlement_native_frame_source_and_unknown_override_keep_precedence();
        promise_settlement_published_jobs_keep_targets_and_value_alive_until_delivery();
        promise_settlement_tracked_refusal_never_panics_or_consumes_ambient_fuel();
        promise_settlement_rejection_notification_root_refusal_is_a_committed_attempt();
        promise_settlement_public_fanout_callbacks_and_async_await_keep_fifo_order();
    }
}
