// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny finally-state tables and numerical peers verify provenance ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    vm.stack.reserve(16);
    vm.next_host_native_gc_bytes = usize::MAX;
    vm.next_host_heap_gc_bytes = usize::MAX;
    vm
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
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn table(vm: &mut Vm, count: usize) -> (Vec<ObjectId>, Vec<RootId>) {
    vm.promise_thunk_origins.reserve(count);
    let mut objects = Vec::new();
    let mut roots = Vec::new();
    for _ in 0..count {
        let state = vm.promise_state().unwrap();
        roots.push(vm.heap.root(state).unwrap());
        objects.push(state);
        vm.promise_thunk_origins.insert(state, origin());
    }
    (objects, roots)
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_gc_preserves_backing_during_staged_managed_allocation() {
    let mut vm = page();
    let (states, roots) = table(&mut vm, 7);
    let capacity = vm.promise_thunk_origins.capacity();
    for root in roots.iter().skip(1) {
        vm.heap.unroot(*root).unwrap();
    }
    vm.preserve_promise_table_capacity = true;
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promise_thunk_origins.len(), 1);
    assert!(vm.promise_thunk_origins.contains_key(&states[0]));
    assert_eq!(vm.promise_thunk_origins.capacity(), capacity);
    vm.preserve_promise_table_capacity = false;
    vm.heap.unroot(roots[0]).unwrap();
}
fn callback_state(vm: &mut Vm) -> (ObjectId, RootId) {
    let promise = vm.new_promise().unwrap();
    let base = vm.stack.len();
    vm.stack.push(Value::Object(promise));
    let callback_state = vm.promise_state().unwrap();
    vm.stack.push(Value::Object(callback_state));
    vm.promise_state_set(callback_state, "value", Value::Object(promise))
        .unwrap();
    let callback = vm
        .promise_native_function(
            NativeFunction::PromiseValueThunk {
                state: callback_state,
                thrower: false,
            },
            0,
        )
        .unwrap();
    vm.stack.push(callback.clone());
    let state = vm.promise_state().unwrap();
    vm.stack.push(Value::Object(state));
    vm.promise_state_set(state, "onFinally", callback).unwrap();
    let constructor = vm.global("Promise").unwrap();
    vm.promise_state_set(state, "constructor", constructor)
        .unwrap();
    let root = vm.heap.root(state).unwrap();
    vm.stack.truncate(base);
    (state, root)
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_finally_refuses_complete_table_before_allocating_thunk_state() {
    let mut vm = page();
    let (_states, _roots) = table(&mut vm, 7);
    let (state, _root) = callback_state(&mut vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let sample = vm.promise_state().unwrap();
    vm.stack.push(Value::Object(sample));
    vm.promise_state_set(sample, "value", Value::Number(7.0))
        .unwrap();
    vm.stack.pop();
    let state_bytes = vm.host_scheduling_usage().unwrap().payload_bytes - before;
    let old_capacity = vm.promise_thunk_origins.capacity();
    let replacement =
        HashMap::<ObjectId, ThrowOrigin>::with_capacity(vm.promise_thunk_origins.len() + 1);
    let net =
        (replacement.capacity() - old_capacity) * std::mem::size_of::<(ObjectId, ThrowOrigin)>();
    let _peer = leave_room(&mut vm, state_bytes + net);
    let outcome = vm.promise_finally_function(state, true, &Value::Number(7.0));
    assert_eq!(outcome, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.promise_thunk_origins.len(), 7);
    assert_eq!(vm.promise_thunk_origins.capacity(), old_capacity);
    assert!(vm.stack.is_empty());

    // The public operation may allocate during its preceding callback/resolve
    // phase. Isolate the native table boundary to verify no thunk allocation.
    let mut direct = page();
    let (_states, _roots) = table(&mut direct, 7);
    let _peer = leave_room(&mut direct, net);
    let heap = direct.heap.stats();
    assert_eq!(
        direct.allocate_finally_thunk_state(&Value::Number(7.0), true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(direct.promise_thunk_origins.len(), 7);
    assert_eq!(direct.heap.stats(), heap);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_thrower_refuses_filename_copy_before_publishing_origin() {
    let mut vm = page();
    let (states, _roots) = table(&mut vm, 1);
    vm.promise_state_set(states[0], "value", Value::Number(7.0))
        .unwrap();
    let epoch = vm.throw_epoch;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.promise_value_thunk(states[0], true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.pending_diagnostic_origin.is_none());
    assert_eq!(vm.throw_epoch, epoch);
}

fn sparse() -> Vm {
    let mut vm = page();
    let (_states, roots) = table(&mut vm, 7);
    for root in roots.iter().skip(1) {
        vm.heap.unroot(*root).unwrap();
    }
    vm.preserve_promise_table_capacity = true;
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.preserve_promise_table_capacity = false;
    assert_eq!(vm.promise_thunk_origins.len(), 1);
    assert_eq!(vm.promise_thunk_origins.capacity(), 7);
    vm
}
fn candidate_bytes() -> usize {
    let table = HashMap::<ObjectId, ThrowOrigin>::with_capacity(1);
    host_scheduling::map_slots(&table)
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn garbage_roots(vm: &mut Vm) -> Vec<RootId> {
    let mut roots = Vec::new();
    for _ in 0..4 {
        let state = vm.new_web_event_state(2, "unused".into());
        let object = vm
            .heap
            .alloc_drag_event(state, vm.object_prototype)
            .unwrap();
        roots.push(vm.heap.root(object).unwrap());
    }
    roots
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_deletion_refunds_payload_without_refunding_backing() {
    let mut vm = page();
    let (states, _roots) = table(&mut vm, 7);
    let backing = vm.promise_thunk_origins.retained_bytes();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.promise_thunk_origins.remove(&states[0]);
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), backing);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        before - "tiny.js".len()
    );
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_optional_compaction_needs_detached_headroom() {
    let mut vm = sparse();
    let bytes = vm.promise_thunk_origins.retained_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.promise_thunk_origins.capacity(), 7);
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), bytes);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_short_reported_compaction_capacity_is_nonterminal() {
    let mut vm = sparse();
    let _peer = leave_room(&mut vm, candidate_bytes() - 1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.promise_thunk_origins.capacity(), 7);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_exact_compaction_reconciles_before_candidate_refund() {
    let mut vm = sparse();
    let old = vm.promise_thunk_origins.retained_bytes();
    let _peer = leave_room(&mut vm, candidate_bytes());
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.promise_thunk_origins.capacity(), 3);
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), candidate_bytes());
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_optional_compaction_retries_after_sibling_refund() {
    let mut vm = sparse();
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.promise_thunk_origins.capacity(), 7);
    drop(peer);
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.promise_thunk_origins.capacity(), 3);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_terminal_empty_cleanup_releases_without_admission() {
    let mut vm = page();
    let (_states, _roots) = table(&mut vm, 7);
    vm.promise_thunk_origins.retain(|_, _| false);
    vm.host_scheduling.as_ref().unwrap().fail();
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), 0);
    assert_eq!(vm.promise_thunk_origins.capacity(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_managed_gc_reuses_pruned_full_table_slots() {
    let mut vm = page();
    let (old, roots) = table(&mut vm, 3);
    let garbage = garbage_roots(&mut vm);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    for root in roots.into_iter().chain(garbage) {
        vm.heap.unroot(root).unwrap();
    }
    vm.heap.allow_only(0);
    let before = vm.heap.stats().major_collections;
    let state = vm
        .allocate_finally_thunk_state(&Value::Number(7.0), true)
        .unwrap();
    assert!(vm.heap.stats().major_collections > before);
    assert_eq!(vm.promise_thunk_origins.len(), 1);
    assert_eq!(vm.promise_thunk_origins.capacity(), 3);
    assert!(vm.promise_thunk_origins.contains_key(&state));
    assert!(old.iter().all(|id| !vm.heap.contains(*id)));
    assert!(!vm.preserve_promise_table_capacity);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_managed_gc_preserves_admitted_spare_slots() {
    let mut vm = page();
    vm.promise_thunk_origins.reserve(7);
    let (_states, roots) = table(&mut vm, 3);
    let garbage = garbage_roots(&mut vm);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    for root in roots.into_iter().skip(1).chain(garbage) {
        vm.heap.unroot(root).unwrap();
    }
    vm.heap.allow_only(0);
    let state = vm
        .allocate_finally_thunk_state(&Value::Number(7.0), true)
        .unwrap();
    assert_eq!(vm.promise_thunk_origins.len(), 2);
    assert_eq!(vm.promise_thunk_origins.capacity(), 7);
    assert!(vm.promise_thunk_origins.contains_key(&state));
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_failed_managed_creation_refunds_all_staging() {
    let mut vm = page();
    let (_states, _roots) = table(&mut vm, 3);
    vm.promise_origin_override = Some(origin());
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let roots = vm.heap.root_storage_state();
    let view = observer(&vm);
    vm.heap.allow_only(0);
    assert!(matches!(
        vm.allocate_finally_thunk_state(&Value::Number(7.0), true),
        Err(RuntimeError::Heap(_))
    ));
    assert_eq!(vm.promise_thunk_origins.len(), 3);
    assert_eq!(vm.promise_thunk_origins.capacity(), 3);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(!vm.preserve_promise_table_capacity);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_exact_current_origin_capture_owns_only_its_clone() {
    let mut vm = page();
    vm.promise_origin_override = Some(origin());
    let _peer = leave_room(&mut vm, "tiny.js".len());
    let (copied, charge) = vm.prepare_current_rejection_origin().unwrap();
    assert_eq!(copied.epoch, 7);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(copied);
    drop(charge);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - "tiny.js".len()
    );
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_frame_capture_and_unknown_override_keep_precedence() {
    for unknown in [false, true] {
        let mut vm = page();
        vm.active_diagnostic_frame = Some(crate::diagnostics::ActiveDiagnosticFrame::Native(
            origin().source,
        ));
        if unknown {
            vm.promise_origin_override = Some(ThrowOrigin::default());
        }
        let _peer = leave_room(&mut vm, if unknown { 0 } else { "tiny.js".len() });
        let (copied, charge) = vm.prepare_current_rejection_origin().unwrap();
        assert_eq!(copied.source.is_none(), unknown);
        drop(copied);
        drop(charge);
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_prepared_source_and_table_lease_survive_vm_drop() {
    let mut vm = page();
    let (_states, _roots) = table(&mut vm, 3);
    vm.promise_origin_override = Some(origin());
    let view = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm.prepare_thunk_origin().unwrap();
    let extra = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, extra);
    assert_eq!(prepared.origin.source.as_ref().unwrap().filename, "tiny.js");
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_abandoned_candidate_refunds_without_live_mutation() {
    let mut vm = page();
    let (_states, _roots) = table(&mut vm, 3);
    vm.promise_origin_override = Some(origin());
    let before = vm.host_scheduling_usage().unwrap();
    let heap = vm.heap.stats();
    let prepared = vm.prepare_thunk_origin().unwrap();
    drop(prepared);
    assert_eq!(vm.host_scheduling_usage().unwrap(), before);
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.promise_thunk_origins.len(), 3);
    assert_eq!(vm.promise_thunk_origins.capacity(), 3);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_nested_guard_defers_compaction_until_outer_completion() {
    let mut vm = page();
    let (_states, roots) = table(&mut vm, 8);
    let garbage = garbage_roots(&mut vm);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    for root in roots.into_iter().skip(1).chain(garbage) {
        vm.heap.unroot(root).unwrap();
    }
    vm.preserve_promise_table_capacity = true;
    vm.heap.allow_only(0);
    vm.allocate_finally_thunk_state(&Value::Number(7.0), true)
        .unwrap();
    assert!(vm.preserve_promise_table_capacity);
    assert_eq!(vm.promise_thunk_origins.len(), 2);
    assert_eq!(vm.promise_thunk_origins.capacity(), 14);
    vm.preserve_promise_table_capacity = false;
    vm.reclaim_scheduling_maps();
    assert_eq!(vm.promise_thunk_origins.capacity(), 3);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_live_function_roots_state_and_reason_then_releases_them() {
    let mut vm = page();
    let reason = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.promise_origin_override = Some(origin());
    let state = vm
        .allocate_finally_thunk_state(&Value::Object(reason), true)
        .unwrap();
    vm.stack.push(Value::Object(state));
    let function = vm
        .promise_native_function(
            NativeFunction::PromiseValueThunk {
                state,
                thrower: true,
            },
            0,
        )
        .unwrap()
        .object_id()
        .unwrap();
    let root = vm.heap.root(function).unwrap();
    vm.stack.pop();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(state) && vm.heap.contains(reason));
    assert_eq!(
        vm.promise_value_thunk(state, true),
        Err(RuntimeError::Thrown(Value::Object(reason)))
    );
    assert_eq!(
        vm.pending_diagnostic_origin.as_ref().unwrap().filename,
        "tiny.js"
    );
    vm.heap.unroot(root).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!([function, state, reason]
        .iter()
        .all(|id| !vm.heap.contains(*id)));
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_metadata_keys_alone_do_not_root_states() {
    let mut vm = page();
    let (states, roots) = table(&mut vm, 7);
    for root in roots {
        vm.heap.unroot(root).unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(states.iter().all(|id| !vm.heap.contains(*id)));
    assert!(vm.promise_thunk_origins.is_empty());
    assert_eq!(vm.promise_thunk_origins.capacity(), 0);
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_public_finally_waits_for_cleanup_and_preserves_reason() {
    let mut vm = page();
    execute(&mut vm, "globalThis.gate=Promise.withResolvers();globalThis.result=undefined;globalThis.chain=Promise.reject('bad').finally(()=>gate.promise).catch(e=>{result=e});0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.promise_thunk_origins.len(), 1);
    let state = *vm.promise_thunk_origins.keys().next().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(state));
    assert_eq!(execute(&mut vm, "result"), Value::Undefined);
    execute(&mut vm, "gate.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(execute(&mut vm, "result"), Value::String("bad".into()));
    execute(&mut vm, "gate=null;chain=null;0;");
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(state));
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_public_cleanup_rejection_overrides_original_reason() {
    let mut vm = page();
    execute(&mut vm, "globalThis.result=null;Promise.reject('original').finally(()=>Promise.reject('cleanup')).catch(e=>{result=e});0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(execute(&mut vm, "result"), Value::String("cleanup".into()));
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_fulfillment_state_needs_no_origin_table() {
    let mut vm = page();
    vm.promise_origin_override = Some(origin());
    let state = vm
        .allocate_finally_thunk_state(&Value::Number(7.0), false)
        .unwrap();
    assert_eq!(vm.promise_value_thunk(state, false), Ok(Value::Number(7.0)));
    assert!(vm.promise_thunk_origins.is_empty());
    assert_eq!(vm.promise_thunk_origins.retained_bytes(), 0);
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_outer_capture_refusal_restores_ambient_stack_and_origin() {
    let mut vm = page();
    let (state, _root) = callback_state(&mut vm);
    let argument = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.promise_origin_override = Some(origin());
    let before = vm.stack.len();
    let heap = vm.heap.stats();
    let active = vm.active_scheduling_bytes;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.promise_finally_function(state, true, &Value::Object(argument)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.stack.len(), before);
    assert_eq!(vm.active_scheduling_bytes, active);
    assert_eq!(vm.promise_origin_override.as_ref().unwrap().epoch, 7);
    assert_eq!(vm.heap.stats(), heap);
    assert!(vm.promise_thunk_origins.is_empty());
}
#[cfg_attr(test, test)]
fn thunk_origin_storage_exact_thrower_copy_moves_into_pending_origin_once() {
    let mut vm = page();
    let (states, _roots) = table(&mut vm, 1);
    vm.promise_state_set(states[0], "value", Value::Number(7.0))
        .unwrap();
    let epoch = vm.throw_epoch;
    let _peer = leave_room(&mut vm, "tiny.js".len());
    assert_eq!(
        vm.promise_value_thunk(states[0], true),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(vm.throw_epoch, epoch + 1);
    assert_eq!(
        vm.pending_diagnostic_origin.as_ref().unwrap().filename,
        "tiny.js"
    );
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_thunk_origin_storage_contracts() {
        thunk_origin_storage_gc_preserves_backing_during_staged_managed_allocation();
        thunk_origin_storage_finally_refuses_complete_table_before_allocating_thunk_state();
        thunk_origin_storage_thrower_refuses_filename_copy_before_publishing_origin();
        thunk_origin_storage_deletion_refunds_payload_without_refunding_backing();
        thunk_origin_storage_optional_compaction_needs_detached_headroom();
        thunk_origin_storage_short_reported_compaction_capacity_is_nonterminal();
        thunk_origin_storage_exact_compaction_reconciles_before_candidate_refund();
        thunk_origin_storage_optional_compaction_retries_after_sibling_refund();
        thunk_origin_storage_terminal_empty_cleanup_releases_without_admission();
        thunk_origin_storage_managed_gc_reuses_pruned_full_table_slots();
        thunk_origin_storage_managed_gc_preserves_admitted_spare_slots();
        thunk_origin_storage_failed_managed_creation_refunds_all_staging();
        thunk_origin_storage_exact_current_origin_capture_owns_only_its_clone();
        thunk_origin_storage_frame_capture_and_unknown_override_keep_precedence();
        thunk_origin_storage_prepared_source_and_table_lease_survive_vm_drop();
        thunk_origin_storage_abandoned_candidate_refunds_without_live_mutation();
        thunk_origin_storage_nested_guard_defers_compaction_until_outer_completion();
        thunk_origin_storage_live_function_roots_state_and_reason_then_releases_them();
        thunk_origin_storage_metadata_keys_alone_do_not_root_states();
        thunk_origin_storage_public_finally_waits_for_cleanup_and_preserves_reason();
        thunk_origin_storage_public_cleanup_rejection_overrides_original_reason();
        thunk_origin_storage_fulfillment_state_needs_no_origin_table();
        thunk_origin_storage_outer_capture_refusal_restores_ambient_stack_and_origin();
        thunk_origin_storage_exact_thrower_copy_moves_into_pending_origin_once();
    }
}
