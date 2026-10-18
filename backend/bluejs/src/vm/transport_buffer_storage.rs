// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native transport snapshots own storage independently of either realm VM.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use crate::heap::{SharedBuffer, TypedArrayKind};

pub(super) struct TransportVector<T> {
    values: Vec<T>,
    // The vector and its primitive payloads drop before their native credit.
    storage: Option<SharedSchedulingCharge>,
}

impl<T> TransportVector<T> {
    fn with_capacity(
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
        if let Some(owner) = &self.storage {
            owner.reserve_peer(0)?;
        }
        Ok(())
    }
}

impl<T: Copy> TransportVector<T> {
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
        let owner = vm.transport_storage_owner(hint)?;
        let mut result = Self::with_capacity(byte_length, owner.as_ref())?;
        result.values.resize(byte_length, 0);
        vm.heap
            .array_buffer_read_into(buffer, 0, &mut result.values)?;
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
        let owner = vm.transport_storage_owner(hint)?;
        let mut result = Self::with_capacity(length, owner.as_ref())?;
        for index in 0..length {
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
        Ok((kind, result))
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
