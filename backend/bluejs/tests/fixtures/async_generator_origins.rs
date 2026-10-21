// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed provenance lifetime and native input handoff without pressure probes.

use super::*;
use std::cell::Cell;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn origin(filename: &str) -> ThrowOrigin {
    ThrowOrigin {
        source: Some(crate::diagnostics::DiagnosticSourceLocation {
            filename: filename.into(),
            line: 7,
            column: 3,
        }),
        ..Default::default()
    }
}
fn ready() -> (Vm, ObjectId) {
    ready_with_scope(true)
}
fn ready_with_scope(scoped: bool) -> (Vm, ObjectId) {
    let mut vm = Vm::default();
    if scoped {
        vm.use_bounded_host_microtask_checkpoints().unwrap();
    }
    execute(&mut vm, "var gate=Promise.withResolvers();var iterator=(async function*(){await gate.promise;yield 7;})();0;");
    let generator = vm
        .lookup_global_name("iterator")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    vm.with_rejection_origin(Some(origin("memory:///async-origin.js")), |vm| {
        vm.async_generator_request(
            &Value::Object(generator),
            Value::Undefined,
            NativeFunction::AsyncGeneratorNext,
        )
    })
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
fn async_generator_origins_retained_provenance_keeps_its_allocation_after_vm_drop() {
    let (vm, generator) = ready();
    let view = observer(&vm);
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let held = control.requests.front().unwrap().origin.clone();
    let bytes = std::mem::size_of_val(held.as_ref())
        + 2 * std::mem::size_of::<usize>()
        + held.payload_bytes();
    drop(control);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(held.source.as_ref().unwrap().line, 7);
    drop(held);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_origins_provenance_does_not_root_a_collected_generator() {
    let (mut vm, generator) = ready();
    let view = observer(&vm);
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let held = control.requests.front().unwrap().origin.clone();
    let bytes = std::mem::size_of_val(held.as_ref())
        + 2 * std::mem::size_of::<usize>()
        + held.payload_bytes();
    drop(control);
    execute(&mut vm, "gate=null;iterator=null;first=null;0;");
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(generator));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(held);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_origins_replacement_leaves_held_old_provenance_owned() {
    let (mut vm, generator) = ready();
    let view = observer(&vm);
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let held = control.requests.front().unwrap().origin.clone();
    let bytes = std::mem::size_of_val(held.as_ref())
        + 2 * std::mem::size_of::<usize>()
        + held.payload_bytes();
    drop(control);
    vm.replace_async_generator_request(
        generator,
        AsyncGeneratorCompletion::Next(Value::Undefined),
        Some(origin("memory:///replacement.js")),
    )
    .unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(
        held.source.as_ref().unwrap().filename,
        "memory:///async-origin.js"
    );
    drop(held);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_origins_neutral_publication_uses_the_original_account() {
    let (mut vm, generator) = ready();
    let view = observer(&vm);
    drop(vm.host_scheduling.take());
    vm.with_rejection_origin(Some(origin("memory:///borrowed-origin.js")), |vm| {
        vm.async_generator_request(
            &Value::Object(generator),
            Value::Undefined,
            NativeFunction::AsyncGeneratorNext,
        )
    })
    .unwrap();
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let held = control.requests.back().unwrap().origin.clone();
    let bytes = std::mem::size_of_val(held.as_ref())
        + 2 * std::mem::size_of::<usize>()
        + held.payload_bytes();
    drop(control);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(held);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_origins_failed_owner_refuses_replacement_before_publication() {
    let (mut vm, generator) = ready();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.replace_async_generator_request(
            generator,
            AsyncGeneratorCompletion::Next(Value::Undefined),
            Some(origin("memory:///refused.js")),
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    let retained = vm
        .heap
        .async_generator_control_ref(generator)
        .unwrap()
        .unwrap();
    assert_eq!(
        retained
            .requests
            .front()
            .unwrap()
            .origin
            .source
            .as_ref()
            .unwrap()
            .filename,
        "memory:///async-origin.js"
    );
}

#[cfg_attr(test, test)]
fn async_generator_origins_neutral_override_keeps_incoming_and_displaced_credit() {
    let (source, _) = ready();
    let before = source.host_scheduling_snapshot().payload_bytes;
    let scope = source.host_scheduling.as_ref().unwrap();
    let first = origin("memory:///first.js");
    let first_bytes = first.payload_bytes();
    let first_charge = scope.shared_charge(first_bytes).unwrap();
    let mut neutral = Vm::default();
    neutral
        .with_leased_rejection_origin(Some(first), Some(first_charge), |vm| {
            assert_eq!(
                source.host_scheduling_snapshot().payload_bytes,
                before + first_bytes
            );
            let second = origin("memory:///second.js");
            let second_bytes = second.payload_bytes();
            let second_charge = scope.shared_charge(second_bytes).unwrap();
            vm.with_leased_rejection_origin(Some(second), Some(second_charge), |_| {
                assert_eq!(
                    source.host_scheduling_snapshot().payload_bytes,
                    before + first_bytes + second_bytes
                );
                Ok(())
            })?;
            assert_eq!(
                source.host_scheduling_snapshot().payload_bytes,
                before + first_bytes
            );
            assert_eq!(
                vm.current_rejection_origin().source.unwrap().filename,
                "memory:///first.js"
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(source.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn async_generator_origins_failed_empty_input_owner_refuses_callback() {
    let (source, _) = ready();
    let scope = source.host_scheduling.as_ref().unwrap();
    let charge = scope.shared_charge(0).unwrap();
    let _ = scope.fail();
    let called = Cell::new(false);
    let mut neutral = Vm::default();
    assert_eq!(
        neutral.with_leased_rejection_origin(Some(ThrowOrigin::default()), Some(charge), |_| {
            called.set(true);
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!called.get());
}

#[cfg_attr(test, test)]
fn async_generator_origins_foreign_leased_input_cannot_rebind_to_the_current_document() {
    let (source, _) = ready();
    let before = source.host_scheduling_snapshot().payload_bytes;
    let input = origin("memory:///foreign.js");
    let charge = source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(input.payload_bytes())
        .unwrap();
    let (mut target, _) = ready();
    let called = Cell::new(false);
    assert_eq!(
        target.with_leased_rejection_origin(Some(input), Some(charge), |_| {
            called.set(true);
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!called.get());
    assert_eq!(source.host_scheduling_snapshot().payload_bytes, before);
    assert!(source.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn async_generator_origins_scoped_handoff_has_no_duplicate_payload_peak() {
    let (mut vm, _) = ready();
    vm.synchronize_scheduling_usage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let input = origin("memory:///handoff.js");
    let bytes = input.payload_bytes();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let charge = scope.shared_charge(bytes).unwrap();
    let (result, peak) = scope.measure_payload_peak(|| {
        vm.with_leased_rejection_origin(Some(input), Some(charge), |vm| {
            vm.synchronize_scheduling_usage()?;
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
            Ok(())
        })
    });
    result.unwrap();
    assert_eq!(peak, 0);
    vm.synchronize_scheduling_usage().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn async_generator_origins_legacy_attachment_reaches_already_held_provenance() {
    let (mut vm, generator) = ready_with_scope(false);
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let held = control.requests.front().unwrap().origin.clone();
    let bytes = std::mem::size_of_val(held.as_ref())
        + 2 * std::mem::size_of::<usize>()
        + held.payload_bytes();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    drop(control);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(held);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_origins_prepared_copy_keeps_only_its_filename_after_vm_drop() {
    let (vm, generator) = ready();
    let view = observer(&vm);
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let held = control.requests.front().unwrap().origin.clone();
    let (copy, storage) = held.copied_value().unwrap();
    let bytes = copy.payload_bytes();
    assert!(bytes > 0);
    drop(held);
    drop(control);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(
        copy.source.as_ref().unwrap().filename,
        "memory:///async-origin.js"
    );
    drop(copy);
    drop(storage);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_origins_failed_original_owner_refuses_an_independent_copy() {
    let (vm, generator) = ready();
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let held = control.requests.front().unwrap().origin.clone();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        held.copied_value().err(),
        Some(HeapError::AsyncGeneratorStorageLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn async_generator_origins_callback_failure_restores_context_and_keeps_terminal_state() {
    let (source, _) = ready();
    let scope = source.host_scheduling.as_ref().unwrap();
    let charge = scope.shared_charge(0).unwrap();
    let mut neutral = Vm::default();
    assert_eq!(
        neutral.with_leased_rejection_origin(Some(ThrowOrigin::default()), Some(charge), |_| {
            let _ = scope.fail();
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.promise_origin_override.is_none());
    assert!(neutral.promise_origin_override_storage.is_none());
}

#[cfg_attr(test, test)]
fn async_generator_origins_empty_override_identity_prevents_foreign_policy_attachment() {
    let (source, _) = ready();
    let charge = source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .unwrap();
    let mut neutral = Vm::default();
    assert_eq!(
        neutral.with_leased_rejection_origin(Some(ThrowOrigin::default()), Some(charge), |vm| {
            vm.use_bounded_host_microtask_checkpoints()
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.check_scheduling_failure().is_ok());
    assert!(neutral.promise_origin_override.is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_async_generator_origin_contracts() {
        async_generator_origins_retained_provenance_keeps_its_allocation_after_vm_drop();
        async_generator_origins_provenance_does_not_root_a_collected_generator();
        async_generator_origins_replacement_leaves_held_old_provenance_owned();
        async_generator_origins_neutral_publication_uses_the_original_account();
        async_generator_origins_failed_owner_refuses_replacement_before_publication();
        async_generator_origins_neutral_override_keeps_incoming_and_displaced_credit();
        async_generator_origins_failed_empty_input_owner_refuses_callback();
        async_generator_origins_foreign_leased_input_cannot_rebind_to_the_current_document();
        async_generator_origins_scoped_handoff_has_no_duplicate_payload_peak();
        async_generator_origins_legacy_attachment_reaches_already_held_provenance();
        async_generator_origins_prepared_copy_keeps_only_its_filename_after_vm_drop();
        async_generator_origins_failed_original_owner_refuses_an_independent_copy();
        async_generator_origins_callback_failure_restores_context_and_keeps_terminal_state();
        async_generator_origins_empty_override_identity_prevents_foreign_policy_attachment();
    }
}
