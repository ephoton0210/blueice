// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drag candidates retain their copied buffers until managed publication.

use super::*;
use crate::heap::WebBlobData;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::web_event_storage::push_slot;

pub(in crate::vm) fn copy_string(
    source: &JsString,
    owner: Option<&SharedSchedulingCharge>,
) -> Result<NativeValue, RuntimeError> {
    let mut storage = owner
        .map(|owner| owner.reserve_peer(source.byte_len()))
        .transpose()?;
    let text = source.clone();
    if let Some(storage) = &mut storage {
        storage.resize(text.owned_heap_payload_bytes().unwrap_or(usize::MAX))?;
    }
    Ok(NativeValue::from_retained_parts(
        Value::String(text),
        storage,
    ))
}

pub(in crate::vm) fn text(value: &NativeValue) -> &JsString {
    let Value::String(text) = value.value() else {
        unreachable!("converted drag string")
    };
    text
}

pub(in crate::vm) fn lowercase(value: NativeValue) -> NativeValue {
    let (value, storage) = value.into_retained_parts();
    let Value::String(mut value) = value else {
        unreachable!("converted drag string")
    };
    value.make_ascii_lowercase();
    NativeValue::from_retained_parts(Value::String(value), storage)
}

fn validate_incoming(
    storage: &Option<SharedSchedulingCharge>,
    incoming: &Option<SharedSchedulingCharge>,
) -> Result<(), RuntimeError> {
    if let Some(storage) = storage {
        storage.reserve_peer(0)?;
        let incoming = incoming.as_ref().expect("same drag input account");
        incoming.reserve_peer(0)?;
        if !storage.same_owner(incoming) {
            return Err(storage.fail());
        }
    } else {
        assert!(incoming.is_none());
    }
    Ok(())
}

pub(in crate::vm) struct NativeDragStore {
    pub state: DragStore,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeDragStore {
    pub(in crate::vm) fn new(
        mode: DragStoreMode,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let drop_effect = utf16_from_utf8("none", owner)?;
        let effect_allowed = utf16_from_utf8("none", owner)?;
        let (Value::String(drop_effect), mut storage) = drop_effect.into_retained_parts() else {
            unreachable!()
        };
        let (Value::String(effect_allowed), incoming) = effect_allowed.into_retained_parts() else {
            unreachable!()
        };
        if let Some(storage) = &mut storage {
            storage.absorb(incoming.unwrap())?;
        }
        Ok(Self {
            state: DragStore {
                mode,
                items: Vec::new(),
                next_id: 0,
                item_list: None,
                file_list: None,
                types: None,
                drop_effect,
                effect_allowed,
                image: None,
            },
            storage,
        })
    }
    pub(in crate::vm) fn copied(
        vm: &Vm,
        id: ObjectId,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let source = vm.heap.drag_store(id)?;
        let mut storage = owner
            .map(|owner| owner.reserve_peer(source.copied_buffer_bytes()))
            .transpose()?;
        let state = source.clone();
        if let Some(storage) = &mut storage {
            storage.resize(state.owned_buffer_bytes())?;
        }
        Ok(Self { state, storage })
    }
    pub(in crate::vm) fn effect(
        &mut self,
        allowed: bool,
        value: NativeValue,
    ) -> Result<(), RuntimeError> {
        value.validate_owner()?;
        let (Value::String(value), incoming) = value.into_retained_parts() else {
            unreachable!()
        };
        validate_incoming(&self.storage, &incoming)?;
        let slot = if allowed {
            &mut self.state.effect_allowed
        } else {
            &mut self.state.drop_effect
        };
        let previous = slot.owned_heap_payload_bytes().unwrap_or(usize::MAX);
        *slot = value;
        if let Some(storage) = &mut self.storage {
            storage.release_to(storage.bytes() - previous);
            storage.absorb(incoming.unwrap())?;
        }
        Ok(())
    }
    pub(in crate::vm) fn item(
        &mut self,
        id: u64,
        format: NativeValue,
        data: NativeValue,
        wrapper: ObjectId,
    ) -> Result<(), RuntimeError> {
        format.validate_owner()?;
        data.validate_owner()?;
        let (Value::String(format), format_storage) = format.into_retained_parts() else {
            unreachable!()
        };
        let (data, data_storage) = data.into_retained_parts();
        validate_incoming(&self.storage, &format_storage)?;
        validate_incoming(&self.storage, &data_storage)?;
        let data = match data {
            Value::String(text) => DragItemData::Text(text),
            Value::Object(file) => DragItemData::File(file),
            Value::Undefined => DragItemData::FileMarker,
            _ => unreachable!("prepared drag item"),
        };
        push_slot(
            &mut self.state.items,
            &mut self.storage,
            DragItem {
                id,
                format,
                data,
                wrapper: Some(wrapper),
            },
        )?;
        if let Some(storage) = &mut self.storage {
            storage.absorb(format_storage.unwrap())?;
            storage.absorb(data_storage.unwrap())?;
        }
        self.state.next_id = id;
        self.state.types = None;
        Ok(())
    }
    pub(in crate::vm) fn reconcile_removed(&mut self) {
        if let Some(storage) = &mut self.storage {
            storage.release_to(self.state.owned_buffer_bytes());
        }
    }
    pub(in crate::vm) fn into_heap(
        mut self,
        vm: &mut Vm,
        id: ObjectId,
    ) -> Result<(), RuntimeError> {
        if let Some(storage) = &mut self.storage {
            storage.reserve_peer(0)?;
            storage.release_to(self.state.owned_buffer_bytes());
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.replace_drag_store(id, self.state)
        })
    }
    pub(in crate::vm) fn allocate(
        mut self,
        vm: &mut Vm,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.alloc_drag_store(self.state, prototype)
        })
    }
}

pub(in crate::vm) struct NativeDragFile {
    pub data: WebBlobData,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeDragFile {
    /// Construction transfers the already paid final byte and UTF16 buffers
    /// without cloning them before the managed Blob allocation.
    pub(in crate::vm) fn from_blob_parts(
        bytes: Vec<u8>,
        mut storage: Option<SharedSchedulingCharge>,
        media_type: NativeValue,
        name: Option<(NativeValue, i64)>,
    ) -> Result<Self, RuntimeError> {
        media_type.validate_owner()?;
        let (Value::String(media_type), media_storage) = media_type.into_retained_parts() else {
            unreachable!()
        };
        validate_incoming(&storage, &media_storage)?;
        if let Some(storage) = &mut storage {
            storage.absorb(media_storage.unwrap())?;
        }
        let file = if let Some((name, time)) = name {
            name.validate_owner()?;
            let (Value::String(name), name_storage) = name.into_retained_parts() else {
                unreachable!()
            };
            validate_incoming(&storage, &name_storage)?;
            if let Some(storage) = &mut storage {
                storage.absorb(name_storage.unwrap())?;
            }
            Some((name, time))
        } else {
            None
        };
        Ok(Self {
            data: WebBlobData {
                bytes,
                media_type,
                file,
            },
            storage,
        })
    }

    /// The provider's byte allocation moves unchanged; UTF8 source metadata
    /// stays paid until the independently admitted UTF16 copies replace it.
    pub(in crate::vm) fn from_owned_host(
        file: HostFileData,
        mut storage: Option<SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        if let Some(storage) = &storage {
            storage.reserve_peer(0)?;
        }
        let media_type = lowercase(utf16_from_utf8(&file.media_type, storage.as_ref())?);
        let name = utf16_from_utf8(&file.name, storage.as_ref())?;
        let old_metadata = file
            .name
            .capacity()
            .saturating_add(file.media_type.capacity());
        let HostFileData {
            name: old_name,
            media_type: old_type,
            bytes,
            last_modified,
        } = file;
        drop(old_name);
        drop(old_type);
        let (Value::String(media_type), media_storage) = media_type.into_retained_parts() else {
            unreachable!()
        };
        let (Value::String(name), name_storage) = name.into_retained_parts() else {
            unreachable!()
        };
        if let Some(storage) = &mut storage {
            storage.release_to(storage.bytes() - old_metadata);
            storage.absorb(media_storage.unwrap())?;
            storage.absorb(name_storage.unwrap())?;
        }
        Ok(Self {
            data: WebBlobData {
                bytes,
                media_type,
                file: Some((name, last_modified)),
            },
            storage,
        })
    }

    pub(in crate::vm) fn from_host(
        file: &HostFileData,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut storage = owner
            .map(|owner| owner.reserve_peer(file.bytes.len()))
            .transpose()?;
        let bytes = file.bytes.clone();
        if let Some(storage) = &mut storage {
            storage.resize(bytes.capacity())?;
        }
        let media_type = lowercase(utf16_from_utf8(&file.media_type, owner)?);
        let name = utf16_from_utf8(&file.name, owner)?;
        let (Value::String(media_type), media_storage) = media_type.into_retained_parts() else {
            unreachable!()
        };
        let (Value::String(name), name_storage) = name.into_retained_parts() else {
            unreachable!()
        };
        if let Some(storage) = &mut storage {
            storage.absorb(media_storage.unwrap())?;
            storage.absorb(name_storage.unwrap())?;
        }
        Ok(Self {
            data: WebBlobData {
                bytes,
                media_type,
                file: Some((name, file.last_modified)),
            },
            storage,
        })
    }
    pub(in crate::vm) fn copied(
        vm: &Vm,
        file: ObjectId,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let source = vm.heap.web_blob(file)?;
        let mut storage = owner
            .map(|owner| owner.reserve_peer(source.copied_buffer_bytes()))
            .transpose()?;
        let data = source.clone();
        if let Some(storage) = &mut storage {
            storage.resize(data.owned_buffer_bytes())?;
        }
        Ok(Self { data, storage })
    }
    pub(in crate::vm) fn allocate(
        mut self,
        vm: &mut Vm,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.alloc_web_blob(self.data, prototype)
        })
    }
}
