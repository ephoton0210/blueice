// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise record storage stays detached until managed allocation succeeds.

use super::*;

impl Vm {
    pub(super) fn allocate_promise_record(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let before = self.heap.stats();
        let result = self.allocate_promise_record_inner(prototype);
        let after = self.heap.stats();
        let collected = before.minor_collections != after.minor_collections
            || before.major_collections != after.major_collections;
        // Collection may have emptied the old table during a refused creation.
        // Preserve an existing empty cache when preparation failed without GC.
        // Releasing an actually collected empty table requires no allocation.
        if collected && !self.preserve_promise_table_capacity && self.promises.is_empty() {
            self.promises = HashMap::new();
        }
        self.reconcile_scheduling_ownership();
        result
    }

    fn allocate_promise_record_inner(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let growth = promise_queue_storage::PreparedMap::for_insert(
            &self.promises,
            self.host_scheduling.as_ref(),
        )?;
        // GC may remove dead records, but an already-admitted spare slot must
        // survive until the new object can be published. No JavaScript runs.
        let previous = std::mem::replace(&mut self.preserve_promise_table_capacity, true);
        let allocation = self.with_roots(|heap| heap.alloc_object(Some(prototype)));
        self.preserve_promise_table_capacity = previous;
        let promise = allocation?;
        self.check_scheduling_failure()?;
        growth.commit_after_pruning(&mut self.promises);
        self.promises.insert(
            promise,
            PromiseRecord {
                status: PromiseStatus::Pending,
                rejection_origin: ThrowOrigin::default(),
                reactions: Vec::new(),
                handled: false,
                rejection_notified: false,
            },
        );
        Ok(promise)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_record_storage.rs"]
mod ownership_tests;
