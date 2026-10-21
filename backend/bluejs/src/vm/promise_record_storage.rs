// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise record storage stays detached until managed allocation succeeds.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct PreparedPromiseRecord {
    pub(super) promise: ObjectId,
    growth: promise_queue_storage::PreparedMap<ObjectId, PromiseRecord>,
    status_storage: Option<SharedSchedulingCharge>,
}
impl PreparedPromiseRecord {
    /// No collection may run between preparation and publication.
    pub(super) fn commit(self, vm: &mut Vm) -> ObjectId {
        let mut status_storage = self.status_storage;
        let reaction_storage = status_storage.as_mut().map(|owner| owner.split_off(0));
        self.growth.commit_after_pruning(&mut vm.promises);
        vm.promises.insert(
            self.promise,
            PromiseRecord {
                status: PromiseStatus::Pending,
                rejection_origin: ThrowOrigin::default(),
                reactions: Vec::new(),
                handled: false,
                rejection_notified: false,
                status_storage,
                reaction_storage,
            },
        );
        self.promise
    }
}

impl PromiseRecord {
    pub(super) fn unowned_reaction_count(&self) -> usize {
        if self.reaction_storage.is_some() {
            0
        } else {
            self.reactions.len()
        }
    }

    fn reaction_payload_bytes(&self) -> usize {
        self.reactions.iter().fold(0usize, |bytes, reaction| {
            bytes.saturating_add(host_scheduling::reaction_bytes(reaction))
        })
    }

    fn reaction_storage_bytes(&self) -> usize {
        host_scheduling::slots(&self.reactions).saturating_add(self.reaction_payload_bytes())
    }

    pub(super) fn unowned_reaction_payload_bytes(&self) -> usize {
        if self.reaction_storage.is_some() {
            0
        } else {
            self.reaction_storage_bytes()
        }
    }

    fn reaction_owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.reaction_storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(super) fn publish_reaction(
        &mut self,
        growth: promise_queue_storage::PreparedVec<PromiseReaction>,
        reaction: PromiseReaction,
        input: Option<SharedSchedulingCharge>,
    ) {
        let payload = self.reaction_payload_bytes();
        if let Some(backing) = growth.commit_owned(&mut self.reactions) {
            let storage = self
                .reaction_storage
                .as_mut()
                .expect("prepared reaction owner");
            // The old Vec has been destroyed; its elements remain retained.
            storage.release_to(payload);
            storage
                .absorb(backing)
                .expect("prepared backing shares the original account");
        }
        self.reactions.push(reaction);
        if let Some(input) = input {
            self.reaction_storage
                .as_mut()
                .expect("prepared reaction owner")
                .absorb(input)
                .expect("prepared input shares the original account");
        }
        debug_assert!(self
            .reaction_storage
            .as_ref()
            .is_none_or(|storage| storage.bytes() == self.reaction_storage_bytes()));
        debug_assert!(self
            .reaction_storage
            .as_ref()
            .is_none_or(|storage| storage.reaction_count() == self.reactions.len()));
    }

    pub(super) fn release_reactions(&mut self) {
        self.reactions = Vec::new();
        if let Some(storage) = &mut self.reaction_storage {
            storage.release_to(0);
            storage.release_reactions_to(0);
        }
    }

    #[cfg(any(test, coverage))]
    fn reconcile_reaction_removal(&mut self) {
        let bytes = self.reaction_storage_bytes();
        let count = self.reactions.len();
        if let Some(storage) = &mut self.reaction_storage {
            storage.release_to(bytes);
            storage.release_reactions_to(count);
        }
    }

    #[cfg(any(test, coverage))]
    pub(super) fn retain_reactions(&mut self, predicate: impl FnMut(&PromiseReaction) -> bool) {
        self.reactions.retain(predicate);
        self.reconcile_reaction_removal();
    }

    #[cfg(any(test, coverage))]
    pub(super) fn truncate_reactions(&mut self, count: usize) {
        self.reactions.truncate(count);
        self.reconcile_reaction_removal();
    }

    pub(super) fn status_payload_bytes(&self) -> usize {
        let value = match &self.status {
            PromiseStatus::Pending => 0,
            PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => {
                host_scheduling::value_payload_bytes(value)
            }
        };
        value.saturating_add(self.rejection_origin.payload_bytes())
    }

    pub(super) fn unowned_status_payload_bytes(&self) -> usize {
        if self.status_storage.is_some() {
            0
        } else {
            self.status_payload_bytes()
        }
    }

    pub(super) fn status_owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.status_storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
}

impl Vm {
    pub(super) fn promise_map_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.promises.can_join(scope)
            && self.host_promises.pending.can_join(scope)
            && self.host_promises.counts_can_join(scope)
            && self.promise_jobs.can_join(scope)
            && self.promises.values().all(|record| {
                record
                    .status_storage
                    .as_ref()
                    .is_none_or(|owner| owner.belongs_to(scope))
                    && record
                        .reaction_storage
                        .as_ref()
                        .is_none_or(|owner| owner.belongs_to(scope))
            })
    }

    pub(super) fn validate_promise_map_owners(&self) -> Result<(), RuntimeError> {
        self.promises.owner_view()?;
        self.host_promises.pending.owner_view()?;
        self.host_promises.validate_counts()?;
        for record in self.promises.values() {
            record.status_owner()?;
            record.reaction_owner()?;
        }
        Ok(())
    }

    // Call only after synchronizing published VM fields. Legacy data acquires
    // its original credit before new table or reaction storage can publish.
    fn prepare_promise_record_adoption(
        &mut self,
        owner: &SharedSchedulingCharge,
    ) -> Result<SharedSchedulingCharge, RuntimeError> {
        for record in self.promises.values() {
            record.status_owner()?;
            record.reaction_owner()?;
            if let (Some(scope), Some(storage)) = (&self.host_scheduling, &record.status_storage) {
                if !storage.belongs_to(scope) {
                    return Err(scope.fail());
                }
            }
            if let (Some(scope), Some(storage)) = (&self.host_scheduling, &record.reaction_storage)
            {
                if !storage.belongs_to(scope) {
                    return Err(scope.fail());
                }
            }
        }
        if self.host_scheduling.is_none() {
            for record in self.promises.values_mut() {
                if record.reaction_storage.is_none() {
                    // A legacy reaction buffer follows its record, even if
                    // the enclosing table has a different backing account.
                    let original = record.status_storage.as_ref().unwrap_or(owner);
                    let mut storage = original.reserve_peer(record.reaction_storage_bytes())?;
                    storage.absorb(original.reserve_reactions(record.reactions.len())?)?;
                    record.reaction_storage = Some(storage);
                }
            }
        }
        let bytes = self.promises.values().fold(0usize, |bytes, record| {
            bytes
                .saturating_add(record.unowned_status_payload_bytes())
                .saturating_add(record.unowned_reaction_payload_bytes())
        });
        let count = self
            .promises
            .values()
            .map(PromiseRecord::unowned_reaction_count)
            .sum();
        match &mut self.host_scheduling {
            Some(scope) => {
                let mut storage = scope.detach_owned_payload(bytes);
                storage
                    .absorb(scope.detach_owned_reactions(count))
                    .expect("published record shares the document account");
                Ok(storage)
            }
            None => {
                let mut storage = owner.reserve_peer(bytes)?;
                storage.absorb(owner.reserve_reactions(count)?)?;
                Ok(storage)
            }
        }
    }

    fn publish_promise_record_adoption(&mut self, mut storage: SharedSchedulingCharge) {
        for record in self.promises.values_mut() {
            if record.status_storage.is_none() {
                record.status_storage = Some(storage.split_off(record.status_payload_bytes()));
            }
            if record.reaction_storage.is_none() {
                let mut reactions = storage.split_off(record.reaction_storage_bytes());
                reactions
                    .absorb(storage.split_reactions(record.reactions.len()))
                    .expect("adopted reaction shares its record account");
                record.reaction_storage = Some(reactions);
            }
        }
        debug_assert_eq!(storage.bytes(), 0);
        debug_assert_eq!(storage.reaction_count(), 0);
    }

    pub(super) fn retain_host_wait_map_storage(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        let promises = self.promises.owner_view()?;
        let waits = self.host_promises.pending.owner_view()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.promise_map_storage_can_join(scope)
                || hint.is_some_and(|owner| !owner.belongs_to(scope))
            {
                return Err(scope.fail());
            }
        }
        let owner = hint.or(promises.as_ref()).or(waits.as_ref());
        let Some(owner) = owner else {
            return Ok(());
        };
        self.synchronize_scheduling_usage()?;
        let status_storage = self.prepare_promise_record_adoption(owner)?;
        let need_waits = !self.host_promises.pending.has_owner();
        let need_promises = !self.promises.has_owner();
        let (wait_storage, promise_storage) = if let Some(scope) = &mut self.host_scheduling {
            // Move existing published field credit, without a duplicate peak.
            let waits = need_waits
                .then(|| scope.detach_owned_payload(self.host_promises.pending.retained_bytes()));
            let promises =
                need_promises.then(|| scope.detach_owned_payload(self.promises.retained_bytes()));
            (waits, promises)
        } else {
            // Stage both adoptions before publishing either ownership view.
            let waits = need_waits
                .then(|| owner.reserve_peer(self.host_promises.pending.retained_bytes()))
                .transpose()?;
            let promises = need_promises
                .then(|| owner.reserve_peer(self.promises.retained_bytes()))
                .transpose()?;
            (waits, promises)
        };
        if let Some(storage) = wait_storage {
            self.host_promises.pending.install_storage(storage);
        }
        if let Some(storage) = promise_storage {
            self.promises.install_storage(storage);
        }
        self.publish_promise_record_adoption(status_storage);
        self.retain_host_wait_counts(Some(owner))?;
        Ok(())
    }

    pub(super) fn retain_promise_table_storage(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        let retained = self.promises.owner_view()?;
        let current = self.argument_list_owner()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.promises.can_join(scope) {
                return Err(scope.fail());
            }
        }
        let owner = retained.as_ref().or(current.as_ref()).or(hint);
        let Some(owner) = owner else {
            return Ok(());
        };
        self.synchronize_scheduling_usage()?;
        let status_storage = self.prepare_promise_record_adoption(owner)?;
        let bytes = self.promises.retained_bytes();
        if !self.promises.has_owner() {
            let storage = match &mut self.host_scheduling {
                Some(scope) => scope.detach_owned_payload(bytes),
                None => owner.reserve_peer(bytes)?,
            };
            self.promises.install_storage(storage);
        }
        self.publish_promise_record_adoption(status_storage);
        Ok(())
    }

    pub(super) fn prepare_promise_record_owner(
        &mut self,
        promise: ObjectId,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        if !self.promises.contains_key(&promise) {
            return Err(RuntimeError::TypeError("invalid Promise receiver".into()));
        }
        self.retain_promise_table_storage(hint)?;
        self.promises[&promise].status_owner()
    }

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
        self.retain_promise_table_storage(None)?;
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
        let status_storage = self.promises.owner_view()?;
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
        growth.validate_owner()?;
        if let Some(owner) = &status_storage {
            owner.reserve_peer(0)?;
        }
        Ok(PreparedPromiseRecord {
            promise,
            growth,
            status_storage,
        })
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_record_storage.rs"]
mod ownership_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_reaction_counts.rs"]
mod count_tests;

#[cfg(any(test, coverage))]
impl Vm {
    /// Promise table backing and per-record status/reaction storage that an
    /// account already owns (the unowned remainder is in `own_scheduling_usage`).
    pub(super) fn owned_promise_storage_bytes(&self) -> usize {
        let records: usize = self
            .promises
            .values()
            .map(|record| {
                [&record.status_storage, &record.reaction_storage]
                    .into_iter()
                    .flatten()
                    .map(SharedSchedulingCharge::bytes)
                    .sum::<usize>()
            })
            .sum();
        records + (self.promises.retained_bytes() - self.promises.unowned_bytes())
    }
}
