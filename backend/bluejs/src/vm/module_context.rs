// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module names move with one scoped owner across frame and debugger changes.

use super::*;

pub(super) struct ModuleNameLease {
    name: Option<String>,
    charge: execution_payload::ExecutionPayloadCharge,
}

impl ModuleNameLease {
    pub(super) fn into_name(mut self) -> Option<String> {
        self.name.take()
    }

    pub(super) fn swap_into(&mut self, vm: &mut Vm) {
        std::mem::swap(&mut self.name, &mut vm.active_module_name);
        self.charge.resize(if self.charge.enabled() {
            self.name.as_ref().map_or(0, String::capacity)
        } else {
            0
        });
    }

    pub(super) fn restore(mut self, vm: &mut Vm) {
        vm.active_module_name = self.name.take();
        drop(self);
        vm.reconcile_scheduling_ownership();
    }
}

impl Vm {
    pub(super) fn prepare_module_name(
        &mut self,
        name: &str,
    ) -> Result<ModuleNameLease, RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: name.len(),
            ..Default::default()
        })?;
        let name = module_referrers::copy_referrer(name, self.host_scheduling.as_ref())?;
        self.lease_module_name_copy(Some(name))
    }

    pub(super) fn prepare_active_module_name_copy(
        &mut self,
    ) -> Result<ModuleNameLease, RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: self.active_module_name.as_ref().map_or(0, String::len),
            ..Default::default()
        })?;
        let name = self
            .active_module_name
            .as_deref()
            .map(|name| module_referrers::copy_referrer(name, self.host_scheduling.as_ref()))
            .transpose()?;
        self.lease_module_name_copy(name)
    }

    fn lease_module_name_copy(
        &mut self,
        name: Option<String>,
    ) -> Result<ModuleNameLease, RuntimeError> {
        let prepared = ModuleNameLease {
            charge: self.charge_execution_control(name.as_ref().map_or(0, String::capacity)),
            name,
        };
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(prepared);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        Ok(prepared)
    }

    pub(super) fn take_module_name(&mut self) -> ModuleNameLease {
        let name = self.active_module_name.take();
        ModuleNameLease {
            charge: self.charge_execution_control(name.as_ref().map_or(0, String::capacity)),
            name,
        }
    }
}
