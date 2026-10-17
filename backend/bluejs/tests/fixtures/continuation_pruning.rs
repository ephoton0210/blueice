// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite awaited frames verify post-collection pruning without native scratch.

use super::*;

fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm.next_host_native_gc_bytes = usize::MAX;
    vm.next_host_heap_gc_bytes = usize::MAX;
    vm
}
fn async_page() -> Vm {
    let mut vm = page();
    execute(
        &mut vm,
        "var gate=Promise.withResolvers();var done=(async()=>{await gate.promise;return 42;})();0;",
    );
    assert_eq!(vm.async_continuations.len(), 1);
    vm
}
fn module_page() -> Vm {
    let mut vm = page();
    execute(&mut vm, "var gate=Promise.withResolvers();0;");
    let code = crate::compile_module(
        &crate::parse_module("await gate.promise;export const answer=42;").unwrap(),
    )
    .unwrap();
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), code)]));
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
    assert_eq!(vm.module_continuations.len(), 1);
    vm
}
#[cfg(test)]
fn assert_no_scratch(vm: &mut Vm) {
    // Optional map compaction is a separate admitted producer. Isolate the
    // required pruning phase, retaining all current backing and live frames.
    vm.preserve_promise_table_capacity = true;
    vm.prune_collected_promise_state();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let (_, allocations) = allocation_observer::count(|| vm.prune_collected_promise_state());
    assert_eq!(
        allocations, 0,
        "post-GC ownership pruning must allocate no scratch"
    );
    assert!(vm.check_scheduling_failure().is_ok());
    vm.preserve_promise_table_capacity = false;
}
#[cfg(test)]
#[test]
fn continuation_pruning_live_async_needs_no_native_scratch() {
    let mut vm = async_page();
    assert_no_scratch(&mut vm);
    assert_eq!(vm.async_continuations.len(), 1);
}
#[cfg(test)]
#[test]
fn continuation_pruning_live_module_needs_no_native_scratch() {
    let mut vm = module_page();
    assert_no_scratch(&mut vm);
    assert_eq!(vm.module_continuations.len(), 1);
}
#[cfg(test)]
#[test]
fn continuation_pruning_queued_frames_need_no_native_scratch() {
    let mut vm = module_page();
    execute(
        &mut vm,
        "var done=(async()=>{await gate.promise;return 7;})();gate.resolve(3);0;",
    );
    assert_eq!(vm.async_continuations.len(), 1);
    assert_eq!(vm.module_continuations.len(), 1);
    assert!(vm
        .promise_jobs
        .iter()
        .any(|job| matches!(job, PromiseJob::AsyncAwait { .. })));
    assert!(vm
        .promise_jobs
        .iter()
        .any(|job| matches!(job, PromiseJob::ModuleAwait { .. })));
    assert_no_scratch(&mut vm);
    assert_eq!(vm.async_continuations.len(), 1);
    assert_eq!(vm.module_continuations.len(), 1);
}

fn collect(vm: &mut Vm) {
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
}
fn clear_global(vm: &mut Vm, name: &str) {
    // Normal assignment updates both the global property and var-binding
    // owners; changing the heap property alone leaves the latter rooted.
    execute(vm, &format!("{name}=null;0;"));
    assert_eq!(vm.lookup_global_name(name).unwrap(), Some(Value::Null));
}
fn held_async_page() -> (Vm, ObjectId) {
    let mut vm = page();
    execute(&mut vm, "var gate=Promise.withResolvers();var held={key:7};var done=(async()=>{let saved=held;await gate.promise;return saved;})();0;");
    let held = vm
        .lookup_global_name("held")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    (vm, held)
}
fn remove_reactions(vm: &mut Vm, module: bool) {
    vm.promises.retain(|_, record| {
        record.reactions.retain(|reaction| {
            !matches!(
                (module, reaction),
                (true, PromiseReaction::ModuleAwait { .. })
                    | (false, PromiseReaction::AsyncAwait { .. })
            )
        });
        true
    });
}
fn prune(vm: &mut Vm) {
    let previous = vm.preserve_promise_table_capacity;
    vm.preserve_promise_table_capacity = true;
    vm.prune_collected_promise_state();
    vm.preserve_promise_table_capacity = previous;
}
#[cfg_attr(test, test)]
fn continuation_pruning_live_async_frame_keeps_local_object_and_resumes() {
    let (mut vm, held) = held_async_page();
    execute(&mut vm, "var answer;done.then(value=>answer=value);0;");
    clear_global(&mut vm, "held");
    collect(&mut vm);
    assert!(vm.heap.contains(held));
    assert_eq!(vm.async_continuations.len(), 1);
    execute(&mut vm, "gate.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(vm.async_continuations.is_empty());
    assert_eq!(
        vm.lookup_global_name("answer").unwrap(),
        Some(Value::Object(held))
    );
}
#[cfg_attr(test, test)]
fn continuation_pruning_live_module_frame_survives_collection_and_resumes() {
    let mut vm = module_page();
    collect(&mut vm);
    assert_eq!(vm.module_continuations.len(), 1);
    execute(&mut vm, "gate.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(vm.module_continuations.is_empty());
    assert!(vm.module_graph.as_ref().unwrap().linked["entry"].evaluated);
}
#[cfg_attr(test, test)]
fn continuation_pruning_orphan_async_releases_local_object_and_frame() {
    let (mut vm, held) = held_async_page();
    for name in ["held", "done", "gate"] {
        clear_global(&mut vm, name);
    }
    collect(&mut vm);
    assert!(!vm.heap.contains(held));
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.async_continuations.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn continuation_pruning_queued_async_is_live_until_its_job_finishes() {
    let (mut vm, held) = held_async_page();
    execute(&mut vm, "gate.resolve(0);0;");
    assert!(vm
        .promise_jobs
        .iter()
        .any(|job| matches!(job, PromiseJob::AsyncAwait { .. })));
    for name in ["held", "done", "gate"] {
        clear_global(&mut vm, name);
    }
    collect(&mut vm);
    assert!(vm.heap.contains(held));
    assert_eq!(vm.async_continuations.len(), 1);
    vm.run_host_microtask_checkpoint().unwrap();
    collect(&mut vm);
    assert!(vm.async_continuations.is_empty());
    assert!(!vm.heap.contains(held));
}
#[cfg_attr(test, test)]
fn continuation_pruning_queued_module_survives_without_its_old_reaction() {
    let mut vm = module_page();
    execute(&mut vm, "gate.resolve(0);0;");
    assert!(vm
        .promise_jobs
        .iter()
        .any(|job| matches!(job, PromiseJob::ModuleAwait { .. })));
    assert!(!vm.promises.values().any(|record| record
        .reactions
        .iter()
        .any(|reaction| matches!(reaction, PromiseReaction::ModuleAwait { .. }))));
    clear_global(&mut vm, "gate");
    collect(&mut vm);
    assert_eq!(vm.module_continuations.len(), 1);
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(vm.module_continuations.is_empty());
    assert!(vm.module_graph.as_ref().unwrap().linked["entry"].evaluated);
}
#[cfg_attr(test, test)]
fn continuation_pruning_async_marks_do_not_outlive_removed_reactions() {
    let mut vm = async_page();
    prune(&mut vm);
    assert_eq!(vm.async_continuations.len(), 1);
    remove_reactions(&mut vm, false);
    prune(&mut vm);
    assert!(vm.async_continuations.is_empty());
}
#[cfg_attr(test, test)]
fn continuation_pruning_module_marks_do_not_outlive_removed_reactions() {
    let mut vm = module_page();
    prune(&mut vm);
    assert_eq!(vm.module_continuations.len(), 1);
    remove_reactions(&mut vm, true);
    prune(&mut vm);
    assert!(vm.module_continuations.is_empty());
}
#[cfg_attr(test, test)]
fn continuation_pruning_marks_do_not_outlive_removed_jobs() {
    let mut vm = module_page();
    execute(
        &mut vm,
        "var done=(async()=>{await gate.promise;return 7;})();gate.resolve(0);0;",
    );
    prune(&mut vm);
    assert_eq!(vm.async_continuations.len(), 1);
    assert_eq!(vm.module_continuations.len(), 1);
    vm.promise_jobs.retain(|job| {
        !matches!(
            job,
            PromiseJob::ModuleAwait { .. } | PromiseJob::AsyncAwait { .. }
        )
    });
    prune(&mut vm);
    assert!(vm.async_continuations.is_empty());
    assert!(vm.module_continuations.is_empty());
}
#[cfg_attr(test, test)]
fn continuation_pruning_async_serial_does_not_mark_same_numbered_module() {
    let mut vm = module_page();
    execute(
        &mut vm,
        "var done=(async()=>{await gate.promise;return 7;})();0;",
    );
    assert_eq!(
        vm.async_continuations.keys().next(),
        vm.module_continuations.keys().next()
    );
    remove_reactions(&mut vm, true);
    prune(&mut vm);
    assert_eq!(vm.async_continuations.len(), 1);
    assert!(vm.module_continuations.is_empty());
}
#[cfg_attr(test, test)]
fn continuation_pruning_module_serial_does_not_mark_same_numbered_async_frame() {
    let mut vm = module_page();
    execute(
        &mut vm,
        "var done=(async()=>{await gate.promise;return 7;})();0;",
    );
    assert_eq!(
        vm.async_continuations.keys().next(),
        vm.module_continuations.keys().next()
    );
    remove_reactions(&mut vm, false);
    prune(&mut vm);
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.module_continuations.len(), 1);
}
#[cfg_attr(test, test)]
fn continuation_pruning_duplicate_and_unknown_owners_create_no_frames() {
    let mut vm = async_page();
    let serial = *vm.async_continuations.keys().next().unwrap();
    vm.promise_jobs.extend([
        PromiseJob::AsyncAwait {
            continuation: serial,
            value: Value::Undefined,
            fulfilled: true,
            origin: ThrowOrigin::default(),
        },
        PromiseJob::AsyncAwait {
            continuation: serial,
            value: Value::Undefined,
            fulfilled: true,
            origin: ThrowOrigin::default(),
        },
        PromiseJob::AsyncAwait {
            continuation: serial + 1,
            value: Value::Undefined,
            fulfilled: true,
            origin: ThrowOrigin::default(),
        },
        PromiseJob::ModuleAwait {
            continuation: serial,
            value: Value::Undefined,
            fulfilled: true,
            origin: ThrowOrigin::default(),
        },
    ]);
    prune(&mut vm);
    assert_eq!(vm.async_continuations.len(), 1);
    assert!(vm.module_continuations.is_empty());
    remove_reactions(&mut vm, false);
    vm.promise_jobs.clear();
    prune(&mut vm);
    assert!(vm.async_continuations.is_empty());
}
#[cfg_attr(test, test)]
fn continuation_pruning_terminal_cleanup_needs_no_headroom_and_preserves_backing() {
    let mut vm = async_page();
    remove_reactions(&mut vm, false);
    let backing = vm.async_continuations.retained_bytes();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    vm.preserve_promise_table_capacity = true;
    #[cfg(test)]
    {
        let (_, allocations) = allocation_observer::count(|| vm.prune_collected_promise_state());
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    vm.prune_collected_promise_state();
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.async_continuations.retained_bytes(), backing);
    assert!(vm.host_scheduling_snapshot().payload_bytes < MAX_HOST_SCHEDULING_BYTES);
    vm.preserve_promise_table_capacity = false;
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.async_continuations.retained_bytes(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn continuation_pruning_async_generator_is_retained_then_collects_with_its_request() {
    let mut vm = page();
    execute(&mut vm, "var gate=Promise.withResolvers();var iterator=(async function*(){await gate.promise;yield 42;})();var request=iterator.next();0;");
    let iterator = vm
        .lookup_global_name("iterator")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    assert_eq!(
        vm.async_continuations.values().next().unwrap().generator,
        Some(iterator)
    );
    collect(&mut vm);
    assert!(vm.heap.contains(iterator));
    assert_eq!(vm.async_continuations.len(), 1);
    for name in ["iterator", "request", "gate"] {
        clear_global(&mut vm, name);
    }
    collect(&mut vm);
    assert!(!vm.heap.contains(iterator));
    assert!(vm.async_continuations.is_empty());
}

impl crate::Vm {
    /// Runs finite continuation-owner contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_continuation_pruning_contracts() {
        #[cfg(test)]
        {
            continuation_pruning_live_async_needs_no_native_scratch();
            continuation_pruning_live_module_needs_no_native_scratch();
            continuation_pruning_queued_frames_need_no_native_scratch();
        }
        continuation_pruning_live_async_frame_keeps_local_object_and_resumes();
        continuation_pruning_live_module_frame_survives_collection_and_resumes();
        continuation_pruning_orphan_async_releases_local_object_and_frame();
        continuation_pruning_queued_async_is_live_until_its_job_finishes();
        continuation_pruning_queued_module_survives_without_its_old_reaction();
        continuation_pruning_async_marks_do_not_outlive_removed_reactions();
        continuation_pruning_module_marks_do_not_outlive_removed_reactions();
        continuation_pruning_marks_do_not_outlive_removed_jobs();
        continuation_pruning_async_serial_does_not_mark_same_numbered_module();
        continuation_pruning_module_serial_does_not_mark_same_numbered_async_frame();
        continuation_pruning_duplicate_and_unknown_owners_create_no_frames();
        continuation_pruning_terminal_cleanup_needs_no_headroom_and_preserves_backing();
        continuation_pruning_async_generator_is_retained_then_collects_with_its_request();
    }
}
