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
impl Vm {
    pub(super) fn prepare_import_dependencies(
        &self,
        modules: Vec<String>,
    ) -> Result<PreparedImportDependencies, RuntimeError> {
        let input_bytes = modules
            .iter()
            .fold(host_scheduling::slots(&modules), |bytes, name| {
                bytes.saturating_add(name.capacity())
            });
        let requested = modules.len().saturating_mul(std::mem::size_of::<String>());
        let scope = self.host_scheduling.as_ref();
        let fail = || scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail);
        let charge = scope
            .map(|scope| scope.shared_charge(input_bytes.saturating_add(requested)))
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
        if self.deferred_import_waiters.is_empty() {
            self.deferred_import_waiters = Vec::new();
            return;
        }
        let minimum = self
            .deferred_import_waiters
            .len()
            .saturating_mul(std::mem::size_of::<DeferredImportWaiter>());
        let old = host_scheduling::slots(&self.deferred_import_waiters);
        if minimum.saturating_mul(4) >= old {
            return;
        }
        let mut charge = match &self.host_scheduling {
            Some(scope) => match scope.try_reclamation_charge(minimum) {
                Some(charge) => Some(charge),
                None => return,
            },
            None => None,
        };
        let mut replacement = Vec::new();
        if replacement
            .try_reserve_exact(self.deferred_import_waiters.len())
            .is_err()
        {
            return;
        }
        let actual = host_scheduling::slots(&replacement);
        if actual >= old {
            return;
        }
        if let Some(charge) = &mut charge {
            if !charge.try_resize_for_reclamation(actual) {
                return;
            }
        }
        replacement.append(&mut self.deferred_import_waiters);
        self.deferred_import_waiters = replacement;
        self.reconcile_scheduling_ownership();
        drop(charge);
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/import_waiter_storage.rs"]
mod ownership_tests;
