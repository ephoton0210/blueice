// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host registration slots and concrete callback allocations retain credit.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

pub(in crate::vm) trait RegistrationRecord {
    fn callback_bytes(&self) -> usize;
}

impl RegistrationRecord for Box<dyn HostFunction> {
    fn callback_bytes(&self) -> usize {
        std::mem::size_of_val(&**self)
    }
}

impl RegistrationRecord for HostObjectFactoryRegistration {
    fn callback_bytes(&self) -> usize {
        std::mem::size_of_val(&*self.factory)
    }
}
impl RegistrationRecord for HostObjectMethodRegistration {
    fn callback_bytes(&self) -> usize {
        std::mem::size_of_val(&*self.method)
    }
}
impl RegistrationRecord for HostObjectPairMethodRegistration {
    fn callback_bytes(&self) -> usize {
        std::mem::size_of_val(&*self.method)
    }
}

pub(in crate::vm) struct HostRegistrations<T> {
    records: Vec<T>,
    // Every callback and the Vec allocation die before their pooled credit.
    storage: Option<SharedSchedulingCharge>,
}

impl<T> Default for HostRegistrations<T> {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            storage: None,
        }
    }
}
impl<T> std::ops::Deref for HostRegistrations<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.records
    }
}
impl<T> std::ops::DerefMut for HostRegistrations<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.records
    }
}

pub(in crate::vm) struct PreparedRegistrations<T> {
    replacement: Option<Vec<T>>,
    replacement_storage: Option<SharedSchedulingCharge>,
    adoption: Option<SharedSchedulingCharge>,
    owner: Option<SharedSchedulingCharge>,
    previous: (usize, usize),
    additional: usize,
}

pub(in crate::vm) struct PreparedCallbacks<T, const N: usize> {
    records: [T; N],
    // Partial installation and unwind destroy callbacks before their credit.
    storage: Option<SharedSchedulingCharge>,
}

impl<T: RegistrationRecord> PreparedRegistrations<T> {
    #[cfg(any(test, coverage))]
    pub(in crate::vm) fn staging_bytes(&self) -> usize {
        self.replacement.as_ref().map_or(0, host_scheduling::slots)
    }

    pub(in crate::vm) fn callbacks<const N: usize>(
        &self,
        sizes: &[usize; N],
        make: impl FnOnce() -> [T; N],
    ) -> Result<PreparedCallbacks<T, N>, RuntimeError> {
        let bytes = sizes.iter().try_fold(0usize, |total, size| {
            total.checked_add(*size).ok_or_else(|| {
                self.owner
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })
        })?;
        let storage = self
            .owner
            .as_ref()
            .map(|owner| owner.reserve_peer(bytes))
            .transpose()?;
        let records = make();
        debug_assert_eq!(
            records
                .iter()
                .map(RegistrationRecord::callback_bytes)
                .sum::<usize>(),
            bytes
        );
        Ok(PreparedCallbacks { records, storage })
    }
}

impl<T: RegistrationRecord> HostRegistrations<T> {
    #[cfg(any(test, coverage))]
    pub(in crate::vm) fn capacity(&self) -> usize {
        self.records.capacity()
    }

    fn bytes(&self) -> usize {
        self.records
            .iter()
            .fold(host_scheduling::slots(&self.records), |bytes, record| {
                bytes.saturating_add(record.callback_bytes())
            })
    }

    /// Bytes an account already owns (the rest is in `own_scheduling_usage`).
    #[cfg(any(test, coverage))]
    pub(in crate::vm) fn owned_bytes(&self) -> usize {
        self.bytes() - self.unowned_bytes()
    }

    pub(in crate::vm) fn unowned_bytes(&self) -> usize {
        if self.storage.is_some() {
            0
        } else {
            self.bytes()
        }
    }

    pub(in crate::vm) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(in crate::vm) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }

    pub(in crate::vm) fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if !self.can_join(scope) {
            return Err(scope.fail());
        }
        if self.storage.is_none() && self.records.capacity() != 0 {
            self.storage = Some(scope.shared_charge(self.bytes())?);
        }
        Ok(())
    }

    /// A previously neutral table becomes owned before its callback can run.
    pub(in crate::vm) fn retain_for_entry(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        if self.storage.is_none() && self.records.capacity() != 0 {
            self.storage = hint
                .map(|owner| owner.reserve_peer(self.bytes()))
                .transpose()?;
        }
        self.owner_view()
    }

    pub(in crate::vm) fn prepare(
        &self,
        additional: usize,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedRegistrations<T>, RuntimeError> {
        let original = self.owner_view()?;
        let owner = original.as_ref().or(hint);
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        let needed = self.records.len().checked_add(additional).ok_or_else(|| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        let adoption = if self.storage.is_none() {
            owner
                .map(|owner| owner.reserve_peer(self.bytes()))
                .transpose()?
        } else {
            None
        };
        let mut replacement = None;
        let mut replacement_storage = None;
        if needed > self.records.capacity() {
            let count = self.records.capacity().saturating_mul(2).max(needed).max(4);
            let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
            let mut records = Vec::new();
            records.try_reserve_exact(count).map_err(|_| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            if let Some(storage) = &mut storage {
                storage.resize(host_scheduling::slots(&records))?;
            }
            replacement = Some(records);
            replacement_storage = storage;
        }
        let owner = owner.map(|owner| owner.reserve_peer(0)).transpose()?;
        Ok(PreparedRegistrations {
            replacement,
            replacement_storage,
            adoption,
            owner,
            previous: (self.records.len(), self.records.capacity()),
            additional,
        })
    }

    pub(in crate::vm) fn commit<const N: usize>(
        &mut self,
        mut prepared: PreparedRegistrations<T>,
        mut callbacks: PreparedCallbacks<T, N>,
    ) {
        assert_eq!(
            (self.records.len(), self.records.capacity()),
            prepared.previous
        );
        assert_eq!(N, prepared.additional);
        if self.storage.is_none() {
            self.storage = prepared.adoption.take();
        }
        if let Some(mut replacement) = prepared.replacement.take() {
            let old_slots = host_scheduling::slots(&self.records);
            replacement.append(&mut self.records);
            self.records = replacement;
            if let Some(storage) = &mut self.storage {
                storage.release_to(storage.bytes() - old_slots);
                storage
                    .absorb(prepared.replacement_storage.take().unwrap())
                    .expect("prepared registration replacement owner");
            }
        }
        if let Some(storage) = &mut callbacks.storage {
            self.storage
                .as_mut()
                .expect("prepared registration owner")
                .absorb_from(storage)
                .expect("prepared callback and slots have one owner");
        }
        assert!(N <= self.records.capacity() - self.records.len());
        self.records.extend(callbacks.records);
    }
}

impl Vm {
    pub(in crate::vm) fn host_registration_storage_can_join(
        &self,
        scope: &SchedulingScope,
    ) -> bool {
        self.host_functions.can_join(scope)
            && self.host_object_factories.can_join(scope)
            && self.host_object_methods.can_join(scope)
            && self.host_object_pair_methods.can_join(scope)
            && self.file_input_storage_can_join(scope)
            && self.host_async_registration_storage_can_join(scope)
    }

    pub(in crate::vm) fn attach_host_registration_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        if !self.host_registration_storage_can_join(scope) {
            return Err(scope.fail());
        }
        self.host_functions.attach(scope)?;
        self.host_object_factories.attach(scope)?;
        self.host_object_methods.attach(scope)?;
        self.host_object_pair_methods.attach(scope)?;
        self.attach_file_input_storage(scope)?;
        self.attach_host_async_registration_storage(scope)
    }

    pub(in crate::vm) fn unowned_host_registration_storage_bytes(&self) -> usize {
        self.host_functions
            .unowned_bytes()
            .saturating_add(self.host_object_factories.unowned_bytes())
            .saturating_add(self.host_object_methods.unowned_bytes())
            .saturating_add(self.host_object_pair_methods.unowned_bytes())
            .saturating_add(self.unowned_file_input_storage_bytes())
            .saturating_add(self.unowned_host_async_registration_storage_bytes())
    }
}
