// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module record tables and canonical names retain admitted native storage.

use super::host_scheduling::{map_slots, SharedSchedulingCharge};
use super::*;

#[derive(Default)]
pub(super) struct ModuleRecords {
    data: HashMap<String, LinkedModule>,
    key_bytes: usize,
    charge: RefCell<Option<SharedSchedulingCharge>>,
}
impl std::ops::Deref for ModuleRecords {
    type Target = HashMap<String, LinkedModule>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::DerefMut for ModuleRecords {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Existing record values may change through this view. Key insertion
        // and removal must use the admitted owner methods below.
        &mut self.data
    }
}
fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
impl ModuleRecords {
    pub(super) fn retained_bytes(&self) -> usize {
        map_slots(&self.data).saturating_add(self.key_bytes)
    }
    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        let mut charge = self.charge.borrow_mut();
        if let Some(charge) = charge.as_ref() {
            if !charge.belongs_to(scope) {
                scope.fail();
                return Err(RuntimeError::TypeError(
                    "module records already have a document owner".into(),
                ));
            }
        } else if self.retained_bytes() != 0 {
            *charge = Some(scope.shared_charge(self.retained_bytes())?);
        }
        Ok(())
    }
    /// A detached replacement coexists with the complete old table. Existing
    /// records and name buffers move only after all capacity is admitted.
    pub(super) fn reserve(
        &mut self,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = scope {
            self.attach(scope)?;
        } else if let Some(charge) = self.charge.get_mut() {
            return Err(charge.fail());
        }
        let required = self
            .len()
            .checked_add(additional)
            .ok_or_else(|| fail(scope))?;
        if required <= self.capacity() {
            return Ok(());
        }
        let minimum = required.saturating_mul(std::mem::size_of::<(String, LinkedModule)>());
        let mut staged_charge = scope
            .map(|scope| scope.shared_charge(minimum))
            .transpose()?;
        let mut replacement = HashMap::new();
        replacement.try_reserve(required).map_err(|_| fail(scope))?;
        if let Some(charge) = &mut staged_charge {
            charge.resize(map_slots(&replacement))?;
        }
        let owner = self.charge.get_mut();
        if let Some(staged) = staged_charge {
            if let Some(owner) = owner {
                owner.absorb(staged)?;
            } else {
                *owner = Some(staged);
            }
        }
        replacement.extend(self.data.drain());
        let old = std::mem::replace(&mut self.data, replacement);
        drop(old);
        let bytes = self.retained_bytes();
        if let Some(charge) = self.charge.get_mut() {
            charge.release_to(bytes);
        }
        Ok(())
    }
    pub(super) fn insert_new(
        &mut self,
        name: &str,
        record: LinkedModule,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        if self.contains_key(name) {
            return Err(RuntimeError::TypeError(
                "module record was already linked".into(),
            ));
        }
        self.reserve(1, scope)?;
        let mut incoming = scope
            .map(|scope| scope.shared_charge(name.len()))
            .transpose()?;
        let key = module_referrers::copy_referrer(name, scope)?;
        if let Some(charge) = &mut incoming {
            charge.resize(key.capacity())?;
        }
        let owner = self.charge.get_mut();
        if let Some(incoming) = incoming {
            if let Some(owner) = owner {
                owner.absorb(incoming)?;
            } else {
                *owner = Some(incoming);
            }
        }
        self.key_bytes = self.key_bytes.saturating_add(key.capacity());
        self.data.insert(key, record);
        Ok(())
    }
    pub(super) fn remove(&mut self, name: &str) -> Option<LinkedModule> {
        let (key, record) = self.data.remove_entry(name)?;
        self.key_bytes -= key.capacity();
        drop(key);
        let bytes = self.retained_bytes();
        if let Some(charge) = self.charge.get_mut() {
            charge.release_to(bytes);
        }
        Some(record)
    }
}

pub(super) struct ModuleNames<'a> {
    names: Vec<&'a str>,
    _charge: execution_payload::ExecutionPayloadCharge,
}
impl<'a> std::ops::Deref for ModuleNames<'a> {
    type Target = [&'a str];
    fn deref(&self) -> &Self::Target {
        &self.names
    }
}
impl Vm {
    #[cfg(any(test, coverage))]
    pub(super) fn module_record_storage_bytes(&self) -> usize {
        self.module_graph
            .as_ref()
            .map_or(0, |graph| graph.linked.retained_bytes())
            .saturating_add(
                self.evaluating_linked
                    .as_ref()
                    .map_or(0, ModuleRecords::retained_bytes),
            )
    }
    pub(super) fn prepare_module_names<'a>(
        &mut self,
        modules: &'a HashMap<String, Bytecode>,
        linked: &ModuleRecords,
    ) -> Result<ModuleNames<'a>, RuntimeError> {
        let count = modules
            .keys()
            .filter(|name| !linked.contains_key(name.as_str()))
            .count();
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: count.saturating_mul(std::mem::size_of::<&str>()),
            ..Default::default()
        })?;
        let mut names = Vec::new();
        names
            .try_reserve_exact(count)
            .map_err(|_| fail(self.host_scheduling.as_ref()))?;
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: host_scheduling::slots(&names),
            ..Default::default()
        })?;
        names.extend(
            modules
                .keys()
                .filter(|name| !linked.contains_key(name.as_str()))
                .map(String::as_str),
        );
        names.sort_unstable();
        let result = ModuleNames {
            _charge: self.charge_execution_control(host_scheduling::slots(&names)),
            names,
        };
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(result);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        Ok(result)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_graph_ownership.rs"]
mod ownership_tests;
