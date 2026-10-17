// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Document quiescence precedes notifications, ClearKeptObjects and collection.

use super::*;

impl Vm {
    pub(super) fn run_document_checkpoint(
        &mut self,
        remaining: &mut usize,
    ) -> Result<u32, RuntimeError> {
        let scope = self
            .host_scheduling
            .as_ref()
            .expect("document checkpoint")
            .child();
        let _deferral = scope.defer_checkpoint()?;
        scope.request_checkpoint();
        let mut uncaught = 0u32;
        loop {
            let before = *remaining;
            uncaught =
                uncaught.saturating_add(self.run_host_microtask_checkpoint_with_budget(remaining)?);
            let (members, mut blocked) = self.host_checkpoint_realms()?;
            for member in members.iter() {
                if let Some(count) = member.enter(self, |vm| {
                    if vm.ensure_no_debugger_continuation().is_err() {
                        blocked = true;
                        return Ok(0);
                    }
                    vm.sample_host_timer_clock();
                    vm.run_host_microtask_checkpoint_with_budget(remaining)
                })? {
                    uncaught = uncaught.saturating_add(count);
                }
            }
            if before != *remaining {
                continue;
            }
            // A borrowed member can still own jobs or kept targets. Retain
            // completion state and request another host wake after it is free.
            if blocked {
                return Ok(uncaught);
            }

            self.prepare_checkpoint_completion()?;
            for member in members.iter() {
                member.enter(self, |vm| {
                    if vm.ensure_no_debugger_continuation().is_ok() {
                        vm.prepare_checkpoint_completion()?;
                    }
                    Ok(())
                })?;
            }
            // Reporting may run a handler that queues more document jobs.
            let (members, blocked) = self.host_checkpoint_realms()?;
            if blocked {
                return Ok(uncaught);
            }
            let mut pending = !self.promise_jobs.is_empty();
            for member in members.iter() {
                pending |= member
                    .read(self, |vm| {
                        Ok(vm.ensure_no_debugger_continuation().is_ok()
                            && !vm.promise_jobs.is_empty())
                    })?
                    .unwrap_or(false);
            }
            if pending {
                continue;
            }

            // Clear all reachable kept sets before any realm's optional GC.
            self.kept_weak_objects.clear();
            for member in members.iter() {
                member.enter(self, |vm| {
                    if vm.checkpoint_context_is_idle()
                        && vm.ensure_no_debugger_continuation().is_ok()
                    {
                        vm.kept_weak_objects.clear();
                    }
                    Ok(())
                })?;
            }
            self.collect_after_checkpoint()?;
            for member in members.iter() {
                member.enter(self, |vm| {
                    if vm.ensure_no_debugger_continuation().is_ok() {
                        vm.collect_after_checkpoint()?;
                    }
                    Ok(())
                })?;
            }
            scope.complete_checkpoint();
            return Ok(uncaught);
        }
    }
}
