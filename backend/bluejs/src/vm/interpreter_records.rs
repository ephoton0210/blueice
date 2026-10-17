// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native interpreter records carry their allocation charge with their owner.

use super::*;
use std::ops::Deref;

pub(super) trait InterpreterRecord {
    fn payload_bytes(&self) -> usize;
}

impl InterpreterRecord for Value {
    fn payload_bytes(&self) -> usize {
        host_scheduling::value_payload_bytes(self)
    }
}

impl InterpreterRecord for HandlerFrame {
    fn payload_bytes(&self) -> usize {
        self.pending_origin.payload_bytes()
    }
}

pub(super) struct InterpreterRecords<T: InterpreterRecord> {
    values: Vec<T>,
    charge: Option<execution_payload::ExecutionPayloadCharge>,
}

pub(super) type IteratorRecords = InterpreterRecords<Value>;
pub(super) type HandlerRecords = InterpreterRecords<HandlerFrame>;
pub(super) type ResourceRecords = InterpreterRecords<DisposableResource>;
pub(super) type DisposeMarks = InterpreterRecords<usize>;

impl InterpreterRecord for usize {
    fn payload_bytes(&self) -> usize {
        0
    }
}

impl InterpreterRecord for ObjectId {
    fn payload_bytes(&self) -> usize {
        0
    }
}

impl InterpreterRecord for HostValue {
    fn payload_bytes(&self) -> usize {
        match self {
            HostValue::String(value) => value.owned_heap_payload_bytes().unwrap_or(usize::MAX),
            _ => 0,
        }
    }
}

impl InterpreterRecord for DisposableResource {
    fn payload_bytes(&self) -> usize {
        self.values().fold(0usize, |bytes, value| {
            bytes.saturating_add(host_scheduling::value_payload_bytes(value))
        })
    }
}

impl<T: InterpreterRecord> Default for InterpreterRecords<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            charge: None,
        }
    }
}

impl<T: InterpreterRecord> Deref for InterpreterRecords<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<'a, T: InterpreterRecord> IntoIterator for &'a InterpreterRecords<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<'a> IntoIterator for &'a mut HandlerRecords {
    type Item = &'a mut HandlerFrame;
    type IntoIter = std::slice::IterMut<'a, HandlerFrame>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter_mut()
    }
}

impl HandlerRecords {
    pub(super) fn refresh_payload(&mut self, vm: &mut Vm) {
        let retained = self
            .charge
            .as_ref()
            .is_some_and(|charge| charge.is_retained());
        self.refresh();
        if retained {
            vm.reconcile_scheduling_ownership();
        }
    }

    pub(super) fn last_mut(&mut self) -> Option<&mut HandlerFrame> {
        self.values.last_mut()
    }
    pub(super) fn as_mut_slice(&mut self) -> &mut [HandlerFrame] {
        &mut self.values
    }
}

impl<T: InterpreterRecord> InterpreterRecords<T> {
    fn storage_bytes(values: &Vec<T>) -> usize {
        values
            .iter()
            .fold(host_scheduling::slots(values), |bytes, value| {
                bytes.saturating_add(value.payload_bytes())
            })
    }

    pub(super) fn bind(&mut self, vm: &Vm) -> Result<(), RuntimeError> {
        if let Some(charge) = &self.charge {
            if charge.is_retained() {
                return charge.validate_retained(vm.host_scheduling.as_ref());
            }
        }
        if self
            .charge
            .as_ref()
            .is_none_or(|charge| charge.enabled() != vm.host_scheduling.is_some())
        {
            self.charge = Some(vm.charge_execution_control(Self::storage_bytes(&self.values)));
        }
        Ok(())
    }

    pub(super) fn retain_native_storage(&mut self, vm: &mut Vm) -> Result<(), RuntimeError> {
        let bytes = Self::storage_bytes(&self.values);
        if self.charge.is_none() {
            self.charge = Some(vm.charge_execution_control(bytes));
        }
        self.charge.as_mut().unwrap().retain_native(vm, bytes)
    }

    fn refresh(&mut self) {
        let bytes = Self::storage_bytes(&self.values);
        if let Some(charge) = &mut self.charge {
            charge.resize(if charge.enabled() { bytes } else { 0 });
        }
    }

    pub(super) fn unleased_bytes(&self) -> usize {
        Self::storage_bytes(&self.values)
            .saturating_sub(self.charge.as_ref().map_or(0, |charge| charge.bytes()))
    }

    #[cfg(any(test, coverage))]
    pub(super) fn retained_native_bytes(&self) -> usize {
        self.charge
            .as_ref()
            .filter(|charge| charge.is_retained())
            .map_or(0, |charge| charge.bytes())
    }

    /// Admission precedes reserve and publication, including a recheck of the
    /// allocator-reported capacity. Existing slots add no growth forecast.
    pub(super) fn try_push(&mut self, vm: &mut Vm, value: T) -> Result<(), RuntimeError> {
        self.bind(vm)?;
        let growth = SchedulingCapacity::for_vec_push(&self.values);
        let extra = HostSchedulingUsage {
            payload_bytes: value.payload_bytes(),
            ..Default::default()
        };
        vm.admit_scheduling_work(extra.with_growth(growth))?;
        growth.reserve_vec(&mut self.values);
        self.refresh();
        vm.admit_scheduling_work(extra)?;
        self.values.push(value);
        self.refresh();
        vm.synchronize_scheduling_usage()
    }

    pub(super) fn pop(&mut self) -> Option<T> {
        let value = self.values.pop();
        self.refresh();
        value
    }

    pub(super) fn pop_if(&mut self, predicate: impl FnOnce(&mut T) -> bool) -> Option<T> {
        let value = self.values.pop_if(predicate);
        self.refresh();
        value
    }

    pub(super) fn retain(&mut self, predicate: impl FnMut(&T) -> bool) {
        self.values.retain(predicate);
        self.refresh();
    }

    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.refresh();
    }

    pub(super) fn try_split_off(&mut self, vm: &mut Vm, at: usize) -> Result<Self, RuntimeError> {
        self.bind(vm)?;
        if at == 0 {
            return Ok(std::mem::take(self));
        }
        let bytes = (self.len() - at).saturating_mul(std::mem::size_of::<T>());
        vm.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: bytes,
            ..Default::default()
        })?;
        let values = self.values.split_off(at);
        self.refresh();
        let records = vm.owned_records(values);
        vm.synchronize_scheduling_usage()?;
        Ok(records)
    }

    /// The managed generator state takes this allocation without intervening
    /// JavaScript. Its heap accounting owns the next published representation.
    pub(super) fn into_vec(self) -> Vec<T> {
        let Self { values, charge } = self;
        drop(charge);
        values
    }
}

#[cfg(any(test, coverage))]
impl<T: InterpreterRecord> From<Vec<T>> for InterpreterRecords<T> {
    fn from(values: Vec<T>) -> Self {
        Self {
            values,
            charge: None,
        }
    }
}

impl Vm {
    pub(super) fn owned_records<T: InterpreterRecord>(
        &self,
        values: Vec<T>,
    ) -> InterpreterRecords<T> {
        let charge = self.charge_execution_control(InterpreterRecords::storage_bytes(&values));
        InterpreterRecords {
            values,
            charge: Some(charge),
        }
    }
    pub(super) fn iterator_records(&self, values: Vec<Value>) -> IteratorRecords {
        let charge = self.charge_execution_control(IteratorRecords::storage_bytes(&values));
        InterpreterRecords {
            values,
            charge: Some(charge),
        }
    }

    pub(super) fn handler_records(&self, values: Vec<HandlerFrame>) -> HandlerRecords {
        let charge = self.charge_execution_control(HandlerRecords::storage_bytes(&values));
        InterpreterRecords {
            values,
            charge: Some(charge),
        }
    }
}
