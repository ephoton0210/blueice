// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Timer records and their table retain native credit through ownership moves.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[derive(Default)]
pub(super) struct HostTimerTable {
    pub(super) data: Vec<HostTimer>,
    pub(super) storage: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for HostTimerTable {
    type Target = Vec<HostTimer>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
#[cfg(any(test, coverage))]
impl std::ops::IndexMut<usize> for HostTimerTable {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}
impl std::ops::Index<usize> for HostTimerTable {
    type Output = HostTimer;
    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}
impl<'a> IntoIterator for &'a HostTimerTable {
    type Item = &'a HostTimer;
    type IntoIter = std::slice::Iter<'a, HostTimer>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}
impl HostTimerTable {
    pub(super) fn owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
            && self.data.iter().all(|timer| {
                timer
                    .storage
                    .as_ref()
                    .is_none_or(|owner| owner.belongs_to(scope))
            })
    }
    pub(super) fn unowned_count(&self) -> usize {
        self.data
            .iter()
            .filter(|timer| timer.storage.is_none())
            .count()
    }
    pub(super) fn unowned_bytes(&self) -> usize {
        self.data
            .iter()
            .filter(|timer| timer.storage.is_none())
            .fold(
                if self.storage.is_none() {
                    host_scheduling::slots(&self.data)
                } else {
                    0
                },
                |bytes, timer| bytes.saturating_add(timer.scheduling_payload_bytes()),
            )
    }
    pub(super) fn remove(&mut self, index: usize) -> HostTimer {
        let mut timer = self.data.remove(index);
        if let Some(storage) = &mut timer.storage {
            storage.release_timers();
        }
        timer
    }
    pub(super) fn iter_mut(&mut self) -> std::slice::IterMut<'_, HostTimer> {
        self.data.iter_mut()
    }
    pub(super) fn set_serial(&mut self, index: usize, serial: u64) {
        self.data[index].serial = serial;
    }
    pub(super) fn shrink_to_fit(&mut self) {
        if self.data.is_empty() {
            self.data = Vec::new();
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn reserve_exact(&mut self, count: usize) {
        self.data.reserve_exact(count);
        if let Some(storage) = &mut self.storage {
            storage.reconcile_owned_bytes(host_scheduling::slots(&self.data));
        }
    }
}

pub(super) struct PreparedTimerRegistration {
    pub(super) arguments: Vec<Value>,
    pub(super) roots: Vec<RootId>,
    pub(super) replacement: Option<Vec<HostTimer>>,
    pub(super) replacement_storage: Option<SharedSchedulingCharge>,
    // Both input buffers drop before the record/count lease.
    pub(super) charge: Option<SharedSchedulingCharge>,
}
impl PreparedTimerRegistration {
    fn admit_reported(&mut self, pending: usize) -> Result<(), RuntimeError> {
        let bytes = host_scheduling::slots(&self.arguments)
            .saturating_add(host_scheduling::slots(&self.roots))
            .saturating_add(pending);
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }
        Ok(())
    }
}

impl Vm {
    pub(super) fn timer_table_owner(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let current = self.argument_list_owner()?;
        let original = self.host_timers.owner()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.host_timers.can_join(scope)
                || hint.is_some_and(|owner| !owner.belongs_to(scope))
            {
                return Err(scope.fail());
            }
        }
        let owner = original.as_ref().or(current.as_ref()).or(hint);
        if let Some(owner) = owner {
            if self.host_timers.storage.is_none() || self.host_timers.unowned_count() != 0 {
                self.synchronize_scheduling_usage()?;
                let bytes = self.host_timers.unowned_bytes();
                let count = self.host_timers.unowned_count();
                let mut storage = match &mut self.host_scheduling {
                    Some(scope) => {
                        let mut storage = scope.detach_owned_payload(bytes);
                        storage
                            .absorb(scope.detach_owned_timers(count))
                            .expect("same document");
                        storage
                    }
                    None => {
                        let mut storage = owner.reserve_peer(bytes)?;
                        storage.absorb(owner.reserve_timers(count)?)?;
                        storage
                    }
                };
                if self.host_timers.storage.is_none() {
                    self.host_timers.storage =
                        Some(storage.split_off(host_scheduling::slots(&self.host_timers.data)));
                }
                for timer in &mut self.host_timers.data {
                    if timer.storage.is_none() {
                        let mut entry = storage.split_off(timer.scheduling_payload_bytes());
                        entry
                            .absorb(storage.split_timers(1))
                            .expect("same document");
                        timer.storage = Some(entry);
                    }
                }
            }
        }
        current
            .as_ref()
            .or(hint)
            .or(original.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn timer_entry_owner(
        &mut self,
        index: usize,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.timer_table_owner(None)?;
        self.host_timers[index]
            .storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn prepare_timer_registration(
        &mut self,
        arguments: &[Value],
        root_count: usize,
    ) -> Result<PreparedTimerRegistration, RuntimeError> {
        let owner = self.timer_table_owner(None)?;
        let backing = self.host_timers.owner()?;
        let capacity = (self.host_timers.len() == self.host_timers.capacity()).then(|| {
            self.host_timers
                .len()
                .saturating_add(1)
                .max(self.host_timers.capacity().saturating_mul(2))
                .max(4)
        });
        let payload = host_scheduling::values_bytes(arguments);
        let forecast = arguments
            .len()
            .saturating_mul(std::mem::size_of::<Value>())
            .saturating_add(root_count.saturating_mul(std::mem::size_of::<RootId>()))
            .saturating_add(payload);
        let charge = owner
            .as_ref()
            .map(|owner| {
                let mut storage = owner.reserve_peer(forecast)?;
                storage.absorb(owner.reserve_timers(1)?)?;
                Ok::<_, RuntimeError>(storage)
            })
            .transpose()?;
        let replacement_storage = capacity
            .and_then(|count| {
                backing.as_ref().map(|owner| {
                    owner.reserve_peer(count.saturating_mul(std::mem::size_of::<HostTimer>()))
                })
            })
            .transpose()?;
        let mut prepared = PreparedTimerRegistration {
            arguments: Vec::new(),
            roots: Vec::new(),
            replacement: capacity.map(|_| Vec::new()),
            replacement_storage,
            charge,
        };
        let fail = || {
            owner
                .as_ref()
                .or(backing.as_ref())
                .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        };
        prepared
            .arguments
            .try_reserve_exact(arguments.len())
            .map_err(|_| fail())?;
        prepared.admit_reported(
            payload.saturating_add(root_count.saturating_mul(std::mem::size_of::<RootId>())),
        )?;
        prepared
            .roots
            .try_reserve_exact(root_count)
            .map_err(|_| fail())?;
        prepared.admit_reported(payload)?;
        if let (Some(replacement), Some(capacity)) = (&mut prepared.replacement, capacity) {
            replacement
                .try_reserve_exact(capacity)
                .map_err(|_| fail())?;
            if let Some(storage) = &mut prepared.replacement_storage {
                storage.resize(host_scheduling::slots(replacement))?;
            }
        }
        prepared.arguments.extend_from_slice(arguments);
        prepared.admit_reported(host_scheduling::values_bytes(&prepared.arguments))?;
        Ok(prepared)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/timer_registration.rs"]
mod ownership_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/timer_lifetime.rs"]
mod lifetime_tests;
