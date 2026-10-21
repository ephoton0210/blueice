// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original reader accounts, returned snapshots and allocation ownership.

use super::*;
use crate::vm::host_objects::HostEventContext;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::realm_call_storage::RealmCallRoots;
use crate::vm::web_drag_drop::storage::NativeDragFile;

pub(super) struct FileInputContext {
    host: HostEventContext,
    original: Option<SharedSchedulingCharge>,
}
impl FileInputContext {
    pub(super) fn capture(vm: &mut Vm) -> Result<Self, RuntimeError> {
        let host = HostEventContext::capture(vm)?;
        let original = vm
            .host_file_inputs
            .retain_for_entry(host.registration_owner())?;
        let owner = original.as_ref().or(host.registration_owner());
        if let (Some(scope), Some(owner)) = (&vm.host_scheduling, owner) {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        }
        if let Some(owner) = owner {
            if !vm
                .host_file_inputs
                .iter()
                .all(|entry| entry.cached.can_join_owner(owner))
            {
                return Err(owner.fail());
            }
            for entry in vm.host_file_inputs.iter_mut() {
                entry.cached.attach_owner(owner)?;
            }
        }
        let context = Self { host, original };
        context.validate(vm)?;
        Ok(context)
    }
    pub(super) fn owner(&self) -> Option<&SharedSchedulingCharge> {
        self.original.as_ref().or(self.host.registration_owner())
    }
    pub(super) fn validate(&self, vm: &Vm) -> Result<(), RuntimeError> {
        self.host.validate(vm)?;
        if let Some(original) = &self.original {
            original.reserve_peer(0)?;
        }
        Ok(())
    }
    pub(super) fn with_roots<T>(
        &self,
        vm: &mut Vm,
        ids: impl Iterator<Item = ObjectId> + Clone,
        extra: usize,
        action: impl FnOnce(&mut Vm, &RealmCallRoots) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        self.validate(vm)?;
        let separate = self.original.as_ref().is_some_and(|owner| {
            self.host
                .event
                .owner
                .as_ref()
                .is_none_or(|event| !event.same_owner(owner))
                && self
                    .host
                    .registration_owner()
                    .is_none_or(|family| !family.same_owner(owner))
        });
        let run = |vm: &mut Vm| self.host.with_roots(vm, ids, extra, action);
        let result = if separate {
            let roots = RealmCallRoots::new(0, self.original.as_ref())?;
            vm.with_realm_call_roots(roots, run)
        } else {
            run(vm)
        };
        self.validate(vm).and(result)
    }
}

fn file_bytes(file: &HostFileData) -> usize {
    file.name
        .capacity()
        .saturating_add(file.media_type.capacity())
        .saturating_add(file.bytes.capacity())
}
pub(super) struct NativeFileUpdate {
    pub value: HostInputFilesUpdate,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeFileUpdate {
    pub(super) fn adopt(
        value: HostInputFilesUpdate,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let bytes = match &value {
            HostInputFilesUpdate::Selected { files, .. } => files
                .iter()
                .fold(host_scheduling::slots(files), |bytes, file| {
                    bytes.saturating_add(file_bytes(file))
                }),
            _ => 0,
        };
        let storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
        Ok(Self { value, storage })
    }
    pub(super) fn into_selected(self) -> (u64, NativeSelectedFiles) {
        let HostInputFilesUpdate::Selected { revision, files } = self.value else {
            unreachable!("selected reader snapshot")
        };
        (
            revision,
            NativeSelectedFiles {
                values: files.into_iter(),
                storage: self.storage,
            },
        )
    }
}
pub(super) struct NativeSelectedFiles {
    values: std::vec::IntoIter<HostFileData>,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeSelectedFiles {
    pub(super) fn len(&self) -> usize {
        self.values.len()
    }
    pub(super) fn next_owned(&mut self) -> Result<Option<NativeSelectedFile>, RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        Ok(self.values.next().map(|value| {
            let storage = self
                .storage
                .as_mut()
                .map(|storage| storage.split_off(file_bytes(&value)));
            NativeSelectedFile { value, storage }
        }))
    }
}
pub(super) struct NativeSelectedFile {
    value: HostFileData,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeSelectedFile {
    pub(super) fn into_blob(self) -> Result<NativeDragFile, RuntimeError> {
        NativeDragFile::from_owned_host(self.value, self.storage)
    }
}

impl Vm {
    pub(in crate::vm) fn file_input_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.host_file_inputs.can_join(scope)
            && self
                .host_file_inputs
                .iter()
                .all(|entry| entry.cached.can_join(scope))
    }
    pub(in crate::vm) fn attach_file_input_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        if !self.file_input_storage_can_join(scope) {
            return Err(scope.fail());
        }
        self.host_file_inputs.attach(scope)?;
        for entry in self.host_file_inputs.iter_mut() {
            entry.cached.attach(scope)?;
        }
        Ok(())
    }
    pub(in crate::vm) fn unowned_file_input_storage_bytes(&self) -> usize {
        self.host_file_inputs
            .iter()
            .fold(self.host_file_inputs.unowned_bytes(), |bytes, entry| {
                bytes.saturating_add(entry.cached.unowned_bytes())
            })
    }
}
