// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny root batches and numeric peers verify admission before heap effects.

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
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn root_batch(vm: &mut Vm) {
    let id = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.stack.extend(std::iter::repeat_n(Value::Object(id), 96));
}

#[cfg_attr(test, test)]
fn gc_trace_storage_root_refusal_precedes_heap_operation() {
    let mut vm = vm();
    root_batch(&mut vm);
    let mut entered = false;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.with_roots(|_| {
            entered = true;
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!entered);
}

#[cfg_attr(test, test)]
fn gc_trace_storage_collection_refusal_preserves_heap_and_frame() {
    let mut vm = vm();
    root_batch(&mut vm);
    let before = vm.heap.stats();
    let pointer = vm.stack.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats().major_collections, before.major_collections);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack.len(), 96);
}

#[cfg_attr(test, test)]
fn gc_trace_storage_edge_refusal_precedes_collection() {
    let mut vm = vm();
    vm.execute_script(&script(
        "globalThis.gate=Promise.withResolvers();gate.promise.then(()=>0);0;",
    ))
    .unwrap();
    let gate = vm.lookup_global_name("gate").unwrap().unwrap();
    let promise = vm
        .get_property(&gate, &"promise".into())
        .unwrap()
        .object_id()
        .unwrap();
    let PromiseReaction::Then(reaction) = &vm.promises[&promise].reactions[0] else {
        panic!("a then reaction")
    };
    let (target, on_fulfilled, on_rejected) = (
        reaction.target.clone(),
        reaction.on_fulfilled.clone(),
        reaction.on_rejected.clone(),
    );
    vm.promises
        .get_mut(&promise)
        .unwrap()
        .reactions
        .extend((0..32).map(|_| {
            PromiseReaction::Then(PromiseThenReaction {
                target: target.clone(),
                on_fulfilled: on_fulfilled.clone(),
                on_rejected: on_rejected.clone(),
            })
        }));
    let mut entered = false;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.with_roots(|_| {
            entered = true;
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!entered);
    assert_eq!(vm.promises[&promise].reactions.len(), 33);
}

fn collect(vm: &mut Vm) {
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
}
fn object(vm: &mut Vm) -> ObjectId {
    let id = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.stack.push(Value::Object(id));
    id
}
fn shape(vm: &Vm) -> (usize, usize, *const ObjectId, *const (ObjectId, ObjectId)) {
    (
        vm.gc_trace_cache.roots.capacity(),
        vm.gc_trace_cache.edges.capacity(),
        vm.gc_trace_cache.roots.as_ptr(),
        vm.gc_trace_cache.edges.as_ptr(),
    )
}

#[cfg_attr(test, test)]
fn gc_trace_storage_cached_and_heap_held_capacity_has_one_native_owner() {
    let mut vm = vm();
    root_batch(&mut vm);
    vm.with_roots(|_| Ok(())).unwrap();
    let bytes = vm.gc_trace_cache.retained_bytes();
    assert!(bytes >= 96 * std::mem::size_of::<ObjectId>());
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.with_roots(|_| {
        assert_eq!(counter.get(), bytes);
        Ok(())
    })
    .unwrap();
    assert_eq!(counter.get(), 0);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    assert!(vm.gc_trace_cache.roots.is_empty() && vm.gc_trace_cache.edges.is_empty());
    vm.gc_trace_cache = Default::default();
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        before - bytes
    );
}

#[cfg_attr(test, test)]
fn gc_trace_storage_warm_capacity_reuses_addresses_at_full_scalar_quota() {
    let mut vm = vm();
    root_batch(&mut vm);
    collect(&mut vm);
    let before = shape(&vm);
    let _peer = leave_room(&mut vm, 0);
    collect(&mut vm);
    assert_eq!(shape(&vm), before);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm
        .stack
        .iter()
        .all(|value| vm.heap.contains(value.object_id().unwrap())));
}

#[cfg_attr(test, test)]
fn gc_trace_storage_terminal_cleanup_reuses_admitted_capacity() {
    let mut vm = vm();
    root_batch(&mut vm);
    collect(&mut vm);
    let before = shape(&vm);
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: 1,
            ..Default::default()
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    collect(&mut vm);
    assert_eq!(shape(&vm), before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn gc_trace_storage_reuse_refuses_newly_over_budget_ownership_before_heap_work() {
    let mut vm = vm();
    root_batch(&mut vm);
    collect(&mut vm);
    let before = shape(&vm);
    let _peer = leave_room(&mut vm, 0);
    // One numeric active owner changed after the last ledger publication.
    vm.active_scheduling_bytes += 1;
    let mut entered = false;
    assert_eq!(
        vm.with_roots(|_| {
            entered = true;
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!entered);
    assert_eq!(shape(&vm), before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn gc_trace_storage_heap_error_returns_the_empty_cache_and_releases_lease() {
    let mut vm = vm();
    root_batch(&mut vm);
    collect(&mut vm);
    let before = shape(&vm);
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(
        vm.with_roots::<()>(|_| Err(HeapError::IdExhausted)),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(shape(&vm), before);
    assert!(vm.gc_trace_cache.roots.is_empty() && vm.gc_trace_cache.edges.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, used);
    collect(&mut vm);
}

#[cfg_attr(test, test)]
fn gc_trace_storage_joint_replacement_refusal_preserves_both_cached_allocations() {
    let mut vm = vm();
    vm.execute_script(&script(
        "globalThis.gate=Promise.withResolvers();gate.promise.then(()=>0);0;",
    ))
    .unwrap();
    let gate = vm.lookup_global_name("gate").unwrap().unwrap();
    let promise = vm
        .get_property(&gate, &"promise".into())
        .unwrap()
        .object_id()
        .unwrap();
    let PromiseReaction::Then(reaction) = &vm.promises[&promise].reactions[0] else {
        panic!("then reaction")
    };
    let target = reaction.target.clone();
    let handler = reaction.on_fulfilled.clone();
    vm.promises
        .get_mut(&promise)
        .unwrap()
        .reactions
        .extend((0..32).map(|_| {
            PromiseReaction::Then(PromiseThenReaction {
                target: target.clone(),
                on_fulfilled: handler.clone(),
                on_rejected: Value::Undefined,
            })
        }));
    root_batch(&mut vm);
    // Warm-up for object creation may have grown edges, so add a second
    // finite suffix after the last heap operation to require joint growth.
    vm.promises
        .get_mut(&promise)
        .unwrap()
        .reactions
        .extend((0..64).map(|_| {
            PromiseReaction::Then(PromiseThenReaction {
                target: target.clone(),
                on_fulfilled: handler.clone(),
                on_rejected: Value::Undefined,
            })
        }));
    let before = shape(&vm);
    let mut root_count = 0;
    vm.visit_execution_roots(|_| root_count += 1);
    let root_bytes = replacement_capacity(&vm.gc_trace_cache.roots, root_count).unwrap()
        * std::mem::size_of::<ObjectId>();
    let _peer = leave_room(&mut vm, root_bytes);
    assert_eq!(
        vm.with_roots(|_| Ok(())),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(shape(&vm), before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert!(vm.gc_trace_cache.roots.is_empty() && vm.gc_trace_cache.edges.is_empty());
}

#[cfg_attr(test, test)]
fn gc_trace_storage_active_dynamic_shadow_and_parameter_environment_survive_gc() {
    let mut vm = vm();
    let current = object(&mut vm);
    let shadow = object(&mut vm);
    let parameter = object(&mut vm);
    vm.dynamic_eval_bindings.insert(
        "binding".into(),
        DynamicEvalBinding {
            cell: current,
            shadowed_cells: vec![shadow],
        },
    );
    vm.parameter_eval_env = Some(parameter);
    vm.stack.clear();
    collect(&mut vm);
    assert!(vm.heap.contains(current) && vm.heap.contains(shadow) && vm.heap.contains(parameter));
    vm.dynamic_eval_bindings.clear();
    vm.parameter_eval_env = None;
    collect(&mut vm);
    assert!(
        !vm.heap.contains(current) && !vm.heap.contains(shadow) && !vm.heap.contains(parameter)
    );
}

#[cfg_attr(test, test)]
fn gc_trace_storage_live_promise_result_keeps_its_owner_edge() {
    let mut vm = vm();
    vm.execute_script(&script("globalThis.owner=Promise.resolve({answer:42});0;"))
        .unwrap();
    let owner = vm
        .lookup_global_name("owner")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let PromiseStatus::Fulfilled(value) = &vm.promises[&owner].status else {
        panic!("fulfilled promise")
    };
    let result = value.object_id().unwrap();
    collect(&mut vm);
    assert!(vm.heap.contains(owner) && vm.heap.contains(result));
    vm.execute_script(&script("owner=null;0;")).unwrap();
    collect(&mut vm);
    assert!(!vm.heap.contains(owner) && !vm.heap.contains(result));
    assert!(!vm.promises.contains_key(&owner));
}

#[cfg_attr(test, test)]
fn gc_trace_storage_unreachable_promise_result_cycle_does_not_become_a_root() {
    let mut vm = vm();
    let owner=vm.execute_script(&script("(()=>{let gate=Promise.withResolvers();gate.resolve({owner:gate.promise});return gate.promise;})();")).unwrap().object_id().unwrap();
    let PromiseStatus::Fulfilled(value) = &vm.promises[&owner].status else {
        panic!("fulfilled promise")
    };
    let result = value.object_id().unwrap();
    vm.execute_script(&script("0;")).unwrap();
    collect(&mut vm);
    assert!(!vm.heap.contains(owner) && !vm.heap.contains(result));
    assert!(!vm.promises.contains_key(&owner));
}

#[cfg_attr(test, test)]
fn gc_trace_storage_retained_async_invocation_keeps_await_edges() {
    let mut vm = vm();
    vm.execute_script(&script("globalThis.gate=Promise.withResolvers();globalThis.result=(async()=>{const value={answer:42};await gate.promise;return value;})();0;")).unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    collect(&mut vm);
    assert_eq!(vm.async_continuations.len(), 1);
    vm.execute_script(&script("gate.resolve();0;")).unwrap();
    vm.run_promise_jobs_bounded(8).unwrap();
    let promise = vm
        .lookup_global_name("result")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let PromiseStatus::Fulfilled(value) = &vm.promises[&promise].status else {
        panic!("fulfilled invocation")
    };
    let value = value.clone();
    assert_eq!(
        vm.get_property(&value, &"answer".into()).unwrap(),
        Value::Number(42.0)
    );
    let id = value.object_id().unwrap();
    vm.execute_script(&script("gate=null;result=null;0;"))
        .unwrap();
    collect(&mut vm);
    assert!(!vm.heap.contains(id));
    assert!(vm.async_continuations.is_empty());
}

#[cfg_attr(test, test)]
fn gc_trace_storage_unreachable_async_await_cycle_is_collectible() {
    let mut vm = vm();
    vm.execute_script(&script(
        "(()=>{let gate=Promise.withResolvers();(async()=>{await gate.promise;})();})();0;",
    ))
    .unwrap();
    collect(&mut vm);
    assert!(vm.async_continuations.is_empty());
}

#[cfg_attr(test, test)]
fn gc_trace_storage_disposal_values_follow_live_stack_owners() {
    for constructor in ["DisposableStack", "AsyncDisposableStack"] {
        let mut vm = vm();
        vm.execute_script(&script(&format!("globalThis.owner=new {constructor}();owner.adopt({{answer:42}},value=>value.answer);0;"))).unwrap();
        let owner = vm
            .lookup_global_name("owner")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        let state = vm
            .disposable_stacks
            .get(&owner)
            .or_else(|| vm.async_disposable_stacks.get(&owner))
            .unwrap();
        let value = state.resources[0]
            .argument
            .as_ref()
            .unwrap()
            .object_id()
            .unwrap();
        collect(&mut vm);
        assert!(vm.heap.contains(value));
        vm.execute_script(&script("owner=null;0;")).unwrap();
        collect(&mut vm);
        assert!(!vm.heap.contains(value));
        assert!(vm.disposable_stacks.is_empty() && vm.async_disposable_stacks.is_empty());
    }
}

#[cfg_attr(test, test)]
fn gc_trace_storage_discarded_child_releases_its_cached_native_capacity() {
    let mut vm = vm();
    vm.execute_script(&script("globalThis.realm=new ShadowRealm();0;"))
        .unwrap();
    let realm = vm
        .lookup_global_name("realm")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let record = vm.shadow_realms.get(&realm).unwrap().clone();
    let weak = Rc::downgrade(&record.vm);
    let child_bytes = {
        let mut child = record.vm.borrow_mut();
        root_batch(&mut child);
        collect(&mut child);
        let bytes = child.gc_trace_cache.retained_bytes();
        child.stack.clear();
        child.reconcile_scheduling_ownership();
        bytes
    };
    assert!(child_bytes >= 96 * std::mem::size_of::<ObjectId>());
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    drop(record);
    vm.execute_script(&script("realm=null;0;")).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
    assert!(before - vm.host_scheduling_usage().unwrap().payload_bytes >= child_bytes);
}

#[cfg_attr(test, test)]
fn gc_trace_storage_host_neutral_cache_attaches_when_bounded_mode_starts() {
    let mut vm = Vm::default();
    root_batch(&mut vm);
    collect(&mut vm);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    let bytes = vm.gc_trace_cache.retained_bytes();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.gc_trace_cache = Default::default();
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        before - vm.host_scheduling_usage().unwrap().payload_bytes,
        bytes
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_gc_trace_storage_contracts() {
        gc_trace_storage_root_refusal_precedes_heap_operation();
        gc_trace_storage_collection_refusal_preserves_heap_and_frame();
        gc_trace_storage_edge_refusal_precedes_collection();
        gc_trace_storage_cached_and_heap_held_capacity_has_one_native_owner();
        gc_trace_storage_warm_capacity_reuses_addresses_at_full_scalar_quota();
        gc_trace_storage_terminal_cleanup_reuses_admitted_capacity();
        gc_trace_storage_reuse_refuses_newly_over_budget_ownership_before_heap_work();
        gc_trace_storage_heap_error_returns_the_empty_cache_and_releases_lease();
        gc_trace_storage_joint_replacement_refusal_preserves_both_cached_allocations();
        gc_trace_storage_active_dynamic_shadow_and_parameter_environment_survive_gc();
        gc_trace_storage_live_promise_result_keeps_its_owner_edge();
        gc_trace_storage_unreachable_promise_result_cycle_does_not_become_a_root();
        gc_trace_storage_retained_async_invocation_keeps_await_edges();
        gc_trace_storage_unreachable_async_await_cycle_is_collectible();
        gc_trace_storage_disposal_values_follow_live_stack_owners();
        gc_trace_storage_discarded_child_releases_its_cached_native_capacity();
        gc_trace_storage_host_neutral_cache_attaches_when_bounded_mode_starts();
    }
}
