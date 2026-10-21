// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Timer callback entry preserves registrations until an attempt can start.

use super::execution_payload::ExecutionPayloadCharge;
use super::*;

pub(super) struct PreparedTimerEntry {
    pub(super) arguments: Vec<Value>,
    pub(super) references: [Option<ObjectId>; host_timers::MAX_HOST_TIMER_ARGUMENTS + 1],
    payload: ExecutionPayloadCharge,
    control: ExecutionPayloadCharge,
}
impl PreparedTimerEntry {
    pub(super) fn adopt_arguments(
        &mut self,
        vm: &Vm,
        arguments: Vec<Value>,
        native: Option<host_scheduling::SharedSchedulingCharge>,
    ) {
        self.arguments = arguments;
        if let Some(mut native) = native {
            let bytes = host_scheduling::values_bytes(&self.arguments);
            self.payload = vm.adopt_job_payload(Some(native.split_off(bytes)), bytes);
            self.control =
                vm.adopt_job_payload(Some(native), host_scheduling::slots(&self.arguments));
            return;
        }
        if self.payload.enabled() {
            self.payload
                .resize(host_scheduling::values_bytes(&self.arguments));
            self.control.resize(host_scheduling::slots(&self.arguments));
        }
    }
    pub(super) fn into_arguments(self) -> Vec<Value> {
        let Self {
            arguments,
            references: _,
            payload,
            control,
        } = self;
        // Ordinary call entry acquires these buffers before its first fallible
        // step. Only the task's separate GC protection survives the handoff.
        drop(payload);
        drop(control);
        arguments
    }
}

impl Vm {
    pub(super) fn prepare_timer_entry(
        &mut self,
        index: usize,
    ) -> Result<PreparedTimerEntry, RuntimeError> {
        let result = self.prepare_timer_entry_inner(index);
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }
    fn prepare_timer_entry_inner(
        &mut self,
        index: usize,
    ) -> Result<PreparedTimerEntry, RuntimeError> {
        self.check_scheduling_failure()?;
        let owner = self.timer_entry_owner(index)?;
        if self.config.instruction_budget == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        let mut prepared = PreparedTimerEntry {
            arguments: Vec::new(),
            references: [None; host_timers::MAX_HOST_TIMER_ARGUMENTS + 1],
            payload: self.charge_execution_payload(0),
            control: self.charge_execution_control(0),
        };
        let timer = &self.host_timers[index];
        if timer.interval_ms.is_none() {
            // A one-shot transfers its existing argument and root allocations.
            return Ok(prepared);
        }
        let count = timer.arguments.len();
        let bytes = host_scheduling::values_bytes(&timer.arguments);
        for (slot, object) in prepared.references.iter_mut().zip(
            std::iter::once(timer.callback)
                .chain(timer.arguments.iter().filter_map(Value::object_id)),
        ) {
            *slot = Some(object);
        }
        prepared.payload.retain_entry(self, bytes, owner.as_ref())?;
        prepared.control.retain_entry(
            self,
            count.saturating_mul(std::mem::size_of::<Value>()),
            owner.as_ref(),
        )?;
        prepared.arguments.try_reserve_exact(count).map_err(|_| {
            owner.as_ref().map_or(
                RuntimeError::SchedulingLimit,
                host_scheduling::SharedSchedulingCharge::fail,
            )
        })?;
        if prepared.control.enabled() {
            prepared
                .control
                .resize_entry(host_scheduling::slots(&prepared.arguments))?;
        }
        self.synchronize_scheduling_usage()?;
        prepared
            .arguments
            .extend_from_slice(&self.host_timers[index].arguments);
        if prepared.payload.enabled() {
            prepared
                .payload
                .resize_entry(host_scheduling::values_bytes(&prepared.arguments))?;
        }
        // The interval may clear its own registration. Temporary object edges
        // remain traced through its callback and Promise checkpoint, without
        // allocating an intermediate object list or new persistent roots.
        if self.host_scheduling.is_some() || owner.is_none() {
            self.prepare_async_frame_root_slots(prepared.references.iter().flatten().count())?;
        }
        Ok(prepared)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/timer_entry.rs"]
mod ownership_tests;
