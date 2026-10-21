// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny existing host input/output copies, lifetime and pure owner return.

use super::drag_storage::{NativeHostDragOutcome, NativeHostDragText};
use super::node_events::HostEventContext;
use super::*;
use crate::heap::DragStoreMode;
use crate::vm::web_drag_drop::storage::NativeDragFile;

fn page() -> (Vm, HostObjectFamily, HostObjectKey) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    vm.install_host_element_validator(family, |_| true).unwrap();
    let key = HostObjectKey::new(7, 4, 3);
    vm.install_host_object_factory("target", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    (vm, family, key)
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
fn file() -> HostFileData {
    HostFileData {
        name: "冰🧊.txt".into(),
        media_type: "TEXT/PLAIN".into(),
        last_modified: 17,
        bytes: vec![7],
    }
}
fn input_text(neutral: bool) {
    let (mut vm, _, _) = page();
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let context = HostEventContext::capture(&mut vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let input =
        NativeHostDragText::from_host("TEXT/PLAIN", "冰🧊", context.event.owner.as_ref()).unwrap();
    assert_eq!(input.format.value(), &Value::String("text/plain".into()));
    assert_eq!(input.data.value(), &Value::String("冰🧊".into()));
    let bytes = host_scheduling::value_payload_bytes(input.format.value())
        + host_scheduling::value_payload_bytes(input.data.value());
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_drag_storage_text_input_keeps_independent_credit_after_vm_drop() {
    input_text(false);
}
#[cfg_attr(test, test)]
fn host_drag_storage_neutral_text_input_keeps_original_credit() {
    input_text(true);
}
fn input_file(neutral: bool) {
    let (mut vm, _, _) = page();
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let context = HostEventContext::capture(&mut vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let input = NativeDragFile::from_host(&file(), context.event.owner.as_ref()).unwrap();
    assert_eq!(input.data.media_type, "text/plain");
    assert_eq!(input.data.file.as_ref().unwrap(), &("冰🧊.txt".into(), 17));
    assert_eq!(input.data.bytes, [7]);
    let bytes = input.data.bytes.capacity()
        + input.data.media_type.owned_heap_payload_bytes().unwrap()
        + input
            .data
            .file
            .as_ref()
            .unwrap()
            .0
            .owned_heap_payload_bytes()
            .unwrap();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_drag_storage_file_input_keeps_independent_credit_after_vm_drop() {
    input_file(false);
}
#[cfg_attr(test, test)]
fn host_drag_storage_neutral_file_input_keeps_original_credit() {
    input_file(true);
}
fn source(vm: &mut Vm) -> ObjectId {
    run(vm,"var source=new DataTransfer();source.items.add('冰🧊','TEXT/PLAIN');source.items.add(new File(['x'],'冰.txt',{type:'application/x-ice'}));source.dropEffect='copy';source.effectAllowed='all';source.setDragImage(target(),4,-3);0;");
    object(vm, "source")
}
fn outcome_bytes(outcome: &HostDragEventOutcome) -> usize {
    outcome.drop_effect.capacity()
        + outcome.effect_allowed.capacity()
        + outcome.items.capacity() * std::mem::size_of::<HostDragItem>()
        + outcome
            .items
            .iter()
            .map(|item| match item {
                HostDragItem::Text { format, data } => format.capacity() + data.capacity(),
                HostDragItem::File(file) => {
                    file.name.capacity() + file.media_type.capacity() + file.bytes.capacity()
                }
            })
            .sum::<usize>()
}
fn output(neutral: bool) {
    let (mut vm, family, key) = page();
    let id = source(&mut vm);
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let context = HostEventContext::capture(&mut vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let output = NativeHostDragOutcome::from_store(
        &vm,
        family,
        key,
        id,
        HostDragEventKind::DragStart,
        true,
        context.event.owner.as_ref(),
    )
    .unwrap();
    assert!(output.outcome.default_prevented);
    assert_eq!(
        output.outcome.image,
        Some(HostDragImage {
            element: key,
            x: 4,
            y: -3
        })
    );
    assert_eq!(output.outcome.items.len(), 2);
    let bytes = outcome_bytes(&output.outcome);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(output);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_drag_storage_output_batch_keeps_all_native_copies_after_vm_drop() {
    output(false);
}
#[cfg_attr(test, test)]
fn host_drag_storage_neutral_output_batch_keeps_original_credit() {
    output(true);
}
#[cfg_attr(test, test)]
fn host_drag_storage_failed_original_text_input_refuses_before_copy() {
    let (mut vm, _, _) = page();
    let context = HostEventContext::capture(&mut vm).unwrap();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        NativeHostDragText::from_host("text/plain", "abc", context.event.owner.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    ));
}
#[cfg_attr(test, test)]
fn host_drag_storage_failed_original_file_input_refuses_before_copy() {
    let (mut vm, _, _) = page();
    let context = HostEventContext::capture(&mut vm).unwrap();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        NativeDragFile::from_host(&file(), context.event.owner.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    ));
}
#[cfg_attr(test, test)]
fn host_drag_storage_failed_original_output_refuses_before_copy() {
    let (mut vm, family, key) = page();
    let id = source(&mut vm);
    let context = HostEventContext::capture(&mut vm).unwrap();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        NativeHostDragOutcome::from_store(
            &vm,
            family,
            key,
            id,
            HostDragEventKind::DragStart,
            false,
            context.event.owner.as_ref()
        ),
        Err(RuntimeError::SchedulingLimit)
    ));
}
#[cfg_attr(test, test)]
fn host_drag_storage_canceled_prepared_output_cannot_cross_owner_return() {
    let (mut vm, family, key) = page();
    let id = source(&mut vm);
    let view = observer(&vm);
    let context = HostEventContext::capture(&mut vm).unwrap();
    let output = NativeHostDragOutcome::from_store(
        &vm,
        family,
        key,
        id,
        HostDragEventKind::DragStart,
        false,
        context.event.owner.as_ref(),
    )
    .unwrap();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        output.into_outcome(),
        Err(RuntimeError::SchedulingLimit)
    ));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_drag_storage_unicode_file_and_text_keep_existing_pure_return_contract() {
    let (mut vm, family, key) = page();
    let view = observer(&vm);
    run(&mut vm,"target().addEventListener('dragstart',e=>{e.preventDefault();e.dataTransfer.setData('x','冰🧊');e.dataTransfer.setDragImage(target(),4,-3);});0;");
    let mut input = HostDragEventData::new(HostDragEventKind::DragStart, 0.0, 0.0);
    input.items.push(HostDragItem::File(file()));
    let output = vm
        .run_host_event_turn(|vm| vm.dispatch_host_drag_event(family, &[key], &input))
        .unwrap();
    assert!(output.default_prevented);
    assert_eq!(
        output.image,
        Some(HostDragImage {
            element: key,
            x: 4,
            y: -3
        })
    );
    assert_eq!(output.items.len(), 2);
    assert!(
        matches!(&output.items[0],HostDragItem::File(file) if file.name=="冰🧊.txt" && file.media_type=="text/plain" && file.bytes==[7] && file.last_modified==17)
    );
    assert!(
        matches!(&output.items[1],HostDragItem::Text {format,data} if format=="x" && data=="冰🧊")
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(output.items.len(), 2);
}
#[cfg_attr(test, test)]
fn host_drag_storage_invalid_utf16_export_keeps_terminal_cleanup() {
    let (mut vm, family, key) = page();
    let view = observer(&vm);
    run(&mut vm,"var keptEvent,keptTransfer;target().addEventListener('dragstart',e=>{keptEvent=e;keptTransfer=e.dataTransfer;e.dataTransfer.setData('text/plain','\\ud800');});0;");
    let input = HostDragEventData::new(HostDragEventKind::DragStart, 0.0, 0.0);
    let result = vm.run_host_event_turn(|vm| vm.dispatch_host_drag_event(family, &[key], &input));
    assert!(
        matches!(result,Err(RuntimeError::TypeError(ref text)) if text=="Non-UTF-8 native drag text")
    );
    let event = object(&mut vm, "keptEvent");
    let transfer = object(&mut vm, "keptTransfer");
    let state = vm.heap.drag_event(event).unwrap();
    assert_eq!(state.phase, 0);
    assert!(state.path.is_empty() && state.current_target.is_none());
    let store = vm.heap.drag_store(transfer).unwrap();
    assert!(
        store.mode == DragStoreMode::Disabled
            && store.items.is_empty()
            && store.types.is_none()
            && store.image.is_none()
    );
    assert!(vm.realm_call_roots.is_none());
    vm.collect_realm_garbage().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_host_drag_storage_contracts() {
        host_drag_storage_text_input_keeps_independent_credit_after_vm_drop();
        host_drag_storage_neutral_text_input_keeps_original_credit();
        host_drag_storage_file_input_keeps_independent_credit_after_vm_drop();
        host_drag_storage_neutral_file_input_keeps_original_credit();
        host_drag_storage_output_batch_keeps_all_native_copies_after_vm_drop();
        host_drag_storage_neutral_output_batch_keeps_original_credit();
        host_drag_storage_failed_original_text_input_refuses_before_copy();
        host_drag_storage_failed_original_file_input_refuses_before_copy();
        host_drag_storage_failed_original_output_refuses_before_copy();
        host_drag_storage_canceled_prepared_output_cannot_cross_owner_return();
        host_drag_storage_unicode_file_and_text_keep_existing_pure_return_contract();
        host_drag_storage_invalid_utf16_export_keeps_terminal_cleanup();
    }
}
