// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed primitive request buffers and copies, without pressure/quota probes.

use super::*;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn ready(scoped: bool) -> (Vm, ObjectId) {
    let mut vm = Vm::default();
    if scoped {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
    }
    execute(&mut vm, "var gate=Promise.withResolvers();var iterator=(async function*(){await gate.promise;var sent=yield 7;return sent;})();var first=iterator.next('tiny');0;");
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
fn completion_value(completion: &AsyncGeneratorCompletion) -> &Value {
    match completion {
        AsyncGeneratorCompletion::Next(value)
        | AsyncGeneratorCompletion::Return(value)
        | AsyncGeneratorCompletion::Throw(value)
        | AsyncGeneratorCompletion::ReturnAwaited(value)
        | AsyncGeneratorCompletion::ResumeReturn(value)
        | AsyncGeneratorCompletion::ResumeThrow(value) => value,
    }
}
fn payload(request: &AsyncGeneratorRequest) -> usize {
    host_scheduling::value_payload_bytes(completion_value(&request.completion))
}
fn request_bytes(request: &AsyncGeneratorRequest) -> usize {
    HeapReactionLease::allocation_bytes()
        + std::mem::size_of_val(request.origin.as_ref())
        + 2 * std::mem::size_of::<usize>()
        + request.origin.payload_bytes()
        + payload(request)
}
fn snapshot_bytes(control: &crate::heap::AsyncGeneratorControl) -> usize {
    control.requests.capacity() * std::mem::size_of::<AsyncGeneratorRequest>()
        + control.requests.iter().map(request_bytes).sum::<usize>()
}

#[cfg_attr(test, test)]
fn async_generator_values_snapshot_admits_the_independent_primitive_copy() {
    let (vm, generator) = ready(true);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let extra = copy.requests.capacity() * std::mem::size_of::<AsyncGeneratorRequest>()
        + copy.requests.iter().map(payload).sum::<usize>();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + extra);
    drop(copy);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn async_generator_values_detached_request_keeps_its_buffer_after_vm_drop() {
    let (vm, generator) = ready(true);
    let view = observer(&vm);
    let mut copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let request = copy.requests.pop_front().unwrap();
    let bytes = request_bytes(&request);
    drop(copy);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(
        completion_value(&request.completion),
        &Value::String("tiny".into())
    );
    drop(request);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_retired_snapshot_keeps_live_data_without_a_pending_count() {
    let (mut vm, generator) = ready(true);
    let view = observer(&vm);
    let copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let bytes = snapshot_bytes(&copy);
    execute(
        &mut vm,
        "var second=iterator.next('later');gate.resolve(0);0;",
    );
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_replacement_does_not_release_an_independent_old_copy() {
    let (mut vm, generator) = ready(true);
    let view = observer(&vm);
    let copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let bytes = snapshot_bytes(&copy);
    vm.replace_async_generator_request(
        generator,
        AsyncGeneratorCompletion::Next(Value::String("replacement".into())),
        None,
    )
    .unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(
        completion_value(&copy.requests.front().unwrap().completion),
        &Value::String("tiny".into())
    );
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_neutral_publication_preserves_original_payload_ownership() {
    let (mut vm, generator) = ready(true);
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    vm.async_generator_request(
        &Value::Object(generator),
        Value::String("later".into()),
        NativeFunction::AsyncGeneratorNext,
    )
    .unwrap();
    let copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    assert_eq!(copy.requests.len(), 2);
    let bytes = snapshot_bytes(&copy);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 2);
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_legacy_attachment_accounts_existing_primitive_data() {
    let (mut vm, generator) = ready(false);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let bytes = snapshot_bytes(&copy);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_failed_owner_refuses_snapshot_without_changing_the_queue() {
    let (vm, generator) = ready(true);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.heap.async_generator_control(generator).err(),
        Some(HeapError::AsyncGeneratorStorageLimit)
    );
    let retained = vm
        .heap
        .async_generator_control_ref(generator)
        .unwrap()
        .unwrap();
    assert_eq!(retained.requests.len(), 1);
    assert_eq!(
        completion_value(&retained.requests.front().unwrap().completion),
        &Value::String("tiny".into())
    );
}

#[cfg_attr(test, test)]
fn async_generator_values_fifo_primitive_delivery_and_normal_gc_remain_intact() {
    let (mut vm, generator) = ready(true);
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(generator));
    execute(&mut vm, "var second=iterator.next('later');var seen=[];first.then(v=>seen.push(v.value,v.done));second.then(v=>seen.push(v.value,v.done));gate.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "seen.join(',');"),
        Value::String("7,false,later,true".into())
    );
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_bigint_and_symbol_buffers_are_retained_with_their_records() {
    let (mut vm, generator) = ready(true);
    execute(
        &mut vm,
        "var nextBigInt=iterator.next(7n);var nextSymbol=iterator.next(Symbol('tiny'));0;",
    );
    let view = observer(&vm);
    let copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    assert_eq!(copy.requests.len(), 3);
    let bytes = snapshot_bytes(&copy);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_request_copies_own_distinct_buffers_and_share_one_count() {
    let (vm, generator) = ready(true);
    let view = observer(&vm);
    let mut copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let first = copy.requests.pop_front().unwrap();
    let second = first.try_snapshot(None).unwrap();
    let both = request_bytes(&first) + payload(&second);
    let remaining = request_bytes(&second);
    drop(copy);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, both);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(first);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, remaining);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(second);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_values_failed_original_owner_refuses_an_independent_request_copy() {
    let (vm, generator) = ready(true);
    let copy = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let request = copy.requests.front().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        request.try_snapshot(None).err(),
        Some(HeapError::AsyncGeneratorStorageLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_async_generator_value_contracts() {
        async_generator_values_snapshot_admits_the_independent_primitive_copy();
        async_generator_values_detached_request_keeps_its_buffer_after_vm_drop();
        async_generator_values_retired_snapshot_keeps_live_data_without_a_pending_count();
        async_generator_values_replacement_does_not_release_an_independent_old_copy();
        async_generator_values_neutral_publication_preserves_original_payload_ownership();
        async_generator_values_legacy_attachment_accounts_existing_primitive_data();
        async_generator_values_failed_owner_refuses_snapshot_without_changing_the_queue();
        async_generator_values_fifo_primitive_delivery_and_normal_gc_remain_intact();
        async_generator_values_bigint_and_symbol_buffers_are_retained_with_their_records();
        async_generator_values_request_copies_own_distinct_buffers_and_share_one_count();
        async_generator_values_failed_original_owner_refuses_an_independent_request_copy();
    }
}
