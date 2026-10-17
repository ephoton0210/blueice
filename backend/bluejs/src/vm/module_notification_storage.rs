// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Notification work owns admitted queue backing and independently owned names.

use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedDeque;
use super::*;

#[derive(Default)]
pub(super) struct NotificationQueue {
    names: VecDeque<ModuleName>,
    charge: Option<SharedSchedulingCharge>,
}
impl NotificationQueue {
    fn attach(&mut self, scope: Option<&SchedulingScope>) -> Result<(), RuntimeError> {
        match (&self.charge, scope) {
            (Some(owner), Some(scope)) if !owner.belongs_to(scope) => return Err(scope.fail()),
            (Some(owner), None) => return Err(owner.fail()),
            (None, Some(scope)) => {
                for name in &mut self.names {
                    name.attach(Some(scope))?;
                }
                self.charge = Some(scope.shared_charge(self.backing_bytes())?);
            }
            _ => {}
        }
        if let Some(scope) = scope {
            scope.shared_charge(0)?;
        }
        Ok(())
    }
    pub(super) fn singleton_copy(
        name: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        Self::singleton_owned(ModuleName::copy(name, scope)?, scope)
    }
    pub(super) fn singleton_owned(
        mut name: ModuleName,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        name.attach(scope)?;
        let mut queue = Self::default();
        queue.reserve(1, scope)?;
        queue.push_prepared(name, scope)?;
        Ok(queue)
    }
    pub(super) fn reserve(
        &mut self,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        self.attach(scope)?;
        let growth = PreparedDeque::for_additional(&self.names, additional, scope)?;
        let replaces = growth.staging_bytes() != 0;
        let owner = growth.commit_owned(&mut self.names);
        if replaces {
            self.charge = owner;
        }
        Ok(())
    }
    pub(super) fn push_prepared(
        &mut self,
        mut name: ModuleName,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        self.attach(scope)?;
        name.attach(scope)?;
        assert!(
            self.names.len() < self.names.capacity(),
            "whole parent batch precedes publication"
        );
        self.names.push_back(name);
        Ok(())
    }
    pub(super) fn pop_front(&mut self) -> Option<ModuleName> {
        self.names.pop_front()
    }
    fn backing_bytes(&self) -> usize {
        self.names
            .capacity()
            .saturating_mul(std::mem::size_of::<ModuleName>())
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.backing_bytes()
            .saturating_add(self.names.iter().map(ModuleName::bytes).sum::<usize>())
    }
}

impl Vm {
    /// Remove the reverse row without copying buffers or transiently charging
    /// them twice. No allocation/callback separates removal and ownership move.
    pub(super) fn take_notification_parents(
        &mut self,
        child: &str,
    ) -> Result<module_name_storage::ModuleNameIterator, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let names = self.module_async_parents.remove(child).unwrap_or_default();
        let bytes = module_dependencies::string_vector_bytes(&names);
        let charge = self
            .host_scheduling
            .as_mut()
            .map(|scope| scope.detach_owned_payload(bytes));
        let names = ModuleNames::from_admitted(names, charge).into_iter();
        self.reconcile_scheduling_ownership();
        Ok(names)
    }
}
