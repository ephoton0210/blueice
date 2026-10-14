// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-owned task sources for one page realm: `setTimeout`,
//! `setInterval`, `clearTimeout`, `clearInterval` and `queueMicrotask`.
//!
//! The VM has no clock and no thread. The embedding host supplies the
//! current monotonic time in milliseconds when it calls
//! [`Vm::run_due_host_timers`], and every callback and its bound arguments
//! stay in this VM's rooted table: a function object never crosses the
//! primitive-only embedding ABI, exactly like a click listener.
//!
//! Each timer callback is one task, followed by one microtask checkpoint,
//! matching the HTML event loop's "run a task, then perform a microtask
//! checkpoint" shape (`research/js-bytecode-eventloop.md`).

use super::*;

/// Maximum pending timers in one realm. Further `setTimeout`/`setInterval`
/// calls throw a `RangeError` until a timer fires or is cleared.
pub const MAX_HOST_TIMERS_PER_REALM: usize = 1_024;

/// Jobs one host microtask checkpoint may run before it reports a limit.
const MAX_HOST_CHECKPOINT_JOBS: usize = 1_024;

/// Largest delay in milliseconds, matching the 2^31-1 limit browsers apply.
const MAX_HOST_TIMER_DELAY_MS: f64 = 2_147_483_647.0;

/// Which host timer function a native call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostTimerFunction {
    SetTimeout,
    SetInterval,
    Clear,
    QueueMicrotask,
}

/// Result of one [`Vm::run_due_host_timers`] pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlueJsTimerRun {
    /// Timer callbacks that were invoked.
    pub tasks_run: u32,
    /// Callbacks, or microtasks they queued, that ended in an uncaught page
    /// exception. Such an exception ends only that callback.
    pub uncaught_errors: u32,
}

pub(super) struct HostTimer {
    id: u32,
    callback: ObjectId,
    arguments: Vec<Value>,
    due_ms: u64,
    interval_ms: Option<u64>,
    /// One registration for the callback and one per object argument.
    roots: Vec<RootId>,
}

/// An uncaught page exception, as opposed to a resource failure that must
/// stop the host's pass.
fn is_page_exception(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Thrown(_)
            | RuntimeError::ReferenceError(_)
            | RuntimeError::TypeError(_)
            | RuntimeError::RangeError(_)
            | RuntimeError::SyntaxError(_)
    )
}

impl Vm {
    pub(crate) fn use_bounded_host_microtask_checkpoints(&mut self) {
        self.bounded_host_microtask_checkpoints = true;
    }

    /// Installs `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`
    /// and `queueMicrotask` as globals. Installation is all-or-nothing and is
    /// refused when any of those names already exists.
    pub fn install_host_timer_functions(&mut self) -> Result<(), RuntimeError> {
        const FUNCTIONS: [(&str, u32, HostTimerFunction); 5] = [
            ("setTimeout", 1, HostTimerFunction::SetTimeout),
            ("setInterval", 1, HostTimerFunction::SetInterval),
            ("clearTimeout", 1, HostTimerFunction::Clear),
            ("clearInterval", 1, HostTimerFunction::Clear),
            ("queueMicrotask", 1, HostTimerFunction::QueueMicrotask),
        ];
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is always an object");
        for (name, _, _) in FUNCTIONS {
            if self
                .heap
                .get_own(global, name)
                .expect("the materialized realm global remains rooted during host installation")
                .is_some()
            {
                return Err(RuntimeError::TypeError(
                    "host global name is invalid or already defined".into(),
                ));
            }
        }
        self.function_prototype()?;
        for (index, (name, length, function)) in FUNCTIONS.iter().enumerate() {
            let result = self.install_host_callable_native(
                global,
                name,
                *length,
                NativeFunction::HostTimer(*function),
            );
            if result.is_err() {
                // The earlier names are configurable own properties and no
                // script ran, so a failed installation can be retried whole.
                for (installed, _, _) in &FUNCTIONS[..index] {
                    self.heap
                        .delete(global, *installed)
                        .expect("the global owns its freshly installed configurable function");
                }
                return result;
            }
        }
        Ok(())
    }

    /// The earliest due time among pending timers, in the host's millisecond
    /// clock, or `None` when no timer is pending.
    pub fn next_host_timer_due(&self) -> Option<u64> {
        self.host_timers.iter().map(|timer| timer.due_ms).min()
    }

    /// Runs every timer due at `now_ms` as its own task, in (due time,
    /// creation) order, with a microtask checkpoint after each.
    ///
    /// Only timers already due when the pass starts run in it: a timer
    /// scheduled by a callback waits for the next call, so a callback that
    /// always re-arms itself cannot hold the host inside one pass. A late
    /// interval runs once and is rescheduled `interval` after `now_ms`
    /// rather than replaying every missed period.
    pub fn run_due_host_timers(&mut self, now_ms: u64) -> Result<BlueJsTimerRun, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        self.host_timer_clock_ms = self.host_timer_clock_ms.max(now_ms);
        let now = self.host_timer_clock_ms;
        let mut due = self
            .host_timers
            .iter()
            .filter(|timer| timer.due_ms <= now)
            .map(|timer| (timer.due_ms, timer.id))
            .collect::<Vec<_>>();
        due.sort_unstable();
        let mut run = BlueJsTimerRun::default();
        for (_, id) in due {
            // A callback earlier in this pass may have cleared this timer.
            let Some(index) = self.host_timers.iter().position(|timer| timer.id == id) else {
                continue;
            };
            let (callback, arguments) = {
                let timer = &mut self.host_timers[index];
                if let Some(interval) = timer.interval_ms {
                    timer.due_ms = now.saturating_add(interval);
                }
                (timer.callback, timer.arguments.clone())
            };
            if self.host_timers[index].interval_ms.is_none() {
                let timer = self.host_timers.remove(index);
                // The snapshot roots below keep the callback alive while the
                // timer's own registrations are released.
                let snapshot = self.root_timer_task(callback, &arguments);
                self.release_timer_roots(timer.roots);
                let snapshot = snapshot?;
                let outcome = self.run_timer_task(callback, arguments, &mut run);
                self.release_timer_roots(snapshot);
                outcome?;
            } else {
                let snapshot = self.root_timer_task(callback, &arguments)?;
                let outcome = self.run_timer_task(callback, arguments, &mut run);
                self.release_timer_roots(snapshot);
                outcome?;
            }
        }
        Ok(run)
    }

    fn root_timer_task(
        &mut self,
        callback: ObjectId,
        arguments: &[Value],
    ) -> Result<Vec<RootId>, RuntimeError> {
        let mut roots = Vec::with_capacity(arguments.len() + 1);
        let objects = std::iter::once(callback)
            .chain(arguments.iter().filter_map(Value::object_id))
            .collect::<Vec<_>>();
        for object in objects {
            match self.heap.root(object) {
                Ok(root) => roots.push(root),
                Err(error) => {
                    self.release_timer_roots(roots);
                    return Err(error.into());
                }
            }
        }
        Ok(roots)
    }

    fn release_timer_roots(&mut self, roots: Vec<RootId>) {
        for root in roots {
            self.heap
                .unroot(root)
                .expect("a timer task releases only registrations it created");
        }
    }

    fn run_timer_task(
        &mut self,
        callback: ObjectId,
        arguments: Vec<Value>,
        run: &mut BlueJsTimerRun,
    ) -> Result<(), RuntimeError> {
        self.remaining_instructions = self.config.instruction_budget;
        run.tasks_run += 1;
        match self.call_native(Value::Object(callback), Value::Undefined, arguments, false) {
            Ok(_) => {}
            Err(error) if is_page_exception(&error) => run.uncaught_errors += 1,
            Err(error) => return Err(error),
        }
        run.uncaught_errors += self.run_host_microtask_checkpoint()?;
        Ok(())
    }

    /// Drains the job queue as one HTML "perform a microtask checkpoint".
    /// A job that throws a page exception is counted and the checkpoint
    /// continues with the next job; a resource failure stops it. Returns the
    /// number of uncaught page exceptions.
    ///
    /// The checkpoint is bounded like the host's click checkpoint: a chain
    /// longer than [`MAX_HOST_CHECKPOINT_JOBS`] stops the pass with
    /// `InstructionLimit` instead of holding the host indefinitely.
    pub fn run_host_microtask_checkpoint(&mut self) -> Result<u32, RuntimeError> {
        self.run_host_microtask_checkpoint_bounded(MAX_HOST_CHECKPOINT_JOBS)
    }

    pub(crate) fn run_host_microtask_checkpoint_bounded(
        &mut self,
        max_jobs: usize,
    ) -> Result<u32, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        let mut uncaught = 0;
        for _ in 0..max_jobs {
            self.remaining_instructions = self.config.instruction_budget;
            match self.run_next_promise_job() {
                Ok(true) => {}
                Ok(false) => return Ok(uncaught),
                Err(error) if is_page_exception(&error) => uncaught += 1,
                Err(error) => return Err(error),
            }
        }
        if self.promise_jobs.is_empty() {
            Ok(uncaught)
        } else {
            Err(RuntimeError::InstructionLimit)
        }
    }

    pub(super) fn host_timer_call(
        &mut self,
        function: HostTimerFunction,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "timer functions are not constructors".into(),
            ));
        }
        match function {
            HostTimerFunction::SetTimeout => self.host_timer_schedule(args, false),
            HostTimerFunction::SetInterval => self.host_timer_schedule(args, true),
            HostTimerFunction::Clear => {
                let id = match args.first() {
                    Some(value) => self.coerce_number(value)?,
                    None => f64::NAN,
                };
                if let Some(index) = self
                    .host_timers
                    .iter()
                    .position(|timer| f64::from(timer.id) == id)
                {
                    let timer = self.host_timers.remove(index);
                    self.release_timer_roots(timer.roots);
                }
                Ok(Value::Undefined)
            }
            HostTimerFunction::QueueMicrotask => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if !self.is_callable(&callback)? {
                    return Err(RuntimeError::TypeError(
                        "queueMicrotask requires a callable callback".into(),
                    ));
                }
                self.promise_jobs
                    .push_back(PromiseJob::HostMicrotask { callback });
                Ok(Value::Undefined)
            }
        }
    }

    fn host_timer_schedule(&mut self, args: &[Value], repeat: bool) -> Result<Value, RuntimeError> {
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        if !self.is_callable(&callback)? {
            return Err(RuntimeError::TypeError(
                "timer callback must be callable".into(),
            ));
        }
        let callback = callback
            .object_id()
            .expect("a successfully classified callable value is an object");
        let delay = match args.get(1) {
            Some(value) => self.coerce_number(value)?,
            None => 0.0,
        };
        let delay_ms = if delay.is_nan() || delay < 0.0 {
            0
        } else {
            delay.min(MAX_HOST_TIMER_DELAY_MS).trunc() as u64
        };
        if self.host_timers.len() >= MAX_HOST_TIMERS_PER_REALM {
            return Err(RuntimeError::RangeError("host timer limit exceeded".into()));
        }
        let id = self.host_timer_next_id;
        let Some(next) = id.checked_add(1) else {
            return Err(RuntimeError::RangeError(
                "host timer identifiers are exhausted".into(),
            ));
        };
        let arguments = args.iter().skip(2).cloned().collect::<Vec<_>>();
        let roots = self.root_timer_task(callback, &arguments)?;
        self.host_timer_next_id = next;
        self.host_timers.push(HostTimer {
            id,
            callback,
            arguments,
            due_ms: self.host_timer_clock_ms.saturating_add(delay_ms),
            interval_ms: repeat.then_some(delay_ms),
            roots,
        });
        Ok(Value::Number(f64::from(id)))
    }
}
