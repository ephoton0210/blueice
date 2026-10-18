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

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_referrer_ownership.rs"]
mod module_referrer_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_context_ownership.rs"]
mod module_context_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_source_ownership.rs"]
mod module_source_tests;

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

pub(super) fn pending_references<'a>(
    pending: &'a [Completion],
    saves: &'a [(Value, bool)],
) -> impl Iterator<Item = ObjectId> + Clone + 'a {
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
    native: Option<host_scheduling::SharedSchedulingCharge>,
}

pub(super) struct CallEntryCharge {
    payload: ExecutionPayloadCharge,
    control: ExecutionPayloadCharge,
}

impl CallEntryCharge {
    pub(super) fn replace_argument_control(
        &mut self,
        owner: Option<host_scheduling::SharedSchedulingCharge>,
    ) {
        assert!(self.control.native.is_some() || !self.control.enabled);
        self.control.bytes = owner
            .as_ref()
            .map_or(0, host_scheduling::SharedSchedulingCharge::bytes);
        self.control.enabled = owner.is_some();
        self.control.native = owner;
    }

    pub(super) fn append_argument_payload(&mut self, value: &Value) -> Result<(), RuntimeError> {
        if self.payload.enabled {
            self.payload.resize_entry(
                self.payload
                    .bytes
                    .saturating_add(value_payload_bytes(value)),
            )?;
        }
        Ok(())
    }

    /// A pure heap consumer admits this same buffer immediately after release,
    /// without a guest/GC boundary between the two ownership views.
    pub(super) fn release_argument_payload(&mut self, bytes: usize) {
        if let Some(owner) = &mut self.payload.native {
            let retained = self.payload.bytes - bytes;
            owner.release_to(retained);
            self.payload.bytes = retained;
        }
    }

    /// Forwarded metadata can reserve additional slots before VM adoption.
    pub(super) fn add_control_storage(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if self.control.enabled() {
            self.control
                .resize_entry(self.control.bytes().saturating_add(bytes))?;
        }
        Ok(())
    }

    pub(super) fn refresh(
        &mut self,
        callee: &Value,
        receiver: &Value,
        args: &Vec<Value>,
        metadata: usize,
    ) -> Result<(), RuntimeError> {
        let payload = values_bytes(args)
            .saturating_add(value_payload_bytes(callee))
            .saturating_add(value_payload_bytes(receiver));
        self.payload
            .resize_entry(if self.payload.enabled() { payload } else { 0 })?;
        let control = host_scheduling::slots(args).saturating_add(metadata);
        self.control
            .resize_entry(if self.control.enabled() { control } else { 0 })
    }

    /// Detach the original entry counters before metadata getters reenter the
    /// caller. A neutral caller can borrow the active document's native owner.
    pub(super) fn retain_native(
        &mut self,
        vm: &mut Vm,
        callee: &Value,
        receiver: &Value,
        args: &Vec<Value>,
        hint: Option<&host_scheduling::SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        self.refresh(callee, receiver, args, 0)?;
        let payload = values_bytes(args)
            .saturating_add(value_payload_bytes(callee))
            .saturating_add(value_payload_bytes(receiver));
        self.payload.retain_entry(vm, payload, hint)?;
        self.control
            .retain_entry(vm, host_scheduling::slots(args), hint)
    }

    /// The destination assumes this same allocation when its frame adopts the
    /// input. Separate documents admit their replacement lease before release.
    pub(super) fn retarget_native(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        for charge in [&mut self.payload, &mut self.control] {
            if let Some(owner) = &charge.native {
                owner.reserve_peer(0)?;
                if let Some(scope) = scope {
                    if !owner.belongs_to(scope) {
                        charge.native = Some(scope.shared_charge(charge.bytes)?);
                    }
                }
            }
        }
        Ok(())
    }

    /// A zero-byte view survives frame adoption so a native callback's terminal
    /// marker is still observed when the complete invocation returns.
    pub(super) fn native_owner(
        &self,
    ) -> Result<Option<host_scheduling::SharedSchedulingCharge>, RuntimeError> {
        self.payload
            .native
            .as_ref()
            .or(self.control.native.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
}

impl ExecutionPayloadCharge {
    fn resize_entry(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if let Some(owner) = &mut self.native {
            owner.resize(bytes)?;
            self.bytes = bytes;
        } else {
            self.resize(bytes);
        }
        Ok(())
    }

    fn retain_entry(
        &mut self,
        vm: &mut Vm,
        bytes: usize,
        hint: Option<&host_scheduling::SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        if self.native.is_some() {
            return self.resize_entry(bytes);
        }
        if vm.host_scheduling.is_some() {
            return self.retain_native(vm, bytes);
        }
        if let Some(owner) = hint {
            debug_assert!(!self.enabled);
            self.native = Some(owner.reserve_peer(bytes)?);
            self.bytes = bytes;
            self.enabled = true;
        }
        Ok(())
    }

    pub(super) fn is_retained(&self) -> bool {
        self.native.is_some()
    }

    pub(super) fn validate_retained(
        &self,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        if let Some(owner) = &self.native {
            owner.reserve_peer(0)?;
            if let Some(scope) = scope {
                if !owner.belongs_to(scope) {
                    return Err(scope.fail());
                }
            }
        }
        Ok(())
    }

    /// Move an existing published counter charge to its durable data owner.
    /// A previously neutral snapshot instead admits its retained allocation.
    pub(super) fn retain_native(&mut self, vm: &mut Vm, bytes: usize) -> Result<(), RuntimeError> {
        if vm.host_scheduling.is_none() {
            return Ok(());
        }
        self.validate_retained(vm.host_scheduling.as_ref())?;
        vm.check_scheduling_failure()?;
        if self.native.is_some() {
            self.resize(bytes);
            vm.reconcile_scheduling_ownership();
            return vm.check_scheduling_failure();
        }
        let native = if self.enabled {
            self.resize(bytes);
            vm.reconcile_scheduling_ownership();
            vm.check_scheduling_failure()?;
            let native = vm
                .host_scheduling
                .as_mut()
                .unwrap()
                .detach_owned_payload(bytes);
            self.counter.set(self.counter.get() - self.bytes);
            native
        } else {
            vm.host_scheduling.as_ref().unwrap().shared_charge(bytes)?
        };
        self.bytes = bytes;
        self.enabled = true;
        self.native = Some(native);
        Ok(())
    }

    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn resize(&mut self, bytes: usize) {
        if let Some(owner) = &mut self.native {
            owner.reconcile_owned_bytes(bytes);
        } else {
            self.counter.set(self.counter.get() - self.bytes + bytes);
        }
        self.bytes = bytes;
    }
}

impl SuspendedModuleExecution {
    pub(super) fn retain_native_storage(&mut self, vm: &mut Vm) -> Result<(), RuntimeError> {
        let payload = self.payload_bytes();
        let control = self.control_bytes();
        self.payload_charge.retain_native(vm, payload)?;
        self.control_charge.retain_native(vm, control)?;
        self.disposables.retain_native_storage(vm)?;
        self.dispose_marks.retain_native_storage(vm)
    }

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
        if self.native.is_none() {
            self.counter.set(self.counter.get() - self.bytes);
        }
    }
}

impl Vm {
    pub(super) fn charge_module_cells(&self, bytes: usize) -> ExecutionPayloadCharge {
        let enabled = self.host_scheduling.is_some();
        let bytes = if enabled { bytes } else { 0 };
        let counter = self.linked_module_cell_bytes.clone();
        counter.set(counter.get().saturating_add(bytes));
        ExecutionPayloadCharge {
            counter,
            bytes,
            enabled,
            native: None,
        }
    }
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
            native: None,
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
            native: None,
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
            .saturating_add(self.gc_trace_cache.retained_bytes())
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
