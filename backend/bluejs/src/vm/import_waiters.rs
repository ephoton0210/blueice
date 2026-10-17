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
        if let Some(waiters) = self.module_import_waiters.get_mut(&module) {
            // Discard the temporary lookup key before preparing new storage.
            drop(module);
            let growth = promise_queue_storage::PreparedVec::for_push(
                waiters,
                self.host_scheduling.as_ref(),
            )?;
            growth.commit(waiters);
            waiters.push(promise);
        } else {
            let incoming = self
                .host_scheduling
                .as_ref()
                .map(|scope| scope.shared_charge(module.capacity()))
                .transpose()?;
            let table = promise_queue_storage::PreparedMap::for_insert(
                &self.module_import_waiters,
                self.host_scheduling.as_ref(),
            )?;
            let mut waiters = Vec::new();
            let growth = promise_queue_storage::PreparedVec::for_push(
                &waiters,
                self.host_scheduling.as_ref(),
            )?;
            growth.commit(&mut waiters);
            table.commit(&mut self.module_import_waiters);
            waiters.push(promise);
            self.module_import_waiters.insert(module, waiters);
            drop(incoming);
        }
        self.reconcile_scheduling_ownership();
        Ok(())
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
        let prepared = self.prepare_import_dependencies(modules)?;
        let growth = promise_queue_storage::PreparedVec::for_push(
            &self.deferred_import_waiters,
            self.host_scheduling.as_ref(),
        )?;
        let (pending, charge) = prepared.into_owned();
        growth.commit(&mut self.deferred_import_waiters);
        self.deferred_import_waiters.push(DeferredImportWaiter {
            promise,
            namespace,
            pending,
        });
        drop(charge);
        self.reconcile_scheduling_ownership();
        Ok(())
    }

    pub(super) fn reclaim_import_waiter_storage(&mut self) {
        self.reconcile_scheduling_ownership();
        let charge = self
            .module_import_waiters
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        for index in 0..self.deferred_import_waiters.len() {
            let charge = self.deferred_import_waiters[index]
                .pending
                .reclaim(self.host_scheduling.as_ref());
            self.reconcile_scheduling_ownership();
            drop(charge);
        }
        self.reclaim_deferred_import_vector();
        self.reconcile_scheduling_ownership();
    }
}
