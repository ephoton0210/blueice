// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Page cleanup uses the HTML JavaScript engine task source, with one pending
//! task per registry and document-wide FIFO order, including ShadowRealms.
//! https://html.spec.whatwg.org/multipage/webappapis.html#hostenqueuefinalizationregistrycleanupjob

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_finalization_boundaries.rs"]
mod boundary_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_finalization_lifetime.rs"]
mod lifetime_tests;

pub(super) struct HostFinalizationTask {
    registry: ObjectId,
    root: RootId,
    pub(super) serial: u64,
}

/// Page cleanup tasks share one document account. Their records contain only
/// IDs; backing and pending-count reservations therefore travel together.
#[derive(Default)]
pub(super) struct HostFinalizationQueue {
    data: VecDeque<HostFinalizationTask>,
    // Every task and the queue allocation drop before their independent fee.
    storage: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for HostFinalizationQueue {
    type Target = VecDeque<HostFinalizationTask>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::Index<usize> for HostFinalizationQueue {
    type Output = HostFinalizationTask;
    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}
impl std::ops::IndexMut<usize> for HostFinalizationQueue {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}
impl HostFinalizationQueue {
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }
    fn owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    fn backing_bytes(&self) -> usize {
        self.data
            .capacity()
            .saturating_mul(std::mem::size_of::<HostFinalizationTask>())
    }
    pub(super) fn unowned_count(&self) -> usize {
        if self.storage.is_none() {
            self.data.len()
        } else {
            0
        }
    }
    pub(super) fn unowned_bytes(&self) -> usize {
        if self.storage.is_none() {
            self.backing_bytes()
        } else {
            0
        }
    }
    fn pop_front(&mut self) -> Option<HostFinalizationTask> {
        let task = self.data.pop_front()?;
        if let Some(storage) = &mut self.storage {
            storage.release_browser_tasks_to(self.data.len());
        }
        Some(task)
    }
    fn shrink_to_fit(&mut self) {
        if self.data.is_empty() {
            self.data = VecDeque::new();
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn reserve(&mut self, count: usize) {
        self.data.reserve(count);
        let bytes = self.backing_bytes();
        if let Some(storage) = &mut self.storage {
            storage.reconcile_owned_bytes(bytes);
        }
    }
}

pub(super) struct PreparedFinalizationTask {
    growth: promise_queue_storage::PreparedDeque<HostFinalizationTask>,
    count: Option<SharedSchedulingCharge>,
}
impl PreparedFinalizationTask {
    fn commit(self, vm: &mut Vm, task: HostFinalizationTask) {
        let queue = &mut vm.host_finalization_tasks;
        if let Some(backing) = self.growth.commit_owned(&mut queue.data) {
            if let Some(storage) = &mut queue.storage {
                storage.release_to(0);
                storage
                    .absorb(backing)
                    .expect("cleanup backing has one document");
            } else {
                queue.storage = Some(backing);
            }
        }
        if let Some(count) = self.count {
            queue
                .storage
                .as_mut()
                .expect("prepared cleanup owner")
                .absorb(count)
                .expect("cleanup count has the queue's document");
        }
        queue.data.push_back(task);
        vm.reconcile_scheduling_ownership();
    }
}

impl Vm {
    pub(super) fn finalization_queue_owner(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let current = self.argument_list_owner()?;
        let original = self.host_finalization_tasks.owner()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.host_finalization_tasks.can_join(scope)
                || hint.is_some_and(|owner| !owner.belongs_to(scope))
            {
                return Err(scope.fail());
            }
        }
        let Some(owner) = original.as_ref().or(current.as_ref()).or(hint) else {
            return Ok(None);
        };
        if self.host_finalization_tasks.storage.is_none() {
            self.synchronize_scheduling_usage()?;
            let bytes = self.host_finalization_tasks.backing_bytes();
            let count = self.host_finalization_tasks.len();
            let storage = match &mut self.host_scheduling {
                Some(scope) => {
                    let mut storage = scope.detach_owned_payload(bytes);
                    storage
                        .absorb(scope.detach_owned_browser_tasks(count))
                        .expect("published cleanup has one document");
                    storage
                }
                None => {
                    let mut storage = owner.reserve_peer(bytes)?;
                    storage.absorb(owner.reserve_browser_tasks(count)?)?;
                    storage
                }
            };
            self.host_finalization_tasks.storage = Some(storage);
        }
        owner.reserve_peer(0).map(Some)
    }

    pub(super) fn prepare_host_finalization_task(
        &mut self,
    ) -> Result<PreparedFinalizationTask, RuntimeError> {
        let owner = self.finalization_queue_owner(None)?;
        let count = owner
            .as_ref()
            .map(|owner| owner.reserve_browser_tasks(1))
            .transpose()?;
        let growth = promise_queue_storage::PreparedDeque::for_additional_owned(
            &self.host_finalization_tasks.data,
            1,
            owner.as_ref(),
        )?;
        Ok(PreparedFinalizationTask { growth, count })
    }

    pub(super) fn prepare_finalization_registry_snapshot(
        &self,
    ) -> Result<host_task_storage::TaskVector<ObjectId>, RuntimeError> {
        let mut registries = host_task_storage::TaskVector::new(self.host_scheduling.as_ref())?;
        let pending = self.heap.pending_finalization_registries();
        registries.reserve(pending.len())?;
        for registry in pending {
            registries.push(registry)?;
        }
        registries.sort_unstable_by_key(|id| id.serial);
        Ok(registries)
    }

    pub(super) fn enqueue_host_finalization_tasks(&mut self) -> Result<(), RuntimeError> {
        let registries = self.prepare_finalization_registry_snapshot()?;
        for &registry in registries.iter() {
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
            self.admit_scheduling_work(extra)?;
            self.heap.check_root_registration_count(1)?;
            let prepared = self.prepare_host_finalization_task()?;
            let objects = [registry];
            let roots = self.heap.prepare_root_batch(&objects)?;
            self.check_scheduling_failure()?;
            // Acquire the serial only after both native owners are prepared.
            let serial = self
                .host_scheduling
                .as_ref()
                .expect("page cleanup has an owner")
                .next_task_serial()?;
            let root = self.heap.commit_single_root(roots);
            prepared.commit(
                self,
                HostFinalizationTask {
                    registry,
                    root,
                    serial,
                },
            );
            self.synchronize_scheduling_usage()?;
        }
        Ok(())
    }

    pub(super) fn run_next_host_finalization_task(&mut self) -> Result<(), RuntimeError> {
        let Some(registry) = self
            .host_finalization_tasks
            .front()
            .map(|task| task.registry)
        else {
            return Ok(());
        };
        let prepared = self.prepare_finalization_entry(registry)?;
        let previous_fuel = self.remaining_instructions;
        self.remaining_instructions = self.config.instruction_budget;
        if prepared.is_some() {
            if let Err(error) = self.charge_step() {
                self.remaining_instructions = previous_fuel;
                drop(prepared);
                self.reconcile_scheduling_ownership();
                return Err(error);
            }
        }
        // No JavaScript or heap collection ran during preparation. Commit the
        // queued task and one ready cell only after its native owners and fuel fit.
        let task = self
            .host_finalization_tasks
            .pop_front()
            .expect("prepared task remains queued");
        let previous = self.active_finalization_registry.replace(task.registry);
        let root_base = self.async_frame_roots.len();
        let result = (|| {
            if let Some(mut prepared) = prepared {
                self.async_frame_roots
                    .extend(prepared.references.into_iter().flatten());
                if let Some((callback, holdings)) = self
                    .heap
                    .take_finalization_registry_cleanup_cell(task.registry)?
                {
                    prepared.arguments.push(holdings);
                    self.reconcile_scheduling_ownership();
                    // An attempted call owns the consumed cell, even if dispatch
                    // throws or fails. Native entry takes over the staged leases.
                    let arguments = prepared.into_arguments();
                    let called = self
                        .call_native(callback, Value::Undefined, arguments, false)
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
                    let fuel = self.remaining_instructions;
                    let checkpoint = self.run_host_microtask_checkpoint();
                    self.remaining_instructions = fuel;
                    checkpoint?;
                }
            }
            Ok(())
        })();
        self.async_frame_roots.truncate(root_base);
        self.active_finalization_registry = previous;
        self.release_root(task.root);
        if self.host_finalization_tasks.is_empty() {
            self.host_finalization_tasks.shrink_to_fit();
        }
        self.reconcile_scheduling_ownership();
        self.synchronize_scheduling_usage()?;
        if let Err(error) = result {
            if !host_timers::is_page_exception(&error) {
                return Err(error);
            }
        }
        self.run_host_microtask_checkpoint()?;
        Ok(())
    }

    pub(super) fn run_document_browser_task_turn(
        &mut self,
        max_tasks: usize,
    ) -> Result<bool, RuntimeError> {
        self.run_host_task_turn_inner(self.host_timer_clock_ms, max_tasks, false)?;
        Ok(self.host_scheduling_snapshot().pending_browser_tasks == 0)
    }
}
