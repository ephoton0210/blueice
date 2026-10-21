// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cleanup entry prepares native ownership before committing its cell.

use super::execution_payload::ExecutionPayloadCharge;
use super::*;

pub(super) struct PreparedFinalizationEntry {
    pub(super) arguments: Vec<Value>,
    pub(super) references: [Option<ObjectId>; 2],
    payload: ExecutionPayloadCharge,
    control: ExecutionPayloadCharge,
}
impl PreparedFinalizationEntry {
    pub(super) fn into_arguments(self) -> Vec<Value> {
        let Self {
            arguments,
            references: _,
            payload,
            control,
        } = self;
        // call_native acquires the same owners before its first fallible step.
        // The document contribution stays published during this direct handoff.
        drop(payload);
        drop(control);
        arguments
    }
}

impl Vm {
    pub(super) fn prepare_finalization_entry(
        &mut self,
        registry: ObjectId,
    ) -> Result<Option<PreparedFinalizationEntry>, RuntimeError> {
        let result = self.prepare_finalization_entry_inner(registry);
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }
    fn prepare_finalization_entry_inner(
        &mut self,
        registry: ObjectId,
    ) -> Result<Option<PreparedFinalizationEntry>, RuntimeError> {
        let Some((callback, holdings)) = self
            .heap
            .peek_finalization_registry_cleanup_cell(registry)?
        else {
            return Ok(None);
        };
        let references = [callback.object_id(), holdings.object_id()];
        let bytes = host_scheduling::value_payload_bytes(callback)
            .saturating_add(host_scheduling::value_payload_bytes(holdings));
        self.check_scheduling_failure()?;
        if self.config.instruction_budget == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        let owner = self.finalization_queue_owner(None)?;
        let mut prepared = PreparedFinalizationEntry {
            arguments: Vec::new(),
            references,
            payload: self.charge_execution_payload(0),
            control: self.charge_execution_control(0),
        };
        prepared.payload.retain_entry(self, bytes, owner.as_ref())?;
        prepared
            .control
            .retain_entry(self, std::mem::size_of::<Value>(), owner.as_ref())?;
        prepared.arguments.try_reserve_exact(1).map_err(|_| {
            owner.as_ref().map_or(
                RuntimeError::SchedulingLimit,
                host_scheduling::SharedSchedulingCharge::fail,
            )
        })?;
        prepared
            .control
            .resize_entry(host_scheduling::slots(&prepared.arguments))?;
        self.synchronize_scheduling_usage()?;
        self.prepare_async_frame_root_slots(references.iter().flatten().count())?;
        Ok(Some(prepared))
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/finalization_entry.rs"]
mod ownership_tests;
