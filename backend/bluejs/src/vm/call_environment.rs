// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admit forwarded environment growth and recovery copies before call effects.

use super::*;

type RecoveryParts = (Vec<HashMap<String, DynamicEvalBinding>>, Option<String>);

pub(super) struct PreparedCallEnvironment {
    recovery: Vec<HashMap<String, DynamicEvalBinding>>,
    module: Option<String>,
    _charge: execution_payload::ExecutionPayloadCharge,
}

impl PreparedCallEnvironment {
    pub(super) fn take_parts(&mut self) -> RecoveryParts {
        (std::mem::take(&mut self.recovery), self.module.take())
    }
}

fn cloned_binding_bytes(bindings: &HashMap<String, DynamicEvalBinding>) -> usize {
    bindings.iter().fold(
        host_scheduling::map_slots(bindings),
        |bytes, (name, binding)| {
            bytes.saturating_add(name.len()).saturating_add(
                binding
                    .shadowed_cells
                    .len()
                    .saturating_mul(std::mem::size_of::<ObjectId>()),
            )
        },
    )
}

impl Vm {
    pub(super) fn prepare_call_environment(
        &mut self,
        can_suspend: bool,
    ) -> Result<PreparedCallEnvironment, RuntimeError> {
        let result = self.prepare_call_environment_storage(can_suspend);
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    fn prepare_call_environment_storage(
        &mut self,
        can_suspend: bool,
    ) -> Result<PreparedCallEnvironment, RuntimeError> {
        let growth = SchedulingCapacity::for_vec_push(&self.dynamic_eval_outer_bindings);
        let mut bytes = 0usize;
        if can_suspend {
            bytes = self
                .dynamic_eval_outer_bindings
                .len()
                .saturating_add(1)
                .saturating_mul(std::mem::size_of::<HashMap<String, DynamicEvalBinding>>())
                .saturating_add(self.active_module_name.as_ref().map_or(0, String::len));
            for bindings in self
                .dynamic_eval_outer_bindings
                .iter()
                .chain([&self.dynamic_eval_bindings])
            {
                bytes = bytes.saturating_add(cloned_binding_bytes(bindings));
            }
        }
        self.admit_scheduling_work(
            HostSchedulingUsage {
                payload_bytes: bytes,
                ..Default::default()
            }
            .with_growth(growth),
        )?;
        let (recovery, module) = if can_suspend {
            let mut recovery = Vec::with_capacity(self.dynamic_eval_outer_bindings.len() + 1);
            recovery.extend(self.dynamic_eval_outer_bindings.iter().cloned());
            recovery.push(self.dynamic_eval_bindings.clone());
            (recovery, self.active_module_name.clone())
        } else {
            (Vec::new(), None)
        };
        let actual = execution_payload::outer_binding_bytes(&recovery)
            .saturating_add(module.as_ref().map_or(0, String::capacity));
        let prepared = PreparedCallEnvironment {
            recovery,
            module,
            _charge: self.charge_execution_control(actual),
        };
        self.admit_scheduling_work(HostSchedulingUsage::default().with_growth(growth))?;
        growth.reserve_vec(&mut self.dynamic_eval_outer_bindings);
        self.synchronize_scheduling_usage()?;
        Ok(prepared)
    }
}
