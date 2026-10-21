// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Capacity admission with tiny containers and an abstract shared owner.

use super::*;

fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
}

fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    // Model another document-owned allocation numerically. No corresponding
    // large buffer or large task queue is allocated by this fixture.
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

#[cfg_attr(test, test)]
fn scheduling_capacity_refuses_timer_growth_before_ids_or_entries_change() {
    let mut vm = page_vm();
    vm.install_host_timer_functions().unwrap();
    execute(
        &mut vm,
        "function cb() {} for (var i=0;i<8;i++) setTimeout(cb,100); 0;",
    )
    .unwrap();
    let capacity = vm.host_timers.capacity();
    let callback = vm.lookup_global_name("cb").unwrap().unwrap();
    assert_eq!(capacity, vm.host_timers.len());
    let next_id = vm.host_timer_next_id;
    let _owner = leave_room(&mut vm, capacity * std::mem::size_of::<HostTimer>() - 1);
    assert_eq!(
        vm.host_timer_call(
            host_timers::HostTimerFunction::SetTimeout,
            &[callback, Value::Number(100.0)],
            false,
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_timers.len(), 8);
    assert_eq!(vm.host_timers.capacity(), capacity);
    assert_eq!(vm.host_timer_next_id, next_id);
    assert_eq!(vm.host_scheduling_snapshot().pending_timers, 8);
}

#[cfg_attr(test, test)]
fn scheduling_capacity_uses_existing_timer_slots_without_a_growth_charge() {
    let mut vm = page_vm();
    vm.install_host_timer_functions().unwrap();
    execute(
        &mut vm,
        "function cb() {} for (var i=0;i<5;i++) setTimeout(cb,100); 0;",
    )
    .unwrap();
    let capacity = vm.host_timers.capacity();
    let callback = vm.lookup_global_name("cb").unwrap().unwrap();
    assert!(vm.host_timers.len() < capacity);
    let _owner = leave_room(&mut vm, std::mem::size_of::<RootId>());
    assert_eq!(
        vm.host_timer_call(
            host_timers::HostTimerFunction::SetTimeout,
            &[callback, Value::Number(100.0)],
            false,
        ),
        Ok(Value::Number(6.0))
    );
    assert_eq!(vm.host_timers.capacity(), capacity);
    assert_eq!(vm.host_timers.len(), 6);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn scheduling_capacity_refuses_frame_growth_before_handles_or_entries_change() {
    let mut vm = page_vm();
    vm.install_host_animation_frame_functions().unwrap();
    execute(
        &mut vm,
        "function cb() {} for (var i=0;i<8;i++) requestAnimationFrame(cb); 0;",
    )
    .unwrap();
    let capacity = vm.host_animation_frames.capacity();
    let callback = vm.lookup_global_name("cb").unwrap().unwrap();
    assert_eq!(capacity, vm.host_animation_frames.len());
    let next_id = vm.host_animation_frame_next_id;
    let _owner = leave_room(
        &mut vm,
        capacity * std::mem::size_of::<HostAnimationFrame>() - 1,
    );
    assert_eq!(
        vm.host_animation_frame_call(
            host_animation_frames::HostAnimationFrameFunction::Request,
            &[callback],
            false,
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_animation_frames.len(), 8);
    assert_eq!(vm.host_animation_frames.capacity(), capacity);
    assert_eq!(vm.host_animation_frame_next_id, next_id);
}

#[cfg_attr(test, test)]
fn scheduling_capacity_uses_existing_frame_slots_without_a_growth_charge() {
    let mut vm = page_vm();
    vm.install_host_animation_frame_functions().unwrap();
    execute(
        &mut vm,
        "function cb() {} for (var i=0;i<5;i++) requestAnimationFrame(cb); 0;",
    )
    .unwrap();
    let capacity = vm.host_animation_frames.capacity();
    let callback = vm.lookup_global_name("cb").unwrap().unwrap();
    assert!(vm.host_animation_frames.len() < capacity);
    let _owner = leave_room(&mut vm, 0);
    assert_eq!(
        vm.host_animation_frame_call(
            host_animation_frames::HostAnimationFrameFunction::Request,
            &[callback],
            false,
        ),
        Ok(Value::Number(6.0))
    );
    assert_eq!(vm.host_animation_frames.capacity(), capacity);
    assert_eq!(vm.host_animation_frames.len(), 6);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn scheduling_capacity_refuses_job_growth_before_queue_publication() {
    let mut vm = page_vm();
    let callback = execute(&mut vm, "globalThis.cb = function() {}; cb;").unwrap();
    for _ in 0..8 {
        vm.enqueue_promise_job(PromiseJob::HostMicrotask {
            callback: callback.clone(),
        });
    }
    let capacity = vm.promise_jobs.capacity();
    assert_eq!(capacity, vm.promise_jobs.len());
    let _owner = leave_room(
        &mut vm,
        capacity * std::mem::size_of::<promise_job_storage::QueuedPromiseJob>() - 1,
    );
    vm.enqueue_promise_job(PromiseJob::HostMicrotask { callback });
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promise_jobs.len(), 8);
    assert_eq!(vm.promise_jobs.capacity(), capacity);
}

#[cfg_attr(test, test)]
fn scheduling_capacity_uses_existing_job_slots_without_a_growth_charge() {
    let mut vm = page_vm();
    let callback = execute(&mut vm, "globalThis.cb = function() {}; cb;").unwrap();
    for _ in 0..5 {
        vm.enqueue_promise_job(PromiseJob::HostMicrotask {
            callback: callback.clone(),
        });
    }
    let capacity = vm.promise_jobs.capacity();
    assert!(vm.promise_jobs.len() < capacity);
    let _owner = leave_room(&mut vm, 0);
    vm.enqueue_promise_job(PromiseJob::HostMicrotask { callback });
    assert_eq!(vm.promise_jobs.len(), 6);
    assert_eq!(vm.promise_jobs.capacity(), capacity);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn scheduling_capacity_refuses_reaction_growth_before_owner_publication() {
    let mut vm = page_vm();
    execute(
        &mut vm,
        "globalThis.p = new Promise(() => {}); globalThis.cb = function() {}; 0;",
    )
    .unwrap();
    let promise = vm
        .lookup_global_name("p")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let callback = vm.lookup_global_name("cb").unwrap().unwrap();
    let target = vm.new_promise().unwrap();
    let reaction = || {
        PromiseReaction::Then(PromiseThenReaction {
            target: ReactionTarget::Native(target),
            on_fulfilled: callback.clone(),
            on_rejected: Value::Undefined,
        })
    };
    for _ in 0..8 {
        vm.enqueue_promise_reaction(promise, reaction());
    }
    let capacity = vm.promises[&promise].reactions.capacity();
    assert_eq!(capacity, vm.promises[&promise].reactions.len());
    let _owner = leave_room(
        &mut vm,
        capacity * std::mem::size_of::<PromiseReaction>() - 1,
    );
    vm.enqueue_promise_reaction(promise, reaction());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promises[&promise].reactions.len(), 8);
    assert_eq!(vm.promises[&promise].reactions.capacity(), capacity);
}

#[cfg_attr(test, test)]
fn scheduling_capacity_uses_existing_reaction_slots_without_a_growth_charge() {
    let mut vm = page_vm();
    execute(
        &mut vm,
        "globalThis.p = new Promise(() => {}); globalThis.cb = function() {}; 0;",
    )
    .unwrap();
    let promise = vm
        .lookup_global_name("p")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let callback = vm.lookup_global_name("cb").unwrap().unwrap();
    let target = vm.new_promise().unwrap();
    let reaction = || {
        PromiseReaction::Then(PromiseThenReaction {
            target: ReactionTarget::Native(target),
            on_fulfilled: callback.clone(),
            on_rejected: Value::Undefined,
        })
    };
    for _ in 0..5 {
        vm.enqueue_promise_reaction(promise, reaction());
    }
    let capacity = vm.promises[&promise].reactions.capacity();
    assert!(vm.promises[&promise].reactions.len() < capacity);
    let _owner = leave_room(&mut vm, 0);
    vm.enqueue_promise_reaction(promise, reaction());
    assert_eq!(vm.promises[&promise].reactions.len(), 6);
    assert_eq!(vm.promises[&promise].reactions.capacity(), capacity);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

impl crate::Vm {
    /// Runs scheduling capacity contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_scheduling_capacity_contracts() {
        scheduling_capacity_refuses_timer_growth_before_ids_or_entries_change();
        scheduling_capacity_uses_existing_timer_slots_without_a_growth_charge();
        scheduling_capacity_refuses_frame_growth_before_handles_or_entries_change();
        scheduling_capacity_uses_existing_frame_slots_without_a_growth_charge();
        scheduling_capacity_refuses_job_growth_before_queue_publication();
        scheduling_capacity_uses_existing_job_slots_without_a_growth_charge();
        scheduling_capacity_refuses_reaction_growth_before_owner_publication();
        scheduling_capacity_uses_existing_reaction_slots_without_a_growth_charge();
    }
}
