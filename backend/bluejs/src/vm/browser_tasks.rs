// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser task delivery is separate from Promise microtask checkpoints.

use super::*;

#[derive(Clone, Copy)]
pub(super) enum AbortTimeoutDeadline {
    Host(u64),
    Monotonic {
        started: std::time::Instant,
        delay: std::time::Duration,
    },
}

impl AbortTimeoutDeadline {
    pub(super) fn wake(self, now_ms: u64) -> u64 {
        match self {
            Self::Host(due) => due.saturating_sub(now_ms),
            Self::Monotonic { started, delay } => u64::try_from(
                delay
                    .saturating_sub(started.elapsed())
                    .as_nanos()
                    .div_ceil(1_000_000),
            )
            .unwrap_or(u64::MAX),
        }
    }
}

pub(super) enum BrowserTask {
    AbortTimeout {
        signal: ObjectId,
        deadline: AbortTimeoutDeadline,
        serial: u64,
        root: Option<RootId>,
    },
    DragString {
        serial: u64,
        callback: ObjectId,
        text: JsString,
        root: RootId,
    },
    FileRead {
        serial: u64,
        reader: ObjectId,
        epoch: u64,
        stage: u8,
        root: RootId,
    },
    HostPromise {
        token: HostPromiseToken,
        completion: HostPromiseCompletion,
        source: HostTaskSource,
        serial: u64,
    },
    PromiseRejections {
        serial: u64,
        entries: Vec<host_rejections::PendingRejection>,
        handled: bool,
    },
}

impl BrowserTask {
    pub(super) fn source(&self) -> host_task_turns::TaskSource {
        use host_task_turns::TaskSource;
        match self {
            Self::AbortTimeout { .. } => TaskSource::Timer,
            Self::DragString { .. } | Self::PromiseRejections { .. } => TaskSource::DomManipulation,
            Self::FileRead { .. } => TaskSource::FileReading,
            Self::HostPromise {
                source: HostTaskSource::Networking,
                ..
            } => TaskSource::Networking,
            Self::HostPromise {
                source: HostTaskSource::UserInteraction,
                ..
            } => TaskSource::UserInteraction,
        }
    }

    pub(super) fn order(&self) -> (u64, u64) {
        match self {
            Self::AbortTimeout {
                deadline: AbortTimeoutDeadline::Host(due),
                serial,
                ..
            } => (*due, *serial),
            Self::AbortTimeout { serial, .. }
            | Self::DragString { serial, .. }
            | Self::FileRead { serial, .. }
            | Self::HostPromise { serial, .. } => (0, *serial),
            Self::PromiseRejections { serial, .. } => (0, *serial),
        }
    }

    fn set_serial(&mut self, next: u64) {
        match self {
            Self::AbortTimeout { serial, .. }
            | Self::DragString { serial, .. }
            | Self::FileRead { serial, .. }
            | Self::HostPromise { serial, .. } => *serial = next,
            Self::PromiseRejections { serial, .. } => *serial = next,
        }
    }
}

impl Vm {
    /// Reserve one browser entry before publishing roots, wait flags or read
    /// state. `extra` describes payload/reaction ownership added by that entry.
    pub(super) fn reserve_browser_task(
        &mut self,
        mut extra: HostSchedulingUsage,
    ) -> Result<(), RuntimeError> {
        if self.host_scheduling.is_none() && self.browser_tasks.len() >= MAX_PENDING_BROWSER_TASKS {
            return Err(RuntimeError::RangeError(
                "Browser task queue limit exceeded".into(),
            ));
        }
        extra.pending_browser_tasks = extra.pending_browser_tasks.saturating_add(1);
        let growth = SchedulingCapacity::for_deque_push(&self.browser_tasks);
        self.admit_scheduling_work(extra.with_growth(growth))?;
        growth.reserve_deque(&mut self.browser_tasks);
        // Publish the actual capacity to the ledger even if a later native
        // allocation fails. No entry/count has been added yet.
        self.admit_scheduling_work(extra)
    }

    pub(super) fn browser_task_wake(&self, task: &BrowserTask) -> u64 {
        if let BrowserTask::AbortTimeout {
            signal, deadline, ..
        } = task
        {
            if self
                .heap
                .abort_signal(*signal)
                .is_ok_and(|state| !state.aborted())
            {
                return deadline.wake(self.host_timer_clock_ms);
            }
        }
        0
    }

    pub(super) fn next_own_browser_task_wake(&self) -> Option<u64> {
        self.browser_tasks
            .iter()
            .map(|task| self.browser_task_wake(task))
            .min()
    }

    pub(super) fn queue_drag_string_task(
        &mut self,
        callback: ObjectId,
        text: JsString,
    ) -> Result<(), RuntimeError> {
        let bytes = self
            .browser_tasks
            .iter()
            .fold(text.byte_len(), |total, task| {
                total.saturating_add(match task {
                    BrowserTask::DragString { text, .. } => text.byte_len(),
                    _ => 0,
                })
            });
        if self.browser_tasks.len() >= 64 || bytes > 1_048_576 {
            return Err(RuntimeError::RangeError(
                "Browser task queue limit exceeded".into(),
            ));
        }
        self.reserve_browser_task(HostSchedulingUsage {
            payload_bytes: text.owned_heap_payload_bytes().unwrap_or(usize::MAX),
            ..HostSchedulingUsage::default()
        })?;
        let serial = self.next_host_task_serial()?;
        let root = self.heap.root(callback)?;
        self.browser_tasks.push_back(BrowserTask::DragString {
            serial,
            callback,
            text,
            root,
        });
        self.synchronize_scheduling_usage()?;
        Ok(())
    }

    /// Compatibility entrypoint; all queued browser tasks share a bounded
    /// turn scheduler, with a microtask checkpoint between task deliveries.
    pub fn run_drag_tasks_bounded(&mut self, max_tasks: usize) -> Result<(), RuntimeError> {
        self.run_browser_tasks_bounded(max_tasks)
    }

    pub fn run_browser_tasks_bounded(&mut self, max_tasks: usize) -> Result<(), RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        if self.run_browser_task_turn(max_tasks)? {
            Ok(())
        } else {
            Err(RuntimeError::InstructionLimit)
        }
    }

    /// Runs one bounded browser turn, returning whether the task queue drained.
    /// A paused debugger realm keeps its jobs and tasks until it resumes. Core
    /// can yield between turns without cancelling pending FileReader reads.
    pub fn run_browser_task_turn(&mut self, max_tasks: usize) -> Result<bool, RuntimeError> {
        if self.host_scheduling.is_some() {
            return self.run_document_browser_task_turn(max_tasks);
        }
        self.run_own_browser_task_turn(max_tasks)
    }

    pub(super) fn run_own_browser_task_turn(
        &mut self,
        max_tasks: usize,
    ) -> Result<bool, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(false);
        }
        self.run_host_microtask_checkpoint()?;
        for _ in 0..max_tasks {
            let Some(index) = self
                .browser_tasks
                .iter()
                .position(|task| self.browser_task_wake(task) == 0)
            else {
                self.browser_tasks.shrink_to_fit();
                self.synchronize_scheduling_usage()?;
                return Ok(self.browser_tasks.is_empty());
            };
            self.run_own_browser_task(index)?;
        }
        if self.browser_tasks.is_empty() {
            self.browser_tasks.shrink_to_fit();
        }
        self.synchronize_scheduling_usage()?;
        Ok(self.browser_tasks.is_empty())
    }

    pub(super) fn run_own_browser_task(&mut self, index: usize) -> Result<(), RuntimeError> {
        let task = self
            .browser_tasks
            .remove(index)
            .expect("a runnable task was selected");
        let active_bytes = match &task {
            BrowserTask::PromiseRejections { entries, .. } => {
                entries.capacity() * std::mem::size_of::<host_rejections::PendingRejection>()
            }
            _ => 0,
        };
        let moved_payload = self.charge_execution_payload(match &task {
            BrowserTask::DragString { text, .. } => {
                text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
            }
            BrowserTask::HostPromise { completion, .. } => completion.payload_bytes(),
            _ => 0,
        });
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_add(active_bytes);
        self.synchronize_scheduling_usage()?;
        self.remaining_instructions = self.config.instruction_budget;
        let (result, root, next, report_page_exception) = match task {
            BrowserTask::PromiseRejections {
                entries, handled, ..
            } => (
                self.run_host_rejection_task(entries, handled),
                None,
                None,
                false,
            ),
            BrowserTask::HostPromise {
                token, completion, ..
            } => (
                self.run_host_promise_task(token, completion, moved_payload),
                None,
                None,
                true,
            ),
            BrowserTask::AbortTimeout {
                signal,
                deadline,
                root,
                serial,
            } => match self.abort_timeout_task(signal, deadline) {
                Ok(done) => (
                    Ok(()),
                    root,
                    (!done).then_some(BrowserTask::AbortTimeout {
                        signal,
                        deadline,
                        serial,
                        root,
                    }),
                    false,
                ),
                Err(error) => (Err(error), root, None, false),
            },
            BrowserTask::DragString {
                callback,
                text,
                root,
                ..
            } => {
                // The callback receives the text by move; dispatch takes
                // the next lease instead of adding a second payload owner.
                drop(moved_payload);
                let result = self
                    .call_native(
                        Value::Object(callback),
                        Value::Undefined,
                        vec![Value::String(text)],
                        false,
                    )
                    .map(|_| ());
                (result, Some(root), None, true)
            }
            BrowserTask::FileRead {
                reader,
                epoch,
                stage,
                root,
                serial,
            } => match self.file_read_task(reader, epoch, stage) {
                Ok(next) => (
                    Ok(()),
                    Some(root),
                    next.map(|stage| BrowserTask::FileRead {
                        reader,
                        epoch,
                        stage,
                        root,
                        serial,
                    }),
                    false,
                ),
                Err(error) => (Err(error), Some(root), None, false),
            },
        };
        // Report before releasing task roots or synchronizing accounting,
        // either of which can collect an object-valued abrupt completion.
        let result = match &result {
            Err(error) if report_page_exception && host_timers::is_page_exception(error) => {
                self.report_host_exception(error).and(result)
            }
            _ => result,
        };
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_sub(active_bytes);
        if let Some(mut next) = next {
            // FileReader event callbacks can consume the freed slot and the
            // document's remaining task budget before this stage returns.
            let admitted = self
                .reserve_browser_task(HostSchedulingUsage::default())
                .and_then(|_| self.next_host_task_serial());
            match admitted {
                Ok(serial) => next.set_serial(serial),
                Err(error) => {
                    if let Some(root) = root {
                        self.release_root(root);
                    }
                    return Err(error);
                }
            }
            self.browser_tasks.push_back(next);
        } else if let Some(root) = root {
            self.heap.unroot(root)?;
        }
        self.synchronize_scheduling_usage()?;
        if let Err(error) = result {
            if !report_page_exception || !super::web_drag_drop::drag_page_exception(&error) {
                return Err(error);
            }
        }
        self.run_host_microtask_checkpoint()?;
        if self.browser_tasks.is_empty() {
            self.browser_tasks.shrink_to_fit();
        }
        self.synchronize_scheduling_usage()?;
        Ok(())
    }

    pub(super) fn cancel_file_read_tasks(&mut self, reader: ObjectId) -> Result<(), RuntimeError> {
        let mut index = 0;
        while index < self.browser_tasks.len() {
            if matches!(&self.browser_tasks[index], BrowserTask::FileRead { reader: owner, .. } if *owner == reader)
            {
                let BrowserTask::FileRead { root, .. } = self.browser_tasks.remove(index).unwrap()
                else {
                    unreachable!("selected FileReader task")
                };
                self.heap.unroot(root)?;
            } else {
                index += 1;
            }
        }
        if self.browser_tasks.is_empty() {
            self.browser_tasks.shrink_to_fit();
        }
        self.synchronize_scheduling_usage()?;
        Ok(())
    }
}
