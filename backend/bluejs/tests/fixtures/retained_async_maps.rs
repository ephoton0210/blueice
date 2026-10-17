// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small awaits and in-memory host callbacks exercise retained control maps.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}
#[cfg_attr(test, test)]
fn retained_async_maps_last_async_resume_releases_empty_map_backing() {
    let mut vm = page();
    execute(&mut vm, "var seen=0;var gate=Promise.withResolvers();var done=(async()=>{await gate.promise;seen=42;})();0;");
    assert_eq!(vm.async_continuations.len(), 1);
    execute(&mut vm, "gate.resolve(7);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.async_continuations.capacity(), 0);
    assert_eq!(
        vm.lookup_global_name("seen").unwrap(),
        Some(Value::Number(42.0))
    );
}
#[cfg_attr(test, test)]
fn retained_async_maps_orphan_gc_releases_empty_async_map_backing() {
    let mut vm = page();
    execute(
        &mut vm,
        "globalThis.gate=new Promise(()=>{});globalThis.done=(async()=>{await gate;})();0;",
    );
    assert_eq!(vm.async_continuations.len(), 1);
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap.set(global, "gate", Value::Null).unwrap();
    vm.heap.set(global, "done", Value::Null).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.async_continuations.capacity(), 0);
}
fn suspended_module() -> Vm {
    let mut vm = page();
    execute(&mut vm, "globalThis.gate=Promise.withResolvers();0;");
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
#[cfg_attr(test, test)]
fn retained_async_maps_last_module_resume_releases_empty_map_backing() {
    let mut vm = suspended_module();
    execute(&mut vm, "gate.resolve(7);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(vm.module_continuations.is_empty());
    assert_eq!(vm.module_continuations.capacity(), 0);
    assert!(vm.module_graph.as_ref().unwrap().linked["entry"].evaluated);
}

type Tokens = Rc<RefCell<Vec<HostPromiseToken>>>;
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
struct Mock {
    calls: Tokens,
    canceled: Tokens,
    cancellation_usage: Rc<RefCell<Vec<usize>>>,
    view: Vm,
}
impl HostAsyncFunction for Mock {
    fn start(
        &mut self,
        token: HostPromiseToken,
        _args: &[HostValue],
    ) -> Result<(), HostFunctionError> {
        self.calls.borrow_mut().push(token);
        Ok(())
    }
    fn cancel(&mut self, token: HostPromiseToken) {
        self.canceled.borrow_mut().push(token);
        self.cancellation_usage
            .borrow_mut()
            .push(self.view.host_scheduling_snapshot().payload_bytes);
    }
}
struct HostWork {
    vm: Vm,
    calls: Tokens,
    canceled: Tokens,
    cancellation_usage: Rc<RefCell<Vec<usize>>>,
    roots: Vec<RootId>,
    objects: Vec<ObjectId>,
}
impl HostWork {
    fn new(count: usize) -> Self {
        let mut vm = page();
        vm.remaining_instructions = vm.config.instruction_budget;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let canceled = Rc::new(RefCell::new(Vec::new()));
        let cancellation_usage = Rc::new(RefCell::new(Vec::new()));
        let mock = Mock {
            calls: calls.clone(),
            canceled: canceled.clone(),
            cancellation_usage: cancellation_usage.clone(),
            view: observer(&vm),
        };
        vm.install_host_async_function("wait", 0, HostTaskSource::UserInteraction, mock)
            .unwrap();
        let mut roots = Vec::new();
        let mut objects = Vec::new();
        for _ in 0..count {
            let object = vm
                .host_async_function_call(0, Value::Undefined, &[], false)
                .unwrap()
                .object_id()
                .unwrap();
            objects.push(object);
            roots.push(vm.heap.root(object).unwrap());
        }
        vm.with_roots(|heap| heap.try_collect_major()).unwrap();
        Self {
            vm,
            calls,
            canceled,
            cancellation_usage,
            roots,
            objects,
        }
    }
    fn finish(&mut self, index: usize) {
        let token = self.calls.borrow()[index];
        self.vm
            .queue_host_promise_completion(
                token,
                HostPromiseCompletion::Resolve(HostValue::Number(index as f64)),
            )
            .unwrap();
        self.vm.run_own_browser_task(0).unwrap();
    }
    fn sparse() -> Self {
        let mut work = Self::new(7);
        for index in 0..6 {
            work.finish(index);
        }
        assert_eq!(work.vm.host_promises.pending.len(), 1);
        assert_eq!(work.vm.host_promises.pending.capacity(), 7);
        work
    }
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn pending_bytes(vm: &Vm) -> usize {
    vm.host_promises.pending.retained_bytes()
}
fn cache_bytes(vm: &Vm) -> usize {
    vm.promises.retained_bytes()
        + pending_bytes(vm)
        + vm.async_continuations.retained_bytes()
        + vm.module_continuations.retained_bytes()
}
fn cached(vm: &mut Vm) {
    vm.global("Promise").unwrap();
    vm.promises.reserve(1);
    vm.host_promises.pending.reserve(1);
    vm.async_continuations.reserve(1);
    vm.module_continuations.reserve(1);
    vm.synchronize_scheduling_usage().unwrap();
}
fn new_host(vm: &mut Vm) -> Result<Value, RuntimeError> {
    vm.host_async_function_call(0, Value::Undefined, &[], false)
}

#[cfg_attr(test, test)]
fn retained_async_maps_host_completion_keeps_backing_until_the_last_wait() {
    let mut work = HostWork::new(2);
    let bytes = pending_bytes(&work.vm);
    let roots = work.vm.heap.root_storage_state().0;
    work.finish(0);
    assert_eq!(work.vm.host_promises.pending.len(), 1);
    assert_eq!(pending_bytes(&work.vm), bytes);
    work.finish(1);
    assert_eq!(pending_bytes(&work.vm), 0);
    assert_eq!(work.vm.heap.root_storage_state().0, roots - 2);
    for root in work.roots.drain(..) {
        work.vm.heap.unroot(root).unwrap();
    }
    work.vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(work.objects.iter().all(|id| !work.vm.heap.contains(*id)));
    assert!(work.canceled.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn retained_async_maps_host_gc_skips_compaction_without_candidate_headroom() {
    let mut work = HostWork::sparse();
    let bytes = pending_bytes(&work.vm);
    let _peer = leave_room(&mut work.vm, 0);
    work.vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(pending_bytes(&work.vm), bytes);
    assert!(work.vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn retained_async_maps_host_gc_exact_candidate_preserves_live_tokens() {
    let mut work = HostWork::sparse();
    let old = pending_bytes(&work.vm);
    let bytes = 3 * std::mem::size_of::<(HostPromiseToken, host_promises::PendingHostPromise)>();
    let peer = leave_room(&mut work.vm, bytes);
    work.vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(pending_bytes(&work.vm), bytes);
    assert_eq!(
        work.vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
    drop(peer);
    work.finish(6);
    assert_eq!(pending_bytes(&work.vm), 0);
}
#[cfg_attr(test, test)]
fn retained_async_maps_host_gc_reported_capacity_refusal_is_not_terminal() {
    let mut work = HostWork::sparse();
    let old = pending_bytes(&work.vm);
    let bytes =
        3 * std::mem::size_of::<(HostPromiseToken, host_promises::PendingHostPromise)>() - 1;
    let _peer = leave_room(&mut work.vm, bytes);
    work.vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(pending_bytes(&work.vm), old);
    assert!(work.vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn retained_async_maps_managed_registration_gc_keeps_staged_wait_backing_stable() {
    let mut work = HostWork::sparse();
    let old = pending_bytes(&work.vm);
    let mut garbage = Vec::new();
    for _ in 0..4 {
        garbage.push(work.vm.heap.alloc_object(None).unwrap());
    }
    work.vm.heap.allow_only(0);
    let before = work.vm.heap.stats().major_collections;
    new_host(&mut work.vm).unwrap();
    assert!(work.vm.heap.stats().major_collections > before);
    assert!(garbage.iter().all(|id| !work.vm.heap.contains(*id)));
    assert_eq!(pending_bytes(&work.vm), old);
    assert_eq!(work.vm.host_promises.pending.len(), 2);
    assert_eq!(work.calls.borrow().len(), 8);
}
#[cfg_attr(test, test)]
fn retained_async_maps_failed_registration_gc_reclaims_unstaged_empty_caches() {
    let mut work = HostWork::new(7);
    work.vm.async_continuations.reserve(1);
    work.vm.module_continuations.reserve(1);
    work.vm.synchronize_scheduling_usage().unwrap();
    let old = pending_bytes(&work.vm);
    let roots = work.vm.heap.root_storage_state();
    let serial = work
        .vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .task_serial_state();
    work.vm.heap.allow_only(0);
    assert!(matches!(
        new_host(&mut work.vm),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(pending_bytes(&work.vm), old);
    assert_eq!(work.vm.heap.root_storage_state(), roots);
    assert_eq!(
        work.vm
            .host_scheduling
            .as_ref()
            .unwrap()
            .task_serial_state(),
        serial
    );
    assert_eq!(work.vm.async_continuations.retained_bytes(), 0);
    assert_eq!(work.vm.module_continuations.retained_bytes(), 0);
    assert_eq!(work.calls.borrow().len(), 7);
}
#[cfg_attr(test, test)]
fn retained_async_maps_admission_refusal_preserves_wait_ledger_roots_and_serial() {
    let mut work = HostWork::new(7);
    let old = pending_bytes(&work.vm);
    let roots = work.vm.heap.root_storage_state();
    let serial = work
        .vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .task_serial_state();
    let _peer = leave_room(&mut work.vm, 0);
    assert_eq!(new_host(&mut work.vm), Err(RuntimeError::SchedulingLimit));
    assert_eq!(pending_bytes(&work.vm), old);
    assert_eq!(work.vm.heap.root_storage_state(), roots);
    assert_eq!(
        work.vm
            .host_scheduling
            .as_ref()
            .unwrap()
            .task_serial_state(),
        serial
    );
    assert_eq!(work.calls.borrow().len(), 7);
}
#[cfg_attr(test, test)]
fn retained_async_maps_dropped_wait_candidate_keeps_its_lease_after_vm_drop() {
    let work = HostWork::new(7);
    let view = observer(&work.vm);
    let canceled = work.canceled.clone();
    let prepared = promise_queue_storage::PreparedMap::for_insert(
        &work.vm.host_promises.pending,
        work.vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let bytes = 14 * std::mem::size_of::<(HostPromiseToken, host_promises::PendingHostPromise)>();
    drop(work);
    assert_eq!(canceled.borrow().len(), 7);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn retained_async_maps_vm_drop_cancels_once_while_wait_backing_is_charged() {
    let work = HostWork::new(2);
    let bytes = pending_bytes(&work.vm);
    let view = observer(&work.vm);
    let canceled = work.canceled.clone();
    let observations = work.cancellation_usage.clone();
    let tokens = work.calls.borrow().clone();
    drop(work);
    assert_eq!(canceled.borrow().len(), 2);
    assert!(tokens.iter().all(|token| canceled.borrow().contains(token)));
    assert!(observations.borrow().iter().all(|used| *used >= bytes));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn retained_async_maps_partial_async_resume_preserves_the_remaining_frame() {
    let mut vm = page();
    execute(&mut vm, "var first=Promise.withResolvers();var second=Promise.withResolvers();var outputs=[(async()=>{await first.promise;return 1;})(),(async()=>{await second.promise;return 2;})()];0;");
    let old = vm.async_continuations.retained_bytes();
    execute(&mut vm, "first.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    assert_eq!(vm.async_continuations.retained_bytes(), old);
    execute(&mut vm, "second.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.async_continuations.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn retained_async_maps_chained_module_awaits_publish_fresh_owned_backing() {
    let mut vm = page();
    execute(
        &mut vm,
        "var first=Promise.withResolvers();var second=Promise.withResolvers();0;",
    );
    let code = crate::compile_module(
        &crate::parse_module("await first.promise;await second.promise;export const answer=42;")
            .unwrap(),
    )
    .unwrap();
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), code)]));
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
    let old = vm.module_continuations.retained_bytes();
    let serial = *vm.module_continuations.keys().next().unwrap();
    execute(&mut vm, "first.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.module_continuations.len(), 1);
    assert_eq!(vm.module_continuations.retained_bytes(), old);
    assert!(*vm.module_continuations.keys().next().unwrap() > serial);
    execute(&mut vm, "second.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.module_continuations.retained_bytes(), 0);
    assert!(vm.module_graph.as_ref().unwrap().linked["entry"].evaluated);
}
#[cfg_attr(test, test)]
fn retained_async_maps_preparation_guard_defers_all_four_table_replacements() {
    let mut vm = page();
    cached(&mut vm);
    let bytes = cache_bytes(&vm);
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.preserve_promise_table_capacity = true;
    vm.reclaim_scheduling_maps();
    assert_eq!(cache_bytes(&vm), bytes);
    vm.preserve_promise_table_capacity = false;
    vm.reclaim_scheduling_maps();
    assert_eq!(cache_bytes(&vm), 0);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        used - bytes
    );
}
#[cfg_attr(test, test)]
fn retained_async_maps_terminal_cleanup_releases_empty_backing_without_admission() {
    let mut vm = page();
    cached(&mut vm);
    let bytes = cache_bytes(&vm);
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    vm.reclaim_scheduling_maps();
    assert_eq!(cache_bytes(&vm), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used - bytes);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn retained_async_maps_managed_guard_reclaims_collected_orphans_after_publication() {
    let mut work = HostWork::new(0);
    execute(
        &mut work.vm,
        "globalThis.gate=new Promise(()=>{});globalThis.done=(async()=>{await gate;})();0;",
    );
    let global = work.vm.global("globalThis").unwrap().object_id().unwrap();
    work.vm.heap.set(global, "gate", Value::Null).unwrap();
    work.vm.heap.set(global, "done", Value::Null).unwrap();
    work.vm.heap.allow_only(0);
    new_host(&mut work.vm).unwrap();
    assert!(work.vm.async_continuations.is_empty());
    assert_eq!(work.vm.async_continuations.retained_bytes(), 0);
    assert_eq!(work.vm.host_promises.pending.len(), 1);
}
#[cfg_attr(test, test)]
fn retained_async_maps_sparse_async_frames_compact_without_losing_the_live_owner() {
    let mut vm = page();
    execute(&mut vm, "var gates=[];var outputs=[];for(let i=0;i<7;i++){let gate=Promise.withResolvers();gates.push(gate);outputs.push((async p=>{await p;return 1;})(gate.promise));}0;");
    assert_eq!(vm.async_continuations.len(), 7);
    execute(&mut vm, "for(let i=0;i<6;i++){gates[i].resolve(0);}0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    let old = vm.async_continuations.retained_bytes();
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.async_continuations.retained_bytes(), old);
    drop(peer);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.async_continuations.capacity(), 3);
    assert!(vm.async_continuations.retained_bytes() < old);
    execute(&mut vm, "gates[6].resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.async_continuations.retained_bytes(), 0);
}
#[derive(Clone, Default)]
struct CollisionHasher;
impl std::hash::Hasher for CollisionHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _bytes: &[u8]) {}
}
#[cfg_attr(test, test)]
fn retained_async_maps_removal_keeps_tombstone_backing_until_explicit_reclaim() {
    let mut map = retained_control_map::RetainedControlMap::<
        u64,
        u64,
        std::hash::BuildHasherDefault<CollisionHasher>,
    >::default();
    map.reserve(28);
    for key in 0..28 {
        map.insert(key, key);
    }
    let old = map.retained_bytes();
    assert_eq!(map.remove(&12), Some(12));
    assert!(host_scheduling::map_slots(&map) < old);
    for key in 0..28 {
        map.remove(&key);
    }
    assert!(map.is_empty());
    assert_eq!(map.retained_bytes(), old);
    assert!(map.reclaim(None).is_none());
    assert_eq!(map.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn retained_async_maps_refusal_without_gc_preserves_existing_empty_caches() {
    let mut work = HostWork::new(0);
    cached(&mut work.vm);
    let bytes = cache_bytes(&work.vm);
    let before = work.vm.heap.stats();
    let _peer = leave_room(&mut work.vm, 0);
    assert_eq!(
        work.vm
            .host_async_function_call(0, Value::Undefined, &[Value::Number(7.0)], false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(cache_bytes(&work.vm), bytes);
    assert_eq!(work.vm.heap.stats(), before);
    assert!(work.calls.borrow().is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_retained_async_maps_contracts() {
        retained_async_maps_last_async_resume_releases_empty_map_backing();
        retained_async_maps_orphan_gc_releases_empty_async_map_backing();
        retained_async_maps_last_module_resume_releases_empty_map_backing();
        retained_async_maps_host_completion_keeps_backing_until_the_last_wait();
        retained_async_maps_host_gc_skips_compaction_without_candidate_headroom();
        retained_async_maps_host_gc_exact_candidate_preserves_live_tokens();
        retained_async_maps_host_gc_reported_capacity_refusal_is_not_terminal();
        retained_async_maps_managed_registration_gc_keeps_staged_wait_backing_stable();
        retained_async_maps_failed_registration_gc_reclaims_unstaged_empty_caches();
        retained_async_maps_admission_refusal_preserves_wait_ledger_roots_and_serial();
        retained_async_maps_dropped_wait_candidate_keeps_its_lease_after_vm_drop();
        retained_async_maps_vm_drop_cancels_once_while_wait_backing_is_charged();
        retained_async_maps_partial_async_resume_preserves_the_remaining_frame();
        retained_async_maps_chained_module_awaits_publish_fresh_owned_backing();
        retained_async_maps_preparation_guard_defers_all_four_table_replacements();
        retained_async_maps_terminal_cleanup_releases_empty_backing_without_admission();
        retained_async_maps_managed_guard_reclaims_collected_orphans_after_publication();
        retained_async_maps_sparse_async_frames_compact_without_losing_the_live_owner();
        retained_async_maps_removal_keeps_tombstone_backing_until_explicit_reclaim();
        retained_async_maps_refusal_without_gc_preserves_existing_empty_caches();
    }
}
