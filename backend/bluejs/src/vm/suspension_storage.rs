// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Staged buffers used by generator, async and debugger snapshots.

use super::*;
use crate::heap::GeneratorPendingCompletion;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/suspension_storage.rs"]
mod tests;

pub(super) struct SuspensionStorage {
    at: usize,
    stack: Vec<Value>,
    pending: Vec<GeneratorPendingCompletion>,
    convert_pending: bool,
    stack_taken: bool,
    pending_taken: bool,
    stack_bytes: usize,
    pending_bytes: usize,
    charge: execution_payload::ExecutionPayloadCharge,
}

impl SuspensionStorage {
    fn refresh(&mut self) {
        self.charge.resize(if self.charge.enabled() {
            self.stack_bytes.saturating_add(self.pending_bytes)
        } else {
            0
        });
    }

    pub(super) fn take_stack(&mut self, vm: &mut Vm) -> Vec<Value> {
        assert!(!self.stack_taken, "snapshot stack transfers once");
        self.stack_taken = true;
        if self.at == 0 {
            self.stack = std::mem::take(&mut vm.stack);
        } else {
            debug_assert!(vm.stack.len() - self.at <= self.stack.capacity());
            self.stack.extend(vm.stack.drain(self.at..));
        }
        self.stack_bytes = host_scheduling::slots(&self.stack)
            .saturating_add(host_scheduling::values_bytes(&self.stack));
        self.refresh();
        std::mem::take(&mut self.stack)
    }

    pub(super) fn take_pending(&mut self, vm: &mut Vm) -> Vec<GeneratorPendingCompletion> {
        assert!(
            self.convert_pending && !self.pending_taken,
            "snapshot completions transfer once"
        );
        self.pending_taken = true;
        debug_assert!(vm.pending_completions.len() <= self.pending.capacity());
        let payload = vm
            .pending_completions
            .iter()
            .fold(0usize, |bytes, completion| {
                bytes.saturating_add(host_scheduling::completion_payload_bytes(completion))
            });
        self.pending
            .extend(
                std::mem::take(&mut vm.pending_completions)
                    .into_iter()
                    .map(|completion| {
                        completion
                            .into_generator_pending()
                            .expect("only catchable completions can survive a generator yield")
                    }),
            );
        self.pending_bytes = host_scheduling::slots(&self.pending).saturating_add(payload);
        self.refresh();
        std::mem::take(&mut self.pending)
    }

    pub(super) fn into_generator(self, vm: &Vm, state: GeneratorState) -> OwnedGeneratorState {
        debug_assert!(self.stack_taken && (!self.convert_pending || self.pending_taken));
        let state = vm.own_generator_state(state);
        drop(self);
        state
    }
}

impl Vm {
    pub(super) fn prepare_suspension_storage(
        &mut self,
        at: usize,
        pending: bool,
    ) -> Result<SuspensionStorage, RuntimeError> {
        let result = self.prepare_suspension_buffers(at, pending);
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    fn prepare_suspension_buffers(
        &mut self,
        at: usize,
        pending: bool,
    ) -> Result<SuspensionStorage, RuntimeError> {
        let len = self
            .stack
            .len()
            .checked_sub(at)
            .ok_or(RuntimeError::Unsupported(
                "snapshot stack base is out of bounds",
            ))?;
        let stack_len = if at == 0 { 0 } else { len };
        let pending_len = if pending {
            self.pending_completions.len()
        } else {
            0
        };
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: stack_len
                .saturating_mul(std::mem::size_of::<Value>())
                .saturating_add(
                    pending_len.saturating_mul(std::mem::size_of::<GeneratorPendingCompletion>()),
                ),
            ..Default::default()
        })?;
        let mut stack = Vec::new();
        let mut target = Vec::new();
        if stack.try_reserve_exact(stack_len).is_err()
            || target.try_reserve_exact(pending_len).is_err()
        {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        let stack_bytes = host_scheduling::slots(&stack);
        let pending_bytes = host_scheduling::slots(&target);
        let prepared = SuspensionStorage {
            at,
            stack,
            pending: target,
            convert_pending: pending,
            stack_taken: false,
            pending_taken: false,
            stack_bytes,
            pending_bytes,
            charge: self.charge_execution_control(stack_bytes.saturating_add(pending_bytes)),
        };
        self.synchronize_scheduling_usage()?;
        Ok(prepared)
    }

    pub(super) fn capture_suspension_execution(
        &mut self,
        at: usize,
    ) -> Result<SuspendedModuleExecution, RuntimeError> {
        let storage = self.prepare_suspension_storage(at, false)?;
        let mut execution = self.capture_prepared_execution(storage);
        if let Err(error) = execution.retain_native_storage(self) {
            self.restore_captured_execution(execution);
            return Err(error);
        }
        Ok(execution)
    }

    pub(super) fn restore_captured_execution(&mut self, mut execution: SuspendedModuleExecution) {
        if !self.stack.is_empty() {
            let mut prefix = std::mem::take(&mut self.stack);
            debug_assert!(prefix.capacity() >= prefix.len() + execution.stack.len());
            prefix.append(&mut execution.stack);
            execution.stack = prefix;
        }
        self.restore_module_execution(execution);
    }

    pub(super) fn capture_prepared_execution(
        &mut self,
        mut storage: SuspensionStorage,
    ) -> SuspendedModuleExecution {
        let stack = storage.take_stack(self);
        let mut execution = self.suspend_module_execution();
        // The retained caller prefix belongs to the active VM immediately;
        // the detached suffix belongs to the continuation's scoped owner.
        self.stack = std::mem::replace(&mut execution.stack, stack);
        execution.refresh_execution_charges();
        drop(storage);
        self.reconcile_scheduling_ownership();
        execution
    }
}
