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
        let required = self.data.len().checked_add(1).ok_or_else(|| self.fail())?;
        if required <= self.data.capacity() {
            return Ok(());
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
        if let Some(incoming) = incoming {
            if let Some(owner) = &mut self.charge {
                owner
                    .absorb(incoming)
                    .map_err(|_| HeapError::RootStorageLimit)?;
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
        Ok(())
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
    #[cfg(any(test, coverage))]
    pub(crate) fn root_storage_bytes(&self) -> usize {
        self.roots.retained_bytes()
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn root_storage_state(&self) -> (usize, usize, u64) {
        (self.roots.len(), self.roots.capacity(), self.next_root)
    }
}
