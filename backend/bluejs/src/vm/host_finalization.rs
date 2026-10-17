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
            let growth = promise_queue_storage::PreparedDeque::for_push(
                &self.host_finalization_tasks,
                self.host_scheduling.as_ref(),
            )?;
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
            growth.commit(&mut self.host_finalization_tasks);
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

    pub(super) fn host_shadow_realms(
        &self,
    ) -> Result<host_task_storage::TaskVector<std::rc::Weak<RefCell<Vm>>>, RuntimeError> {
        let mut realms = host_task_storage::TaskVector::new(self.host_scheduling.as_ref())?;
        let Some(owner) = &self.host_scheduling else {
            return Ok(realms);
        };
        if self.shadow_realms.is_empty() {
            return Ok(realms);
        }
        let mut seen_storage = realm_collection::MetadataStorage::new(Some(owner))?;
        let mut seen = HashSet::new();
        seen_storage.reserve_set(&mut seen, 1)?;
        seen.insert(self.object_prototype.heap);
        let mut queue = host_task_storage::TaskVector::new(Some(owner))?;
        queue.reserve(self.shadow_realms.len())?;
        for record in self.shadow_realms.values() {
            queue.push(Rc::downgrade(&record.vm))?;
        }
        let mut index = 0;
        while index < queue.len() {
            let record = queue[index].clone();
            index += 1;
            let Some(realm) = record.upgrade() else {
                continue;
            };
            let Ok(vm) = realm.try_borrow() else {
                continue;
            };
            let same_document = vm
                .host_scheduling
                .as_ref()
                .is_some_and(|child| owner.same_document(child));
            let heap = vm.object_prototype.heap;
            if !same_document || seen.contains(&heap) {
                continue;
            }
            seen_storage.reserve_set(&mut seen, 1)?;
            seen.insert(heap);
            queue.reserve(vm.shadow_realms.len())?;
            for child in vm.shadow_realms.values() {
                queue.push(Rc::downgrade(&child.vm))?;
            }
            drop(vm);
            realms.push(record)?;
        }
        realms.sort_unstable_by_key(|record| {
            record
                .upgrade()
                .expect("discovered realms remain owned during preparation")
                .borrow()
                .object_prototype
                .heap
        });
        // Only weak handles survive preparation. The snapshot's slots retain
        // their native lease across callbacks without keeping dead VMs alive.
        Ok(realms)
    }

    pub(super) fn run_document_browser_task_turn(
        &mut self,
        max_tasks: usize,
    ) -> Result<bool, RuntimeError> {
        self.run_host_task_turn_inner(self.host_timer_clock_ms, max_tasks, false)?;
        Ok(self.host_scheduling_snapshot().pending_browser_tasks == 0)
    }
}
