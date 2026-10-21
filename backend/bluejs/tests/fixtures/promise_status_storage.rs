// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Settled values and rejection provenance retain credit beside map backing.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn loan<T>(
    vm: &mut Vm,
    source: &Vm,
    action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let owner = source.argument_list_owner()?;
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?;
    vm.with_realm_call_roots(roots, action)
}
fn record_bytes(record: &PromiseRecord) -> usize {
    let value = match &record.status {
        PromiseStatus::Pending => 0,
        PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => {
            value_payload_bytes(value)
        }
    };
    value + record.rejection_origin.payload_bytes()
}
fn bytes(vm: &Vm) -> usize {
    vm.promises.retained_bytes() + vm.promises.values().map(record_bytes).sum::<usize>()
}
fn fulfilled() -> PromiseStatus {
    PromiseStatus::Fulfilled(Value::String("tiny".into()))
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
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn promise_status_storage_scoped_fulfillment_outlives_the_vm() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.settle_promise(promise, fulfilled()).unwrap();
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_status_storage_borrowed_fulfillment_keeps_the_table_account() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    vm.settle_promise(promise, fulfilled()).unwrap();
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn promise_status_storage_rejection_value_and_origin_outlive_the_vm() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.promise_origin_override = Some(origin());
    vm.settle_promise(
        promise,
        PromiseStatus::Rejected(Value::String("tiny".into())),
    )
    .unwrap();
    assert_eq!(vm.promises[&promise].rejection_origin.epoch, 7);
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_status_storage_legacy_values_adopt_native_credit_with_the_table() {
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    vm.settle_promise(promise, fulfilled()).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.new_promise().unwrap();
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_status_storage_reaction_result_keeps_its_own_settled_copy() {
    let mut vm = page();
    run(
        &mut vm,
        "var a=Promise.resolve('tiny');var b=a.then(value=>value);0;",
    );
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(vm.promises.values().filter(|record| matches!(&record.status, PromiseStatus::Fulfilled(Value::String(text)) if text == "tiny")).count() >= 2);
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_status_storage_major_gc_keeps_the_surviving_value_credit() {
    let mut vm = page();
    let first = vm.new_promise().unwrap();
    let _root = vm.heap.root(first).unwrap();
    let second = vm.new_promise().unwrap();
    vm.settle_promise(first, fulfilled()).unwrap();
    vm.settle_promise(second, fulfilled()).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.len(), 1);
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn promise_status_storage_terminal_table_account_refuses_settlement_publication() {
    let source = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    vm.settle_promise(promise, fulfilled()).unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert!(vm.promise_jobs.is_empty());
}
#[cfg_attr(test, test)]
fn promise_status_storage_foreign_input_credit_moves_to_the_record_account() {
    let source = page();
    let caller = page();
    let home = observer(&source);
    let other = observer(&caller);
    let before = home.host_scheduling_snapshot().payload_bytes;
    let other_before = other.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    let input = caller
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(8)
        .unwrap();
    vm.settle_promise_with_input_charge(promise, fulfilled(), Some(input))
        .unwrap();
    assert_eq!(other.host_scheduling_snapshot().payload_bytes, other_before);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(
        home.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    drop(records);
    assert_eq!(home.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn promise_status_storage_repeated_settlement_preserves_the_first_value() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Number(7.0)))
        .unwrap();
    vm.settle_promise(promise, fulfilled()).unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
}
#[cfg_attr(test, test)]
fn promise_status_storage_public_reactions_wait_for_the_checkpoint() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var trace=[];var resolve;var p=new Promise(r=>resolve=r);p.then(value=>trace.push(value));resolve(7);trace.length;"), Value::Number(0.0));
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(run(&mut vm, "trace[0];"), Value::Number(7.0));
}

#[cfg_attr(test, test)]
fn promise_status_storage_detached_record_outlives_its_map() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.promise_origin_override = Some(origin());
    vm.settle_promise(
        promise,
        PromiseStatus::Rejected(Value::String("tiny".into())),
    )
    .unwrap();
    let view = observer(&vm);
    let record = vm.promises.remove(&promise).unwrap();
    let retained = record_bytes(&record);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(record);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_status_storage_legacy_field_transfer_has_no_duplicate_peak() {
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    vm.settle_promise(promise, fulfilled()).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    assert!(vm.promises[&promise].status_storage.is_none());
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| vm.retain_promise_table_storage(None));
    result.unwrap();
    assert_eq!(peak, 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_status_storage_borrowed_adoption_covers_existing_values() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let first = vm.new_promise().unwrap();
    vm.settle_promise(first, fulfilled()).unwrap();
    let second = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    vm.settle_promise(second, fulfilled()).unwrap();
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn promise_status_storage_incoming_credit_adopts_a_previously_unowned_table() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    let input = source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(8)
        .unwrap();
    vm.settle_promise_with_input_charge(promise, fulfilled(), Some(input))
        .unwrap();
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn promise_status_storage_held_preparation_retains_value_and_origin_after_vm_drop() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    vm.promise_origin_override = Some(origin());
    let prepared = vm
        .prepare_promise_settlement(
            promise,
            PromiseStatus::Rejected(Value::String("tiny".into())),
            None,
        )
        .unwrap()
        .unwrap();
    let retained = 8 + prepared.origin.payload_bytes();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn promise_status_storage_failed_input_account_refuses_publication() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    let input = caller
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(8)
        .unwrap();
    let _ = caller.host_scheduling.as_ref().unwrap().fail();
    vm.settle_promise_with_input_charge(promise, fulfilled(), Some(input))
        .unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn promise_status_storage_healthy_caller_cannot_revive_the_original_account() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    loan(&mut vm, &caller, |vm| {
        vm.settle_promise(promise, fulfilled())
    })
    .unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert!(caller.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn promise_status_storage_policy_discovery_checks_status_beside_unowned_backing() {
    let source = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    let promise = loan(&mut vm, &source, |vm| vm.new_promise()).unwrap();
    vm.settle_promise(promise, fulfilled()).unwrap();
    let record = vm.promises.remove(&promise).unwrap();
    let mut legacy = retained_control_map::RetainedControlMap::default();
    promise_queue_storage::PreparedMap::for_insert(&legacy, None)
        .unwrap()
        .commit(&mut legacy);
    legacy.insert(promise, record);
    vm.promises = legacy;
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    // Activation preserves a failed scope to prevent work from resuming. It
    // must not publish the policy or rewrite this independently owned value.
    assert!(!vm.document_policy_ready);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!vm.promises.has_owner());
    assert!(vm.promises[&promise]
        .status_owner()
        .unwrap()
        .unwrap()
        .belongs_to(source.host_scheduling.as_ref().unwrap()));
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn promise_status_storage_manual_timer_and_gc_preserve_settled_strings() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(&mut vm,"var result='';var p;setTimeout(()=>{p=Promise.resolve('tiny').then(value=>{$262.gc();result=value;return value;});},1);0;");
    assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut vm, "result;"), Value::String("tiny".into()));
    assert!(vm.promises.values().any(|record| matches!(&record.status, PromiseStatus::Fulfilled(Value::String(text)) if text == "tiny")));
    let view = observer(&vm);
    let retained = bytes(&vm);
    let records = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn promise_status_storage_preleased_rejection_retains_origin_with_the_value() {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    vm.promise_origin_override = Some(origin());
    let input = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(8)
        .unwrap();
    vm.settle_promise_with_input_charge(
        promise,
        PromiseStatus::Rejected(Value::String("tiny".into())),
        Some(input),
    )
    .unwrap();
    let record = vm.promises.remove(&promise).unwrap();
    let retained = record_bytes(&record);
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(record);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_promise_status_storage_contracts() {
        promise_status_storage_scoped_fulfillment_outlives_the_vm();
        promise_status_storage_borrowed_fulfillment_keeps_the_table_account();
        promise_status_storage_rejection_value_and_origin_outlive_the_vm();
        promise_status_storage_legacy_values_adopt_native_credit_with_the_table();
        promise_status_storage_reaction_result_keeps_its_own_settled_copy();
        promise_status_storage_major_gc_keeps_the_surviving_value_credit();
        promise_status_storage_terminal_table_account_refuses_settlement_publication();
        promise_status_storage_foreign_input_credit_moves_to_the_record_account();
        promise_status_storage_repeated_settlement_preserves_the_first_value();
        promise_status_storage_public_reactions_wait_for_the_checkpoint();
        promise_status_storage_detached_record_outlives_its_map();
        promise_status_storage_legacy_field_transfer_has_no_duplicate_peak();
        promise_status_storage_borrowed_adoption_covers_existing_values();
        promise_status_storage_incoming_credit_adopts_a_previously_unowned_table();
        promise_status_storage_held_preparation_retains_value_and_origin_after_vm_drop();
        promise_status_storage_failed_input_account_refuses_publication();
        promise_status_storage_healthy_caller_cannot_revive_the_original_account();
        promise_status_storage_policy_discovery_checks_status_beside_unowned_backing();
        promise_status_storage_manual_timer_and_gc_preserve_settled_strings();
        promise_status_storage_preleased_rejection_retains_origin_with_the_value();
    }
}
