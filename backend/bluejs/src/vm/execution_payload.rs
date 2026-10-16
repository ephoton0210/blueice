// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primitive buffers and control storage owned by execution contexts.

use super::host_scheduling::{completion_payload_bytes, value_payload_bytes, values_bytes};
use super::*;
use std::cell::Cell;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/execution_control_capacity.rs"]
mod control_capacity_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/interpreter_record_ownership.rs"]
mod interpreter_record_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/call_entry_storage.rs"]
mod call_entry_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/frame_binding_admission.rs"]
mod frame_binding_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/scope_admission.rs"]
mod scope_admission_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/frame_map_admission.rs"]
mod frame_map_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/generator_storage_ownership.rs"]
mod generator_storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/environment_admission.rs"]
mod environment_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/generator_entry_admission.rs"]
mod generator_entry_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/realm_template_cache.rs"]
mod template_cache_tests;

/// These transfers displace different owners. Ordinary calls forward dynamic
/// eval environments into the callee; generator entry saves them in Rust.
pub(super) enum ControlTransfer {
    Call,
    Generator,
    Eval,
    NestedScript,
}

macro_rules! frame_control_bytes {
    ($frame:expr) => {{
        use super::host_scheduling::{map_slots, named_cells_bytes, named_slots_bytes, slots};
        let frame = $frame;
        let mut bytes = slots(&frame.stack)
            .saturating_add(slots(&frame.arguments))
            .saturating_add(slots(&frame.bindings))
            .saturating_add(binding_metadata_bytes(&frame.binding_metadata))
            .saturating_add(slots(&frame.with_objects))
            .saturating_add(scope_bytes(&frame.active_scopes, &frame.active_scope_slots))
            .saturating_add(slots(&frame.pending_completions))
            .saturating_add(slots(&frame.completion_saves))
            .saturating_add(map_slots(&frame.cells))
            .saturating_add(named_cells_bytes(&frame.dynamic_eval_bindings))
            .saturating_add(named_slots_bytes(&frame.eval_dynamic_slots))
            .saturating_add(outer_binding_bytes(&frame.dynamic_eval_outer_bindings))
            .saturating_add(named_slots_bytes(&frame.script_global_slots))
            .saturating_add(slots(&frame.variable_scope_lexicals))
            .saturating_add(
                frame
                    .active_module_name
                    .as_ref()
                    .map_or(0, String::capacity),
            );
        for name in &frame.variable_scope_lexicals {
            bytes = bytes.saturating_add(name.capacity());
        }
        bytes
    }};
}

fn binding_metadata_bytes(values: &Vec<Binding>) -> usize {
    values
        .iter()
        .fold(host_scheduling::slots(values), |bytes, binding| {
            bytes.saturating_add(binding.name.capacity())
        })
}

fn scope_bytes(scopes: &Vec<u32>, slots: &Vec<Vec<u32>>) -> usize {
    slots.iter().fold(
        host_scheduling::slots(scopes).saturating_add(host_scheduling::slots(slots)),
        |bytes, scope| bytes.saturating_add(host_scheduling::slots(scope)),
    )
}

pub(super) fn outer_binding_bytes(values: &Vec<HashMap<String, DynamicEvalBinding>>) -> usize {
    values
        .iter()
        .fold(host_scheduling::slots(values), |bytes, bindings| {
            bytes.saturating_add(host_scheduling::named_cells_bytes(bindings))
        })
}

fn pending_references<'a>(
    pending: &'a [Completion],
    saves: &'a [(Value, bool)],
) -> impl Iterator<Item = ObjectId> + 'a {
    pending
        .iter()
        .flat_map(|completion| match completion {
            Completion::Return(value)
            | Completion::Yield(value)
            | Completion::Halt(value)
            | Completion::Throw(RuntimeError::Thrown(value)) => std::slice::from_ref(value),
            Completion::TailCall(values) | Completion::TailRecur(values) => values.as_slice(),
            _ => &[],
        })
        .filter_map(Value::object_id)
        .chain(saves.iter().filter_map(|(value, _)| value.object_id()))
}

pub(super) struct ExecutionPayloadCharge {
    counter: Rc<Cell<usize>>,
    bytes: usize,
    enabled: bool,
}

pub(super) struct CallEntryCharge {
    payload: ExecutionPayloadCharge,
    control: ExecutionPayloadCharge,
}

impl CallEntryCharge {
    /// Forwarded metadata can reserve additional slots before VM adoption.
    pub(super) fn add_control_storage(&mut self, bytes: usize) {
        if self.control.enabled() {
            self.control
                .resize(self.control.bytes().saturating_add(bytes));
        }
    }

    pub(super) fn refresh(
        &mut self,
        callee: &Value,
        receiver: &Value,
        args: &Vec<Value>,
        metadata: usize,
    ) {
        let payload = values_bytes(args)
            .saturating_add(value_payload_bytes(callee))
            .saturating_add(value_payload_bytes(receiver));
        self.payload
            .resize(if self.payload.enabled() { payload } else { 0 });
        let control = host_scheduling::slots(args).saturating_add(metadata);
        self.control
            .resize(if self.control.enabled() { control } else { 0 });
    }
}

impl ExecutionPayloadCharge {
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn resize(&mut self, bytes: usize) {
        self.counter.set(self.counter.get() - self.bytes + bytes);
        self.bytes = bytes;
    }
}

impl SuspendedModuleExecution {
    pub(super) fn payload_bytes(&self) -> usize {
        let mut bytes = values_bytes(&self.stack)
            .saturating_add(values_bytes(&self.arguments))
            .saturating_add(values_bytes(&self.with_objects));
        for value in self.bindings.iter().flatten().chain([
            &self.this,
            &self.callee,
            &self.new_target,
            &self.completion,
        ]) {
            bytes = bytes.saturating_add(value_payload_bytes(value));
        }
        for completion in &self.pending_completions {
            bytes = bytes.saturating_add(completion_payload_bytes(completion));
        }
        for (value, _) in &self.completion_saves {
            bytes = bytes.saturating_add(value_payload_bytes(value));
        }
        bytes
    }

    pub(super) fn control_bytes(&self) -> usize {
        frame_control_bytes!(self)
    }

    /// Stack splitting and debugger edits change both the allocation and its
    /// values. A returned primitive can enter a still-paused parent frame.
    pub(super) fn refresh_execution_charges(&mut self) {
        let payload = self.payload_bytes();
        self.payload_charge.resize(if self.payload_charge.enabled {
            payload
        } else {
            0
        });
        let bytes = self.control_bytes();
        self.control_charge.resize(if self.control_charge.enabled {
            bytes
        } else {
            0
        });
    }
}

impl Drop for ExecutionPayloadCharge {
    fn drop(&mut self) {
        self.counter.set(self.counter.get() - self.bytes);
    }
}

impl Vm {
    pub(super) fn call_entry_charge(
        &self,
        callee: &Value,
        receiver: &Value,
        args: &Vec<Value>,
    ) -> CallEntryCharge {
        CallEntryCharge {
            payload: self.charge_execution_payload(
                values_bytes(args)
                    .saturating_add(value_payload_bytes(callee))
                    .saturating_add(value_payload_bytes(receiver)),
            ),
            control: self.charge_execution_control(host_scheduling::slots(args)),
        }
    }
    /// Finished page jobs return their frame allocations. Persistent template
    /// identities and module association remain owned by the realm.
    pub(super) fn release_finished_execution_control(&mut self) {
        if self.host_scheduling.is_none() {
            return;
        }
        self.stack = Vec::new();
        self.bindings = Vec::new();
        self.binding_metadata = Vec::new();
        self.arguments = Vec::new();
        self.with_objects = Vec::new();
        self.active_scopes = Vec::new();
        self.active_scope_slots = Vec::new();
        self.pending_completions = Vec::new();
        self.completion_saves = Vec::new();
        self.call_stack = Vec::new();
        self.async_frame_roots = Vec::new();
        self.cells = HashMap::new();
        self.dynamic_eval_bindings = HashMap::new();
        self.dynamic_eval_outer_bindings = Vec::new();
        self.eval_dynamic_slots = HashMap::new();
        self.script_global_slots = HashMap::new();
        self.variable_scope_lexicals = Vec::new();
    }

    pub(super) fn charge_execution_control(&self, bytes: usize) -> ExecutionPayloadCharge {
        let bytes = if self.host_scheduling.is_some() {
            bytes
        } else {
            0
        };
        let counter = self.displaced_execution_control_bytes.clone();
        counter.set(counter.get().saturating_add(bytes));
        ExecutionPayloadCharge {
            counter,
            bytes,
            enabled: self.host_scheduling.is_some(),
        }
    }

    pub(super) fn current_execution_control_bytes(&self) -> usize {
        frame_control_bytes!(self)
    }

    pub(super) fn displaced_control_bytes(&self, transfer: ControlTransfer) -> usize {
        use host_scheduling::{map_slots, named_cells_bytes, named_slots_bytes, slots};
        let base = slots(&self.bindings)
            .saturating_add(binding_metadata_bytes(&self.binding_metadata))
            .saturating_add(map_slots(&self.cells))
            .saturating_add(named_slots_bytes(&self.script_global_slots))
            .saturating_add(scope_bytes(&self.active_scopes, &self.active_scope_slots));
        let base = if matches!(transfer, ControlTransfer::NestedScript) {
            base
        } else {
            base.saturating_add(named_slots_bytes(&self.eval_dynamic_slots))
        };
        if matches!(transfer, ControlTransfer::Eval) {
            return base;
        }
        let base = base
            .saturating_add(slots(&self.arguments))
            .saturating_add(slots(&self.with_objects));
        if matches!(transfer, ControlTransfer::NestedScript) {
            return base;
        }
        let mut bytes = base
            .saturating_add(slots(&self.pending_completions))
            .saturating_add(slots(&self.completion_saves))
            .saturating_add(slots(&self.variable_scope_lexicals));
        for name in &self.variable_scope_lexicals {
            bytes = bytes.saturating_add(name.capacity());
        }
        if matches!(transfer, ControlTransfer::Generator) {
            bytes = bytes
                .saturating_add(named_cells_bytes(&self.dynamic_eval_bindings))
                .saturating_add(outer_binding_bytes(&self.dynamic_eval_outer_bindings));
        }
        bytes
    }
    /// Normal dispatch moves primitive buffers to its Rust argument owner.
    /// Exact debugger continuations retain their original operand snapshots.
    pub(super) fn take_call_operand(&mut self, index: usize) -> Value {
        if self.debugger_nested_pause_request.is_some()
            || matches!(self.stack[index], Value::Object(_))
        {
            self.stack[index].clone()
        } else {
            std::mem::replace(&mut self.stack[index], Value::Undefined)
        }
    }
    pub(super) fn charge_execution_payload(&self, bytes: usize) -> ExecutionPayloadCharge {
        let bytes = if self.host_scheduling.is_some() {
            bytes
        } else {
            0
        };
        let counter = self.displaced_execution_bytes.clone();
        counter.set(counter.get().saturating_add(bytes));
        ExecutionPayloadCharge {
            counter,
            bytes,
            enabled: self.host_scheduling.is_some(),
        }
    }

    /// Fields replaced by ordinary calls and generator entry. Operand-stack
    /// copies stay in the VM; this charge follows the displaced originals.
    pub(super) fn caller_execution_payload_bytes(&self) -> usize {
        self.bindings
            .iter()
            .flatten()
            .fold(
                values_bytes(&self.arguments).saturating_add(values_bytes(&self.with_objects)),
                |bytes, value| bytes.saturating_add(value_payload_bytes(value)),
            )
            .saturating_add(value_payload_bytes(&self.this))
            .saturating_add(value_payload_bytes(&self.callee))
            .saturating_add(value_payload_bytes(&self.completion))
            .saturating_add(value_payload_bytes(&self.new_target))
    }

    pub(super) fn pending_execution_references(&self) -> Vec<ObjectId> {
        pending_references(&self.pending_completions, &self.completion_saves).collect()
    }

    pub(super) fn pending_execution_reference_count(&self) -> usize {
        pending_references(&self.pending_completions, &self.completion_saves).count()
    }

    pub(super) fn push_pending_execution_roots(&mut self) {
        self.stack.extend(
            pending_references(&self.pending_completions, &self.completion_saves)
                .map(Value::Object),
        );
    }

    pub(super) fn pending_execution_payload_bytes(&self) -> usize {
        self.pending_completions
            .iter()
            .fold(0usize, |bytes, completion| {
                bytes.saturating_add(completion_payload_bytes(completion))
            })
            .saturating_add(
                self.completion_saves
                    .iter()
                    .fold(0usize, |bytes, (value, _)| {
                        bytes.saturating_add(value_payload_bytes(value))
                    }),
            )
    }

    pub(super) fn current_execution_payload_bytes(&self) -> usize {
        self.caller_execution_payload_bytes()
            .saturating_add(values_bytes(&self.stack))
            .saturating_add(self.pending_execution_payload_bytes())
    }

    pub(super) fn owned_execution_payload_bytes(&self) -> usize {
        if self.host_scheduling.is_none() {
            return 0;
        }
        self.current_execution_payload_bytes()
            .saturating_add(self.current_execution_control_bytes())
            .saturating_add(host_scheduling::slots(&self.call_stack))
            .saturating_add(host_scheduling::slots(&self.async_frame_roots))
            .saturating_add(self.displaced_execution_control_bytes.get())
            .saturating_add(self.displaced_execution_bytes.get())
            .saturating_add(self.pending_tail_call.as_ref().map_or(0, |call| {
                host_scheduling::slots(call).saturating_add(values_bytes(call))
            }))
    }

    /// Check at interpreter/native safepoints, including operations which
    /// grow primitive values without allocating a managed object. An
    /// unchanged payload avoids rescanning all Promise and task owners.
    pub(super) fn synchronize_execution_payload(&mut self) -> Result<(), RuntimeError> {
        self.check_scheduling_failure()?;
        if self.host_scheduling.is_none() {
            return Ok(());
        }
        let bytes = self.owned_execution_payload_bytes();
        if bytes != self.last_execution_payload_bytes {
            self.synchronize_scheduling_usage()?;
            self.last_execution_payload_bytes = bytes;
        }
        Ok(())
    }
}
