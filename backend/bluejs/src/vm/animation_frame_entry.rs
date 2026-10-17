// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Render callback preparation preserves unattempted registrations.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct PreparedAnimationFrameEntry {
    arguments: Vec<Value>,
    charge: Option<SharedSchedulingCharge>,
}
impl PreparedAnimationFrameEntry {
    pub(super) fn into_arguments(self) -> Vec<Value> {
        let Self { arguments, charge } = self;
        // Ordinary dispatch adopts the vector before its first fallible step.
        // No collection or JavaScript runs across this direct handoff.
        drop(charge);
        arguments
    }
}
impl Vm {
    pub(super) fn prepare_animation_frame_arguments(
        &mut self,
        timestamp: f64,
    ) -> Result<PreparedAnimationFrameEntry, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.config.instruction_budget == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        self.admit_scheduling_work(HostSchedulingUsage::default())?;
        let charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(std::mem::size_of::<Value>()))
            .transpose()?;
        let mut prepared = PreparedAnimationFrameEntry {
            arguments: Vec::new(),
            charge,
        };
        prepared.arguments.try_reserve_exact(1).map_err(|_| {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        })?;
        if let Some(charge) = &mut prepared.charge {
            charge.resize(host_scheduling::slots(&prepared.arguments))?;
        }
        prepared.arguments.push(Value::Number(timestamp));
        Ok(prepared)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/animation_frame_entry.rs"]
mod ownership_tests;
