// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted foreign pins release without allocation or reborrowing an active VM.

use super::*;
use crate::vm::{HeapNativeBudget, HeapNativeCharge};
use std::cell::RefCell;

pub(crate) struct RealmRoot {
    pub(crate) id: RootId,
    charge: RefCell<Option<HeapNativeCharge>>,
}

impl RealmRoot {
    pub(crate) fn storage_bytes() -> usize {
        size_of::<Self>() + 2 * size_of::<usize>()
    }
}

#[derive(Default)]
pub(super) struct RealmPins {
    // Root IDs are monotonically increasing. Retain preserves their order.
    data: Vec<Rc<RealmRoot>>,
    budget: Option<HeapNativeBudget>,
    charge: Option<HeapNativeCharge>,
}
struct PreparedPin {
    replacement: Option<Vec<Rc<RealmRoot>>>,
    slots: Option<HeapNativeCharge>,
    header: Option<HeapNativeCharge>,
}
impl RealmPins {
    pub(super) fn retained_bytes(&self) -> usize {
        self.data.capacity() * size_of::<Rc<RealmRoot>>()
            + self.data.len() * RealmRoot::storage_bytes()
    }
    pub(super) fn budget(&self) -> Option<&HeapNativeBudget> {
        self.budget.as_ref()
    }
    pub(super) fn bind_admitted(&mut self, budget: HeapNativeBudget, mut charge: HeapNativeCharge) {
        for pin in &self.data {
            assert!(pin.charge.borrow().is_none());
            *pin.charge.borrow_mut() = Some(charge.split_off(RealmRoot::storage_bytes()));
        }
        self.budget = Some(budget);
        self.charge = Some(charge);
    }
    fn fail(&self) -> HeapError {
        if let Some(budget) = &self.budget {
            let _ = budget.fail();
        }
        HeapError::RootStorageLimit
    }
    fn reserve(&self, bytes: usize) -> Result<Option<HeapNativeCharge>, HeapError> {
        self.budget
            .as_ref()
            .map(|budget| {
                budget
                    .reserve(bytes)
                    .map_err(|_| HeapError::RootStorageLimit)
            })
            .transpose()
    }
    fn prepare(&self) -> Result<PreparedPin, HeapError> {
        let header = self.reserve(RealmRoot::storage_bytes())?;
        let mut prepared = PreparedPin {
            replacement: None,
            slots: None,
            header,
        };
        let required = self.data.len().checked_add(1).ok_or_else(|| self.fail())?;
        if required > self.data.capacity() {
            let capacity = required.max(self.data.capacity().saturating_mul(2)).max(4);
            let mut slots = self.reserve(capacity.saturating_mul(size_of::<Rc<RealmRoot>>()))?;
            let mut replacement = Vec::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|_| self.fail())?;
            if let Some(charge) = &mut slots {
                charge
                    .resize(replacement.capacity() * size_of::<Rc<RealmRoot>>())
                    .map_err(|_| HeapError::RootStorageLimit)?;
            }
            prepared.replacement = Some(replacement);
            prepared.slots = slots;
        }
        Ok(prepared)
    }
    fn insert(&mut self, pin: Rc<RealmRoot>, mut prepared: PreparedPin) {
        if let Some(mut replacement) = prepared.replacement.take() {
            replacement.append(&mut self.data);
            self.data = replacement;
            self.charge = prepared.slots;
        }
        assert!(self.data.len() < self.data.capacity());
        self.data.push(pin);
    }
    pub(super) fn is_released(&self, root: &RootId) -> bool {
        self.data
            .binary_search_by_key(&root.serial, |pin| pin.id.serial)
            .is_ok_and(|index| Rc::strong_count(&self.data[index]) == 1)
    }
    fn release(&mut self, roots: &mut RootTable) {
        self.data.retain(|pin| {
            if Rc::strong_count(pin) == 1 {
                roots.remove(&pin.id);
                false
            } else {
                true
            }
        });
        if self.data.is_empty() {
            self.data = Vec::new();
            self.charge = None;
        }
    }
}

impl Heap {
    pub(crate) fn realm_root(&mut self, target: ObjectId) -> Result<Rc<RealmRoot>, HeapError> {
        self.object(target)?;
        self.next_root
            .checked_add(1)
            .ok_or(HeapError::IdExhausted)?;
        let mut prepared = self.realm_pins.prepare()?;
        let id = self.root(target)?;
        let pin = Rc::new(RealmRoot {
            id,
            charge: RefCell::new(prepared.header.take()),
        });
        self.realm_pins.insert(pin.clone(), prepared);
        Ok(pin)
    }

    pub(crate) fn release_realm_roots(&mut self) {
        self.realm_pins.release(&mut self.roots);
    }

    #[cfg(any(test, coverage))]
    pub(crate) fn realm_pin_storage_bytes(&self) -> usize {
        self.realm_pins.retained_bytes()
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn realm_pin_storage_state(&self) -> (usize, usize) {
        (self.realm_pins.data.len(), self.realm_pins.data.capacity())
    }

    pub(crate) fn realm_reachable(
        &mut self,
        ignored: &HashSet<RootId>,
        extra: &[ObjectId],
    ) -> Result<CollectorMarks, HeapError> {
        self.mark_without_roots(false, ProtectedRoots::Slice(extra), ignored)
    }
}
