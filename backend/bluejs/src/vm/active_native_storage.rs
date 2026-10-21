// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native algorithm activity records follow their synchronous callback extent.

use super::host_scheduling::SharedSchedulingCharge;
use super::transport_buffer_storage::TransportVector;
use super::*;

#[derive(Default)]
pub(super) struct ActiveDelegations {
    values: HashSet<ObjectId>,
    allocation_bytes: usize,
    // Backing must be destroyed before its independent native lease.
    owner: Option<SharedSchedulingCharge>,
}

impl std::ops::Deref for ActiveDelegations {
    type Target = HashSet<ObjectId>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl ActiveDelegations {
    fn bytes(&self) -> usize {
        self.allocation_bytes
    }

    fn unowned_bytes(&self) -> usize {
        if self.owner.is_none() {
            self.bytes()
        } else {
            0
        }
    }

    fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.owner
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }

    fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if !self.can_join(scope) {
            return Err(scope.fail());
        }
        if self.owner.is_none() && self.bytes() != 0 {
            self.owner = Some(scope.shared_charge(self.bytes())?);
        }
        Ok(())
    }

    fn insert(
        &mut self,
        object: ObjectId,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        assert!(!self.values.contains(&object));
        let retained = if let Some(owner) = &self.owner {
            owner.reserve_peer(0)?;
            None
        } else {
            hint.map(|owner| owner.reserve_peer(self.bytes()))
                .transpose()?
        };
        let owner = self.owner.as_ref().or(retained.as_ref());
        if self.values.len() == self.values.capacity() {
            let count = self.values.capacity().saturating_mul(2).max(4);
            let bytes = count
                .checked_mul(std::mem::size_of::<ObjectId>())
                .ok_or_else(|| {
                    owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
                })?;
            let mut charge = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
            let mut replacement = HashSet::with_hasher(self.values.hasher().clone());
            replacement.try_reserve(count).map_err(|_| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            let actual = replacement.capacity() * std::mem::size_of::<ObjectId>();
            if let Some(charge) = &mut charge {
                charge.resize(actual)?;
            }
            replacement.extend(self.values.iter().copied());
            self.values = replacement;
            self.allocation_bytes = actual;
            self.owner = charge;
        } else if self.owner.is_none() {
            self.owner = retained;
        }
        assert!(self.values.insert(object));
        Ok(())
    }

    fn remove(&mut self, object: &ObjectId) {
        assert!(self.values.remove(object));
        if self.values.is_empty() {
            self.values = HashSet::with_hasher(self.values.hasher().clone());
            self.allocation_bytes = 0;
            self.owner = None;
        }
    }
}

impl Vm {
    pub(super) fn begin_array_join(&mut self, object: ObjectId) -> Result<(), RuntimeError> {
        let owner = self.argument_list_owner()?;
        if self.joining.is_empty() {
            let mut prepared = TransportVector::with_capacity(4, owner.as_ref())?;
            prepared.push(object)?;
            self.joining = prepared;
            return Ok(());
        }
        if let Some(owner) = &owner {
            self.joining.attach_owner(owner)?;
        }
        self.joining.validate_owner()?;
        self.joining.push(object)
    }

    pub(super) fn end_array_join(&mut self, object: ObjectId) {
        assert_eq!(self.joining.last(), Some(&object));
        self.joining.truncate(self.joining.len() - 1);
        if self.joining.is_empty() {
            self.joining = TransportVector::default();
        }
    }

    pub(super) fn begin_generator_delegation(
        &mut self,
        generator: ObjectId,
    ) -> Result<(), RuntimeError> {
        let owner = self.argument_list_owner()?;
        self.active_generator_delegations
            .insert(generator, owner.as_ref())
    }

    pub(super) fn end_generator_delegation(&mut self, generator: ObjectId) {
        self.active_generator_delegations.remove(&generator);
    }

    pub(super) fn unowned_active_native_storage_bytes(&self) -> usize {
        self.joining
            .unowned_slots()
            .saturating_add(self.active_generator_delegations.unowned_bytes())
    }

    pub(super) fn active_native_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.joining.can_join(scope) && self.active_generator_delegations.can_join(scope)
    }

    pub(super) fn attach_active_native_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        if !self.active_native_storage_can_join(scope) {
            return Err(scope.fail());
        }
        self.joining.attach(scope)?;
        self.active_generator_delegations.attach(scope)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/active_native_storage.rs"]
mod ownership_tests;
