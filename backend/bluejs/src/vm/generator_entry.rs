// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admit generator parameter roots before creating or displacing a frame.

use super::*;

impl Vm {
    fn generator_entry_root_count(&self, inherited: &[Value]) -> usize {
        self.bindings
            .iter()
            .flatten()
            .filter_map(Value::object_id)
            .count()
            .saturating_add(self.cells.len())
            // Receiver, completion, this and callee keep their placeholders.
            .saturating_add(4)
            .saturating_add(self.arguments.iter().filter_map(Value::object_id).count())
            .saturating_add(
                self.with_objects
                    .iter()
                    .filter_map(Value::object_id)
                    .count(),
            )
            .saturating_add(inherited.iter().filter_map(Value::object_id).count())
            .saturating_add(
                self.disposables
                    .iter()
                    .flat_map(DisposableResource::values)
                    .filter_map(Value::object_id)
                    .count(),
            )
            .saturating_add(self.pending_execution_reference_count())
            .saturating_add(self.new_target.object_id().is_some() as usize)
            .saturating_add(self.home_object.is_some() as usize)
            .saturating_add(self.parameter_eval_env.is_some() as usize)
            .saturating_add(
                self.dynamic_eval_bindings
                    .values()
                    .chain(
                        self.dynamic_eval_outer_bindings
                            .iter()
                            .flat_map(HashMap::values),
                    )
                    .fold(0usize, |count, binding| {
                        count
                            .saturating_add(1)
                            .saturating_add(binding.shadowed_cells.len())
                    }),
            )
    }

    pub(super) fn prepare_generator_entry(
        &mut self,
        inherited: &mut Vec<Value>,
        receiver_is_object: bool,
        parameter_env: bool,
        entry: &mut execution_payload::CallEntryCharge,
    ) -> Result<(), RuntimeError> {
        let result = self.prepare_generator_entry_storage(
            inherited,
            receiver_is_object,
            parameter_env,
            entry,
        );
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    fn prepare_generator_entry_storage(
        &mut self,
        inherited: &mut Vec<Value>,
        receiver_is_object: bool,
        parameter_env: bool,
        entry: &mut execution_payload::CallEntryCharge,
    ) -> Result<(), RuntimeError> {
        // The outer invocation protects its boxed receiver and generator;
        // parameter environment creation temporarily pushes one more root.
        let required = self
            .stack
            .len()
            .saturating_add(self.generator_entry_root_count(inherited))
            .saturating_add(1 + usize::from(receiver_is_object) + usize::from(parameter_env));
        let stack_growth = required
            .saturating_sub(self.stack.capacity())
            .saturating_mul(std::mem::size_of::<Value>());
        let inherited_required = inherited.len().saturating_add(usize::from(parameter_env));
        let inherited_growth = inherited_required
            .saturating_sub(inherited.capacity())
            .saturating_mul(std::mem::size_of::<Value>());
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: stack_growth.saturating_add(inherited_growth),
            ..Default::default()
        })?;
        let before = host_scheduling::slots(inherited);
        if inherited
            .try_reserve_exact(inherited_required.saturating_sub(inherited.len()))
            .is_err()
        {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        entry.add_control_storage(host_scheduling::slots(inherited).saturating_sub(before))?;
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: stack_growth,
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
        self.synchronize_scheduling_usage()
    }

    pub(super) fn push_generator_entry_roots(&mut self, receiver: &Value, inherited: &[Value]) {
        self.stack
            .push(receiver.object_id().map_or(Value::Undefined, Value::Object));
        self.stack.extend(
            self.bindings
                .iter()
                .flatten()
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        for value in [&self.completion, &self.this, &self.callee] {
            self.stack
                .push(value.object_id().map_or(Value::Undefined, Value::Object));
        }
        self.stack.extend(
            self.arguments
                .iter()
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack.extend(
            self.with_objects
                .iter()
                .chain(inherited)
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack.extend(
            self.disposables
                .iter()
                .flat_map(DisposableResource::values)
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack.extend(
            self.new_target
                .object_id()
                .into_iter()
                .chain(self.home_object)
                .chain(self.parameter_eval_env)
                .map(Value::Object),
        );
        self.stack.extend(
            self.dynamic_eval_bindings
                .values()
                .chain(
                    self.dynamic_eval_outer_bindings
                        .iter()
                        .flat_map(HashMap::values),
                )
                .flat_map(|binding| {
                    std::iter::once(binding.cell).chain(binding.shadowed_cells.iter().copied())
                })
                .map(Value::Object),
        );
        self.push_pending_execution_roots();
    }
}
