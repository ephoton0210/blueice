// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny Promise queues and scalar native owners verify complete admission.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn fixture() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn job() -> PromiseJob {
    PromiseJob::HostMicrotask {
        callback: Value::Undefined,
    }
}
fn reaction() -> PromiseReaction {
    PromiseReaction::AsyncAwait { continuation: 0 }
}
fn pending(vm: &mut Vm) -> ObjectId {
    execute(vm, "globalThis.input = new Promise(()=>{}); 0;");
    vm.lookup_global_name("input")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap()
}

#[cfg_attr(test, test)]
fn promise_queue_storage_jobs_refuse_a_net_capacity_only_allowance() {
    let mut vm = fixture();
    for _ in 0..4 {
        vm.enqueue_promise_job(job());
    }
    assert_eq!(vm.promise_jobs.len(), vm.promise_jobs.capacity());
    let _peer = leave_room(&mut vm, job_bytes(4));
    vm.enqueue_promise_job(job());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promise_jobs.len(), 4);
    assert_eq!(vm.promise_jobs.capacity(), 4);
}

#[cfg_attr(test, test)]
fn promise_queue_storage_reactions_refuse_a_net_capacity_only_allowance() {
    let mut vm = fixture();
    let input = pending(&mut vm);
    for _ in 0..4 {
        vm.enqueue_promise_reaction(input, reaction());
    }
    assert_eq!(vm.promises[&input].reactions.len(), 4);
    assert_eq!(vm.promises[&input].reactions.capacity(), 4);
    let _peer = leave_room(&mut vm, 4 * std::mem::size_of::<PromiseReaction>());
    vm.enqueue_promise_reaction(input, reaction());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promises[&input].reactions.len(), 4);
    assert_eq!(vm.promises[&input].reactions.capacity(), 4);
}

#[cfg_attr(test, test)]
fn promise_queue_storage_await_refusal_keeps_the_frame_serial_and_reaction_capacity() {
    let mut vm = fixture();
    execute(
        &mut vm,
        "globalThis.input=new Promise(()=>{}); globalThis.output=(async()=>await input)(); 0;",
    );
    let input = vm
        .lookup_global_name("input")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let id = *vm.async_continuations.keys().next().unwrap();
    let frame = vm.async_continuations.remove(&id).unwrap();
    vm.promises.get_mut(&input).unwrap().truncate_reactions(0);
    for _ in 0..4 {
        vm.enqueue_promise_reaction(input, reaction());
    }
    let serial = vm.next_async_continuation;
    let room = frame.scheduling_payload_bytes() + 4 * std::mem::size_of::<PromiseReaction>();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        vm.suspend_async_await(frame, input),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.next_async_continuation, serial);
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.promises[&input].reactions.len(), 4);
    assert_eq!(vm.promises[&input].reactions.capacity(), 4);
}

fn job_bytes(count: usize) -> usize {
    count * std::mem::size_of::<promise_job_storage::QueuedPromiseJob>()
}
fn reaction_slots(count: usize) -> usize {
    count * std::mem::size_of::<PromiseReaction>()
}
fn payload_job() -> PromiseJob {
    PromiseJob::AsyncAwait {
        continuation: 0,
        value: Value::String("tiny".into()),
        fulfilled: true,
        origin: ThrowOrigin::default(),
    }
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}

#[cfg_attr(test, test)]
fn promise_queue_storage_exact_initial_job_allows_slots_and_payload_once() {
    let mut vm = fixture();
    let _peer = leave_room(&mut vm, job_bytes(4) + 8);
    vm.enqueue_promise_job(payload_job());
    assert_eq!(vm.promise_jobs.len(), 1);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_queue_storage_job_one_byte_short_keeps_the_original_deque() {
    let mut vm = fixture();
    let _peer = leave_room(&mut vm, job_bytes(4) + 7);
    vm.enqueue_promise_job(payload_job());
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(vm.promise_jobs.capacity(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - job_bytes(4) - 7
    );
}
#[cfg_attr(test, test)]
fn promise_queue_storage_complete_job_replacement_refunds_old_slots() {
    let mut vm = fixture();
    for _ in 0..4 {
        vm.enqueue_promise_job(job());
    }
    let _peer = leave_room(&mut vm, job_bytes(8));
    vm.enqueue_promise_job(job());
    assert_eq!(vm.promise_jobs.len(), 5);
    assert_eq!(vm.promise_jobs.capacity(), 8);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - job_bytes(4)
    );
}
#[cfg_attr(test, test)]
fn promise_queue_storage_existing_job_slots_accept_zero_payload_at_full_allowance() {
    let mut vm = fixture();
    vm.enqueue_promise_job(job());
    let _peer = leave_room(&mut vm, 0);
    vm.enqueue_promise_job(job());
    assert_eq!(vm.promise_jobs.len(), 2);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_queue_storage_existing_job_slot_charges_only_new_payload() {
    let mut vm = fixture();
    vm.enqueue_promise_job(job());
    let _peer = leave_room(&mut vm, 8);
    vm.enqueue_promise_job(payload_job());
    assert_eq!(vm.promise_jobs.len(), 2);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn promise_queue_storage_payload_refusal_preserves_the_existing_jobs() {
    let mut vm = fixture();
    vm.enqueue_promise_job(job());
    let _peer = leave_room(&mut vm, 7);
    vm.enqueue_promise_job(payload_job());
    assert_eq!(vm.promise_jobs.len(), 1);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert!(matches!(
        vm.promise_jobs.front(),
        Some(PromiseJob::HostMicrotask {
            callback: Value::Undefined
        })
    ));
}
#[cfg_attr(test, test)]
fn promise_queue_storage_wrapped_job_replacement_preserves_fifo_order() {
    let mut vm = fixture();
    let item = |number| PromiseJob::AsyncAwait {
        continuation: 0,
        value: Value::Number(number),
        fulfilled: true,
        origin: ThrowOrigin::default(),
    };
    for number in 0..4 {
        vm.enqueue_promise_job(item(f64::from(number)));
    }
    vm.promise_jobs.pop_front();
    vm.promise_jobs.pop_front();
    for number in 4..7 {
        vm.enqueue_promise_job(item(f64::from(number)));
    }
    let values: Vec<_> = vm
        .promise_jobs
        .iter()
        .map(|job| match job {
            PromiseJob::AsyncAwait { value, .. } => value.clone(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(values, [2.0, 3.0, 4.0, 5.0, 6.0].map(Value::Number));
    assert_eq!(vm.promise_jobs.capacity(), 8);
}
#[cfg_attr(test, test)]
fn promise_queue_storage_complete_reaction_replacement_refunds_old_slots() {
    let mut vm = fixture();
    let input = pending(&mut vm);
    for _ in 0..4 {
        vm.enqueue_promise_reaction(input, reaction());
    }
    let _peer = leave_room(&mut vm, reaction_slots(8));
    vm.enqueue_promise_reaction(input, reaction());
    assert_eq!(vm.promises[&input].reactions.len(), 5);
    assert_eq!(vm.promises[&input].reactions.capacity(), 8);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - reaction_slots(4)
    );
}
#[cfg_attr(test, test)]
fn promise_queue_storage_reaction_spare_slots_need_no_growth_charge() {
    let mut vm = fixture();
    let input = pending(&mut vm);
    vm.enqueue_promise_reaction(input, reaction());
    let _peer = leave_room(&mut vm, 0);
    vm.enqueue_promise_reaction(input, reaction());
    assert_eq!(vm.promises[&input].reactions.len(), 2);
    assert_eq!(vm.promises[&input].reactions.capacity(), 4);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn promise_queue_storage_first_reaction_one_byte_short_publishes_no_slots() {
    let mut vm = fixture();
    let input = pending(&mut vm);
    let _peer = leave_room(&mut vm, reaction_slots(4) - 1);
    vm.enqueue_promise_reaction(input, reaction());
    assert!(vm.promises[&input].reactions.is_empty());
    assert_eq!(vm.promises[&input].reactions.capacity(), 0);
}
#[cfg_attr(test, test)]
fn promise_queue_storage_partial_reaction_removal_retains_its_actual_capacity_charge() {
    let mut vm = fixture();
    let input = pending(&mut vm);
    for _ in 0..2 {
        vm.enqueue_promise_reaction(input, reaction());
    }
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.promises.get_mut(&input).unwrap().truncate_reactions(1);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    vm.promises.get_mut(&input).unwrap().release_reactions();
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - reaction_slots(4)
    );
}
#[cfg_attr(test, test)]
fn promise_queue_storage_rust_held_deque_candidate_outlives_its_vm_owner() {
    let mut vm = fixture();
    for _ in 0..4 {
        vm.enqueue_promise_job(job());
    }
    let observed = observer(&vm);
    let prepared =
        PreparedDeque::for_additional(&vm.promise_jobs, 1, vm.host_scheduling.as_ref()).unwrap();
    drop(vm);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        job_bytes(8)
    );
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_queue_storage_rust_held_map_candidate_outlives_its_vm_owner() {
    let vm = fixture();
    let values = HashMap::from([(0u64, 0u64), (1, 1), (2, 2)]);
    assert_eq!(values.len(), values.capacity());
    let observed = observer(&vm);
    let prepared = PreparedMap::for_insert(&values, vm.host_scheduling.as_ref()).unwrap();
    let bytes = host_scheduling::map_slots(prepared.replacement.as_ref().unwrap());
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_queue_storage_queued_callback_is_traced_then_collectible_after_delivery() {
    let mut vm = fixture();
    let callback = execute(&mut vm, "globalThis.queued=(()=>{let held={marker:42}; return ()=>{globalThis.seen=held.marker;};})(); queued;");
    let object = callback.object_id().unwrap();
    vm.enqueue_promise_job(PromiseJob::HostMicrotask { callback });
    execute(&mut vm, "queued=undefined;0;");
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(object));
    assert_eq!(vm.run_next_promise_job(), Ok(true));
    assert_eq!(
        vm.lookup_global_name("seen").unwrap(),
        Some(Value::Number(42.0))
    );
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}
#[cfg_attr(test, test)]
fn promise_queue_storage_pending_reaction_targets_follow_their_owner_through_gc() {
    let mut vm = fixture();
    let input = pending(&mut vm);
    let target = vm.new_promise().unwrap();
    vm.enqueue_promise_reaction(
        input,
        PromiseReaction::Then(PromiseThenReaction {
            target: ReactionTarget::Native(target),
            on_fulfilled: Value::Undefined,
            on_rejected: Value::Undefined,
        }),
    );
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(target));
    execute(&mut vm, "input=undefined;0;");
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(input));
    assert!(!vm.heap.contains(target));
}

enum Frame {
    Async(Box<AsyncContinuation>),
    Module(Box<ModuleContinuation>),
}
struct AwaitFixture {
    vm: Vm,
    input: ObjectId,
    frame: Option<Frame>,
    asynchronous: bool,
}
impl AwaitFixture {
    fn new(asynchronous: bool, retained: usize, settled: Option<bool>) -> Self {
        let mut vm = fixture();
        let input = pending(&mut vm);
        if asynchronous {
            execute(&mut vm, &format!("globalThis.outputs=[]; for(var i=0;i<{};i++) outputs.push((async()=>await input)());0;", retained + 1));
        } else {
            let mut registry = HashMap::new();
            for index in 0..=retained {
                registry.insert(
                    format!("m{index}.mjs"),
                    crate::compile_module(
                        &crate::parse_module("await input;export const value=1;").unwrap(),
                    )
                    .unwrap(),
                );
            }
            for index in 0..=retained {
                vm.execute_module_graph(&format!("m{index}.mjs"), &registry)
                    .unwrap();
            }
        }
        let (id, frame) = if asynchronous {
            let id = *vm.async_continuations.keys().max().unwrap();
            let frame = vm.async_continuations.remove(&id).unwrap();
            vm.async_continuations.shrink_to_fit();
            (id, Frame::Async(Box::new(frame)))
        } else {
            let id = *vm.module_continuations.keys().max().unwrap();
            let frame = vm.module_continuations.remove(&id).unwrap();
            vm.module_continuations.shrink_to_fit();
            (id, Frame::Module(Box::new(frame)))
        };
        vm.promises
            .get_mut(&input)
            .unwrap()
            .retain_reactions(|reaction| match reaction {
                PromiseReaction::AsyncAwait { continuation } => {
                    !asynchronous || *continuation != id
                }
                PromiseReaction::ModuleAwait { continuation } => {
                    asynchronous || *continuation != id
                }
                _ => true,
            });
        if let Some(fulfilled) = settled {
            vm.settle_promise(
                input,
                if fulfilled {
                    PromiseStatus::Fulfilled(Value::String("tiny".into()))
                } else {
                    PromiseStatus::Rejected(Value::String("tiny".into()))
                },
            )
            .unwrap();
        }
        Self {
            vm,
            input,
            frame: Some(frame),
            asynchronous,
        }
    }
    fn frame_bytes(&self) -> usize {
        match self.frame.as_ref().unwrap() {
            Frame::Async(frame) => frame.scheduling_payload_bytes(),
            Frame::Module(frame) => frame.scheduling_payload_bytes(),
        }
    }
    fn table(&self) -> (usize, usize, u64) {
        if self.asynchronous {
            (
                self.vm.async_continuations.len(),
                self.vm.async_continuations.capacity(),
                self.vm.next_async_continuation,
            )
        } else {
            (
                self.vm.module_continuations.len(),
                self.vm.module_continuations.capacity(),
                self.vm.next_module_continuation,
            )
        }
    }
    fn candidate_map_bytes(&self) -> usize {
        if self.asynchronous {
            host_scheduling::map_slots(&HashMap::<u64, AsyncContinuation>::with_capacity(
                self.table().0 + 1,
            ))
        } else {
            host_scheduling::map_slots(&HashMap::<u64, ModuleContinuation>::with_capacity(
                self.table().0 + 1,
            ))
        }
    }
    fn minimum_map_bytes(&self) -> usize {
        if self.asynchronous {
            std::mem::size_of::<(u64, AsyncContinuation)>()
        } else {
            std::mem::size_of::<(u64, ModuleContinuation)>()
        }
    }
    fn fill_queue(&mut self, ready: bool) {
        if ready {
            for _ in 0..4 {
                self.vm.enqueue_promise_job(job());
            }
        } else {
            let additional = 4usize.saturating_sub(self.vm.promises[&self.input].reactions.len());
            for _ in 0..additional {
                self.vm.enqueue_promise_reaction(self.input, reaction());
            }
        }
    }
    fn register(&mut self) -> Result<(), RuntimeError> {
        match self.frame.take().unwrap() {
            Frame::Async(frame) => self.vm.suspend_async_await(*frame, self.input),
            Frame::Module(frame) => self.vm.suspend_module_await(*frame, self.input),
        }
    }
    fn assert_refusal(&mut self) {
        let table = self.table();
        let queue = (
            self.vm.promise_jobs.len(),
            self.vm.promise_jobs.capacity(),
            self.vm.promises[&self.input].reactions.len(),
            self.vm.promises[&self.input].reactions.capacity(),
        );
        assert_eq!(self.register(), Err(RuntimeError::SchedulingLimit));
        assert_eq!(self.table(), table);
        assert_eq!(
            (
                self.vm.promise_jobs.len(),
                self.vm.promise_jobs.capacity(),
                self.vm.promises[&self.input].reactions.len(),
                self.vm.promises[&self.input].reactions.capacity()
            ),
            queue
        );
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_await_map_refusal_refunds_a_prepared_queue_without_mutation() {
    for asynchronous in [false, true] {
        for ready in [false, true] {
            let mut f = AwaitFixture::new(asynchronous, 0, ready.then_some(true));
            f.fill_queue(ready);
            let bytes = f.frame_bytes()
                + if ready {
                    8 + job_bytes(8)
                } else {
                    reaction_slots(8)
                };
            let _peer = leave_room(&mut f.vm, bytes);
            f.assert_refusal();
        }
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_await_reported_map_capacity_refusal_preserves_both_owners() {
    for asynchronous in [false, true] {
        let mut f = AwaitFixture::new(asynchronous, 0, Some(false));
        f.fill_queue(true);
        let bytes = f.frame_bytes() + 8 + job_bytes(8) + f.minimum_map_bytes();
        let _peer = leave_room(&mut f.vm, bytes);
        f.assert_refusal();
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_exact_joint_await_admission_refunds_old_queue_storage() {
    for asynchronous in [false, true] {
        for settled in [None, Some(true), Some(false)] {
            let ready = settled.is_some();
            let mut f = AwaitFixture::new(asynchronous, 0, settled);
            f.fill_queue(ready);
            let bytes = f.frame_bytes()
                + f.candidate_map_bytes()
                + if ready {
                    8 + job_bytes(8)
                } else {
                    reaction_slots(8)
                };
            let _peer = leave_room(&mut f.vm, bytes);
            f.register().unwrap();
            assert_eq!(f.table().0, 1);
            assert_eq!(
                f.vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
                    - if ready {
                        job_bytes(4)
                    } else {
                        reaction_slots(4)
                    }
            );
            assert!(f.vm.check_scheduling_failure().is_ok());
        }
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_await_map_growth_requires_the_complete_candidate() {
    for asynchronous in [false, true] {
        let mut f = AwaitFixture::new(asynchronous, 7, None);
        assert_eq!(f.table().0, f.table().1);
        let old = if asynchronous {
            host_scheduling::map_slots(&f.vm.async_continuations)
        } else {
            host_scheduling::map_slots(&f.vm.module_continuations)
        };
        let bytes = f.frame_bytes() + f.candidate_map_bytes() - old;
        let _peer = leave_room(&mut f.vm, bytes);
        f.assert_refusal();
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_existing_await_map_and_queue_slots_need_only_new_payload() {
    for asynchronous in [false, true] {
        let mut f = AwaitFixture::new(asynchronous, 0, Some(true));
        if asynchronous {
            f.vm.async_continuations.reserve(1);
        } else {
            f.vm.module_continuations.reserve(1);
        }
        f.vm.promise_jobs.reserve_exact(4);
        let bytes = f.frame_bytes() + 8;
        let _peer = leave_room(&mut f.vm, bytes);
        f.register().unwrap();
        assert_eq!(f.table().0, 1);
        assert_eq!(f.vm.promise_jobs.len(), 1);
        assert!(f.vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_await_primitive_copy_refuses_before_any_map_or_queue_growth() {
    for asynchronous in [false, true] {
        let mut f = AwaitFixture::new(asynchronous, 0, Some(false));
        let _peer = leave_room(&mut f.vm, 7);
        f.assert_refusal();
        assert_eq!(f.table().1, 0);
        assert_eq!(f.vm.promise_jobs.capacity(), 0);
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_await_serial_boundary_precedes_native_copy_admission() {
    for asynchronous in [false, true] {
        let mut f = AwaitFixture::new(asynchronous, 0, Some(true));
        if asynchronous {
            f.vm.next_async_continuation = u64::MAX;
        } else {
            f.vm.next_module_continuation = u64::MAX;
        }
        let _peer = leave_room(&mut f.vm, 0);
        assert_eq!(f.register(), Err(RuntimeError::InstructionLimit));
        assert_eq!(f.table().0, 0);
        assert_eq!(f.table().1, 0);
        assert_eq!(f.vm.promise_jobs.capacity(), 0);
        assert!(f.vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_terminal_queue_cleanup_refunds_its_committed_storage() {
    let mut vm = fixture();
    vm.enqueue_promise_job(payload_job());
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    vm.promise_jobs = Default::default();
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - job_bytes(4) - 8
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn promise_queue_storage_scalar_job_and_reaction_count_refusal_precedes_growth() {
    for reactions in [false, true] {
        let mut vm = fixture();
        let input = pending(&mut vm);
        let mut peer = vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            if reactions {
                HostSchedulingUsage {
                    pending_reactions: MAX_PENDING_HOST_REACTIONS,
                    ..Default::default()
                }
            } else {
                HostSchedulingUsage {
                    pending_microtasks: MAX_PENDING_HOST_MICROTASKS,
                    ..Default::default()
                }
            },
            Default::default(),
        )
        .unwrap();
        if reactions {
            vm.enqueue_promise_reaction(input, reaction());
        } else {
            vm.enqueue_promise_job(job());
        }
        assert_eq!(
            vm.check_scheduling_failure(),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(vm.promise_jobs.capacity(), 0);
        assert_eq!(vm.promises[&input].reactions.capacity(), 0);
    }
}
#[cfg_attr(test, test)]
fn promise_queue_storage_refused_await_reconciles_dropped_frame_leases_for_other_realms() {
    for asynchronous in [false, true] {
        let mut f = AwaitFixture::new(asynchronous, 0, None);
        let observed = observer(&f.vm);
        let _peer = leave_room(&mut f.vm, 0);
        f.assert_refusal();
        assert_eq!(
            observed.host_scheduling_snapshot().payload_bytes,
            f.vm.host_scheduling_snapshot().payload_bytes
        );
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_queue_storage_contracts() {
        promise_queue_storage_jobs_refuse_a_net_capacity_only_allowance();
        promise_queue_storage_reactions_refuse_a_net_capacity_only_allowance();
        promise_queue_storage_await_refusal_keeps_the_frame_serial_and_reaction_capacity();
        promise_queue_storage_exact_initial_job_allows_slots_and_payload_once();
        promise_queue_storage_job_one_byte_short_keeps_the_original_deque();
        promise_queue_storage_complete_job_replacement_refunds_old_slots();
        promise_queue_storage_existing_job_slots_accept_zero_payload_at_full_allowance();
        promise_queue_storage_existing_job_slot_charges_only_new_payload();
        promise_queue_storage_payload_refusal_preserves_the_existing_jobs();
        promise_queue_storage_wrapped_job_replacement_preserves_fifo_order();
        promise_queue_storage_complete_reaction_replacement_refunds_old_slots();
        promise_queue_storage_reaction_spare_slots_need_no_growth_charge();
        promise_queue_storage_first_reaction_one_byte_short_publishes_no_slots();
        promise_queue_storage_partial_reaction_removal_retains_its_actual_capacity_charge();
        promise_queue_storage_rust_held_deque_candidate_outlives_its_vm_owner();
        promise_queue_storage_rust_held_map_candidate_outlives_its_vm_owner();
        promise_queue_storage_queued_callback_is_traced_then_collectible_after_delivery();
        promise_queue_storage_pending_reaction_targets_follow_their_owner_through_gc();
        promise_queue_storage_await_map_refusal_refunds_a_prepared_queue_without_mutation();
        promise_queue_storage_await_reported_map_capacity_refusal_preserves_both_owners();
        promise_queue_storage_exact_joint_await_admission_refunds_old_queue_storage();
        promise_queue_storage_await_map_growth_requires_the_complete_candidate();
        promise_queue_storage_existing_await_map_and_queue_slots_need_only_new_payload();
        promise_queue_storage_await_primitive_copy_refuses_before_any_map_or_queue_growth();
        promise_queue_storage_await_serial_boundary_precedes_native_copy_admission();
        promise_queue_storage_terminal_queue_cleanup_refunds_its_committed_storage();
        promise_queue_storage_scalar_job_and_reaction_count_refusal_precedes_growth();
        promise_queue_storage_refused_await_reconciles_dropped_frame_leases_for_other_realms();
    }
}
