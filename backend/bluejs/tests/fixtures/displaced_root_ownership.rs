// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny displaced frames preserve context across admission/registration errors.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn queued_vm() -> Vm {
    let mut vm = bounded_vm();
    vm.execute_script(&script("Promise.resolve().then(()=>0);0;"))
        .unwrap();
    assert!(!vm.promise_jobs.is_empty());
    vm
}
fn object_on_stack(vm: &mut Vm) -> ObjectId {
    let id = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.stack.push(Value::Object(id));
    id
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
fn displaced_root_ownership_partial_job_registration_restores_frame_and_releases_roots() {
    let mut vm = queued_vm();
    let first = object_on_stack(&mut vm);
    let second = object_on_stack(&mut vm);
    vm.active_module_name = Some("outer".into());
    vm.call_depth = 3;
    vm.remaining_instructions = 41;
    let pointer = vm.stack.as_ptr();
    let jobs = vm.promise_jobs.len();
    vm.heap.allow_root_registrations(1);
    assert_eq!(
        vm.run_next_job_while_module_suspended(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack, vec![Value::Object(first), Value::Object(second)]);
    assert_eq!(vm.active_module_name.as_deref(), Some("outer"));
    assert_eq!(vm.call_depth, 3);
    assert_eq!(vm.remaining_instructions, 41);
    assert_eq!(vm.promise_jobs.len(), jobs);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.stack.clear();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(!vm.heap.contains(first) && !vm.heap.contains(second));
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_native_refusal_precedes_registration_and_keeps_the_job() {
    let mut vm = queued_vm();
    object_on_stack(&mut vm);
    let pointer = vm.stack.as_ptr();
    let jobs = vm.promise_jobs.len();
    let registrations = vm.heap.stats().root_registrations;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.run_next_job_while_module_suspended(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats().root_registrations, registrations);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.promise_jobs.len(), jobs);
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_empty_checkpoint_needs_no_roots_or_capacity() {
    let mut vm = bounded_vm();
    object_on_stack(&mut vm);
    let pointer = vm.stack.as_ptr();
    vm.remaining_instructions = 41;
    let _peer = leave_room(&mut vm, 0);
    vm.heap.allow_root_registrations(0);
    assert_eq!(vm.run_next_job_while_module_suspended(), Ok(false));
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.remaining_instructions, 41);
}

fn collect(vm: &mut Vm) {
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_saved_edges_survive_collection_and_release() {
    let mut vm = bounded_vm();
    let objects: Vec<_> = (0..16).map(|_| object_on_stack(&mut vm)).collect();
    vm.bindings.push(Some(Value::Object(objects[1])));
    vm.completion = Value::Object(objects[2]);
    vm.with_objects.push(Value::Object(objects[3]));
    vm.this = Value::Object(objects[4]);
    vm.arguments.push(Value::Object(objects[5]));
    vm.callee = Value::Object(objects[6]);
    vm.new_target = Value::Object(objects[7]);
    vm.completion_saves.push((Value::Object(objects[8]), true));
    vm.pending_completions
        .push(Completion::Return(Value::Object(objects[9])));
    vm.pending_completions
        .push(Completion::Throw(RuntimeError::Thrown(Value::Object(
            objects[10],
        ))));
    vm.cells.insert(0, objects[11]);
    vm.dynamic_eval_bindings.insert(
        "local".into(),
        DynamicEvalBinding {
            cell: objects[12],
            shadowed_cells: vec![objects[13]],
        },
    );
    vm.dynamic_eval_outer_bindings.push(HashMap::from([(
        "outer".into(),
        DynamicEvalBinding {
            cell: objects[14],
            shadowed_cells: vec![objects[13]],
        },
    )]));
    vm.home_object = Some(objects[15]);
    vm.stack.truncate(1);
    let execution = vm.suspend_module_execution();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut roots = vm.root_suspended_module_execution(&execution).unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        roots.retained_bytes()
    );
    collect(&mut vm);
    assert!(objects.iter().all(|&id| vm.heap.contains(id)));
    vm.release_module_roots_from(&mut roots, 0);
    drop(execution);
    collect(&mut vm);
    assert!(objects.iter().all(|&id| !vm.heap.contains(id)));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_failed_append_keeps_the_registered_prefix() {
    let mut vm = bounded_vm();
    let object = object_on_stack(&mut vm);
    let mut roots = ModuleRoots::default();
    Vm::append_registered_frame_roots(
        &mut roots,
        &mut vm.heap,
        vm.host_scheduling.as_ref(),
        [object; 4],
    )
    .unwrap();
    let prefix = roots.snapshot();
    let bytes = roots.retained_bytes();
    vm.heap.allow_root_registrations(2);
    assert_eq!(
        Vm::append_registered_frame_roots(
            &mut roots,
            &mut vm.heap,
            vm.host_scheduling.as_ref(),
            [object; 3]
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(roots.snapshot(), prefix);
    assert_eq!(roots.retained_bytes(), bytes);
    vm.stack.clear();
    collect(&mut vm);
    assert!(vm.heap.contains(object));
    vm.release_module_roots_from(&mut roots, 0);
    collect(&mut vm);
    assert!(!vm.heap.contains(object));
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_queued_job_gc_preserves_and_releases_the_frame() {
    let mut vm = bounded_vm();
    vm.install_test262_harness().unwrap();
    vm.execute_script(&script("Promise.resolve().then(()=>{$262.gc();});0;"))
        .unwrap();
    let object = object_on_stack(&mut vm);
    vm.call_depth = 3;
    vm.remaining_instructions = 41;
    let pointer = vm.stack.as_ptr();
    assert_eq!(vm.run_next_job_while_module_suspended(), Ok(true));
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack, vec![Value::Object(object)]);
    assert_eq!(vm.call_depth, 3);
    assert_eq!(vm.remaining_instructions, 41);
    assert!(vm.heap.contains(object));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.stack.clear();
    collect(&mut vm);
    assert!(!vm.heap.contains(object));
}

fn deferred_namespace(vm: &mut Vm, gc: bool) -> Value {
    let module = |source| crate::compile_module(&crate::parse_module(source).unwrap()).unwrap();
    vm.set_module_loader_context(
        "entry",
        HashMap::from([
            (
                "entry".into(),
                module("import defer * as ns from './dep';export {ns};"),
            ),
            (
                "dep".into(),
                module(if gc {
                    "$262.gc();globalThis.bodyRuns=1;export const answer=42;"
                } else {
                    "globalThis.bodyRuns=1;export const answer=42;"
                }),
            ),
        ]),
    );
    let registry = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &registry, false, false, ImportPhase::Evaluation)
        .unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    vm.get_property(&namespace, &"ns".into()).unwrap()
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_deferred_refusal_restores_the_observer_without_running_body() {
    let mut vm = bounded_vm();
    let namespace = deferred_namespace(&mut vm, false);
    let first = object_on_stack(&mut vm);
    let second = object_on_stack(&mut vm);
    vm.stack.reserve(16);
    let pointer = vm.stack.as_ptr();
    vm.call_depth = 3;
    vm.active_module_name = Some("observer".into());
    vm.heap.allow_root_registrations(1);
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack, vec![Value::Object(first), Value::Object(second)]);
    assert_eq!(vm.call_depth, 3);
    assert_eq!(vm.active_module_name.as_deref(), Some("observer"));
    assert_eq!(vm.lookup_global_name("bodyRuns").unwrap(), None);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.stack.clear();
    collect(&mut vm);
    assert!(!vm.heap.contains(first) && !vm.heap.contains(second));
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_deferred_gc_preserves_and_releases_the_observer() {
    let mut vm = bounded_vm();
    vm.install_test262_harness().unwrap();
    let namespace = deferred_namespace(&mut vm, true);
    let object = object_on_stack(&mut vm);
    vm.call_depth = 3;
    vm.active_module_name = Some("observer".into());
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Ok(Value::Number(42.0))
    );
    assert_eq!(vm.stack, vec![Value::Object(object)]);
    assert_eq!(vm.call_depth, 3);
    assert_eq!(vm.active_module_name.as_deref(), Some("observer"));
    assert!(vm.heap.contains(object));
    assert_eq!(
        vm.lookup_global_name("bodyRuns").unwrap(),
        Some(Value::Number(1.0))
    );
    vm.stack.clear();
    collect(&mut vm);
    assert!(!vm.heap.contains(object));
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_scalar_frame_needs_no_registration_or_native_capacity() {
    let mut vm = bounded_vm();
    vm.stack.push(Value::Number(42.0));
    let _peer = leave_room(&mut vm, 0);
    let execution = vm.suspend_module_execution();
    let registrations = vm.heap.stats().root_registrations;
    vm.heap.allow_root_registrations(0);
    let roots = vm.root_suspended_module_execution(&execution).unwrap();
    assert!(roots.is_empty());
    assert_eq!(roots.retained_bytes(), 0);
    assert_eq!(vm.heap.stats().root_registrations, registrations);
    vm.restore_module_execution(execution);
    assert_eq!(vm.stack, vec![Value::Number(42.0)]);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_displaced_root_ownership_contracts() {
        displaced_root_ownership_partial_job_registration_restores_frame_and_releases_roots();
        displaced_root_ownership_native_refusal_precedes_registration_and_keeps_the_job();
        displaced_root_ownership_empty_checkpoint_needs_no_roots_or_capacity();
        displaced_root_ownership_saved_edges_survive_collection_and_release();
        displaced_root_ownership_failed_append_keeps_the_registered_prefix();
        displaced_root_ownership_queued_job_gc_preserves_and_releases_the_frame();
        displaced_root_ownership_deferred_refusal_restores_the_observer_without_running_body();
        displaced_root_ownership_deferred_gc_preserves_and_releases_the_observer();
        displaced_root_ownership_scalar_frame_needs_no_registration_or_native_capacity();
        Self::verify_shadow_displaced_root_contracts();
    }
}
