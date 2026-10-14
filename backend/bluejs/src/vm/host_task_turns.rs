// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One host task turn arbitrates timers and document browser work. Rendering
//! remains a separate entry; Core can offer it after each selected task.

use super::*;

const MAX_DOCUMENT_TASKS_PER_TURN: usize = 64;

/// Result of a bounded page task turn, without callbacks or source text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlueJsTaskTurn {
    /// Task deliveries, each followed by its microtask checkpoint.
    pub tasks_run: u32,
    /// Relative host milliseconds. Zero means work is immediately runnable;
    /// `None` means no task wake is needed, including while debugger-paused.
    pub next_wake_in_ms: Option<u64>,
}

impl Vm {
    fn document_browser_wake(&self) -> Option<u64> {
        let mut wake = self.next_own_browser_task_wake();
        if !self.host_finalization_tasks.is_empty() {
            wake = Some(0);
        }
        for record in self.host_shadow_realms() {
            let vm = record.vm.borrow();
            if vm.ensure_no_debugger_continuation().is_err() {
                continue;
            }
            let child = if vm.host_finalization_tasks.is_empty() {
                vm.next_own_browser_task_wake()
            } else {
                Some(0)
            };
            wake = earliest_wake(wake, child);
        }
        wake
    }

    fn checkpoint_document_task_entry(&mut self) -> Result<(), RuntimeError> {
        self.run_host_microtask_checkpoint()?;
        for record in self.host_shadow_realms() {
            let mut vm = record.vm.borrow_mut();
            if vm.ensure_no_debugger_continuation().is_ok() {
                vm.run_host_microtask_checkpoint()?;
            }
        }
        Ok(())
    }

    pub(crate) fn next_host_task_wake(&mut self, now_ms: u64) -> Result<Option<u64>, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(None);
        }
        self.host_timer_clock_ms = self.host_timer_clock_ms.max(now_ms);
        let now = self.sample_host_timer_clock();
        Ok(earliest_wake(
            self.document_browser_wake(),
            self.next_host_timer_due()
                .map(|due| due.saturating_sub(now)),
        ))
    }

    pub(crate) fn run_host_task_turn(
        &mut self,
        now_ms: u64,
        max_tasks: usize,
    ) -> Result<BlueJsTaskTurn, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(BlueJsTaskTurn::default());
        }
        self.host_timer_clock_ms = self.host_timer_clock_ms.max(now_ms);
        self.checkpoint_document_task_entry()?;
        // Preserve timer turn snapshots: callbacks and their jobs cannot
        // repeatedly rearm a timer into the same host entry.
        let mut timers: VecDeque<_> = self.snapshot_due_host_timers(now_ms).into();
        let mut turn = BlueJsTaskTurn::default();
        while (turn.tasks_run as usize) < max_tasks.min(MAX_DOCUMENT_TASKS_PER_TURN) {
            while timers.front().is_some_and(|id| !self.has_host_timer(*id)) {
                timers.pop_front();
            }
            let browser_ready = self.document_browser_wake() == Some(0);
            let timer_ready = !timers.is_empty();
            if !browser_ready && !timer_ready {
                break;
            }
            let timer = timer_ready && (self.next_task_is_timer || !browser_ready);
            if timer {
                let id = timers.pop_front().expect("a ready timer was selected");
                self.run_host_timer_entry(id, &mut BlueJsTimerRun::default())?;
            } else {
                self.run_document_browser_task_turn(1)?;
            }
            self.next_task_is_timer = !timer;
            turn.tasks_run += 1;
        }
        turn.next_wake_in_ms = self.next_host_task_wake(now_ms)?;
        Ok(turn)
    }
}

fn earliest_wake(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}
