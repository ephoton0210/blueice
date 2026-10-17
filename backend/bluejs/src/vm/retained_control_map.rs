// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime map allocations outlive changes to their reported insertion capacity.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use std::hash::{BuildHasher, Hash};

/// The VM owns live backing; detached replacements own independent leases.
/// Deletion changes insertion headroom, never this allocation's ledger.
pub(super) struct RetainedControlMap<K, V, S = std::collections::hash_map::RandomState> {
    data: HashMap<K, V, S>,
    allocation_bytes: usize,
}
impl<K, V, S: Default> Default for RetainedControlMap<K, V, S> {
    fn default() -> Self {
        Self {
            data: HashMap::with_hasher(S::default()),
            allocation_bytes: 0,
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
impl<K: Eq + Hash, V, S: BuildHasher + Clone> RetainedControlMap<K, V, S> {
    pub(super) fn retained_bytes(&self) -> usize {
        self.allocation_bytes
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
    }
    pub(super) fn replace_prepared(&mut self, mut replacement: HashMap<K, V, S>) {
        assert!(replacement.is_empty());
        assert!(replacement.capacity() >= self.data.len());
        let bytes = host_scheduling::map_slots(&replacement);
        replacement.extend(self.data.drain());
        self.data = replacement;
        self.allocation_bytes = bytes;
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
        let charge = match scope {
            Some(scope) => Some(scope.try_reclamation_charge(minimum)?),
            None => None,
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
        self.replace_prepared(prepared.replacement);
        // The caller reconciles the VM's smaller live allocation before refund.
        prepared.charge
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
