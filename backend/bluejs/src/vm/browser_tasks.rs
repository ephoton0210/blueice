// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser task delivery is separate from Promise microtask checkpoints.

use super::*;

pub(super) enum BrowserTask {
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
        let root = self.heap.root(callback)?;
        self.browser_tasks.push_back(BrowserTask::DragString {
            callback,
            text,
            root,
        });
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
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(false);
        }
        self.run_promise_jobs_bounded(1024)?;
        for _ in 0..max_tasks {
            let Some(task) = self.browser_tasks.pop_front() else {
                return Ok(true);
            };
            self.remaining_instructions = self.config.instruction_budget;
            let (result, root, next, report_page_exception) = match task {
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
                    (result, root, None, true)
                }
                BrowserTask::FileRead {
                    reader,
                    epoch,
                    stage,
                    root,
                } => match self.file_read_task(reader, epoch, stage) {
                    Ok(next) => (
                        Ok(()),
                        root,
                        next.map(|stage| BrowserTask::FileRead {
                            reader,
                            epoch,
                            stage,
                            root,
                        }),
                        false,
                    ),
                    Err(error) => (Err(error), root, None, false),
                },
            };
            if let Some(next) = next {
                self.browser_tasks.push_back(next);
            } else {
                self.heap.unroot(root)?;
            }
            if let Err(error) = result {
                if !report_page_exception || !super::web_drag_drop::drag_page_exception(&error) {
                    return Err(error);
                }
            }
            self.run_promise_jobs_bounded(1024)?;
        }
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
        Ok(())
    }
}
