// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Weak closure associations own admitted native tables and referrer names.

use super::*;

fn copy_referrer(name: &str, scope: Option<&SchedulingScope>) -> Result<String, RuntimeError> {
    let mut copy = String::new();
    copy.try_reserve_exact(name.len())
        .map_err(|_| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?;
    copy.push_str(name);
    Ok(copy)
}

struct ReferrerReservation(Rc<std::cell::Cell<usize>>);

impl Drop for ReferrerReservation {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

pub(super) struct PreparedClosureModuleReferrer {
    name: Option<String>,
    table: Option<HashMap<ObjectId, String>>,
    _charge: execution_payload::ExecutionPayloadCharge,
    _reservation: Option<ReferrerReservation>,
}

impl PreparedClosureModuleReferrer {
    pub(super) fn publish(mut self, vm: &mut Vm, closure: ObjectId) {
        if let Some(name) = self.name.take() {
            if let Some(mut table) = self.table.take() {
                table.extend(vm.module_closure_referrers.drain());
                vm.module_closure_referrers = table;
            }
            debug_assert!(
                vm.module_closure_referrers.len() < vm.module_closure_referrers.capacity()
            );
            vm.module_closure_referrers.insert(closure, name);
        }
        drop(self);
        vm.reconcile_scheduling_ownership();
    }
}

pub(super) struct PreparedCallModuleReferrer {
    name: Option<String>,
    _charge: execution_payload::ExecutionPayloadCharge,
}

impl PreparedCallModuleReferrer {
    pub(super) fn install(&mut self, vm: &mut Vm) -> Option<Option<String>> {
        self.name
            .take()
            .map(|name| vm.active_module_name.replace(name))
    }
}

impl Vm {
    pub(super) fn module_referrer_storage_bytes(&self) -> usize {
        self.module_closure_referrers.values().fold(
            host_scheduling::map_slots(&self.module_closure_referrers),
            |bytes, name| bytes.saturating_add(name.capacity()),
        )
    }

    pub(super) fn release_empty_module_referrer_storage(&mut self) {
        if self.module_closure_referrers.is_empty() && self.pending_module_referrers.get() == 0 {
            self.module_closure_referrers = HashMap::new();
        }
    }

    pub(super) fn prune_module_closure_referrers(&mut self) {
        self.module_closure_referrers
            .retain(|closure, _| self.heap.contains(*closure));
        // A prepared publication may still rely on the table's spare slots
        // while prototype/capture allocation collects its old weak keys.
        self.release_empty_module_referrer_storage();
    }

    pub(super) fn prepare_closure_module_referrer(
        &mut self,
    ) -> Result<PreparedClosureModuleReferrer, RuntimeError> {
        let result = self.prepare_closure_referrer_storage();
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    fn prepare_closure_referrer_storage(
        &mut self,
    ) -> Result<PreparedClosureModuleReferrer, RuntimeError> {
        let Some(name_bytes) = self.active_module_name.as_ref().map(String::len) else {
            self.check_scheduling_failure()?;
            return Ok(PreparedClosureModuleReferrer {
                name: None,
                table: None,
                _charge: self.charge_execution_control(0),
                _reservation: None,
            });
        };
        let required = self
            .module_closure_referrers
            .len()
            .saturating_add(self.pending_module_referrers.get())
            .saturating_add(1);
        let replace = required > self.module_closure_referrers.capacity();
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: name_bytes.saturating_add(if replace {
                required.saturating_mul(std::mem::size_of::<(ObjectId, String)>())
            } else {
                0
            }),
            ..Default::default()
        })?;
        let table = if replace {
            let mut table = HashMap::new();
            if table.try_reserve(required).is_err() {
                return Err(self
                    .host_scheduling
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
            }
            Some(table)
        } else {
            None
        };
        let name = Some(copy_referrer(
            self.active_module_name
                .as_deref()
                .expect("the entry has a referrer"),
            self.host_scheduling.as_ref(),
        )?);
        let actual = table
            .as_ref()
            .map_or(0, host_scheduling::map_slots)
            .saturating_add(name.as_ref().map_or(0, String::capacity));
        let mut prepared = PreparedClosureModuleReferrer {
            name,
            table,
            _charge: self.charge_execution_control(actual),
            _reservation: None,
        };
        self.synchronize_scheduling_usage()?;
        self.pending_module_referrers
            .set(self.pending_module_referrers.get() + 1);
        prepared._reservation = Some(ReferrerReservation(self.pending_module_referrers.clone()));
        Ok(prepared)
    }

    pub(super) fn prepare_call_module_referrer(
        &mut self,
        callee: &Value,
    ) -> Result<PreparedCallModuleReferrer, RuntimeError> {
        let Some(bytes) = callee
            .object_id()
            .and_then(|id| self.module_closure_referrers.get(&id))
            .map(String::len)
        else {
            self.check_scheduling_failure()?;
            return Ok(PreparedCallModuleReferrer {
                name: None,
                _charge: self.charge_execution_control(0),
            });
        };
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: bytes,
            ..Default::default()
        })?;
        let source = callee
            .object_id()
            .and_then(|id| self.module_closure_referrers.get(&id))
            .expect("the call has an associated closure");
        let name = Some(copy_referrer(source, self.host_scheduling.as_ref())?);
        let actual = name.as_ref().map_or(0, String::capacity);
        let prepared = PreparedCallModuleReferrer {
            name,
            _charge: self.charge_execution_control(actual),
        };
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(prepared);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        Ok(prepared)
    }
}
