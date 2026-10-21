// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed error reports retain input and console storage through callbacks and drop.

use super::*;
use std::cell::Cell;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_window_aliases().unwrap();
    vm.install_host_error_reporting().unwrap();
    run(&mut vm, "0;");
    vm
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
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

fn origin(capacity: usize) -> crate::diagnostics::DiagnosticSourceLocation {
    let mut filename = String::with_capacity(capacity);
    filename.push_str("input.js");
    crate::diagnostics::DiagnosticSourceLocation {
        filename,
        line: 2,
        column: 3,
    }
}

fn origin_sample(capacity: usize, neutral: bool) -> (usize, usize) {
    let mut vm = page();
    let source = observer(&vm);
    let view = observer(&vm);
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        assert!(output.get().is_none());
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "onerror=function(){observe();return true};0;");
    let origin = origin(capacity);
    let actual = origin.filename.capacity();
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let report = |vm: &mut Vm| {
        vm.report_host_exception_with_origin(
            &RuntimeError::Thrown(Value::Number(7.0)),
            Some(origin),
        )
    };
    if neutral {
        loan(&mut vm, &source, report).unwrap();
    } else {
        report(&mut vm).unwrap();
    }
    assert!(vm.host_console.as_ref().unwrap().records.is_empty());
    (actual, sample.get().unwrap())
}

#[cfg_attr(test, test)]
fn host_report_storage_scoped_origin_keeps_its_actual_capacity_during_callback() {
    let small = origin_sample(1024, false);
    let large = origin_sample(2048, false);
    assert_eq!(large.1 - small.1, large.0 - small.0);
}

#[cfg_attr(test, test)]
fn host_report_storage_neutral_origin_keeps_its_actual_capacity_during_callback() {
    let small = origin_sample(1024, true);
    let large = origin_sample(2048, true);
    assert_eq!(large.1 - small.1, large.0 - small.0);
}

fn detached_console(neutral: bool) {
    let mut vm = page();
    let view = observer(&vm);
    let source = observer(&vm);
    vm.host_error_reporting = Some(true);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let report = |vm: &mut Vm| {
        vm.report_host_exception_with_origin(
            &RuntimeError::Thrown(Value::Number(7.0)),
            Some(origin(32)),
        )
    };
    if neutral {
        loan(&mut vm, &source, report).unwrap();
    } else {
        report(&mut vm).unwrap();
    }
    let console = vm.host_console.take().unwrap();
    assert_eq!(console.records.len(), 1);
    let bytes = console.payload_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(console);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_report_storage_scoped_console_data_outlives_the_vm_with_native_credit() {
    detached_console(false);
}

#[cfg_attr(test, test)]
fn host_report_storage_neutral_console_data_outlives_the_vm_with_native_credit() {
    detached_console(true);
}

#[cfg_attr(test, test)]
fn host_report_storage_another_caller_cannot_revive_the_console_owner() {
    let mut vm = page();
    let source = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.host_error_reporting = Some(true);
    drop(vm.host_scheduling.take());
    let mut caller = Vm::default();
    caller.use_bounded_host_microtask_checkpoints().unwrap();
    let _ = scope.fail();
    assert_eq!(
        loan(&mut vm, &caller, |vm| vm.report_host_exception_with_origin(
            &RuntimeError::Thrown(Value::Number(7.0)),
            None
        )),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.host_console.as_ref().unwrap().records.is_empty());
    assert!(caller.check_scheduling_failure().is_ok());
    assert_eq!(
        source.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn host_report_storage_clock_cancellation_precedes_record_and_sequence_publication() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_timer_functions_with_clock(move || {
        let _ = scope.fail();
        0
    })
    .unwrap();
    assert_eq!(
        vm.console_emit_with_source(
            ConsoleFunction::Error,
            "error",
            Output::message("tiny"),
            None
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    let console = vm.host_console.as_ref().unwrap();
    assert!(console.records.is_empty());
    assert_eq!(console.sequence, 0);
}

#[cfg_attr(test, test)]
fn host_report_storage_records_keep_previews_without_retaining_live_objects() {
    let mut vm = page();
    vm.host_error_reporting = Some(true);
    let object = vm.heap.alloc_object(None).unwrap();
    vm.report_host_exception_with_origin(
        &RuntimeError::Thrown(Value::Object(object)),
        Some(origin(32)),
    )
    .unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(object));
    let batch = vm.drain_host_console().unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(
        batch.records[0].source.as_ref().unwrap().filename,
        "input.js"
    );
    assert!(vm.host_console.as_ref().unwrap().records.is_empty());
}

#[cfg_attr(test, test)]
fn host_report_storage_failed_calling_account_cannot_publish_a_record() {
    let mut vm = page();
    let view = observer(&vm);
    vm.host_error_reporting = Some(true);
    drop(vm.host_scheduling.take());
    let mut caller = Vm::default();
    caller.use_bounded_host_microtask_checkpoints().unwrap();
    let result = loan(&mut vm, &caller, |vm| {
        let _ = caller.host_scheduling.as_ref().unwrap().fail();
        vm.report_host_exception_with_origin(&RuntimeError::Thrown(Value::Number(7.0)), None)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(view.check_scheduling_failure().is_ok());
    assert!(vm.host_console.as_ref().unwrap().records.is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_report_storage_contracts() {
        host_report_storage_scoped_origin_keeps_its_actual_capacity_during_callback();
        host_report_storage_neutral_origin_keeps_its_actual_capacity_during_callback();
        host_report_storage_scoped_console_data_outlives_the_vm_with_native_credit();
        host_report_storage_neutral_console_data_outlives_the_vm_with_native_credit();
        host_report_storage_another_caller_cannot_revive_the_console_owner();
        host_report_storage_clock_cancellation_precedes_record_and_sequence_publication();
        host_report_storage_records_keep_previews_without_retaining_live_objects();
        host_report_storage_failed_calling_account_cannot_publish_a_record();
    }
}
