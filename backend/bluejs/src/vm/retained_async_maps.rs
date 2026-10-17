// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scheduling and provenance maps retain their native allocation owners.

use super::*;

impl Vm {
    /// No staged registration may observe replacement of its old backing.
    /// Reconcile after pruning, and after each move, before refunding candidates.
    pub(super) fn reclaim_scheduling_maps(&mut self) {
        self.reconcile_scheduling_ownership();
        if self.preserve_promise_table_capacity {
            return;
        }
        let charge = self.promises.reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        let charge = self
            .promise_thunk_origins
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        self.reclaim_module_referrer_storage();
        let charge = self
            .disposable_stacks
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        let charge = self
            .async_disposable_stacks
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        let charge = self
            .host_promises
            .pending
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        let charge = self
            .async_continuations
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        let charge = self
            .module_continuations
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        self.reclaim_import_waiter_storage();
        self.reclaim_module_dependency_storage();
        self.reclaim_namespace_cache_storage();
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/retained_async_maps.rs"]
mod ownership_tests;
