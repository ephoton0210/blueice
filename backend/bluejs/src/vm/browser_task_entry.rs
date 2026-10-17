// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser task entry prepares an attempt before removing queued owners.

use super::browser_tasks::BrowserTask;
use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserEntryAction {
    Run,
    Cleanup,
    Deferred,
}

pub(super) struct PreparedBrowserArguments {
    pub(super) values: Vec<Value>,
    charge: Option<SharedSchedulingCharge>,
}
impl PreparedBrowserArguments {
    pub(super) fn into_arguments(self) -> Vec<Value> {
        let Self { values, charge } = self;
        // Ordinary call entry adopts this capacity before its first fallible
        // step. The queued primitive moves directly into the argument vector.
        drop(charge);
        values
    }
}
pub(super) struct PreparedBrowserEntry {
    pub(super) action: BrowserEntryAction,
    pub(super) arguments: Option<PreparedBrowserArguments>,
}
impl Vm {
    pub(super) fn prepare_browser_task_entry(
        &mut self,
        index: usize,
    ) -> Result<PreparedBrowserEntry, RuntimeError> {
        self.check_scheduling_failure()?;
        let task = self
            .browser_tasks
            .get(index)
            .expect("a runnable task was selected");
        let action = match task {
            BrowserTask::DragString { .. } => BrowserEntryAction::Run,
            BrowserTask::HostPromise { token, .. } => {
                if self.host_promises.pending.contains_key(token) {
                    BrowserEntryAction::Run
                } else {
                    BrowserEntryAction::Cleanup
                }
            }
            BrowserTask::FileRead { reader, epoch, .. } => {
                let state = self.heap.file_reader(*reader)?;
                if state.ready == 1 && state.epoch == *epoch {
                    BrowserEntryAction::Run
                } else {
                    BrowserEntryAction::Cleanup
                }
            }
            BrowserTask::PromiseRejections {
                entries, handled, ..
            } => {
                let eligible = entries.iter().any(|entry| {
                    let record = self
                        .promises
                        .get(&entry.promise)
                        .expect("notification roots its Promise");
                    (*handled || !record.handled)
                        && matches!(&record.status, PromiseStatus::Rejected(_))
                });
                if eligible {
                    BrowserEntryAction::Run
                } else {
                    BrowserEntryAction::Cleanup
                }
            }
            BrowserTask::AbortTimeout {
                signal, deadline, ..
            } => match self.heap.abort_signal(*signal) {
                Ok(state) if !state.aborted() => {
                    let now = self
                        .host_scheduling
                        .as_ref()
                        .map_or(self.host_timer_clock_ms, |scope| {
                            scope.advance_clock(self.host_timer_clock_ms)
                        });
                    if deadline.wake(now) == 0 {
                        BrowserEntryAction::Run
                    } else {
                        BrowserEntryAction::Deferred
                    }
                }
                _ => BrowserEntryAction::Cleanup,
            },
        };
        let mut prepared = PreparedBrowserEntry {
            action,
            arguments: None,
        };
        if action != BrowserEntryAction::Run {
            return Ok(prepared);
        }
        if self.config.instruction_budget == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        let drag = matches!(task, BrowserTask::DragString { .. });
        self.admit_scheduling_work(HostSchedulingUsage::default())?;
        if drag {
            let charge = self
                .host_scheduling
                .as_ref()
                .map(|scope| scope.shared_charge(std::mem::size_of::<Value>()))
                .transpose()?;
            let mut arguments = PreparedBrowserArguments {
                values: Vec::new(),
                charge,
            };
            arguments.values.try_reserve_exact(1).map_err(|_| {
                self.host_scheduling
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
            })?;
            if let Some(charge) = &mut arguments.charge {
                charge.resize(host_scheduling::slots(&arguments.values))?;
            }
            prepared.arguments = Some(arguments);
        }
        Ok(prepared)
    }

    pub(super) fn discard_inactive_browser_task(&mut self, task: BrowserTask) {
        match task {
            BrowserTask::PromiseRejections { entries, .. } => {
                self.discard_host_rejection_entries(entries)
            }
            BrowserTask::AbortTimeout { root, .. } => {
                if let Some(root) = root {
                    self.release_root(root);
                }
            }
            BrowserTask::FileRead { root, .. } | BrowserTask::DragString { root, .. } => {
                self.release_root(root)
            }
            BrowserTask::HostPromise { .. } => {}
        }
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/browser_task_entry.rs"]
mod ownership_tests;
