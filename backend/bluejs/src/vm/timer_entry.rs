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
    pub(super) fn adopt_arguments(&mut self, arguments: Vec<Value>) {
        self.arguments = arguments;
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
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: count
                .saturating_mul(std::mem::size_of::<Value>())
                .saturating_add(bytes),
            ..Default::default()
        })?;
        if prepared.payload.enabled() {
            prepared.payload.resize(bytes);
        }
        prepared.arguments.try_reserve_exact(count).map_err(|_| {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        })?;
        if prepared.control.enabled() {
            prepared
                .control
                .resize(host_scheduling::slots(&prepared.arguments));
        }
        self.synchronize_scheduling_usage()?;
        prepared
            .arguments
            .extend_from_slice(&self.host_timers[index].arguments);
        // The interval may clear its own registration. Temporary object edges
        // remain traced through its callback and Promise checkpoint, without
        // allocating an intermediate object list or new persistent roots.
        self.prepare_async_frame_root_slots(prepared.references.iter().flatten().count())?;
        Ok(prepared)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/timer_entry.rs"]
mod ownership_tests;
