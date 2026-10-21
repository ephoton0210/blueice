// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Membrane table backing keeps its document owner when a realm record moves.

use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedMap;
use super::retained_control_map::RetainedControlMap;
use super::*;
use std::hash::Hash;

pub(super) struct MembraneMap<K, V> {
    // Backing and records must drop before their native credit.
    data: RetainedControlMap<K, V>,
    owner: Option<SharedSchedulingCharge>,
}

impl<K, V> Default for MembraneMap<K, V> {
    fn default() -> Self {
        Self {
            data: RetainedControlMap::default(),
            owner: None,
        }
    }
}
impl<K, V> std::ops::Deref for MembraneMap<K, V> {
    type Target = HashMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<'a, K, V> IntoIterator for &'a MembraneMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::hash_map::Iter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}
impl<K: Eq + Hash, V> MembraneMap<K, V> {
    pub(super) fn unowned_bytes(&self) -> usize {
        if self.owner.is_none() {
            self.data.retained_bytes()
        } else {
            0
        }
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.owner
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }
    pub(super) fn can_join_owner(&self, owner: &SharedSchedulingCharge) -> bool {
        self.owner
            .as_ref()
            .is_none_or(|current| current.same_owner(owner))
    }
    pub(super) fn attach_owner(
        &mut self,
        owner: &SharedSchedulingCharge,
    ) -> Result<(), RuntimeError> {
        if !self.can_join_owner(owner) {
            return Err(owner.fail());
        }
        if self.owner.is_none() && self.data.retained_bytes() != 0 {
            self.owner = Some(owner.reserve_peer(self.data.retained_bytes())?);
        }
        Ok(())
    }
    pub(super) fn prepare_insert_owned(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedMap<K, V>, RuntimeError> {
        if self.owner.is_none() {
            if let Some(owner) = hint {
                self.attach_owner(owner)?;
            }
        }
        PreparedMap::for_additional_owned(&self.data, 1, self.owner.as_ref().or(hint))
    }
    pub(super) fn prepare_set_owned(
        &mut self,
        key: &K,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedMap<K, V>, RuntimeError> {
        if self.owner.is_none() {
            if let Some(owner) = hint {
                self.attach_owner(owner)?;
            }
        }
        PreparedMap::for_additional_owned(
            &self.data,
            usize::from(!self.data.contains_key(key)),
            self.owner.as_ref().or(hint),
        )
    }
    pub(super) fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if !self.can_join(scope) {
            return Err(scope.fail());
        }
        if self.owner.is_none() && self.data.retained_bytes() != 0 {
            self.owner = Some(scope.shared_charge(self.data.retained_bytes())?);
        }
        Ok(())
    }
    /// Prepare after guest execution and heap allocation, before publishing any
    /// member of a paired membrane record. No guest work may intervene.
    pub(super) fn prepare_insert(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Result<PreparedMap<K, V>, RuntimeError> {
        self.prepare_additional(1, scope)
    }
    pub(super) fn prepare_set(
        &mut self,
        key: &K,
        scope: Option<&SchedulingScope>,
    ) -> Result<PreparedMap<K, V>, RuntimeError> {
        self.prepare_additional(usize::from(!self.data.contains_key(key)), scope)
    }
    fn prepare_additional(
        &mut self,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<PreparedMap<K, V>, RuntimeError> {
        if let Some(scope) = scope {
            self.attach(scope)?;
            // A reusable slot must also respect an already terminal document.
            scope.shared_charge(0)?;
        } else if self.owner.is_some() {
            return Err(RuntimeError::SchedulingLimit);
        }
        PreparedMap::for_additional(&self.data, additional, scope)
    }
    pub(super) fn commit(&mut self, prepared: PreparedMap<K, V>) {
        if let Some(owner) = prepared.commit_owned(&mut self.data) {
            // The old allocation has dropped before its credit is returned.
            self.owner = Some(owner);
        }
    }
    pub(super) fn insert(&mut self, key: K, value: V) {
        self.data.insert(key, value);
    }
    pub(super) fn set(&mut self, key: K, value: V) {
        if let Some(current) = self.data.get_mut(&key) {
            *current = value;
        } else {
            self.data.insert(key, value);
        }
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
    pub(super) fn remove<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
    {
        self.data.remove(key)
    }
    pub(super) fn retain(&mut self, keep: impl FnMut(&K, &mut V) -> bool) {
        self.data.retain(keep);
    }
    pub(super) fn release_empty(&mut self) {
        if self.data.is_empty() {
            self.data.release_empty();
            self.owner = None;
        }
    }
    pub(super) fn clear(&mut self) {
        self.data.retain(|_, _| false);
        self.release_empty();
    }
    #[cfg(any(test, coverage))]
    pub(super) fn retained_bytes(&self) -> usize {
        self.data.retained_bytes()
    }
}

impl Vm {
    pub(super) fn unowned_membrane_storage_bytes(&self) -> usize {
        let bytes = self
            .test262_realms
            .unowned_bytes()
            .saturating_add(self.test262_foreign_values.unowned_bytes())
            .saturating_add(self.test262_foreign_buffer_mirrors.unowned_bytes())
            .saturating_add(self.test262_reverse_values.unowned_bytes())
            .saturating_add(self.shadow_realms.unowned_bytes())
            .saturating_add(self.shadow_realm_by_heap.unowned_bytes())
            .saturating_add(self.shadow_wrapped_functions.unowned_bytes());
        self.test262_realms.values().fold(bytes, |bytes, realm| {
            bytes
                .saturating_add(realm.wrappers.unowned_bytes())
                .saturating_add(realm.imported_sources.unowned_bytes())
                .saturating_add(realm.imported_values.unowned_bytes())
                .saturating_add(realm.shadow_realm_reexports.unowned_bytes())
        })
    }
    pub(super) fn membrane_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.test262_realms.can_join(scope)
            && self.test262_foreign_values.can_join(scope)
            && self.test262_foreign_buffer_mirrors.can_join(scope)
            && self.test262_reverse_values.can_join(scope)
            && self.shadow_realms.can_join(scope)
            && self.shadow_realm_by_heap.can_join(scope)
            && self.shadow_wrapped_functions.can_join(scope)
            && self.test262_realms.values().all(|realm| {
                realm.wrappers.can_join(scope)
                    && realm.imported_sources.can_join(scope)
                    && realm.imported_values.can_join(scope)
                    && realm.shadow_realm_reexports.can_join(scope)
            })
    }
    pub(super) fn attach_membrane_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        if !self.test262_realms.can_join(scope)
            || !self.test262_foreign_values.can_join(scope)
            || !self.test262_foreign_buffer_mirrors.can_join(scope)
            || !self.test262_reverse_values.can_join(scope)
            || !self.shadow_realms.can_join(scope)
            || !self.shadow_realm_by_heap.can_join(scope)
            || !self.shadow_wrapped_functions.can_join(scope)
        {
            return Err(scope.fail());
        }
        self.test262_realms.attach(scope)?;
        self.test262_foreign_values.attach(scope)?;
        self.test262_foreign_buffer_mirrors.attach(scope)?;
        self.test262_reverse_values.attach(scope)?;
        self.shadow_realms.attach(scope)?;
        self.shadow_realm_by_heap.attach(scope)?;
        self.shadow_wrapped_functions.attach(scope)?;
        for record in self.shadow_realms.values() {
            // Foreign shared records remain owned by their original document.
            // Their own discovery node validates attachment when it can join.
            record.attach_owner_storage(scope)?;
        }
        for realm in self.test262_realms.values_mut() {
            // A Rust-held foreign record keeps its original owner. Activation
            // rejects this graph in its read-only discovery pass; ordinary
            // delivery of an already active document leaves it outside routing.
            if !realm.wrappers.can_join(scope)
                || !realm.imported_sources.can_join(scope)
                || !realm.imported_values.can_join(scope)
                || !realm.shadow_realm_reexports.can_join(scope)
            {
                continue;
            }
            realm.wrappers.attach(scope)?;
            realm.imported_sources.attach(scope)?;
            realm.imported_values.attach(scope)?;
            realm.shadow_realm_reexports.attach(scope)?;
        }
        Ok(())
    }
}
