// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Collector-owned drag stores. Wrappers retain their store, never an OS path
//! or an embedding handle. Removed items are disabled by their stable ID.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragStoreMode {
    ReadWrite,
    ReadOnly,
    Protected,
    Disabled,
}

#[derive(Clone)]
pub(crate) enum DragItemData {
    Text(JsString),
    File(ObjectId),
    FileMarker,
}

#[derive(Clone)]
pub(crate) struct DragItem {
    pub id: u64,
    pub format: JsString,
    pub data: DragItemData,
    pub wrapper: Option<ObjectId>,
}

#[derive(Clone)]
pub(crate) struct DragStore {
    pub mode: DragStoreMode,
    pub items: Vec<DragItem>,
    pub next_id: u64,
    pub item_list: Option<ObjectId>,
    pub file_list: Option<ObjectId>,
    pub types: Option<ObjectId>,
    pub drop_effect: JsString,
    pub effect_allowed: JsString,
    pub image: Option<(ObjectId, i32, i32)>,
}

impl DragStore {
    pub fn new(mode: DragStoreMode) -> Self {
        Self {
            mode,
            items: Vec::new(),
            next_id: 0,
            item_list: None,
            file_list: None,
            types: None,
            drop_effect: "none".into(),
            effect_allowed: "none".into(),
            image: None,
        }
    }

    pub fn references(&self) -> Vec<ObjectId> {
        self.items
            .iter()
            .flat_map(|item| {
                item.wrapper.into_iter().chain(match &item.data {
                    DragItemData::File(file) => Some(*file),
                    DragItemData::Text(_) | DragItemData::FileMarker => None,
                })
            })
            .chain(self.item_list)
            .chain(self.file_list)
            .chain(self.types)
            .chain(self.image.map(|(element, _, _)| element))
            .collect()
    }

    pub fn bytes(&self) -> usize {
        self.items.iter().fold(
            size_of::<Self>() + self.drop_effect.byte_len() + self.effect_allowed.byte_len(),
            |bytes, item| {
                bytes
                    .saturating_add(size_of::<DragItem>())
                    .saturating_add(item.format.byte_len())
                    .saturating_add(match &item.data {
                        DragItemData::Text(text) => text.byte_len(),
                        DragItemData::File(_) | DragItemData::FileMarker => 0,
                    })
            },
        )
    }
}

#[derive(Clone)]
pub(crate) struct DragEventState {
    /// Event, UIEvent, MouseEvent and DragEvent share branded internal slots.
    pub interface: u8,
    pub attributes: Vec<(&'static str, Value)>,
    pub path: Vec<ObjectId>,
    pub phase: u8,
    pub passive: bool,
    pub transfer: Option<ObjectId>,
    pub current_target: Option<ObjectId>,
    pub cancelable: bool,
    pub default_prevented: bool,
    pub propagation_stopped: bool,
    pub immediate_stopped: bool,
}

impl DragEventState {
    pub fn new(interface: u8) -> Self {
        Self {
            interface,
            attributes: Vec::new(),
            path: Vec::new(),
            phase: 0,
            passive: false,
            transfer: None,
            current_target: None,
            cancelable: false,
            default_prevented: false,
            propagation_stopped: false,
            immediate_stopped: false,
        }
    }

    pub fn get(&self, name: &str) -> Value {
        self.attributes
            .iter()
            .find(|(key, _)| *key == name)
            .map_or(Value::Undefined, |(_, value)| value.clone())
    }

    pub fn set(&mut self, name: &'static str, value: Value) {
        if let Some((_, existing)) = self.attributes.iter_mut().find(|(key, _)| *key == name) {
            *existing = value;
        } else {
            self.attributes.push((name, value));
        }
    }

    pub fn bytes(&self) -> usize {
        self.attributes.iter().fold(
            size_of::<Self>().saturating_add(self.path.len().saturating_mul(size_of::<ObjectId>())),
            |bytes, (_, value)| {
                bytes
                    .saturating_add(size_of::<(&str, Value)>())
                    .saturating_add(value.payload_bytes())
            },
        )
    }

    pub fn references(&self) -> Vec<ObjectId> {
        self.transfer
            .into_iter()
            .chain(self.current_target)
            .chain(self.path.iter().copied())
            .chain(
                self.attributes
                    .iter()
                    .filter_map(|(_, value)| value.object_id()),
            )
            .collect()
    }
}

impl Heap {
    pub(crate) fn alloc_dom_exception(
        &mut self,
        name: JsString,
        message: JsString,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::WebDOMException { name, message },
            Some(prototype),
        )
    }

    pub(crate) fn dom_exception(
        &self,
        object: ObjectId,
    ) -> Result<(&JsString, &JsString), HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebDOMException { name, message } => Ok((name, message)),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }

    pub(crate) fn alloc_drag_store(
        &mut self,
        store: DragStore,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::WebDataTransfer(Box::new(store)),
            Some(prototype),
        )
    }

    pub(crate) fn drag_store(&self, object: ObjectId) -> Result<&DragStore, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebDataTransfer(store) => Ok(store),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }

    pub(crate) fn replace_drag_store(
        &mut self,
        object: ObjectId,
        store: DragStore,
    ) -> Result<(), HeapError> {
        let previous = self.drag_store(object)?.bytes();
        let bytes = store.bytes();
        let references = store.references();
        for reference in &references {
            self.object(*reference)?;
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(bytes.saturating_sub(previous), &protected)?;
        let entry = self
            .objects
            .get_mut(&object)
            .expect("protected across collection");
        entry.kind = ObjectKind::WebDataTransfer(Box::new(store));
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        // Live indices must never reuse an ordinary property cache entry.
        self.structure_epoch = self.structure_epoch.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn alloc_drag_items(
        &mut self,
        store: ObjectId,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.drag_store(store)?;
        self.alloc(ObjectKind::WebDragItems(store), Some(prototype))
    }

    pub(crate) fn alloc_drag_files(
        &mut self,
        store: ObjectId,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.drag_store(store)?;
        self.alloc(ObjectKind::WebDragFiles(store), Some(prototype))
    }

    pub(crate) fn alloc_drag_item(
        &mut self,
        store: ObjectId,
        id: u64,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.drag_store(store)?;
        self.alloc(ObjectKind::WebDragItem { store, id }, Some(prototype))
    }

    pub(crate) fn drag_items_store(&self, object: ObjectId) -> Result<ObjectId, HeapError> {
        match self.object(object)?.kind {
            ObjectKind::WebDragItems(store) => Ok(store),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }

    pub(crate) fn drag_item_identity(
        &self,
        object: ObjectId,
    ) -> Result<(ObjectId, u64), HeapError> {
        match self.object(object)?.kind {
            ObjectKind::WebDragItem { store, id } => Ok((store, id)),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }

    pub(crate) fn drag_files(&self, store: ObjectId) -> Result<Vec<ObjectId>, HeapError> {
        let store = self.drag_store(store)?;
        if matches!(
            store.mode,
            DragStoreMode::Protected | DragStoreMode::Disabled
        ) {
            return Ok(Vec::new());
        }
        Ok(store
            .items
            .iter()
            .filter_map(|item| match item.data {
                DragItemData::File(file) => Some(file),
                DragItemData::Text(_) | DragItemData::FileMarker => None,
            })
            .collect())
    }

    pub(super) fn drag_index_value(
        &self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<Option<Value>>, HeapError> {
        let Some(index) = key.index() else {
            return Ok(None);
        };
        match self.object(object)?.kind {
            ObjectKind::WebDragItems(store) => {
                let store = self.drag_store(store)?;
                Ok(Some(if store.mode == DragStoreMode::Disabled {
                    None
                } else {
                    store
                        .items
                        .get(index)
                        .and_then(|item| item.wrapper)
                        .map(Value::Object)
                }))
            }
            ObjectKind::WebDragFiles(store) => Ok(Some(
                self.drag_files(store)?
                    .get(index)
                    .copied()
                    .map(Value::Object),
            )),
            _ => Ok(None),
        }
    }

    pub(crate) fn drag_index_count(&self, object: ObjectId) -> Result<Option<usize>, HeapError> {
        match self.object(object)?.kind {
            ObjectKind::WebDragItems(store) => {
                let store = self.drag_store(store)?;
                Ok(Some(if store.mode == DragStoreMode::Disabled {
                    0
                } else {
                    store.items.len()
                }))
            }
            ObjectKind::WebDragFiles(store) => Ok(Some(self.drag_files(store)?.len())),
            _ => Ok(None),
        }
    }

    pub(crate) fn alloc_drag_event(
        &mut self,
        state: DragEventState,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebDragEvent(Box::new(state)), Some(prototype))
    }

    pub(crate) fn drag_event(&self, object: ObjectId) -> Result<&DragEventState, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebDragEvent(state) => Ok(state),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }

    pub(crate) fn set_drag_event_state(
        &mut self,
        object: ObjectId,
        state: DragEventState,
    ) -> Result<(), HeapError> {
        let previous = self.drag_event(object)?.bytes();
        let bytes = state.bytes();
        let references = state.references();
        for reference in &references {
            self.object(*reference)?;
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(bytes.saturating_sub(previous), &protected)?;
        let entry = self
            .objects
            .get_mut(&object)
            .expect("protected across collection");
        entry.kind = ObjectKind::WebDragEvent(Box::new(state));
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }
}
