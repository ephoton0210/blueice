// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Root registry allocations retain their document native owner.

use super::*;
use crate::vm::{HeapNativeBudget, HeapNativeCharge};

#[derive(Default)]
pub(super) struct RootTable {
    data: HashMap<RootId, ObjectId>,
    // Removal can reduce HashMap::capacity without freeing its allocation.
    // Keep the allocation's admitted slot count until replacement or release.
    allocation_bytes: usize,
    budget: Option<HeapNativeBudget>,
    charge: Option<HeapNativeCharge>,
}

struct PreparedRootTable {
    replacement: Option<HashMap<RootId, ObjectId>>,
    incoming: Option<HeapNativeCharge>,
    previous_len: usize,
    previous_capacity: usize,
}

/// Internal native transaction: no collection or root mutation may run between
/// preparation and commit. Input IDs are borrowed without scratch allocation.
pub(crate) struct PreparedRootBatch<'a> {
    objects: &'a [ObjectId],
    table: PreparedRootTable,
    heap: u64,
    first: u64,
    next: u64,
    registrations: u64,
}
impl std::ops::Deref for RootTable {
    type Target = HashMap<RootId, ObjectId>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl RootTable {
    pub(super) fn retained_bytes(&self) -> usize {
        self.allocation_bytes
    }
    pub(super) fn budget(&self) -> Option<&HeapNativeBudget> {
        self.budget.as_ref()
    }
    pub(super) fn bind_admitted(&mut self, budget: HeapNativeBudget, charge: HeapNativeCharge) {
        self.budget = Some(budget);
        self.charge = Some(charge);
    }
    fn fail(&self) -> HeapError {
        if let Some(budget) = &self.budget {
            let _ = budget.fail();
        }
        HeapError::RootStorageLimit
    }
    pub(super) fn prepare_registration(&mut self) -> Result<(), HeapError> {
        let prepared = self.prepare_additional(1)?;
        self.commit_prepared(prepared);
        Ok(())
    }
    fn prepare_additional(&self, count: usize) -> Result<PreparedRootTable, HeapError> {
        let required = self
            .data
            .len()
            .checked_add(count)
            .ok_or_else(|| self.fail())?;
        let mut prepared = PreparedRootTable {
            replacement: None,
            incoming: None,
            previous_len: self.data.len(),
            previous_capacity: self.data.capacity(),
        };
        if required <= self.data.capacity() {
            return Ok(prepared);
        }
        let requested = required.max(self.data.len().saturating_mul(2));
        let minimum = requested.saturating_mul(size_of::<(RootId, ObjectId)>());
        let mut incoming = self
            .budget
            .as_ref()
            .map(|budget| {
                budget
                    .reserve(minimum)
                    .map_err(|_| HeapError::RootStorageLimit)
            })
            .transpose()?;
        let mut replacement = HashMap::new();
        replacement
            .try_reserve(requested)
            .map_err(|_| self.fail())?;
        let actual = replacement
            .capacity()
            .saturating_mul(size_of::<(RootId, ObjectId)>());
        if let Some(charge) = &mut incoming {
            charge
                .resize(actual)
                .map_err(|_| HeapError::RootStorageLimit)?;
        }
        prepared.replacement = Some(replacement);
        prepared.incoming = incoming;
        Ok(prepared)
    }
    fn commit_prepared(&mut self, prepared: PreparedRootTable) {
        assert_eq!(self.data.len(), prepared.previous_len);
        assert_eq!(self.data.capacity(), prepared.previous_capacity);
        let Some(mut replacement) = prepared.replacement else {
            return;
        };
        let actual = replacement
            .capacity()
            .saturating_mul(size_of::<(RootId, ObjectId)>());
        if let Some(incoming) = prepared.incoming {
            if let Some(owner) = &mut self.charge {
                owner
                    .absorb(incoming)
                    .expect("prepared roots retain their original native account");
            } else {
                self.charge = Some(incoming);
            }
        }
        replacement.extend(self.data.drain());
        self.data = replacement;
        self.allocation_bytes = actual;
        if let Some(charge) = &mut self.charge {
            charge.release_to(actual);
        }
    }
    pub(super) fn insert(&mut self, id: RootId, object: ObjectId) {
        assert!(
            self.data.len() < self.data.capacity(),
            "root slot admitted before publication"
        );
        assert!(
            !self.data.contains_key(&id),
            "root identities are never reused"
        );
        self.data.insert(id, object);
    }
    pub(super) fn remove(&mut self, id: &RootId) -> Option<ObjectId> {
        let object = self.data.remove(id)?;
        if self.data.is_empty() {
            self.data = HashMap::new();
            self.allocation_bytes = 0;
            if let Some(charge) = &mut self.charge {
                charge.release_to(0);
            }
        }
        Some(object)
    }
}

impl Heap {
    pub(crate) fn check_root_registration_count(&self, count: usize) -> Result<(), HeapError> {
        self.root_registration_range(count).map(|_| ())
    }

    fn root_registration_range(&self, count: usize) -> Result<(u64, u64), HeapError> {
        let count = u64::try_from(count).map_err(|_| HeapError::IdExhausted)?;
        let next = self
            .next_root
            .checked_add(count)
            .ok_or(HeapError::IdExhausted)?;
        let registrations = self
            .root_registrations
            .checked_add(count)
            .ok_or(HeapError::IdExhausted)?;
        Ok((next, registrations))
    }

    #[cfg(any(test, coverage))]
    pub(crate) fn set_root_counter_for_test(&mut self, next: u64) {
        self.next_root = next;
    }

    pub(crate) fn prepare_root_batch<'a>(
        &self,
        objects: &'a [ObjectId],
    ) -> Result<PreparedRootBatch<'a>, HeapError> {
        for object in objects {
            self.object(*object)?;
        }
        let (next, registrations) = self.root_registration_range(objects.len())?;
        Ok(PreparedRootBatch {
            objects,
            table: self.roots.prepare_additional(objects.len())?,
            heap: self.identity,
            first: self.next_root,
            next,
            registrations,
        })
    }

    pub(crate) fn commit_root_batch(
        &mut self,
        prepared: PreparedRootBatch<'_>,
        roots: &mut Vec<RootId>,
    ) {
        assert!(roots.capacity() - roots.len() >= prepared.objects.len());
        self.commit_root_batch_using(prepared, |root| roots.push(root));
    }

    pub(crate) fn commit_single_root(&mut self, prepared: PreparedRootBatch<'_>) -> RootId {
        assert_eq!(prepared.objects.len(), 1);
        let mut root = None;
        self.commit_root_batch_using(prepared, |id| root = Some(id));
        root.expect("one admitted root")
    }

    fn commit_root_batch_using(
        &mut self,
        prepared: PreparedRootBatch<'_>,
        mut publish: impl FnMut(RootId),
    ) {
        assert_eq!(self.identity, prepared.heap);
        assert_eq!(self.next_root, prepared.first);
        self.roots.commit_prepared(prepared.table);
        for (index, object) in prepared.objects.iter().enumerate() {
            let root = RootId {
                heap: self.identity,
                serial: prepared.first + index as u64,
            };
            self.roots.insert(root, *object);
            publish(root);
        }
        self.next_root = prepared.next;
        self.root_registrations = prepared.registrations;
    }

    #[cfg(any(test, coverage))]
    pub(crate) fn root_storage_bytes(&self) -> usize {
        self.roots.retained_bytes()
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn root_storage_state(&self) -> (usize, usize, u64) {
        (self.roots.len(), self.roots.capacity(), self.next_root)
    }
}
