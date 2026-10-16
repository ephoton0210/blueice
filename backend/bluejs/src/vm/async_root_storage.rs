// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admit temporary execution root capacity before publishing heap edges.

use super::*;

impl Vm {
    /// Reserve a detached replacement while the existing allocation remains
    /// charged. No heap operation or JavaScript runs during preparation.
    pub(super) fn prepare_async_frame_root_slots(
        &mut self,
        additional: usize,
    ) -> Result<(), RuntimeError> {
        self.check_scheduling_failure()?;
        let required = self.async_frame_roots.len().saturating_add(additional);
        if required <= self.async_frame_roots.capacity() {
            return Ok(());
        }
        let capacity = required
            .max(self.async_frame_roots.capacity().saturating_mul(2))
            .max(4);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: capacity.saturating_mul(std::mem::size_of::<ObjectId>()),
            ..Default::default()
        })?;
        let mut replacement = Vec::new();
        replacement.try_reserve_exact(capacity).map_err(|_| {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        })?;
        let staging = self.charge_execution_control(host_scheduling::slots(&replacement));
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(replacement);
            drop(staging);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        replacement.append(&mut self.async_frame_roots);
        self.async_frame_roots = replacement;
        drop(staging);
        self.reconcile_scheduling_ownership();
        Ok(())
    }

    /// Cloning a borrowed iterator counts edges without copying its objects.
    pub(super) fn extend_async_frame_roots(
        &mut self,
        references: impl Iterator<Item = ObjectId> + Clone,
    ) -> Result<(), RuntimeError> {
        self.prepare_async_frame_root_slots(references.clone().count())?;
        self.async_frame_roots.extend(references);
        Ok(())
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/async_root_storage.rs"]
mod ownership_tests;
