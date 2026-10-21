// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed in-memory host-node paths, accounting and terminal event cleanup.

use super::*;
use std::cell::Cell;

fn page_with_path(
    path: impl FnMut(HostObjectKey) -> Result<Vec<HostObjectKey>, HostFunctionError> + 'static,
) -> (Vm, HostObjectFamily) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    vm.install_host_node_event_methods(family, path).unwrap();
    for (name, id) in [("target", 3), ("parent", 2), ("doc", 1), ("movedParent", 9)] {
        vm.install_host_object_factory(name, 0, family, move |_: &[HostValue]| {
            Ok(Some(HostObjectKey::new(7, 4, id)))
        })
        .unwrap();
    }
    (vm, family)
}

fn page(capacity: usize) -> (Vm, HostObjectFamily, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    let (vm, family) = page_with_path(move |key| {
        output.set(output.get() + 1);
        let mut path = Vec::with_capacity(capacity);
        path.push(key);
        Ok(path)
    });
    (vm, family, calls)
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

fn global_string(vm: &mut Vm, name: &str, text: &str) {
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.define_data(global, name, Value::String(text.into()), true, false, true)
        .unwrap();
}

fn dispatch(
    vm: &mut Vm,
    family: HostObjectFamily,
    target: ObjectId,
    event: ObjectId,
) -> Result<Value, RuntimeError> {
    vm.run_host_event_turn(|vm| {
        vm.host_node_dispatch_call(
            family.index,
            &Value::Object(target),
            &[Value::Object(event)],
            false,
        )
    })
}

fn native_sample(name: &str, capacity: usize, neutral: bool, completion: &str) -> usize {
    let (mut vm, family, _) = page(capacity);
    let view = observer(&vm);
    let baseline = Rc::new(Cell::new(0));
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
    global_string(&mut vm, "typeName", name);
    global_string(&mut vm, "returned", completion);
    run(
        &mut vm,
        if completion.is_empty() {
            "var node=target(),event=new Event(typeName);node.addEventListener(typeName,()=>observe());0;"
        } else {
            "var node=target(),event=new Event(typeName);node.addEventListener(typeName,()=>{Promise.resolve().then(()=>observe());return returned;});0;"
        },
    );
    let target = object(&mut vm, "node");
    let event = object(&mut vm, "event");
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(view.host_scheduling_snapshot().payload_bytes);
    dispatch(&mut vm, family, target, event).unwrap();
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn node_event_storage_scoped_frozen_name_stays_paid_during_callback() {
    assert_eq!(
        native_sample("abc", 1, false, "") - native_sample("a", 1, false, ""),
        4
    );
}

#[cfg_attr(test, test)]
fn node_event_storage_neutral_frozen_name_keeps_original_credit() {
    assert_eq!(
        native_sample("abc", 1, true, "") - native_sample("a", 1, true, ""),
        4
    );
}

#[cfg_attr(test, test)]
fn node_event_storage_scoped_path_spare_capacity_stays_paid_during_callback() {
    assert_eq!(
        native_sample("x", 5, false, "") - native_sample("x", 1, false, ""),
        4 * std::mem::size_of::<HostObjectKey>()
    );
}

#[cfg_attr(test, test)]
fn node_event_storage_neutral_path_spare_capacity_keeps_original_credit() {
    assert_eq!(
        native_sample("x", 5, true, "") - native_sample("x", 1, true, ""),
        4 * std::mem::size_of::<HostObjectKey>()
    );
}

#[cfg_attr(test, test)]
fn node_event_storage_returned_host_path_outlives_the_vm() {
    let (mut vm, family, _) = page(5);
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let path = vm
        .host_node_event_path(family.index, HostObjectKey::new(7, 4, 3))
        .unwrap();
    let bytes = path.capacity() * std::mem::size_of::<HostObjectKey>();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(path);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn node_event_storage_neutral_completion_stays_paid_through_checkpoint() {
    assert_eq!(
        native_sample("x", 1, true, "abc") - native_sample("x", 1, true, "a"),
        4
    );
}

fn refused_entry(foreign: bool) {
    let (mut vm, family, calls) = page(1);
    run(&mut vm, "var node=target(),event=new Event('x');0;");
    let target = object(&mut vm, "node");
    let event = object(&mut vm, "event");
    let original = observer(&vm);
    let (borrower, _, _) = page(1);
    let scope = vm.host_scheduling.take().unwrap();
    if foreign {
        vm.host_scheduling = borrower
            .host_scheduling
            .as_ref()
            .map(SchedulingScope::child);
    } else {
        let _ = scope.fail();
    }
    let result = if foreign {
        dispatch(&mut vm, family, target, event)
    } else {
        let owner = borrower.argument_list_owner().unwrap();
        let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
        vm.with_realm_call_roots(roots, |vm| dispatch(vm, family, target, event))
    };
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(calls.get(), 0);
    let state = vm.heap.drag_event(event).unwrap();
    assert_eq!(state.get("target"), Value::Null);
    assert_eq!(state.phase, 0);
    assert!(state.path.is_empty());
    if foreign {
        assert!(original.check_scheduling_failure().is_ok());
    } else {
        assert!(borrower.check_scheduling_failure().is_ok());
    }
}

#[cfg_attr(test, test)]
fn node_event_storage_failed_original_refuses_before_provider_or_publication() {
    refused_entry(false);
}

#[cfg_attr(test, test)]
fn node_event_storage_foreign_document_refuses_before_provider_or_publication() {
    refused_entry(true);
}

fn terminal_callback(neutral: bool, throwing: bool) {
    let (mut vm, family, _) = page(1);
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        output.set(output.get() + 1);
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let output = calls.clone();
    vm.install_host_function("peer", 0, move |_: &[HostValue]| {
        output.set(output.get() + 10);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, &format!("var node=target(),event=new Event('x',{{cancelable:true}});node.addEventListener('x',()=>{{cancel();{}}},{{passive:true}});node.addEventListener('x',()=>peer());0;", if throwing { "throw 7;" } else { "" }));
    let target = object(&mut vm, "node");
    let event = object(&mut vm, "event");
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        dispatch(&mut vm, family, target, event),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 1);
    let state = vm.heap.drag_event(event).unwrap();
    assert_eq!(state.phase, 0);
    assert_eq!(state.current_target, None);
    assert!(state.path.is_empty() && !state.passive);
    assert!(!state.propagation_stopped && !state.immediate_stopped);
    assert!(vm.realm_call_roots.is_none());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn node_event_storage_scoped_terminal_cleanup_needs_no_admission() {
    terminal_callback(false, false);
}

#[cfg_attr(test, test)]
fn node_event_storage_neutral_terminal_cleanup_needs_no_admission() {
    terminal_callback(true, false);
}

#[cfg_attr(test, test)]
fn node_event_storage_terminal_language_error_cannot_skip_cleanup() {
    terminal_callback(true, true);
}

#[cfg_attr(test, test)]
fn node_event_storage_default_ancestry_and_live_listener_order_are_preserved() {
    let moved = Rc::new(Cell::new(false));
    let path = moved.clone();
    let (mut vm, _) = page_with_path(move |key| {
        Ok(vec![
            key,
            HostObjectKey::new(7, 4, if path.get() { 9 } else { 2 }),
            HostObjectKey::new(7, 4, 1),
        ])
    });
    vm.install_host_function("movePath", 0, move |_: &[HostValue]| {
        moved.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var calls=[],node=target(),event=new Event('x',{bubbles:true,cancelable:true});
        doc().addEventListener('x',()=>{calls.push('doc');movePath();parent().addEventListener('x',()=>calls.push('late'),{once:true});},true);
        parent().addEventListener('x',e=>{calls.push('parent:'+e.eventPhase);e.preventDefault();},{passive:true});
        movedParent().addEventListener('x',()=>calls.push('moved'));
        node.addEventListener('x',e=>calls.push('target:'+e.eventPhase+':'+(e.composedPath()[0]===node)));
        var first=node.dispatchEvent(event),second=node.dispatchEvent(event);
        first&&second&&calls.join()==='doc,target:2:true,parent:3,late,doc,target:2:true,moved'&&event.currentTarget===null&&event.eventPhase===0&&event.composedPath().length===0&&!event.isTrusted;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn node_event_storage_native_click_and_selection_preserve_trust_and_cancellation() {
    let (mut vm, family, _) = page(1);
    run(&mut vm, "var seen=[],node=target();node.addEventListener('click',e=>{seen.push(e instanceof Event&&e.isTrusted&&e.cancelable&&e.composed);e.preventDefault();},{once:true});node.addEventListener('input',e=>seen.push(e instanceof Event&&e.isTrusted&&!e.cancelable&&e.composed));0;");
    let key = HostObjectKey::new(7, 4, 3);
    assert!(vm
        .run_host_event_turn(|vm| vm.dispatch_host_click(family, key))
        .unwrap());
    vm.run_host_event_turn(|vm| {
        vm.dispatch_host_file_selection_event(family, &[key], HostFileSelectionEvent::Input)
    })
    .unwrap();
    assert_eq!(
        run(&mut vm, "seen.join()==='true,true';"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn node_event_storage_once_callback_releases_after_normal_gc() {
    let (mut vm, family, _) = page(1);
    run(&mut vm, "var node=target(),event=new Event('x'),callback=()=>{};node.addEventListener('x',callback,{once:true});0;");
    let callback = object(&mut vm, "callback");
    let target = object(&mut vm, "node");
    let event = object(&mut vm, "event");
    run(&mut vm, "callback=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(callback));
    dispatch(&mut vm, family, target, event).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(callback));
}

#[cfg_attr(test, test)]
fn node_event_storage_cancelled_provider_error_preserves_unpublished_event() {
    let (mut vm, _, _) = page(1);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_node_event_methods(family, move |_| {
        let _ = scope.fail();
        Err(HostFunctionError::new("replaced"))
    })
    .unwrap();
    let target = vm
        .mint_host_wrapper(family.index as usize, HostObjectKey::new(7, 4, 3))
        .unwrap();
    run(&mut vm, "var event=new Event('x');0;");
    let event = object(&mut vm, "event");
    assert_eq!(
        dispatch(&mut vm, family, target, event),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.heap.drag_event(event).unwrap().get("target"),
        Value::Null
    );
    assert_eq!(vm.heap.drag_event(event).unwrap().phase, 0);
}

#[cfg_attr(test, test)]
fn node_event_storage_mixed_original_family_is_guarded_without_policy_rebinding() {
    let (event_owner, _, _) = page(1);
    let (family_owner, _, _) = page(1);
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_node_event_methods(family, |key| Ok(vec![key]))
        .unwrap();
    let owner = family_owner.argument_list_owner().unwrap();
    let family_context = FamilyContext::capture_with_owner(&mut vm, owner.as_ref()).unwrap();
    drop(family_context);
    vm.heap
        .bind_native_budget(
            event_owner
                .host_scheduling
                .as_ref()
                .unwrap()
                .heap_native_budget(),
        )
        .unwrap();
    let context = HostEventContext::capture(&mut vm).unwrap();
    let result = context.with_roots(&mut vm, std::iter::empty(), 0, |vm, _| {
        let _ = family_owner.host_scheduling.as_ref().unwrap().fail();
        vm.argument_list_owner().map(|_| ())
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(event_owner.check_scheduling_failure().is_ok());
    assert!(vm.host_scheduling.is_none() && vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn node_event_storage_shared_cleanup_keeps_legacy_click_and_selection_order() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    let key = HostObjectKey::new(7, 4, 3);
    vm.install_host_object_factory("node", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    run(&mut vm, "var order=[],saved,targetNode=node();targetNode.addEventListener('click',e=>{saved=e;order.push('click');Promise.resolve().then(()=>order.push('job'));e.preventDefault();return 'abc';},{once:true});targetNode.addEventListener('click',()=>order.push('peer'));targetNode.addEventListener('input',e=>order.push('input:'+e.isTrusted+':'+e.cancelable));0;");
    assert!(vm
        .run_host_event_turn(|vm| vm.dispatch_host_click(family, key))
        .unwrap());
    vm.run_host_event_turn(|vm| {
        vm.dispatch_host_file_selection_event(family, &[key], HostFileSelectionEvent::Input)
    })
    .unwrap();
    assert_eq!(
        run(&mut vm, "order.join();"),
        Value::String("click,job,peer,input:true:false".into())
    );
    // The legacy click object keeps its original readonly wrapper property.
    assert_eq!(
        run(&mut vm, "saved.currentTarget===targetNode;"),
        Value::Bool(true)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_node_event_storage_contracts() {
        node_event_storage_scoped_frozen_name_stays_paid_during_callback();
        node_event_storage_neutral_frozen_name_keeps_original_credit();
        node_event_storage_scoped_path_spare_capacity_stays_paid_during_callback();
        node_event_storage_neutral_path_spare_capacity_keeps_original_credit();
        node_event_storage_returned_host_path_outlives_the_vm();
        node_event_storage_neutral_completion_stays_paid_through_checkpoint();
        node_event_storage_failed_original_refuses_before_provider_or_publication();
        node_event_storage_foreign_document_refuses_before_provider_or_publication();
        node_event_storage_scoped_terminal_cleanup_needs_no_admission();
        node_event_storage_neutral_terminal_cleanup_needs_no_admission();
        node_event_storage_terminal_language_error_cannot_skip_cleanup();
        node_event_storage_default_ancestry_and_live_listener_order_are_preserved();
        node_event_storage_native_click_and_selection_preserve_trust_and_cancellation();
        node_event_storage_once_callback_releases_after_normal_gc();
        node_event_storage_cancelled_provider_error_preserves_unpublished_event();
        node_event_storage_mixed_original_family_is_guarded_without_policy_rebinding();
        node_event_storage_shared_cleanup_keeps_legacy_click_and_selection_order();
    }
}
