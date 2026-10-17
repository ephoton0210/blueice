// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Weak closure associations own admitted native tables and referrer names.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/referrer_storage.rs"]
mod storage_tests;

pub(super) fn copy_referrer(
    name: &str,
    scope: Option<&SchedulingScope>,
) -> Result<String, RuntimeError> {
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
    _charge: Option<SharedSchedulingCharge>,
    _reservation: Option<ReferrerReservation>,
}

impl PreparedClosureModuleReferrer {
    pub(super) fn publish(mut self, vm: &mut Vm, closure: ObjectId) {
        if let Some(name) = self.name.take() {
            // Other outstanding preparations may rely on the old spare slots.
            // An older candidate can be smaller than backing already installed
            // by a later preparation. It may reuse its reserved slot, while a
            // newer growth preparation will supply any further needed backing.
            let required = vm.module_closure_referrers.len() + vm.pending_module_referrers.get();
            if required > vm.module_closure_referrers.capacity() {
                if let Some(table) = self.table.take() {
                    if table.capacity() > vm.module_closure_referrers.capacity() {
                        vm.module_closure_referrers.replace_prepared(table);
                    }
                }
            }
            debug_assert!(
                vm.module_closure_referrers.len() < vm.module_closure_referrers.capacity()
            );
            vm.module_closure_referrers.insert(closure, name);
        }
        // Fields have adopted the data. Refund staging immediately before the
        // synchronous scope update; checking both owners would double-charge
        // an exact allowance. No allocation or callback may intervene.
        drop(self);
        vm.reconcile_scheduling_ownership();
        vm.reclaim_module_referrer_storage();
    }
}

pub(super) struct PreparedCallModuleReferrer {
    name: Option<String>,
    _charge: Option<SharedSchedulingCharge>,
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
            self.module_closure_referrers.retained_bytes(),
            |bytes, name| bytes.saturating_add(name.capacity()),
        )
    }

    pub(super) fn release_empty_module_referrer_storage(&mut self) {
        if self.module_closure_referrers.is_empty() && self.pending_module_referrers.get() == 0 {
            self.module_closure_referrers.release_empty();
        }
    }

    pub(super) fn reclaim_module_referrer_storage(&mut self) {
        if self.pending_module_referrers.get() != 0 || self.preserve_promise_table_capacity {
            return;
        }
        let charge = self
            .module_closure_referrers
            .reclaim(self.host_scheduling.as_ref());
        self.reconcile_scheduling_ownership();
        drop(charge);
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
                _charge: None,
                _reservation: None,
            });
        };
        self.synchronize_scheduling_usage()?;
        let required = self
            .module_closure_referrers
            .len()
            .checked_add(self.pending_module_referrers.get())
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| {
                self.host_scheduling
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
            })?;
        let replace = required > self.module_closure_referrers.capacity();
        let minimum = name_bytes.saturating_add(if replace {
            required.saturating_mul(std::mem::size_of::<(ObjectId, String)>())
        } else {
            0
        });
        // This independent owner is acquired before either native allocation.
        let mut charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(minimum))
            .transpose()?;
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
        let table_bytes = table.as_ref().map_or(0, host_scheduling::map_slots);
        if let Some(charge) = &mut charge {
            // Hash capacity may exceed the requested minimum. Admit it before
            // copying the filename, while the old allocation remains owned.
            charge.resize(table_bytes.saturating_add(name_bytes))?;
        }
        let name = Some(copy_referrer(
            self.active_module_name
                .as_deref()
                .expect("the entry has a referrer"),
            self.host_scheduling.as_ref(),
        )?);
        let actual = table_bytes.saturating_add(name.as_ref().map_or(0, String::capacity));
        if let Some(charge) = &mut charge {
            charge.resize(actual)?;
        }
        let mut prepared = PreparedClosureModuleReferrer {
            name,
            table,
            _charge: charge,
            _reservation: None,
        };
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
                _charge: None,
            });
        };
        self.synchronize_scheduling_usage()?;
        let mut charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(bytes))
            .transpose()?;
        let source = callee
            .object_id()
            .and_then(|id| self.module_closure_referrers.get(&id))
            .expect("the call has an associated closure");
        let name = Some(copy_referrer(source, self.host_scheduling.as_ref())?);
        let actual = name.as_ref().map_or(0, String::capacity);
        if let Some(charge) = &mut charge {
            charge.resize(actual)?;
        }
        let prepared = PreparedCallModuleReferrer {
            name,
            _charge: charge,
        };
        Ok(prepared)
    }
}
