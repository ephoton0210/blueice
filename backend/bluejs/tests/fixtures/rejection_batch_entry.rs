// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny rejection batches use numerical peers without represented allocations.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_rejection_tracking().unwrap();
    vm
}
fn with_source(source: &str) -> Vm {
    let mut vm = page();
    execute(&mut vm, source);
    vm.notify_host_rejections().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.remaining_instructions = 73;
    assert_eq!(vm.browser_tasks.len(), 1);
    vm
}
fn fixture() -> Vm {
    with_source("onunhandledrejection=e=>e.preventDefault();var p=Promise.reject('tiny');var q=Promise.reject(7);0;")
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_public_refusal_keeps_notification_roots_and_checkpoint_fuel() {
    let mut vm = fixture();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.run_host_task_turn(0, 1),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    // The public turn first runs its own checkpoint; preparation adds no fuel use.
    assert_eq!(vm.remaining_instructions, vm.config.instruction_budget);
    assert_eq!(vm.host_rejections.as_ref().unwrap().active, 0);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_preparation_refuses_before_copying_a_batch() {
    let mut vm = fixture();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 1);
    assert!(matches!(
        vm.prepare_browser_task_entry(0),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
}
fn prepared(vm: &mut Vm) -> PreparedRejectionBatch {
    vm.prepare_browser_task_entry(0)
        .unwrap()
        .rejections
        .unwrap()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn promise(vm: &mut Vm, name: &str) -> ObjectId {
    vm.lookup_global_name(name)
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap()
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_exact_allowance_covers_the_complete_snapshot() {
    let mut vm = fixture();
    let original = vm.host_scheduling_usage().unwrap().payload_bytes;
    let first = prepared(&mut vm);
    let bytes = first.retained_bytes();
    assert_eq!(first.entries.len(), 2);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        original + bytes
    );
    drop(first);
    let _peer = leave_room(&mut vm, bytes);
    let batch = prepared(&mut vm);
    assert_eq!(batch.retained_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert_eq!(vm.remaining_instructions, 73);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_one_byte_short_keeps_the_whole_batch_queued() {
    let mut vm = fixture();
    let batch = prepared(&mut vm);
    let bytes = batch.retained_bytes();
    drop(batch);
    let roots = vm.heap.root_storage_state();
    let serial = vm.host_scheduling.as_ref().unwrap().task_serial_state();
    let _peer = leave_room(&mut vm, bytes - 1);
    assert!(matches!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(
        vm.host_scheduling.as_ref().unwrap().task_serial_state(),
        serial
    );
    assert_eq!(vm.remaining_instructions, 73);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_dropped_preparation_refunds_without_changing_notification_state() {
    let mut vm = fixture();
    let before = vm.host_scheduling_usage().unwrap();
    let roots = vm.heap.root_storage_state();
    let p = promise(&mut vm, "p");
    let batch = prepared(&mut vm);
    assert!(!vm.promises[&p].rejection_notified);
    drop(batch);
    assert_eq!(vm.host_scheduling_usage().unwrap(), before);
    assert_eq!(vm.heap.root_storage_state(), roots);
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.promises[&p].rejection_notified);
    assert!(vm.browser_tasks.is_empty());
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_rust_held_snapshot_outlives_vm_with_its_own_charge() {
    let mut vm = fixture();
    let child = observer(&vm);
    let batch = prepared(&mut vm);
    let bytes = batch.retained_bytes();
    drop(vm);
    assert_eq!(child.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(batch.entries[0].reason, Value::String("tiny".into()));
    drop(batch);
    assert_eq!(child.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_zero_fuel_precedes_snapshot_admission() {
    let mut vm = fixture();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    vm.config.instruction_budget = 0;
    assert!(matches!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::InstructionLimit)
    ));
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_stale_cleanup_needs_no_snapshot_or_host_instruction() {
    let mut vm = fixture();
    for name in ["p", "q"] {
        let id = promise(&mut vm, name);
        vm.set_promise_handled(id);
    }
    let roots = vm.heap.root_storage_state().0;
    vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut vm, 0);
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots - 2);
    assert_eq!(vm.remaining_instructions, 73);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_mixed_batch_does_not_copy_a_suppressed_reason() {
    let mut vm = fixture();
    let p = promise(&mut vm, "p");
    vm.set_promise_handled(p);
    let batch = prepared(&mut vm);
    assert!(batch.entries[0].state.is_none());
    assert_eq!(batch.entries[0].reason, Value::Undefined);
    assert!(batch.entries[1].state.is_some());
    drop(batch);
    let q = promise(&mut vm, "q");
    vm.run_own_browser_task(0).unwrap();
    assert!(!vm.promises[&p].rejection_notified);
    assert!(vm.promises[&q].rejection_notified);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_callback_checkpoint_can_suppress_a_later_snapshot() {
    let mut vm = with_source("var seen=[];var p=Promise.reject('first');var q=Promise.reject('second');onunhandledrejection=e=>{seen.push(e.reason);e.preventDefault();Promise.resolve().then(()=>q.catch(()=>{}));};0;");
    let p = promise(&mut vm, "p");
    let q = promise(&mut vm, "q");
    vm.run_own_browser_task(0).unwrap();
    execute(&mut vm, "globalThis.observed=seen.join(',');0;");
    assert_eq!(
        vm.lookup_global_name("observed").unwrap().unwrap(),
        Value::String("first".into())
    );
    assert!(vm.browser_tasks.is_empty());
    assert!(vm.drain_host_console().unwrap().records.is_empty());
    // Callback jobs may register unrelated roots. Prove these two notification
    // owners actually become collectible instead of comparing aggregate counts.
    execute(&mut vm, "p=null;q=null;onunhandledrejection=null;0;");
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(p));
    assert!(!vm.heap.contains(q));
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_sources_and_utf16_reasons_survive_the_snapshot() {
    let mut vm = with_source(r"var p=Promise.reject('\ud800');onunhandledrejection=null;0;");
    let p = promise(&mut vm, "p");
    let source = crate::diagnostics::DiagnosticSourceLocation {
        filename: "tiny.js".into(),
        line: 3,
        column: 5,
    };
    vm.promises.get_mut(&p).unwrap().rejection_origin.source = Some(source.clone());
    let batch = prepared(&mut vm);
    assert_eq!(batch.entries[0].source.as_ref(), Some(&source));
    assert_eq!(
        batch.entries[0].reason,
        Value::String(JsString::from_code_units(vec![0xd800]))
    );
    drop(batch);
    vm.run_own_browser_task(0).unwrap();
    let console = vm.drain_host_console().unwrap();
    assert_eq!(console.records.len(), 1);
    assert_eq!(console.records[0].source.as_ref(), Some(&source));
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_committed_instruction_failure_releases_roots_and_snapshots() {
    let mut vm = fixture();
    let child = observer(&vm);
    let roots = vm.heap.root_storage_state().0;
    vm.config.instruction_budget = 1;
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots - 2);
    assert_eq!(vm.host_rejections.as_ref().unwrap().active, 0);
    assert_eq!(vm.active_scheduling_bytes, 0);
    drop(vm);
    assert_eq!(child.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_object_reason_is_rooted_through_managed_event_allocation() {
    let mut vm = with_source("globalThis.p=Promise.reject({x:9});globalThis.seen=0;onunhandledrejection=e=>{seen=e.reason.x;e.preventDefault();};0;");
    let p = promise(&mut vm, "p");
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap.set(global, "p", Value::Undefined).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let mut garbage = Vec::new();
    // Four small managed event states provide deterministic collection headroom.
    let prototype = vm.globals["%WebPromiseRejectionEvent%"];
    for _ in 0..4 {
        let state = vm.new_web_event_state(2, "unused".into());
        garbage.push(vm.heap.alloc_drag_event(state, prototype).unwrap());
    }
    vm.heap.allow_only(0);
    let collections = vm.heap.stats().major_collections;
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.heap.stats().major_collections > collections);
    assert!(garbage.iter().all(|id| !vm.heap.contains(*id)));
    assert_eq!(vm.heap.get(global, "seen").unwrap(), Value::Number(9.0));
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(p));
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_handled_notifications_prepare_non_cancelable_events() {
    let mut vm = fixture();
    vm.run_own_browser_task(0).unwrap();
    execute(&mut vm, "p.catch(()=>{});0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(vm.browser_tasks.len(), 1);
    let batch = prepared(&mut vm);
    let state = batch.entries[0].state.as_ref().unwrap();
    assert!(!state.cancelable);
    assert_eq!(state.get("type"), Value::String("rejectionhandled".into()));
    assert_eq!(state.get("reason"), Value::String("tiny".into()));
    drop(batch);
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.browser_tasks.is_empty());
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_managed_refusal_releases_the_committed_batch() {
    let mut vm = fixture();
    let child = observer(&vm);
    let roots = vm.heap.root_storage_state().0;
    vm.heap.allow_only(0);
    assert!(matches!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots - 2);
    assert_eq!(vm.host_rejections.as_ref().unwrap().active, 0);
    assert_eq!(vm.active_scheduling_bytes, 0);
    drop(vm);
    assert_eq!(child.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn rejection_batch_entry_console_adopts_source_before_its_budget_check() {
    use crate::vm::host_console::formatting::Output;
    let source = crate::diagnostics::DiagnosticSourceLocation {
        filename: "tiny.js".into(),
        line: 1,
        column: 1,
    };
    let mut control = page();
    let before = control.host_scheduling_usage().unwrap().payload_bytes;
    control
        .console_emit_with_source(
            ConsoleFunction::Error,
            "error",
            Output::message("tiny"),
            Some(source.clone()),
        )
        .unwrap();
    let added = control.host_scheduling_usage().unwrap().payload_bytes - before;
    let mut vm = page();
    let charge = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(source.filename.capacity())
        .unwrap();
    let _peer = leave_room(&mut vm, added - source.filename.capacity());
    vm.console_emit_with_leased_source(
        ConsoleFunction::Error,
        "error",
        Output::message("tiny"),
        Some(source),
        Some(charge),
    )
    .unwrap();
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn rejection_batch_entry_delivery_refreshes_the_prepared_timestamp() {
    let mut vm = with_source("var stamp=-1;var p=Promise.reject('tiny');onunhandledrejection=e=>{stamp=e.timeStamp;e.preventDefault();};0;");
    let mut batch = prepared(&mut vm);
    // A fixed sentinel proves delivery refreshes the field without sleeping.
    batch.entries[0]
        .state
        .as_mut()
        .unwrap()
        .set("timeStamp", Value::Number(-7.0));
    let BrowserTask::PromiseRejections {
        entries, handled, ..
    } = vm.browser_tasks.remove(0).unwrap()
    else {
        unreachable!()
    };
    vm.remaining_instructions = vm.config.instruction_budget;
    vm.run_host_rejection_task(entries, handled, batch).unwrap();
    let stamp = vm
        .lookup_global_name("stamp")
        .unwrap()
        .unwrap()
        .as_number()
        .unwrap();
    assert!(stamp >= 0.0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_rejection_batch_entry_contracts() {
        rejection_batch_entry_public_refusal_keeps_notification_roots_and_checkpoint_fuel();
        rejection_batch_entry_preparation_refuses_before_copying_a_batch();
        rejection_batch_entry_exact_allowance_covers_the_complete_snapshot();
        rejection_batch_entry_one_byte_short_keeps_the_whole_batch_queued();
        rejection_batch_entry_dropped_preparation_refunds_without_changing_notification_state();
        rejection_batch_entry_rust_held_snapshot_outlives_vm_with_its_own_charge();
        rejection_batch_entry_zero_fuel_precedes_snapshot_admission();
        rejection_batch_entry_stale_cleanup_needs_no_snapshot_or_host_instruction();
        rejection_batch_entry_mixed_batch_does_not_copy_a_suppressed_reason();
        rejection_batch_entry_callback_checkpoint_can_suppress_a_later_snapshot();
        rejection_batch_entry_sources_and_utf16_reasons_survive_the_snapshot();
        rejection_batch_entry_committed_instruction_failure_releases_roots_and_snapshots();
        rejection_batch_entry_object_reason_is_rooted_through_managed_event_allocation();
        rejection_batch_entry_handled_notifications_prepare_non_cancelable_events();
        rejection_batch_entry_managed_refusal_releases_the_committed_batch();
        rejection_batch_entry_console_adopts_source_before_its_budget_check();
        rejection_batch_entry_delivery_refreshes_the_prepared_timestamp();
    }
}
