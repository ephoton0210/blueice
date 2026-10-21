// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny DataTransfer conversion ownership and original-account boundaries.

use super::*;
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
fn conversion_sample(items: bool, neutral: bool, text: &str) -> usize {
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
    run(&mut vm, "var transfer=new DataTransfer();var list=transfer.items;var converted={toString(){observe();return 'x';}};0;");
    let id = object(&mut vm, if items { "list" } else { "transfer" });
    let converted = object(&mut vm, "converted");
    let view = observer(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(view.host_scheduling_snapshot().payload_bytes);
    vm.web_drag_call(
        if items {
            DragNative::ItemsAdd
        } else {
            DragNative::SetData
        },
        &Value::Object(id),
        &[Value::String(text.into()), Value::Object(converted)],
        false,
    )
    .unwrap();
    sample.get().unwrap()
}
fn retained_conversion(items: bool, neutral: bool) {
    let short = conversion_sample(items, neutral, "abc");
    let long = conversion_sample(items, neutral, "abcdefgh");
    assert_eq!(
        long - short,
        10,
        "first converted primitive remains owned through the next guest conversion"
    );
}
#[cfg_attr(test, test)]
fn drag_store_storage_set_data_retains_format_through_data_conversion() {
    retained_conversion(false, false);
}
#[cfg_attr(test, test)]
fn drag_store_storage_neutral_set_data_retains_original_format_credit() {
    retained_conversion(false, true);
}
#[cfg_attr(test, test)]
fn drag_store_storage_items_add_retains_text_through_type_conversion() {
    retained_conversion(true, false);
}
#[cfg_attr(test, test)]
fn drag_store_storage_neutral_items_add_retains_original_text_credit() {
    retained_conversion(true, true);
}

fn foreign_entry(mutate: bool) {
    let mut vm = page();
    run(&mut vm, "var transfer=new DataTransfer();transfer.setData('text/plain','old');var calls=0;var converted={toString(){calls++;return 'text/plain';}};0;");
    let transfer = object(&mut vm, "transfer");
    let converted = object(&mut vm, "converted");
    let original = observer(&vm);
    let borrower = page();
    drop(vm.host_scheduling.take());
    vm.host_scheduling = borrower
        .host_scheduling
        .as_ref()
        .map(SchedulingScope::child);
    let args = [Value::Object(converted), Value::String("new".into())];
    let result = vm.web_drag_call(
        if mutate {
            DragNative::SetData
        } else {
            DragNative::DropEffect
        },
        &Value::Object(transfer),
        if mutate { &args } else { &[] },
        false,
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(original.check_scheduling_failure().is_ok());
    assert!(
        matches!(&vm.heap.drag_store(transfer).unwrap().items[0].data,DragItemData::Text(text) if text=="old")
    );
}
#[cfg_attr(test, test)]
fn drag_store_storage_foreign_getter_refuses_original_heap() {
    foreign_entry(false);
}
#[cfg_attr(test, test)]
fn drag_store_storage_foreign_mutation_refuses_before_conversion() {
    foreign_entry(true);
}
#[cfg_attr(test, test)]
fn drag_store_storage_neutral_getter_cannot_borrow_failed_original_heap() {
    let mut vm = page();
    run(&mut vm, "var transfer=new DataTransfer();0;");
    let transfer = object(&mut vm, "transfer");
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    drop(vm.host_scheduling.take());
    assert_eq!(
        vm.web_drag_call(DragNative::DropEffect, &Value::Object(transfer), &[], false),
        Err(RuntimeError::SchedulingLimit)
    );
}
fn canceled_conversion(neutral: bool) {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm,"var transfer=new DataTransfer();transfer.setData('text/plain','old');var calls=0;var first={toString(){cancel();return 'text/plain';}};var second={toString(){calls++;return 'new';}};0;");
    let transfer = object(&mut vm, "transfer");
    let first = object(&mut vm, "first");
    let second = object(&mut vm, "second");
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let result = vm.web_drag_call(
        DragNative::SetData,
        &Value::Object(transfer),
        &[Value::Object(first), Value::Object(second)],
        false,
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(
        matches!(&vm.heap.drag_store(transfer).unwrap().items[0].data,DragItemData::Text(text) if text=="old")
    );
}
#[cfg_attr(test, test)]
fn drag_store_storage_canceling_format_conversion_stops_before_data() {
    canceled_conversion(false);
}
#[cfg_attr(test, test)]
fn drag_store_storage_neutral_canceling_conversion_keeps_original_data() {
    canceled_conversion(true);
}
#[cfg_attr(test, test)]
fn drag_store_storage_reentrant_conversion_uses_live_store_and_append_order() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"var d=new DataTransfer();d.setData('text/plain','old');d.setData('x','keep');var old=d.items[0];d.setData('text',{toString(){d.setData('y','nested');return 'new';}});[d.types.join(','),d.getData('text'),old.kind,d.items.length].join('|');"),Value::String("x,y,text/plain|new||3".into()));
}
#[cfg_attr(test, test)]
fn drag_store_storage_types_cache_removal_and_duplicate_contracts_remain() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"var d=new DataTransfer();d.items.add('one','X');var types=d.types;var first=d.items[0];var duplicate='';try{d.items.add('two','x');}catch(e){duplicate=e.name;}d.items.add('three','y');var second=d.types;d.items.remove(0);[types.join(','),second.join(','),d.types.join(','),first.kind,duplicate,Object.isFrozen(d.types)].join('|');"),Value::String("x|x,y|y||NotSupportedError|true".into()));
}
#[cfg_attr(test, test)]
fn drag_store_storage_alias_url_and_utf16_contracts_remain() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"var d=new DataTransfer();d.setData('URL','#comment\\r\\nhttps://example.invalid/a\\r\\nsecond');d.setData('TEXT','\\ud800X');[d.getData('url'),d.types.join(','),d.getData('text').charCodeAt(0),d.getData('TEXT').length].join('|');"),Value::String("https://example.invalid/a|text/uri-list,text/plain|55296|2".into()));
}

#[cfg_attr(test, test)]
fn drag_store_storage_native_store_copy_keeps_credit_after_vm_drop() {
    let mut vm = page();
    run(
        &mut vm,
        "var d=new DataTransfer();d.setData('text/plain','abc');d.setData('x','def');0;",
    );
    let id = object(&mut vm, "d");
    let view = observer(&vm);
    let context = EventContext::capture(&vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let store = NativeDragStore::copied(&vm, id, context.owner.as_ref()).unwrap();
    let bytes = store.state.owned_buffer_bytes();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(store);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn drag_store_storage_native_file_copy_keeps_credit_after_vm_drop() {
    let mut vm = page();
    run(
        &mut vm,
        "var f=new File(['a'],'one.txt',{type:'text/plain'});0;",
    );
    let file = object(&mut vm, "f");
    let view = observer(&vm);
    let context = EventContext::capture(&vm).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let file = NativeDragFile::copied(&vm, file, context.owner.as_ref()).unwrap();
    let bytes = file.data.owned_buffer_bytes();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - before,
        bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(file);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn drag_store_storage_string_task_keeps_snapshot_after_clear_and_normal_gc() {
    let mut vm = page();
    run(&mut vm,"var output='';var d=new DataTransfer();d.items.add('one','x').getAsString(s=>output+=s);d.clearData();0;");
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 1);
    vm.collect_realm_garbage().unwrap();
    vm.run_browser_tasks_bounded(1).unwrap();
    assert_eq!(
        vm.lookup_global_name("output").unwrap(),
        Some(Value::String("one".into()))
    );
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 0);
}
fn managed_blob_sample(capacity: usize) -> usize {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let before = heap.stats().managed_bytes;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.push(7);
    let mut kind = Vec::with_capacity(capacity);
    kind.push(u16::from(b'x'));
    let mut name = Vec::with_capacity(capacity);
    name.push(u16::from(b'a'));
    heap.alloc_web_blob(
        crate::heap::WebBlobData {
            bytes,
            media_type: JsString::from_code_units(kind),
            file: Some((JsString::from_code_units(name), 0)),
        },
        prototype,
    )
    .unwrap();
    heap.stats().managed_bytes - before
}
#[cfg_attr(test, test)]
fn drag_store_storage_managed_file_counts_retained_spare_buffers() {
    assert_eq!(managed_blob_sample(7) - managed_blob_sample(1), 30);
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_drag_store_storage_contracts() {
        drag_store_storage_native_store_copy_keeps_credit_after_vm_drop();
        drag_store_storage_native_file_copy_keeps_credit_after_vm_drop();
        drag_store_storage_string_task_keeps_snapshot_after_clear_and_normal_gc();
        drag_store_storage_managed_file_counts_retained_spare_buffers();
        drag_store_storage_set_data_retains_format_through_data_conversion();
        drag_store_storage_neutral_set_data_retains_original_format_credit();
        drag_store_storage_items_add_retains_text_through_type_conversion();
        drag_store_storage_neutral_items_add_retains_original_text_credit();
        drag_store_storage_foreign_getter_refuses_original_heap();
        drag_store_storage_foreign_mutation_refuses_before_conversion();
        drag_store_storage_neutral_getter_cannot_borrow_failed_original_heap();
        drag_store_storage_canceling_format_conversion_stops_before_data();
        drag_store_storage_neutral_canceling_conversion_keeps_original_data();
        drag_store_storage_reentrant_conversion_uses_live_store_and_append_order();
        drag_store_storage_types_cache_removal_and_duplicate_contracts_remain();
        drag_store_storage_alias_url_and_utf16_contracts_remain();
    }
}
