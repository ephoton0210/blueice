// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed default-configuration EventTarget accounting and terminal cleanup.

use super::*;
use crate::heap::DragEventState;
use std::cell::Cell;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
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

fn actual_state_bytes(state: &DragEventState) -> usize {
    std::mem::size_of::<DragEventState>()
        + host_scheduling::slots(&state.attributes)
        + host_scheduling::slots(&state.path)
        + state
            .attributes
            .iter()
            .map(|(_, value)| host_scheduling::value_payload_bytes(value))
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn event_target_storage_managed_event_keeps_all_container_capacity_paid() {
    let mut state = DragEventState::new(0);
    state.attributes = Vec::with_capacity(11);
    state.path = Vec::with_capacity(3);
    state.set("type", Value::String("x".into()));
    assert_eq!(state.bytes(), actual_state_bytes(&state));
}

#[cfg_attr(test, test)]
fn event_target_storage_managed_event_keeps_spare_primitive_storage_paid() {
    let mut units = Vec::with_capacity(16);
    units.push(u16::from(b'x'));
    let mut state = DragEventState::new(5);
    state.set("detail", Value::String(JsString::from_code_units(units)));
    state.set("number", Value::BigInt(42.into()));
    assert_eq!(state.bytes(), actual_state_bytes(&state));
}

fn native_sample(name: &str, count: usize, neutral: bool, completion: Option<&str>) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let baseline_view = observer(&vm);
    let baseline = Rc::new(Cell::new(0usize));
    let before = baseline.clone();
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        if output.get().is_none() {
            output.set(Some(
                view.host_scheduling_snapshot().payload_bytes - before.get(),
            ));
        }
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = completion.map_or_else(
        || "function(){observe();}".to_owned(),
        |value| format!("function(){{Promise.resolve().then(()=>observe());return '{value}';}}"),
    );
    run(
        &mut vm,
        &format!(
            "var target=new EventTarget(),event=new Event('{name}');
             for(var i=0;i<{count};i++)target.addEventListener('{name}',{callback});0;"
        ),
    );
    let target = object(&mut vm, "target");
    let event = object(&mut vm, "event");
    if neutral {
        drop(vm.host_scheduling.take());
    }
    // The observer captured by the host callback shares the original account.
    baseline.set(baseline_view.host_scheduling_snapshot().payload_bytes);
    vm.dispatch_web_target_event(target, event, false).unwrap();
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn event_target_storage_scoped_dispatch_name_copy_stays_paid_during_callback() {
    assert_eq!(
        native_sample("abc", 1, false, None) - native_sample("a", 1, false, None),
        4
    );
}

#[cfg_attr(test, test)]
fn event_target_storage_neutral_dispatch_name_keeps_the_original_account() {
    assert_eq!(
        native_sample("abc", 1, true, None) - native_sample("a", 1, true, None),
        4
    );
}

#[cfg_attr(test, test)]
fn event_target_storage_serial_snapshot_stays_paid_across_the_callback() {
    assert_eq!(
        native_sample("x", 3, false, None) - native_sample("x", 1, false, None),
        16
    );
}

#[cfg_attr(test, test)]
fn event_target_storage_neutral_callback_completion_stays_paid_through_checkpoint() {
    assert_eq!(
        native_sample("x", 1, true, Some("abc")) - native_sample("x", 1, true, Some("a")),
        4
    );
}

fn terminal_callback(language_error: bool) {
    let mut vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let calls = Rc::new(Cell::new(0usize));
    let output = calls.clone();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        output.set(output.get() + 1);
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let peer = calls.clone();
    vm.install_host_function("peer", 0, move |_: &[HostValue]| {
        peer.set(peer.get() + 10);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        &format!(
            "var target=new EventTarget(),event=new Event('x');
         target.addEventListener('x',()=>{{cancel();{}}});
         target.addEventListener('x',()=>peer());0;",
            if language_error { "throw 7;" } else { "" }
        ),
    );
    let target = object(&mut vm, "target");
    let event = object(&mut vm, "event");
    drop(vm.host_scheduling.take());
    let result = vm.dispatch_web_target_event(target, event, false);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(calls.get(), 1);
    let state = vm.heap.drag_event(event).unwrap();
    assert_eq!(state.phase, 0);
    assert_eq!(state.current_target, None);
    assert!(state.path.is_empty() && !state.passive);
    assert!(!state.propagation_stopped && !state.immediate_stopped);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn event_target_storage_neutral_last_success_checks_original_cancellation() {
    terminal_callback(false);
}

#[cfg_attr(test, test)]
fn event_target_storage_neutral_language_error_cannot_hide_original_cancellation() {
    terminal_callback(true);
}

#[cfg_attr(test, test)]
fn event_target_storage_failed_original_owner_cannot_borrow_a_healthy_root_account() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let borrower = page();
    let entered = Rc::new(Cell::new(false));
    let output = entered.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var target=new EventTarget(),event=new Event('x');target.addEventListener('x',()=>observe());0;");
    let target = object(&mut vm, "target");
    let event = object(&mut vm, "event");
    drop(vm.host_scheduling.take());
    let _ = scope.fail();
    let owner = borrower.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let result = vm.with_realm_call_roots(roots, |vm| {
        vm.dispatch_web_target_event(target, event, false)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!entered.get());
    assert!(borrower.check_scheduling_failure().is_ok());
    assert_eq!(
        vm.heap.drag_event(event).unwrap().get("target"),
        Value::Null
    );
}

#[cfg_attr(test, test)]
fn event_target_storage_foreign_document_refuses_publication_without_poisoning_original() {
    let mut vm = page();
    let view = observer(&vm);
    run(
        &mut vm,
        "var target=new EventTarget(),event=new Event('x');0;",
    );
    let target = object(&mut vm, "target");
    let event = object(&mut vm, "event");
    let borrower = page();
    drop(vm.host_scheduling.take());
    vm.host_scheduling = borrower
        .host_scheduling
        .as_ref()
        .map(SchedulingScope::child);
    let result = vm.dispatch_web_target_event(target, event, false);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.heap.drag_event(event).unwrap().get("target"),
        Value::Null
    );
    assert!(view.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn event_target_storage_empty_dispatch_keeps_path_capacity_managed_after_cleanup() {
    let mut vm = page();
    run(
        &mut vm,
        "var target=new EventTarget(),event=new Event('x');0;",
    );
    let target = object(&mut vm, "target");
    let event = object(&mut vm, "event");
    let before = vm.heap.stats().managed_bytes;
    vm.dispatch_web_target_event(target, event, false).unwrap();
    let state = vm.heap.drag_event(event).unwrap();
    assert!(state.path.is_empty() && state.path.capacity() > 0);
    assert_eq!(
        vm.heap.stats().managed_bytes - before,
        host_scheduling::slots(&state.path)
    );
    assert_eq!(state.bytes(), actual_state_bytes(state));
}

#[cfg_attr(test, test)]
fn event_target_storage_normal_gc_releases_a_consumed_once_callback() {
    let mut vm = page();
    run(&mut vm, "var target=new EventTarget(),event=new Event('x'),calls=0,callback=()=>{calls++;};target.addEventListener('x',callback,{once:true});0;");
    let callback = object(&mut vm, "callback");
    let target = object(&mut vm, "target");
    let event = object(&mut vm, "event");
    run(&mut vm, "callback=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(callback));
    vm.dispatch_web_target_event(target, event, false).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(callback));
    assert!(vm
        .heap
        .web_event_target(target)
        .unwrap()
        .listeners
        .is_empty());
    assert_eq!(run(&mut vm, "calls;"), Value::Number(1.0));
}

#[cfg_attr(test, test)]
fn event_target_storage_healthy_page_exception_keeps_peer_and_cleanup_semantics() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var target=new EventTarget(),event=new Event('x'),calls=0;target.addEventListener('x',()=>{throw 7;});target.addEventListener('x',()=>calls++);target.dispatchEvent(event)&&calls===1&&event.currentTarget===null&&event.eventPhase===0;"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn event_target_storage_default_passive_once_and_live_snapshot_order() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var target=new EventTarget(),event=new Event('x',{cancelable:true}),calls=[];
        function removed(){calls.push('removed');}
        target.addEventListener('x',function(){calls.push('capture');target.removeEventListener('x',removed);target.addEventListener('x',()=>calls.push('late'));},{capture:true,once:true});
        target.addEventListener('x',function(e){e.preventDefault();calls.push('passive');},{passive:true});
        target.addEventListener('x',removed);
        target.addEventListener('x',function(){calls.push('false');return false;});
        var first=target.dispatchEvent(event),second=target.dispatchEvent(event);
        first&&second&&!event.defaultPrevented&&calls.join()==='capture,passive,false,late,passive,false,late'&&event.currentTarget===null&&event.eventPhase===0;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn event_target_storage_default_stop_cleanup_preserves_event_identity() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var target=new EventTarget(),event=new Event('x',{cancelable:true}),calls=0;
        target.addEventListener('x',function(e){if(e===event&&this===target&&e.target===target&&e.currentTarget===target&&e.composedPath()[0]===target)calls++;e.stopImmediatePropagation();e.preventDefault();});
        target.addEventListener('x',()=>{calls+=10;});
        var first=target.dispatchEvent(event),second=target.dispatchEvent(event);
        !first&&!second&&calls===2&&event.defaultPrevented&&!event.cancelBubble&&event.currentTarget===null&&event.composedPath().length===0;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn event_target_storage_empty_prepared_path_is_refused_before_publication() {
    let mut vm = page();
    run(
        &mut vm,
        "var target=new EventTarget(),event=new Event('x');0;",
    );
    let target = object(&mut vm, "target");
    let event = object(&mut vm, "event");
    let before = vm.heap.stats().managed_bytes;
    assert_eq!(
        vm.heap
            .begin_event_dispatch(event, target, false, Vec::new()),
        Err(HeapError::InvalidInternalSlot(event))
    );
    let state = vm.heap.drag_event(event).unwrap();
    assert_eq!(state.phase, 0);
    assert_eq!(state.current_target, None);
    assert_eq!(state.get("target"), Value::Null);
    assert_eq!(vm.heap.stats().managed_bytes, before);
}

#[cfg_attr(test, test)]
fn event_target_storage_context_keeps_the_inherited_callers_cancellation() {
    let mut vm = page();
    let original = observer(&vm);
    drop(vm.host_scheduling.take());
    let borrower = page();
    let mut caller = page();
    let scope = caller.host_scheduling.as_ref().unwrap().child();
    let owner = borrower.argument_list_owner().unwrap();
    let roots = RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let active = realm_reentrancy::register_active(&mut caller).unwrap();
    let checked = RefCell::new(None);
    let result = vm.with_realm_call_roots(roots, |vm| {
        // The nearest active node now borrows B too. Only the retained
        // ancestor snapshots still identify C when its account fails.
        let nested = realm_reentrancy::register_active(vm)?;
        let context = EventContext::capture(vm)?;
        let _ = scope.fail();
        *checked.borrow_mut() = Some(context.validate());
        drop(nested);
        Ok(())
    });
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        checked.into_inner(),
        Some(Err(RuntimeError::SchedulingLimit))
    );
    assert!(original.check_scheduling_failure().is_ok());
    assert!(borrower.check_scheduling_failure().is_ok());
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn event_target_storage_owned_context_ignores_unrelated_failed_ambient_work() {
    let vm = page();
    let mut unrelated = page();
    let active = realm_reentrancy::register_active(&mut unrelated).unwrap();
    let _ = unrelated.host_scheduling.as_ref().unwrap().fail();
    let result = EventContext::capture(&vm).and_then(|context| context.validate());
    drop(active);
    assert_eq!(result, Ok(()));
    assert!(vm.check_scheduling_failure().is_ok());
}

fn registration_name_sample(name: &str, neutral: bool) -> usize {
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
    run(&mut vm, "var target=new EventTarget(),callback=()=>{},options={get capture(){observe();return false;}};0;");
    let target = object(&mut vm, "target");
    let callback = object(&mut vm, "callback");
    let options = object(&mut vm, "options");
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(view.host_scheduling_snapshot().payload_bytes);
    vm.web_target_call(
        TargetNative::Add,
        &Value::Object(target),
        &[
            Value::String(name.into()),
            Value::Object(callback),
            Value::Object(options),
        ],
        false,
    )
    .unwrap();
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn event_target_registration_scoped_name_copy_stays_paid_during_options() {
    assert_eq!(
        registration_name_sample("abc", false) - registration_name_sample("a", false),
        4
    );
}

#[cfg_attr(test, test)]
fn event_target_registration_neutral_name_keeps_original_credit_during_options() {
    assert_eq!(
        registration_name_sample("abc", true) - registration_name_sample("a", true),
        4
    );
}

fn registration_cancelled_conversion(source: &str, name_conversion: bool, null_callback: bool) {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let view = observer(&vm);
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
    run(
        &mut vm,
        &format!("var target=new EventTarget(),callback=()=>{{}},input={source};0;"),
    );
    let target = object(&mut vm, "target");
    let callback = object(&mut vm, "callback");
    let input = Value::Object(object(&mut vm, "input"));
    let name = if name_conversion {
        input.clone()
    } else {
        Value::String("x".into())
    };
    let options = if name_conversion {
        Value::Undefined
    } else {
        input
    };
    drop(vm.host_scheduling.take());
    let result = vm.web_target_call(
        TargetNative::Add,
        &Value::Object(target),
        &[
            name,
            if null_callback {
                Value::Null
            } else {
                Value::Object(callback)
            },
            options,
        ],
        false,
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(calls.get(), 1);
    let state = vm.heap.web_event_target(target).unwrap();
    assert!(state.listeners.is_empty());
    assert_eq!(state.next_serial, 0);
    assert!(vm.realm_call_roots.is_none());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn event_target_registration_cancelled_capture_never_enters_later_getters() {
    registration_cancelled_conversion("{get capture(){cancel();return false;},get once(){peer();return false;},get passive(){peer();return false;},get signal(){peer();}}", false, false);
}

#[cfg_attr(test, test)]
fn event_target_registration_cancelled_last_getter_refuses_null_callback() {
    registration_cancelled_conversion("{get signal(){cancel();}}", false, true);
}

#[cfg_attr(test, test)]
fn event_target_registration_cancelled_name_throw_keeps_terminal_failure() {
    registration_cancelled_conversion("{toString(){cancel();throw 7;}}", true, false);
}

#[cfg_attr(test, test)]
fn event_target_registration_failed_original_cannot_borrow_healthy_root_credit() {
    let mut vm = page();
    run(&mut vm, "var target=new EventTarget(),callback=()=>{};0;");
    let target = object(&mut vm, "target");
    let callback = object(&mut vm, "callback");
    let scope = vm.host_scheduling.take().unwrap();
    let _ = scope.fail();
    let borrower = page();
    let owner = borrower.argument_list_owner().unwrap();
    let roots = RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let result = vm.with_realm_call_roots(roots, |vm| {
        vm.web_target_call(
            TargetNative::Add,
            &Value::Object(target),
            &[Value::String("x".into()), Value::Object(callback)],
            false,
        )
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm
        .heap
        .web_event_target(target)
        .unwrap()
        .listeners
        .is_empty());
    assert_eq!(vm.heap.web_event_target(target).unwrap().next_serial, 0);
    assert!(borrower.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn event_target_registration_foreign_document_cannot_publish_a_listener() {
    let mut vm = page();
    let original = observer(&vm);
    run(&mut vm, "var target=new EventTarget(),callback=()=>{};0;");
    let target = object(&mut vm, "target");
    let callback = object(&mut vm, "callback");
    let borrower = page();
    drop(vm.host_scheduling.take());
    vm.host_scheduling = borrower
        .host_scheduling
        .as_ref()
        .map(SchedulingScope::child);
    let result = vm.web_target_call(
        TargetNative::Add,
        &Value::Object(target),
        &[Value::String("x".into()), Value::Object(callback)],
        false,
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm
        .heap
        .web_event_target(target)
        .unwrap()
        .listeners
        .is_empty());
    assert!(original.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn event_target_registration_healthy_options_duplicates_and_abort_keep_order() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var target=new EventTarget(),controller=new AbortController(),order=[],calls=0;
        var callback={handleEvent(){calls++;}};
        var options={get capture(){order.push('capture');return false;},get once(){order.push('once');return false;},get passive(){order.push('passive');return false;},get signal(){order.push('signal');return controller.signal;}};
        target.addEventListener({toString(){order.push('name');return 'x';}},callback,options);
        target.addEventListener('x',callback,{once:true});
        target.dispatchEvent(new Event('x'));target.dispatchEvent(new Event('x'));
        controller.abort();target.dispatchEvent(new Event('x'));
        calls===2&&order.join()==='name,capture,once,passive,signal';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn event_target_registration_remove_detaches_abort_roots_before_normal_gc() {
    let mut vm = page();
    run(&mut vm, "var target=new EventTarget(),controller=new AbortController(),callback=()=>{};target.addEventListener('x',callback,{signal:controller.signal});0;");
    let callback = object(&mut vm, "callback");
    let controller = object(&mut vm, "controller");
    let signal = vm.heap.abort_controller(controller).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(callback));
    run(
        &mut vm,
        "target.removeEventListener('x',callback);callback=undefined;0;",
    );
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(callback));
    assert!(vm.heap.abort_signal(signal).unwrap().hooks.is_empty());
}

#[cfg_attr(test, test)]
fn event_target_registration_snapshot_growth_outlives_the_vm() {
    let mut vm = page();
    run(
        &mut vm,
        "var target=new EventTarget(),callback=()=>{};target.addEventListener('x',callback);0;",
    );
    let target = object(&mut vm, "target");
    let callback = object(&mut vm, "callback");
    let view = observer(&vm);
    let context = EventContext::capture(&vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut copied = NativeEventTarget::copied(&vm, target, context.owner.as_ref()).unwrap();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        copied.state.owned_buffer_bytes()
    );
    let name = NativeValue::copied(&Value::String("three".into()), context.owner.as_ref()).unwrap();
    let (name, storage) = name.into_retained_parts();
    let Value::String(name) = name else {
        unreachable!()
    };
    copied
        .listener(
            WebListener {
                serial: 2,
                name,
                callback,
                capture: false,
                once: false,
                passive: false,
                handler: false,
                signal: None,
            },
            storage,
        )
        .unwrap();
    let bytes = copied.state.owned_buffer_bytes();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copied);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn event_target_registration_self_signal_preserves_hook_retirement_and_handler() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var controller=new AbortController(),signal=controller.signal,calls=0;signal.addEventListener('abort',()=>{calls++;},{signal});signal.onabort=()=>{calls+=10;};controller.abort();signal.aborted&&calls===10;"), Value::Bool(true));
    let signal = object(&mut vm, "signal");
    assert!(vm.heap.abort_signal(signal).unwrap().hooks.is_empty());
    assert_eq!(vm.heap.web_event_target(signal).unwrap().listeners.len(), 1);
}

#[cfg_attr(test, test)]
fn event_target_registration_removal_releases_the_observed_timeout_root() {
    use crate::vm::browser_tasks::BrowserTask;
    let mut vm = page();
    run(&mut vm, "var target=new EventTarget(),signal=AbortSignal.timeout(100),callback=()=>{};target.addEventListener('x',callback,{signal});0;");
    let signal = object(&mut vm, "signal");
    assert!(vm.browser_tasks.iter().any(|task| matches!(task, BrowserTask::AbortTimeout { signal: id, root: Some(_), .. } if *id == signal)));
    run(&mut vm, "signal=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(signal));
    run(
        &mut vm,
        "target.removeEventListener('x',callback);callback=undefined;0;",
    );
    assert!(vm.browser_tasks.iter().all(|task| !matches!(task, BrowserTask::AbortTimeout { signal: id, root: Some(_), .. } if *id == signal)));
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(signal));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_event_target_storage_contracts() {
        event_target_storage_managed_event_keeps_all_container_capacity_paid();
        event_target_storage_managed_event_keeps_spare_primitive_storage_paid();
        event_target_storage_scoped_dispatch_name_copy_stays_paid_during_callback();
        event_target_storage_neutral_dispatch_name_keeps_the_original_account();
        event_target_storage_serial_snapshot_stays_paid_across_the_callback();
        event_target_storage_neutral_callback_completion_stays_paid_through_checkpoint();
        event_target_storage_neutral_last_success_checks_original_cancellation();
        event_target_storage_neutral_language_error_cannot_hide_original_cancellation();
        event_target_storage_failed_original_owner_cannot_borrow_a_healthy_root_account();
        event_target_storage_foreign_document_refuses_publication_without_poisoning_original();
        event_target_storage_empty_dispatch_keeps_path_capacity_managed_after_cleanup();
        event_target_storage_normal_gc_releases_a_consumed_once_callback();
        event_target_storage_healthy_page_exception_keeps_peer_and_cleanup_semantics();
        event_target_storage_default_passive_once_and_live_snapshot_order();
        event_target_storage_default_stop_cleanup_preserves_event_identity();
        event_target_storage_empty_prepared_path_is_refused_before_publication();
        event_target_storage_context_keeps_the_inherited_callers_cancellation();
        event_target_storage_owned_context_ignores_unrelated_failed_ambient_work();
        event_target_registration_scoped_name_copy_stays_paid_during_options();
        event_target_registration_neutral_name_keeps_original_credit_during_options();
        event_target_registration_cancelled_capture_never_enters_later_getters();
        event_target_registration_cancelled_last_getter_refuses_null_callback();
        event_target_registration_cancelled_name_throw_keeps_terminal_failure();
        event_target_registration_failed_original_cannot_borrow_healthy_root_credit();
        event_target_registration_foreign_document_cannot_publish_a_listener();
        event_target_registration_healthy_options_duplicates_and_abort_keep_order();
        event_target_registration_remove_detaches_abort_roots_before_normal_gc();
        event_target_registration_snapshot_growth_outlives_the_vm();
        event_target_registration_self_signal_preserves_hook_retirement_and_handler();
        event_target_registration_removal_releases_the_observed_timeout_root();
    }
}
