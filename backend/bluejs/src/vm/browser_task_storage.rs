// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser producers prepare queue, payload, root and state owners together.

use super::browser_tasks::BrowserTask;
use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct PreparedBrowserTask {
    queue: promise_queue_storage::PreparedDeque<BrowserTask>,
    incoming: Option<SharedSchedulingCharge>,
}
impl PreparedBrowserTask {
    pub(super) fn resize_payload(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if let Some(charge) = &mut self.incoming {
            charge.resize(bytes)?;
        }
        Ok(())
    }
    /// The task queue must stay unchanged. Managed-only collection may prune
    /// other owners while later producer preparation runs.
    pub(super) fn commit(self, vm: &mut Vm, task: BrowserTask) {
        self.queue.commit(&mut vm.browser_tasks);
        vm.browser_tasks.push_back(task);
        drop(self.incoming);
        vm.reconcile_scheduling_ownership();
    }
}
impl Vm {
    pub(super) fn prepare_browser_task(
        &mut self,
        mut extra: HostSchedulingUsage,
    ) -> Result<PreparedBrowserTask, RuntimeError> {
        if self.host_scheduling.is_none() && self.browser_tasks.len() >= MAX_PENDING_BROWSER_TASKS {
            return Err(RuntimeError::RangeError(
                "Browser task queue limit exceeded".into(),
            ));
        }
        let bytes = std::mem::take(&mut extra.payload_bytes);
        extra.pending_browser_tasks = extra.pending_browser_tasks.saturating_add(1);
        self.admit_scheduling_work(extra)?;
        let incoming = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(bytes))
            .transpose()?;
        let queue = promise_queue_storage::PreparedDeque::for_push(
            &self.browser_tasks,
            self.host_scheduling.as_ref(),
        )?;
        Ok(PreparedBrowserTask { queue, incoming })
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/browser_task_storage.rs"]
mod ownership_tests;
