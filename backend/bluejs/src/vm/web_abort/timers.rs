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
        let context = AbortContext::capture(self)?;
        context.with_roots(self, args[0].object_id().into_iter(), |vm| {
            let converted = vm.coerce_number(&args[0]);
            let number = context.validate().and(converted)?.trunc();
            if !number.is_finite() || !(0.0..18_446_744_073_709_551_616.0).contains(&number) {
                return Err(RuntimeError::TypeError(
                    "Timeout is outside unsigned long long range".into(),
                ));
            }
            if vm.browser_tasks.len() >= 64 {
                return Err(RuntimeError::RangeError(
                    "Browser task queue limit exceeded".into(),
                ));
            }
            let deadline = if vm.host_scheduling.is_some() {
                AbortTimeoutDeadline::Host(
                    vm.sample_host_timer_clock().saturating_add(number as u64),
                )
            } else {
                AbortTimeoutDeadline::Monotonic {
                    started: Instant::now(),
                    delay: Duration::from_millis(number as u64),
                }
            };
            context.validate()?;
            let prepared = vm.prepare_browser_task(HostSchedulingUsage::default())?;
            let signal = vm.fresh_abort_signal()?;
            context.validate()?;
            let serial = vm.next_host_task_serial()?;
            prepared.commit(
                vm,
                BrowserTask::AbortTimeout {
                    signal,
                    deadline,
                    serial,
                    root: None,
                },
            );
            vm.synchronize_scheduling_usage()?;
            Ok(Value::Object(signal))
        })
    }
    pub(in crate::vm) fn refresh_abort_timer_roots(
        &mut self,
        _changed: ObjectId,
    ) -> Result<(), RuntimeError> {
        AbortContext::check_affinity(self)?;
        // Returning obsolete roots needs no admission. Complete this pass even
        // when the account cannot prepare a new root in the next pass.
        for task in &mut self.browser_tasks {
            if let BrowserTask::AbortTimeout { signal, root, .. } = task {
                if !timeout_observed(&self.heap, *signal) {
                    if let Some(released) = root.take() {
                        self.heap.unroot(released)?;
                    }
                }
            }
        }
        let context = AbortContext::capture(self)?;
        let result = (|| {
            for task in &mut self.browser_tasks {
                if let BrowserTask::AbortTimeout { signal, root, .. } = task {
                    if root.is_none() && timeout_observed(&self.heap, *signal) {
                        *root = Some(self.heap.root(*signal)?);
                    }
                }
            }
            Ok(())
        })();
        context.validate().and(result)
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

// No script, collection or task mutation occurs between the two refresh passes.
fn timeout_observed(heap: &Heap, signal: ObjectId) -> bool {
    heap.abort_signal(signal).is_ok_and(|state| {
        !state.aborted()
            && (state.observed()
                || state.dependents.iter().any(|dependent| {
                    heap.abort_signal(*dependent)
                        .is_ok_and(|state| !state.aborted() && state.observed())
                }))
    })
}
