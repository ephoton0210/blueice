// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny FileList original accounts, live iteration, physical slots and GC.

use super::*;
use crate::vm::realm_call_storage::RealmCallRoots;

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
fn global(vm: &mut Vm, name: &str, id: ObjectId) {
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.define_data(global, name, Value::Object(id), true, false, true)
        .unwrap();
}
fn list(vm: &mut Vm) -> (ObjectId, ObjectId) {
    let prototype = vm.globals["%WebFile%"];
    let file = vm
        .with_roots(|heap| {
            heap.alloc_web_blob(
                WebBlobData {
                    bytes: vec![7],
                    media_type: "text/plain".into(),
                    file: Some(("one.txt".into(), 0)),
                },
                prototype,
            )
        })
        .unwrap();
    let prototype = vm.globals["%WebFileList%"];
    let list = vm
        .with_roots(|heap| heap.alloc_web_file_list(vec![file], prototype))
        .unwrap();
    global(vm, "list", list);
    vm.define_data(list, "0", Value::Object(file), false, true, false)
        .unwrap();
    (list, file)
}
fn iterator(vm: &mut Vm, list: ObjectId) -> ObjectId {
    let iterator = vm
        .web_file_list_call(NativeFunction::FileListIterator, &Value::Object(list), &[])
        .unwrap()
        .object_id()
        .unwrap();
    global(vm, "iterator", iterator);
    iterator
}
fn foreign(vm: &mut Vm) {
    let borrower = page();
    drop(vm.host_scheduling.take());
    vm.host_scheduling = borrower
        .host_scheduling
        .as_ref()
        .map(SchedulingScope::child);
}
#[cfg_attr(test, test)]
fn file_list_storage_foreign_length_refuses_original_heap() {
    let mut vm = page();
    let (list, _) = list(&mut vm);
    foreign(&mut vm);
    assert_eq!(
        vm.web_file_list_call(NativeFunction::FileListLength, &Value::Object(list), &[]),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn file_list_storage_neutral_length_checks_failed_original_heap() {
    let mut vm = page();
    let (list, _) = list(&mut vm);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    drop(vm.host_scheduling.take());
    assert_eq!(
        vm.web_file_list_call(NativeFunction::FileListLength, &Value::Object(list), &[]),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn file_list_storage_foreign_item_refuses_before_index_conversion() {
    let mut vm = page();
    let (list, _) = list(&mut vm);
    run(
        &mut vm,
        "var calls=0;var index={valueOf(){calls++;return 0;}};0;",
    );
    let index = object(&mut vm, "index");
    foreign(&mut vm);
    assert_eq!(
        vm.web_file_list_call(
            NativeFunction::FileListItem,
            &Value::Object(list),
            &[Value::Object(index)]
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(0.0))
    );
}
fn canceled_index(neutral: bool) {
    let mut vm = page();
    let (list, _) = list(&mut vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var index={valueOf(){cancel();return 0;}};0;");
    let index = object(&mut vm, "index");
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        vm.web_file_list_call(
            NativeFunction::FileListItem,
            &Value::Object(list),
            &[Value::Object(index)]
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_list_storage_canceled_index_refuses_selected_file() {
    canceled_index(false);
}
#[cfg_attr(test, test)]
fn file_list_storage_neutral_canceled_index_keeps_original_context() {
    canceled_index(true);
}
#[cfg_attr(test, test)]
fn file_list_storage_foreign_iterator_refuses_before_lazy_publication() {
    let mut vm = page();
    let (list, _) = list(&mut vm);
    assert!(!vm.globals.contains_key("%WebFileListIterator%"));
    foreign(&mut vm);
    assert_eq!(
        vm.web_file_list_call(NativeFunction::FileListIterator, &Value::Object(list), &[]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!vm.globals.contains_key("%WebFileListIterator%"));
}
fn refused_next(neutral: bool) {
    let mut vm = page();
    let (list, _) = list(&mut vm);
    let iterator = iterator(&mut vm, list);
    if neutral {
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
        drop(vm.host_scheduling.take());
    } else {
        foreign(&mut vm);
    }
    let result = vm.web_file_list_iterator_next(&Value::Object(iterator));
    let (_, index, done, _) = vm.heap.array_iterator(iterator).unwrap().unwrap();
    assert_eq!(index, 0, "refused entry never advances the iterator");
    assert!(!done);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
}
#[cfg_attr(test, test)]
fn file_list_storage_foreign_next_refuses_before_iterator_progress() {
    refused_next(false);
}
#[cfg_attr(test, test)]
fn file_list_storage_neutral_failed_next_never_advances_iterator() {
    refused_next(true);
}
#[cfg_attr(test, test)]
fn file_list_storage_iterator_keeps_file_through_normal_gc_then_releases_it() {
    let mut vm = page();
    let (list, file) = list(&mut vm);
    iterator(&mut vm, list);
    run(&mut vm, "list=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(file));
    assert_eq!(
        run(&mut vm, "iterator.next().value.name;"),
        Value::String("one.txt".into())
    );
    run(&mut vm, "iterator=undefined;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(file));
}
#[cfg_attr(test, test)]
fn file_list_storage_static_and_live_drag_iteration_keep_existing_semantics() {
    let mut vm = page();
    list(&mut vm);
    assert_eq!(run(&mut vm,"var it=list[Symbol.iterator]();var first=it.next();var second=it.next();list.length===1&&list.item(0)===list[0]&&list.item(1)===null&&list.item(-1)===null&&first.value===list[0]&&!first.done&&second.done&&second.value===undefined;"),Value::Bool(true));
    assert_eq!(run(&mut vm,"var d=new DataTransfer();d.items.add(new File(['a'],'a.txt'));var live=d.files;var next=live[Symbol.iterator]();d.items.clear();live.length===0&&live.item(0)===null&&next.next().done;"),Value::Bool(true));
}
fn managed_slots(capacity: usize) -> usize {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    let file = heap.alloc_object(None).unwrap();
    let before = heap.stats().managed_bytes;
    let mut files = Vec::with_capacity(capacity);
    files.push(file);
    heap.alloc_web_file_list(files, prototype).unwrap();
    heap.stats().managed_bytes - before
}
#[cfg_attr(test, test)]
fn file_list_storage_managed_list_counts_all_retained_slots() {
    assert_eq!(
        managed_slots(5) - managed_slots(1),
        4 * std::mem::size_of::<ObjectId>()
    );
}
#[cfg_attr(test, test)]
fn file_list_storage_failed_heap_cannot_borrow_healthy_call_root_for_length() {
    let mut vm = page();
    let (list, _) = list(&mut vm);
    let original = vm.host_scheduling.as_ref().unwrap().child();
    let caller = page();
    let owner = caller.argument_list_owner().unwrap();
    let roots = RealmCallRoots::new(0, owner.as_ref()).unwrap();
    drop(vm.host_scheduling.take());
    let mut refused = None;
    let result = vm.with_realm_call_roots(roots, |vm| {
        let _ = original.fail();
        let result =
            vm.web_file_list_call(NativeFunction::FileListLength, &Value::Object(list), &[]);
        refused = Some(matches!(result, Err(RuntimeError::SchedulingLimit)));
        result
    });
    assert_eq!(refused, Some(true));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(caller.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn file_list_storage_reader_revisions_preserve_identity_and_release_old_lists() {
    use std::cell::Cell;
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let key = HostObjectKey::new(7, 4, 3);
    vm.install_host_object_factory("target", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    let calls = Rc::new(Cell::new(0u64));
    let count = calls.clone();
    vm.install_host_file_input_reader(family, move |_: HostObjectKey, known: Option<u64>| {
        let revision = count.get() + 1;
        count.set(revision);
        assert_eq!(
            known,
            if revision == 1 {
                None
            } else {
                Some(revision - 1)
            }
        );
        Ok(if revision == 4 {
            HostInputFilesUpdate::NotFileInput
        } else {
            HostInputFilesUpdate::Selected {
                revision,
                files: vec![HostFileData {
                    name: "one.txt".into(),
                    media_type: if revision == 2 {
                        "TEXT/PLAIN"
                    } else {
                        "text/plain"
                    }
                    .into(),
                    last_modified: 17,
                    bytes: vec![if revision == 3 { 8 } else { 7 }],
                }],
            }
        })
    })
    .unwrap();
    assert_eq!(run(&mut vm,"var first=target().files,second=target().files,third=target().files,none=target().files;var kept=first[0];first===second&&first!==third&&first[0]===second[0]&&first[0]!==third[0]&&none===null&&first.item(0)===first[0]&&third.item(0)===third[0];"),Value::Bool(true));
    assert_eq!(calls.get(), 4);
    let file = object(&mut vm, "kept");
    run(
        &mut vm,
        "first=undefined;second=undefined;third=undefined;kept=undefined;0;",
    );
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(file));
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_file_list_storage_contracts() {
        file_list_storage_reader_revisions_preserve_identity_and_release_old_lists();
        file_list_storage_foreign_length_refuses_original_heap();
        file_list_storage_neutral_length_checks_failed_original_heap();
        file_list_storage_foreign_item_refuses_before_index_conversion();
        file_list_storage_canceled_index_refuses_selected_file();
        file_list_storage_neutral_canceled_index_keeps_original_context();
        file_list_storage_foreign_iterator_refuses_before_lazy_publication();
        file_list_storage_foreign_next_refuses_before_iterator_progress();
        file_list_storage_neutral_failed_next_never_advances_iterator();
        file_list_storage_iterator_keeps_file_through_normal_gc_then_releases_it();
        file_list_storage_static_and_live_drag_iteration_keep_existing_semantics();
        file_list_storage_managed_list_counts_all_retained_slots();
        file_list_storage_failed_heap_cannot_borrow_healthy_call_root_for_length();
    }
}
