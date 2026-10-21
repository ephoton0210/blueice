// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny retained listener, cancellation and dispatch-root contracts.

use super::*;

fn setup(bounded: bool, nodes: bool) -> (Vm, HostObjectFamily, HostObjectKey) {
    let mut vm = Vm::default();
    if bounded {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
    }
    let family = vm.create_host_object_family().unwrap();
    if nodes {
        vm.install_host_node_event_methods(family, |key| Ok(vec![key]))
            .unwrap();
    } else {
        vm.install_host_click_event_methods(family).unwrap();
    }
    let key = HostObjectKey::new(7, 4, 1);
    vm.install_host_object_factory("target", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    (vm, family, key)
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn retained_bytes(vm: &Vm) -> usize {
    vm.host_click_listeners.capacity() * std::mem::size_of::<HostClickListener>()
        + vm.host_click_listeners
            .iter()
            .map(|listener| listener.event_type.owned_heap_payload_bytes().unwrap())
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn host_listener_storage_retained_slots_and_names_outlive_the_vm() {
    let (mut vm, _, _) = setup(true, true);
    run(&mut vm, "target().addEventListener('event',()=>0);0;");
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    assert!(bytes > 0);
    let listeners = std::mem::take(&mut vm.host_click_listeners);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(listeners);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_listener_storage_neutral_table_adopts_all_backing_and_names() {
    let (mut vm, _, _) = setup(false, true);
    run(&mut vm, "target().addEventListener('event',()=>0);0;");
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    let listeners = std::mem::take(&mut vm.host_click_listeners);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(listeners);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_listener_storage_cancelled_options_do_not_publish_a_listener_or_serial() {
    let (source, _, _) = setup(true, false);
    let (mut vm, family, _) = setup(false, true);
    let scope = source.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancelOwner", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Bool(false))
    })
    .unwrap();
    let receiver = run(
        &mut vm,
        "var callback=()=>0,options={get capture(){return cancelOwner();}};target();",
    );
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    let options = vm.lookup_global_name("options").unwrap().unwrap();
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(3, owner.as_ref()).unwrap();
    for value in [&receiver, &callback, &options] {
        roots.push(&vm.heap, value.object_id().unwrap()).unwrap();
    }
    let serial = vm.next_host_listener_serial;
    let result = vm.with_realm_call_roots(roots, |vm| {
        vm.host_click_listener_call(
            family.index,
            receiver,
            &[Value::String("event".into()), callback, options],
            false,
            true,
        )
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.host_click_listeners.is_empty());
    assert_eq!(vm.next_host_listener_serial, serial);
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn host_listener_storage_terminal_name_conversion_skips_options() {
    let (source, _, _) = setup(true, false);
    let (mut vm, family, _) = setup(false, true);
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let observed = Rc::new(std::cell::Cell::new(false));
    let seen = observed.clone();
    vm.install_host_function("cancelOwner", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::String("event".into()))
    })
    .unwrap();
    vm.install_host_function("observeOptions", 0, move |_: &[HostValue]| {
        seen.set(true);
        Ok(HostValue::Bool(false))
    })
    .unwrap();
    let receiver = run(&mut vm,"var callback=()=>0,name={toString(){return cancelOwner();}},options={get capture(){return observeOptions();}};target();");
    let args = [
        vm.lookup_global_name("name").unwrap().unwrap(),
        vm.lookup_global_name("callback").unwrap().unwrap(),
        vm.lookup_global_name("options").unwrap().unwrap(),
    ];
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(4, owner.as_ref()).unwrap();
    roots.push(&vm.heap, receiver.object_id().unwrap()).unwrap();
    for value in &args {
        roots.push(&vm.heap, value.object_id().unwrap()).unwrap();
    }
    let result = vm.with_realm_call_roots(roots, |vm| {
        vm.host_click_listener_call(family.index, receiver, &args, false, true)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!observed.get());
    assert!(vm.host_click_listeners.is_empty());
}

#[cfg_attr(test, test)]
fn host_listener_storage_direct_entry_pins_callback_during_options_gc() {
    let (mut vm, family, key) = setup(true, true);
    vm.install_test262_harness().unwrap();
    let receiver = run(&mut vm,"var called=0,callback={handleEvent(){called++;}},options={get capture(){callback=null;$262.gc();return false;}};target();");
    let args = [
        Value::String("event".into()),
        vm.lookup_global_name("callback").unwrap().unwrap(),
        vm.lookup_global_name("options").unwrap().unwrap(),
    ];
    assert_eq!(
        vm.host_click_listener_call(family.index, receiver, &args, false, true),
        Ok(Value::Undefined)
    );
    vm.dispatch_native_node_event(family.index, &[key], "event", false, false)
        .unwrap();
    assert_eq!(run(&mut vm, "called;"), Value::Number(1.0));
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_listener_storage_removed_future_callback_is_collectible_in_current_callback() {
    let (mut vm, family, key) = setup(true, false);
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        r#"
        var later={handleEvent(){throw 'removed callback';}},weak=new WeakRef(later),collected=false;
        target().addEventListener('click',()=>{target().removeEventListener('click',later);later=null;$262.gc();collected=weak.deref()===undefined;});
        target().addEventListener('click',later);0;
    "#,
    );
    vm.finish_host_checkpoint().unwrap();
    vm.dispatch_host_click(family, key).unwrap();
    assert_eq!(run(&mut vm, "collected;"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn host_listener_storage_foreign_activation_keeps_the_original_table_owner() {
    let (source, _, _) = setup(true, false);
    let (mut vm, family, _) = setup(false, true);
    let receiver = run(&mut vm, "var callback=()=>0;target();");
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots.push(&vm.heap, receiver.object_id().unwrap()).unwrap();
    roots.push(&vm.heap, callback.object_id().unwrap()).unwrap();
    vm.with_realm_call_roots(roots, |vm| {
        vm.host_click_listener_call(
            family.index,
            receiver,
            &[Value::String("event".into()), callback],
            false,
            true,
        )
    })
    .unwrap();
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!vm.document_policy_ready);
    assert!(source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
}

#[cfg_attr(test, test)]
fn host_listener_storage_duplicate_remove_once_and_abort_semantics_are_preserved() {
    let (mut vm, _, _) = setup(true, true);
    vm.install_web_abort().unwrap();
    vm.install_web_dom_exception().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var calls=[],node=target(),controller=new AbortController();
        function one(){calls.push('one');}
        node.addEventListener('event',one,{once:true});node.addEventListener('event',one);
        node.addEventListener('event',()=>calls.push('aborted'),{signal:controller.signal});
        controller.abort();node.dispatchEvent(new Event('event'));node.dispatchEvent(new Event('event'));
        calls.join()==='one';
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.host_click_listeners.is_empty());
}

#[cfg_attr(test, test)]
fn host_listener_storage_callback_throw_does_not_skip_the_next_listener() {
    let (mut vm, _, _) = setup(true, true);
    assert_eq!(
        run(
            &mut vm,
            r#"
        var calls=[],node=target();
        node.addEventListener('event',()=>{calls.push(1);throw 7;});
        node.addEventListener('event',()=>calls.push(2));
        node.dispatchEvent(new Event('event'));calls.join()==='1,2';
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_listener_storage_serial_snapshot_outlives_the_vm_without_name_copies() {
    let (mut vm, _, _) = setup(true, true);
    run(&mut vm, "var callback=()=>0;target().addEventListener('first',callback);target().addEventListener('second',callback);0;");
    let view = observer(&vm);
    let snapshot = vm.listener_snapshot(|_| true, false).unwrap();
    let bytes = snapshot.serials.retained_slots();
    assert_eq!(snapshot.serials.len(), 2);
    assert!(bytes > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_listener_storage_removed_record_retains_its_name_until_destroyed() {
    let (mut vm, _, _) = setup(true, true);
    run(&mut vm, "target().addEventListener('event',()=>0);0;");
    let view = observer(&vm);
    let bytes = vm.host_click_listeners[0]
        .event_type
        .owned_heap_payload_bytes()
        .unwrap();
    let removed = vm.host_click_listeners.remove(0);
    assert!(vm.host_click_listeners.is_empty());
    assert_eq!(vm.host_click_listeners.capacity(), 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(removed);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_listener_storage_prepared_replacement_is_paid_before_publication() {
    let (mut vm, _, _) = setup(true, true);
    run(&mut vm, "var callback=()=>0;for(var name of ['a','b','c','d'])target().addEventListener(name,callback);0;");
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let owner = vm.argument_list_owner().unwrap();
    let serial = vm.next_host_listener_serial;
    let prepared = vm.host_click_listeners.prepare(owner.as_ref()).unwrap();
    let bytes = prepared.retained_slots();
    assert!(bytes > 0);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    assert_eq!(vm.host_click_listeners.len(), 4);
    assert_eq!(vm.next_host_listener_serial, serial);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_listener_storage_abandoned_preparation_does_not_reassign_empty_storage() {
    let (source, _, _) = setup(true, false);
    let (mut vm, _, _) = setup(false, true);
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let owner = source.argument_list_owner().unwrap();
    let prepared = vm.host_click_listeners.prepare(owner.as_ref()).unwrap();
    let bytes = prepared.retained_slots();
    assert!(bytes > 0);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert!(!vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(source.host_scheduling.as_ref().unwrap()));
    assert!(vm.host_click_listeners.is_empty());
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_listener_storage_once_object_survives_getter_gc_then_becomes_collectible() {
    let (mut vm, family, key) = setup(true, false);
    vm.install_test262_harness().unwrap();
    run(&mut vm,"var called=0,callback={get handleEvent(){callback=null;$262.gc();return function(){called++;};}};target().addEventListener('click',callback,{once:true});0;");
    let callback = vm
        .lookup_global_name("callback")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    vm.dispatch_host_click(family, key).unwrap();
    assert_eq!(run(&mut vm, "called;"), Value::Number(1.0));
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(callback));
    assert!(vm.host_click_listeners.is_empty());
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_listener_storage_once_cancellation_survives_removing_the_entire_table() {
    let (source, _, _) = setup(true, false);
    let (mut vm, family, key) = setup(false, false);
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let observed = Rc::new(std::cell::Cell::new(false));
    let seen = observed.clone();
    vm.install_host_function("cancelOwner", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    vm.install_host_function("observeLater", 0, move |_: &[HostValue]| {
        seen.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    vm.with_realm_call_roots(roots, |vm| {
        run(vm,"function later(){observeLater();}target().addEventListener('click',()=>{target().removeEventListener('click',later);cancelOwner();},{once:true});target().addEventListener('click',later);0;");
        Ok(())
    }).unwrap();
    assert_eq!(
        vm.dispatch_host_click(family, key),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!observed.get());
    assert!(vm.host_click_listeners.is_empty());
    assert_eq!(vm.host_click_listeners.capacity(), 0);
    assert!(vm.active_host_click_event.is_none());
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_listener_storage_legacy_capture_order_and_late_registration_are_preserved() {
    let (mut vm, family, key) = setup(true, false);
    run(&mut vm,"var calls=[];target().addEventListener('click',()=>calls.push('bubble'));target().addEventListener('click',()=>{calls.push('capture');target().addEventListener('click',()=>calls.push('late'),{once:true});},{capture:true,once:true});0;");
    vm.dispatch_host_click(family, key).unwrap();
    assert_eq!(
        run(&mut vm, "calls.join();"),
        Value::String("capture,bubble".into())
    );
    vm.dispatch_host_click(family, key).unwrap();
    assert_eq!(
        run(&mut vm, "calls.join();"),
        Value::String("capture,bubble,bubble,late".into())
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_listener_storage_contracts() {
        host_listener_storage_retained_slots_and_names_outlive_the_vm();
        host_listener_storage_neutral_table_adopts_all_backing_and_names();
        host_listener_storage_cancelled_options_do_not_publish_a_listener_or_serial();
        host_listener_storage_terminal_name_conversion_skips_options();
        host_listener_storage_direct_entry_pins_callback_during_options_gc();
        host_listener_storage_removed_future_callback_is_collectible_in_current_callback();
        host_listener_storage_foreign_activation_keeps_the_original_table_owner();
        host_listener_storage_duplicate_remove_once_and_abort_semantics_are_preserved();
        host_listener_storage_callback_throw_does_not_skip_the_next_listener();
        host_listener_storage_serial_snapshot_outlives_the_vm_without_name_copies();
        host_listener_storage_removed_record_retains_its_name_until_destroyed();
        host_listener_storage_prepared_replacement_is_paid_before_publication();
        host_listener_storage_abandoned_preparation_does_not_reassign_empty_storage();
        host_listener_storage_once_object_survives_getter_gc_then_becomes_collectible();
        host_listener_storage_once_cancellation_survives_removing_the_entire_table();
        host_listener_storage_legacy_capture_order_and_late_registration_are_preserved();
    }
}
