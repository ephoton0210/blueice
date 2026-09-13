// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test262's multi-agent host is deliberately outside the ordinary realm.
//! Each agent owns an independent VM and Heap; SharedArrayBuffer wrappers are
//! the only values that cross the host boundary.

use super::*;
use crate::heap::{RootId, SharedBuffer};
use crate::{compile, parse};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Broadcast {
    backing: Arc<SharedBuffer>,
    max_byte_length: Option<usize>,
}

pub(super) struct Test262AgentControl {
    // Test262 permits an agent to receive more than one broadcast during its
    // lifetime.  A slot loses a second message when the host broadcasts again
    // before the agent reaches receiveBroadcast, so retain host message order
    // explicitly.
    broadcasts: Mutex<VecDeque<Broadcast>>,
    broadcast_ready: Condvar,
    leaving: AtomicBool,
}

/// A host-owned completion queue lets an operating-system waiter thread wake
/// a Promise without ever accessing the agent VM or its GC heap. Each VM has
/// its own queue; only the ObjectId is returned to its owning VM thread.
pub(super) struct Test262AsyncWaits {
    events: Mutex<VecDeque<AsyncHostEvent>>,
    ready: Condvar,
}

enum AsyncHostEvent {
    Wait {
        promise: ObjectId,
        result: crate::heap::SharedWaitResult,
    },
    Timer {
        callback: ObjectId,
        root: RootId,
    },
}

impl Test262AsyncWaits {
    pub(super) fn new() -> Self {
        Self {
            events: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        }
    }

    fn push(&self, event: AsyncHostEvent) {
        self.events
            .lock()
            .expect("Test262 async-wait queue lock poisoned")
            .push_back(event);
        self.ready.notify_one();
    }

    fn take_all(&self) -> Vec<AsyncHostEvent> {
        self.events
            .lock()
            .expect("Test262 async-wait queue lock poisoned")
            .drain(..)
            .collect()
    }
}

impl Test262AgentControl {
    fn new() -> Self {
        Self {
            broadcasts: Mutex::new(VecDeque::new()),
            broadcast_ready: Condvar::new(),
            leaving: AtomicBool::new(false),
        }
    }
}

struct AgentHostState {
    controls: Vec<Arc<Test262AgentControl>>,
    reports: VecDeque<JsString>,
    handles: Vec<JoinHandle<()>>,
    errors: Vec<String>,
    shutting_down: bool,
}

/// Shared host state is Send + Sync, while individual VMs remain thread-local.
pub(super) struct Test262AgentHost {
    state: Mutex<AgentHostState>,
    reports_ready: Condvar,
    started: Instant,
}

impl Test262AgentHost {
    fn new() -> Self {
        Self {
            state: Mutex::new(AgentHostState {
                controls: Vec::new(),
                reports: VecDeque::new(),
                handles: Vec::new(),
                errors: Vec::new(),
                shutting_down: false,
            }),
            reports_ready: Condvar::new(),
            started: Instant::now(),
        }
    }

    fn register(&self) -> Arc<Test262AgentControl> {
        let control = Arc::new(Test262AgentControl::new());
        self.state
            .lock()
            .expect("Test262 agent host lock poisoned")
            .controls
            .push(Arc::clone(&control));
        control
    }

    fn add_handle(&self, handle: JoinHandle<()>) {
        self.state
            .lock()
            .expect("Test262 agent host lock poisoned")
            .handles
            .push(handle);
    }

    fn broadcast(&self, broadcast: Broadcast) {
        let controls = self
            .state
            .lock()
            .expect("Test262 agent host lock poisoned")
            .controls
            .clone();
        for control in controls {
            let mut broadcasts = control
                .broadcasts
                .lock()
                .expect("Test262 agent control lock poisoned");
            broadcasts.push_back(broadcast.clone());
            control.broadcast_ready.notify_one();
        }
    }

    fn receive_broadcast(&self, control: &Test262AgentControl) -> Option<Broadcast> {
        let mut broadcasts = control
            .broadcasts
            .lock()
            .expect("Test262 agent control lock poisoned");
        loop {
            if let Some(broadcast) = broadcasts.pop_front() {
                return Some(broadcast);
            }
            if self
                .state
                .lock()
                .expect("Test262 agent host lock poisoned")
                .shutting_down
            {
                return None;
            }
            broadcasts = control
                .broadcast_ready
                .wait(broadcasts)
                .expect("Test262 agent control condition poisoned");
        }
    }

    fn report(&self, value: JsString) {
        self.state
            .lock()
            .expect("Test262 agent host lock poisoned")
            .reports
            .push_back(value);
        self.reports_ready.notify_one();
    }

    fn get_report(&self) -> Option<JsString> {
        self.state
            .lock()
            .expect("Test262 agent host lock poisoned")
            .reports
            .pop_front()
    }

    fn report_error(&self, error: RuntimeError) {
        self.state
            .lock()
            .expect("Test262 agent host lock poisoned")
            .errors
            .push(error.to_string());
        self.reports_ready.notify_all();
    }

    fn is_shutting_down(&self) -> bool {
        self.state
            .lock()
            .expect("Test262 agent host lock poisoned")
            .shutting_down
    }

    fn shutdown(&self) -> Result<(), RuntimeError> {
        let (controls, handles, errors) = {
            let mut state = self.state.lock().expect("Test262 agent host lock poisoned");
            state.shutting_down = true;
            let controls = state.controls.clone();
            let handles = std::mem::take(&mut state.handles);
            let errors = std::mem::take(&mut state.errors);
            (controls, handles, errors)
        };
        for control in controls {
            control.broadcast_ready.notify_all();
        }
        for handle in handles {
            let _ = handle.join();
        }
        if let Some(error) = errors.into_iter().next() {
            return Err(RuntimeError::Test262(format!("agent failed: {error}")));
        }
        Ok(())
    }
}

impl Vm {
    pub(super) fn install_test262_agent(
        &mut self,
        host_object: ObjectId,
        function_prototype: ObjectId,
    ) -> Result<(), RuntimeError> {
        if self.test262_agent_host.is_none() {
            self.test262_agent_host = Some(Arc::new(Test262AgentHost::new()));
        }
        let object_prototype = self.object_prototype;
        let agent = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        self.stack.push(Value::Object(agent));
        let result = (|| {
            self.define_data(
                host_object,
                "agent",
                Value::Object(agent),
                true,
                false,
                true,
            )?;
            for (name, length) in [
                ("start", 1),
                ("broadcast", 1),
                ("receiveBroadcast", 1),
                ("report", 1),
                ("getReport", 0),
                ("sleep", 1),
                ("monotonicNow", 0),
                ("leaving", 0),
            ] {
                let native = match name {
                    "start" => "agentStart",
                    "broadcast" => "agentBroadcast",
                    "receiveBroadcast" => "agentReceiveBroadcast",
                    "report" => "agentReport",
                    "getReport" => "agentGetReport",
                    "sleep" => "agentSleep",
                    "monotonicNow" => "agentMonotonicNow",
                    "leaving" => "agentLeaving",
                    _ => unreachable!(),
                };
                self.install_native(
                    agent,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::Test262(native),
                )?;
            }
            let timeouts = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(timeouts));
            for (name, value) in [
                ("yield", 100.0),
                ("small", 200.0),
                ("long", 1_000.0),
                ("huge", 10_000.0),
            ] {
                self.define_data(timeouts, name, Value::Number(value), true, true, true)?;
            }
            self.define_data(agent, "timeouts", Value::Object(timeouts), true, true, true)?;
            self.stack.pop();
            Ok(())
        })();
        self.stack.pop();
        result
    }

    pub(super) fn test262_agent_call(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Option<Result<Value, RuntimeError>> {
        let result = match name {
            "agentStart" => self.test262_agent_start(native::argument(args, 0)),
            "agentBroadcast" => self.test262_agent_broadcast(native::argument(args, 0)),
            "agentReceiveBroadcast" => self.test262_agent_receive(native::argument(args, 0)),
            "agentReport" => self.test262_agent_report(native::argument(args, 0)),
            "agentGetReport" => self.test262_agent_get_report(),
            "agentSleep" => self.test262_agent_sleep(native::argument(args, 0)),
            "agentMonotonicNow" => self.test262_agent_monotonic_now(),
            "agentLeaving" => self.test262_agent_leaving(),
            _ => return None,
        };
        Some(result)
    }

    fn agent_host(&self) -> Result<Arc<Test262AgentHost>, RuntimeError> {
        self.test262_agent_host
            .as_ref()
            .cloned()
            .ok_or_else(|| RuntimeError::TypeError("Test262 agent host is unavailable".into()))
    }

    fn test262_agent_start(&mut self, source: &Value) -> Result<Value, RuntimeError> {
        let source = self
            .coerce_string(source)?
            .to_utf8()
            .map_err(|_| RuntimeError::TypeError("agent source is not UTF-8".into()))?;
        let host = self.agent_host()?;
        let control = host.register();
        let config = self.config;
        let thread_host = Arc::clone(&host);
        let thread_control = Arc::clone(&control);
        let handle = thread::spawn(move || {
            let result = (|| {
                let mut vm = Vm::new(config)?;
                vm.test262_agent_host = Some(Arc::clone(&thread_host));
                vm.test262_agent_control = Some(thread_control);
                vm.install_test262_harness()?;
                let program =
                    parse(&source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
                let code = compile(&program)
                    .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
                vm.execute_script(&code)?;
                while !vm
                    .test262_agent_control
                    .as_ref()
                    .expect("agent control is installed")
                    .leaving
                    .load(Ordering::Acquire)
                    && !thread_host.is_shutting_down()
                {
                    let woke = vm.process_test262_async_wait_events()?;
                    if !woke && !vm.run_next_promise_job()? {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                thread_host.report_error(error);
            }
        });
        host.add_handle(handle);
        Ok(Value::Undefined)
    }

    fn test262_agent_broadcast(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let buffer = value.object_id().ok_or_else(|| {
            RuntimeError::TypeError("agent.broadcast requires a SharedArrayBuffer".into())
        })?;
        let backing = self.heap.shared_buffer_backing(buffer)?;
        let maximum = self.heap.buffer_max_byte_length(buffer)?;
        let byte_length = self.heap.buffer_byte_length(buffer)?;
        self.agent_host()?.broadcast(Broadcast {
            backing,
            max_byte_length: (maximum != byte_length).then_some(maximum),
        });
        Ok(Value::Undefined)
    }

    fn test262_agent_receive(&mut self, callback: &Value) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "agent.receiveBroadcast callback must be callable".into(),
            ));
        }
        let host = self.agent_host()?;
        let control = self
            .test262_agent_control
            .as_ref()
            .cloned()
            .ok_or_else(|| RuntimeError::TypeError("receiveBroadcast requires an agent".into()))?;
        let Some(broadcast) = host.receive_broadcast(&control) else {
            return Ok(Value::Undefined);
        };
        let constructor = self.global("SharedArrayBuffer")?;
        let prototype = self
            .get_property(&constructor, &"prototype".into())?
            .object_id()
            .ok_or_else(|| {
                RuntimeError::TypeError("SharedArrayBuffer prototype is unavailable".into())
            })?;
        let buffer = self.with_roots(|heap| {
            heap.alloc_shared_array_buffer_backing(
                broadcast.backing,
                broadcast.max_byte_length,
                Some(prototype),
            )
        })?;
        self.call_native(
            callback.clone(),
            Value::Undefined,
            vec![Value::Object(buffer)],
            false,
        )?;
        Ok(Value::Undefined)
    }

    fn test262_agent_report(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        self.agent_host()?.report(self.coerce_string(value)?);
        Ok(Value::Undefined)
    }

    fn test262_agent_get_report(&self) -> Result<Value, RuntimeError> {
        Ok(self
            .agent_host()?
            .get_report()
            .map_or(Value::Null, Value::String))
    }

    fn test262_agent_sleep(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let milliseconds = self.coerce_number(value)?;
        if milliseconds.is_finite() && milliseconds > 0.0 {
            thread::sleep(Duration::from_secs_f64(milliseconds / 1_000.0));
        }
        Ok(Value::Undefined)
    }

    fn test262_agent_monotonic_now(&self) -> Result<Value, RuntimeError> {
        Ok(Value::Number(
            self.agent_host()?.started.elapsed().as_secs_f64() * 1_000.0,
        ))
    }

    fn test262_agent_leaving(&self) -> Result<Value, RuntimeError> {
        let control = self
            .test262_agent_control
            .as_ref()
            .ok_or_else(|| RuntimeError::TypeError("agent.leaving requires an agent".into()))?;
        control.leaving.store(true, Ordering::Release);
        Ok(Value::Undefined)
    }

    /// Register an asynchronous waiter with the current VM's host queue. The
    /// spawned thread owns only the SharedArrayBuffer waiter and that queue;
    /// it never dereferences a Value, Heap, or VM from another thread.
    pub(in crate::vm) fn schedule_test262_async_wait(
        &self,
        backing: Arc<SharedBuffer>,
        byte_offset: usize,
        timeout: Option<Duration>,
        promise: ObjectId,
    ) {
        let waiter = backing.register_waiter(byte_offset);
        let queue = Arc::clone(&self.test262_async_waits);
        thread::spawn(move || {
            let result = backing.wait_for(waiter, timeout);
            queue.push(AsyncHostEvent::Wait { promise, result });
        });
    }

    /// Schedule a Test262 harness callback on the VM's event queue. This is
    /// intentionally limited to callback and delay, the surface used by
    /// atomicsHelper.js; it is not exposed to ordinary BlueJS realms.
    pub(in crate::vm) fn schedule_test262_timer(
        &mut self,
        callback: ObjectId,
        delay: Duration,
    ) -> Result<(), RuntimeError> {
        // The timer thread may outlive several allocations and collections in
        // this VM. Keep the callback alive in its owning heap until the event
        // loop has invoked it.
        let root = self.heap.root(callback)?;
        let queue = Arc::clone(&self.test262_async_waits);
        thread::spawn(move || {
            thread::sleep(delay);
            queue.push(AsyncHostEvent::Timer { callback, root });
        });
        Ok(())
    }

    /// Complete all waitAsync promises whose host waiters have settled. This
    /// must be called only by the VM that allocated the promises.
    pub(in crate::vm) fn process_test262_async_wait_events(
        &mut self,
    ) -> Result<bool, RuntimeError> {
        let events = self.test262_async_waits.take_all();
        let woke = !events.is_empty();
        for event in events {
            match event {
                AsyncHostEvent::Wait { promise, result } => {
                    let value = match result {
                        crate::heap::SharedWaitResult::Ok => "ok",
                        crate::heap::SharedWaitResult::TimedOut => "timed-out",
                    };
                    self.settle_promise(
                        promise,
                        PromiseStatus::Fulfilled(Value::String(value.into())),
                    )?;
                }
                AsyncHostEvent::Timer { callback, root } => {
                    let result = self.call_native(
                        Value::Object(callback),
                        Value::Undefined,
                        Vec::new(),
                        false,
                    );
                    self.heap.unroot(root)?;
                    result?;
                }
            }
        }
        Ok(woke)
    }

    /// Drive Test262's asynchronous host work until `$DONE` settles. The
    /// external JSON-lines supervisor owns the wall-clock deadline, so this
    /// loop never turns a valid long host wait into a VM instruction failure.
    pub fn run_test262_async_until_done(
        &mut self,
    ) -> Result<Option<Result<(), Value>>, RuntimeError> {
        loop {
            if self.test262_done.is_some() {
                return Ok(self.take_test262_done());
            }
            let woke = self.process_test262_async_wait_events()?;
            let ran_job = self.run_next_promise_job()?;
            if !woke && !ran_job {
                thread::sleep(Duration::from_millis(1));
            }
        }
    }

    pub fn shutdown_test262_agents(&mut self) -> Result<(), RuntimeError> {
        let Some(host) = self.test262_agent_host.as_ref() else {
            return Ok(());
        };
        host.shutdown()
    }
}
