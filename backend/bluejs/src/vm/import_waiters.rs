// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import waiters own their targets and dependency storage until settlement.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct ModuleImportWaiterGroup {
    data: Vec<ObjectId>,
    name_bytes: usize,
    // The name and vector are destroyed before their storage/count credit.
    storage: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for ModuleImportWaiterGroup {
    type Target = Vec<ObjectId>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<'a> IntoIterator for &'a ModuleImportWaiterGroup {
    type Item = &'a ObjectId;
    type IntoIter = std::slice::Iter<'a, ObjectId>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}
impl ModuleImportWaiterGroup {
    pub(super) fn unowned_count(&self) -> usize {
        if self.storage.is_none() {
            self.data.len()
        } else {
            0
        }
    }
    pub(super) fn unowned_bytes(&self, name_bytes: usize) -> usize {
        if self.storage.is_none() {
            name_bytes.saturating_add(host_scheduling::slots(&self.data))
        } else {
            0
        }
    }
    pub(super) fn remove(&mut self, index: usize) {
        self.data.remove(index);
        if let Some(storage) = &mut self.storage {
            storage.release_reactions_to(self.data.len());
        }
    }
    fn replace_backing(&mut self, backing: Option<SharedSchedulingCharge>) {
        if let Some(backing) = backing {
            let storage = self.storage.as_mut().expect("prepared waiter owner");
            storage.release_to(self.name_bytes);
            storage
                .absorb(backing)
                .expect("waiter backing keeps its account");
        }
    }
}

struct ModuleImportInput {
    name: String,
    waiters: ModuleImportWaiterGroup,
}

impl Vm {
    pub(super) fn module_import_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.module_import_waiters.can_join(scope)
            && self.module_import_waiters.values().all(|waiters| {
                waiters
                    .storage
                    .as_ref()
                    .is_none_or(|owner| owner.belongs_to(scope))
            })
    }

    pub(super) fn module_import_owner(
        &mut self,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let current = self.argument_list_owner()?;
        let original = self.module_import_waiters.owner_view()?;
        let record_owner = self
            .module_import_waiters
            .values()
            .find_map(|waiters| waiters.storage.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose()?;
        let deferred_owner = self.deferred_import_waiters.owner_view()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.module_import_storage_can_join(scope)
                || !self.deferred_import_waiters.can_join(scope)
            {
                return Err(scope.fail());
            }
        }
        let Some(owner) = original
            .as_ref()
            .or(record_owner.as_ref())
            .or(deferred_owner.as_ref())
            .or(current.as_ref())
        else {
            return Ok(None);
        };
        if !self.deferred_import_waiters.matches_owner(owner)
            || self.module_import_waiters.values().any(|waiters| {
                waiters
                    .storage
                    .as_ref()
                    .is_some_and(|storage| !storage.same_owner(owner))
            })
        {
            return Err(owner.fail());
        }
        let backing = self.module_import_waiters.unowned_bytes();
        let mut bytes = backing;
        let mut count = 0usize;
        for (name, waiters) in self.module_import_waiters.iter() {
            bytes = bytes.saturating_add(waiters.unowned_bytes(name.capacity()));
            count = count.saturating_add(waiters.unowned_count());
        }
        if !self.module_import_waiters.has_owner()
            || self
                .module_import_waiters
                .values()
                .any(|waiters| waiters.storage.is_none())
        {
            self.synchronize_scheduling_usage()?;
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
            if !self.module_import_waiters.has_owner() {
                self.module_import_waiters
                    .install_storage(storage.split_off(backing));
            }
            for (name, waiters) in self.module_import_waiters.iter_mut() {
                if waiters.storage.is_none() {
                    let mut input = storage.split_off(waiters.unowned_bytes(name.capacity()));
                    input
                        .absorb(storage.split_reactions(waiters.data.len()))
                        .expect("same document");
                    waiters.name_bytes = name.capacity();
                    waiters.storage = Some(input);
                }
            }
        }
        owner.reserve_peer(0).map(Some)
    }

    pub(super) fn enqueue_module_import_waiter(
        &mut self,
        module: String,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        let owner = self.module_import_owner()?;
        let extra = HostSchedulingUsage {
            pending_reactions: 1,
            ..HostSchedulingUsage::default()
        };
        self.admit_scheduling_work(extra)?;
        if let Some(waiters) = self.module_import_waiters.get_mut(&module) {
            // Discard the temporary lookup key before preparing new storage.
            drop(module);
            let count = owner
                .as_ref()
                .map(|owner| owner.reserve_reactions(1))
                .transpose()?;
            let growth =
                promise_queue_storage::PreparedVec::for_push_owned(&waiters.data, owner.as_ref())?;
            let backing = growth.commit_owned(&mut waiters.data);
            waiters.replace_backing(backing);
            if let Some(count) = count {
                waiters
                    .storage
                    .as_mut()
                    .expect("waiter owner")
                    .absorb(count)
                    .expect("same account");
            }
            waiters.data.push(promise);
        } else {
            let name_bytes = module.capacity();
            let mut input = ModuleImportInput {
                name: module,
                waiters: ModuleImportWaiterGroup {
                    data: Vec::new(),
                    name_bytes,
                    storage: owner
                        .as_ref()
                        .map(|owner| owner.reserve_reactions(1))
                        .transpose()?,
                },
            };
            if let Some(storage) = &mut input.waiters.storage {
                storage.resize(name_bytes)?;
            }
            let table = promise_queue_storage::PreparedMap::for_additional_owned(
                &self.module_import_waiters,
                1,
                owner.as_ref(),
            )?;
            let growth = promise_queue_storage::PreparedVec::for_push_owned(
                &input.waiters.data,
                owner.as_ref(),
            )?;
            let backing = growth.commit_owned(&mut input.waiters.data);
            input.waiters.replace_backing(backing);
            table.commit_owned(&mut self.module_import_waiters);
            input.waiters.data.push(promise);
            self.module_import_waiters.insert(input.name, input.waiters);
        }
        self.reconcile_scheduling_ownership();
        Ok(())
    }

    pub(super) fn enqueue_deferred_import_waiter(
        &mut self,
        promise: ObjectId,
        namespace: ObjectId,
        modules: Vec<String>,
    ) -> Result<(), RuntimeError> {
        let eager = self.module_import_owner()?;
        let owner = self.deferred_import_owner(eager.as_ref())?;
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_reactions: 1,
            ..HostSchedulingUsage::default()
        })?;
        let count = owner
            .as_ref()
            .map(|owner| owner.reserve_reactions(1))
            .transpose()?;
        let prepared = self.prepare_import_dependencies_with_owner(modules, owner.as_ref())?;
        let growth = promise_queue_storage::PreparedVec::for_push_owned(
            &self.deferred_import_waiters.data,
            owner.as_ref(),
        )?;
        let (pending, mut charge) = prepared.into_owned();
        if let Some(count) = count {
            charge
                .as_mut()
                .expect("prepared dependency owner")
                .absorb(count)
                .expect("same account");
        }
        let waiter = DeferredImportWaiter {
            promise,
            namespace,
            pending: import_waiter_storage::DeferredImportDependencies {
                data: pending,
                storage: charge,
            },
        };
        if let Some(backing) = growth.commit_owned(&mut self.deferred_import_waiters.data) {
            self.deferred_import_waiters.storage = Some(backing);
        }
        self.deferred_import_waiters.data.push(waiter);
        self.reconcile_scheduling_ownership();
        Ok(())
    }

    pub(super) fn reclaim_import_waiter_storage(&mut self) {
        self.reconcile_scheduling_ownership();
        let charge = self
            .module_import_waiters
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
        for index in 0..self.deferred_import_waiters.len() {
            let charge = self.deferred_import_waiters[index]
                .pending
                .reclaim(self.host_scheduling.as_ref());
            self.reconcile_scheduling_ownership();
            drop(charge);
        }
        self.reclaim_deferred_import_vector();
        self.reconcile_scheduling_ownership();
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_import_lifetime.rs"]
mod lifetime_tests;
