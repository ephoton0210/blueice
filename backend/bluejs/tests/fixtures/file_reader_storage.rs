// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny existing FileReader state, producer ownership and canceled epochs.

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
fn view(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn fixture() -> (Vm, ObjectId, ObjectId) {
    let mut vm = page();
    run(
        &mut vm,
        "var reader=new FileReader(),blob=new Blob(['one']);0;",
    );
    let reader = object(&mut vm, "reader");
    let blob = object(&mut vm, "blob");
    (vm, reader, blob)
}
fn set_failed(vm: &mut Vm, foreign: bool) {
    if foreign {
        let peer = page();
        drop(vm.host_scheduling.take());
        vm.host_scheduling = peer.host_scheduling.as_ref().map(SchedulingScope::child);
    } else {
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
        drop(vm.host_scheduling.take());
    }
}
fn binary_sample(text: &str, neutral: bool) -> usize {
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
    run(&mut vm,&format!("var blob=new Blob(['{text}']);var original=ArrayBuffer;var wrapped=new Proxy(function(){{}},{{get(t,k){{if(k==='prototype'){{observe();return original.prototype;}}return Reflect.get(t,k);}}}});0;"));
    let blob = object(&mut vm, "blob");
    let wrapped = object(&mut vm, "wrapped");
    vm.globals.insert("ArrayBuffer".into(), wrapped);
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(observer.host_scheduling_snapshot().payload_bytes);
    vm.package_file_read(blob, FileReadKind::ArrayBuffer, None)
        .unwrap();
    assert!(vm.realm_call_roots.is_none());
    sample.get().unwrap()
}
#[cfg_attr(test, test)]
fn file_reader_storage_binary_copy_stays_paid_through_prototype() {
    assert_eq!(
        binary_sample("abcdefgh", false) - binary_sample("abc", false),
        5
    );
}
#[cfg_attr(test, test)]
fn file_reader_storage_neutral_binary_copy_keeps_original_credit() {
    assert_eq!(
        binary_sample("abcdefgh", true) - binary_sample("abc", true),
        5
    );
}
fn refused_members(foreign: bool) {
    let (mut vm, reader, blob) = fixture();
    set_failed(&mut vm, foreign);
    for native in [
        ReaderNative::Ready,
        ReaderNative::Result,
        ReaderNative::Error,
        ReaderNative::Handler("load", false),
        ReaderNative::Handler("load", true),
        ReaderNative::Abort,
        ReaderNative::Read(FileReadKind::Text),
    ] {
        assert!(
            matches!(
                vm.web_reader_call(
                    native,
                    &Value::Object(reader),
                    &[Value::Object(blob)],
                    false
                ),
                Err(RuntimeError::SchedulingLimit)
            ),
            "{native:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn file_reader_storage_foreign_entry_refuses_existing_members() {
    refused_members(true);
}
#[cfg_attr(test, test)]
fn file_reader_storage_neutral_failed_heap_refuses_existing_members() {
    refused_members(false);
}
fn refused_constructor(foreign: bool) {
    let mut vm = page();
    run(&mut vm,"var calls=0;var wrapped=new Proxy(function(){},{get(t,k){if(k==='prototype'){calls++;return FileReader.prototype;}return Reflect.get(t,k);}});0;");
    let target = object(&mut vm, "wrapped");
    vm.new_target = Value::Object(target);
    set_failed(&mut vm, foreign);
    assert!(matches!(
        vm.web_reader_call(ReaderNative::Constructor, &Value::Undefined, &[], true),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_reader_storage_foreign_constructor_stops_before_prototype() {
    refused_constructor(true);
}
#[cfg_attr(test, test)]
fn file_reader_storage_neutral_failed_constructor_stops_before_prototype() {
    refused_constructor(false);
}
fn refused_task(foreign: bool) {
    let (mut vm, reader, _) = fixture();
    run(&mut vm, "reader.readAsText(blob);0;");
    let epoch = vm.heap.file_reader(reader).unwrap().epoch;
    set_failed(&mut vm, foreign);
    assert!(matches!(
        vm.file_read_task(reader, epoch, 1),
        Err(RuntimeError::SchedulingLimit)
    ));
    let state = vm.heap.file_reader(reader).unwrap();
    assert_eq!((state.epoch, state.loaded, state.ready), (epoch, 0, 1));
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_reader_storage_foreign_task_refuses_before_state_update() {
    refused_task(true);
}
#[cfg_attr(test, test)]
fn file_reader_storage_neutral_failed_task_refuses_before_state_update() {
    refused_task(false);
}
fn spare(capacity: usize) -> JsString {
    let mut units = Vec::with_capacity(capacity);
    units.push(65);
    JsString::from_code_units(units)
}
#[cfg_attr(test, test)]
fn file_reader_storage_managed_encoding_counts_retained_capacity() {
    let small = crate::heap::FileReaderState {
        encoding: Some(spare(1)),
        ..Default::default()
    };
    let large = crate::heap::FileReaderState {
        encoding: Some(spare(16)),
        ..Default::default()
    };
    assert_eq!(large.bytes() - small.bytes(), 30);
}
#[cfg_attr(test, test)]
fn file_reader_storage_managed_result_counts_retained_capacity() {
    let small = crate::heap::FileReaderState {
        result: Value::String(spare(1)),
        ..Default::default()
    };
    let large = crate::heap::FileReaderState {
        result: Value::String(spare(16)),
        ..Default::default()
    };
    assert_eq!(large.bytes() - small.bytes(), 30);
}
#[cfg_attr(test, test)]
fn file_reader_storage_managed_header_counts_inline_target_once() {
    assert_eq!(
        crate::heap::FileReaderState::default().bytes(),
        std::mem::size_of::<crate::heap::FileReaderState>()
    );
}
fn canceled_encoding(neutral: bool) {
    let (mut vm, reader, blob) = fixture();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        "var label={toString(){cancel();return 'utf-8';}};0;",
    );
    let label = object(&mut vm, "label");
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert!(matches!(
        vm.web_reader_call(
            ReaderNative::Read(FileReadKind::Text),
            &Value::Object(reader),
            &[Value::Object(blob), Value::Object(label)],
            false
        ),
        Err(RuntimeError::SchedulingLimit)
    ));
    let state = vm.heap.file_reader(reader).unwrap();
    assert_eq!((state.ready, state.epoch), (0, 0));
    assert!(vm.browser_tasks.is_empty() && vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_reader_storage_canceled_encoding_preserves_state_and_queue() {
    canceled_encoding(false);
}
#[cfg_attr(test, test)]
fn file_reader_storage_neutral_canceled_encoding_preserves_state_and_queue() {
    canceled_encoding(true);
}
#[cfg_attr(test, test)]
fn file_reader_storage_encoding_throw_keeps_initial_state() {
    let (mut vm, reader, blob) = fixture();
    run(&mut vm, "var label={toString(){throw 7;}};0;");
    let label = object(&mut vm, "label");
    assert!(matches!(
        vm.web_reader_call(
            ReaderNative::Read(FileReadKind::Text),
            &Value::Object(reader),
            &[Value::Object(blob), Value::Object(label)],
            false
        ),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    ));
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 0);
    assert!(vm.browser_tasks.is_empty() && vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_reader_storage_pending_root_survives_then_releases_under_normal_gc() {
    let (mut vm, reader, blob) = fixture();
    run(&mut vm,"var output='';reader.onload=event=>output=event.target.result;reader.readAsText(blob);reader=undefined;blob=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(reader) && vm.heap.contains(blob));
    vm.run_browser_tasks_bounded(3).unwrap();
    assert_eq!(
        vm.lookup_global_name("output").unwrap(),
        Some(Value::String("one".into()))
    );
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(reader) && !vm.heap.contains(blob));
}
#[cfg_attr(test, test)]
fn file_reader_storage_packaging_throw_preserves_exception_and_root_prefix() {
    for neutral in [false, true] {
        let mut vm = page();
        run(&mut vm,"var blob=new Blob(['one']);var wrapped=new Proxy(function(){},{get(t,k){if(k==='prototype')throw 7;return Reflect.get(t,k);}});0;");
        let blob = object(&mut vm, "blob");
        let wrapped = object(&mut vm, "wrapped");
        vm.globals.insert("ArrayBuffer".into(), wrapped);
        if neutral {
            drop(vm.host_scheduling.take());
        }
        let base = vm.stack.len();
        assert!(matches!(
            vm.package_file_read(blob, FileReadKind::ArrayBuffer, None),
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        ));
        assert_eq!(vm.stack.len(), base);
        assert!(vm.realm_call_roots.is_none());
    }
}
fn reentrant_packaging(restart: bool) {
    let (mut vm, reader, _) = fixture();
    run(&mut vm,"var loads=0;reader.onload=()=>loads++;reader.readAsArrayBuffer(blob);var original=ArrayBuffer;var once=false;0;");
    let epoch = vm.heap.file_reader(reader).unwrap().epoch;
    run(
        &mut vm,
        if restart {
            "var wrapped=new Proxy(function(){},{get(t,k){if(k==='prototype'){if(!once){once=true;reader.abort();reader.readAsText(new Blob(['two']));}return original.prototype;}return Reflect.get(t,k);}});0;"
        } else {
            "var wrapped=new Proxy(function(){},{get(t,k){if(k==='prototype'){if(!once){once=true;reader.abort();}return original.prototype;}return Reflect.get(t,k);}});0;"
        },
    );
    let wrapped = object(&mut vm, "wrapped");
    vm.globals.insert("ArrayBuffer".into(), wrapped);
    assert_eq!(vm.file_read_task(reader, epoch, 2).unwrap(), None);
    assert_eq!(
        vm.lookup_global_name("loads").unwrap(),
        Some(Value::Number(0.0))
    );
    let state = vm.heap.file_reader(reader).unwrap();
    assert_ne!(state.epoch, epoch);
    if restart {
        assert_eq!(state.ready, 1);
        assert_eq!(vm.browser_tasks.len(), 1);
        vm.run_browser_tasks_bounded(3).unwrap();
        assert_eq!(
            vm.heap.file_reader(reader).unwrap().result,
            Value::String("two".into())
        );
    } else {
        assert_eq!(state.ready, 2);
        assert_eq!(state.result, Value::Null);
        assert!(vm.browser_tasks.is_empty());
    }
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_reader_storage_abort_during_packaging_keeps_abort_state() {
    reentrant_packaging(false);
}
#[cfg_attr(test, test)]
fn file_reader_storage_restart_during_packaging_preserves_new_read() {
    reentrant_packaging(true);
}

fn retained_lifetime(neutral: bool) {
    let (mut vm, reader, blob) = fixture();
    run(
        &mut vm,
        "reader.onload=function(){};reader.readAsText(blob,'utf-8');0;",
    );
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let context = EventContext::capture(&vm).unwrap();
    let state = NativeReaderState::copied(&vm, reader, context.owner.as_ref()).unwrap();
    let state_bytes = state.state.bytes();
    let state = state.prepare(&mut vm, reader).unwrap();
    let metadata = packaging::NativeUtf8::copied(&JsString::from("utf-8"), context.owner.as_ref())
        .unwrap()
        .unwrap();
    let binary = vm
        .package_file_read(blob, FileReadKind::BinaryString, None)
        .unwrap();
    let text = vm
        .package_file_read(blob, FileReadKind::Text, None)
        .unwrap();
    let url = vm
        .package_file_read(blob, FileReadKind::DataUrl, None)
        .unwrap();
    assert_eq!(binary.value(), &Value::String("one".into()));
    assert_eq!(text.value(), &Value::String("one".into()));
    assert_eq!(
        url.value(),
        &Value::String("data:application/octet-stream;base64,b25l".into())
    );
    let result_bytes = host_scheduling::value_payload_bytes(binary.value())
        + host_scheduling::value_payload_bytes(text.value())
        + host_scheduling::value_payload_bytes(url.value());
    let metadata_bytes = metadata.data.capacity();
    drop(vm);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        state_bytes + metadata_bytes + result_bytes
    );
    drop(state);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        metadata_bytes + result_bytes
    );
    drop(metadata);
    drop(binary);
    drop(text);
    drop(url);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn file_reader_storage_prepared_state_metadata_and_results_outlive_vm_with_credit() {
    retained_lifetime(false);
}
#[cfg_attr(test, test)]
fn file_reader_storage_neutral_prepared_state_and_results_keep_original_credit() {
    retained_lifetime(true);
}
#[cfg_attr(test, test)]
fn file_reader_storage_ordinary_packaging_error_can_start_a_new_read() {
    let (mut vm, reader, _) = fixture();
    run(&mut vm,"var events='';reader.onerror=()=>{events+='error,';reader.readAsText(new Blob(['retry']));};reader.onload=()=>events+=reader.result+',';reader.onloadend=()=>events+='end';reader.readAsArrayBuffer(blob);var wrapped=new Proxy(function(){},{get(t,k){if(k==='prototype')throw 7;return Reflect.get(t,k);}});0;");
    let wrapped = object(&mut vm, "wrapped");
    vm.globals.insert("ArrayBuffer".into(), wrapped);
    vm.run_browser_tasks_bounded(6).unwrap();
    assert_eq!(
        vm.lookup_global_name("events").unwrap(),
        Some(Value::String("error,retry,end".into()))
    );
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 2);
    assert!(vm.heap.file_reader(reader).unwrap().error.is_none());
    assert!(vm.browser_tasks.is_empty() && vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_reader_storage_binary_result_traces_between_normal_task_collections() {
    let (mut vm, reader, blob) = fixture();
    run(&mut vm, "reader.readAsArrayBuffer(blob);0;");
    for _ in 0..3 {
        vm.collect_realm_garbage().unwrap();
        vm.run_browser_task_turn(1).unwrap();
    }
    let buffer = vm
        .heap
        .file_reader(reader)
        .unwrap()
        .result
        .object_id()
        .unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.heap.buffer_byte_length(buffer).unwrap(), 3);
    run(&mut vm, "reader=undefined;blob=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(reader) && !vm.heap.contains(blob) && !vm.heap.contains(buffer));
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_file_reader_storage_contracts() {
        file_reader_storage_binary_copy_stays_paid_through_prototype();
        file_reader_storage_neutral_binary_copy_keeps_original_credit();
        file_reader_storage_foreign_entry_refuses_existing_members();
        file_reader_storage_neutral_failed_heap_refuses_existing_members();
        file_reader_storage_foreign_constructor_stops_before_prototype();
        file_reader_storage_neutral_failed_constructor_stops_before_prototype();
        file_reader_storage_foreign_task_refuses_before_state_update();
        file_reader_storage_neutral_failed_task_refuses_before_state_update();
        file_reader_storage_managed_encoding_counts_retained_capacity();
        file_reader_storage_managed_result_counts_retained_capacity();
        file_reader_storage_managed_header_counts_inline_target_once();
        file_reader_storage_canceled_encoding_preserves_state_and_queue();
        file_reader_storage_neutral_canceled_encoding_preserves_state_and_queue();
        file_reader_storage_encoding_throw_keeps_initial_state();
        file_reader_storage_pending_root_survives_then_releases_under_normal_gc();
        file_reader_storage_packaging_throw_preserves_exception_and_root_prefix();
        file_reader_storage_abort_during_packaging_keeps_abort_state();
        file_reader_storage_restart_during_packaging_preserves_new_read();
        file_reader_storage_prepared_state_metadata_and_results_outlive_vm_with_credit();
        file_reader_storage_neutral_prepared_state_and_results_keep_original_credit();
        file_reader_storage_ordinary_packaging_error_can_start_a_new_read();
        file_reader_storage_binary_result_traces_between_normal_task_collections();
    }
}
