// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Timer registration prepares storage before publishing roots and identities.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct PreparedTimerRegistration {
    pub(super) arguments: Vec<Value>,
    pub(super) roots: Vec<RootId>,
    pub(super) replacement: Option<Vec<HostTimer>>,
    // Detached buffers release before their independent document lease.
    pub(super) charge: Option<SharedSchedulingCharge>,
}
impl PreparedTimerRegistration {
    fn admit_reported(&mut self, pending: usize) -> Result<(), RuntimeError> {
        let bytes = host_scheduling::slots(&self.arguments)
            .saturating_add(host_scheduling::slots(&self.roots))
            .saturating_add(self.replacement.as_ref().map_or(0, host_scheduling::slots))
            .saturating_add(pending);
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }
        Ok(())
    }
}

impl Vm {
    pub(super) fn prepare_timer_registration(
        &mut self,
        arguments: &[Value],
        root_count: usize,
    ) -> Result<PreparedTimerRegistration, RuntimeError> {
        let capacity = (self.host_timers.len() == self.host_timers.capacity()).then(|| {
            self.host_timers
                .len()
                .saturating_add(1)
                .max(self.host_timers.capacity().saturating_mul(2))
                .max(4)
        });
        let payload = host_scheduling::values_bytes(arguments);
        let forecast = arguments
            .len()
            .saturating_mul(std::mem::size_of::<Value>())
            .saturating_add(root_count.saturating_mul(std::mem::size_of::<RootId>()))
            .saturating_add(
                capacity
                    .unwrap_or(0)
                    .saturating_mul(std::mem::size_of::<HostTimer>()),
            )
            .saturating_add(payload);
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_timers: 1,
            payload_bytes: forecast,
            ..Default::default()
        })?;
        let charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(forecast))
            .transpose()?;
        let mut prepared = PreparedTimerRegistration {
            arguments: Vec::new(),
            roots: Vec::new(),
            replacement: capacity.map(|_| Vec::new()),
            charge,
        };
        let fail = || {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        };
        prepared
            .arguments
            .try_reserve_exact(arguments.len())
            .map_err(|_| fail())?;
        let table_bytes = capacity
            .unwrap_or(0)
            .saturating_mul(std::mem::size_of::<HostTimer>());
        prepared.admit_reported(
            payload
                .saturating_add(table_bytes)
                .saturating_add(root_count.saturating_mul(std::mem::size_of::<RootId>())),
        )?;
        prepared
            .roots
            .try_reserve_exact(root_count)
            .map_err(|_| fail())?;
        prepared.admit_reported(payload.saturating_add(table_bytes))?;
        if let (Some(replacement), Some(capacity)) = (&mut prepared.replacement, capacity) {
            replacement
                .try_reserve_exact(capacity)
                .map_err(|_| fail())?;
        }
        prepared.admit_reported(payload)?;
        prepared.arguments.extend_from_slice(arguments);
        Ok(prepared)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/timer_registration.rs"]
mod ownership_tests;
