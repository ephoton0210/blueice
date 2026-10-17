// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny batches and numerical peers exercise import delivery lifetime.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    vm
}
fn fixture() -> (Vm, Vec<ObjectId>, ObjectId) {
    let mut vm = page();
    let namespace = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let root = vm.heap.root(namespace).unwrap();
    let mut targets = Vec::new();
    for names in [vec!["done".into()], vec!["done".into(), "later".into()]] {
        let promise = vm.new_promise().unwrap();
        vm.enqueue_deferred_import_waiter(promise, namespace, names)
            .unwrap();
        targets.push(promise);
    }
    vm.heap.unroot(root).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    (vm, targets, namespace)
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn pending(vm: &Vm, targets: &[ObjectId]) {
    assert_eq!(vm.deferred_import_waiters.len(), 2);
    assert!(vm
        .deferred_import_waiters
        .iter()
        .all(|waiter| waiter.pending.contains("done")));
    assert!(targets
        .iter()
        .all(|target| matches!(vm.promises[target].status, PromiseStatus::Pending)));
}
#[cfg_attr(test, test)]
fn import_delivery_native_refusal_preserves_ready_and_partial_waiters() {
    let (mut vm, targets, _) = fixture();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.settle_dynamic_import_waiters("done", &mut HashMap::new()),
        Err(RuntimeError::SchedulingLimit)
    );
    pending(&vm, &targets);
}
#[cfg_attr(test, test)]
fn import_delivery_rejection_native_refusal_preserves_both_waiter_kinds() {
    let (mut vm, mut targets, _) = fixture();
    let eager = vm.new_promise().unwrap();
    vm.enqueue_module_import_waiter("done".into(), eager)
        .unwrap();
    targets.push(eager);
    let error = vm
        .retain_module_error(&Value::Number(7.0), ThrowOrigin::default())
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.reject_dynamic_import_waiters("done", error),
        Err(RuntimeError::SchedulingLimit)
    );
    pending(&vm, &targets);
    assert_eq!(vm.module_import_waiters["done"], [eager]);
}
#[cfg_attr(test, test)]
fn import_delivery_root_refusal_preserves_dependency_membership_and_targets() {
    let (mut vm, targets, _) = fixture();
    vm.heap.allow_root_registrations(0);
    let roots = vm.heap.root_storage_state();
    assert_eq!(
        vm.settle_dynamic_import_waiters("done", &mut HashMap::new()),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    pending(&vm, &targets);
    assert_eq!(vm.heap.root_storage_state(), roots);
}

fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn release(vm: &mut Vm, batch: &mut PreparedImportDelivery) {
    for root in batch.roots.drain(..) {
        vm.release_root(root);
    }
    vm.reconcile_scheduling_ownership();
}
fn retained(batch: &PreparedImportDelivery) -> usize {
    batch
        .entries
        .iter()
        .fold(batch.slots(), |bytes, entry| bytes + entry.payload_bytes())
}
fn module_error(vm: &mut Vm) -> ModuleEvaluationError {
    vm.retain_module_error(
        &Value::String("tiny".into()),
        ThrowOrigin {
            source: Some(crate::diagnostics::DiagnosticSourceLocation {
                filename: "tiny.js".into(),
                line: 3,
                column: 5,
            }),
            ..Default::default()
        },
    )
    .unwrap()
}
#[cfg_attr(test, test)]
fn import_delivery_partial_only_completion_reclaims_sparse_dependencies() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.enqueue_deferred_import_waiter(promise, promise, (0..7).map(|i| format!("m{i}")).collect())
        .unwrap();
    for index in 0..6 {
        vm.settle_dynamic_import_waiters(&format!("m{index}"), &mut HashMap::new())
            .unwrap();
    }
    assert_eq!(vm.deferred_import_waiters.len(), 1);
    assert_eq!(vm.deferred_import_waiters[0].pending.capacity(), 3);
    assert!(vm.deferred_import_waiters[0].pending.contains("m6"));
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
}
#[cfg_attr(test, test)]
fn import_delivery_unrelated_completion_needs_no_snapshot_or_root_storage() {
    let (mut vm, targets, _) = fixture();
    vm.heap.allow_root_registrations(0);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    vm.settle_dynamic_import_waiters("other", &mut HashMap::new())
        .unwrap();
    pending(&vm, &targets);
    assert_eq!(vm.heap.root_storage_state(), roots);
}
#[cfg_attr(test, test)]
fn import_delivery_whole_root_batch_refusal_can_retry_without_consuming_a_prefix() {
    let (mut vm, targets, namespace) = fixture();
    let counter = vm.heap.root_storage_state().2;
    vm.heap.allow_root_registrations(1);
    let roots = vm.heap.root_storage_state();
    assert_eq!(
        vm.settle_dynamic_import_waiters("done", &mut HashMap::new()),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    pending(&vm, &targets);
    vm.heap.set_root_counter_for_test(counter);
    vm.settle_dynamic_import_waiters("done", &mut HashMap::new())
        .unwrap();
    assert!(
        matches!(vm.promises[&targets[0]].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == namespace)
    );
    assert!(matches!(
        vm.promises[&targets[1]].status,
        PromiseStatus::Pending
    ));
    assert!(!vm.deferred_import_waiters[0].pending.contains("done"));
}
#[cfg_attr(test, test)]
fn import_delivery_abandoned_preparation_releases_roots_and_temporary_native_storage() {
    let (mut vm, targets, _) = fixture();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let old_root_bytes = vm.heap.root_storage_bytes();
    let root_count = vm.heap.root_storage_state().0;
    let mut batch = vm.prepare_import_delivery("done", None, None).unwrap();
    assert_eq!(batch.entries.len(), 1);
    assert_eq!(batch.roots.len(), 2);
    release(&mut vm, &mut batch);
    drop(batch);
    assert_eq!(vm.heap.root_storage_state().0, root_count);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        before + vm.heap.root_storage_bytes() - old_root_bytes
    );
    pending(&vm, &targets);
}
#[cfg_attr(test, test)]
fn import_delivery_owned_rejection_snapshot_keeps_its_native_lease_after_vm_drop() {
    let (mut vm, _, _) = fixture();
    let view = observer(&vm);
    let error = module_error(&mut vm);
    let batch = vm
        .prepare_import_delivery("done", None, Some(&error))
        .unwrap();
    let bytes = retained(&batch);
    assert_eq!(batch.entries.len(), 2);
    drop(error);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(
        batch.entries[0].rejection.as_ref().unwrap().0,
        Value::String("tiny".into())
    );
    drop(batch);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn import_delivery_one_byte_short_metadata_allowance_keeps_membership_and_roots() {
    let (mut vm, targets, _) = fixture();
    let mut batch = vm.prepare_import_delivery("done", None, None).unwrap();
    let bytes = retained(&batch);
    release(&mut vm, &mut batch);
    drop(batch);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, bytes - 1);
    assert_eq!(
        vm.settle_dynamic_import_waiters("done", &mut HashMap::new()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    pending(&vm, &targets);
}
#[cfg_attr(test, test)]
fn import_delivery_mixed_rejection_preserves_reason_origin_and_consumes_all_selected_waiters() {
    let (mut vm, mut targets, _) = fixture();
    let eager = vm.new_promise().unwrap();
    vm.enqueue_module_import_waiter("done".into(), eager)
        .unwrap();
    targets.push(eager);
    let error = module_error(&mut vm);
    vm.reject_dynamic_import_waiters("done", error.clone())
        .unwrap();
    assert!(vm.deferred_import_waiters.is_empty() && vm.module_import_waiters.is_empty());
    for target in targets {
        assert!(
            matches!(&vm.promises[&target].status, PromiseStatus::Rejected(value) if *value == error.value)
        );
        assert_eq!(
            vm.promises[&target].rejection_origin.source,
            error.origin.source
        );
    }
    assert!(vm.promise_origin_override.is_none());
    assert_eq!(vm.active_scheduling_bytes, 0);
}
fn without_heap_storage(vm: &Vm) -> usize {
    vm.host_scheduling_snapshot().payload_bytes
        - vm.heap.root_storage_bytes()
        - vm.heap.collector_storage_bytes()
}
#[cfg_attr(test, test)]
fn import_delivery_rejection_inputs_transfer_once_to_the_ordinary_settlement_owner() {
    let (mut vm, _, _) = fixture();
    let error = module_error(&mut vm);
    vm.reject_dynamic_import_waiters("done", error).unwrap();
    let (mut control, targets, _) = fixture();
    let error = module_error(&mut control);
    control.deferred_import_waiters.clear();
    control.reclaim_import_waiter_storage();
    for target in targets {
        control
            .reject_promise_with_origin(target, error.value.clone(), error.origin.clone())
            .unwrap();
    }
    drop(error);
    control.prune_module_error_origins();
    vm.prune_module_error_origins();
    assert_eq!(without_heap_storage(&vm), without_heap_storage(&control));
}
fn getter_fixture(stop: bool) -> (Vm, Vec<ObjectId>, ObjectId) {
    let mut vm = page();
    if stop {
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        vm.install_host_function("stop", 0, move |_args: &[HostValue]| {
            scope.fail();
            Err(HostFunctionError::new("declined"))
        })
        .unwrap();
    }
    execute(
        &mut vm,
        if stop {
            "globalThis.ns={get then(){stop();return undefined;}};globalThis.a=new Promise(()=>{});globalThis.b=new Promise(()=>{});0;"
        } else {
            "globalThis.marker=null;globalThis.ns={get then(){marker={};return undefined;}};globalThis.a=new Promise(()=>{});globalThis.b=new Promise(()=>{});0;"
        },
    );
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let namespace = vm.heap.get(global, "ns").unwrap().object_id().unwrap();
    let mut targets = Vec::new();
    for name in ["a", "b"] {
        let promise = vm.heap.get(global, name).unwrap().object_id().unwrap();
        vm.enqueue_deferred_import_waiter(promise, namespace, vec!["done".into()])
            .unwrap();
        targets.push(promise);
        vm.heap.set(global, name, Value::Null).unwrap();
    }
    vm.heap.set(global, "ns", Value::Null).unwrap();
    (vm, targets, namespace)
}
#[cfg_attr(test, test)]
fn import_delivery_committed_receiver_failure_preserves_the_unattempted_suffix() {
    let (mut vm, targets, _) = getter_fixture(true);
    let view = observer(&vm);
    assert_eq!(
        vm.settle_dynamic_import_waiters("done", &mut HashMap::new()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.deferred_import_waiters.len(), 1);
    assert_eq!(vm.deferred_import_waiters[0].promise, targets[1]);
    assert!(vm.deferred_import_waiters[0].pending.contains("done"));
    assert!(matches!(
        vm.promises[&targets[1]].status,
        PromiseStatus::Pending
    ));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn import_delivery_committed_reaction_admission_failure_keeps_the_suffix_queued() {
    let (mut vm, targets, _) = fixture();
    vm.promise_then(&Value::Object(targets[0]), &[]).unwrap();
    let error = vm
        .retain_module_error(&Value::Number(7.0), ThrowOrigin::default())
        .unwrap();
    let batch = vm
        .prepare_import_delivery("done", None, Some(&error))
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.dispatch_import_delivery("done", batch, true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.deferred_import_waiters.len(), 1);
    assert_eq!(vm.deferred_import_waiters[0].promise, targets[1]);
    assert!(matches!(
        vm.promises[&targets[0]].status,
        PromiseStatus::Pending
    ));
    // Import delivery consumed its current attempt; the nested settlement
    // transaction refused before publishing its status or reaction jobs.
    assert_eq!(vm.promises[&targets[0]].reactions.len(), 1);
    assert!(matches!(
        vm.promises[&targets[1]].status,
        PromiseStatus::Pending
    ));
}
#[cfg_attr(test, test)]
fn import_delivery_getter_gc_keeps_later_targets_and_namespaces_alive() {
    let (mut vm, targets, namespace) = getter_fixture(false);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let prototype = vm.object_prototype;
    let mut garbage = Vec::new();
    for _ in 0..4 {
        let state = vm.new_web_event_state(2, "unused".into());
        garbage.push(vm.heap.alloc_drag_event(state, prototype).unwrap());
    }
    vm.heap.allow_only(0);
    let before = vm.heap.stats().major_collections;
    vm.settle_dynamic_import_waiters("done", &mut HashMap::new())
        .unwrap();
    assert!(vm.heap.stats().major_collections > before);
    assert!(garbage.iter().all(|id| !vm.heap.contains(*id)));
    for target in &targets {
        assert!(
            matches!(vm.promises[target].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == namespace)
        );
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(targets.iter().all(|id| !vm.heap.contains(*id)));
    assert!(!vm.heap.contains(namespace));
}
#[cfg_attr(test, test)]
fn import_delivery_prepared_roots_survive_removal_of_the_original_queue_owners() {
    let (mut vm, targets, namespace) = fixture();
    vm.deferred_import_waiters[1].pending.remove("later");
    let mut batch = vm.prepare_import_delivery("done", None, None).unwrap();
    vm.deferred_import_waiters.clear();
    vm.reclaim_import_waiter_storage();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(targets.iter().all(|id| vm.heap.contains(*id)) && vm.heap.contains(namespace));
    release(&mut vm, &mut batch);
    drop(batch);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(targets.iter().all(|id| !vm.heap.contains(*id)) && !vm.heap.contains(namespace));
}
fn eager_fixture(source: &str) -> (Vm, Vec<ObjectId>, ObjectId) {
    let mut vm = page();
    let code = crate::compile_module(&crate::parse_module(source).unwrap()).unwrap();
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), code)]));
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
    let namespace = vm.last_module_namespace.unwrap();
    let mut targets = Vec::new();
    for _ in 0..2 {
        let promise = vm.new_promise().unwrap();
        vm.enqueue_module_import_waiter("entry".into(), promise)
            .unwrap();
        targets.push(promise);
    }
    (vm, targets, namespace)
}
#[cfg_attr(test, test)]
fn import_delivery_eager_namespace_fulfillment_keeps_module_exports() {
    let (mut vm, targets, namespace) = eager_fixture("export const answer=42;");
    let mut graph = vm.module_graph.take().unwrap();
    vm.settle_dynamic_import_waiters("entry", &mut graph.linked)
        .unwrap();
    vm.module_graph = Some(graph);
    assert!(vm.module_import_waiters.is_empty());
    for target in targets {
        assert!(
            matches!(vm.promises[&target].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == namespace)
        );
    }
}
#[cfg_attr(test, test)]
fn import_delivery_callable_exports_transfer_rooting_to_thenable_jobs() {
    let (mut vm, targets, _) = eager_fixture("export function then(resolve){resolve(42);}");
    let mut graph = vm.module_graph.take().unwrap();
    vm.settle_dynamic_import_waiters("entry", &mut graph.linked)
        .unwrap();
    vm.module_graph = Some(graph);
    assert_eq!(vm.promise_jobs.len(), 2);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(targets.iter().all(|id| vm.heap.contains(*id)));
    vm.run_host_microtask_checkpoint().unwrap();
    for target in &targets {
        assert!(matches!(
            vm.promises[target].status,
            PromiseStatus::Fulfilled(Value::Number(42.0))
        ));
    }
}
#[cfg_attr(test, test)]
fn import_delivery_receiver_instruction_failure_keeps_suffix_and_ambient_fuel() {
    let (mut vm, targets, _) = getter_fixture(false);
    vm.remaining_instructions = 0;
    assert_eq!(
        vm.settle_dynamic_import_waiters("done", &mut HashMap::new()),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.remaining_instructions, 0);
    assert_eq!(vm.deferred_import_waiters.len(), 1);
    assert_eq!(vm.deferred_import_waiters[0].promise, targets[1]);
}
#[cfg_attr(test, test)]
fn import_delivery_rejection_object_roots_survive_without_the_error_tracker_or_queue() {
    let (mut vm, targets, _) = fixture();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let error = vm
        .retain_module_error(&Value::Object(object), ThrowOrigin::default())
        .unwrap();
    let batch = vm
        .prepare_import_delivery("done", None, Some(&error))
        .unwrap();
    drop(error);
    vm.deferred_import_waiters.clear();
    vm.reclaim_import_waiter_storage();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(object));
    assert!(targets.iter().all(|id| vm.heap.contains(*id)));
    vm.dispatch_import_delivery("done", batch, true).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_import_delivery_contracts() {
        import_delivery_native_refusal_preserves_ready_and_partial_waiters();
        import_delivery_rejection_native_refusal_preserves_both_waiter_kinds();
        import_delivery_root_refusal_preserves_dependency_membership_and_targets();
        import_delivery_partial_only_completion_reclaims_sparse_dependencies();
        import_delivery_unrelated_completion_needs_no_snapshot_or_root_storage();
        import_delivery_whole_root_batch_refusal_can_retry_without_consuming_a_prefix();
        import_delivery_abandoned_preparation_releases_roots_and_temporary_native_storage();
        import_delivery_owned_rejection_snapshot_keeps_its_native_lease_after_vm_drop();
        import_delivery_one_byte_short_metadata_allowance_keeps_membership_and_roots();
        import_delivery_mixed_rejection_preserves_reason_origin_and_consumes_all_selected_waiters();
        import_delivery_rejection_inputs_transfer_once_to_the_ordinary_settlement_owner();
        import_delivery_committed_receiver_failure_preserves_the_unattempted_suffix();
        import_delivery_committed_reaction_admission_failure_keeps_the_suffix_queued();
        import_delivery_getter_gc_keeps_later_targets_and_namespaces_alive();
        import_delivery_prepared_roots_survive_removal_of_the_original_queue_owners();
        import_delivery_eager_namespace_fulfillment_keeps_module_exports();
        import_delivery_callable_exports_transfer_rooting_to_thenable_jobs();
        import_delivery_receiver_instruction_failure_keeps_suffix_and_ambient_fuel();
        import_delivery_rejection_object_roots_survive_without_the_error_tracker_or_queue();
    }
}
