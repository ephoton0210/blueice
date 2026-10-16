// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frame callbacks are a render entry, separate from browser tasks/timers.
//! The embedding host owns rendering cadence and supplies the timestamp.
//! https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#animation-frames

use super::host_timers::{is_page_exception, unsigned_long};
use super::*;

/// Bounds both retained callbacks and work in one frame's complete snapshot.
pub const MAX_HOST_ANIMATION_FRAME_CALLBACKS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostAnimationFrameFunction {
    Request,
    Cancel,
}

pub(super) struct HostAnimationFrame {
    handle: u32,
    callback: ObjectId,
    root: RootId,
}

/// Outcome of one render opportunity; uncaught page errors do not stop peers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlueJsAnimationFrameRun {
    pub callbacks_run: u32,
    pub uncaught_errors: u32,
}

impl Vm {
    /// Installs the two frame functions atomically. Neither is a constructor;
    /// ordinary timer/browser turns never invoke their callbacks.
    pub fn install_host_animation_frame_functions(&mut self) -> Result<(), RuntimeError> {
        const FUNCTIONS: [(&str, HostAnimationFrameFunction); 2] = [
            ("requestAnimationFrame", HostAnimationFrameFunction::Request),
            ("cancelAnimationFrame", HostAnimationFrameFunction::Cancel),
        ];
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("realm global is an object");
        for (name, _) in FUNCTIONS {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "host global name is invalid or already defined".into(),
                ));
            }
        }
        self.function_prototype()?;
        for (index, (name, function)) in FUNCTIONS.iter().enumerate() {
            if let Err(error) = self.install_host_callable_native(
                global,
                name,
                1,
                NativeFunction::HostAnimationFrame(*function),
            ) {
                for (installed, _) in &FUNCTIONS[..index] {
                    self.heap
                        .delete(global, *installed)
                        .expect("fresh configurable frame function");
                }
                return Err(error);
            }
        }
        Ok(())
    }

    /// Paused realms keep roots without asking the host to spin a frame loop.
    pub fn host_animation_frame_pending(&self) -> bool {
        self.ensure_no_debugger_continuation().is_ok() && !self.host_animation_frames.is_empty()
    }

    pub(super) fn host_animation_frame_call(
        &mut self,
        function: HostAnimationFrameFunction,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "frame functions are not constructors".into(),
            ));
        }
        let argument = args
            .first()
            .ok_or_else(|| RuntimeError::TypeError("frame function requires an argument".into()))?;
        match function {
            HostAnimationFrameFunction::Request => {
                if !self.is_callable(argument)? {
                    return Err(RuntimeError::TypeError(
                        "frame callback must be callable".into(),
                    ));
                }
                if self.host_animation_frames.len() >= MAX_HOST_ANIMATION_FRAME_CALLBACKS {
                    return Err(RuntimeError::RangeError(
                        "frame callback limit exceeded".into(),
                    ));
                }
                let handle = u32::try_from(self.host_animation_frame_next_id).map_err(|_| {
                    RuntimeError::RangeError("frame identifiers are exhausted".into())
                })?;
                let callback = argument.object_id().expect("callable is an object");
                let extra = HostSchedulingUsage::default();
                let growth = SchedulingCapacity::for_vec_push(&self.host_animation_frames);
                self.admit_scheduling_work(extra.with_growth(growth))?;
                let root = self.heap.root(callback)?;
                if growth.reserve_vec(&mut self.host_animation_frames) {
                    if let Err(error) = self.admit_scheduling_work(extra) {
                        self.release_root(root);
                        return Err(error);
                    }
                }
                self.host_animation_frame_next_id += 1;
                self.host_animation_frames.push(HostAnimationFrame {
                    handle,
                    callback,
                    root,
                });
                self.synchronize_scheduling_usage()?;
                Ok(Value::Number(f64::from(handle)))
            }
            HostAnimationFrameFunction::Cancel => {
                let handle = unsigned_long(self.coerce_number(argument)?);
                if let Some(index) = self
                    .host_animation_frames
                    .iter()
                    .position(|frame| frame.handle == handle)
                {
                    let frame = self.host_animation_frames.remove(index);
                    self.heap
                        .unroot(frame.root)
                        .expect("frame registration owns its root");
                }
                if self.host_animation_frames.is_empty() {
                    self.host_animation_frames.shrink_to_fit();
                }
                self.synchronize_scheduling_usage()?;
                Ok(Value::Undefined)
            }
        }
    }

    /// Runs the registration-order handle snapshot with one finite,
    /// non-negative, monotonic timestamp. New registrations wait for another
    /// frame; cancellation (including by a checkpoint) affects this snapshot.
    /// Callback roots are kept through invocation and released on all outcomes.
    /// Each callback and the render exit perform bounded job checkpoints.
    pub fn run_host_animation_frame(
        &mut self,
        timestamp_ms: f64,
    ) -> Result<BlueJsAnimationFrameRun, RuntimeError> {
        self.check_scheduling_failure()?;
        if !timestamp_ms.is_finite() || timestamp_ms < 0.0 {
            return Err(RuntimeError::RangeError(
                "frame timestamp must be finite and non-negative".into(),
            ));
        }
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(BlueJsAnimationFrameRun::default());
        }
        self.host_animation_frame_timestamp_ms =
            self.host_animation_frame_timestamp_ms.max(timestamp_ms);
        let timestamp = self.host_animation_frame_timestamp_ms;
        let handles = self
            .host_animation_frames
            .iter()
            .map(|frame| frame.handle)
            .collect::<Vec<_>>();
        let mut run = BlueJsAnimationFrameRun::default();
        for handle in handles {
            let Some(index) = self
                .host_animation_frames
                .iter()
                .position(|frame| frame.handle == handle)
            else {
                continue;
            };
            let frame = self.host_animation_frames.remove(index);
            self.synchronize_scheduling_usage()?;
            self.remaining_instructions = self.config.instruction_budget;
            run.callbacks_run += 1;
            let result = self.call_native(
                Value::Object(frame.callback),
                Value::Undefined,
                vec![Value::Number(timestamp)],
                false,
            );
            self.heap
                .unroot(frame.root)
                .expect("invoked callback owns its frame root");
            match result {
                Ok(_) => {}
                Err(error) if is_page_exception(&error) => {
                    self.report_host_exception(&error)?;
                    run.uncaught_errors += 1;
                }
                Err(error) => return Err(error),
            }
            run.uncaught_errors += self.run_host_microtask_checkpoint()?;
        }
        run.uncaught_errors += self.run_host_microtask_checkpoint()?;
        if self.host_animation_frames.is_empty() {
            self.host_animation_frames.shrink_to_fit();
        }
        self.synchronize_scheduling_usage()?;
        Ok(run)
    }
}
