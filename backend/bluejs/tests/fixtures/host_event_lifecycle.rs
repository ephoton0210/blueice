// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny synthetic file/drag dispatch, original accounts and terminal release.

use super::*;
use crate::heap::{DragItem, DragItemData, DragStore, DragStoreMode};

fn page(bounded: bool, node: bool) -> (Vm, HostObjectFamily, [HostObjectKey; 2]) {
    let mut vm = Vm::default();
    if bounded {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
    }
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    vm.install_host_element_validator(family, |_| true).unwrap();
    if node {
        vm.install_host_node_event_methods(family, |key| Ok(vec![key]))
            .unwrap();
    }
    let keys = [HostObjectKey::new(7, 4, 3), HostObjectKey::new(7, 4, 2)];
    for (name, key) in [("target", keys[0]), ("parent", keys[1])] {
        vm.install_host_object_factory(name, 0, family, move |_: &[HostValue]| Ok(Some(key)))
            .unwrap();
    }
    (vm, family, keys)
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

fn store_bytes(store: &DragStore) -> usize {
    std::mem::size_of::<DragStore>()
        + host_scheduling::slots(&store.items)
        + store.drop_effect.owned_heap_payload_bytes().unwrap()
        + store.effect_allowed.owned_heap_payload_bytes().unwrap()
        + store
            .items
            .iter()
            .map(|item| {
                item.format.owned_heap_payload_bytes().unwrap()
                    + match &item.data {
                        DragItemData::Text(text) => text.owned_heap_payload_bytes().unwrap(),
                        _ => 0,
                    }
            })
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_managed_store_counts_all_retained_item_slots() {
    let mut store = DragStore::new(DragStoreMode::ReadOnly);
    store.items = Vec::with_capacity(7);
    assert_eq!(store.bytes(), store_bytes(&store));
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_managed_store_counts_spare_utf16_payloads() {
    let mut units = Vec::with_capacity(9);
    units.push(u16::from(b'x'));
    let mut store = DragStore::new(DragStoreMode::ReadOnly);
    store.items.push(DragItem {
        id: 1,
        format: JsString::from_code_units(units.clone()),
        data: DragItemData::Text(JsString::from_code_units(units)),
        wrapper: None,
    });
    assert_eq!(store.bytes(), store_bytes(&store));
}

fn foreign_entry(drag: bool) {
    let (mut vm, family, keys) = page(true, false);
    run(&mut vm, "var calls=0;target().addEventListener('input',()=>calls++);target().addEventListener('dragover',()=>calls++);0;");
    let original = observer(&vm);
    let (borrower, _, _) = page(true, false);
    drop(vm.host_scheduling.take());
    vm.host_scheduling = borrower
        .host_scheduling
        .as_ref()
        .map(SchedulingScope::child);
    let before = vm.heap.stats();
    let result = if drag {
        vm.dispatch_host_drag_event(
            family,
            &keys,
            &HostDragEventData::new(HostDragEventKind::DragOver, 1.0, 2.0),
        )
        .map(|_| ())
    } else {
        vm.dispatch_host_file_selection_event(family, &keys, HostFileSelectionEvent::Input)
    };
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    let after = vm.heap.stats();
    assert_eq!(
        before.nursery_objects + before.tenured_objects,
        after.nursery_objects + after.tenured_objects
    );
    assert!(original.check_scheduling_failure().is_ok());
    assert!(vm.active_host_click_event.is_none() && vm.active_host_drag_event.is_none());
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_foreign_file_entry_refuses_before_publication() {
    foreign_entry(false);
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_foreign_drag_entry_refuses_before_publication() {
    foreign_entry(true);
}

fn failed_original_entry(drag: bool) {
    let (mut vm, family, keys) = page(false, false);
    run(&mut vm, "var calls=0;target().addEventListener('input',()=>calls++);target().addEventListener('dragover',()=>calls++);0;");
    let (heap_owner, _, _) = page(true, false);
    let (family_owner, _, _) = page(true, false);
    let (caller, _, _) = page(true, false);
    let owner = family_owner.argument_list_owner().unwrap();
    drop(FamilyContext::capture_with_owner(&mut vm, owner.as_ref()).unwrap());
    vm.heap
        .bind_native_budget(
            heap_owner
                .host_scheduling
                .as_ref()
                .unwrap()
                .heap_native_budget(),
        )
        .unwrap();
    let _ = heap_owner.host_scheduling.as_ref().unwrap().fail();
    let owner = caller.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let result = vm.with_realm_call_roots(roots, |vm| {
        if drag {
            vm.dispatch_host_drag_event(
                family,
                &keys,
                &HostDragEventData::new(HostDragEventKind::DragOver, 1.0, 2.0),
            )
            .map(|_| ())
        } else {
            vm.dispatch_host_file_selection_event(family, &keys, HostFileSelectionEvent::Input)
        }
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(
        caller.check_scheduling_failure().is_ok()
            && family_owner.check_scheduling_failure().is_ok()
    );
    assert!(vm.host_scheduling.is_none() && vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_failed_heap_file_entry_cannot_borrow_healthy_family() {
    failed_original_entry(false);
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_failed_heap_drag_entry_cannot_borrow_healthy_family() {
    failed_original_entry(true);
}

fn terminal_drag(neutral: bool, throwing: bool) {
    let (mut vm, family, keys) = page(true, false);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, &format!("var savedEvent,savedTransfer,calls=0;target().addEventListener('dragstart',e=>{{savedEvent=e;savedTransfer=e.dataTransfer;calls++;cancel();{}}});parent().addEventListener('dragstart',()=>calls+=10);0;",if throwing{"throw 7;"}else{""}));
    let mut input = HostDragEventData::new(HostDragEventKind::DragStart, 1.0, 2.0);
    input.items.push(HostDragItem::Text {
        format: "text/plain".into(),
        data: "abc".into(),
    });
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let result = vm
        .run_host_event_turn(|vm| vm.dispatch_host_drag_event(family, &keys, &input))
        .map(|_| ());
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    let event = object(&mut vm, "savedEvent");
    let transfer = object(&mut vm, "savedTransfer");
    let event = vm.heap.drag_event(event).unwrap();
    assert_eq!(event.phase, 0);
    assert_eq!(event.current_target, None);
    assert!(
        event.path.is_empty()
            && !event.passive
            && !event.propagation_stopped
            && !event.immediate_stopped
    );
    let store = vm.heap.drag_store(transfer).unwrap();
    assert!(store.mode == DragStoreMode::Disabled && store.items.is_empty());
    assert_eq!(store.bytes(), store_bytes(store));
    assert!(
        vm.active_host_click_event.is_none()
            && vm.active_host_drag_event.is_none()
            && vm.realm_call_roots.is_none()
    );
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_scoped_drag_failure_disables_store_and_cleans_phase() {
    terminal_drag(false, false);
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_neutral_drag_failure_disables_store_and_cleans_phase() {
    terminal_drag(true, false);
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_terminal_throw_cannot_skip_drag_release() {
    terminal_drag(true, true);
}

fn terminal_file(neutral: bool) {
    let (mut vm, family, keys) = page(true, false);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm,"var savedEvent,calls=0;target().addEventListener('input',e=>{savedEvent=e;calls++;cancel();});parent().addEventListener('input',()=>calls+=10);0;");
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        vm.run_host_event_turn(|vm| vm.dispatch_host_file_selection_event(
            family,
            &keys,
            HostFileSelectionEvent::Input
        )),
        Err(RuntimeError::SchedulingLimit)
    );
    let event = object(&mut vm, "savedEvent");
    assert_eq!(vm.heap.host_selection_current_target(event).unwrap(), None);
    assert!(
        vm.active_host_click_event.is_none()
            && vm.active_host_drag_event.is_none()
            && vm.realm_call_roots.is_none()
    );
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_scoped_file_failure_restores_current_target() {
    terminal_file(false);
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_neutral_file_failure_restores_current_target() {
    terminal_file(true);
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_file_capture_and_checkpoint_order_remain_unchanged() {
    let (mut vm, family, keys) = page(true, false);
    run(&mut vm,"var order=[],saved;parent().addEventListener('input',e=>order.push('capture:'+e.isTrusted),true);target().addEventListener('input',e=>{saved=e;order.push('target:'+e.cancelable);e.preventDefault();Promise.resolve().then(()=>order.push('job'));});parent().addEventListener('input',()=>order.push('bubble'));0;");
    vm.run_host_event_turn(|vm| {
        vm.dispatch_host_file_selection_event(family, &keys, HostFileSelectionEvent::Input)
    })
    .unwrap();
    assert_eq!(run(&mut vm,"order.join()==='capture:true,target:false,job,bubble'&&saved.currentTarget===null&&!saved.defaultPrevented;"),Value::Bool(true));
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_drag_modes_exports_and_retained_disable_remain_unchanged() {
    let (mut vm, family, keys) = page(true, false);
    run(&mut vm,"var order=[],saved;target().addEventListener('dragstart',e=>{saved=e;order.push(e instanceof DragEvent&&e.isTrusted&&e.clientX===1.5&&e.getModifierState('Control'));e.dataTransfer.setData('text/plain','edited');e.dataTransfer.setDragImage(target(),4,-3);e.preventDefault();});target().addEventListener('dragover',e=>order.push(e.dataTransfer.getData('text/plain')===''&&e.dataTransfer.types.length===1));0;");
    let mut input = HostDragEventData::new(HostDragEventKind::DragStart, 1.5, 2.0);
    input.ctrl_key = true;
    input.items.push(HostDragItem::Text {
        format: "TEXT/PLAIN".into(),
        data: "abc".into(),
    });
    let outcome = vm
        .run_host_event_turn(|vm| vm.dispatch_host_drag_event(family, &keys, &input))
        .unwrap();
    assert!(outcome.default_prevented);
    assert!(
        matches!(outcome.image, Some(HostDragImage { element, x:4, y:-3 }) if element==keys[0])
    );
    assert!(
        matches!(outcome.items.as_slice(),[HostDragItem::Text{format,data}] if format=="text/plain"&&data=="edited")
    );
    let mut protected = HostDragEventData::new(HostDragEventKind::DragOver, 0.0, 0.0);
    protected.types = vec!["text/plain".into(), "TEXT/PLAIN".into()];
    vm.run_host_event_turn(|vm| vm.dispatch_host_drag_event(family, &keys, &protected))
        .unwrap();
    assert_eq!(run(&mut vm,"order.join()==='true,true'&&saved.currentTarget===null&&saved.eventPhase===0&&saved.composedPath().length===0&&saved.dataTransfer.getData('text/plain')==='';"),Value::Bool(true));
    let transfer = run(&mut vm, "saved.dataTransfer;").object_id().unwrap();
    let store = vm.heap.drag_store(transfer).unwrap();
    assert_eq!(store.bytes(), store_bytes(store));
}

#[cfg_attr(test, test)]
fn host_event_lifecycle_independent_file_survives_disabled_transfer_and_normal_gc() {
    let (mut vm, family, keys) = page(true, false);
    run(&mut vm,"var keptFile,keptTransfer;target().addEventListener('drop',e=>{keptTransfer=e.dataTransfer;keptFile=e.dataTransfer.files[0];});0;");
    let mut input = HostDragEventData::new(HostDragEventKind::Drop, 0.0, 0.0);
    input.items.push(HostDragItem::File(HostFileData {
        name: "one.txt".into(),
        media_type: "text/plain".into(),
        last_modified: 0,
        bytes: vec![7],
    }));
    vm.run_host_event_turn(|vm| vm.dispatch_host_drag_event(family, &keys, &input))
        .unwrap();
    let file = object(&mut vm, "keptFile");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(file));
    assert_eq!(
        run(&mut vm, "keptFile.size===1&&keptTransfer.files.length===0;"),
        Value::Bool(true)
    );
    run(&mut vm, "keptFile=undefined;keptTransfer=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(file));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_event_lifecycle_contracts() {
        host_event_lifecycle_managed_store_counts_all_retained_item_slots();
        host_event_lifecycle_managed_store_counts_spare_utf16_payloads();
        host_event_lifecycle_foreign_file_entry_refuses_before_publication();
        host_event_lifecycle_foreign_drag_entry_refuses_before_publication();
        host_event_lifecycle_failed_heap_file_entry_cannot_borrow_healthy_family();
        host_event_lifecycle_failed_heap_drag_entry_cannot_borrow_healthy_family();
        host_event_lifecycle_scoped_drag_failure_disables_store_and_cleans_phase();
        host_event_lifecycle_neutral_drag_failure_disables_store_and_cleans_phase();
        host_event_lifecycle_terminal_throw_cannot_skip_drag_release();
        host_event_lifecycle_scoped_file_failure_restores_current_target();
        host_event_lifecycle_neutral_file_failure_restores_current_target();
        host_event_lifecycle_file_capture_and_checkpoint_order_remain_unchanged();
        host_event_lifecycle_drag_modes_exports_and_retained_disable_remain_unchanged();
        host_event_lifecycle_independent_file_survives_disabled_transfer_and_normal_gc();
    }
}
