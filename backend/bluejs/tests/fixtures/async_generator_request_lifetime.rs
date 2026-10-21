// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed async-generator request counts across snapshots, completion and GC.

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

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_snapshot_keeps_one_count_after_vm_drop() {
    let (vm, generator) = ready(true);
    let view = observer(&vm);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    assert_eq!(snapshot.requests.len(), 1);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_request_clones_share_one_count() {
    let (vm, generator) = ready(true);
    let view = observer(&vm);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let first = snapshot
        .requests
        .front()
        .unwrap()
        .try_snapshot(None)
        .unwrap();
    let second = first.try_snapshot(None).unwrap();
    drop(snapshot);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    let bytes = view.host_scheduling_snapshot().payload_bytes;
    assert!(bytes >= HeapReactionLease::allocation_bytes());
    drop(first);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(second);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_legacy_activation_keeps_snapshot_count() {
    let (mut vm, generator) = ready(false);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_collection_keeps_only_the_held_request_count() {
    let (mut vm, generator) = ready(true);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let view = observer(&vm);
    execute(&mut vm, "gate=null;iterator=null;first=null;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(generator));
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_fifo_completion_retires_even_a_held_snapshot() {
    let (mut vm, generator) = ready(true);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    execute(&mut vm, "var second=iterator.next();var seen=[];first.then(v=>seen.push(v.value,v.done));second.then(v=>seen.push(v.value,v.done));gate.resolve(0);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "seen.join(',');"),
        Value::String("7,false,8,true".into())
    );
    assert!(vm
        .heap
        .async_generator_control(generator)
        .unwrap()
        .unwrap()
        .requests
        .is_empty());
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(
        vm.heap.set_async_generator_control(generator, snapshot),
        Err(HeapError::InvalidConfig)
    );
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_field_credit_transfers_without_a_duplicate_peak() {
    let (mut vm, generator) = ready(false);
    let snapshot = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let mut peer = Vm::default();
    peer.use_bounded_host_microtask_checkpoints().unwrap();
    let before = peer.host_scheduling_snapshot().payload_bytes;
    let scope = peer.host_scheduling.as_ref().unwrap().child();
    let budget = scope.heap_native_budget();
    vm.host_scheduling = Some(scope.child());
    let own = vm.own_scheduling_usage();
    vm.host_scheduling
        .as_mut()
        .unwrap()
        .check(own, HostSchedulingUsage::default())
        .unwrap();
    let (result, peak) =
        scope.measure_payload_peak(|| vm.retain_async_generator_request_counts(&budget));
    result.unwrap();
    assert_eq!(peak, 0);
    drop(vm);
    assert_eq!(peer.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(
        peer.host_scheduling_snapshot().payload_bytes,
        before + HeapReactionLease::allocation_bytes()
    );
    drop(snapshot);
    assert_eq!(peer.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_failed_owner_refuses_before_promise_publication() {
    let (mut vm, generator) = ready(true);
    let view = observer(&vm);
    let promises = vm.promises.len();
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    drop(vm.host_scheduling.take());
    assert_eq!(
        vm.async_generator_request(
            &Value::Object(generator),
            Value::Undefined,
            NativeFunction::AsyncGeneratorNext
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promises.len(), promises);
    assert_eq!(
        vm.heap
            .async_generator_control_ref(generator)
            .unwrap()
            .unwrap()
            .requests
            .len(),
        1
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_empty_control_keeps_original_document_identity() {
    let (mut source, generator) = ready(true);
    execute(&mut source, "var second=iterator.next();gate.resolve(0);0;");
    source.run_host_microtask_checkpoint().unwrap();
    let control = source
        .heap
        .async_generator_control(generator)
        .unwrap()
        .unwrap();
    assert!(control.requests.is_empty());
    let mut target = Vm::default();
    let other = target
        .heap
        .alloc_generator(GeneratorState::Done, target.object_prototype)
        .unwrap();
    target.heap.enable_async_generator(other).unwrap();
    target
        .heap
        .set_async_generator_control(other, control)
        .unwrap();
    assert_eq!(
        target.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_fifo_refusal_preserves_both_pending_counts() {
    let (mut vm, generator) = ready(true);
    let mut control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let first = control.requests.front().unwrap().target;
    let second = vm.new_promise().unwrap();
    control
        .push_request(AsyncGeneratorRequest {
            value_storage: None,
            id: 0,
            completion: AsyncGeneratorCompletion::Next(Value::Undefined),
            target: second,
            origin: crate::heap::AsyncGeneratorOrigin::unowned(ThrowOrigin::default()),
            scheduling: HeapReactionLease::unowned(),
        })
        .unwrap();
    vm.with_roots(|heap| heap.set_async_generator_control(generator, control))
        .unwrap();
    let count = vm.host_scheduling_snapshot().pending_reactions;
    assert_eq!(count, 3);
    assert_eq!(
        vm.complete_async_generator_request(
            generator,
            first,
            PromiseStatus::Fulfilled(Value::Undefined)
        ),
        Err(RuntimeError::Unsupported(
            "async generator queue identifiers are not FIFO"
        ))
    );
    assert_eq!(
        vm.heap
            .async_generator_control(generator)
            .unwrap()
            .unwrap()
            .requests
            .len(),
        2
    );
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, count);
}

#[cfg_attr(test, test)]
fn async_generator_request_lifetime_neutral_heap_keeps_distinct_generator_accounts() {
    let (mut vm, first) = ready(false);
    let second = execute(&mut vm, "var other=(async function*(){await gate.promise;yield 9;})();var pending=other.next();other;").object_id().unwrap();
    let first_snapshot = vm.heap.async_generator_control(first).unwrap().unwrap();
    let second_snapshot = vm.heap.async_generator_control(second).unwrap().unwrap();
    let mut first_owner = Vm::default();
    first_owner
        .use_bounded_host_microtask_checkpoints()
        .unwrap();
    let mut second_owner = Vm::default();
    second_owner
        .use_bounded_host_microtask_checkpoints()
        .unwrap();
    let first_budget = first_owner
        .host_scheduling
        .as_ref()
        .unwrap()
        .heap_native_budget();
    let second_budget = second_owner
        .host_scheduling
        .as_ref()
        .unwrap()
        .heap_native_budget();
    vm.retain_async_generator_request_counts_for(&first_budget, Some(first))
        .unwrap();
    vm.retain_async_generator_request_counts_for(&second_budget, Some(second))
        .unwrap();
    assert_eq!(first_owner.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(second_owner.host_scheduling_snapshot().pending_reactions, 1);
    drop(vm);
    drop(first_snapshot);
    assert_eq!(first_owner.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(second_owner.host_scheduling_snapshot().pending_reactions, 1);
    drop(second_snapshot);
    assert_eq!(second_owner.host_scheduling_snapshot().pending_reactions, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_async_generator_request_lifetime_contracts() {
        async_generator_request_lifetime_neutral_heap_keeps_distinct_generator_accounts();
        async_generator_request_lifetime_fifo_refusal_preserves_both_pending_counts();
        async_generator_request_lifetime_field_credit_transfers_without_a_duplicate_peak();
        async_generator_request_lifetime_failed_owner_refuses_before_promise_publication();
        async_generator_request_lifetime_empty_control_keeps_original_document_identity();
        async_generator_request_lifetime_snapshot_keeps_one_count_after_vm_drop();
        async_generator_request_lifetime_request_clones_share_one_count();
        async_generator_request_lifetime_legacy_activation_keeps_snapshot_count();
        async_generator_request_lifetime_collection_keeps_only_the_held_request_count();
        async_generator_request_lifetime_fifo_completion_retires_even_a_held_snapshot();
    }
}
