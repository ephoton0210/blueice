// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared Array sort snapshots retain native data across guest comparisons.

use super::host_scheduling::SharedSchedulingCharge;
use super::native_value_storage::NativeValue;
use super::realm_call_storage::RealmCallRoots;
use super::transport_buffer_storage::TransportVector;
use super::*;

pub(super) struct SortKey {
    pub(super) name: PropertyName,
    _storage: Option<SharedSchedulingCharge>,
}

impl Vm {
    pub(super) fn array_sort_input(&self) -> Result<TransportVector<Value>, RuntimeError> {
        let owner = self.argument_list_owner()?;
        TransportVector::with_capacity(0, owner.as_ref())
    }

    pub(super) fn push_array_sort_input(
        &self,
        values: &mut TransportVector<Value>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let owner = values.owner_view()?.or(self.argument_list_owner()?);
        values.push_owned(NativeValue::adopt(value, owner.as_ref())?)
    }

    pub(super) fn array_sort_key(
        index: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<SortKey, RuntimeError> {
        let name: PropertyName = index.to_string().into();
        let storage = owner
            .map(|owner| owner.reserve_peer(host_scheduling::property_name_payload_bytes(&name)))
            .transpose()?;
        Ok(SortKey {
            name,
            _storage: storage,
        })
    }

    fn validate_sort_origin(
        owner: Option<&SharedSchedulingCharge>,
        original: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        for source in [owner, original].into_iter().flatten() {
            source.reserve_peer(0)?;
        }
        Ok(())
    }

    /// Keep the existing stable bottom-up comparator sequence. Only admitted
    /// indices enter merge scratch; each primitive payload has one snapshot.
    pub(super) fn array_sort_values(
        &mut self,
        mut values: TransportVector<Value>,
        compare: &Value,
    ) -> Result<TransportVector<Value>, RuntimeError> {
        let original = values.retain_for_vm(self)?;
        let owner = values.owner_view()?;
        let roots = RealmCallRoots::new(
            values.iter().filter_map(Value::object_id).count()
                + usize::from(compare.object_id().is_some()),
            owner.as_ref(),
        )?;
        values
            .iter()
            .filter_map(Value::object_id)
            .try_for_each(|id| roots.push(&self.heap, id))?;
        compare
            .object_id()
            .into_iter()
            .try_for_each(|id| roots.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(roots, |vm| {
            if values.len() < 2 {
                return Ok(());
            }
            let mut order = TransportVector::capture(0..values.len(), owner.as_ref())?;
            let mut scratch = TransportVector::capture(
                std::iter::repeat_n(0usize, values.len()),
                owner.as_ref(),
            )?;
            let mut width = 1usize;
            while width < order.len() {
                let mut start = 0usize;
                while start < order.len() {
                    let middle = start.saturating_add(width).min(order.len());
                    let end = middle.saturating_add(width).min(order.len());
                    let (mut left, mut right, mut target) = (start, middle, start);
                    while left < middle && right < end {
                        if vm.array_sort_order(
                            compare,
                            &values[order[right]],
                            &values[order[left]],
                            owner.as_ref(),
                            original.as_ref(),
                        )? == std::cmp::Ordering::Less
                        {
                            scratch[target] = order[right];
                            right += 1;
                        } else {
                            scratch[target] = order[left];
                            left += 1;
                        }
                        target += 1;
                    }
                    while left < middle {
                        scratch[target] = order[left];
                        target += 1;
                        left += 1;
                    }
                    while right < end {
                        scratch[target] = order[right];
                        target += 1;
                        right += 1;
                    }
                    order[start..end].copy_from_slice(&scratch[start..end]);
                    start = end;
                }
                width = width.saturating_mul(2);
            }
            Self::validate_sort_origin(owner.as_ref(), original.as_ref())?;
            values.apply_order(&order, &mut scratch);
            Ok(())
        });
        Self::validate_sort_origin(owner.as_ref(), original.as_ref())?;
        result?;
        Ok(values)
    }

    fn array_sort_order(
        &mut self,
        compare: &Value,
        left: &Value,
        right: &Value,
        owner: Option<&SharedSchedulingCharge>,
        original: Option<&SharedSchedulingCharge>,
    ) -> Result<std::cmp::Ordering, RuntimeError> {
        Self::validate_sort_origin(owner, original)?;
        if *compare == Value::Undefined {
            let left = self.coerce_string(left)?;
            let left = NativeValue::adopt(Value::String(left), owner)?;
            Self::validate_sort_origin(owner, original)?;
            let right = self.coerce_string(right)?;
            let right = NativeValue::adopt(Value::String(right), owner)?;
            Self::validate_sort_origin(owner, original)?;
            let (Value::String(left), Value::String(right)) = (left.value(), right.value()) else {
                unreachable!("default sort comparisons converted both values to String")
            };
            return Ok(left.cmp(right));
        }
        let mut arguments = TransportVector::with_capacity(2, owner)?;
        arguments.push_owned(NativeValue::copied(left, owner)?)?;
        arguments.push_owned(NativeValue::copied(right, owner)?)?;
        let value = arguments.with_arguments(self, |vm, arguments| {
            arguments.call_native_borrowed(vm, compare, &Value::Undefined)
        })?;
        let value = NativeValue::adopt(value, owner)?;
        Self::validate_sort_origin(owner, original)?;
        let value = value.with_value(self, |vm, value| vm.coerce_number(value))?;
        Self::validate_sort_origin(owner, original)?;
        Ok(if value.is_nan() || value == 0.0 {
            std::cmp::Ordering::Equal
        } else if value < 0.0 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        })
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/array_sort_storage.rs"]
mod ownership_tests;
