// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny console label/formatter storage and callback-before-publication cases.

use super::*;
use std::cell::Cell;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_host_console().unwrap();
    run(&mut vm, "0;");
    vm
}
fn view(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn object(vm: &mut Vm, name: &str) -> ObjectId {
    vm.lookup_global_name(name)
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap()
}
fn key(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}
fn format_sample(text: &str, neutral: bool) -> usize {
    let mut vm = page();
    let observer = view(&vm);
    let baseline = Rc::new(Cell::new(0));
    let before = baseline.clone();
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(Some(
            observer.host_scheduling_snapshot().payload_bytes - before.get(),
        ));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm,&format!("var label='{text}',first={{toString(){{return label;}}}},second={{toString(){{observe();return 'tail';}}}};0;"));
    let first = object(&mut vm, "first");
    let second = object(&mut vm, "second");
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(observer.host_scheduling_snapshot().payload_bytes);
    vm.console_call(
        ConsoleFunction::Log,
        &[
            Value::String("%s %s".into()),
            Value::Object(first),
            Value::Object(second),
        ],
        false,
    )
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
    sample.get().unwrap()
}
#[cfg_attr(test, test)]
fn console_runtime_storage_formatted_prefix_stays_paid_through_next_conversion() {
    assert_eq!(
        format_sample("abcdefghijklmno", false) - format_sample("abc", false),
        8
    );
}
#[cfg_attr(test, test)]
fn console_runtime_storage_neutral_prefix_keeps_original_credit() {
    assert_eq!(
        format_sample("abcdefghijklmno", true) - format_sample("abc", true),
        8
    );
}
fn detached(neutral: bool) {
    let mut vm = page();
    run(&mut vm, "console.count('one');console.time('two');0;");
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let state = vm.host_console.take().unwrap();
    let bytes = state.payload_bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(state);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn console_runtime_storage_detached_label_maps_keep_credit_after_vm_drop() {
    detached(false);
}
#[cfg_attr(test, test)]
fn console_runtime_storage_neutral_detached_labels_keep_original_credit() {
    detached(true);
}
fn cancel_clock(vm: &mut Vm, at: usize) {
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let calls = Rc::new(Cell::new(0));
    vm.install_host_timer_functions_with_clock(move || {
        calls.set(calls.get() + 1);
        if calls.get() == at {
            let _ = scope.fail();
        }
        5
    })
    .unwrap();
}
fn count_cancel(neutral: bool) {
    let mut vm = page();
    cancel_clock(&mut vm, 1);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        vm.console_call(
            ConsoleFunction::Count,
            &[Value::String("one".into())],
            false
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    let state = vm.host_console.as_ref().unwrap();
    assert!(state.counts.get(&key("one")).is_none());
    assert!(state.records.is_empty());
    assert_eq!(state.sequence, 0);
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn console_runtime_storage_clock_cancellation_preserves_count_map() {
    count_cancel(false);
}
#[cfg_attr(test, test)]
fn console_runtime_storage_neutral_clock_cancellation_preserves_count_map() {
    count_cancel(true);
}
fn end_cancel(neutral: bool) {
    let mut vm = page();
    run(&mut vm, "console.time('one');0;");
    cancel_clock(&mut vm, 2);
    let before = vm
        .host_console
        .as_ref()
        .unwrap()
        .timers
        .get(&key("one"))
        .copied();
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        vm.console_call(
            ConsoleFunction::TimeEnd,
            &[Value::String("one".into())],
            false
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    let state = vm.host_console.as_ref().unwrap();
    assert_eq!(state.timers.get(&key("one")).copied(), before);
    assert!(state.records.is_empty());
    assert_eq!(state.sequence, 0);
}
#[cfg_attr(test, test)]
fn console_runtime_storage_late_clock_cancellation_preserves_timer() {
    end_cancel(false);
}
#[cfg_attr(test, test)]
fn console_runtime_storage_neutral_late_clock_cancellation_preserves_timer() {
    end_cancel(true);
}
fn header_cancel(clear: bool) {
    let mut vm = page();
    run(&mut vm, "console.log('one');console.group('group');0;");
    let state = vm.host_console.as_ref().unwrap();
    let before = (
        state.records.len(),
        state.bytes,
        state.sequence,
        state.depth,
    );
    cancel_clock(&mut vm, 1);
    assert_eq!(
        vm.console_call(
            if clear {
                ConsoleFunction::Clear
            } else {
                ConsoleFunction::GroupEnd
            },
            &[],
            false
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    let state = vm.host_console.as_ref().unwrap();
    assert_eq!(
        (
            state.records.len(),
            state.bytes,
            state.sequence,
            state.depth
        ),
        before
    );
}
#[cfg_attr(test, test)]
fn console_runtime_storage_clock_cancellation_preserves_clear_state() {
    header_cancel(true);
}
#[cfg_attr(test, test)]
fn console_runtime_storage_clock_cancellation_preserves_group_depth() {
    header_cancel(false);
}
fn label_sample(text: &str, neutral: bool) -> usize {
    let mut vm = page();
    run(
        &mut vm,
        &format!("var label='{text}',value={{toString(){{return label;}}}};console.time(value);0;"),
    );
    let value = object(&mut vm, "value");
    let observer = view(&vm);
    let baseline = Rc::new(Cell::new(0));
    let before = baseline.clone();
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    vm.install_host_timer_functions_with_clock(move || {
        if output.get().is_none() {
            output.set(Some(
                observer.host_scheduling_snapshot().payload_bytes - before.get(),
            ));
        }
        5
    })
    .unwrap();
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(observer.host_scheduling_snapshot().payload_bytes);
    vm.console_call(ConsoleFunction::TimeLog, &[Value::Object(value)], false)
        .unwrap();
    sample.get().unwrap()
}
#[cfg_attr(test, test)]
fn console_runtime_storage_label_key_and_display_stay_paid_through_clock() {
    assert_eq!(
        label_sample("abcdefghijklmno", false) - label_sample("abc", false),
        32
    );
}
#[cfg_attr(test, test)]
fn console_runtime_storage_neutral_label_key_and_display_keep_credit() {
    assert_eq!(
        label_sample("abcdefghijklmno", true) - label_sample("abc", true),
        32
    );
}
#[cfg_attr(test, test)]
fn console_runtime_storage_utf16_label_identity_remains_distinct() {
    let mut vm = page();
    run(
        &mut vm,
        "console.count('\\ud800');console.count('�');console.count('\\ud800');0;",
    );
    let records = vm.drain_host_console().unwrap().records;
    assert_eq!(records.len(), 3);
    assert!(
        records[0].message.ends_with(": 1")
            && records[1].message.ends_with(": 1")
            && records[2].message.ends_with(": 2")
    );
}
#[cfg_attr(test, test)]
fn console_runtime_storage_late_adoption_covers_label_backing_and_keys() {
    let mut vm = Vm::default();
    vm.install_host_console().unwrap();
    run(&mut vm, "console.count('one');console.time('two');0;");
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let observer = view(&vm);
    let state = vm.host_console.take().unwrap();
    let bytes = state.payload_bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(state);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn console_runtime_storage_canceling_label_conversion_leaves_state_unchanged() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var value={toString(){cancel();return 'one';}};0;");
    let value = object(&mut vm, "value");
    assert_eq!(
        vm.console_call(ConsoleFunction::Count, &[Value::Object(value)], false),
        Err(RuntimeError::SchedulingLimit)
    );
    let state = vm.host_console.as_ref().unwrap();
    assert!(state.counts.is_empty() && state.records.is_empty());
}
#[cfg_attr(test, test)]
fn console_runtime_storage_throwing_substitution_preserves_console_records() {
    let mut vm = page();
    run(
        &mut vm,
        "console.log('one');var value={toString(){throw 7;}};0;",
    );
    let value = object(&mut vm, "value");
    assert_eq!(
        vm.console_call(
            ConsoleFunction::Log,
            &[Value::String("%s".into()), Value::Object(value)],
            false
        ),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    let records = vm.drain_host_console().unwrap().records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].message, "one");
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn console_runtime_storage_clock_cancellation_preserves_existing_count() {
    let mut vm = page();
    run(&mut vm, "console.count('one');0;");
    cancel_clock(&mut vm, 1);
    assert_eq!(
        vm.console_call(
            ConsoleFunction::Count,
            &[Value::String("one".into())],
            false
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    let state = vm.host_console.as_ref().unwrap();
    assert_eq!(state.counts.get(&key("one")), Some(&1));
    assert_eq!(state.records.len(), 1);
    assert_eq!(state.sequence, 1);
}

fn refused_context(foreign: bool) {
    let mut vm = page();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        observed.set(observed.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        "var value={toString(){observe();return 'one';}};0;",
    );
    let value = object(&mut vm, "value");
    let original = view(&vm);
    if foreign {
        let other = page();
        vm.host_scheduling = other.host_scheduling.as_ref().map(SchedulingScope::child);
    } else {
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        vm.console_call(ConsoleFunction::Count, &[Value::Object(value)], false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 0);
    let state = vm.host_console.as_ref().unwrap();
    assert!(state.counts.is_empty() && state.records.is_empty());
    assert!(vm.realm_call_roots.is_none());
    assert_eq!(original.check_scheduling_failure().is_ok(), foreign);
}

#[cfg_attr(test, test)]
fn console_runtime_storage_foreign_entry_stops_before_label_conversion() {
    refused_context(true);
}

#[cfg_attr(test, test)]
fn console_runtime_storage_failed_original_neutral_entry_stops_before_conversion() {
    refused_context(false);
}

#[cfg_attr(test, test)]
fn console_runtime_storage_retained_root_caller_cancellation_stops_publication() {
    let mut vm = page();
    let original = view(&vm);
    let caller = page();
    let scope = caller.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var value={toString(){cancel();return 'one';}};0;");
    let value = object(&mut vm, "value");
    drop(vm.host_scheduling.take());
    let owner = caller.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    assert_eq!(
        vm.with_realm_call_roots(roots, |vm| vm.console_call(
            ConsoleFunction::Log,
            &[Value::String("prefix %s".into()), Value::Object(value)],
            false,
        )),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(original.check_scheduling_failure().is_ok());
    assert!(vm.host_console.as_ref().unwrap().records.is_empty());
    assert!(vm.realm_call_roots.is_none());
}

fn detached_preparation(neutral: bool) {
    let mut vm = page();
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let owners = vm.console_owners(None).unwrap();
    let baseline = observer.host_scheduling_snapshot().payload_bytes;
    let label = vm
        .console_label(Some(&Value::String("one".into())), &owners)
        .unwrap()
        .unwrap();
    let display = label.display;
    let display_bytes = display.text.capacity();
    let prepared = vm
        .host_console
        .as_ref()
        .unwrap()
        .counts
        .prepare(label.key)
        .unwrap();
    let bytes = observer.host_scheduling_snapshot().payload_bytes - baseline;
    assert!(bytes > display_bytes + key("one").len() * 2);
    drop(owners);
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(display);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        bytes - display_bytes
    );
    drop(prepared);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn console_runtime_storage_prepared_label_and_display_outlive_vm_independently() {
    detached_preparation(false);
}

#[cfg_attr(test, test)]
fn console_runtime_storage_neutral_preparation_keeps_original_credit_after_vm_drop() {
    detached_preparation(true);
}

#[cfg_attr(test, test)]
fn console_runtime_storage_sparse_timer_table_keeps_slots_until_last_removal() {
    let mut vm = page();
    run(&mut vm, "console.time('one');console.time('two');0;");
    let observer = view(&vm);
    let mut state = vm.host_console.take().unwrap();
    drop(vm);
    let before = observer.host_scheduling_snapshot().payload_bytes;
    assert_eq!(before, state.payload_bytes());
    state.timers.remove(&key("one"));
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        before - 6
    );
    assert!(state.timers.owned_bytes() > 6);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        state.payload_bytes()
    );
    state.timers.remove(&key("two"));
    assert_eq!(state.timers.owned_bytes(), 0);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        state.payload_bytes()
    );
    drop(state);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn console_runtime_storage_finished_preview_roots_release_before_normal_collection() {
    let mut vm = page();
    let value = vm.heap.alloc_object(None).unwrap();
    vm.console_call(ConsoleFunction::Dir, &[Value::Object(value)], false)
        .unwrap();
    assert!(vm.realm_call_roots.is_none());
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(value));
    assert_eq!(vm.drain_host_console().unwrap().records.len(), 1);
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_console_runtime_storage_contracts() {
        console_runtime_storage_formatted_prefix_stays_paid_through_next_conversion();
        console_runtime_storage_neutral_prefix_keeps_original_credit();
        console_runtime_storage_detached_label_maps_keep_credit_after_vm_drop();
        console_runtime_storage_neutral_detached_labels_keep_original_credit();
        console_runtime_storage_clock_cancellation_preserves_count_map();
        console_runtime_storage_neutral_clock_cancellation_preserves_count_map();
        console_runtime_storage_late_clock_cancellation_preserves_timer();
        console_runtime_storage_neutral_late_clock_cancellation_preserves_timer();
        console_runtime_storage_clock_cancellation_preserves_clear_state();
        console_runtime_storage_clock_cancellation_preserves_group_depth();
        console_runtime_storage_label_key_and_display_stay_paid_through_clock();
        console_runtime_storage_neutral_label_key_and_display_keep_credit();
        console_runtime_storage_utf16_label_identity_remains_distinct();
        console_runtime_storage_late_adoption_covers_label_backing_and_keys();
        console_runtime_storage_canceling_label_conversion_leaves_state_unchanged();
        console_runtime_storage_throwing_substitution_preserves_console_records();
        console_runtime_storage_clock_cancellation_preserves_existing_count();
        console_runtime_storage_foreign_entry_stops_before_label_conversion();
        console_runtime_storage_failed_original_neutral_entry_stops_before_conversion();
        console_runtime_storage_retained_root_caller_cancellation_stops_publication();
        console_runtime_storage_prepared_label_and_display_outlive_vm_independently();
        console_runtime_storage_neutral_preparation_keeps_original_credit_after_vm_drop();
        console_runtime_storage_sparse_timer_table_keeps_slots_until_last_removal();
        console_runtime_storage_finished_preview_roots_release_before_normal_collection();
    }
}
