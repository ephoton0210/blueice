// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML rejection notifications are document tasks, not microtasks. Pending
//! delivery owns roots; the outstanding notification bit never roots a Promise.
//! https://html.spec.whatwg.org/multipage/webappapis.html#unhandled-promise-rejections

use super::host_scheduling::SharedSchedulingCharge;
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
    pub(super) active_owned: usize,
    // Pending records and backing die before their count/storage credit.
    pending_storage: Option<SharedSchedulingCharge>,
}

impl HostRejections {
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.pending_storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }
    pub(super) fn unowned_count(&self) -> usize {
        if self.pending_storage.is_none() {
            self.pending.len()
        } else {
            0
        }
    }
    pub(super) fn unowned_bytes(&self) -> usize {
        if self.pending_storage.is_none() {
            host_scheduling::slots(&self.pending)
        } else {
            0
        }
    }
    fn take_pending(&mut self) -> PendingRejectionTransfer {
        let entries = std::mem::take(&mut self.pending);
        let storage = self.pending_storage.as_mut().map(|owner| {
            let mut storage = owner.split_off(host_scheduling::slots(&entries));
            storage
                .absorb(owner.split_reactions(entries.len()))
                .expect("pending batch has one owner");
            storage
        });
        PendingRejectionTransfer { entries, storage }
    }
}

pub(super) struct PendingRejectionTransfer {
    pub(super) entries: Vec<PendingRejection>,
    pub(super) storage: Option<SharedSchedulingCharge>,
}

struct PreparedPendingRejection {
    growth: promise_queue_storage::PreparedVec<PendingRejection>,
    count: Option<SharedSchedulingCharge>,
}
impl PreparedPendingRejection {
    fn commit(self, vm: &mut Vm, promise: ObjectId, root: RootId) {
        let state = vm.host_rejections.as_mut().expect("pending notifications");
        if let Some(backing) = self.growth.commit_owned(&mut state.pending) {
            let storage = state
                .pending_storage
                .as_mut()
                .expect("prepared pending owner");
            storage.release_to(0);
            storage
                .absorb(backing)
                .expect("pending backing shares its account");
        }
        if let Some(count) = self.count {
            state
                .pending_storage
                .as_mut()
                .expect("prepared pending owner")
                .absorb(count)
                .expect("pending count shares its account");
        }
        state.pending.push(PendingRejection { promise, root });
        vm.reconcile_scheduling_ownership();
    }
}

impl Vm {
    pub(super) fn pending_rejection_owner(
        &mut self,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        if self.host_rejections.is_none() {
            return Ok(None);
        }
        let current = self.argument_list_owner()?;
        let original = self
            .host_rejections
            .as_ref()
            .unwrap()
            .pending_storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.host_rejections.as_ref().unwrap().can_join(scope) {
                return Err(scope.fail());
            }
        }
        let Some(owner) = original.as_ref().or(current.as_ref()) else {
            return Ok(None);
        };
        if self
            .host_rejections
            .as_ref()
            .unwrap()
            .pending_storage
            .is_none()
        {
            self.synchronize_scheduling_usage()?;
            let state = self.host_rejections.as_ref().unwrap();
            let bytes = host_scheduling::slots(&state.pending);
            let count = state.pending.len();
            let storage = match &mut self.host_scheduling {
                Some(scope) => {
                    let mut storage = scope.detach_owned_payload(bytes);
                    storage
                        .absorb(scope.detach_owned_reactions(count))
                        .expect("published pending notifications share their document");
                    storage
                }
                None => {
                    let mut storage = owner.reserve_peer(bytes)?;
                    storage.absorb(owner.reserve_reactions(count)?)?;
                    storage
                }
            };
            self.host_rejections.as_mut().unwrap().pending_storage = Some(storage);
        }
        owner.reserve_peer(0).map(Some)
    }

    fn prepare_pending_rejection(&mut self) -> Result<PreparedPendingRejection, RuntimeError> {
        let owner = self.pending_rejection_owner()?;
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_reactions: 1,
            ..Default::default()
        })?;
        self.heap.check_root_registration_count(1)?;
        let count = owner
            .as_ref()
            .map(|owner| owner.reserve_reactions(1))
            .transpose()?;
        let growth = promise_queue_storage::PreparedVec::for_push_owned(
            &self
                .host_rejections
                .as_ref()
                .expect("pending notifications")
                .pending,
            owner.as_ref(),
        )?;
        Ok(PreparedPendingRejection { growth, count })
    }

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
        let result = (|| {
            let prepared = self.prepare_pending_rejection()?;
            let objects = [promise];
            let roots = self.heap.prepare_root_batch(&objects)?;
            self.check_scheduling_failure()?;
            let root = self.heap.commit_single_root(roots);
            prepared.commit(self, promise, root);
            Ok::<(), RuntimeError>(())
        })();
        if result.is_err() {
            // This producer is infallible. Root or native refusal must latch
            // the document abort after detached owners have refunded.
            if let Some(scope) = &self.host_scheduling {
                let _ = scope.fail();
            }
        } else if let Some(scope) = &self.host_scheduling {
            scope.request_checkpoint();
        }
        self.reconcile_scheduling_ownership();
    }

    pub(super) fn mark_promise_handled(&mut self, promise: ObjectId) {
        let Some(record) = self.promises.get_mut(&promise) else {
            return;
        };
        if record.handled {
            return;
        }
        record.handled = true;
        let notified = record.rejection_notified;
        let Some(state) = &mut self.host_rejections else {
            record.rejection_notified = false;
            return;
        };
        if let Some(index) = state
            .pending
            .iter()
            .position(|entry| entry.promise == promise)
        {
            let entry = state.pending.remove(index);
            if let Some(storage) = &mut state.pending_storage {
                storage.release_reactions_to(state.pending.len());
            }
            self.release_root(entry.root);
            self.promises.get_mut(&promise).unwrap().rejection_notified = false;
        } else if notified {
            // The language handled status is already committed. Keep the
            // notification bit until its follow-up event can also publish.
            let result = (|| {
                self.heap.check_root_registration_count(1)?;
                let mut prepared = self.prepare_browser_task(HostSchedulingUsage {
                    pending_reactions: 1,
                    payload_bytes: std::mem::size_of::<PendingRejection>(),
                    ..Default::default()
                })?;
                let mut entries = Vec::new();
                entries.try_reserve_exact(1).map_err(|_| {
                    self.host_scheduling
                        .as_ref()
                        .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
                })?;
                prepared.resize_payload(host_scheduling::slots(&entries))?;
                let objects = [promise];
                let roots = self.heap.prepare_root_batch(&objects)?;
                self.check_scheduling_failure()?;
                let serial = self.next_host_task_serial()?;
                let root = self.heap.commit_single_root(roots);
                entries.push(PendingRejection { promise, root });
                self.promises.get_mut(&promise).unwrap().rejection_notified = false;
                prepared.commit(
                    self,
                    browser_tasks::BrowserTask::PromiseRejections {
                        serial,
                        entries,
                        handled: true,
                    },
                );
                Ok::<(), RuntimeError>(())
            })();
            // Resource failure poisons the existing document account and is
            // surfaced by the execution/checkpoint boundary, like job admission.
            if result.is_err() {
                if let Some(scope) = &self.host_scheduling {
                    let _ = scope.fail();
                }
            }
        }
        self.reconcile_scheduling_ownership();
    }

    pub(super) fn notify_host_rejections(&mut self) -> Result<(), RuntimeError> {
        if self
            .host_rejections
            .as_ref()
            .is_none_or(|state| state.pending.is_empty())
        {
            return Ok(());
        }
        let prepared = self.prepare_browser_rejection_transfer()?;
        let serial = self.next_host_task_serial()?;
        let entries = self.host_rejections.as_mut().unwrap().take_pending();
        prepared.commit_pending_rejections(self, serial, entries);
        self.synchronize_scheduling_usage()
    }

    pub(super) fn run_host_rejection_task(
        &mut self,
        entries: Vec<PendingRejection>,
        handled: bool,
        native: bool,
        prepared: rejection_batch_entry::PreparedRejectionBatch,
    ) -> Result<(), RuntimeError> {
        let count = entries.len();
        self.host_rejections
            .as_mut()
            .expect("notification owner")
            .active += count;
        if native {
            self.host_rejections.as_mut().unwrap().active_owned += count;
        }
        let result = self.run_host_event_turn(|vm| {
            let mut batch = prepared;
            vm.synchronize_scheduling_usage()?;
            for mut entry in batch.entries.drain(..) {
                vm.charge_step()?;
                let record = vm
                    .promises
                    .get(&entry.promise)
                    .expect("notification roots its Promise");
                if !handled && record.handled {
                    continue;
                }
                let PromiseStatus::Rejected(_) = &record.status else {
                    continue;
                };
                let base = vm.stack.len();
                let fired = (|| {
                    let mut state = entry.state.take().expect("eligible prepared notification");
                    // Storage is prepared as a batch, but each event is born
                    // when its delivery begins, after preceding callbacks.
                    state.set(
                        "timeStamp",
                        Value::Number(vm.event_time_origin.elapsed().as_secs_f64() * 1000.0),
                    );
                    let prototype = vm.globals["%WebPromiseRejectionEvent%"];
                    let event = vm.with_roots(|heap| heap.alloc_drag_event(state, prototype));
                    drop(entry.state_charge.take());
                    let event = event?;
                    vm.stack.push(Value::Object(event));
                    vm.finish_web_event(event)?;
                    let global = vm.global("globalThis")?.object_id().unwrap();
                    let not_canceled =
                        vm.dispatch_web_target_event(global, event, true)? == Value::Bool(true);
                    if !handled && not_canceled {
                        let owners = vm.console_owners(entry.source_charge.as_ref())?;
                        let mut output =
                            Output::message_owned("Uncaught (in promise) ", owners.owner.as_ref())?;
                        let preview = vm.console_preview(&entry.reason, &mut output);
                        owners.validate().and(preview)?;
                        vm.console_emit_with_leased_source(
                            ConsoleFunction::Error,
                            "error",
                            output,
                            entry.source.take(),
                            entry.source_charge.take(),
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
        self.discard_host_rejection_entries(entries);
        self.host_rejections.as_mut().unwrap().active -= count;
        if native {
            self.host_rejections.as_mut().unwrap().active_owned -= count;
        }
        self.synchronize_scheduling_usage()?;
        result
    }

    pub(super) fn discard_host_rejection_entries(&mut self, entries: Vec<PendingRejection>) {
        for entry in entries {
            self.release_root(entry.root);
        }
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/realm_rejection_lifetime.rs"]
mod lifetime_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/pending_rejection_lifetime.rs"]
mod pending_lifetime_tests;
