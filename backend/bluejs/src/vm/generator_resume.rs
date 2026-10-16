// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admit resume buffers and incoming ownership before extracting saved state.

use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/generator_resume_admission.rs"]
mod generator_resume_tests;

enum PreparedModule {
    Referrer(String),
    Caller(Option<String>),
}

pub(super) struct PreparedGeneratorResume {
    completions: Vec<Completion>,
    module: Option<PreparedModule>,
    charge: execution_payload::ExecutionPayloadCharge,
}

impl PreparedGeneratorResume {
    /// The source buffers leave managed ownership before active fields adopt
    /// them. Keep their full allocation, including payloads, leased in between.
    pub(super) fn retain_state(&mut self, state: &GeneratorState) {
        if self.charge.enabled() {
            self.charge.resize(
                self.charge
                    .bytes()
                    .saturating_add(state.owned_buffer_bytes()),
            );
        }
    }

    pub(super) fn take_completions(
        &mut self,
        pending: Vec<crate::heap::GeneratorPendingCompletion>,
    ) -> Vec<Completion> {
        debug_assert!(pending.len() <= self.completions.capacity());
        self.completions
            .extend(pending.into_iter().map(Completion::from_generator_pending));
        std::mem::take(&mut self.completions)
    }

    pub(super) fn install_module(&mut self, vm: &mut Vm) -> Option<String> {
        match self
            .module
            .take()
            .expect("a resumable state prepared its module association")
        {
            PreparedModule::Referrer(module) => vm.active_module_name.replace(module),
            PreparedModule::Caller(module) => module,
        }
    }
}

impl Vm {
    pub(super) fn prepare_generator_resume(
        &mut self,
        generator: ObjectId,
    ) -> Result<PreparedGeneratorResume, RuntimeError> {
        let result = self.prepare_generator_resume_storage(generator);
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    fn prepare_generator_resume_storage(
        &mut self,
        generator: ObjectId,
    ) -> Result<PreparedGeneratorResume, RuntimeError> {
        let Some(layout) = self.heap.generator_resume_layout(generator)? else {
            self.synchronize_scheduling_usage()?;
            return Ok(PreparedGeneratorResume {
                completions: Vec::new(),
                module: None,
                charge: self.charge_execution_control(0),
            });
        };
        let roots = self
            .bindings
            .iter()
            .flatten()
            .filter_map(Value::object_id)
            .count()
            .saturating_add(self.cells.len())
            // Completion, this and callee preserve their existing placeholders.
            .saturating_add(3)
            .saturating_add(self.arguments.iter().filter_map(Value::object_id).count())
            .saturating_add(self.with_objects.len())
            .saturating_add(
                self.disposables
                    .iter()
                    .flat_map(DisposableResource::values)
                    .filter_map(Value::object_id)
                    .count(),
            )
            .saturating_add(self.pending_execution_reference_count())
            .saturating_add(layout.with_objects);
        // One slot covers a resumed operand or the result rooted at frame exit.
        let required = self
            .stack
            .len()
            .saturating_add(roots)
            .saturating_add(layout.stack)
            .saturating_add(1);
        let growth = required
            .saturating_sub(self.stack.capacity())
            .saturating_mul(std::mem::size_of::<Value>());
        let referrer = layout
            .callee
            .and_then(|callee| self.module_closure_referrers.get(&callee));
        let module_bytes = referrer
            .or(self.active_module_name.as_ref())
            .map_or(0, String::len);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: layout
                .bytes
                .saturating_add(growth)
                .saturating_add(module_bytes)
                .saturating_add(
                    layout
                        .completions
                        .saturating_mul(std::mem::size_of::<Completion>()),
                ),
            ..Default::default()
        })?;
        let mut completions = Vec::new();
        if completions.try_reserve_exact(layout.completions).is_err() {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        let module = match layout
            .callee
            .and_then(|callee| self.module_closure_referrers.get(&callee))
        {
            Some(name) => PreparedModule::Referrer(name.clone()),
            None => PreparedModule::Caller(self.active_module_name.clone()),
        };
        let module_bytes = match &module {
            PreparedModule::Referrer(name) => name.capacity(),
            PreparedModule::Caller(name) => name.as_ref().map_or(0, String::capacity),
        };
        let actual = host_scheduling::slots(&completions).saturating_add(module_bytes);
        let prepared = PreparedGeneratorResume {
            completions,
            module: Some(module),
            charge: self.charge_execution_control(actual),
        };
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: layout.bytes.saturating_add(growth),
            ..Default::default()
        })?;
        if self
            .stack
            .try_reserve_exact(required.saturating_sub(self.stack.len()))
            .is_err()
        {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        // reserve_exact may report extra capacity; recheck it while the saved
        // state is still intact and no caller field has been displaced.
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: layout.bytes,
            ..Default::default()
        })?;
        Ok(prepared)
    }
}
