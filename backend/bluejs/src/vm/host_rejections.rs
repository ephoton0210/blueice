// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML rejection notifications are document tasks, not microtasks. Pending
//! delivery owns roots; the outstanding notification bit never roots a Promise.
//! https://html.spec.whatwg.org/multipage/webappapis.html#unhandled-promise-rejections

use super::*;
use host_console::formatting::Output;

pub(super) struct PendingRejection {
    pub(super) promise: ObjectId,
    root: RootId,
}

#[derive(Default)]
pub(super) struct HostRejections {
    pub(super) pending: Vec<PendingRejection>,
    pub(super) active: usize,
}

impl Vm {
    /// Specifications such as Streams mark internal promises handled
    /// directly, without performing a user-visible Then or a handle hook.
    pub(super) fn set_promise_handled(&mut self, promise: ObjectId) {
        self.promises
            .get_mut(&promise)
            .expect("internal Promise owner")
            .handled = true;
    }

    pub fn install_host_rejection_tracking(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.host_rejections.is_some() || self.host_error_reporting.is_none() {
            return Err(RuntimeError::TypeError("rejection tracking requires the owner's global error bindings and may be installed only once".into()));
        }
        for name in [
            "PromiseRejectionEvent",
            "onunhandledrejection",
            "onrejectionhandled",
        ] {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "global rejection binding already defined".into(),
                ));
            }
        }
        if self.host_scheduling.is_none() {
            self.use_bounded_host_microtask_checkpoints()?;
        }
        self.install_web_rejection_events()?;
        let function = self.function_prototype()?;
        for (attribute, event) in [
            ("onunhandledrejection", "unhandledrejection"),
            ("onrejectionhandled", "rejectionhandled"),
        ] {
            self.install_native_accessor(
                global,
                function,
                attribute,
                NativeFunction::HostRejectionHandler(event, false),
                NativeFunction::HostRejectionHandler(event, true),
            )?;
            self.heap.define_own_property(
                global,
                attribute,
                PropertyDescriptor {
                    enumerable: Some(true),
                    ..Default::default()
                },
            )?;
        }
        self.host_rejections = Some(HostRejections::default());
        self.synchronize_scheduling_usage()
    }

    pub(super) fn host_rejection_handler(
        &mut self,
        event: &'static str,
        set: bool,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let global = self.global("globalThis")?;
        if construct || (!matches!(receiver, Value::Null | Value::Undefined) && *receiver != global)
        {
            return Err(RuntimeError::TypeError(
                "Invalid global rejection receiver".into(),
            ));
        }
        self.host_event_handler(global.object_id().unwrap(), event, set, args)
    }

    pub(super) fn track_host_rejection(&mut self, promise: ObjectId) {
        if self.host_rejections.is_none() {
            return;
        }
        let extra = HostSchedulingUsage {
            pending_reactions: 1,
            ..Default::default()
        };
        let growth =
            SchedulingCapacity::for_vec_push(&self.host_rejections.as_ref().unwrap().pending);
        if self
            .admit_scheduling_work(extra.with_growth(growth))
            .is_err()
        {
            return;
        }
        growth.reserve_vec(&mut self.host_rejections.as_mut().unwrap().pending);
        if self.admit_scheduling_work(extra).is_err() {
            return;
        }
        let root = match self.heap.root(promise) {
            Ok(root) => root,
            Err(_) => {
                // This producer is infallible. Root admission/identifier
                // refusal must latch the document abort before publication.
                if let Some(scope) = &self.host_scheduling {
                    let _ = scope.fail();
                }
                self.reconcile_scheduling_ownership();
                return;
            }
        };
        self.host_rejections
            .as_mut()
            .unwrap()
            .pending
            .push(PendingRejection { promise, root });
        let _ = self.synchronize_scheduling_usage();
    }

    pub(super) fn mark_promise_handled(&mut self, promise: ObjectId) {
        let Some(record) = self.promises.get_mut(&promise) else {
            return;
        };
        if record.handled {
            return;
        }
        record.handled = true;
        let notified = std::mem::take(&mut record.rejection_notified);
        let Some(state) = &mut self.host_rejections else {
            return;
        };
        if let Some(index) = state
            .pending
            .iter()
            .position(|entry| entry.promise == promise)
        {
            let entry = state.pending.remove(index);
            self.release_root(entry.root);
        } else if notified {
            // Removing the outstanding bit precedes publishing this task.
            // Only the queued event, not the outstanding set, retains it.
            let result = (|| {
                let mut entries = Vec::with_capacity(1);
                self.reserve_browser_task(HostSchedulingUsage {
                    pending_reactions: 1,
                    payload_bytes: entries.capacity() * std::mem::size_of::<PendingRejection>(),
                    ..Default::default()
                })?;
                let serial = self.next_host_task_serial()?;
                let root = self.heap.root(promise)?;
                entries.push(PendingRejection { promise, root });
                self.browser_tasks
                    .push_back(browser_tasks::BrowserTask::PromiseRejections {
                        serial,
                        entries,
                        handled: true,
                    });
                Ok::<(), RuntimeError>(())
            })();
            // Resource failure poisons the existing document account and is
            // surfaced by the execution/checkpoint boundary, like job admission.
            if result.is_err() {
                return;
            }
        }
        let _ = self.synchronize_scheduling_usage();
    }

    pub(super) fn notify_host_rejections(&mut self) -> Result<(), RuntimeError> {
        if self
            .host_rejections
            .as_ref()
            .is_none_or(|state| state.pending.is_empty())
        {
            return Ok(());
        }
        self.reserve_browser_task(HostSchedulingUsage::default())?;
        let serial = self.next_host_task_serial()?;
        let entries = std::mem::take(&mut self.host_rejections.as_mut().unwrap().pending);
        self.browser_tasks
            .push_back(browser_tasks::BrowserTask::PromiseRejections {
                serial,
                entries,
                handled: false,
            });
        self.synchronize_scheduling_usage()
    }

    pub(super) fn run_host_rejection_task(
        &mut self,
        entries: Vec<PendingRejection>,
        handled: bool,
    ) -> Result<(), RuntimeError> {
        let count = entries.len();
        self.host_rejections
            .as_mut()
            .expect("notification owner")
            .active += count;
        let result = self.run_host_event_turn(|vm| {
            vm.synchronize_scheduling_usage()?;
            for entry in &entries {
                vm.charge_step()?;
                let record = vm
                    .promises
                    .get(&entry.promise)
                    .expect("notification roots its Promise");
                if !handled && record.handled {
                    continue;
                }
                let PromiseStatus::Rejected(reason) = &record.status else {
                    continue;
                };
                let reason = reason.clone();
                let source = record.rejection_origin.source.clone();
                let event_name = if handled {
                    "rejectionhandled"
                } else {
                    "unhandledrejection"
                };
                let base = vm.stack.len();
                let fired = (|| {
                    let mut state = vm.new_web_event_state(7, event_name.into());
                    state.cancelable = !handled;
                    state.set("promise", Value::Object(entry.promise));
                    state.set("reason", reason.clone());
                    let prototype = vm.globals["%WebPromiseRejectionEvent%"];
                    let event = vm.with_roots(|heap| heap.alloc_drag_event(state, prototype))?;
                    vm.stack.push(Value::Object(event));
                    vm.finish_web_event(event)?;
                    let global = vm.global("globalThis")?.object_id().unwrap();
                    let not_canceled =
                        vm.dispatch_web_target_event(global, event, true)? == Value::Bool(true);
                    if !handled && not_canceled {
                        let mut output = Output::message("Uncaught (in promise) ");
                        vm.console_preview(&reason, &mut output)?;
                        vm.console_emit_with_source(
                            ConsoleFunction::Error,
                            "error",
                            output,
                            source,
                        )?;
                    }
                    if !handled {
                        let record = vm
                            .promises
                            .get_mut(&entry.promise)
                            .expect("rooted notification owner");
                        if !record.handled {
                            record.rejection_notified = true;
                        }
                    }
                    Ok::<(), RuntimeError>(())
                })();
                vm.stack.truncate(base);
                fired?;
            }
            Ok(())
        });
        for entry in entries {
            self.release_root(entry.root);
        }
        self.host_rejections.as_mut().unwrap().active -= count;
        self.synchronize_scheduling_usage()?;
        result
    }
}
