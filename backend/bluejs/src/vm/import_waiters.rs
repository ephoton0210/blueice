// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import waiters own their targets and dependency storage until settlement.

use super::*;

impl Vm {
    pub(super) fn enqueue_module_import_waiter(
        &mut self,
        module: String,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        let extra = HostSchedulingUsage {
            pending_reactions: 1,
            ..HostSchedulingUsage::default()
        };
        self.admit_scheduling_work(extra)?;
        if let Some(waiters) = self.module_import_waiters.get(&module) {
            // The incoming canonical name is temporary when its key exists.
            // Existing storage retains that key without another string copy.
            let growth = SchedulingCapacity::for_vec_push(waiters);
            self.admit_scheduling_work(extra.with_growth(growth))?;
            growth.reserve_vec(self.module_import_waiters.get_mut(&module).unwrap());
            self.admit_scheduling_work(extra)?;
            self.module_import_waiters
                .get_mut(&module)
                .unwrap()
                .push(promise);
        } else {
            let table = SchedulingMapCapacity::for_insert(
                &self.module_import_waiters,
                self.host_scheduling.as_ref(),
            )?;
            let mut waiters = Vec::new();
            let growth = SchedulingCapacity::for_vec_push(&waiters);
            let extra = HostSchedulingUsage {
                payload_bytes: module.capacity(),
                ..extra
            };
            self.admit_scheduling_work(extra.with_growth(growth).with_map_growth(&table))?;
            growth.reserve_vec(&mut waiters);
            let extra = HostSchedulingUsage {
                payload_bytes: extra.payload_bytes.saturating_add(
                    waiters
                        .capacity()
                        .saturating_mul(std::mem::size_of::<ObjectId>()),
                ),
                ..extra
            };
            self.admit_scheduling_work(extra.with_map_growth(&table))?;
            table.commit(&mut self.module_import_waiters);
            waiters.push(promise);
            self.module_import_waiters.insert(module, waiters);
        }
        self.synchronize_scheduling_usage()
    }

    pub(super) fn enqueue_deferred_import_waiter(
        &mut self,
        promise: ObjectId,
        namespace: ObjectId,
        modules: Vec<String>,
    ) -> Result<(), RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_reactions: 1,
            ..HostSchedulingUsage::default()
        })?;
        // Prepare the actual dependency set without changing live waiters.
        // Names move into it; duplicate names do not retain duplicate buffers.
        let mut pending = HashSet::new();
        pending.try_reserve(modules.len()).map_err(|_| {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        })?;
        pending.extend(modules);
        let extra = HostSchedulingUsage {
            pending_reactions: 1,
            payload_bytes: pending.iter().fold(
                pending
                    .capacity()
                    .saturating_mul(std::mem::size_of::<String>()),
                |bytes, name| bytes.saturating_add(name.capacity()),
            ),
            ..HostSchedulingUsage::default()
        };
        let growth = SchedulingCapacity::for_vec_push(&self.deferred_import_waiters);
        self.admit_scheduling_work(extra.with_growth(growth))?;
        growth.reserve_vec(&mut self.deferred_import_waiters);
        self.admit_scheduling_work(extra)?;
        self.deferred_import_waiters.push(DeferredImportWaiter {
            promise,
            namespace,
            pending,
        });
        self.synchronize_scheduling_usage()
    }

    pub(super) fn reclaim_import_waiter_storage(&mut self) {
        if self.module_import_waiters.len().saturating_mul(4)
            < self.module_import_waiters.capacity()
        {
            self.module_import_waiters.shrink_to_fit();
        }
        if self.deferred_import_waiters.len().saturating_mul(4)
            < self.deferred_import_waiters.capacity()
        {
            self.deferred_import_waiters.shrink_to_fit();
        }
    }
}
