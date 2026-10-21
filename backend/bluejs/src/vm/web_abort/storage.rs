// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Abort inputs and graph snapshots keep native credit through cancellation.

use super::*;
use crate::heap::AbortData;
use crate::vm::error_input_storage::utf16_from_utf8;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::native_value_storage::NativeValue;
use crate::vm::realm_call_storage::RealmCallRoots;

pub(super) use crate::vm::web_event_storage::EventContext as AbortContext;

pub(super) use crate::vm::web_event_storage::{push_slot, NativeEventTarget as NativeAbortTarget};

pub(super) struct NativeAbortState {
    pub state: AbortData,
    storage: Option<SharedSchedulingCharge>,
}

impl NativeAbortState {
    pub(super) fn copied(
        vm: &Vm,
        signal: ObjectId,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let source = vm.heap.abort_signal(signal)?;
        let mut storage = owner
            .map(|owner| owner.reserve_peer(source.copied_buffer_bytes()))
            .transpose()?;
        let state = source.clone();
        if let Some(storage) = &mut storage {
            storage.resize(state.owned_buffer_bytes())?;
        }
        Ok(Self { state, storage })
    }

    pub(super) fn reason(&mut self, reason: NativeValue) -> Result<(), RuntimeError> {
        reason.validate_owner()?;
        let previous = host_scheduling::value_payload_bytes(&self.state.reason);
        let (value, incoming) = reason.into_retained_parts();
        self.state.reason = value;
        if let Some(storage) = &mut self.storage {
            storage.release_to(storage.bytes() - previous);
            storage.absorb(incoming.expect("abort reason has its state account"))?;
        } else {
            assert!(incoming.is_none());
        }
        Ok(())
    }

    pub(super) fn source(&mut self, source: ObjectId) -> Result<(), RuntimeError> {
        push_slot(&mut self.state.sources, &mut self.storage, source)
    }
    pub(super) fn dependent(&mut self, dependent: ObjectId) -> Result<(), RuntimeError> {
        push_slot(&mut self.state.dependents, &mut self.storage, dependent)
    }
    pub(super) fn hook(&mut self, hook: AbortHook) -> Result<(), RuntimeError> {
        push_slot(&mut self.state.hooks, &mut self.storage, hook)
    }

    pub(super) fn take_hooks(&mut self) -> NativeAbortHooks {
        let bytes = host_scheduling::slots(&self.state.hooks);
        NativeAbortHooks {
            values: std::mem::take(&mut self.state.hooks),
            storage: self
                .storage
                .as_mut()
                .map(|storage| storage.split_off(bytes)),
        }
    }

    pub(super) fn into_heap(mut self, vm: &mut Vm, signal: ObjectId) -> Result<(), RuntimeError> {
        if let Some(storage) = &mut self.storage {
            storage.reserve_peer(0)?;
            storage.release_to(self.state.owned_buffer_bytes());
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.set_abort_signal(signal, self.state)
        })
    }
}

pub(super) struct NativeAbortHooks {
    pub values: Vec<AbortHook>,
    storage: Option<SharedSchedulingCharge>,
}

pub(super) struct NativeAbortError {
    pub error: RuntimeError,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeAbortError {
    pub(super) fn adopt(
        error: RuntimeError,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let storage = owner
            .map(|owner| owner.reserve_peer(host_scheduling::runtime_error_payload_bytes(&error)))
            .transpose()?;
        Ok(Self { error, storage })
    }
    pub(super) fn into_error(self) -> RuntimeError {
        drop(self.storage);
        self.error
    }
}
impl NativeAbortHooks {
    pub(super) fn roots(&self, heap: &Heap) -> Result<Rc<RealmCallRoots>, RuntimeError> {
        let targets = self.values.iter().filter_map(|hook| match hook {
            AbortHook::Web { target, .. } => Some(*target),
            _ => None,
        });
        let roots = RealmCallRoots::new(targets.clone().count(), self.storage.as_ref())?;
        for target in targets {
            roots.push(heap, target)?;
        }
        Ok(roots)
    }
}

pub(super) fn new_abort_event(
    vm: &mut Vm,
    owner: Option<&SharedSchedulingCharge>,
) -> Result<ObjectId, RuntimeError> {
    let name = utf16_from_utf8("abort", owner)?;
    let event = crate::vm::web_event_storage::NativeEventState::new(vm, 0, name)?;
    let prototype = vm.globals["%WebEvent%"];
    event.allocate(vm, prototype)
}

struct NativeAbortReason {
    name: JsString,
    message: JsString,
    name_storage: Option<SharedSchedulingCharge>,
    message_storage: Option<SharedSchedulingCharge>,
}

pub(super) fn default_reason(
    vm: &mut Vm,
    timeout: bool,
    owner: Option<&SharedSchedulingCharge>,
) -> Result<NativeValue, RuntimeError> {
    let name = utf16_from_utf8(
        if timeout {
            "TimeoutError"
        } else {
            "AbortError"
        },
        owner,
    )?;
    let message = utf16_from_utf8("The operation was aborted", owner)?;
    let (name, name_storage) = name.into_retained_parts();
    let (message, message_storage) = message.into_retained_parts();
    let (Value::String(name), Value::String(message)) = (name, message) else {
        unreachable!()
    };
    let mut input = NativeAbortReason {
        name,
        message,
        name_storage,
        message_storage,
    };
    let prototype = vm.globals["%WebDOMException%"];
    let object = vm.with_roots(|heap| {
        for storage in [&mut input.name_storage, &mut input.message_storage]
            .into_iter()
            .flatten()
        {
            storage.release_to(0);
        }
        heap.alloc_dom_exception(input.name, input.message, prototype)
    })?;
    NativeValue::adopt(Value::Object(object), owner)
}
