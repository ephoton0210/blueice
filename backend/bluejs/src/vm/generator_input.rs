// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained generator operands move into their actual execution consumers.

use super::host_scheduling::SharedSchedulingCharge;
use super::native_value_storage::NativeValue;
use super::realm_call_storage::RealmCallRoots;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/runtime_error_inputs.rs"]
mod ownership_tests;

pub(super) struct InterpreterResumeInput<'a> {
    pub(super) value: Option<Value>,
    pub(super) storage: Option<&'a mut Option<SharedSchedulingCharge>>,
}

pub(super) enum GeneratorInputKind {
    Next,
    Return,
    Throw,
}

pub(super) fn release_generator_payload(
    storage: &mut Option<SharedSchedulingCharge>,
    bytes: usize,
) {
    if let Some(storage) = storage {
        storage.release_to(bytes);
    }
}

pub(super) fn retain_generator_payload(
    storage: &mut Option<SharedSchedulingCharge>,
    bytes: usize,
) -> Result<(), RuntimeError> {
    if let Some(storage) = storage {
        if bytes > storage.bytes() {
            storage.resize(bytes)?;
        } else {
            storage.release_to(bytes);
        }
    }
    Ok(())
}

pub(super) fn generator_result_bytes(result: &Result<(Value, bool), RuntimeError>) -> usize {
    match result {
        Ok((value, _)) => host_scheduling::value_payload_bytes(value),
        Err(error) => host_scheduling::runtime_error_payload_bytes(error),
    }
}

pub(super) fn interpreter_result_bytes(result: &Result<InterpreterExit, RuntimeError>) -> usize {
    match result {
        Ok(InterpreterExit::Return(value) | InterpreterExit::Yield { value, .. }) => {
            host_scheduling::value_payload_bytes(value)
        }
        Err(error) => host_scheduling::runtime_error_payload_bytes(error),
        _ => 0,
    }
}

pub(super) fn raw_result_bytes(result: &Result<Value, RuntimeError>) -> usize {
    match result {
        Ok(value) => host_scheduling::value_payload_bytes(value),
        Err(error) => host_scheduling::runtime_error_payload_bytes(error),
    }
}

pub(super) fn scalar_completion_bytes(completion: &Completion) -> usize {
    match completion {
        Completion::Return(value) => host_scheduling::value_payload_bytes(value),
        Completion::Throw(error) => host_scheduling::runtime_error_payload_bytes(error),
        _ => 0,
    }
}

pub(super) fn copy_scalar_completion(
    completion: &Completion,
    owner: &Option<SharedSchedulingCharge>,
) -> Result<(Completion, Option<SharedSchedulingCharge>), RuntimeError> {
    let bytes = match completion {
        Completion::Throw(error) => host_scheduling::runtime_error_copy_bytes(error),
        _ => scalar_completion_bytes(completion),
    };
    let mut storage = owner
        .as_ref()
        .map(|owner| owner.reserve_peer(bytes))
        .transpose()?;
    let copy = completion.clone();
    if let Some(storage) = &mut storage {
        storage.resize(scalar_completion_bytes(&copy))?;
    }
    Ok((copy, storage))
}

impl Vm {
    pub(super) fn generator_resume_owned(
        &mut self,
        receiver: &Value,
        target: ObjectId,
        input: NativeValue,
        kind: GeneratorInputKind,
        storage: &mut Option<SharedSchedulingCharge>,
    ) -> Result<Value, RuntimeError> {
        let mut result = input.with_owned_context(self, |vm, input| {
            let (value, incoming) = input.into_retained_parts();
            *storage = incoming;
            match kind {
                GeneratorInputKind::Next => vm.generator_resume_with_input(
                    receiver,
                    Some(value),
                    Some(target),
                    None,
                    storage,
                ),
                GeneratorInputKind::Return => vm.generator_resume_with_input(
                    receiver,
                    None,
                    Some(target),
                    Some(Completion::Return(value)),
                    storage,
                ),
                GeneratorInputKind::Throw => vm.generator_resume_with_input(
                    receiver,
                    None,
                    Some(target),
                    Some(Completion::Throw(RuntimeError::Thrown(value))),
                    storage,
                ),
            }
        });
        if storage.is_some() {
            if let Err(error) = retain_generator_payload(storage, raw_result_bytes(&result)) {
                result = Err(error);
                release_generator_payload(storage, 0);
            }
        }
        result
    }

    pub(super) fn iterator_result_retained(
        &mut self,
        input: NativeValue,
        done: bool,
    ) -> Result<Value, RuntimeError> {
        input.with_owned_context(self, |vm, input| {
            let owner = input.owner_view()?;
            let roots = RealmCallRoots::new(1, owner.as_ref())?;
            vm.with_realm_call_roots(roots.clone(), |vm| {
                let prototype = vm.object_prototype;
                let result = vm.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                roots.push(&vm.heap, result)?;
                input.into_heap(vm, |heap, value| {
                    heap.set(result, "value", value)?;
                    heap.set(result, "done", Value::Bool(done))?;
                    Ok(Value::Object(result))
                })
            })
        })
    }

    pub(super) fn promise_resolve_retained(
        &mut self,
        input: NativeValue,
    ) -> Result<Value, RuntimeError> {
        input.with_owned_context(self, |vm, input| {
            if input.value().object_id().is_some() {
                // An object handle has no independent primitive allocation;
                // the outer context keeps its root and original identity.
                return vm.promise_resolve(input.into_value());
            }
            vm.global("Promise")?;
            let base = vm.stack.len();
            let result = (|| {
                let promise = vm.new_promise()?;
                vm.stack.push(Value::Object(promise));
                let (value, storage) = input.into_retained_parts();
                vm.settle_promise_with_input_charge(
                    promise,
                    PromiseStatus::Fulfilled(value),
                    storage,
                )?;
                Ok(Value::Object(promise))
            })();
            vm.stack.truncate(base);
            result
        })
    }
}
