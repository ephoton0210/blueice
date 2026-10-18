// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native transport snapshots own storage independently of either realm VM.

use super::host_scheduling::SharedSchedulingCharge;
use super::native_value_storage::NativeValue;
use super::*;
use crate::heap::{SharedBuffer, TypedArrayKind};

pub(super) struct TransportVector<T> {
    values: Vec<T>,
    // The vector and its primitive payloads drop before their native credit.
    storage: Option<SharedSchedulingCharge>,
}

impl<T> TransportVector<T> {
    #[cfg(any(test, coverage))]
    pub(super) fn retained_slots(&self) -> usize {
        host_scheduling::slots(&self.values)
    }

    pub(super) fn with_capacity(
        count: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(host_scheduling::slots(&values))?;
        }
        Ok(Self { values, storage })
    }

    pub(super) fn validate_owner(&self) -> Result<(), RuntimeError> {
        self.owner_view()?;
        Ok(())
    }

    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
}

impl<T: Copy> TransportVector<T> {
    pub(super) fn push(&mut self, value: T) -> Result<(), RuntimeError> {
        if self.values.len() == self.values.capacity() {
            let capacity = self.values.capacity().saturating_mul(2).max(4);
            let mut replacement = Self::with_capacity(capacity, self.storage.as_ref())?;
            replacement.values.extend_from_slice(&self.values);
            // Destroy the old slots before their native credit is returned.
            *self = replacement;
        }
        self.values.push(value);
        Ok(())
    }

    pub(super) fn truncate(&mut self, length: usize) {
        self.values.truncate(length);
    }

    /// Both iterator passes inspect the same immutable metadata, with no guest
    /// calls or collection between counting and filling the admitted slots.
    pub(super) fn capture(
        values: impl Iterator<Item = T> + Clone,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let count = values.clone().count();
        let mut result = Self::with_capacity(count, owner)?;
        for value in values {
            assert!(result.values.len() < count);
            result.values.push(value);
        }
        Ok(result)
    }
}

impl<T: Copy + PartialEq> TransportVector<T> {
    pub(super) fn dedup(&mut self) {
        // Vec::dedup preserves capacity, so all reusable slots stay owned.
        self.values.dedup();
    }
}

impl TransportVector<u8> {
    pub(super) fn buffer_bytes(
        vm: &Vm,
        buffer: ObjectId,
        byte_length: usize,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        Self::buffer_range(vm, buffer, 0, byte_length, hint)
    }

    pub(super) fn buffer_range(
        vm: &Vm,
        buffer: ObjectId,
        byte_offset: usize,
        byte_length: usize,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let owner = vm.transport_storage_owner(hint)?;
        let mut result = Self::with_capacity(byte_length, owner.as_ref())?;
        result.values.resize(byte_length, 0);
        vm.heap
            .array_buffer_read_into(buffer, byte_offset, &mut result.values)?;
        Ok(result)
    }

    fn into_immutable_buffer(
        mut self,
        vm: &mut Vm,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        self.validate_owner()?;
        let source_owner = self
            .storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()?;
        if let Some(scope) = vm.host_scheduling.as_ref() {
            if !self
                .storage
                .as_ref()
                .is_some_and(|owner| owner.belongs_to(scope))
            {
                let replacement = scope.shared_charge(host_scheduling::slots(&self.values))?;
                self.storage = Some(replacement);
            }
        }
        let values = std::mem::take(&mut self.values);
        let result = vm.with_roots(|heap| {
            // The same Vec moves directly into the managed data block. Keep
            // native ownership through root preparation, then release it only
            // immediately before the pure heap consumer admits that Vec.
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.alloc_immutable_array_buffer(values, Some(prototype))
        });
        self.validate_owner()?;
        if let Some(owner) = source_owner {
            owner.reserve_peer(0)?;
        }
        result
    }
}

impl TransportVector<Value> {
    pub(super) fn push_owned(&mut self, value: NativeValue) -> Result<(), RuntimeError> {
        self.validate_owner()?;
        value.validate_owner()?;
        if self.storage.is_none() {
            if let Some(owner) = value.owner_view()? {
                let bytes = host_scheduling::slots(&self.values)
                    .saturating_add(host_scheduling::values_bytes(&self.values));
                self.storage = Some(owner.reserve_peer(bytes)?);
            }
        }
        if self.values.len() == self.values.capacity() {
            let count = self.values.capacity().saturating_mul(2).max(4);
            let old_control = host_scheduling::slots(&self.values);
            let mut charge = self
                .storage
                .as_ref()
                .map(|owner| owner.reserve_peer(count.saturating_mul(std::mem::size_of::<Value>())))
                .transpose()?;
            let mut replacement = Vec::new();
            replacement.try_reserve_exact(count).map_err(|_| {
                self.storage
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            if let Some(charge) = &mut charge {
                charge.resize(host_scheduling::slots(&replacement))?;
            }
            // Merge the two live control leases before moving data, so a
            // refused transfer leaves both allocations with their owners.
            if let (Some(owner), Some(charge)) = (&mut self.storage, &mut charge) {
                owner.absorb_from(charge)?;
            }
            replacement.append(&mut self.values);
            self.values = replacement;
            if let Some(owner) = &mut self.storage {
                owner.release_to(owner.bytes() - old_control);
            }
        }
        value.move_into(&mut self.values, &mut self.storage)
    }

    pub(super) fn typed_range(
        vm: &Vm,
        array: ObjectId,
        start: usize,
        length: usize,
    ) -> Result<Self, RuntimeError> {
        let end = start
            .checked_add(length)
            .ok_or_else(|| RuntimeError::RangeError("TypedArray range is too large".into()))?;
        if length == 0 {
            let owner = vm.argument_list_owner()?;
            return Self::with_capacity(0, owner.as_ref());
        }
        let (_, _, available, kind) = vm.heap.typed_array_info(array)?;
        if end > available {
            return Err(RuntimeError::TypeError(
                "TypedArray is out of bounds".into(),
            ));
        }
        Self::read_typed_range(vm, array, start, length, kind, None)
    }

    pub(super) fn typed_values(
        vm: &Vm,
        array: ObjectId,
        hint: Option<&SharedSchedulingCharge>,
        invalid_source: &'static str,
    ) -> Result<(TypedArrayKind, Self), RuntimeError> {
        let (buffer, _, length, kind) = vm.heap.typed_array_info(array)?;
        if vm.heap.buffer_is_detached(buffer)? || vm.heap.typed_array_is_out_of_bounds(array)? {
            return Err(RuntimeError::TypeError(invalid_source.into()));
        }
        Ok((
            kind,
            Self::read_typed_range(vm, array, 0, length, kind, hint)?,
        ))
    }

    fn read_typed_range(
        vm: &Vm,
        array: ObjectId,
        start: usize,
        length: usize,
        kind: TypedArrayKind,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let owner = vm.transport_storage_owner(hint)?;
        let mut result = Self::with_capacity(length, owner.as_ref())?;
        for index in start..start + length {
            // TypedArray elements are Numbers or at most two logical u32
            // BigInt limbs under the existing primitive-payload byte policy.
            // Wider BigInt allocator staging remains a separate work item.
            let payload_bytes =
                if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64) {
                    8
                } else {
                    0
                };
            let retained = result
                .storage
                .as_ref()
                .map_or(0, SharedSchedulingCharge::bytes);
            if let Some(storage) = &mut result.storage {
                storage.resize(retained.saturating_add(payload_bytes))?;
            }
            let value = vm
                .heap
                .typed_array_index_value(array, index)?
                .expect("a pure snapshot preserves each validated TypedArray index");
            if let Some(storage) = &mut result.storage {
                storage.resize(
                    retained.saturating_add(host_scheduling::value_payload_bytes(&value)),
                )?;
            }
            result.values.push(value);
        }
        Ok(result)
    }

    /// Reordering moves existing primitives, so it neither clones payloads nor
    /// changes the vector's total native credit. Scratch is admitted separately.
    pub(super) fn apply_order(&mut self, order: &[usize], inverse: &mut [usize]) {
        assert_eq!(self.values.len(), order.len());
        assert_eq!(self.values.len(), inverse.len());
        for (target, &original) in order.iter().enumerate() {
            inverse[original] = target;
        }
        for index in 0..inverse.len() {
            while inverse[index] != index {
                let next = inverse[index];
                self.values.swap(index, next);
                inverse.swap(index, next);
            }
        }
    }

    pub(super) fn reverse(&mut self) {
        self.values.reverse();
    }

    pub(super) fn retain_for_vm(
        &mut self,
        vm: &Vm,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let original = self.owner_view()?;
        let bytes = host_scheduling::slots(&self.values)
            .saturating_add(host_scheduling::values_bytes(&self.values));
        if let Some(scope) = vm.host_scheduling.as_ref() {
            if !self
                .storage
                .as_ref()
                .is_some_and(|owner| owner.belongs_to(scope))
            {
                self.storage = Some(scope.shared_charge(bytes)?);
            }
        } else if self.storage.is_none() {
            self.storage = vm
                .argument_list_owner()?
                .map(|owner| owner.reserve_peer(bytes))
                .transpose()?;
        }
        Ok(original)
    }

    pub(super) fn with_arguments<T>(
        mut self,
        vm: &mut Vm,
        operation: impl FnOnce(
            &mut Vm,
            argument_list_storage::ArgumentValues,
        ) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let original = self.retain_for_vm(vm)?;
        let control = self
            .storage
            .as_mut()
            .map(|owner| owner.split_off(host_scheduling::slots(&self.values)));
        let entry = execution_payload::CallEntryCharge::adopt_native_arguments(
            vm,
            self.storage.take(),
            control,
        );
        let result = operation(
            vm,
            argument_list_storage::ArgumentValues::adopt_retained(self.values, entry),
        );
        if let Some(owner) = original {
            owner.reserve_peer(0)?;
        }
        result
    }

    pub(super) fn into_array(self, vm: &mut Vm) -> Result<Value, RuntimeError> {
        self.with_arguments(vm, |vm, arguments| arguments.into_array(vm))
    }
}

impl<T> std::ops::Deref for TransportVector<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}

impl<T: Copy> std::ops::DerefMut for TransportVector<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}

pub(super) struct BufferSnapshot {
    shared: bool,
    detached: bool,
    byte_length: usize,
    maximum: usize,
    flexible: bool,
    immutable: bool,
    bytes: Option<TransportVector<u8>>,
    backing: Option<std::sync::Arc<SharedBuffer>>,
}

impl BufferSnapshot {
    pub(super) fn capture(
        vm: &Vm,
        buffer: ObjectId,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let shared = vm.heap.buffer_is_shared(buffer)?;
        let detached = vm.heap.buffer_is_detached(buffer)?;
        let byte_length = vm.heap.buffer_byte_length(buffer)?;
        let maximum = vm.heap.buffer_max_byte_length(buffer)?;
        let flexible = vm.heap.buffer_resizable(buffer)? || vm.heap.buffer_growable(buffer)?;
        let immutable = !shared && vm.heap.buffer_is_immutable(buffer)?;
        let bytes = if !shared && !detached {
            Some(TransportVector::buffer_bytes(
                vm,
                buffer,
                byte_length,
                hint,
            )?)
        } else {
            None
        };
        let backing = if shared {
            Some(vm.heap.shared_buffer_backing(buffer)?)
        } else {
            None
        };
        Ok(Self {
            shared,
            detached,
            byte_length,
            maximum,
            flexible,
            immutable,
            bytes,
            backing,
        })
    }

    pub(super) fn is_ordinary(&self) -> bool {
        !self.shared
    }

    pub(super) fn needs_local_mirror(&self) -> bool {
        !self.shared && !self.detached
    }

    pub(super) fn into_buffer(self, vm: &mut Vm) -> Result<ObjectId, RuntimeError> {
        let prototype = vm.buffer_prototype(if self.shared {
            "SharedArrayBuffer"
        } else {
            "ArrayBuffer"
        })?;
        let buffer = if let Some(backing) = self.backing {
            vm.with_roots(|heap| {
                heap.alloc_shared_array_buffer_backing(
                    backing,
                    self.flexible.then_some(self.maximum),
                    Some(prototype),
                )
            })?
        } else if self.immutable {
            return self
                .bytes
                .expect("an immutable buffer retains its snapshot")
                .into_immutable_buffer(vm, prototype);
        } else if self.flexible {
            vm.with_roots(|heap| {
                heap.alloc_resizable_array_buffer(self.byte_length, self.maximum, Some(prototype))
            })?
        } else {
            vm.with_roots(|heap| heap.alloc_array_buffer(self.byte_length, Some(prototype)))?
        };
        if self.detached {
            vm.heap.detach_array_buffer(buffer)?;
        } else if let Some(bytes) = self.bytes {
            vm.heap.array_buffer_write(buffer, 0, &bytes)?;
            bytes.validate_owner()?;
        }
        Ok(buffer)
    }
}

impl Vm {
    fn transport_storage_owner(
        &self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        if let Some(owner) = hint {
            owner.reserve_peer(0).map(Some)
        } else {
            self.argument_list_owner()
        }
    }

    /// A neutral context borrows the operation's active/native owner, or its
    /// bounded peer when neither this VM nor the active stack captured one.
    pub(super) fn transport_realm_owner(
        &self,
        realm_id: ObjectId,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        if let Some(owner) = self.argument_list_owner()? {
            return Ok(Some(owner));
        }
        self.test262_realm_handle(realm_id)?
            .read(self.object_prototype.heap, Vm::argument_list_owner)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/transport_buffer_storage.rs"]
mod ownership_tests;
