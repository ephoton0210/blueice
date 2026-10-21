// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime map allocations outlive changes to their reported insertion capacity.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use std::hash::{BuildHasher, Hash};

/// Backing can retain an independent lease instead of a VM-field charge.
/// Deletion changes insertion headroom, never this allocation's ledger.
pub(super) struct RetainedControlMap<K, V, S = std::collections::hash_map::RandomState> {
    data: HashMap<K, V, S>,
    allocation_bytes: usize,
    // Records and the hash allocation die before their independent credit.
    storage: Option<SharedSchedulingCharge>,
}
impl<K, V, S: Default> Default for RetainedControlMap<K, V, S> {
    fn default() -> Self {
        Self {
            data: HashMap::with_hasher(S::default()),
            allocation_bytes: 0,
            storage: None,
        }
    }
}
impl<K, V, S> std::ops::Deref for RetainedControlMap<K, V, S> {
    type Target = HashMap<K, V, S>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

pub(super) struct PreparedCompaction<K, V, S> {
    replacement: HashMap<K, V, S>,
    previous: (usize, usize, usize),
    charge: Option<SharedSchedulingCharge>,
}

struct MapReplacement<K, V, S> {
    data: HashMap<K, V, S>,
    storage: Option<SharedSchedulingCharge>,
}

impl<K: Eq + Hash, V, S: BuildHasher + Clone> RetainedControlMap<K, V, S> {
    pub(super) fn retained_bytes(&self) -> usize {
        self.allocation_bytes
    }
    pub(super) fn unowned_bytes(&self) -> usize {
        if self.storage.is_some() {
            0
        } else {
            self.allocation_bytes
        }
    }
    pub(super) fn has_owner(&self) -> bool {
        self.storage.is_some()
    }
    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }
    /// The VM has detached its published bytes, or admitted neutral backing.
    pub(super) fn install_storage(&mut self, storage: SharedSchedulingCharge) {
        assert!(self.storage.is_none());
        assert_eq!(storage.bytes(), self.allocation_bytes);
        self.storage = Some(storage);
    }
    pub(super) fn has_sparse_backing(&self) -> bool {
        self.data
            .len()
            .saturating_mul(4)
            .saturating_mul(std::mem::size_of::<(K, V)>())
            < self.allocation_bytes
    }
    pub(super) fn get_mut<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
    {
        self.data.get_mut(key)
    }
    pub(super) fn values_mut(&mut self) -> std::collections::hash_map::ValuesMut<'_, K, V> {
        self.data.values_mut()
    }
    pub(super) fn iter_mut(&mut self) -> std::collections::hash_map::IterMut<'_, K, V> {
        self.data.iter_mut()
    }
    #[cfg(any(test, coverage))]
    pub(super) fn remove_entry<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: std::borrow::Borrow<Q>,
    {
        self.data.remove_entry(key)
    }
    pub(super) fn remove<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
    {
        self.data.remove(key)
    }
    pub(super) fn insert(&mut self, key: K, value: V) {
        assert!(!self.data.contains_key(&key), "record identity is new");
        assert!(
            self.data.len() < self.data.capacity(),
            "record slot is prepared"
        );
        self.data.insert(key, value);
    }
    pub(super) fn retain(&mut self, keep: impl FnMut(&K, &mut V) -> bool) {
        self.data.retain(keep);
    }
    pub(super) fn release_empty(&mut self) {
        assert!(self.data.is_empty(), "only dead records may be released");
        self.data = HashMap::with_hasher(self.data.hasher().clone());
        self.allocation_bytes = 0;
        if let Some(storage) = &mut self.storage {
            storage.release_to(0);
        }
    }
    /// Legacy field owners reconcile the moved backing in their VM scope.
    pub(super) fn replace_prepared(&mut self, replacement: HashMap<K, V, S>) {
        assert!(
            self.storage.is_none(),
            "independent maps transfer candidate credit"
        );
        drop(self.replace_prepared_owned(replacement, None));
    }
    pub(super) fn replace_prepared_owned(
        &mut self,
        replacement: HashMap<K, V, S>,
        storage: Option<SharedSchedulingCharge>,
    ) -> Option<SharedSchedulingCharge> {
        // Keep data before credit even if hashing unwinds during the move.
        let mut replacement = MapReplacement {
            data: replacement,
            storage,
        };
        assert!(replacement.data.is_empty());
        assert!(replacement.data.capacity() >= self.data.len());
        let bytes = host_scheduling::map_slots(&replacement.data);
        if let Some(owner) = &self.storage {
            let incoming = replacement
                .storage
                .as_ref()
                .expect("owned map has an admitted candidate");
            assert!(owner.same_owner(incoming));
            assert_eq!(incoming.bytes(), bytes);
        }
        let empty = HashMap::with_hasher(replacement.data.hasher().clone());
        replacement.data.extend(self.data.drain());
        self.data = std::mem::replace(&mut replacement.data, empty);
        self.allocation_bytes = bytes;
        if self.storage.is_some() {
            // The old allocation has dropped. Keep the complete new credit.
            self.storage = replacement.storage.take();
            None
        } else {
            replacement.storage.take()
        }
    }
    /// Compaction is optional reclamation. Refusal keeps the old allocation
    /// charged, without turning a successful collection into a document abort.
    pub(super) fn prepare_compaction(
        &self,
        scope: Option<&SchedulingScope>,
    ) -> Option<PreparedCompaction<K, V, S>> {
        assert!(!self.data.is_empty(), "empty backing needs no replacement");
        let minimum = self
            .data
            .len()
            .saturating_mul(std::mem::size_of::<(K, V)>());
        if minimum >= self.allocation_bytes {
            return None;
        }
        let charge = match &self.storage {
            Some(owner) => {
                if scope.is_some_and(|scope| !owner.belongs_to(scope)) {
                    return None;
                }
                Some(owner.try_reserve_peer(minimum)?)
            }
            None => match scope {
                Some(scope) => Some(scope.try_reclamation_charge(minimum)?),
                None => None,
            },
        };
        let mut prepared = PreparedCompaction {
            replacement: HashMap::with_hasher(self.data.hasher().clone()),
            previous: (self.data.len(), self.data.capacity(), self.allocation_bytes),
            charge,
        };
        prepared.replacement.try_reserve(self.data.len()).ok()?;
        let actual = host_scheduling::map_slots(&prepared.replacement);
        if actual >= self.allocation_bytes {
            return None;
        }
        if let Some(charge) = &mut prepared.charge {
            if !charge.try_resize_for_reclamation(actual) {
                return None;
            }
        }
        Some(prepared)
    }
    pub(super) fn commit_compaction(
        &mut self,
        prepared: PreparedCompaction<K, V, S>,
    ) -> Option<SharedSchedulingCharge> {
        assert_eq!(
            (self.data.len(), self.data.capacity(), self.allocation_bytes),
            prepared.previous
        );
        // Independent tables keep the candidate; VM-owned tables return it for
        // reconciliation of their smaller published allocation before refund.
        self.replace_prepared_owned(prepared.replacement, prepared.charge)
    }
    pub(super) fn reclaim(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Option<SharedSchedulingCharge> {
        if self.data.is_empty() {
            self.release_empty();
            None
        } else if self.has_sparse_backing() {
            let prepared = self.prepare_compaction(scope)?;
            self.commit_compaction(prepared)
        } else {
            None
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn reserve(&mut self, additional: usize) {
        // Fixture-only cache setup; ordinary producers use prepared replacement.
        self.data.reserve(additional);
        self.allocation_bytes = self
            .allocation_bytes
            .max(host_scheduling::map_slots(&self.data));
        if let Some(storage) = &mut self.storage {
            storage
                .resize(self.allocation_bytes)
                .expect("fixture reserve fits its native owner");
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn shrink_to_fit(&mut self) {
        // Fixture-only shaping of a full table before admission assertions.
        if self.data.is_empty() {
            self.release_empty();
        } else if let Some(prepared) = self.prepare_compaction(None) {
            self.commit_compaction(prepared);
        }
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/retained_control_map.rs"]
mod ownership_tests;
