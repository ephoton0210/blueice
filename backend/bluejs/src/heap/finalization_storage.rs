// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finalization readiness is admitted before collection mutates weak state.

use super::*;
use crate::vm::{HeapNativeBudget, HeapNativeCharge};

#[derive(Default)]
pub(super) struct FinalizationIndex {
    data: HashMap<ObjectId, usize>,
    allocation_bytes: usize,
    budget: Option<HeapNativeBudget>,
    charge: Option<HeapNativeCharge>,
}
impl std::ops::Deref for FinalizationIndex {
    type Target = HashMap<ObjectId, usize>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl FinalizationIndex {
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
        HeapError::CollectorStorageLimit
    }
    fn prepare_additional(&mut self, additional: usize) -> Result<(), HeapError> {
        let required = self
            .data
            .len()
            .checked_add(additional)
            .ok_or_else(|| self.fail())?;
        if required <= self.data.capacity() {
            return Ok(());
        }
        let requested = required.max(self.data.len().saturating_mul(2));
        let mut incoming = self
            .budget
            .as_ref()
            .map(|budget| {
                budget
                    .reserve(requested.saturating_mul(size_of::<(ObjectId, usize)>()))
                    .map_err(|_| HeapError::CollectorStorageLimit)
            })
            .transpose()?;
        let mut replacement = HashMap::new();
        replacement
            .try_reserve(requested)
            .map_err(|_| self.fail())?;
        let actual = replacement
            .capacity()
            .saturating_mul(size_of::<(ObjectId, usize)>());
        if let Some(charge) = &mut incoming {
            charge
                .resize(actual)
                .map_err(|_| HeapError::CollectorStorageLimit)?;
        }
        if let Some(incoming) = incoming {
            if let Some(charge) = &mut self.charge {
                charge
                    .absorb(incoming)
                    .map_err(|_| HeapError::CollectorStorageLimit)?;
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
    pub(super) fn insert(&mut self, registry: ObjectId, index: usize) {
        // Updating an existing key in a full HashMap must not grow the table.
        if let Some(previous) = self.data.get_mut(&registry) {
            *previous = index;
        } else {
            assert!(
                self.data.len() < self.data.capacity(),
                "ready index admitted before publication"
            );
            self.data.insert(registry, index);
        }
    }
    pub(super) fn release_empty(&mut self) {
        if self.data.is_empty() {
            self.clear();
        }
    }
    pub(super) fn remove(&mut self, registry: &ObjectId) {
        self.data.remove(registry);
    }
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&ObjectId) -> bool) {
        self.data.retain(|id, _| keep(id));
        self.release_empty();
    }
    pub(super) fn clear(&mut self) {
        self.data = HashMap::new();
        self.allocation_bytes = 0;
        if let Some(charge) = &mut self.charge {
            charge.release_to(0);
        }
    }
}

impl Heap {
    pub(super) fn prepare_finalization_index(
        &mut self,
        young: bool,
        marked: &CollectorMarks,
    ) -> Result<(), HeapError> {
        let is_young = |id: &ObjectId| {
            id.heap == self.identity
                && self
                    .nursery
                    .binary_search_by_key(&id.serial, |id| id.serial)
                    .is_ok()
        };
        let additional = self
            .objects
            .iter()
            .filter(|(id, object)| {
                if (object.young || !young) && !marked.contains(id) {
                    return false;
                }
                let ObjectKind::FinalizationRegistry { cells, .. } = &object.kind else {
                    return false;
                };
                !self.pending_finalization.contains_key(id)
                    && cells.iter().any(|cell| {
                        cell.target.as_ref().is_none_or(|target| {
                            matches!(target, WeakCollectionKey::Object(id)
                        if (!young || is_young(id)) && !marked.contains(id))
                        })
                    })
            })
            .count();
        self.pending_finalization.prepare_additional(additional)
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn finalization_index_bytes(&self) -> usize {
        self.pending_finalization.retained_bytes()
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn finalization_cell_slot_bytes() -> usize {
        size_of::<FinalizationCell>()
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn finalization_cell_storage_state(&self, registry: ObjectId) -> (usize, usize) {
        let ObjectKind::FinalizationRegistry { cells, .. } = &self.objects[&registry].kind else {
            panic!("registry fixture");
        };
        (cells.len(), cells.capacity())
    }
}
