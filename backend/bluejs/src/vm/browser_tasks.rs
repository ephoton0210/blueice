// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser task delivery is separate from Promise microtask checkpoints.

use super::*;

pub(super) enum BrowserTask {
    AbortTimeout {
        signal: ObjectId,
        started: std::time::Instant,
        delay: std::time::Duration,
        root: Option<RootId>,
    },
    DragString {
        callback: ObjectId,
        text: JsString,
        root: RootId,
    },
    FileRead {
        reader: ObjectId,
        epoch: u64,
        stage: u8,
        root: RootId,
    },
}

impl Vm {
    fn browser_task_wake(&self, task: &BrowserTask) -> u64 {
        if let BrowserTask::AbortTimeout {
            signal,
            started,
            delay,
            ..
        } = task
        {
            if self
                .heap
                .abort_signal(*signal)
                .is_ok_and(|state| !state.aborted())
            {
                let remaining = delay.saturating_sub(started.elapsed());
                return u64::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap_or(u64::MAX);
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

    pub(super) fn has_runnable_browser_task(&self) -> bool {
        self.next_own_browser_task_wake() == Some(0)
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
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_browser_tasks: 1,
            payload_bytes: text
                .owned_heap_payload_bytes()
                .unwrap_or(usize::MAX)
                .saturating_add(4 * std::mem::size_of::<BrowserTask>()),
            ..HostSchedulingUsage::default()
        })?;
        let root = self.heap.root(callback)?;
        self.browser_tasks.push_back(BrowserTask::DragString {
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
            let task = self
                .browser_tasks
                .remove(index)
                .expect("a runnable task was selected");
            let active_bytes = match &task {
                BrowserTask::DragString { text, .. } => {
                    text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
                }
                _ => 0,
            };
            self.active_scheduling_bytes =
                self.active_scheduling_bytes.saturating_add(active_bytes);
            self.synchronize_scheduling_usage()?;
            self.remaining_instructions = self.config.instruction_budget;
            let (result, root, next, report_page_exception) = match task {
                BrowserTask::AbortTimeout {
                    signal,
                    started,
                    delay,
                    root,
                } => match self.abort_timeout_task(signal, started, delay) {
                    Ok(done) => (
                        Ok(()),
                        root,
                        (!done).then_some(BrowserTask::AbortTimeout {
                            signal,
                            started,
                            delay,
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
                } => {
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
                } => match self.file_read_task(reader, epoch, stage) {
                    Ok(next) => (
                        Ok(()),
                        Some(root),
                        next.map(|stage| BrowserTask::FileRead {
                            reader,
                            epoch,
                            stage,
                            root,
                        }),
                        false,
                    ),
                    Err(error) => (Err(error), Some(root), None, false),
                },
            };
            self.active_scheduling_bytes =
                self.active_scheduling_bytes.saturating_sub(active_bytes);
            if let Some(next) = next {
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
        }
        if self.browser_tasks.is_empty() {
            self.browser_tasks.shrink_to_fit();
        }
        self.synchronize_scheduling_usage()?;
        Ok(self.browser_tasks.is_empty())
    }

    pub(super) fn cancel_file_read_tasks(&mut self, reader: ObjectId) -> Result<(), RuntimeError> {
        let mut retained = VecDeque::new();
        while let Some(task) = self.browser_tasks.pop_front() {
            match task {
                BrowserTask::FileRead {
                    reader: owner,
                    root,
                    ..
                } if owner == reader => {
                    self.heap.unroot(root)?;
                }
                task => retained.push_back(task),
            }
        }
        self.browser_tasks = retained;
        self.synchronize_scheduling_usage()?;
        Ok(())
    }
}
