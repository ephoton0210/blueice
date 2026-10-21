// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Argument-list construction owns its buffers through callbacks and adoption.

use super::host_scheduling::SharedSchedulingCharge;
use super::realm_call_storage::RealmCallRoots;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/argument_list_storage.rs"]
mod ownership_tests;

pub(super) struct ArgumentValues {
    pub(super) values: Vec<Value>,
    // The input allocation is destroyed before its native ownership.
    entry: execution_payload::CallEntryCharge,
}

impl ArgumentValues {
    pub(super) fn single_retained(
        vm: &mut Vm,
        input: native_value_storage::NativeValue,
    ) -> Result<Self, RuntimeError> {
        let owner = input.owner_view()?;
        let mut result = Self::new(vm, owner.as_ref())?;
        result.reserve_capacity(1)?;
        result
            .entry
            .push_retained_argument(&mut result.values, input)?;
        Ok(result)
    }

    pub(super) fn adopt_retained(
        values: Vec<Value>,
        entry: execution_payload::CallEntryCharge,
    ) -> Self {
        Self { values, entry }
    }

    pub(super) fn copied_pair(
        vm: &mut Vm,
        left: &Value,
        right: &Value,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::new(vm, owner)?;
        result.reserve_capacity(2)?;
        for value in [left, right] {
            result.entry.append_argument_payload(value)?;
            result.values.push(value.clone());
        }
        Ok(result)
    }

    pub(super) fn single(
        vm: &mut Vm,
        value: Value,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::new(vm, owner)?;
        result.reserve_capacity(1)?;
        result.push(value)?;
        Ok(result)
    }

    fn new(vm: &mut Vm, owner: Option<&SharedSchedulingCharge>) -> Result<Self, RuntimeError> {
        let values = Vec::new();
        let mut entry = vm.call_entry_charge(&Value::Undefined, &Value::Undefined, &values);
        entry.retain_native(vm, &Value::Undefined, &Value::Undefined, &values, owner)?;
        Ok(Self { values, entry })
    }

    fn prepare_push(&mut self) -> Result<(), RuntimeError> {
        if self.values.len() < self.values.capacity() {
            return Ok(());
        }
        let count = self.values.capacity().saturating_mul(2).max(4);
        self.reserve_capacity(count)
    }

    fn reserve_capacity(&mut self, count: usize) -> Result<(), RuntimeError> {
        let owner = self.entry.native_owner()?;
        let mut charge = owner
            .as_ref()
            .map(|owner| owner.reserve_peer(count.saturating_mul(std::mem::size_of::<Value>())))
            .transpose()?;
        let mut replacement = Vec::new();
        replacement.try_reserve_exact(count).map_err(|_| {
            owner
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(charge) = &mut charge {
            charge.resize(host_scheduling::slots(&replacement))?;
        }
        replacement.append(&mut self.values);
        self.values = replacement;
        self.entry.replace_argument_control(charge);
        Ok(())
    }

    fn push(&mut self, value: Value) -> Result<(), RuntimeError> {
        assert!(self.values.len() < self.values.capacity());
        self.entry.append_argument_payload(&value)?;
        self.values.push(value);
        Ok(())
    }

    pub(super) fn call(
        mut self,
        vm: &mut Vm,
        callee: Value,
        receiver: Value,
        construct: bool,
        target: Value,
    ) -> Result<Value, RuntimeError> {
        self.entry.retarget_native(vm.host_scheduling.as_ref())?;
        self.entry.refresh(&callee, &receiver, &self.values, 0)?;
        let owner = self.entry.native_owner()?;
        let result =
            vm.call_with_target_owned(callee, receiver, self.values, construct, target, self.entry);
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    pub(super) fn call_native(
        self,
        vm: &mut Vm,
        callee: Value,
        receiver: Value,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let target = if construct {
            callee.clone()
        } else {
            Value::Undefined
        };
        self.call(vm, callee, receiver, construct, target)
    }

    pub(super) fn call_native_borrowed(
        mut self,
        vm: &mut Vm,
        callee: &Value,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        self.entry.retarget_native(vm.host_scheduling.as_ref())?;
        // Cover copied callee/receiver buffers before making those copies.
        self.entry.refresh(callee, receiver, &self.values, 0)?;
        self.call_native(vm, callee.clone(), receiver.clone(), false)
    }

    pub(super) fn super_call(self, vm: &mut Vm, constructor: Value) -> Result<Value, RuntimeError> {
        if !vm.is_constructor(&constructor)? {
            return Err(RuntimeError::TypeError(
                "super constructor is not a constructor".into(),
            ));
        }
        let target = vm.new_target.clone();
        self.call(vm, constructor, Value::Undefined, true, target)
    }

    /// Transport converts values in place, then transfers each primitive to a
    /// pure heap consumer without keeping a second native payload charge.
    pub(super) fn into_array(mut self, vm: &mut Vm) -> Result<Value, RuntimeError> {
        self.entry.retarget_native(vm.host_scheduling.as_ref())?;
        let owner = self.entry.native_owner()?;
        let roots = RealmCallRoots::new(
            self.values
                .iter()
                .filter_map(Value::object_id)
                .count()
                .saturating_add(2),
            owner.as_ref(),
        )?;
        self.values
            .iter()
            .filter_map(Value::object_id)
            .try_for_each(|id| roots.push(&vm.heap, id))?;
        let result = vm.with_realm_call_roots(roots.clone(), |vm| {
            let prototype = vm.array_create_prototype()?;
            roots.push(&vm.heap, prototype)?;
            let array =
                vm.with_roots(|heap| heap.alloc_array(self.values.len() as u32, Some(prototype)))?;
            roots.push(&vm.heap, array)?;
            let mut values = self.values.into_iter();
            for (index, value) in values.by_ref().enumerate() {
                let bytes = host_scheduling::value_payload_bytes(&value);
                vm.with_roots(|heap| {
                    // Heap::set admits the same moved value immediately. No
                    // guest code or safepoint separates these ownership views.
                    self.entry.release_argument_payload(bytes);
                    heap.set(array, index.to_string(), value)
                })?;
            }
            drop(values);
            drop(self.entry);
            Ok(Value::Object(array))
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }
}

impl Vm {
    pub(super) fn argument_list_owner(
        &self,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let root = self.current_realm_root_owner()?;
        if let Some(scope) = self.host_scheduling.as_ref() {
            scope.shared_charge(0).map(Some)
        } else {
            // A suspended caller still supplies a terminal-state guard, while
            // an explicit native loan owns preparation inside its root frame.
            let ambient = realm_reentrancy::reserve_active_native(0)?;
            Ok(root.or(ambient))
        }
    }

    pub(super) fn with_array_like_values<T>(
        &mut self,
        value: &Value,
        operation: impl FnOnce(&mut Vm, ArgumentValues) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let Some(source) = value.object_id() else {
            return Err(RuntimeError::TypeError(
                "argument list must be an object".into(),
            ));
        };
        let owner = self.argument_list_owner()?;
        let input_roots = RealmCallRoots::new(1, owner.as_ref())?;
        input_roots.push(&self.heap, source)?;
        let result = self.with_realm_call_roots(input_roots, |vm| {
            let length = vm.get_property(value, &"length".into())?;
            let length = vm.coerce_length(&length)? as u64;
            let roots = RealmCallRoots::new(0, owner.as_ref())?;
            vm.with_realm_call_roots(roots.clone(), |vm| {
                let mut arguments = ArgumentValues::new(vm, owner.as_ref())?;
                // Grow from actual work, rather than reserving an untrusted
                // complete length before the first indexed getter can run.
                for index in 0..length {
                    vm.charge_step()?;
                    arguments.prepare_push()?;
                    let value = vm.get_property(value, &index.to_string().into())?;
                    if let Some(id) = value.object_id() {
                        roots.push_admitted(&vm.heap, id)?;
                    }
                    arguments.push(value)?;
                }
                operation(vm, arguments)
            })
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }
}
