// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny foreign pins and scalar native allowances exercise cleanup ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn realm_pin_storage_refusal_precedes_root_publication() {
    let mut vm = bounded_vm();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let before = vm.heap.stats();
    let state = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    let result = vm.heap.realm_root(object).map_err(RuntimeError::from);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.heap.root_storage_state(), state);
}

#[cfg_attr(test, test)]
fn realm_pin_storage_live_pin_has_a_native_owner() {
    let mut vm = bounded_vm();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let root_bytes = vm.heap.root_storage_bytes();
    let _pin = vm.heap.realm_root(object).unwrap();
    assert_eq!(vm.heap.root_storage_bytes(), root_bytes);
    assert!(vm.host_scheduling_snapshot().payload_bytes > before);
}

fn heap(vm: &Vm) -> Heap {
    let mut heap = Heap::default();
    heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget())
        .unwrap();
    heap
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    }
}
fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn pin_header() -> usize {
    crate::heap::RealmRoot::storage_bytes()
}

#[cfg_attr(test, test)]
fn realm_pin_storage_clones_share_ownership_at_full_scalar_quota() {
    let mut vm = bounded_vm();
    let object = vm.heap.alloc_object(None).unwrap();
    let pin = vm.heap.realm_root(object).unwrap();
    let state = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    let first = pin.clone();
    let second = first.clone();
    drop(pin);
    drop(first);
    vm.heap.release_realm_roots();
    assert_eq!(vm.heap.root_storage_state(), state);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
    assert_eq!(vm.heap.unroot(second.id), Ok(object));
}

#[cfg_attr(test, test)]
fn realm_pin_storage_last_handle_drop_does_not_grow_or_refund_storage() {
    let mut vm = bounded_vm();
    let object = vm.heap.alloc_object(None).unwrap();
    let pin = vm.heap.realm_root(object).unwrap();
    let root = vm.heap.root_storage_state();
    let pins = vm.heap.realm_pin_storage_state();
    let bytes = vm.heap.realm_pin_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(pin);
    assert_eq!(vm.heap.root_storage_state(), root);
    assert_eq!(vm.heap.realm_pin_storage_state(), pins);
    assert_eq!(vm.heap.realm_pin_storage_bytes(), bytes);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn realm_pin_storage_partial_release_keeps_live_pins_and_reserved_slots() {
    let vm = bounded_vm();
    let mut heap = heap(&vm);
    let object = heap.alloc_object(None).unwrap();
    let first = heap.realm_root(object).unwrap();
    let second = heap.realm_root(object).unwrap();
    let state = heap.realm_pin_storage_state();
    let bytes = heap.realm_pin_storage_bytes();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(first);
    heap.release_realm_roots();
    assert_eq!(heap.realm_pin_storage_state(), (1, state.1));
    assert_eq!(heap.realm_pin_storage_bytes(), bytes - pin_header());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - pin_header()
    );
    heap.try_collect_major().unwrap();
    assert!(heap.contains(object));
    assert_eq!(heap.unroot(second.id), Ok(object));
}

#[cfg_attr(test, test)]
fn realm_pin_storage_final_release_refunds_all_pin_and_root_capacity_after_failure() {
    let mut vm = bounded_vm();
    let mut heap = heap(&vm);
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    let bytes = heap.root_storage_bytes() + heap.realm_pin_storage_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(pin);
    heap.release_realm_roots();
    assert_eq!(heap.realm_pin_storage_state(), (0, 0));
    assert_eq!(heap.root_storage_bytes(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
    heap.release_realm_roots();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
}

#[cfg_attr(test, test)]
fn realm_pin_storage_rust_held_handle_outlives_its_heap() {
    let vm = bounded_vm();
    let observed = observer(&vm);
    let mut heap = heap(&vm);
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    let copy = pin.clone();
    drop(heap);
    drop(vm);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        pin_header()
    );
    drop(pin);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        pin_header()
    );
    drop(copy);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn realm_pin_storage_heap_outlives_the_original_document_scope() {
    let vm = bounded_vm();
    let observed = observer(&vm);
    let mut heap = heap(&vm);
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    let bytes = heap.realm_pin_storage_bytes() + heap.root_storage_bytes();
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(pin);
    heap.release_realm_roots();
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn realm_pin_storage_late_binding_attaches_shared_headers_and_slots_once() {
    let mut vm = Vm::default();
    let object = vm.heap.alloc_object(None).unwrap();
    let pin = vm.heap.realm_root(object).unwrap();
    let copy = pin.clone();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let bytes = vm.heap.root_storage_bytes()
        + vm.heap.realm_pin_storage_bytes()
        + vm.heap.collector_storage_bytes();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, bytes);
    let _peer = leave_room(&mut vm, 0);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    drop(pin);
    vm.heap.release_realm_roots();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(copy);
    let pins = vm.heap.realm_pin_storage_bytes();
    vm.heap.release_realm_roots();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - pins
    );
}

#[cfg_attr(test, test)]
fn realm_pin_storage_late_binding_refusal_preserves_pins_and_refunds_every_candidate() {
    let mut vm = bounded_vm();
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    heap.try_collect_major().unwrap();
    let room = heap.root_storage_bytes() + heap.collector_storage_bytes();
    let root = heap.root_storage_state();
    let pins = heap.realm_pin_storage_state();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        heap.bind_native_budget(vm.host_scheduling.as_ref().unwrap().heap_native_budget()),
        Err(HeapError::RootStorageLimit)
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
    assert_eq!(heap.root_storage_state(), root);
    assert_eq!(heap.realm_pin_storage_state(), pins);
    assert_eq!(heap.collector_storage_bytes(), 0);
    // Neither earlier owner was installed: another healthy document can bind.
    let other = bounded_vm();
    heap.bind_native_budget(other.host_scheduling.as_ref().unwrap().heap_native_budget())
        .unwrap();
    assert_eq!(
        other.host_scheduling_snapshot().payload_bytes,
        other.heap.root_storage_bytes()
            + heap.root_storage_bytes()
            + heap.realm_pin_storage_bytes()
    );
    drop(pin);
    heap.release_realm_roots();
    assert_eq!(
        other.host_scheduling_snapshot().payload_bytes,
        other.heap.root_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn realm_pin_storage_another_document_cannot_adopt_bound_pins() {
    let first = bounded_vm();
    let other = bounded_vm();
    let mut heap = heap(&first);
    let object = heap.alloc_object(None).unwrap();
    let _pin = heap.realm_root(object).unwrap();
    let before = first.host_scheduling_snapshot().payload_bytes;
    let other_before = other.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        heap.bind_native_budget(other.host_scheduling.as_ref().unwrap().heap_native_budget()),
        Err(HeapError::InvalidConfig)
    );
    assert_eq!(first.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(other.host_scheduling_snapshot().payload_bytes, other_before);
}

#[cfg_attr(test, test)]
fn realm_pin_storage_identifier_refusal_precedes_native_admission() {
    let mut vm = bounded_vm();
    let object = vm.heap.alloc_object(None).unwrap();
    vm.heap.allow_root_registrations(0);
    let root = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.heap.realm_root(object),
        Err(HeapError::IdExhausted)
    ));
    assert_eq!(vm.heap.root_storage_state(), root);
    assert_eq!(vm.heap.realm_pin_storage_state(), (0, 0));
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn realm_pin_storage_root_growth_refusal_rolls_back_prepared_pin_storage() {
    let mut vm = bounded_vm();
    let object = vm.heap.alloc_object(None).unwrap();
    vm.heap.root(object).unwrap();
    let room = pin_header() + 4 * std::mem::size_of::<Rc<crate::heap::RealmRoot>>();
    let before = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, room);
    assert!(matches!(
        vm.heap.realm_root(object),
        Err(HeapError::RootStorageLimit)
    ));
    assert_eq!(vm.heap.root_storage_state(), before);
    assert_eq!(vm.heap.realm_pin_storage_state(), (0, 0));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}

#[cfg_attr(test, test)]
fn realm_pin_storage_replacement_requires_the_complete_new_slot_allocation() {
    let mut vm = bounded_vm();
    let object = vm.heap.alloc_object(None).unwrap();
    let pins: Vec<_> = (0..4)
        .map(|_| vm.heap.realm_root(object).unwrap())
        .collect();
    assert_eq!(vm.heap.realm_pin_storage_state(), (4, 4));
    let before = vm.heap.root_storage_state();
    let room = pin_header() + 4 * std::mem::size_of::<Rc<crate::heap::RealmRoot>>();
    let _peer = leave_room(&mut vm, room);
    assert!(matches!(
        vm.heap.realm_root(object),
        Err(HeapError::RootStorageLimit)
    ));
    assert_eq!(vm.heap.root_storage_state(), before);
    assert_eq!(vm.heap.realm_pin_storage_state(), (4, 4));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
    drop(pins);
}

#[cfg_attr(test, test)]
fn realm_pin_storage_exact_replacement_allowance_refunds_the_old_slots() {
    let mut vm = bounded_vm();
    let object = vm.heap.alloc_object(None).unwrap();
    let mut pins: Vec<_> = (0..4)
        .map(|_| vm.heap.realm_root(object).unwrap())
        .collect();
    let old = 4 * std::mem::size_of::<Rc<crate::heap::RealmRoot>>();
    let _peer = leave_room(&mut vm, pin_header() + 2 * old);
    pins.push(vm.heap.realm_root(object).unwrap());
    assert_eq!(vm.heap.realm_pin_storage_state(), (5, 8));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old
    );
}

#[cfg_attr(test, test)]
fn realm_pin_storage_live_pins_survive_minor_and_major_collection() {
    let vm = bounded_vm();
    let mut heap = heap(&vm);
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    heap.try_collect_minor().unwrap();
    heap.try_collect_major().unwrap();
    assert!(heap.contains(object));
    drop(pin);
    heap.try_collect_major().unwrap();
    assert!(!heap.contains(object));
    assert_eq!(heap.realm_pin_storage_bytes(), 0);
}

#[cfg_attr(test, test)]
fn realm_pin_storage_ignored_live_pin_remains_a_conditional_edge() {
    let vm = bounded_vm();
    let mut heap = heap(&vm);
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    let marks = heap.realm_reachable(&HashSet::from([pin.id]), &[]).unwrap();
    assert!(!marks.contains(&object));
    assert!(heap.contains(object));
    assert_eq!(heap.realm_pin_storage_state().0, 1);
}

#[cfg_attr(test, test)]
fn realm_pin_storage_marking_skips_released_pins_without_mutating_the_registry() {
    let vm = bounded_vm();
    let mut heap = heap(&vm);
    let object = heap.alloc_object(None).unwrap();
    let pin = heap.realm_root(object).unwrap();
    drop(pin);
    let state = heap.realm_pin_storage_state();
    let roots = heap.root_storage_state();
    let marks = heap.realm_reachable(&HashSet::new(), &[]).unwrap();
    assert!(!marks.contains(&object));
    assert_eq!(heap.realm_pin_storage_state(), state);
    assert_eq!(heap.root_storage_state(), roots);
    heap.release_realm_roots();
    assert_eq!(heap.realm_pin_storage_state(), (0, 0));
}

#[cfg_attr(test, test)]
fn realm_pin_storage_invalid_object_refuses_before_native_admission() {
    let mut vm = bounded_vm();
    let mut other = Heap::default();
    let foreign = other.alloc_object(None).unwrap();
    let _peer = leave_room(&mut vm, 0);
    assert!(
        matches!(vm.heap.realm_root(foreign), Err(HeapError::InvalidObject(id)) if id == foreign)
    );
    assert_eq!(vm.heap.realm_pin_storage_state(), (0, 0));
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn realm_pin_storage_public_foreign_callbacks_survive_collection_and_release() {
    let mut vm = bounded_vm();
    assert_eq!(
        vm.execute_script(&script(
            "var realm=new ShadowRealm();var bridge=realm.evaluate('(fn)=>fn()');bridge(()=>42);"
        )),
        Ok(Value::Number(42.0))
    );
    vm.collect_realm_garbage().unwrap();
    assert_eq!(
        vm.execute_script(&script("bridge(()=>43);")),
        Ok(Value::Number(43.0))
    );
    vm.execute_script(&script("bridge=null;realm=null;0;"))
        .unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.heap.realm_pin_storage_state(), (0, 0));
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_realm_pin_storage_contracts() {
        realm_pin_storage_refusal_precedes_root_publication();
        realm_pin_storage_live_pin_has_a_native_owner();
        realm_pin_storage_clones_share_ownership_at_full_scalar_quota();
        realm_pin_storage_last_handle_drop_does_not_grow_or_refund_storage();
        realm_pin_storage_partial_release_keeps_live_pins_and_reserved_slots();
        realm_pin_storage_final_release_refunds_all_pin_and_root_capacity_after_failure();
        realm_pin_storage_rust_held_handle_outlives_its_heap();
        realm_pin_storage_heap_outlives_the_original_document_scope();
        realm_pin_storage_late_binding_attaches_shared_headers_and_slots_once();
        realm_pin_storage_late_binding_refusal_preserves_pins_and_refunds_every_candidate();
        realm_pin_storage_another_document_cannot_adopt_bound_pins();
        realm_pin_storage_identifier_refusal_precedes_native_admission();
        realm_pin_storage_root_growth_refusal_rolls_back_prepared_pin_storage();
        realm_pin_storage_replacement_requires_the_complete_new_slot_allocation();
        realm_pin_storage_exact_replacement_allowance_refunds_the_old_slots();
        realm_pin_storage_live_pins_survive_minor_and_major_collection();
        realm_pin_storage_ignored_live_pin_remains_a_conditional_edge();
        realm_pin_storage_marking_skips_released_pins_without_mutating_the_registry();
        realm_pin_storage_invalid_object_refuses_before_native_admission();
        realm_pin_storage_public_foreign_callbacks_survive_collection_and_release();
    }
}
