// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed borrowed diagnostics and numeric text retained across radix conversion.

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
fn output(vm: &mut Vm) -> Output {
    let owners = vm.console_owners(None).unwrap();
    let mut output =
        Output::message_owned("warm output capacity stays paid", owners.owner.as_ref()).unwrap();
    output.text.clear();
    output
}
fn allocation_free(operation: impl FnOnce() -> Result<(), RuntimeError>) {
    #[cfg(test)]
    {
        let (result, count) = crate::vm::allocation_observer::count(operation);
        result.unwrap();
        assert_eq!(
            count, 0,
            "borrowed diagnostic text needs no copied key/value"
        );
    }
    #[cfg(all(coverage, not(test)))]
    operation().unwrap();
}
#[cfg_attr(test, test)]
fn console_text_storage_function_preview_borrows_its_stored_name() {
    let mut vm = page();
    run(&mut vm, "function named(){};0;");
    let function = object(&mut vm, "named");
    let mut text = output(&mut vm);
    allocation_free(|| vm.console_preview(&Value::Object(function), &mut text));
    assert_eq!(text.text, "[Function: named]");
}
#[cfg_attr(test, test)]
fn console_text_storage_error_preview_borrows_its_stored_message() {
    let mut vm = page();
    run(&mut vm, "var error=new Error('kept');0;");
    let error = object(&mut vm, "error");
    let mut text = output(&mut vm);
    allocation_free(|| vm.console_preview(&Value::Object(error), &mut text));
    assert_eq!(text.text, "[Object: kept]");
}
#[cfg_attr(test, test)]
fn console_text_storage_trace_borrows_its_stored_function_name() {
    let mut vm = page();
    run(&mut vm, "function named(){};0;");
    let function = object(&mut vm, "named");
    let mut text = output(&mut vm);
    vm.call_stack.push(function);
    allocation_free(|| vm.console_trace(&mut text));
    vm.call_stack.pop();
    assert_eq!(text.text, "\n    at named");
}
fn radix_sample(text: &str, neutral: bool) -> usize {
    let mut vm = page();
    let observer = view(&vm);
    let before = Rc::new(Cell::new(0));
    let baseline = before.clone();
    let observed = Rc::new(Cell::new(None));
    let sample = observed.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        sample.set(Some(
            observer.host_scheduling_snapshot().payload_bytes - baseline.get(),
        ));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm,&format!("var text='{text}',input={{toString(){{return text;}}}},radix={{valueOf(){{observe();return 10;}}}};0;"));
    let input = object(&mut vm, "input");
    let radix = object(&mut vm, "radix");
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    before.set(observer.host_scheduling_snapshot().payload_bytes);
    assert!(matches!(
        vm.parse_int(&Value::Object(input), &Value::Object(radix))
            .unwrap(),
        Value::Number(_)
    ));
    assert!(vm.realm_call_roots.is_none());
    observed.get().unwrap()
}
#[cfg_attr(test, test)]
fn console_text_storage_parse_int_keeps_both_text_encodings_through_radix_callback() {
    assert_eq!(
        radix_sample("123456789012345", false) - radix_sample("123", false),
        36
    );
}
#[cfg_attr(test, test)]
fn console_text_storage_neutral_parse_int_keeps_original_text_credit() {
    assert_eq!(
        radix_sample("123456789012345", true) - radix_sample("123", true),
        36
    );
}
fn refused(foreign: bool) {
    let mut vm = page();
    let original = view(&vm);
    if foreign {
        let other = page();
        vm.host_scheduling = other.host_scheduling.as_ref().map(SchedulingScope::child);
    } else {
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        vm.parse_int(&Value::String("12".into()), &Value::Number(10.0)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.parse_float(&Value::String("12".into())),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(original.check_scheduling_failure().is_ok(), foreign);
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn console_text_storage_foreign_numeric_entry_refuses_before_conversion() {
    refused(true);
}
#[cfg_attr(test, test)]
fn console_text_storage_failed_original_neutral_numeric_entry_is_refused() {
    refused(false);
}
#[cfg_attr(test, test)]
fn console_text_storage_numeric_prefix_whitespace_and_utf16_semantics_stay_intact() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"parseInt(' -0x10tail',0)===-16 && Object.is(parseInt('-0x',10),-0) && parseFloat('　-.25e+2tail')===-25 && parseFloat('1e+tail')===1 && parseFloat('+Infinityx')===Infinity && Number.isNaN(parseFloat('\\ud8001')) && parseInt('12\\ud8003',10)===12"),Value::Bool(true));
}
#[cfg_attr(test, test)]
fn console_text_storage_radix_throw_restores_roots_and_releases_converted_text() {
    let mut vm = page();
    run(&mut vm, "var radix={valueOf(){throw 7;}};0;");
    let radix = object(&mut vm, "radix");
    let prefix = vm.stack.len();
    assert_eq!(
        vm.parse_int(&Value::String("123".into()), &Value::Object(radix)),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(vm.stack.len(), prefix);
    assert!(vm.realm_call_roots.is_none());
    let observer = view(&vm);
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn console_text_storage_retained_caller_cancellation_during_radix_stops_return() {
    let mut vm = page();
    let caller = page();
    let original = view(&vm);
    let scope = caller.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var radix={valueOf(){cancel();return 10;}};0;");
    let radix = object(&mut vm, "radix");
    drop(vm.host_scheduling.take());
    let owner = caller.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    assert_eq!(
        vm.with_realm_call_roots(roots, |vm| vm
            .parse_int(&Value::String("123".into()), &Value::Object(radix))),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(original.check_scheduling_failure().is_ok());
    assert!(vm.realm_call_roots.is_none());
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_console_text_storage_contracts() {
        console_text_storage_function_preview_borrows_its_stored_name();
        console_text_storage_error_preview_borrows_its_stored_message();
        console_text_storage_trace_borrows_its_stored_function_name();
        console_text_storage_parse_int_keeps_both_text_encodings_through_radix_callback();
        console_text_storage_neutral_parse_int_keeps_original_text_credit();
        console_text_storage_foreign_numeric_entry_refuses_before_conversion();
        console_text_storage_failed_original_neutral_numeric_entry_is_refused();
        console_text_storage_numeric_prefix_whitespace_and_utf16_semantics_stay_intact();
        console_text_storage_radix_throw_restores_roots_and_releases_converted_text();
        console_text_storage_retained_caller_cancellation_during_radix_stops_return();
    }
}
