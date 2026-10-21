// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cached module values retain native owners independently of active frames.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use std::cell::Cell;

pub(super) struct ModuleValueLease {
    bytes: Cell<usize>,
    header: usize,
    charge: RefCell<Option<SharedSchedulingCharge>>,
}
impl ModuleValueLease {
    fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        let mut charge = self.charge.borrow_mut();
        if let Some(owner) = charge.as_ref() {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        } else {
            *charge = Some(scope.shared_charge(self.bytes.get())?);
        }
        Ok(())
    }
    fn release_payload(&self) {
        self.bytes.set(self.header);
        if let Some(charge) = self.charge.borrow_mut().as_mut() {
            charge.release_to(self.header);
        }
    }
}

pub(super) struct ModuleValueData<O: Default> {
    pub(super) value: Value,
    pub(super) origin: O,
    lease: Rc<ModuleValueLease>,
}
pub(super) type ModuleCompletion = Rc<ModuleValueData<()>>;
pub(super) type ModuleEvaluationError = Rc<ModuleValueData<ThrowOrigin>>;

impl<O: Default> ModuleValueData<O> {
    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.lease
            .charge
            .borrow()
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    fn header_bytes() -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of::<ModuleValueLease>()
            + 4 * std::mem::size_of::<usize>()
    }
    fn copy(
        value: &Value,
        origin: O,
        origin_bytes: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<Rc<Self>, RuntimeError> {
        let header = Self::header_bytes();
        let mut charge = scope
            .map(|scope| scope.shared_charge(header.saturating_add(origin_bytes)))
            .transpose()?;
        let copy = module_values::ModuleValueCopy::copy(value, scope)?;
        let (value, payload_charge) = copy.into_retained();
        let bytes = header
            .saturating_add(origin_bytes)
            .saturating_add(host_scheduling::value_payload_bytes(&value));
        if let (Some(owner), Some(payload)) = (&mut charge, payload_charge) {
            owner.absorb(payload)?;
        }
        Ok(Rc::new(Self {
            value,
            origin,
            lease: Rc::new(ModuleValueLease {
                bytes: Cell::new(bytes),
                header,
                charge: RefCell::new(charge),
            }),
        }))
    }
    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        self.lease.attach(scope)
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.lease.bytes.get()
    }
}
impl<O: Default> Drop for ModuleValueData<O> {
    fn drop(&mut self) {
        // Weak discovery keeps only the allocation headers. Free primitive
        // and origin buffers before releasing their part of the native lease.
        self.value = Value::Undefined;
        self.origin = O::default();
        self.lease.release_payload();
    }
}

pub(super) struct ModuleErrorTracker {
    owner: std::rc::Weak<ModuleValueData<ThrowOrigin>>,
    // The Weak allocation and its lease remain owned until discovery prunes
    // them, without retaining a JavaScript value or its primitive buffers.
    lease: Rc<ModuleValueLease>,
}
impl ModuleErrorTracker {
    pub(super) fn is_live(&self) -> bool {
        self.owner.strong_count() != 0
    }
}

impl Vm {
    pub(super) fn retain_module_completion(
        &mut self,
        value: &Value,
    ) -> Result<ModuleCompletion, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        ModuleValueData::copy(value, (), 0, self.host_scheduling.as_ref())
    }
    pub(super) fn retain_module_error(
        &mut self,
        value: &Value,
        origin: ThrowOrigin,
    ) -> Result<ModuleEvaluationError, RuntimeError> {
        let result = self.retain_module_error_inner(value, origin);
        self.reconcile_scheduling_ownership();
        result
    }
    fn retain_module_error_inner(
        &mut self,
        value: &Value,
        origin: ThrowOrigin,
    ) -> Result<ModuleEvaluationError, RuntimeError> {
        self.prune_module_error_origins();
        let old_capacity = self.module_error_origins.capacity();
        let capacity = (self.module_error_origins.len() == old_capacity)
            .then(|| old_capacity.saturating_mul(2).max(4));
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: capacity
                .unwrap_or(0)
                .saturating_mul(std::mem::size_of::<ModuleErrorTracker>()),
            ..Default::default()
        })?;
        let mut replacement = capacity
            .map(|capacity| {
                let mut values = Vec::new();
                values.try_reserve_exact(capacity).map_err(|_| {
                    self.host_scheduling
                        .as_ref()
                        .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
                })?;
                Ok::<Vec<ModuleErrorTracker>, RuntimeError>(values)
            })
            .transpose()?;
        let staging =
            self.charge_execution_control(replacement.as_ref().map_or(0, host_scheduling::slots));
        self.synchronize_scheduling_usage()?;
        let origin_bytes = origin.payload_bytes();
        let owner =
            ModuleValueData::copy(value, origin, origin_bytes, self.host_scheduling.as_ref())?;
        if let Some(mut values) = replacement.take() {
            values.append(&mut self.module_error_origins);
            self.module_error_origins = values;
        }
        assert!(self.module_error_origins.len() < self.module_error_origins.capacity());
        self.module_error_origins.push(ModuleErrorTracker {
            owner: Rc::downgrade(&owner),
            lease: owner.lease.clone(),
        });
        drop(staging);
        Ok(owner)
    }
    pub(super) fn prune_module_error_origins(&mut self) {
        self.module_error_origins
            .retain(ModuleErrorTracker::is_live);
        if self.module_error_origins.is_empty() {
            self.module_error_origins = Vec::new();
        }
    }
    pub(super) fn attach_cached_module_values(
        &self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        for graph in self.module_graph.iter() {
            graph.linked.attach_values(scope)?;
        }
        if let Some(records) = &self.evaluating_linked {
            records.attach_values(scope)?;
        }
        for tracker in &self.module_error_origins {
            tracker.lease.attach(scope)?;
        }
        Ok(())
    }
    // Includes Rust-held errors through their weak trackers. Completion owners
    // are not shared between different records, so no temporary set is needed.
    #[cfg(any(test, coverage))]
    pub(super) fn module_value_storage_bytes(&self) -> usize {
        self.cached_completion_bytes().saturating_add(
            self.module_error_origins
                .iter()
                .map(|tracker| tracker.lease.bytes.get())
                .sum::<usize>(),
        )
    }
    fn cached_completion_bytes(&self) -> usize {
        self.module_graph
            .iter()
            .map(|graph| &graph.linked)
            .chain(self.evaluating_linked.iter())
            .flat_map(|records| records.values())
            .filter_map(|record| record.completion.as_ref())
            .map(|value| value.retained_bytes())
            .sum()
    }
    pub(super) fn host_neutral_module_value_bytes(&self) -> usize {
        if self.host_scheduling.is_some() {
            return 0;
        }
        self.cached_completion_bytes().saturating_add(
            self.module_error_origins
                .iter()
                .map(|tracker| tracker.lease.bytes.get())
                .sum::<usize>(),
        )
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_value_ownership.rs"]
mod ownership_tests;
