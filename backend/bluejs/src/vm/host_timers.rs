// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-owned task sources for one page realm: `setTimeout`,
//! `setInterval`, `clearTimeout`, `clearInterval` and `queueMicrotask`.
//!
//! The VM has no clock and no thread. The embedding host supplies the
//! current monotonic time in milliseconds through an injected clock or when
//! it calls [`Vm::run_due_host_timers`], and every callback and its bound arguments
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

/// A turn yields with pending timers intact, independent of the realm cap.
const MAX_HOST_TIMER_TASKS_PER_TURN: usize = 64;
const MAX_HOST_TIMER_ARGUMENTS: usize = 32;

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
    pub(super) id: u32,
    callback: ObjectId,
    arguments: Vec<Value>,
    pub(super) due_ms: u64,
    pub(super) serial: u64,
    interval_ms: Option<u64>,
    nesting: u32,
    /// One registration for the callback and one per object argument.
    roots: Vec<RootId>,
}

/// An uncaught page exception, as opposed to a resource failure that must
/// stop the host's pass.
pub(super) fn is_page_exception(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Thrown(_)
            | RuntimeError::ReferenceError(_)
            | RuntimeError::TypeError(_)
            | RuntimeError::RangeError(_)
            | RuntimeError::SyntaxError(_)
    )
}

/// Web IDL's wraparound unsigned `long` conversion, shared with frame handles.
pub(super) fn unsigned_long(number: f64) -> u32 {
    if !number.is_finite() || number == 0.0 {
        return 0;
    }
    number.trunc().rem_euclid(4_294_967_296.0) as u32
}

fn timer_long(number: f64) -> i32 {
    unsigned_long(number) as i32
}

fn nested_delay(delay: u64, nesting: u32) -> u64 {
    if nesting > 5 {
        delay.max(4)
    } else {
        delay
    }
}

impl Vm {
    pub(crate) fn use_bounded_host_microtask_checkpoints(&mut self) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            self.check_scheduling_failure()?;
            self.symbol_registry.borrow_mut().attach(scope)?;
            self.bounded_host_microtask_checkpoints = true;
            return Ok(());
        }
        let mut scope = SchedulingScope::new();
        scope.check(self.own_scheduling_usage(), HostSchedulingUsage::default())?;
        self.symbol_registry.borrow_mut().attach(&scope)?;
        self.heap.begin_document_heap_budget();
        self.bounded_host_microtask_checkpoints = true;
        self.host_scheduling = Some(scope);
        Ok(())
    }

    /// Installs `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`
    /// and `queueMicrotask` as globals. Installation is all-or-nothing and is
    /// refused when any of those names already exists.
    pub fn install_host_timer_functions(&mut self) -> Result<(), RuntimeError> {
        const FUNCTIONS: [(&str, u32, HostTimerFunction); 5] = [
            ("setTimeout", 1, HostTimerFunction::SetTimeout),
            ("setInterval", 1, HostTimerFunction::SetInterval),
            ("clearTimeout", 0, HostTimerFunction::Clear),
            ("clearInterval", 0, HostTimerFunction::Clear),
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

    /// Installs host timers and samples the embedding host's clock whenever
    /// a timer is scheduled or an interval is rearmed. No thread is created.
    pub fn install_host_timer_functions_with_clock(
        &mut self,
        clock: impl Fn() -> u64 + 'static,
    ) -> Result<(), RuntimeError> {
        self.install_host_timer_functions()?;
        self.host_timer_clock = Some(Box::new(clock));
        Ok(())
    }

    pub(super) fn sample_host_timer_clock(&mut self) -> u64 {
        if let Some(clock) = &self.host_timer_clock {
            self.host_timer_clock_ms = self.host_timer_clock_ms.max(clock());
        }
        if let Some(scope) = &self.host_scheduling {
            self.host_timer_clock_ms = scope.advance_clock(self.host_timer_clock_ms);
        }
        self.host_timer_clock_ms
    }

    /// The earliest due time among pending timers, in the host's millisecond
    /// clock, or `None` when no timer is runnable (empty or debugger-paused).
    pub fn next_host_timer_due(&self) -> Option<u64> {
        // A paused realm retains timers, but has no runnable wake-up until
        // its debugger-owned continuation resumes.
        if self.ensure_no_debugger_continuation().is_err() {
            return None;
        }
        self.host_timers.iter().map(|timer| timer.due_ms).min()
    }

    /// Runs at most 64 timers due at `now_ms` as their own tasks, in (due time,
    /// creation) order, with a microtask checkpoint after each.
    ///
    /// Only timers already due when the pass starts run in it: a timer
    /// scheduled by a callback waits for the next call, so a callback that
    /// always re-arms itself cannot hold the host inside one pass. A late
    /// interval runs once and is rescheduled after its callback and checkpoint
    /// rather than replaying every missed period. Debugger pauses retain the
    /// queue and return an empty run until the continuation resumes.
    pub fn run_due_host_timers(&mut self, now_ms: u64) -> Result<BlueJsTimerRun, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(BlueJsTimerRun::default());
        }
        let due = self.snapshot_due_host_timers(now_ms);
        let mut run = BlueJsTimerRun::default();
        for id in due.into_iter().take(MAX_HOST_TIMER_TASKS_PER_TURN) {
            self.run_host_timer_entry(id, &mut run)?;
        }
        Ok(run)
    }

    pub(super) fn snapshot_due_host_timers(&mut self, now_ms: u64) -> Vec<u32> {
        self.host_timer_clock_ms = self.host_timer_clock_ms.max(now_ms);
        let now = self.sample_host_timer_clock();
        let mut due: Vec<_> = self
            .host_timers
            .iter()
            .filter(|timer| timer.due_ms <= now)
            .map(|timer| (timer.due_ms, timer.id))
            .collect();
        due.sort_unstable();
        due.into_iter().map(|(_, id)| id).collect()
    }

    pub(super) fn run_host_timer_entry(
        &mut self,
        id: u32,
        run: &mut BlueJsTimerRun,
    ) -> Result<(), RuntimeError> {
        // Earlier task callbacks and their checkpoints may have canceled it.
        let Some(index) = self.host_timers.iter().position(|timer| timer.id == id) else {
            return Ok(());
        };
        let interval = self.host_timers[index].interval_ms;
        let (callback, arguments, nesting, snapshot) = if interval.is_none() {
            let timer = self.host_timers.remove(index);
            (timer.callback, timer.arguments, timer.nesting, timer.roots)
        } else {
            let timer = &self.host_timers[index];
            // The registration stays alive while the interval runs; its
            // argument copy must also fit before it is allocated.
            let bytes = timer.scheduling_payload_bytes();
            self.admit_scheduling_work(HostSchedulingUsage {
                payload_bytes: bytes,
                ..HostSchedulingUsage::default()
            })?;
            let timer = &self.host_timers[index];
            let (callback, arguments, nesting) =
                (timer.callback, timer.arguments.clone(), timer.nesting);
            let roots = self.root_timer_task(callback, &arguments)?;
            (callback, arguments, nesting, roots)
        };
        let active_bytes = arguments
            .capacity()
            .saturating_mul(std::mem::size_of::<Value>())
            .saturating_add(
                snapshot
                    .capacity()
                    .saturating_mul(std::mem::size_of::<RootId>()),
            )
            .saturating_add(arguments.iter().fold(0usize, |total, value| {
                total.saturating_add(super::host_scheduling::value_payload_bytes(value))
            }));
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_add(active_bytes);
        if self.host_timers.is_empty() {
            self.host_timers.shrink_to_fit();
        }
        let outcome = self
            .synchronize_scheduling_usage()
            .and_then(|()| self.run_timer_task(callback, arguments, nesting, run));
        self.release_timer_roots(snapshot);
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_sub(active_bytes);
        self.synchronize_scheduling_usage()?;
        outcome?;
        if let Some(interval) = interval {
            let now = self.sample_host_timer_clock();
            // A callback or its checkpoint can clear its own interval.
            let serial = self.next_host_task_serial()?;
            if let Some(timer) = self.host_timers.iter_mut().find(|timer| timer.id == id) {
                timer.serial = serial;
                timer.due_ms = now.saturating_add(nested_delay(interval, nesting));
                timer.nesting = nesting.saturating_add(1);
            }
        }
        Ok(())
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
        nesting: u32,
        run: &mut BlueJsTimerRun,
    ) -> Result<(), RuntimeError> {
        self.remaining_instructions = self.config.instruction_budget;
        run.tasks_run += 1;
        let this = self.global("globalThis")?;
        let previous_nesting = std::mem::replace(&mut self.host_timer_nesting, nesting);
        let outcome = self.call_native(Value::Object(callback), this, arguments, false);
        // Checkpoint jobs run after the timer task's running callback; they
        // are not timer tasks for HTML's nesting calculation.
        self.host_timer_nesting = previous_nesting;
        match outcome {
            Ok(_) => {}
            Err(error) if is_page_exception(&error) => {
                self.report_host_exception(&error)?;
                run.uncaught_errors += 1;
            }
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
        mut max_jobs: usize,
    ) -> Result<u32, RuntimeError> {
        self.run_host_microtask_checkpoint_with_budget(&mut max_jobs)
    }

    pub(super) fn run_host_microtask_checkpoint_with_budget(
        &mut self,
        remaining_jobs: &mut usize,
    ) -> Result<u32, RuntimeError> {
        self.check_scheduling_failure()?;
        self.ensure_no_debugger_continuation()?;
        self.enqueue_finalization_cleanup_jobs()?;
        let mut uncaught = 0;
        while *remaining_jobs > 0 {
            self.remaining_instructions = self.config.instruction_budget;
            match self.run_next_promise_job() {
                Ok(true) => *remaining_jobs -= 1,
                Ok(false) => {
                    self.finish_host_checkpoint()?;
                    return Ok(uncaught);
                }
                Err(error) if is_page_exception(&error) => {
                    *remaining_jobs -= 1;
                    self.report_host_exception(&error)?;
                    uncaught += 1;
                }
                Err(error) => return Err(error),
            }
            self.enqueue_finalization_cleanup_jobs()?;
            self.report_pending_host_module_errors()?;
        }
        if self.promise_jobs.is_empty() {
            self.finish_host_checkpoint()?;
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
                    Some(value) => timer_long(self.coerce_number(value)?),
                    None => 0,
                };
                if let Some(index) = self
                    .host_timers
                    .iter()
                    .position(|timer| i64::from(timer.id) == i64::from(id))
                {
                    let timer = self.host_timers.remove(index);
                    self.release_timer_roots(timer.roots);
                    if self.host_timers.is_empty() {
                        self.host_timers.shrink_to_fit();
                    }
                    self.synchronize_scheduling_usage()?;
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
                self.enqueue_promise_job(PromiseJob::HostMicrotask { callback });
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
        let delay_ms = timer_long(delay).max(0) as u64;
        if args.len().saturating_sub(2) > MAX_HOST_TIMER_ARGUMENTS {
            return Err(RuntimeError::RangeError(
                "host timer argument limit exceeded".into(),
            ));
        }
        if self.host_timers.len() >= MAX_HOST_TIMERS_PER_REALM {
            return Err(RuntimeError::RangeError("host timer limit exceeded".into()));
        }
        let id = self.host_timer_next_id;
        let Some(next) = id.checked_add(1).filter(|next| *next <= i32::MAX as u32) else {
            return Err(RuntimeError::RangeError(
                "host timer identifiers are exhausted".into(),
            ));
        };
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: std::mem::size_of::<HostTimer>()
                .saturating_mul(4)
                .saturating_add(
                    args.iter()
                        .skip(2)
                        .map(super::host_scheduling::value_payload_bytes)
                        .sum::<usize>(),
                )
                .saturating_add(
                    args.len().saturating_mul(
                        std::mem::size_of::<Value>() + std::mem::size_of::<RootId>(),
                    ),
                ),
            ..HostSchedulingUsage::default()
        })?;
        let serial = self.next_host_task_serial()?;
        let arguments = args.iter().skip(2).cloned().collect::<Vec<_>>();
        let roots = self.root_timer_task(callback, &arguments)?;
        let now = self.sample_host_timer_clock();
        self.host_timer_next_id = next;
        self.host_timers.push(HostTimer {
            id,
            callback,
            arguments,
            due_ms: now.saturating_add(nested_delay(delay_ms, self.host_timer_nesting)),
            serial,
            interval_ms: repeat.then_some(delay_ms),
            nesting: self.host_timer_nesting.saturating_add(1),
            roots,
        });
        self.synchronize_scheduling_usage()?;
        Ok(Value::Number(f64::from(id)))
    }
}

impl HostTimer {
    pub(super) fn scheduling_payload_bytes(&self) -> usize {
        self.arguments
            .capacity()
            .saturating_mul(std::mem::size_of::<Value>())
            .saturating_add(
                self.roots
                    .capacity()
                    .saturating_mul(std::mem::size_of::<RootId>()),
            )
            .saturating_add(
                self.arguments
                    .iter()
                    .map(super::host_scheduling::value_payload_bytes)
                    .sum::<usize>(),
            )
    }
}
