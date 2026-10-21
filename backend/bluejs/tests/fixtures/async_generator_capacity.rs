// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed async control buffers and publication without pressure or quota probes.

use super::*;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn ready() -> (Vm, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    execute(&mut vm, "var gate=Promise.withResolvers();var iterator=(async function*(){await gate.promise;yield 7;return 8;})();var first=iterator.next();0;");
    let generator = vm
        .lookup_global_name("iterator")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    (vm, generator)
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn backing(control: &crate::heap::AsyncGeneratorControl) -> usize {
    control.requests.capacity() * std::mem::size_of::<AsyncGeneratorRequest>()
}

fn origin_bytes(control: &crate::heap::AsyncGeneratorControl) -> usize {
    control
        .requests
        .iter()
        .map(|request| {
            std::mem::size_of_val(request.origin.as_ref())
                + 2 * std::mem::size_of::<usize>()
                + request.origin.payload_bytes()
        })
        .sum()
}

#[cfg_attr(test, test)]
fn async_generator_capacity_snapshot_retains_its_complete_buffer() {
    let (vm, generator) = ready();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + backing(&snapshot)
    );
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn async_generator_capacity_detached_snapshot_keeps_buffer_after_vm_drop() {
    let (vm, generator) = ready();
    let view = observer(&vm);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let bytes =
        backing(&snapshot) + HeapReactionLease::allocation_bytes() + origin_bytes(&snapshot);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_capacity_empty_snapshot_keeps_actual_retained_capacity() {
    let (vm, generator) = ready();
    let view = observer(&vm);
    let mut snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    snapshot.requests.pop_front();
    let bytes = backing(&snapshot);
    assert!(bytes > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_capacity_failed_owner_refuses_a_new_snapshot() {
    let (vm, generator) = ready();
    let count = vm.host_scheduling_snapshot().pending_reactions;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.heap.async_generator_control(generator).err(),
        Some(HeapError::AsyncGeneratorStorageLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, count);
}

#[cfg_attr(test, test)]
fn async_generator_capacity_normal_fifo_delivery_and_gc_remain_intact() {
    let (mut vm, generator) = ready();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(generator));
    execute(&mut vm, "var second=iterator.next();var seen=[];first.then(v=>seen.push(v.value,v.done));second.then(v=>seen.push(v.value,v.done));gate.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "seen.join(',');"),
        Value::String("7,false,8,true".into())
    );
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_capacity_setter_roots_keep_their_own_buffer_after_vm_drop() {
    let (vm, generator) = ready();
    let view = observer(&vm);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let roots = vm
        .heap
        .prepare_async_generator_roots(generator, &snapshot)
        .unwrap();
    let bytes = roots.backing_bytes();
    assert!(bytes > 0);
    assert_eq!(roots.len(), 3);
    drop(snapshot);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(roots);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_capacity_neutral_publication_keeps_the_original_buffer_account() {
    let (mut vm, generator) = ready();
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    vm.async_generator_request(
        &Value::Object(generator),
        Value::Undefined,
        NativeFunction::AsyncGeneratorNext,
    )
    .unwrap();
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    assert_eq!(snapshot.requests.len(), 2);
    let bytes =
        backing(&snapshot) + 2 * HeapReactionLease::allocation_bytes() + origin_bytes(&snapshot);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 2);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg(test)]
#[test]
fn async_generator_capacity_status_and_retirement_allocate_no_buffers() {
    let (mut vm, generator) = ready();
    let (result, allocations) = crate::vm::allocation_observer::count(|| {
        vm.set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting)
    });
    result.unwrap();
    assert_eq!(allocations, 0);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    let (result, allocations) = crate::vm::allocation_observer::count(|| {
        vm.heap
            .pop_async_generator_request(generator, AsyncGeneratorStatus::Completed)
    });
    result.unwrap();
    assert_eq!(allocations, 0);
    let control = vm
        .heap
        .async_generator_control_ref(generator)
        .unwrap()
        .unwrap();
    assert!(control.requests.is_empty());
    assert_eq!(control.requests.capacity(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_async_generator_capacity_contracts() {
        async_generator_capacity_setter_roots_keep_their_own_buffer_after_vm_drop();
        async_generator_capacity_neutral_publication_keeps_the_original_buffer_account();
        async_generator_capacity_snapshot_retains_its_complete_buffer();
        async_generator_capacity_detached_snapshot_keeps_buffer_after_vm_drop();
        async_generator_capacity_empty_snapshot_keeps_actual_retained_capacity();
        async_generator_capacity_failed_owner_refuses_a_new_snapshot();
        async_generator_capacity_normal_fifo_delivery_and_gc_remain_intact();
    }
}
