// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny reader registry/cache ownership and callback publication boundaries.

use super::*;
use std::cell::Cell;

fn page(bounded: bool) -> (Vm, HostObjectFamily, ObjectId, HostObjectKey) {
    let mut vm = Vm::default();
    if bounded {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
    }
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    let key = HostObjectKey::new(7, 4, 3);
    vm.install_host_object_factory("target", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    vm.execute_script(
        &crate::compile(&crate::parse("var targetObject=target();0;").unwrap()).unwrap(),
    )
    .unwrap();
    let object = vm
        .lookup_global_name("targetObject")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    (vm, family, object, key)
}
fn view(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn file() -> HostFileData {
    HostFileData {
        name: "one.txt".into(),
        media_type: "TEXT/PLAIN".into(),
        last_modified: 17,
        bytes: vec![7],
    }
}
fn selected(revision: u64) -> HostInputFilesUpdate {
    HostInputFilesUpdate::Selected {
        revision,
        files: vec![file()],
    }
}
struct Reader {
    marker: [u8; 12],
}
impl HostInputFilesReader for Reader {
    fn read(
        &mut self,
        _: HostObjectKey,
        known: Option<u64>,
    ) -> Result<HostInputFilesUpdate, HostFunctionError> {
        assert_eq!(self.marker[0], 7);
        Ok(if known.is_none() {
            selected(1)
        } else {
            HostInputFilesUpdate::Unchanged
        })
    }
}
fn bytes(vm: &Vm) -> usize {
    vm.host_file_inputs.capacity() * std::mem::size_of::<HostFileInputRegistration>()
        + vm.host_file_inputs
            .iter()
            .map(|record| std::mem::size_of_val(&*record.reader))
            .sum::<usize>()
}
fn records(late: bool) {
    let (mut vm, family, _, _) = page(!late);
    vm.install_host_file_input_reader(family, Reader { marker: [7; 12] })
        .unwrap();
    if late {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
    }
    let observer = view(&vm);
    let expected = bytes(&vm);
    let records = std::mem::take(&mut vm.host_file_inputs);
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, expected);
    drop(records);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn file_input_registry_slots_and_concrete_box_outlive_vm() {
    records(false);
}
#[cfg_attr(test, test)]
fn file_input_registry_late_activation_adopts_existing_slots_and_box() {
    records(true);
}
#[cfg_attr(test, test)]
fn file_input_registry_cached_map_keeps_independent_credit_after_vm_drop() {
    let (mut vm, family, target, _) = page(true);
    vm.install_host_file_input_reader(family, Reader { marker: [7; 12] })
        .unwrap();
    vm.web_input_files(0, &Value::Object(target)).unwrap();
    let observer = view(&vm);
    let expected = host_scheduling::map_slots(&vm.host_file_inputs[0].cached);
    let cache = std::mem::take(&mut vm.host_file_inputs[0].cached);
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, expected);
    drop(cache);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn file_input_registry_sparse_cache_keeps_its_whole_allocation_paid() {
    let (mut vm, family, first, key) = page(true);
    let second_key = HostObjectKey::new(7, 4, 4);
    vm.install_host_object_factory("secondTarget", 0, family, move |_: &[HostValue]| {
        Ok(Some(second_key))
    })
    .unwrap();
    vm.execute_script(
        &crate::compile(&crate::parse("var secondObject=secondTarget();0;").unwrap()).unwrap(),
    )
    .unwrap();
    let second = vm
        .lookup_global_name("secondObject")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let remove = Rc::new(Cell::new(false));
    let mode = remove.clone();
    vm.install_host_file_input_reader(family, move |key: HostObjectKey, _: Option<u64>| {
        Ok(if mode.get() && key.object() == 3 {
            HostInputFilesUpdate::NotFileInput
        } else {
            selected(1)
        })
    })
    .unwrap();
    vm.web_input_files(0, &Value::Object(first)).unwrap();
    vm.web_input_files(0, &Value::Object(second)).unwrap();
    let expected = host_scheduling::map_slots(&vm.host_file_inputs[0].cached);
    remove.set(true);
    assert_eq!(
        vm.web_input_files(0, &Value::Object(first)).unwrap(),
        Value::Null
    );
    assert!(!vm.host_file_inputs[0].cached.contains_key(&key));
    let observer = view(&vm);
    let cache = std::mem::take(&mut vm.host_file_inputs[0].cached);
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, expected);
    drop(cache);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn file_input_registry_foreign_entry_refuses_before_reader() {
    let (mut vm, family, target, _) = page(true);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    vm.install_host_file_input_reader(family, move |_: HostObjectKey, _: Option<u64>| {
        count.set(count.get() + 1);
        Ok(HostInputFilesUpdate::NotFileInput)
    })
    .unwrap();
    let (borrower, _, _, _) = page(true);
    drop(vm.host_scheduling.take());
    vm.host_scheduling = borrower
        .host_scheduling
        .as_ref()
        .map(SchedulingScope::child);
    assert_eq!(
        vm.web_input_files(0, &Value::Object(target)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 0);
}
#[cfg_attr(test, test)]
fn file_input_registry_failed_original_registry_cannot_borrow_healthy_heap() {
    let (mut vm, family, target, _) = page(true);
    vm.install_host_file_input_reader(family, Reader { marker: [7; 12] })
        .unwrap();
    let observer = view(&vm);
    let (mut donor, donor_family, _, _) = page(true);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    donor
        .install_host_file_input_reader(donor_family, move |_: HostObjectKey, _: Option<u64>| {
            count.set(count.get() + 1);
            Ok(HostInputFilesUpdate::NotFileInput)
        })
        .unwrap();
    vm.host_file_inputs = std::mem::take(&mut donor.host_file_inputs);
    vm.host_file_inputs[0].family = family;
    let _ = donor.host_scheduling.as_ref().unwrap().fail();
    drop(vm.host_scheduling.take());
    assert_eq!(
        vm.web_input_files(0, &Value::Object(target)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 0);
    assert!(observer.check_scheduling_failure().is_ok());
}
fn canceled_update(neutral: bool, remove: bool) {
    let (mut vm, family, target, key) = page(true);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_file_input_reader(family, move |_: HostObjectKey, known: Option<u64>| {
        if known.is_none() {
            return Ok(selected(1));
        }
        let _ = scope.fail();
        Ok(if remove {
            HostInputFilesUpdate::NotFileInput
        } else {
            selected(2)
        })
    })
    .unwrap();
    vm.web_input_files(0, &Value::Object(target)).unwrap();
    let before = vm.host_file_inputs[0].cached[&key];
    if neutral {
        drop(vm.host_scheduling.take());
    }
    assert_eq!(
        vm.web_input_files(0, &Value::Object(target)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_file_inputs[0].cached.get(&key), Some(&before));
    assert!(vm.realm_call_roots.is_none());
}
#[cfg_attr(test, test)]
fn file_input_registry_canceled_same_content_keeps_original_revision() {
    canceled_update(false, false);
}
#[cfg_attr(test, test)]
fn file_input_registry_neutral_canceled_same_content_keeps_original_revision() {
    canceled_update(true, false);
}
#[cfg_attr(test, test)]
fn file_input_registry_canceled_retirement_keeps_original_cache() {
    canceled_update(false, true);
}
#[cfg_attr(test, test)]
fn file_input_registry_neutral_canceled_retirement_keeps_original_cache() {
    canceled_update(true, true);
}
#[cfg_attr(test, test)]
fn file_input_registry_callback_cancellation_precedes_host_error() {
    let (mut vm, family, target, _) = page(true);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_file_input_reader(family, move |_: HostObjectKey, _: Option<u64>| {
        let _ = scope.fail();
        Err(HostFunctionError::new("reader failed"))
    })
    .unwrap();
    assert_eq!(
        vm.web_input_files(0, &Value::Object(target)),
        Err(RuntimeError::SchedulingLimit)
    );
}
fn refused_install(foreign: bool) {
    let (mut vm, family, _, _) = page(true);
    let prototype = vm.host_family_prototype(family).unwrap();
    if foreign {
        let (other, _, _, _) = page(true);
        drop(vm.host_scheduling.take());
        vm.host_scheduling = other.host_scheduling.as_ref().map(SchedulingScope::child);
    } else {
        let _ = vm.host_scheduling.as_ref().unwrap().fail();
    }
    assert_eq!(
        vm.install_host_file_input_reader(family, Reader { marker: [7; 12] }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.host_file_inputs.is_empty());
    assert!(vm
        .heap
        .get_own_property_descriptor(prototype, "files")
        .unwrap()
        .is_none());
}
#[cfg_attr(test, test)]
fn file_input_registry_failed_install_never_publishes_getter() {
    refused_install(false);
}
#[cfg_attr(test, test)]
fn file_input_registry_foreign_install_never_publishes_getter() {
    refused_install(true);
}
#[cfg_attr(test, test)]
fn file_input_registry_revision_and_invalid_metadata_keep_previous_snapshot() {
    let (mut vm, family, target, key) = page(true);
    let mode = Rc::new(Cell::new(0));
    let state = mode.clone();
    vm.install_host_file_input_reader(family, move |_: HostObjectKey, _: Option<u64>| {
        Ok(match state.get() {
            0 => selected(1),
            1 => HostInputFilesUpdate::Unchanged,
            2 => selected(1),
            _ => {
                let mut file = file();
                file.name.clear();
                HostInputFilesUpdate::Selected {
                    revision: 2,
                    files: vec![file],
                }
            }
        })
    })
    .unwrap();
    let first = vm.web_input_files(0, &Value::Object(target)).unwrap();
    let before = vm.host_file_inputs[0].cached[&key];
    mode.set(1);
    assert_eq!(
        vm.web_input_files(0, &Value::Object(target)).unwrap(),
        first
    );
    mode.set(2);
    assert!(matches!(
        vm.web_input_files(0, &Value::Object(target)),
        Err(RuntimeError::TypeError(_))
    ));
    mode.set(3);
    assert!(matches!(
        vm.web_input_files(0, &Value::Object(target)),
        Err(RuntimeError::RangeError(_))
    ));
    assert_eq!(vm.host_file_inputs[0].cached[&key], before);
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(before.1));
}
#[cfg_attr(test, test)]
fn file_input_registry_empty_cache_releases_backing_and_old_gc_root() {
    let (mut vm, family, target, _) = page(true);
    vm.install_host_file_input_reader(family, move |_: HostObjectKey, known: Option<u64>| {
        Ok(if known.is_none() {
            selected(1)
        } else {
            HostInputFilesUpdate::NotFileInput
        })
    })
    .unwrap();
    let list = vm
        .web_input_files(0, &Value::Object(target))
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(
        vm.web_input_files(0, &Value::Object(target)).unwrap(),
        Value::Null
    );
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(list));
    assert!(vm.host_file_inputs[0].cached.is_empty());
    assert_eq!(
        host_scheduling::map_slots(&vm.host_file_inputs[0].cached),
        0
    );
}

fn staged_update() -> HostInputFilesUpdate {
    let mut files = Vec::with_capacity(3);
    let mut value = file();
    value.bytes = Vec::with_capacity(7);
    value.bytes.push(7);
    files.push(value);
    HostInputFilesUpdate::Selected { revision: 1, files }
}
#[cfg_attr(test, test)]
fn file_input_registry_returned_snapshot_retains_all_buffers_after_vm_drop() {
    let (vm, _, _, _) = page(true);
    let observer = view(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let value = staged_update();
    let expected = match &value {
        HostInputFilesUpdate::Selected { files, .. } => {
            host_scheduling::slots(files)
                + files
                    .iter()
                    .map(|file| {
                        file.name.capacity() + file.media_type.capacity() + file.bytes.capacity()
                    })
                    .sum::<usize>()
        }
        _ => unreachable!(),
    };
    let before = observer.host_scheduling_snapshot().payload_bytes;
    let value = NativeFileUpdate::adopt(value, owner.as_ref()).unwrap();
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes - before,
        expected
    );
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, expected);
    drop(value);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn file_input_registry_consumed_file_moves_bytes_and_keeps_iterator_slots_paid() {
    let (vm, _, _, _) = page(true);
    let observer = view(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let input = NativeFileUpdate::adopt(staged_update(), owner.as_ref()).unwrap();
    let (_, mut files) = input.into_selected();
    let blob = files.next_owned().unwrap().unwrap().into_blob().unwrap();
    assert_eq!(blob.data.bytes, [7]);
    assert_eq!(blob.data.bytes.capacity(), 7);
    let bytes = blob.data.owned_buffer_bytes();
    drop(vm);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        3 * std::mem::size_of::<HostFileData>() + bytes
    );
    drop(files);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(blob);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_file_input_registry_contracts() {
        file_input_registry_returned_snapshot_retains_all_buffers_after_vm_drop();
        file_input_registry_consumed_file_moves_bytes_and_keeps_iterator_slots_paid();
        file_input_registry_slots_and_concrete_box_outlive_vm();
        file_input_registry_late_activation_adopts_existing_slots_and_box();
        file_input_registry_cached_map_keeps_independent_credit_after_vm_drop();
        file_input_registry_sparse_cache_keeps_its_whole_allocation_paid();
        file_input_registry_foreign_entry_refuses_before_reader();
        file_input_registry_failed_original_registry_cannot_borrow_healthy_heap();
        file_input_registry_canceled_same_content_keeps_original_revision();
        file_input_registry_neutral_canceled_same_content_keeps_original_revision();
        file_input_registry_canceled_retirement_keeps_original_cache();
        file_input_registry_neutral_canceled_retirement_keeps_original_cache();
        file_input_registry_callback_cancellation_precedes_host_error();
        file_input_registry_failed_install_never_publishes_getter();
        file_input_registry_foreign_install_never_publishes_getter();
        file_input_registry_revision_and_invalid_metadata_keep_previous_snapshot();
        file_input_registry_empty_cache_releases_backing_and_old_gc_root();
    }
}
