// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host family backing, identity tables and callback boxes retain their owner.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

#[derive(Default)]
pub(in crate::vm) struct HostFamilies {
    values: Vec<HostObjectFamilyState>,
    // Nested records and all vector backing die before this independent lease.
    storage: Option<SharedSchedulingCharge>,
}

impl std::ops::Deref for HostFamilies {
    type Target = [HostObjectFamilyState];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl std::ops::DerefMut for HostFamilies {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

pub(super) struct PreparedFamilyCapacity {
    replacement: Option<Vec<HostObjectFamilyState>>,
    replacement_storage: Option<SharedSchedulingCharge>,
    adoption: Option<SharedSchedulingCharge>,
}

#[cfg(any(test, coverage))]
impl PreparedFamilyCapacity {
    pub(super) fn staging_bytes(&self) -> usize {
        self.replacement.as_ref().map_or(0, host_scheduling::slots)
    }
}

impl HostFamilies {
    #[cfg(any(test, coverage))]
    pub(super) fn capacity(&self) -> usize {
        self.values.capacity()
    }

    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(super) fn prepare(
        &self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedFamilyCapacity, RuntimeError> {
        let original = self.owner_view()?;
        let owner = original.as_ref().or(hint);
        let adoption = if self.storage.is_none() {
            owner
                .map(|owner| owner.reserve_peer(host_scheduling::slots(&self.values)))
                .transpose()?
        } else {
            None
        };
        let mut replacement = None;
        let mut replacement_storage = None;
        if self.values.len() == self.values.capacity() {
            let count = self.values.capacity().saturating_mul(2).max(4);
            let bytes = count
                .checked_mul(std::mem::size_of::<HostObjectFamilyState>())
                .ok_or_else(|| {
                    owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
                })?;
            let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
            let mut values = Vec::new();
            values.try_reserve_exact(count).map_err(|_| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            if let Some(storage) = &mut storage {
                storage.resize(host_scheduling::slots(&values))?;
            }
            replacement = Some(values);
            replacement_storage = storage;
        }
        Ok(PreparedFamilyCapacity {
            replacement,
            replacement_storage,
            adoption,
        })
    }

    pub(super) fn commit(
        &mut self,
        mut prepared: PreparedFamilyCapacity,
        family: HostObjectFamilyState,
    ) {
        if let Some(mut replacement) = prepared.replacement.take() {
            replacement.append(&mut self.values);
            self.values = replacement;
            self.storage = prepared.replacement_storage.take();
        } else if self.storage.is_none() {
            self.storage = prepared.adoption.take();
        }
        assert!(self.values.len() < self.values.capacity());
        self.values.push(family);
    }

    fn can_join_owner(&self, owner: &SharedSchedulingCharge) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|current| current.same_owner(owner))
            && self.values.iter().all(|family| {
                family.wrappers.can_join_owner(owner)
                    && family.keys_by_wrapper.can_join_owner(owner)
                    && family
                        .callback_storage
                        .as_ref()
                        .is_none_or(|current| current.same_owner(owner))
            })
    }

    fn attach_owner(&mut self, owner: &SharedSchedulingCharge) -> Result<(), RuntimeError> {
        if !self.can_join_owner(owner) {
            return Err(owner.fail());
        }
        if self.storage.is_none() && self.values.capacity() != 0 {
            self.storage = Some(owner.reserve_peer(host_scheduling::slots(&self.values))?);
        }
        for family in &mut self.values {
            family.wrappers.attach_owner(owner)?;
            family.keys_by_wrapper.attach_owner(owner)?;
            let bytes = family.callback_bytes();
            if family.callback_storage.is_none() && bytes != 0 {
                family.callback_storage = Some(owner.reserve_peer(bytes)?);
            }
        }
        Ok(())
    }
}

impl HostObjectFamilyState {
    fn callback_bytes(&self) -> usize {
        self.element_validator
            .as_ref()
            .map_or(0, |callback| std::mem::size_of_val(&**callback))
            .saturating_add(
                self.event_path
                    .as_ref()
                    .map_or(0, |callback| std::mem::size_of_val(&**callback)),
            )
    }

    /// Only a Box's concrete allocation belongs here. External state retained
    /// by embedding callbacks keeps its embedding-owner contract.
    pub(super) fn replace_callback_charge(
        &mut self,
        old_bytes: usize,
        charge: Option<SharedSchedulingCharge>,
    ) {
        if let Some(storage) = &mut self.callback_storage {
            storage.release_to(storage.bytes() - old_bytes);
            storage
                .absorb(charge.expect("callback keeps its original owner"))
                .expect("prepared callback owner");
        } else {
            self.callback_storage = charge;
        }
    }
}

pub(super) struct FamilyContext {
    caller: Option<SharedSchedulingCharge>,
    original: Option<SharedSchedulingCharge>,
}

impl FamilyContext {
    pub(super) fn capture(vm: &mut Vm) -> Result<Self, RuntimeError> {
        Self::capture_with_owner(vm, None)
    }

    pub(super) fn capture_with_owner(
        vm: &mut Vm,
        fallback: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let caller = vm.argument_list_owner()?;
        let original = vm.host_object_families.owner_view()?;
        if let Some(owner) = original.as_ref().or(caller.as_ref()).or(fallback) {
            vm.host_object_families.attach_owner(owner)?;
        }
        Ok(Self {
            caller,
            original: vm.host_object_families.owner_view()?,
        })
    }

    pub(super) fn owner(&self) -> Option<&SharedSchedulingCharge> {
        self.original.as_ref().or(self.caller.as_ref())
    }

    pub(super) fn validate(&self, vm: &Vm) -> Result<(), RuntimeError> {
        for owner in [self.caller.as_ref(), self.original.as_ref()]
            .into_iter()
            .flatten()
        {
            owner.reserve_peer(0)?;
        }
        vm.argument_list_owner()?;
        Ok(())
    }
}

impl Vm {
    pub(in crate::vm) fn host_family_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.host_object_families
            .storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
            && self.host_object_families.iter().all(|family| {
                family.wrappers.can_join(scope)
                    && family.keys_by_wrapper.can_join(scope)
                    && family
                        .callback_storage
                        .as_ref()
                        .is_none_or(|owner| owner.belongs_to(scope))
            })
    }

    pub(in crate::vm) fn attach_host_family_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        if !self.host_family_storage_can_join(scope) {
            return Err(scope.fail());
        }
        self.host_object_families
            .attach_owner(&scope.shared_charge(0)?)
    }

    pub(in crate::vm) fn unowned_host_family_storage_bytes(&self) -> usize {
        let slots = if self.host_object_families.storage.is_none() {
            host_scheduling::slots(&self.host_object_families.values)
        } else {
            0
        };
        self.host_object_families
            .iter()
            .fold(slots, |bytes, family| {
                bytes
                    .saturating_add(family.wrappers.unowned_bytes())
                    .saturating_add(family.keys_by_wrapper.unowned_bytes())
                    .saturating_add(if family.callback_storage.is_none() {
                        family.callback_bytes()
                    } else {
                        0
                    })
            })
    }
}
