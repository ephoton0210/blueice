// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Web IDL callback cleanup performs a checkpoint when the script stack is
//! empty. A script's synchronous dispatch keeps its enclosing native call.
//! https://html.spec.whatwg.org/multipage/webappapis.html#clean-up-after-running-script

use super::*;

impl Vm {
    pub(crate) const MAX_HOST_EVENT_MICROTASK_JOBS: usize = 256;

    pub(super) fn starts_host_event_turn(&self) -> bool {
        self.bounded_host_microtask_checkpoints
            && self.call_depth == 0
            && self.host_error_reporting != Some(true)
            && self.host_event_microtask_jobs.is_none()
    }

    pub(crate) fn run_host_event_turn<T>(
        &mut self,
        dispatch: impl FnOnce(&mut Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if !self.starts_host_event_turn() {
            return dispatch(self);
        }
        self.check_scheduling_failure()?;
        self.ensure_no_debugger_continuation()?;
        self.host_event_microtask_jobs = Some(Self::MAX_HOST_EVENT_MICROTASK_JOBS);
        let result = dispatch(self);
        let checkpoint = if result.is_ok() {
            self.run_host_event_checkpoint().map(|_| ())
        } else {
            Ok(())
        };
        self.host_event_microtask_jobs = None;
        checkpoint?;
        result
    }

    pub(super) fn cleanup_host_event_callback(
        &mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        if let Err(error) = &result {
            self.report_host_exception(error)?;
        }
        if self.call_depth != 0
            || self.host_error_reporting == Some(true)
            || self.host_event_microtask_jobs.is_none()
            || result
                .as_ref()
                .is_err_and(|error| !host_timers::is_page_exception(error))
        {
            return result;
        }
        // A once listener may already have lost its registration root. Keep
        // the completion alive independently while cleanup can collect.
        let value = match &result {
            Ok(value) | Err(RuntimeError::Thrown(value)) => Some(value),
            _ => None,
        };
        let root_base = self.async_frame_roots.len();
        self.extend_async_frame_roots(value.and_then(Value::object_id).into_iter())?;
        let bytes = value.map_or(0, host_scheduling::value_payload_bytes);
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_add(bytes);
        let checkpoint = self
            .synchronize_scheduling_usage()
            .and_then(|()| self.run_host_event_checkpoint());
        self.async_frame_roots.truncate(root_base);
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_sub(bytes);
        self.synchronize_scheduling_usage()?;
        checkpoint?;
        result
    }

    fn run_host_event_checkpoint(&mut self) -> Result<u32, RuntimeError> {
        let mut jobs = self
            .host_event_microtask_jobs
            .take()
            .expect("active native dispatch");
        // Import/module jobs can replace the interpreter's operand stack and
        // bindings. The native dispatch still owns event/path/callback values
        // in that frame, so displace and trace it just like a suspended module.
        // Restoring the frame also preserves the dispatch's instruction fuel.
        let execution = self.suspend_module_execution();
        let root_base = self.async_frame_roots.len();
        if let Err(error) =
            self.extend_async_frame_roots(Self::suspended_execution_references(&execution))
        {
            self.restore_module_execution(execution);
            self.host_event_microtask_jobs = Some(jobs);
            return Err(error);
        }
        let result = self.run_host_microtask_checkpoint_with_budget(&mut jobs);
        if let Some(root) = self.result_root.take() {
            self.release_root(root);
        }
        self.restore_module_execution(execution);
        self.async_frame_roots.truncate(root_base);
        self.host_event_microtask_jobs = Some(jobs);
        result
    }
}
