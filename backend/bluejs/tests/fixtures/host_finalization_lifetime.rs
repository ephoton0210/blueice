// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny cleanup tasks preserve count, backing and prepared-input ownership.

use super::*;
use std::cell::Cell;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn ready(cells: usize) -> (Vm, ObjectId, Rc<Cell<usize>>) {
    let mut vm = page();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    vm.install_host_function("record", 1, move |args: &[HostValue]| {
        assert!(matches!(args, [HostValue::String(text)] if text == "tiny"));
        output.set(output.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm
        .execute_script(&crate::compile(&crate::parse("record;").unwrap()).unwrap())
        .unwrap();
    let registry = vm.heap.alloc_finalization_registry(callback, None).unwrap();
    vm.heap.root(registry).unwrap();
    for _ in 0..cells {
        let target = vm.heap.alloc_object(None).unwrap();
        vm.heap
            .finalization_registry_register(
                registry,
                Value::Object(target),
                Value::String("tiny".into()),
                None,
            )
            .unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.enqueue_host_finalization_tasks().unwrap();
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    (vm, registry, calls)
}

#[cfg_attr(test, test)]
fn host_finalization_lifetime_queue_outlives_vm_with_count_and_capacity() {
    let (mut vm, _, _) = ready(1);
    let view = observer(&vm);
    let bytes = vm.host_finalization_tasks.capacity() * std::mem::size_of::<HostFinalizationTask>();
    let tasks = std::mem::take(&mut vm.host_finalization_tasks);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(tasks);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_finalization_lifetime_prepared_callback_outlives_vm() {
    let (mut vm, registry, _) = ready(1);
    let prepared = vm.prepare_finalization_entry(registry).unwrap().unwrap();
    let bytes = host_scheduling::slots(&prepared.arguments) + 8;
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_finalization_lifetime_empty_queue_retains_original_account() {
    let (mut source, _, calls) = ready(1);
    source.run_browser_tasks_bounded(1).unwrap();
    assert_eq!(calls.get(), 1);
    assert!(source.host_finalization_tasks.is_empty());
    assert_eq!(source.host_finalization_tasks.capacity(), 0);
    let mut target = Vm {
        host_finalization_tasks: std::mem::take(&mut source.host_finalization_tasks),
        ..Vm::default()
    };
    assert_eq!(
        target.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn host_finalization_lifetime_duplicate_enqueues_and_delivery_preserve_one_task_per_registry() {
    let (mut vm, _, calls) = ready(2);
    vm.enqueue_host_finalization_tasks().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert!(!vm.run_browser_task_turn(1).unwrap());
    assert_eq!(calls.get(), 1);
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert!(vm.run_browser_task_turn(1).unwrap());
    assert_eq!(calls.get(), 2);
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(vm.host_finalization_tasks.capacity(), 0);
}
#[cfg_attr(test, test)]
fn host_finalization_lifetime_terminal_stale_cleanup_releases_count_and_root() {
    let (mut vm, registry, calls) = ready(1);
    drop(
        vm.heap
            .take_finalization_registry_cleanup_cell(registry)
            .unwrap()
            .unwrap(),
    );
    let roots = vm.heap.root_storage_state().0;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.run_next_host_finalization_task(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.host_finalization_tasks.is_empty());
    assert_eq!(vm.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(test, test)]
fn host_finalization_lifetime_prepared_task_keeps_slot_reservation_after_vm_drop() {
    let mut vm = page();
    let view = observer(&vm);
    let prepared = vm.prepare_host_finalization_task().unwrap();
    let bytes = prepared.growth.staging_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().pending_browser_tasks, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_finalization_lifetime_contracts() {
        host_finalization_lifetime_prepared_task_keeps_slot_reservation_after_vm_drop();
        host_finalization_lifetime_queue_outlives_vm_with_count_and_capacity();
        host_finalization_lifetime_prepared_callback_outlives_vm();
        host_finalization_lifetime_empty_queue_retains_original_account();
        host_finalization_lifetime_duplicate_enqueues_and_delivery_preserve_one_task_per_registry();
        host_finalization_lifetime_terminal_stale_cleanup_releases_count_and_root();
    }
}
