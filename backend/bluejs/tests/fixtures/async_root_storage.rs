// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny frames and scalar peers verify pre-allocation root admission.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn object(vm: &mut Vm) -> ObjectId {
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.stack.push(Value::Object(object));
    object
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn async_root_storage_native_pressure_refusal_precedes_capacity_and_collection() {
    let mut vm = vm();
    let id = object(&mut vm);
    vm.next_host_native_gc_bytes = 0;
    let capacity = vm.async_frame_roots.capacity();
    let collections = vm.heap.stats().major_collections;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.collect_for_host_native_pressure(&Value::Object(id), &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.async_frame_roots.capacity(), capacity);
    assert_eq!(vm.heap.stats().major_collections, collections);
    assert!(vm.async_frame_roots.is_empty());
}

#[cfg_attr(test, test)]
fn async_root_storage_event_completion_refusal_preserves_root_capacity() {
    let mut vm = vm();
    let id = object(&mut vm);
    vm.host_event_microtask_jobs = Some(8);
    let capacity = vm.async_frame_roots.capacity();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.cleanup_host_event_callback(Ok(Value::Object(id))),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.async_frame_roots.capacity(), capacity);
    assert!(vm.async_frame_roots.is_empty());
    assert_eq!(vm.host_event_microtask_jobs, Some(8));
}

#[cfg_attr(test, test)]
fn async_root_storage_job_refusal_keeps_the_queued_owner_and_fuel() {
    let mut vm = vm();
    vm.execute_script(&script(
        "Promise.resolve().then(()=>{globalThis.bodyRuns=1;});0;",
    ))
    .unwrap();
    vm.remaining_instructions = 41;
    let jobs = vm.promise_jobs.len();
    let capacity = vm.async_frame_roots.capacity();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_next_promise_job(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promise_jobs.len(), jobs);
    assert_eq!(vm.remaining_instructions, 41);
    assert_eq!(vm.async_frame_roots.capacity(), capacity);
    assert_eq!(vm.lookup_global_name("bodyRuns").unwrap(), None);
}

#[cfg_attr(test, test)]
fn async_root_storage_iterator_refusal_precedes_capacity_growth() {
    let mut vm = vm();
    let id = object(&mut vm);
    let mut iterators = vm.iterator_records(vec![Value::Object(id)]);
    let capacity = vm.async_frame_roots.capacity();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.interpret(&script("0;"), &mut iterators, 0, None, None, None)
            .err(),
        Some(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.async_frame_roots.capacity(), capacity);
    assert!(vm.async_frame_roots.is_empty());
}

fn collect(vm: &mut Vm) {
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn async_root_storage_replacement_refusal_preserves_prefix_and_capacity() {
    let mut vm = vm();
    let id = object(&mut vm);
    vm.extend_async_frame_roots([id; 4].into_iter()).unwrap();
    let pointer = vm.async_frame_roots.as_ptr();
    let capacity = vm.async_frame_roots.capacity();
    // The final capacity delta fits; the coexisting replacement does not.
    let _peer = leave_room(&mut vm, capacity * std::mem::size_of::<ObjectId>());
    assert_eq!(
        vm.extend_async_frame_roots(std::iter::once(id)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.async_frame_roots.as_ptr(), pointer);
    assert_eq!(vm.async_frame_roots.capacity(), capacity);
    assert_eq!(vm.async_frame_roots, vec![id; 4]);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn async_root_storage_spare_slots_work_at_full_scalar_quota() {
    let mut vm = vm();
    let id = object(&mut vm);
    vm.extend_async_frame_roots(std::iter::once(id)).unwrap();
    let pointer = vm.async_frame_roots.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    vm.extend_async_frame_roots(std::iter::once(id)).unwrap();
    assert_eq!(vm.async_frame_roots, vec![id; 2]);
    assert_eq!(vm.async_frame_roots.as_ptr(), pointer);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn async_root_storage_empty_append_needs_no_capacity() {
    let mut vm = vm();
    let _peer = leave_room(&mut vm, 0);
    vm.extend_async_frame_roots(std::iter::empty()).unwrap();
    assert_eq!(vm.async_frame_roots.capacity(), 0);
}

#[cfg_attr(test, test)]
fn async_root_storage_growth_retains_native_ownership_and_gc_edges_until_cleanup() {
    let mut vm = vm();
    let id = object(&mut vm);
    vm.stack = Vec::new();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let trace_before = vm.gc_trace_cache.retained_bytes();
    vm.extend_async_frame_roots([id; 4].into_iter()).unwrap();
    vm.extend_async_frame_roots(std::iter::once(id)).unwrap();
    let bytes = vm.async_frame_roots.capacity() * std::mem::size_of::<ObjectId>();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes - before, bytes);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    collect(&mut vm);
    assert!(vm.heap.contains(id));
    let trace_growth = vm.gc_trace_cache.retained_bytes() - trace_before;
    vm.async_frame_roots.truncate(0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        bytes + trace_growth
    );
    vm.release_finished_execution_control();
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        before + trace_growth
    );
    collect(&mut vm);
    assert!(!vm.heap.contains(id));
}

fn suspend_module(vm: &mut Vm) -> u64 {
    vm.execute_script(&script("globalThis.gate=Promise.withResolvers();0;"))
        .unwrap();
    let code = crate::compile_module(
        &crate::parse_module("await gate.promise;export const answer=42;").unwrap(),
    )
    .unwrap();
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), code)]));
    let registry = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &registry, false, false, ImportPhase::Evaluation)
        .unwrap();
    assert_eq!(vm.module_continuations.len(), 1);
    *vm.module_continuations.keys().next().unwrap()
}

#[cfg_attr(test, test)]
fn async_root_storage_module_resume_refusal_restores_ambient_and_keeps_continuation() {
    let mut vm = vm();
    let serial = suspend_module(&mut vm);
    let id = object(&mut vm);
    vm.call_depth = 3;
    vm.remaining_instructions = 41;
    vm.active_module_name = Some("observer".into());
    let pointer = vm.stack.as_ptr();
    // No active roots survive the setup turn; this is a fresh root allocation.
    vm.async_frame_roots = Vec::new();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.resume_module_await(serial, Value::Undefined, true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.stack, vec![Value::Object(id)]);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.call_depth, 3);
    assert_eq!(vm.remaining_instructions, 41);
    assert_eq!(vm.active_module_name.as_deref(), Some("observer"));
    assert!(vm.module_continuations.contains_key(&serial));
    assert_eq!(vm.async_frame_roots.capacity(), 0);
}

fn suspend_async(vm: &mut Vm, generator: bool) -> u64 {
    vm.execute_script(&script(if generator {
        "globalThis.gate=Promise.withResolvers();globalThis.g=(async function*(){await gate.promise;yield 42;})();g.next();0;"
    }else{
        "globalThis.gate=Promise.withResolvers();globalThis.result=(async()=>{await gate.promise;return 42;})();0;"
    })).unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    *vm.async_continuations.keys().next().unwrap()
}

#[cfg_attr(test, test)]
fn async_root_storage_async_resume_refusal_keeps_its_stored_frame() {
    let mut vm = vm();
    let serial = suspend_async(&mut vm, false);
    let id = object(&mut vm);
    vm.remaining_instructions = 41;
    vm.async_frame_roots = Vec::new();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.resume_async_await(serial, Value::Undefined, true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.async_continuations.contains_key(&serial));
    assert_eq!(vm.stack, vec![Value::Object(id)]);
    assert_eq!(vm.remaining_instructions, 41);
    assert_eq!(vm.async_frame_roots.capacity(), 0);
}

#[cfg_attr(test, test)]
fn async_root_storage_event_displacement_refusal_restores_frame_and_turn_budget() {
    let mut vm = vm();
    let id = object(&mut vm);
    vm.remaining_instructions = 41;
    let pointer = vm.stack.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    let result = vm.run_host_event_turn(|vm| vm.cleanup_host_event_callback(Ok(Value::Undefined)));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.stack, vec![Value::Object(id)]);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.remaining_instructions, 41);
    assert_eq!(vm.host_event_microtask_jobs, None);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.async_frame_roots.capacity(), 0);
}

#[cfg_attr(test, test)]
fn async_root_storage_queued_reaction_preserves_its_object_through_gc() {
    let mut vm = vm();
    vm.install_test262_harness().unwrap();
    vm.execute_script(&script("globalThis.answer=0;Promise.resolve({answer:42}).then(value=>{$262.gc();answer=value.answer;});0;")).unwrap();
    assert!(vm.run_next_promise_job().unwrap());
    assert_eq!(
        vm.lookup_global_name("answer").unwrap(),
        Some(Value::Number(42.0))
    );
    assert!(vm.async_frame_roots.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn async_root_storage_host_neutral_roots_need_no_persistent_registration() {
    let mut vm = Vm::default();
    let id = object(&mut vm);
    vm.stack = Vec::new();
    vm.heap.allow_root_registrations(0);
    vm.extend_async_frame_roots(std::iter::once(id)).unwrap();
    collect(&mut vm);
    assert!(vm.heap.contains(id));
    vm.async_frame_roots.clear();
    collect(&mut vm);
    assert!(!vm.heap.contains(id));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn async_root_storage_terminal_cleanup_refunds_retained_capacity() {
    let mut vm = vm();
    let id = object(&mut vm);
    vm.extend_async_frame_roots([id; 4].into_iter()).unwrap();
    let bytes = vm.async_frame_roots.capacity() * std::mem::size_of::<ObjectId>();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.extend_async_frame_roots(std::iter::once(id)),
        Err(RuntimeError::SchedulingLimit)
    );
    vm.async_frame_roots.clear();
    vm.async_frame_roots = Vec::new();
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn async_root_storage_finalization_callback_keeps_its_holdings_through_gc() {
    let mut vm = vm();
    vm.install_test262_harness().unwrap();
    vm.execute_script(&script("globalThis.answer=0;globalThis.registry=new FinalizationRegistry(held=>{$262.gc();answer=held.answer;});registry.register({},{answer:42});0;")).unwrap();
    collect(&mut vm);
    vm.enqueue_finalization_cleanup_jobs().unwrap();
    vm.run_browser_task_turn(1).unwrap();
    assert_eq!(
        vm.lookup_global_name("answer").unwrap(),
        Some(Value::Number(42.0))
    );
    assert!(vm.async_frame_roots.is_empty());
    assert_eq!(vm.active_finalization_registry, None);
}

#[cfg_attr(test, test)]
fn async_root_storage_generator_ambient_refusal_restores_its_observer() {
    let mut vm = vm();
    let serial = suspend_async(&mut vm, true);
    let objects: Vec<_> = (0..8).map(|_| object(&mut vm)).collect();
    vm.call_depth = 3;
    vm.remaining_instructions = 41;
    vm.async_frame_roots = Vec::new();
    let frame = vm.async_continuations.get(&serial).unwrap();
    let count = usize::from(frame.generator.is_some())
        + 1
        + Vm::suspended_execution_references(&frame.execution).count()
        + frame.iterators.iter().filter_map(Value::object_id).count();
    let _peer = leave_room(&mut vm, count.max(4) * std::mem::size_of::<ObjectId>());
    assert_eq!(
        vm.resume_async_await(serial, Value::Undefined, true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.stack,
        objects.into_iter().map(Value::Object).collect::<Vec<_>>()
    );
    assert_eq!(vm.call_depth, 3);
    assert_eq!(vm.remaining_instructions, 41);
    assert!(vm.async_frame_roots.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_async_root_storage_contracts() {
        async_root_storage_native_pressure_refusal_precedes_capacity_and_collection();
        async_root_storage_event_completion_refusal_preserves_root_capacity();
        async_root_storage_job_refusal_keeps_the_queued_owner_and_fuel();
        async_root_storage_iterator_refusal_precedes_capacity_growth();
        async_root_storage_replacement_refusal_preserves_prefix_and_capacity();
        async_root_storage_spare_slots_work_at_full_scalar_quota();
        async_root_storage_empty_append_needs_no_capacity();
        async_root_storage_growth_retains_native_ownership_and_gc_edges_until_cleanup();
        async_root_storage_module_resume_refusal_restores_ambient_and_keeps_continuation();
        async_root_storage_async_resume_refusal_keeps_its_stored_frame();
        async_root_storage_event_displacement_refusal_restores_frame_and_turn_budget();
        async_root_storage_queued_reaction_preserves_its_object_through_gc();
        async_root_storage_host_neutral_roots_need_no_persistent_registration();
        async_root_storage_terminal_cleanup_refunds_retained_capacity();
        async_root_storage_finalization_callback_keeps_its_holdings_through_gc();
        async_root_storage_generator_ambient_refusal_restores_its_observer();
        Self::verify_async_root_host_batch_contracts();
    }
}
