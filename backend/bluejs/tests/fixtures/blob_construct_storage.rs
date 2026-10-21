// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny existing Blob/File construction across guest conversion and GC.

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
fn construct(
    vm: &mut Vm,
    args: &[Value],
    file: bool,
    target: ObjectId,
) -> Result<Value, RuntimeError> {
    let previous = std::mem::replace(&mut vm.new_target, Value::Object(target));
    let result = vm.web_blob_construct(args, true, file);
    vm.new_target = previous;
    result
}
fn sample(kind: &str, text: &str, neutral: bool) -> usize {
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
    run(&mut vm,&format!("var label='{text}',converted={{toString(){{return label;}}}};var parts={};var options={{get endings(){{{}return 'transparent';}}}};var Target=new Proxy(function(){{}},{{get(t,k){{if(k==='prototype'){{{}return {}.prototype;}}return Reflect.get(t,k);}}}});0;",if kind=="name"{"[]"}else{"[converted]"},if kind=="bytes"{""}else{"observe();"},if kind=="bytes"{"observe();"}else{""},if kind=="name"{"File"}else{"Blob"}));
    let parts = object(&mut vm, "parts");
    let converted = object(&mut vm, "converted");
    let options = object(&mut vm, "options");
    let target = object(&mut vm, "Target");
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(observer.host_scheduling_snapshot().payload_bytes);
    let args = if kind == "name" {
        vec![
            Value::Object(parts),
            Value::Object(converted),
            Value::Object(options),
        ]
    } else {
        vec![Value::Object(parts), Value::Object(options)]
    };
    construct(&mut vm, &args, kind == "name", target).unwrap();
    sample.get().unwrap()
}
fn credit(kind: &str, neutral: bool) {
    assert_eq!(
        sample(kind, "abcdefgh", neutral) - sample(kind, "abc", neutral),
        if kind == "bytes" { 5 } else { 10 }
    );
}
#[cfg_attr(test, test)]
fn blob_construct_storage_file_name_credit_survives_options() {
    credit("name", false);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_neutral_file_name_keeps_original_credit() {
    credit("name", true);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_converted_part_credit_survives_options() {
    credit("part", false);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_neutral_part_keeps_original_credit() {
    credit("part", true);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_packed_bytes_stay_paid_through_prototype_getter() {
    credit("bytes", false);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_neutral_packed_bytes_keep_original_credit() {
    credit("bytes", true);
}
fn refused_entry(foreign: bool) {
    let mut vm = page();
    run(&mut vm,"var calls=0;var parts={[Symbol.iterator](){calls++;return {next(){return {done:true};}}}};var Target=Blob;0;");
    let parts = object(&mut vm, "parts");
    let target = object(&mut vm, "Target");
    if foreign {
        let other = page();
        drop(vm.host_scheduling.take());
        vm.host_scheduling = other.host_scheduling.as_ref().map(SchedulingScope::child);
    } else {
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        construct(&mut vm, &[Value::Object(parts)], false, target),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
}
#[cfg_attr(test, test)]
fn blob_construct_storage_foreign_entry_refuses_before_iterator() {
    refused_entry(true);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_neutral_failed_heap_refuses_before_iterator() {
    refused_entry(false);
}
fn canceled(neutral: bool, name: bool) {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        if name {
            "var calls=0,parts=[];var filename={toString(){cancel();return 'one.txt';}};var options={get endings(){calls++;return 'transparent';}};var Target=File;0;"
        } else {
            "var calls=0,parts=[];var options={get type(){cancel();return 'text/plain';}};var Target=new Proxy(function(){},{get(t,k){if(k==='prototype'){calls++;return Blob.prototype;}return Reflect.get(t,k);}});0;"
        },
    );
    let parts = object(&mut vm, "parts");
    let options = object(&mut vm, "options");
    let target = object(&mut vm, "Target");
    let args = if name {
        let filename = object(&mut vm, "filename");
        vec![
            Value::Object(parts),
            Value::Object(filename),
            Value::Object(options),
        ]
    } else {
        vec![Value::Object(parts), Value::Object(options)]
    };
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        construct(&mut vm, &args, name, target),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn blob_construct_storage_canceled_filename_stops_options() {
    canceled(false, true);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_neutral_canceled_filename_stops_options() {
    canceled(true, true);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_canceled_options_stop_prototype() {
    canceled(false, false);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_neutral_canceled_options_stop_prototype() {
    canceled(true, false);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_iterator_close_preserves_original_throw_and_root_prefix() {
    let mut vm = page();
    run(&mut vm,"var closed=0,parts={[Symbol.iterator](){let seen=false;return {next(){if(seen)return {done:true};seen=true;return {done:false,value:{toString(){throw 7;}}};},return(){closed++;throw 9;}};}};var Target=Blob;0;");
    let parts = object(&mut vm, "parts");
    let target = object(&mut vm, "Target");
    let before = vm.stack.len();
    assert_eq!(
        construct(&mut vm, &[Value::Object(parts)], false, target),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(
        vm.lookup_global_name("closed").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(vm.stack.len(), before);
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn blob_construct_storage_usv_metadata_endings_and_dictionary_order_remain() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"var order='';var f=new File(['a\\r\\nb\\rc\\n\\ud800'],'one\\ud800',{get endings(){order+='e';return 'native';},get lastModified(){order+='m';return 17;},get type(){order+='t';return 'TEXT/PLAIN';}});order==='emt'&&f.name==='one�'&&f.type==='text/plain'&&f.lastModified===17;"),Value::Bool(true));
    let file = object(&mut vm, "f");
    let expected = if cfg!(windows) {
        b"a\r\nb\r\nc\r\n\xef\xbf\xbd".as_slice()
    } else {
        b"a\nb\nc\n\xef\xbf\xbd".as_slice()
    };
    assert_eq!(vm.heap.web_blob(file).unwrap().bytes, expected);
}
#[cfg_attr(test, test)]
fn blob_construct_storage_buffers_snapshot_after_options_conversion() {
    let mut vm = page();
    run(&mut vm,"var bytes=new Uint8Array([65]);var b=new Blob([bytes],{get type(){bytes[0]=66;return 'APPLICATION/X-TEST';}});0;");
    let blob = object(&mut vm, "b");
    assert_eq!(vm.heap.web_blob(blob).unwrap().bytes, [66]);
    assert_eq!(
        vm.heap.web_blob(blob).unwrap().media_type,
        "application/x-test"
    );
}
#[cfg_attr(test, test)]
fn blob_construct_storage_completed_blob_and_file_follow_normal_gc() {
    let mut vm = page();
    run(
        &mut vm,
        "var b=new Blob(['one']),f=new File([b],'one.txt');0;",
    );
    let blob = object(&mut vm, "b");
    let file = object(&mut vm, "f");
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.heap.web_blob(file).unwrap().bytes, b"one");
    run(&mut vm, "b=undefined;f=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(blob) && !vm.heap.contains(file));
}

fn candidate_lifetime(neutral: bool) {
    let mut vm = page();
    // This direct native-consumer check needs the same ordinary execution
    // entry as the constructor cases before its bounded per-part step.
    run(&mut vm, "0;");
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let context = EventContext::capture(&vm).unwrap();
    let mut parts = TransportVector::with_capacity(1, context.owner.as_ref()).unwrap();
    parts
        .push_owned(error_input_storage::utf16_from_utf8("one🧊", context.owner.as_ref()).unwrap())
        .unwrap();
    let packed = storage::pack_parts(&mut vm, parts, false, &context).unwrap();
    let media_type =
        error_input_storage::utf16_from_utf8("text/plain", context.owner.as_ref()).unwrap();
    let name = error_input_storage::utf16_from_utf8("one.txt", context.owner.as_ref()).unwrap();
    let candidate = packed.into_candidate(media_type, Some((name, 17))).unwrap();
    let bytes = candidate.data.owned_buffer_bytes();
    assert_eq!(candidate.data.bytes, "one🧊".as_bytes());
    vm.collect_realm_garbage().unwrap();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(candidate);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn blob_construct_storage_candidate_buffers_keep_credit_after_vm_drop() {
    candidate_lifetime(false);
}

#[cfg_attr(test, test)]
fn blob_construct_storage_neutral_candidate_keeps_original_credit_after_vm_drop() {
    candidate_lifetime(true);
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_blob_construct_storage_contracts() {
        blob_construct_storage_file_name_credit_survives_options();
        blob_construct_storage_neutral_file_name_keeps_original_credit();
        blob_construct_storage_converted_part_credit_survives_options();
        blob_construct_storage_neutral_part_keeps_original_credit();
        blob_construct_storage_packed_bytes_stay_paid_through_prototype_getter();
        blob_construct_storage_neutral_packed_bytes_keep_original_credit();
        blob_construct_storage_foreign_entry_refuses_before_iterator();
        blob_construct_storage_neutral_failed_heap_refuses_before_iterator();
        blob_construct_storage_canceled_filename_stops_options();
        blob_construct_storage_neutral_canceled_filename_stops_options();
        blob_construct_storage_canceled_options_stop_prototype();
        blob_construct_storage_neutral_canceled_options_stop_prototype();
        blob_construct_storage_iterator_close_preserves_original_throw_and_root_prefix();
        blob_construct_storage_usv_metadata_endings_and_dictionary_order_remain();
        blob_construct_storage_buffers_snapshot_after_options_conversion();
        blob_construct_storage_completed_blob_and_file_follow_normal_gc();
        blob_construct_storage_candidate_buffers_keep_credit_after_vm_drop();
        blob_construct_storage_neutral_candidate_keeps_original_credit_after_vm_drop();
    }
}
