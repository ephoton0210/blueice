// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind collector, persistent-root and foreign-pin storage to the native account.

use super::*;

impl Vm {
    pub(super) fn retain_async_generator_request_counts(
        &mut self,
        budget: &HeapNativeBudget,
    ) -> Result<(), RuntimeError> {
        self.retain_async_generator_request_counts_for(budget, None)
    }
    pub(super) fn retain_async_generator_request_counts_for(
        &mut self,
        budget: &HeapNativeBudget,
        only: Option<ObjectId>,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            if !scope.heap_native_budget().same_document(budget) {
                return Err(scope.fail());
            }
        }
        if !self.heap.async_generator_counts_can_join_for(budget, only) {
            return Err(budget.fail());
        }
        let count = self.heap.unowned_async_generator_requests_for(only);
        let credit = if count == 0 {
            None
        } else {
            Some(match &self.host_scheduling {
                Some(_) => {
                    // Bind runs before the ordinary admission update. Publish
                    // existing fields once before transferring their credit.
                    let own = self.own_scheduling_usage();
                    let scope = self.host_scheduling.as_mut().unwrap();
                    scope.check(own, HostSchedulingUsage::default())?;
                    let mut credit = scope.detach_owned_payload(
                        count.saturating_mul(HeapReactionLease::allocation_bytes()),
                    );
                    credit
                        .absorb(scope.detach_owned_reactions(count))
                        .expect("same document");
                    credit
                }
                None => budget.reserve_request_credit(count)?,
            })
        };
        let result = self
            .heap
            .attach_async_generator_request_budget(budget, credit, only);
        self.reconcile_scheduling_ownership();
        result?;
        Ok(())
    }

    pub(super) fn bind_native_budget(&mut self) -> Result<(), RuntimeError> {
        if let Some(scope) = self.host_scheduling.as_ref().map(SchedulingScope::child) {
            self.attach_membrane_storage(&scope)?;
            let budget = scope.heap_native_budget();
            if let Some(home) = &self.realm_home {
                home.attach(&scope)?;
            }
            self.retain_async_generator_request_counts(&budget)?;
            self.heap.bind_native_budget(budget)?;
            self.kept_weak_objects.attach(&scope)?;
        }
        self.attach_retained_debugger_storage()
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/collector_workspace.rs"]
mod ownership_tests;
