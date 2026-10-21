// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import registration prepares complete coexisting native owners.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[derive(Default)]
pub(super) struct ImportDependencies {
    data: HashSet<String>,
    allocation_bytes: usize,
}
impl std::ops::Deref for ImportDependencies {
    type Target = HashSet<String>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl ImportDependencies {
    pub(super) fn backing_bytes(&self) -> usize {
        self.allocation_bytes
    }
    pub(super) fn retain(&mut self, keep: impl FnMut(&String) -> bool) {
        self.data.retain(keep);
    }
    pub(super) fn insert(&mut self, name: String) {
        assert!(!self.data.contains(&name), "dependency identity is new");
        assert!(
            self.data.len() < self.data.capacity(),
            "dependency slot is prepared"
        );
        self.data.insert(name);
    }
    pub(super) fn replace_prepared(&mut self, mut data: HashSet<String>) {
        assert!(data.is_empty());
        assert!(data.capacity() >= self.data.len());
        let bytes = data
            .capacity()
            .saturating_mul(std::mem::size_of::<String>());
        data.extend(self.data.drain());
        self.data = data;
        self.allocation_bytes = bytes;
    }
    #[cfg(any(test, coverage))]
    pub(super) fn reserve(&mut self, additional: usize) {
        self.data.reserve(additional);
        self.allocation_bytes = self
            .allocation_bytes
            .max(self.data.capacity() * std::mem::size_of::<String>());
    }
    #[cfg(any(test, coverage))]
    pub(super) fn take(&mut self, name: &str) -> Option<String> {
        self.data.take(name)
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.data.iter().fold(self.allocation_bytes, |bytes, name| {
            bytes.saturating_add(name.capacity())
        })
    }
    pub(super) fn remove(&mut self, name: &str) {
        self.data.remove(name);
    }
    pub(super) fn reclaim(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Option<SharedSchedulingCharge> {
        if self.data.is_empty() {
            self.data = HashSet::new();
            self.allocation_bytes = 0;
            return None;
        }
        let minimum = self
            .data
            .len()
            .saturating_mul(std::mem::size_of::<String>());
        if minimum.saturating_mul(4) >= self.allocation_bytes {
            return None;
        }
        let mut charge = match scope {
            Some(scope) => Some(scope.try_reclamation_charge(minimum)?),
            None => None,
        };
        let mut replacement = HashSet::new();
        replacement.try_reserve(self.data.len()).ok()?;
        let actual = replacement
            .capacity()
            .saturating_mul(std::mem::size_of::<String>());
        if actual >= self.allocation_bytes {
            return None;
        }
        if let Some(charge) = &mut charge {
            if !charge.try_resize_for_reclamation(actual) {
                return None;
            }
        }
        replacement.extend(self.data.drain());
        self.data = replacement;
        self.allocation_bytes = actual;
        charge
    }
}

impl IntoIterator for ImportDependencies {
    type Item = String;
    type IntoIter = std::collections::hash_set::IntoIter<String>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.into_iter()
    }
}

pub(super) struct PreparedImportDependencies {
    input: Vec<String>,
    pending: ImportDependencies,
    charge: Option<SharedSchedulingCharge>,
}
impl PreparedImportDependencies {
    pub(super) fn into_owned(self) -> (ImportDependencies, Option<SharedSchedulingCharge>) {
        let Self {
            pending, charge, ..
        } = self;
        (pending, charge)
    }
}

#[derive(Default)]
pub(super) struct DeferredImportDependencies {
    pub(super) data: ImportDependencies,
    // Dependency buffers die before their payload and pending-count credit.
    pub(super) storage: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for DeferredImportDependencies {
    type Target = ImportDependencies;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl DeferredImportDependencies {
    pub(super) fn unowned_bytes(&self) -> usize {
        if self.storage.is_none() {
            self.data.retained_bytes()
        } else {
            0
        }
    }
    pub(super) fn unowned_count(&self) -> usize {
        usize::from(self.storage.is_none())
    }
    pub(super) fn remove(&mut self, name: &str) {
        self.data.remove(name);
        if let Some(storage) = &mut self.storage {
            storage.release_to(self.data.retained_bytes());
        }
    }
    pub(super) fn reclaim(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Option<SharedSchedulingCharge> {
        let Some(storage) = &mut self.storage else {
            return self.data.reclaim(scope);
        };
        if !self.data.is_empty() && scope.is_some_and(|scope| !storage.belongs_to(scope)) {
            return None;
        }
        let original = storage.scheduling_scope();
        let candidate = self.data.reclaim(Some(&original));
        if let Some(candidate) = candidate {
            let names = self
                .data
                .retained_bytes()
                .saturating_sub(self.data.backing_bytes());
            storage.release_to(names);
            storage
                .absorb(candidate)
                .expect("dependency backing keeps its original account");
        } else {
            storage.release_to(self.data.retained_bytes());
        }
        None
    }
}

#[derive(Default)]
pub(super) struct DeferredImportWaiters {
    pub(super) data: Vec<DeferredImportWaiter>,
    // Records and the vector allocation are destroyed before backing credit.
    pub(super) storage: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for DeferredImportWaiters {
    type Target = Vec<DeferredImportWaiter>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::Index<usize> for DeferredImportWaiters {
    type Output = DeferredImportWaiter;
    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}
impl std::ops::IndexMut<usize> for DeferredImportWaiters {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}
impl<'a> IntoIterator for &'a DeferredImportWaiters {
    type Item = &'a DeferredImportWaiter;
    type IntoIter = std::slice::Iter<'a, DeferredImportWaiter>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}
impl<'a> IntoIterator for &'a mut DeferredImportWaiters {
    type Item = &'a mut DeferredImportWaiter;
    type IntoIter = std::slice::IterMut<'a, DeferredImportWaiter>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter_mut()
    }
}
impl DeferredImportWaiters {
    pub(super) fn backing_bytes(&self) -> usize {
        host_scheduling::slots(&self.data)
    }
    pub(super) fn unowned_bytes(&self) -> usize {
        if self.storage.is_none() {
            self.backing_bytes()
        } else {
            0
        }
    }
    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .or_else(|| {
                self.data
                    .iter()
                    .find_map(|waiter| waiter.pending.storage.as_ref())
            })
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
            && self.data.iter().all(|waiter| {
                waiter
                    .pending
                    .storage
                    .as_ref()
                    .is_none_or(|owner| owner.belongs_to(scope))
            })
    }
    pub(super) fn matches_owner(&self, owner: &SharedSchedulingCharge) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|storage| storage.same_owner(owner))
            && self.data.iter().all(|waiter| {
                waiter
                    .pending
                    .storage
                    .as_ref()
                    .is_none_or(|storage| storage.same_owner(owner))
            })
    }
    pub(super) fn remove(&mut self, index: usize) -> DeferredImportWaiter {
        self.data.remove(index)
    }
    #[cfg(any(test, coverage))]
    pub(super) fn clear(&mut self) {
        self.data.clear();
    }
    #[cfg(any(test, coverage))]
    pub(super) fn truncate(&mut self, length: usize) {
        self.data.truncate(length);
    }
    #[cfg(any(test, coverage))]
    pub(super) fn reserve(&mut self, count: usize) {
        self.data.reserve(count);
        let bytes = self.backing_bytes();
        if let Some(storage) = &mut self.storage {
            storage.resize(bytes).expect("fixed fixture backing fits");
        }
    }
    fn reclaim(&mut self, scope: Option<&SchedulingScope>) -> Option<SharedSchedulingCharge> {
        if self.data.is_empty() {
            self.data = Vec::new();
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            return None;
        }
        let minimum = self
            .data
            .len()
            .saturating_mul(std::mem::size_of::<DeferredImportWaiter>());
        let old = self.backing_bytes();
        if minimum.saturating_mul(4) >= old {
            return None;
        }
        let mut charge = match &self.storage {
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
        let mut replacement = Vec::new();
        replacement.try_reserve_exact(self.data.len()).ok()?;
        let actual = host_scheduling::slots(&replacement);
        if actual >= old {
            return None;
        }
        if let Some(charge) = &mut charge {
            if !charge.try_resize_for_reclamation(actual) {
                return None;
            }
        }
        replacement.append(&mut self.data);
        self.data = replacement;
        if self.storage.is_some() {
            self.storage = charge.take();
        }
        charge
    }
}

impl Vm {
    pub(super) fn deferred_import_owner(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let current = self.argument_list_owner()?;
        let original = self.deferred_import_waiters.owner_view()?;
        let eager = self.module_import_waiters.owner_view()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.deferred_import_waiters.can_join(scope)
                || !self.module_import_waiters.can_join(scope)
                || hint.is_some_and(|owner| !owner.belongs_to(scope))
            {
                return Err(scope.fail());
            }
        }
        let Some(owner) = original
            .as_ref()
            .or(eager.as_ref())
            .or(hint)
            .or(current.as_ref())
        else {
            return Ok(None);
        };
        if !self.deferred_import_waiters.matches_owner(owner)
            || eager
                .as_ref()
                .is_some_and(|storage| !storage.same_owner(owner))
            || hint.is_some_and(|storage| !storage.same_owner(owner))
        {
            return Err(owner.fail());
        }
        if self.deferred_import_waiters.storage.is_none()
            || self
                .deferred_import_waiters
                .iter()
                .any(|waiter| waiter.pending.storage.is_none())
        {
            self.synchronize_scheduling_usage()?;
            let backing = self.deferred_import_waiters.unowned_bytes();
            let bytes = self
                .deferred_import_waiters
                .iter()
                .fold(backing, |bytes, waiter| {
                    bytes.saturating_add(waiter.pending.unowned_bytes())
                });
            let count = self
                .deferred_import_waiters
                .iter()
                .map(|waiter| waiter.pending.unowned_count())
                .sum();
            let mut storage = match &mut self.host_scheduling {
                Some(scope) => {
                    let mut storage = scope.detach_owned_payload(bytes);
                    storage
                        .absorb(scope.detach_owned_reactions(count))
                        .expect("same document");
                    storage
                }
                None => {
                    let mut storage = owner.reserve_peer(bytes)?;
                    storage.absorb(owner.reserve_reactions(count)?)?;
                    storage
                }
            };
            if self.deferred_import_waiters.storage.is_none() {
                self.deferred_import_waiters.storage = Some(storage.split_off(backing));
            }
            for waiter in &mut self.deferred_import_waiters {
                if waiter.pending.storage.is_none() {
                    let mut input = storage.split_off(waiter.pending.unowned_bytes());
                    input
                        .absorb(storage.split_reactions(1))
                        .expect("same document");
                    waiter.pending.storage = Some(input);
                }
            }
        }
        owner.reserve_peer(0).map(Some)
    }

    pub(super) fn prepare_import_dependencies(
        &self,
        modules: Vec<String>,
    ) -> Result<PreparedImportDependencies, RuntimeError> {
        let owner = self.argument_list_owner()?;
        self.prepare_import_dependencies_with_owner(modules, owner.as_ref())
    }
    pub(super) fn prepare_import_dependencies_with_owner(
        &self,
        modules: Vec<String>,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedImportDependencies, RuntimeError> {
        let input_bytes = modules
            .iter()
            .fold(host_scheduling::slots(&modules), |bytes, name| {
                bytes.saturating_add(name.capacity())
            });
        let requested = modules.len().saturating_mul(std::mem::size_of::<String>());
        let scope = self.host_scheduling.as_ref();
        let fail = || {
            owner.map_or_else(
                || scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail),
                SharedSchedulingCharge::fail,
            )
        };
        let charge = owner
            .map(|owner| owner.reserve_peer(input_bytes.saturating_add(requested)))
            .transpose()?;
        let mut prepared = PreparedImportDependencies {
            input: modules,
            pending: ImportDependencies::default(),
            charge,
        };
        prepared
            .pending
            .data
            .try_reserve(prepared.input.len())
            .map_err(|_| fail())?;
        prepared.pending.allocation_bytes = prepared
            .pending
            .data
            .capacity()
            .saturating_mul(std::mem::size_of::<String>());
        if let Some(charge) = &mut prepared.charge {
            charge.resize(input_bytes.saturating_add(prepared.pending.allocation_bytes))?;
        }
        // Names move once; duplicate buffers drop while the lease covers every
        // original name and both coexisting metadata allocations.
        prepared.pending.data.extend(prepared.input.drain(..));
        prepared.input = Vec::new();
        let actual = prepared.pending.retained_bytes();
        if let Some(charge) = &mut prepared.charge {
            charge.resize(actual)?;
        }
        Ok(prepared)
    }

    pub(super) fn reclaim_deferred_import_vector(&mut self) {
        let charge = self
            .deferred_import_waiters
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/import_waiter_storage.rs"]
mod ownership_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/deferred_import_lifetime.rs"]
mod lifetime_tests;
