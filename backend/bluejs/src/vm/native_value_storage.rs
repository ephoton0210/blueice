// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scalar data retains native credit; object IDs use scoped consumer roots.

use super::host_scheduling::{value_payload_bytes, SharedSchedulingCharge};
use super::realm_call_storage::RealmCallRoots;
use super::*;

pub(super) struct NativeValue {
    value: Value,
    // Primitive buffers are destroyed before their credit is returned.
    storage: Option<SharedSchedulingCharge>,
}

pub(super) struct NativeValueIter {
    values: std::vec::IntoIter<Value>,
    // IntoIter retains the full Vec backing until it is dropped. Consumed
    // values take their payload credit; the remaining slots stay paid here.
    storage: Option<SharedSchedulingCharge>,
}

impl NativeValueIter {
    pub(super) fn new(values: Vec<Value>, storage: Option<SharedSchedulingCharge>) -> Self {
        debug_assert!(storage.as_ref().is_none_or(|storage| {
            storage.bytes()
                == host_scheduling::slots(&values)
                    .saturating_add(host_scheduling::values_bytes(&values))
        }));
        Self {
            values: values.into_iter(),
            storage,
        }
    }

    pub(super) fn next_owned(&mut self) -> Result<Option<NativeValue>, RuntimeError> {
        if let Some(owner) = &self.storage {
            owner.reserve_peer(0)?;
        }
        Ok(self.values.next().map(|value| {
            let storage = self
                .storage
                .as_mut()
                .map(|owner| owner.split_off(value_payload_bytes(&value)));
            NativeValue { value, storage }
        }))
    }
}

impl NativeValue {
    pub(super) fn from_retained_parts(
        value: Value,
        storage: Option<SharedSchedulingCharge>,
    ) -> Self {
        debug_assert!(storage
            .as_ref()
            .is_none_or(|storage| storage.bytes() == value_payload_bytes(&value)));
        Self { value, storage }
    }

    pub(super) fn copied_property_key(
        key: &PropertyName,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut storage = owner
            .map(|owner| owner.reserve_peer(host_scheduling::property_name_payload_bytes(key)))
            .transpose()?;
        let value = key.value();
        if let Some(storage) = &mut storage {
            storage.resize(value_payload_bytes(&value))?;
        }
        Ok(Self { value, storage })
    }

    /// The caller already produced this value. Retain its storage before any
    /// subsequent guest call or collection; producer staging is separate.
    pub(super) fn adopt(
        value: Value,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let storage = owner
            .map(|owner| owner.reserve_peer(value_payload_bytes(&value)))
            .transpose()?;
        Ok(Self { value, storage })
    }

    pub(super) fn copied(
        value: &Value,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut storage = owner
            .map(|owner| owner.reserve_peer(value_payload_bytes(value)))
            .transpose()?;
        let value = value.clone();
        if let Some(storage) = &mut storage {
            storage.resize(value_payload_bytes(&value))?;
        }
        Ok(Self { value, storage })
    }

    pub(super) fn typed_element(
        vm: &Vm,
        object: ObjectId,
        index: usize,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let (_, _, _, kind) = vm.heap.typed_array_info(object)?;
        let owner = if let Some(owner) = hint {
            Some(owner.reserve_peer(0)?)
        } else {
            vm.argument_list_owner()?
        };
        let mut storage = owner
            .as_ref()
            .map(|owner| owner.reserve_peer(if kind.bigint() { 8 } else { 0 }))
            .transpose()?;
        // Earlier callbacks can detach or shrink a view. Missing elements
        // remain undefined in the operation's originally captured range.
        let value = vm
            .heap
            .typed_array_index_value(object, index)?
            .unwrap_or(Value::Undefined);
        if let Some(storage) = &mut storage {
            storage.resize(value_payload_bytes(&value))?;
        }
        Ok(Self { value, storage })
    }

    pub(super) fn value(&self) -> &Value {
        &self.value
    }

    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(super) fn validate_owner(&self) -> Result<(), RuntimeError> {
        self.owner_view()?;
        Ok(())
    }

    pub(super) fn with_value<T>(
        &self,
        vm: &mut Vm,
        operation: impl FnOnce(&mut Vm, &Value) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let owner = self.owner_view()?;
        let result = if let Some(object) = self.value.object_id() {
            let roots = RealmCallRoots::new(1, owner.as_ref())?;
            roots.push(&vm.heap, object)?;
            vm.with_realm_call_roots(roots, |vm| operation(vm, &self.value))
        } else {
            operation(vm, &self.value)
        };
        self.validate_owner()?;
        result
    }

    /// Keep an object rooted while its owned value moves into a consumer,
    /// including any getter that runs before that consumer takes the value.
    pub(super) fn with_owned<T>(
        self,
        vm: &mut Vm,
        operation: impl FnOnce(&mut Vm, Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let owner = self.owner_view()?;
        let result = if let Some(object) = self.value.object_id() {
            let roots = RealmCallRoots::new(1, owner.as_ref())?;
            roots.push(&vm.heap, object)?;
            vm.with_realm_call_roots(roots, |vm| operation(vm, self))
        } else {
            operation(vm, self)
        };
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    /// A native input keeps its document context even when a primitive needs
    /// no object root. Nested native preparation borrows that same account.
    pub(super) fn with_owned_context<T>(
        mut self,
        vm: &mut Vm,
        operation: impl FnOnce(&mut Vm, Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if self.storage.is_none() {
            self = self.retain_for_owner(vm.argument_list_owner()?.as_ref())?;
        }
        let owner = self.owner_view()?;
        if let (Some(scope), Some(owner)) = (&vm.host_scheduling, &owner) {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        }
        let roots = RealmCallRoots::new(
            usize::from(self.value.object_id().is_some()),
            owner.as_ref(),
        )?;
        if let Some(object) = self.value.object_id() {
            roots.push(&vm.heap, object)?;
        }
        let result = vm.with_realm_call_roots(roots, |vm| operation(vm, self));
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    /// A pure return boundary transfers this same value to the ordinary VM
    /// result consumer. No guest call or safepoint separates those views.
    pub(super) fn into_value(self) -> Value {
        drop(self.storage);
        self.value
    }

    /// Transfer this same buffer to an independently owned native consumer.
    /// Its caller keeps the original identity through any destination change.
    pub(super) fn retain_for_owner(
        mut self,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        self.validate_owner()?;
        if let Some(owner) = owner {
            if self
                .storage
                .as_ref()
                .is_none_or(|storage| !storage.same_owner(owner))
            {
                self.storage = Some(owner.reserve_peer(value_payload_bytes(&self.value))?);
            }
        }
        Ok(self)
    }

    /// A prepared native consumer takes both data and credit without copying.
    pub(super) fn into_retained_parts(self) -> (Value, Option<SharedSchedulingCharge>) {
        (self.value, self.storage)
    }

    /// Move this same primitive lease into guest-capable property definition;
    /// its consumer admits the key and retains source terminal-state identity.
    pub(super) fn into_property_definition(
        self,
        key: PropertyName,
    ) -> super::property_definition_storage::PropertyDefinitionInput {
        super::property_definition_storage::PropertyDefinitionInput::from_retained(
            key,
            PropertyDescriptor::data(self.value, true, true, true),
            self.storage,
        )
    }

    /// Keep the input paid through root preparation, then hand the same value
    /// to a pure managed-heap consumer without an intermediate guest call.
    pub(super) fn into_heap<T>(
        mut self,
        vm: &mut Vm,
        operation: impl FnOnce(&mut Heap, Value) -> Result<T, HeapError>,
    ) -> Result<T, RuntimeError> {
        let owner = self.owner_view()?;
        let roots = self
            .value
            .object_id()
            .map(|object| {
                let roots = RealmCallRoots::new(1, owner.as_ref())?;
                roots.push(&vm.heap, object)?;
                Ok::<_, RuntimeError>(roots)
            })
            .transpose()?;
        let mut storage = self.storage.take();
        let transfer = |vm: &mut Vm| {
            vm.with_roots(|heap| {
                if let Some(storage) = &mut storage {
                    storage.release_to(0);
                }
                operation(heap, self.value)
            })
        };
        let result = if let Some(roots) = roots {
            vm.with_realm_call_roots(roots, transfer)
        } else {
            transfer(vm)
        };
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    /// The destination prepared its slots before this pure move. Borrowed
    /// transfer preserves source data ownership on every refusal path.
    pub(super) fn move_into(
        mut self,
        values: &mut Vec<Value>,
        storage: &mut Option<SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        self.validate_owner()?;
        assert!(values.len() < values.capacity());
        if let Some(target) = storage {
            if self.storage.is_none() {
                self.storage = Some(target.reserve_peer(value_payload_bytes(&self.value))?);
            }
            target.absorb_from(
                self.storage
                    .as_mut()
                    .expect("the destination supplied this owner"),
            )?;
        } else {
            assert!(
                self.storage.is_none(),
                "the destination admits its existing data first"
            );
        }
        values.push(self.value);
        Ok(())
    }
}
