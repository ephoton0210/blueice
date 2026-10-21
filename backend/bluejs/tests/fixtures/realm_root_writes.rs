// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed root-write refusal, physical-credit and normal-GC contracts.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn append(
    roots: &RealmCallRoots,
    heap: &Heap,
    object: ObjectId,
    grow: bool,
) -> Result<(), RuntimeError> {
    if grow {
        roots.push_admitted(heap, object)
    } else {
        roots.push(heap, object)
    }
}

fn invalid_handle(grow: bool, capacity: usize, stale: bool, foreign_heap: bool) {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let mut target = Vm::default();
    let mut foreign = Vm::default();
    let object = if stale {
        let object = target.heap.alloc_object(None).unwrap();
        target.collect_realm_garbage().unwrap();
        assert!(!target.heap.contains(object));
        let replacement = target.heap.alloc_object(None).unwrap();
        assert_ne!(replacement, object);
        object
    } else {
        foreign.heap.alloc_object(None).unwrap()
    };
    let roots = RealmCallRoots::new(capacity, owner.as_ref()).unwrap();
    let before = roots.retained_bytes();
    let result = target.with_realm_call_roots(roots.clone(), |vm| {
        let heap = if foreign_heap {
            &foreign.heap
        } else {
            &vm.heap
        };
        append(&roots, heap, object, grow)
    });
    assert_eq!(
        result,
        Err(RuntimeError::Heap(HeapError::InvalidObject(object)))
    );
    assert!(roots.values.borrow().is_empty());
    assert_eq!(roots.retained_bytes(), before);
    assert!(target.realm_call_roots.is_none());
    assert!(source.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn realm_root_write_foreign_handle_refuses_spare_capacity() {
    invalid_handle(true, 1, false, false);
}

#[cfg_attr(test, test)]
fn realm_root_write_foreign_heap_cannot_replace_the_active_receiving_heap() {
    invalid_handle(true, 0, false, true);
}

#[cfg_attr(test, test)]
fn realm_root_write_collected_serial_refuses_spare_capacity() {
    invalid_handle(true, 1, true, false);
}

#[cfg_attr(test, test)]
fn realm_root_write_collected_serial_refuses_growth() {
    invalid_handle(true, 0, true, false);
}

#[cfg_attr(test, test)]
fn realm_root_write_fixed_capacity_refuses_a_foreign_handle() {
    invalid_handle(false, 1, false, false);
}

#[cfg_attr(test, test)]
fn realm_root_write_fixed_capacity_refuses_a_collected_serial() {
    invalid_handle(false, 1, true, false);
}

fn cancelled_write(caller: bool, capacity: usize) {
    let source = page();
    let source_scope = source.host_scheduling.as_ref().unwrap().child();
    let owner = source.argument_list_owner().unwrap();
    let mut ambient = page();
    let caller_scope = ambient.host_scheduling.as_ref().unwrap().child();
    let mut target = Vm::default();
    let object = target.heap.alloc_object(None).unwrap();
    let roots = RealmCallRoots::new(capacity, owner.as_ref()).unwrap();
    let before = roots.retained_bytes();
    let observed = RefCell::new(None);
    let active = realm_reentrancy::register_active(&mut ambient).unwrap();
    let result = target.with_realm_call_roots(roots.clone(), |vm| {
        let scope = if caller { &caller_scope } else { &source_scope };
        let _ = scope.fail();
        *observed.borrow_mut() = Some(append(&roots, &vm.heap, object, true));
        Ok(())
    });
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        observed.into_inner(),
        Some(Err(RuntimeError::SchedulingLimit))
    );
    assert!(roots.values.borrow().is_empty());
    assert_eq!(roots.retained_bytes(), before);
    assert!(target.realm_call_roots.is_none());
    assert!(if caller { &source } else { &ambient }
        .check_scheduling_failure()
        .is_ok());
}

#[cfg_attr(test, test)]
fn realm_root_write_failed_owner_refuses_spare_capacity() {
    cancelled_write(false, 1);
}

#[cfg_attr(test, test)]
fn realm_root_write_failed_owner_already_refuses_growth() {
    cancelled_write(false, 0);
}

#[cfg_attr(test, test)]
fn realm_root_write_failed_caller_refuses_spare_capacity() {
    cancelled_write(true, 1);
}

#[cfg_attr(test, test)]
fn realm_root_write_failed_caller_refuses_growth_from_a_healthy_owner() {
    cancelled_write(true, 0);
}

#[cfg_attr(test, test)]
fn realm_root_write_growth_and_truncate_keep_actual_backing_paid_until_drop() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let before = source.host_scheduling_snapshot().payload_bytes;
    let roots = RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let mut target = Vm::default();
    let object = target.heap.alloc_object(None).unwrap();
    target
        .with_realm_call_roots(roots.clone(), |vm| {
            append(&roots, &vm.heap, object, true)?;
            Ok(())
        })
        .unwrap();
    let expected = roots.retained_bytes();
    assert_eq!(
        source.host_scheduling_snapshot().payload_bytes - before,
        expected
    );
    roots.truncate(0);
    assert_eq!(roots.retained_bytes(), expected);
    assert_eq!(
        source.host_scheduling_snapshot().payload_bytes - before,
        expected
    );
    drop(roots);
    assert_eq!(source.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn realm_root_write_normal_gc_keeps_the_new_edge_only_during_the_call() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let roots = RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let mut target = Vm::default();
    let object = target.heap.alloc_object(None).unwrap();
    target
        .with_realm_call_roots(roots.clone(), |vm| {
            append(&roots, &vm.heap, object, true)?;
            vm.collect_realm_garbage()?;
            assert!(vm.heap.contains(object));
            Ok(())
        })
        .unwrap();
    target.collect_realm_garbage().unwrap();
    assert!(!target.heap.contains(object));
    assert_eq!(roots.values.borrow().as_slice(), &[object]);
}

#[cfg_attr(test, test)]
fn realm_root_write_terminal_truncate_still_retires_the_existing_edge() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    let mut target = Vm::default();
    let object = target.heap.alloc_object(None).unwrap();
    let result = target.with_realm_call_roots(roots.clone(), |vm| {
        append(&roots, &vm.heap, object, false)?;
        let _ = scope.fail();
        roots.truncate(0);
        Ok(())
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(roots.values.borrow().is_empty());
    target.collect_realm_garbage().unwrap();
    assert!(!target.heap.contains(object));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_realm_root_write_contracts() {
        realm_root_write_foreign_handle_refuses_spare_capacity();
        realm_root_write_foreign_heap_cannot_replace_the_active_receiving_heap();
        realm_root_write_collected_serial_refuses_spare_capacity();
        realm_root_write_collected_serial_refuses_growth();
        realm_root_write_fixed_capacity_refuses_a_foreign_handle();
        realm_root_write_fixed_capacity_refuses_a_collected_serial();
        realm_root_write_failed_owner_refuses_spare_capacity();
        realm_root_write_failed_owner_already_refuses_growth();
        realm_root_write_failed_caller_refuses_spare_capacity();
        realm_root_write_failed_caller_refuses_growth_from_a_healthy_owner();
        realm_root_write_growth_and_truncate_keep_actual_backing_paid_until_drop();
        realm_root_write_normal_gc_keeps_the_new_edge_only_during_the_call();
        realm_root_write_terminal_truncate_still_retires_the_existing_edge();
    }
}
