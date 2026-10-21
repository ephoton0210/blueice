// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reader candidates keep their Box and nested buffers paid until publication.

use super::*;
use crate::heap::{FileReaderState, PreparedFileReaderUpdate};
use crate::vm::host_scheduling::SharedSchedulingCharge;

pub(super) struct NativeReaderState {
    pub state: Box<FileReaderState>,
    storage: Option<SharedSchedulingCharge>,
}

pub(super) struct PreparedReaderState {
    state: PreparedFileReaderUpdate,
    storage: Option<SharedSchedulingCharge>,
}

impl NativeReaderState {
    pub(super) fn new(owner: Option<&SharedSchedulingCharge>) -> Result<Self, RuntimeError> {
        let storage = owner
            .map(|owner| owner.reserve_peer(std::mem::size_of::<FileReaderState>()))
            .transpose()?;
        Ok(Self {
            state: Box::default(),
            storage,
        })
    }

    pub(super) fn copied(
        vm: &Vm,
        id: ObjectId,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let source = vm.heap.file_reader(id)?;
        let bytes =
            std::mem::size_of::<FileReaderState>().saturating_add(source.copied_buffer_bytes());
        let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
        let state = Box::new(source.clone());
        if let Some(storage) = &mut storage {
            storage.resize(state.bytes())?;
        }
        Ok(Self { state, storage })
    }

    fn check_value(&self, value: &NativeValue) -> Result<(), RuntimeError> {
        value.validate_owner()?;
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
            if value
                .owner_view()?
                .as_ref()
                .is_none_or(|owner| !storage.same_owner(owner))
            {
                return Err(storage.fail());
            }
        } else {
            assert!(value.owner_view()?.is_none());
        }
        Ok(())
    }

    pub(super) fn result(
        &mut self,
        vm: &Vm,
        roots: &RealmCallRoots,
        value: NativeValue,
    ) -> Result<(), RuntimeError> {
        self.check_value(&value)?;
        if let Some(object) = value.value().object_id() {
            roots.push(&vm.heap, object)?;
        }
        let old = host_scheduling::value_payload_bytes(&self.state.result);
        let (value, incoming) = value.into_retained_parts();
        self.state.result = value;
        if let Some(storage) = &mut self.storage {
            storage.release_to(storage.bytes() - old);
            storage.absorb(incoming.expect("same reader account"))?;
        }
        Ok(())
    }

    pub(super) fn encoding(&mut self, value: Option<NativeValue>) -> Result<(), RuntimeError> {
        if let Some(value) = &value {
            self.check_value(value)?;
        }
        let old = self.state.encoding.as_ref().map_or(0, |text| {
            text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
        });
        let (text, incoming) = if let Some(value) = value {
            let (Value::String(text), incoming) = value.into_retained_parts() else {
                unreachable!("converted reader encoding")
            };
            (Some(text), incoming)
        } else {
            (None, None)
        };
        self.state.encoding = text;
        if let Some(storage) = &mut self.storage {
            storage.reserve_peer(0)?;
            storage.release_to(storage.bytes() - old);
            if let Some(incoming) = incoming {
                storage.absorb(incoming)?;
            }
        }
        Ok(())
    }

    fn reconcile(&mut self) -> Result<(), RuntimeError> {
        if let Some(storage) = &mut self.storage {
            storage.reserve_peer(0)?;
            storage.release_to(self.state.bytes());
        }
        Ok(())
    }

    pub(super) fn allocate(
        mut self,
        vm: &mut Vm,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        self.reconcile()?;
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.alloc_file_reader_owned(self.state, prototype)
        })
    }

    pub(super) fn prepare(
        mut self,
        vm: &mut Vm,
        id: ObjectId,
    ) -> Result<PreparedReaderState, RuntimeError> {
        self.reconcile()?;
        let state = vm.with_roots(|heap| heap.prepare_file_reader_update_owned(id, self.state))?;
        Ok(PreparedReaderState {
            state,
            storage: self.storage,
        })
    }

    pub(super) fn into_heap(self, vm: &mut Vm, id: ObjectId) -> Result<(), RuntimeError> {
        self.prepare(vm, id)?.commit(vm);
        Ok(())
    }
}

impl PreparedReaderState {
    /// All fallible root/queue/serial preparation precedes this pure transfer.
    pub(super) fn commit(mut self, vm: &mut Vm) {
        if let Some(storage) = &mut self.storage {
            storage.release_to(0);
        }
        vm.heap.commit_file_reader_update(self.state);
    }
}
