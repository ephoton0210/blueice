// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise record storage stays detached until managed allocation succeeds.

use super::*;

pub(super) struct PreparedPromiseRecord {
    pub(super) promise: ObjectId,
    growth: promise_queue_storage::PreparedMap<ObjectId, PromiseRecord>,
}
impl PreparedPromiseRecord {
    /// No collection may run between preparation and publication.
    pub(super) fn commit(self, vm: &mut Vm) -> ObjectId {
        self.growth.commit_after_pruning(&mut vm.promises);
        vm.promises.insert(
            self.promise,
            PromiseRecord {
                status: PromiseStatus::Pending,
                rejection_origin: ThrowOrigin::default(),
                reactions: Vec::new(),
                handled: false,
                rejection_notified: false,
            },
        );
        self.promise
    }
}

impl Vm {
    pub(super) fn allocate_promise_record(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let before = self.heap.stats();
        let result = self.allocate_promise_record_inner(prototype);
        self.finish_promise_record_attempt(before);
        result
    }

    pub(super) fn finish_promise_record_attempt(&mut self, before: crate::heap::HeapStats) {
        let after = self.heap.stats();
        let collected = before.minor_collections != after.minor_collections
            || before.major_collections != after.major_collections;
        // The guard kept staged backing stable during managed allocation.
        // All prepared candidates have now committed or dropped. Reclaim any
        // pruned control storage, preserving cached capacity without real GC.
        if collected {
            self.reclaim_scheduling_maps();
        }
        self.reconcile_scheduling_ownership();
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
        let prepared = self.prepare_promise_record_with_capacity(growth, Some(prototype))?;
        Ok(prepared.commit(self))
    }

    pub(super) fn prepare_promise_record_with_capacity(
        &mut self,
        growth: promise_queue_storage::PreparedMap<ObjectId, PromiseRecord>,
        prototype: Option<ObjectId>,
    ) -> Result<PreparedPromiseRecord, RuntimeError> {
        // Intrinsic initialization and managed allocation may collect, but
        // neither runs JavaScript. Preserve admitted spare slots throughout.
        let previous = std::mem::replace(&mut self.preserve_promise_table_capacity, true);
        let allocation = (|| {
            let prototype = if let Some(prototype) = prototype {
                prototype
            } else {
                self.global("Promise")?;
                self.promise_prototype()?
            };
            self.with_roots(|heap| heap.alloc_object(Some(prototype)))
        })();
        self.preserve_promise_table_capacity = previous;
        let promise = allocation?;
        self.check_scheduling_failure()?;
        Ok(PreparedPromiseRecord { promise, growth })
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_record_storage.rs"]
mod ownership_tests;
