// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property-definition inputs retain native storage through descriptor routing.

use super::host_scheduling::SharedSchedulingCharge;
use super::realm_call_storage::RealmCallRoots;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/property_definition_storage.rs"]
mod ownership_tests;

pub(super) struct PropertyDefinitionInput {
    key: PropertyName,
    descriptor: PropertyDescriptor,
    // Both input buffers are destroyed before their native credit is returned.
    storage: Option<SharedSchedulingCharge>,
}

pub(super) fn definition_bytes(key: &PropertyName, descriptor: &PropertyDescriptor) -> usize {
    [
        descriptor.value.as_ref(),
        descriptor.get.as_ref(),
        descriptor.set.as_ref(),
    ]
    .into_iter()
    .flatten()
    .fold(
        host_scheduling::property_name_payload_bytes(key),
        |bytes, value| bytes.saturating_add(host_scheduling::value_payload_bytes(value)),
    )
}

pub(super) fn retain_definition_storage(
    vm: &Vm,
    key: &PropertyName,
    descriptor: &PropertyDescriptor,
    storage: &mut Option<SharedSchedulingCharge>,
) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
    if let Some(storage) = storage {
        storage.resize(definition_bytes(key, descriptor))?;
    } else if let Some(owner) = vm.argument_list_owner()? {
        *storage = Some(owner.reserve_peer(definition_bytes(key, descriptor))?);
    }
    storage
        .as_ref()
        .map(|owner| owner.reserve_peer(0))
        .transpose()
}

pub(super) fn validate_definition_storage(
    storage: &Option<SharedSchedulingCharge>,
) -> Result<(), RuntimeError> {
    if let Some(storage) = storage {
        storage.reserve_peer(0)?;
    }
    Ok(())
}

impl PropertyDefinitionInput {
    pub(super) fn new(
        vm: &Vm,
        key: PropertyName,
        descriptor: PropertyDescriptor,
    ) -> Result<Self, RuntimeError> {
        let mut input = Self::from_retained(key, descriptor, None);
        retain_definition_storage(vm, &input.key, &input.descriptor, &mut input.storage)?;
        Ok(input)
    }

    pub(super) fn from_retained(
        key: PropertyName,
        descriptor: PropertyDescriptor,
        storage: Option<SharedSchedulingCharge>,
    ) -> Self {
        Self {
            key,
            descriptor,
            storage,
        }
    }

    pub(super) fn define(mut self, vm: &mut Vm, object: ObjectId) -> Result<bool, RuntimeError> {
        let original = self
            .storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()?;
        if let Some(scope) = vm.host_scheduling.as_ref() {
            if self
                .storage
                .as_ref()
                .is_some_and(|owner| !owner.belongs_to(scope))
            {
                let replacement =
                    scope.shared_charge(definition_bytes(&self.key, &self.descriptor))?;
                self.storage = Some(replacement);
            }
        }
        let owner = retain_definition_storage(vm, &self.key, &self.descriptor, &mut self.storage)?;
        let objects = [
            self.descriptor.value.as_ref(),
            self.descriptor.get.as_ref(),
            self.descriptor.set.as_ref(),
        ];
        let roots = RealmCallRoots::new(
            1 + objects
                .into_iter()
                .flatten()
                .filter_map(Value::object_id)
                .count(),
            owner.as_ref(),
        )?;
        roots.push(&vm.heap, object)?;
        for object in objects.into_iter().flatten().filter_map(Value::object_id) {
            roots.push(&vm.heap, object)?;
        }
        let mut storage = self.storage.take();
        let result = vm.with_realm_call_roots(roots, |vm| {
            vm.object_define_own_property_input(object, self.key, self.descriptor, &mut storage)
        });
        if let Some(owner) = original {
            owner.reserve_peer(0)?;
        }
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }
}
