// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::vm::browser_tasks::{AbortTimeoutDeadline, BrowserTask};
use std::time::{Duration, Instant};

impl Vm {
    pub(in super::super) fn abort_signal_timeout(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if args.is_empty() {
            return Err(RuntimeError::TypeError(
                "AbortSignal.timeout requires milliseconds".into(),
            ));
        }
        let number = self.coerce_number(&args[0])?;
        let number = number.trunc();
        if !number.is_finite() || !(0.0..18_446_744_073_709_551_616.0).contains(&number) {
            return Err(RuntimeError::TypeError(
                "Timeout is outside unsigned long long range".into(),
            ));
        }
        if self.browser_tasks.len() >= 64 {
            return Err(RuntimeError::RangeError(
                "Browser task queue limit exceeded".into(),
            ));
        }
        let deadline = if self.host_scheduling.is_some() {
            AbortTimeoutDeadline::Host(self.sample_host_timer_clock().saturating_add(number as u64))
        } else {
            AbortTimeoutDeadline::Monotonic {
                started: Instant::now(),
                delay: Duration::from_millis(number as u64),
            }
        };
        self.reserve_browser_task(HostSchedulingUsage::default())?;
        let serial = self.next_host_task_serial()?;
        let signal = self.fresh_abort_signal()?;
        self.browser_tasks.push_back(BrowserTask::AbortTimeout {
            signal,
            deadline,
            serial,
            root: None,
        });
        self.synchronize_scheduling_usage()?;
        Ok(Value::Object(signal))
    }
    pub(in crate::vm) fn refresh_abort_timer_roots(
        &mut self,
        _changed: ObjectId,
    ) -> Result<(), RuntimeError> {
        let timers: Vec<_> = self
            .browser_tasks
            .iter()
            .filter_map(|task| match task {
                BrowserTask::AbortTimeout { signal, .. } => Some(*signal),
                _ => None,
            })
            .collect();
        for signal in timers {
            let needed = self.heap.abort_signal(signal).is_ok_and(|state| {
                !state.aborted()
                    && (state.observed()
                        || state.dependents.iter().any(|dependent| {
                            self.heap
                                .abort_signal(*dependent)
                                .is_ok_and(|state| !state.aborted() && state.observed())
                        }))
            });
            for task in &mut self.browser_tasks {
                if let BrowserTask::AbortTimeout {
                    signal: owner,
                    root,
                    ..
                } = task
                {
                    if *owner != signal {
                        continue;
                    }
                    if needed && root.is_none() {
                        *root = Some(self.heap.root(signal)?);
                    } else if !needed {
                        if let Some(released) = root.take() {
                            self.heap.unroot(released)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub(in crate::vm) fn abort_timeout_task(
        &mut self,
        signal: ObjectId,
        deadline: AbortTimeoutDeadline,
    ) -> Result<bool, RuntimeError> {
        let Ok(state) = self.heap.abort_signal(signal) else {
            return Ok(true);
        };
        if state.aborted() {
            return Ok(true);
        }
        if deadline.wake(self.sample_host_timer_clock()) > 0 {
            return Ok(false);
        }
        let base = self.stack.len();
        self.stack.push(Value::Object(signal));
        let result = (|| {
            let reason = self.abort_reason(&Value::Undefined, true)?;
            self.signal_abort(signal, reason)?;
            Ok(true)
        })();
        self.stack.truncate(base);
        result
    }
}
