// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Page cleanup uses the HTML JavaScript engine task source, with one pending
//! task per registry and document-wide FIFO order, including ShadowRealms.
//! https://html.spec.whatwg.org/multipage/webappapis.html#hostenqueuefinalizationregistrycleanupjob

use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_finalization_boundaries.rs"]
mod boundary_tests;

pub(super) struct HostFinalizationTask {
    registry: ObjectId,
    root: RootId,
    pub(super) serial: u64,
}

impl Vm {
    pub(super) fn enqueue_host_finalization_tasks(&mut self) -> Result<(), RuntimeError> {
        for registry in self.heap.pending_finalization_registries() {
            if self.active_finalization_registry == Some(registry)
                || self
                    .host_finalization_tasks
                    .iter()
                    .any(|task| task.registry == registry)
            {
                continue;
            }
            let extra = HostSchedulingUsage {
                pending_browser_tasks: 1,
                ..HostSchedulingUsage::default()
            };
            let growth = SchedulingCapacity::for_deque_push(&self.host_finalization_tasks);
            self.admit_scheduling_work(extra.with_growth(growth))?;
            growth.reserve_deque(&mut self.host_finalization_tasks);
            self.admit_scheduling_work(extra)?;
            let serial = self
                .host_scheduling
                .as_ref()
                .expect("page cleanup has an owner")
                .next_task_serial()?;
            let root = self.heap.root(registry)?;
            self.host_finalization_tasks
                .push_back(HostFinalizationTask {
                    registry,
                    root,
                    serial,
                });
            self.synchronize_scheduling_usage()?;
        }
        Ok(())
    }

    pub(super) fn run_next_host_finalization_task(&mut self) -> Result<(), RuntimeError> {
        let Some(task) = self.host_finalization_tasks.pop_front() else {
            return Ok(());
        };
        let previous = self.active_finalization_registry.replace(task.registry);
        self.remaining_instructions = self.config.instruction_budget;
        let result = (|| {
            self.synchronize_scheduling_usage()?;
            // CleanupFinalizationRegistry permits choosing how many cleared
            // cells to consume. One per task gives the document turn's task
            // budget a bound on cleanup callbacks too, without dropping cells.
            if let Some((callback, holdings)) = self
                .heap
                .take_finalization_registry_cleanup_cell(task.registry)?
            {
                let root_base = self.async_frame_roots.len();
                self.extend_async_frame_roots(
                    callback.object_id().into_iter().chain(holdings.object_id()),
                )?;
                let bytes = host_scheduling::value_payload_bytes(&holdings);
                let holdings_payload = self.charge_execution_payload(bytes);
                let called = (|| {
                    self.charge_step()?;
                    self.synchronize_scheduling_usage()?;
                    drop(holdings_payload);
                    // Cleanup ignores a normal return. Drop it before the
                    // checkpoint rather than retaining an unused buffer.
                    let called = self
                        .call_native(callback, Value::Undefined, vec![holdings], false)
                        .map(|_| ());
                    if let Err(error) = called {
                        if !host_timers::is_page_exception(&error) {
                            return Err(error);
                        }
                        if let RuntimeError::Thrown(value) = &error {
                            self.extend_async_frame_roots(value.object_id().into_iter())?;
                        }
                        self.report_host_exception(&error)?;
                    }
                    // The callback has left the execution stack. Each call
                    // cleans up microtasks before the next cleared cell.
                    // Its separate job budget must not refill this task's fuel.
                    let fuel = self.remaining_instructions;
                    let checkpoint = self.run_host_microtask_checkpoint();
                    self.remaining_instructions = fuel;
                    checkpoint?;
                    Ok(())
                })();
                self.async_frame_roots.truncate(root_base);
                self.synchronize_scheduling_usage()?;
                called?;
            }
            Ok(())
        })();
        self.active_finalization_registry = previous;
        self.release_root(task.root);
        if self.host_finalization_tasks.is_empty() {
            self.host_finalization_tasks.shrink_to_fit();
        }
        self.synchronize_scheduling_usage()?;
        if let Err(error) = result {
            if !host_timers::is_page_exception(&error) {
                return Err(error);
            }
        }
        // A throw consumes only its own cell. Remaining cleared cells are
        // still cancellable and can be scheduled in a later cleanup task.
        self.run_host_microtask_checkpoint()?;
        Ok(())
    }

    pub(super) fn host_shadow_realms(&self) -> Vec<std::rc::Weak<RefCell<Vm>>> {
        let mut queue: VecDeque<_> = self.shadow_realms.values().cloned().collect();
        let mut seen = HashSet::from([self.object_prototype.heap]);
        let mut realms = Vec::new();
        while let Some(record) = queue.pop_front() {
            let Ok(vm) = record.vm.try_borrow() else {
                continue;
            };
            let same_document = self
                .host_scheduling
                .as_ref()
                .zip(vm.host_scheduling.as_ref())
                .is_some_and(|(owner, child)| owner.same_document(child));
            if !same_document || !seen.insert(vm.object_prototype.heap) {
                continue;
            }
            queue.extend(vm.shadow_realms.values().cloned());
            drop(vm);
            realms.push(record);
        }
        realms.sort_by_key(|record| record.vm.borrow().object_prototype.heap);
        // A traversal snapshot is not a document-work owner. Holding every
        // VM strongly across callbacks inflates the native-pressure watermark
        // after agent collection has already removed its actual owners.
        realms
            .into_iter()
            .map(|record| Rc::downgrade(&record.vm))
            .collect()
    }

    pub(super) fn run_document_browser_task_turn(
        &mut self,
        max_tasks: usize,
    ) -> Result<bool, RuntimeError> {
        self.run_host_task_turn_inner(self.host_timer_clock_ms, max_tasks, false)?;
        Ok(self.host_scheduling_snapshot().pending_browser_tasks == 0)
    }
}
