// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny existing Blob reads, native copies and original-account refusal.

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
fn read(
    vm: &mut Vm,
    native: NativeFunction,
    blob: ObjectId,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    vm.web_blob_call(native, &Value::Object(blob), args)
}
fn sample(length: usize, neutral: bool, typed: bool, native: NativeFunction) -> usize {
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
    let constructor = if typed { "Uint8Array" } else { "ArrayBuffer" };
    let content = if length == 3 { "abc" } else { "abcdefgh" };
    run(&mut vm, &format!("var b=new Blob(['{content}']);var original={constructor};var wrapped=new Proxy(function(){{}},{{get(t,k){{if(k==='prototype'){{observe();return original.prototype;}}return Reflect.get(t,k);}}}});0;"));
    let blob = object(&mut vm, "b");
    let wrapped = object(&mut vm, "wrapped");
    // Exercise the existing guest-capable prototype lookup at the native VM
    // boundary; public semantic controls keep the ordinary intrinsic cache.
    vm.globals.insert(constructor.into(), wrapped);
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    baseline.set(observer.host_scheduling_snapshot().payload_bytes);
    read(&mut vm, native, blob, &[]).unwrap();
    assert!(vm.realm_call_roots.is_none());
    sample.get().unwrap()
}
fn binary_credit(neutral: bool, bytes: bool) {
    let native = if bytes {
        NativeFunction::BlobBytes
    } else {
        NativeFunction::BlobArrayBuffer
    };
    assert_eq!(
        sample(8, neutral, false, native) - sample(3, neutral, false, native),
        5
    );
}
#[cfg_attr(test, test)]
fn blob_read_storage_array_buffer_copy_stays_paid_through_prototype() {
    binary_credit(false, false);
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_array_buffer_copy_keeps_original_credit() {
    binary_credit(true, false);
}
#[cfg_attr(test, test)]
fn blob_read_storage_bytes_copy_stays_paid_through_buffer_prototype() {
    binary_credit(false, true);
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_bytes_copy_keeps_original_credit() {
    binary_credit(true, true);
}
#[cfg_attr(test, test)]
fn blob_read_storage_finished_copy_releases_before_view_prototype() {
    assert_eq!(
        sample(8, false, true, NativeFunction::BlobBytes)
            - sample(3, false, true, NativeFunction::BlobBytes),
        0
    );
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_finished_copy_releases_before_view_prototype() {
    assert_eq!(
        sample(8, true, true, NativeFunction::BlobBytes)
            - sample(3, true, true, NativeFunction::BlobBytes),
        0
    );
}

fn refused(foreign: bool) {
    let mut vm = page();
    run(&mut vm, "var b=new File(['one'],'one.txt');0;");
    let blob = object(&mut vm, "b");
    if foreign {
        let other = page();
        drop(vm.host_scheduling.take());
        vm.host_scheduling = other.host_scheduling.as_ref().map(SchedulingScope::child);
    } else {
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
        drop(vm.host_scheduling.take());
    }
    for native in [
        NativeFunction::BlobSize,
        NativeFunction::BlobType,
        NativeFunction::FileName,
        NativeFunction::FileLastModified,
        NativeFunction::BlobSlice,
        NativeFunction::BlobText,
        NativeFunction::BlobArrayBuffer,
        NativeFunction::BlobBytes,
    ] {
        assert_eq!(
            read(&mut vm, native, blob, &[]),
            Err(RuntimeError::SchedulingLimit),
            "{native:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn blob_read_storage_foreign_entry_refuses_all_existing_members() {
    refused(true);
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_failed_heap_refuses_all_existing_members() {
    refused(false);
}
fn canceled(neutral: bool, prototype: bool) {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        if prototype {
            "var calls=0,b=new Blob(['one']);var original=ArrayBuffer;var wrapped=new Proxy(function(){},{get(t,k){if(k==='prototype'){cancel();return original.prototype;}return Reflect.get(t,k);}});0;"
        } else {
            "var calls=0,b=new Blob(['one']);var start={valueOf(){cancel();return 0;}},end={valueOf(){calls++;return 1;}};0;"
        },
    );
    let blob = object(&mut vm, "b");
    let args = if prototype {
        let wrapped = object(&mut vm, "wrapped");
        vm.globals.insert("ArrayBuffer".into(), wrapped);
        vec![]
    } else {
        vec![
            Value::Object(object(&mut vm, "start")),
            Value::Object(object(&mut vm, "end")),
        ]
    };
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let before = vm.stack.len();
    assert_eq!(
        read(
            &mut vm,
            if prototype {
                NativeFunction::BlobArrayBuffer
            } else {
                NativeFunction::BlobSlice
            },
            blob,
            &args
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert_eq!(vm.stack.len(), before);
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn blob_read_storage_canceled_start_stops_later_slice_coercion() {
    canceled(false, false);
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_canceled_start_stops_later_slice_coercion() {
    canceled(true, false);
}
#[cfg_attr(test, test)]
fn blob_read_storage_canceled_buffer_prototype_stops_read() {
    canceled(false, true);
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_canceled_buffer_prototype_stops_read() {
    canceled(true, true);
}
fn thrown(neutral: bool) {
    let mut vm = page();
    run(&mut vm,"var b=new Blob(['one']);var wrapped=new Proxy(function(){},{get(t,k){if(k==='prototype')throw 7;return Reflect.get(t,k);}});0;");
    let blob = object(&mut vm, "b");
    let wrapped = object(&mut vm, "wrapped");
    vm.globals.insert("ArrayBuffer".into(), wrapped);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let before = vm.stack.len();
    assert_eq!(
        read(&mut vm, NativeFunction::BlobBytes, blob, &[]),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(vm.stack.len(), before);
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn blob_read_storage_prototype_throw_preserves_exception_and_root_prefix() {
    thrown(false);
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_prototype_throw_preserves_exception_and_root_prefix() {
    thrown(true);
}
#[cfg_attr(test, test)]
fn blob_read_storage_slice_preserves_coercion_order_type_and_independent_bytes() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"var order='',b=new Blob(['abcdef']);var s=b.slice({valueOf(){order+='s';return 1;}},{valueOf(){order+='e';return 4;}},{toString(){order+='t';return 'TEXT/PLAIN';}});order==='set'&&s.size===3&&s.type==='text/plain';"),Value::Bool(true));
    let slice = object(&mut vm, "s");
    assert_eq!(vm.heap.web_blob(slice).unwrap().bytes, b"bcd");
}
#[cfg_attr(test, test)]
fn blob_read_storage_promises_keep_read_results_and_release_on_normal_gc() {
    let mut vm = page();
    run(&mut vm,"var b=new Blob([new Uint8Array([239,187,191,65,255,239,187,191])]),p=b.text(),q=b.bytes();0;");
    let blob = object(&mut vm, "b");
    let p = object(&mut vm, "p");
    let q = object(&mut vm, "q");
    vm.collect_realm_garbage().unwrap();
    {
        let (status, _storage) = vm.prepare_promise_status_copy(p).unwrap();
        let PromiseAwaitStatus::Fulfilled(value) = status else {
            panic!("fulfilled Blob text")
        };
        assert_eq!(value, Value::String("A�\u{feff}".into()));
    }
    run(&mut vm, "b=undefined;p=undefined;q=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(blob) && !vm.heap.contains(p) && !vm.heap.contains(q));
}

fn copied_lifetime(neutral: bool) {
    let mut vm = page();
    let observer = view(&vm);
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let context = EventContext::capture(&vm).unwrap();
    let bytes = BlobBytes::copied(b"one", context.owner.as_ref()).unwrap();
    let empty_type =
        NativeValue::adopt(Value::String(JsString::default()), context.owner.as_ref()).unwrap();
    let copy = bytes.into_candidate(empty_type, None).unwrap();
    let text = decode_text(
        b"\xef\xbb\xbfone\xf0\x9f\xa7\x8a\xff",
        vm.config.max_string_bytes,
        context.owner.as_ref(),
    )
    .unwrap();
    assert_eq!(text.value(), &Value::String("one🧊�".into()));
    let text_bytes = host_scheduling::value_payload_bytes(text.value());
    let bytes = copy.data.owned_buffer_bytes();
    vm.collect_realm_garbage().unwrap();
    drop(vm);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        bytes + text_bytes
    );
    drop(copy);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        text_bytes
    );
    drop(text);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn blob_read_storage_native_bytes_and_decoded_text_keep_credit_after_vm_drop() {
    copied_lifetime(false);
}
#[cfg_attr(test, test)]
fn blob_read_storage_neutral_copies_keep_original_credit_after_vm_drop() {
    copied_lifetime(true);
}
#[cfg_attr(test, test)]
fn blob_read_storage_utf8_replacement_and_bom_match_existing_decoding() {
    let vm = page();
    let context = EventContext::capture(&vm).unwrap();
    for bytes in [
        b"".as_slice(),
        b"abc",
        b"\xef\xbb\xbf",
        b"a\xef\xbb\xbfb",
        b"\xf0\x9f\xa7\x8a",
        b"\xff\xfe",
        b"a\xe2\x82",
        b"\xe2\x28\xa1\xed\xa0\x80",
    ] {
        let actual =
            decode_text(bytes, vm.config.max_string_bytes, context.owner.as_ref()).unwrap();
        let input = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        let expected: JsString = String::from_utf8_lossy(input).into_owned().into();
        assert_eq!(actual.value(), &Value::String(expected));
    }
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_blob_read_storage_contracts() {
        blob_read_storage_array_buffer_copy_stays_paid_through_prototype();
        blob_read_storage_neutral_array_buffer_copy_keeps_original_credit();
        blob_read_storage_bytes_copy_stays_paid_through_buffer_prototype();
        blob_read_storage_neutral_bytes_copy_keeps_original_credit();
        blob_read_storage_finished_copy_releases_before_view_prototype();
        blob_read_storage_neutral_finished_copy_releases_before_view_prototype();
        blob_read_storage_foreign_entry_refuses_all_existing_members();
        blob_read_storage_neutral_failed_heap_refuses_all_existing_members();
        blob_read_storage_canceled_start_stops_later_slice_coercion();
        blob_read_storage_neutral_canceled_start_stops_later_slice_coercion();
        blob_read_storage_canceled_buffer_prototype_stops_read();
        blob_read_storage_neutral_canceled_buffer_prototype_stops_read();
        blob_read_storage_prototype_throw_preserves_exception_and_root_prefix();
        blob_read_storage_neutral_prototype_throw_preserves_exception_and_root_prefix();
        blob_read_storage_slice_preserves_coercion_order_type_and_independent_bytes();
        blob_read_storage_promises_keep_read_results_and_release_on_normal_gc();
        blob_read_storage_native_bytes_and_decoded_text_keep_credit_after_vm_drop();
        blob_read_storage_neutral_copies_keep_original_credit_after_vm_drop();
        blob_read_storage_utf8_replacement_and_bom_match_existing_decoding();
    }
}
