// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed default-configuration event input ownership and cancellation.

use super::*;
use std::cell::Cell;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    vm.install_web_error_events().unwrap();
    vm.install_web_rejection_events().unwrap();
    vm
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn object(vm: &mut Vm, name: &str) -> ObjectId {
    vm.lookup_global_name(name)
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap()
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn construct(vm: &mut Vm, rank: u8, args: &[Value]) -> Result<Value, RuntimeError> {
    let key = format!("%Web{}%", EVENT_NAMES[rank as usize]);
    let constructor = vm
        .heap
        .get_own(vm.globals[&key], "constructor")
        .unwrap()
        .unwrap();
    let previous = std::mem::replace(&mut vm.new_target, constructor);
    let result = vm.construct_web_event(rank, args, true);
    vm.new_target = previous;
    result
}

fn construction_sample(field: &str, text: &str, neutral: bool) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let baseline = Rc::new(Cell::new(0));
    let before = baseline.clone();
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(Some(
            view.host_scheduling_snapshot().payload_bytes - before.get(),
        ));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let (rank, name, options) = match field {
        "type" => (
            0,
            text,
            "{get bubbles(){observe();return false;}}".to_owned(),
        ),
        "filename" => (
            6,
            "x",
            format!("{{filename:'{text}',get lineno(){{observe();return 0;}}}}"),
        ),
        "error" => (
            6,
            "x",
            format!("{{error:'{text}',get filename(){{observe();return '';}}}}"),
        ),
        _ => unreachable!(),
    };
    run(&mut vm, &format!("var options={options};0;"));
    let options = object(&mut vm, "options");
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(view.host_scheduling_snapshot().payload_bytes);
    construct(
        &mut vm,
        rank,
        &[Value::String(name.into()), Value::Object(options)],
    )
    .unwrap();
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn web_event_storage_scoped_constructor_name_is_paid_during_dictionary_getters() {
    assert_eq!(
        construction_sample("type", "abc", false) - construction_sample("type", "a", false),
        4
    );
}

#[cfg_attr(test, test)]
fn web_event_storage_neutral_constructor_name_keeps_original_credit() {
    assert_eq!(
        construction_sample("type", "abc", true) - construction_sample("type", "a", true),
        4
    );
}

#[cfg_attr(test, test)]
fn web_event_storage_filename_is_paid_before_later_dictionary_getters() {
    assert_eq!(
        construction_sample("filename", "abc", true) - construction_sample("filename", "a", true),
        4
    );
}

#[cfg_attr(test, test)]
fn web_event_storage_neutral_error_payload_is_paid_before_filename_conversion() {
    assert_eq!(
        construction_sample("error", "abc", true) - construction_sample("error", "a", true),
        4
    );
}

fn legacy_sample(name: &str, neutral: bool) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let baseline = Rc::new(Cell::new(0));
    let before = baseline.clone();
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(Some(
            view.host_scheduling_snapshot().payload_bytes - before.get(),
        ));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        "var event=new MouseEvent('old'),detail={valueOf(){observe();return 1;}};0;",
    );
    let event = object(&mut vm, "event");
    let detail = object(&mut vm, "detail");
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(view.host_scheduling_snapshot().payload_bytes);
    vm.web_event_call(
        EventNative::InitMouseEvent,
        &Value::Object(event),
        &[
            Value::String(name.into()),
            Value::Bool(false),
            Value::Bool(true),
            Value::Null,
            Value::Object(detail),
        ],
        false,
    )
    .unwrap();
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn web_event_storage_legacy_name_is_paid_through_coordinate_conversion() {
    assert_eq!(legacy_sample("abc", false) - legacy_sample("a", false), 4);
}

#[cfg_attr(test, test)]
fn web_event_storage_neutral_legacy_name_keeps_original_credit() {
    assert_eq!(legacy_sample("abc", true) - legacy_sample("a", true), 4);
}

fn refused_operation(member: bool, foreign: bool) {
    let mut vm = page();
    let original = observer(&vm);
    run(&mut vm, "var event=new Event('x',{cancelable:true});0;");
    let event = object(&mut vm, "event");
    let scope = vm.host_scheduling.take().unwrap();
    let borrower = page();
    if foreign {
        vm.host_scheduling = borrower
            .host_scheduling
            .as_ref()
            .map(SchedulingScope::child);
    } else {
        let _ = scope.fail();
    }
    let action = |vm: &mut Vm| {
        if member {
            vm.web_event_call(
                EventNative::PreventDefault,
                &Value::Object(event),
                &[],
                false,
            )
        } else {
            construct(vm, 0, &[Value::String("new".into())])
        }
    };
    let before = vm.heap.stats();
    let result = if foreign {
        action(&mut vm)
    } else {
        let owner = borrower.argument_list_owner().unwrap();
        let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
        vm.with_realm_call_roots(roots, action)
    };
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!vm.heap.drag_event(event).unwrap().default_prevented);
    let after = vm.heap.stats();
    assert_eq!(
        before.nursery_objects + before.tenured_objects,
        after.nursery_objects + after.tenured_objects
    );
    if foreign {
        assert!(original.check_scheduling_failure().is_ok());
    } else {
        assert!(borrower.check_scheduling_failure().is_ok());
    }
}

#[cfg_attr(test, test)]
fn web_event_storage_failed_original_constructor_cannot_borrow_healthy_credit() {
    refused_operation(false, false);
}

#[cfg_attr(test, test)]
fn web_event_storage_foreign_constructor_refuses_before_heap_publication() {
    refused_operation(false, true);
}

#[cfg_attr(test, test)]
fn web_event_storage_failed_original_member_cannot_publish_cancellation() {
    refused_operation(true, false);
}

#[cfg_attr(test, test)]
fn web_event_storage_foreign_member_refuses_without_poisoning_original() {
    refused_operation(true, true);
}

#[cfg_attr(test, test)]
fn web_event_storage_terminal_dictionary_getter_never_enters_later_members() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let calls = Rc::new(Cell::new(0));
    let cancelled = calls.clone();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        cancelled.set(cancelled.get() + 1);
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let next = calls.clone();
    vm.install_host_function("peer", 0, move |_: &[HostValue]| {
        next.set(next.get() + 10);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var options={get bubbles(){cancel();return false;},get cancelable(){peer();return true;}};0;");
    let options = object(&mut vm, "options");
    drop(vm.host_scheduling.take());
    assert_eq!(
        construct(
            &mut vm,
            0,
            &[Value::String("x".into()), Value::Object(options)]
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 1);
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn web_event_storage_default_dictionary_legacy_and_reentrant_semantics() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var order=[],detail={marker:7},event=new CustomEvent('x',{get bubbles(){order.push('b');return true;},get cancelable(){order.push('c');return true;},get composed(){order.push('m');return true;},get detail(){order.push('d');return detail;}});
        var target=new EventTarget(),observed=false;
        target.addEventListener('x',()=>{event.initCustomEvent({toString(){order.push('active');return 'other';}},false,false,8);observed=event.type==='x'&&event.detail===detail;});
        target.dispatchEvent(event);event.preventDefault();event.initCustomEvent({toString(){event.preventDefault();return 'new';}},false,true,9);
        var ui=new UIEvent('u',{detail:7}),drag=new DragEvent('d'),progress=new ProgressEvent('p',{lengthComputable:true,loaded:3,total:9});
        var mouse=new MouseEvent('m',{clientX:1.5,buttons:65537,button:65535,ctrlKey:true}),error=new ErrorEvent('e',{filename:'a\uD800b\uD83D\uDE00\uDC00',message:'text',error:detail}),promise=new PromiseRejectionEvent('p',{promise:{},reason:detail});
        ui.detail===7&&drag.dataTransfer===null&&progress.lengthComputable&&progress.loaded===3&&progress.total===9&&observed&&order.join()==='b,c,m,d,active'&&event.type==='new'&&event.detail===9&&!event.defaultPrevented&&mouse.clientX===1.5&&mouse.buttons===1&&mouse.button===-1&&mouse.getModifierState('Control')&&error.filename==='a\uFFFDb\uD83D\uDE00\uFFFD'&&error.error===detail&&promise.reason===detail;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn web_event_storage_normal_gc_keeps_constructed_payload_until_event_release() {
    let mut vm = page();
    run(
        &mut vm,
        "var detail={marker:7},event=new CustomEvent('x',{detail});0;",
    );
    let detail = object(&mut vm, "detail");
    run(&mut vm, "detail=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(detail));
    assert_eq!(run(&mut vm, "event.detail.marker;"), Value::Number(7.0));
    run(&mut vm, "event=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(detail));
}

#[cfg_attr(test, test)]
fn web_event_storage_growing_draft_keeps_spare_primitive_storage_after_vm_drop() {
    let vm = page();
    let view = observer(&vm);
    let context = EventContext::capture(&vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let roots = context.prepare_roots(&vm, std::iter::empty(), 0).unwrap();
    let name = NativeValue::copied(&Value::String("x".into()), context.owner.as_ref()).unwrap();
    let mut state = NativeEventState::new(&vm, 0, name).unwrap();
    let mut units = Vec::with_capacity(9);
    units.push(u16::from(b'a'));
    let detail = NativeValue::adopt(
        Value::String(JsString::from_code_units(units)),
        context.owner.as_ref(),
    )
    .unwrap();
    state.set(&vm, &roots, "detail", detail).unwrap();
    drop(roots);
    let bytes = state.state.owned_buffer_bytes();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(state);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn web_event_storage_copied_active_path_keeps_credit_after_vm_drop() {
    use crate::vm::web_event_storage::EventPath;
    let mut vm = page();
    run(
        &mut vm,
        "var target=new EventTarget(),event=new CustomEvent('x',{detail:'abc'});0;",
    );
    let event = object(&mut vm, "event");
    let target = object(&mut vm, "target");
    let context = EventContext::capture(&vm).unwrap();
    EventPath::single(target, context.owner.as_ref())
        .unwrap()
        .publish(&mut vm, event, target, false)
        .unwrap();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let copied = NativeEventState::copied(&vm, event, context.owner.as_ref()).unwrap();
    let bytes = copied.state.owned_buffer_bytes();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    vm.heap.finish_event_dispatch(event).unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copied);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_web_event_storage_contracts() {
        web_event_storage_scoped_constructor_name_is_paid_during_dictionary_getters();
        web_event_storage_neutral_constructor_name_keeps_original_credit();
        web_event_storage_filename_is_paid_before_later_dictionary_getters();
        web_event_storage_neutral_error_payload_is_paid_before_filename_conversion();
        web_event_storage_legacy_name_is_paid_through_coordinate_conversion();
        web_event_storage_neutral_legacy_name_keeps_original_credit();
        web_event_storage_failed_original_constructor_cannot_borrow_healthy_credit();
        web_event_storage_foreign_constructor_refuses_before_heap_publication();
        web_event_storage_failed_original_member_cannot_publish_cancellation();
        web_event_storage_foreign_member_refuses_without_poisoning_original();
        web_event_storage_terminal_dictionary_getter_never_enters_later_members();
        web_event_storage_default_dictionary_legacy_and_reentrant_semantics();
        web_event_storage_normal_gc_keeps_constructed_payload_until_event_release();
        web_event_storage_growing_draft_keeps_spare_primitive_storage_after_vm_drop();
        web_event_storage_copied_active_path_keeps_credit_after_vm_drop();
    }
}
