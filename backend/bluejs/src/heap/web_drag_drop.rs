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
    #[cfg(any(test, coverage))]
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

    pub(super) fn file_ids(&self) -> impl Iterator<Item = ObjectId> + Clone + '_ {
        self.items.iter().filter_map(|item| {
            if matches!(
                self.mode,
                DragStoreMode::Protected | DragStoreMode::Disabled
            ) {
                return None;
            }
            match item.data {
                DragItemData::File(file) => Some(file),
                _ => None,
            }
        })
    }
    pub fn visit_references(&self, visit: &mut dyn FnMut(ObjectId)) {
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
            .for_each(&mut *visit)
    }

    pub fn bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.owned_buffer_bytes())
    }
    pub(crate) fn owned_buffer_bytes(&self) -> usize {
        self.items.iter().fold(
            self.items
                .capacity()
                .saturating_mul(size_of::<DragItem>())
                .saturating_add(
                    self.drop_effect
                        .owned_heap_payload_bytes()
                        .unwrap_or(usize::MAX),
                )
                .saturating_add(
                    self.effect_allowed
                        .owned_heap_payload_bytes()
                        .unwrap_or(usize::MAX),
                ),
            |bytes, item| {
                bytes
                    .saturating_add(item.format.owned_heap_payload_bytes().unwrap_or(usize::MAX))
                    .saturating_add(match &item.data {
                        DragItemData::Text(text) => {
                            text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
                        }
                        _ => 0,
                    })
            },
        )
    }
    pub(crate) fn copied_buffer_bytes(&self) -> usize {
        self.items.iter().fold(
            self.items
                .len()
                .saturating_mul(size_of::<DragItem>())
                .saturating_add(self.drop_effect.byte_len())
                .saturating_add(self.effect_allowed.byte_len()),
            |bytes, item| {
                bytes
                    .saturating_add(item.format.byte_len())
                    .saturating_add(match &item.data {
                        DragItemData::Text(text) => text.byte_len(),
                        _ => 0,
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
        self.get_ref(name).cloned().unwrap_or(Value::Undefined)
    }

    pub(crate) fn get_ref(&self, name: &str) -> Option<&Value> {
        self.attributes
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    pub fn set(&mut self, name: &'static str, value: Value) {
        if let Some((_, existing)) = self.attributes.iter_mut().find(|(key, _)| *key == name) {
            *existing = value;
        } else {
            self.attributes.push((name, value));
        }
    }

    pub fn bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.owned_buffer_bytes())
    }

    pub(crate) fn owned_buffer_bytes(&self) -> usize {
        self.attributes.iter().fold(
            self.path
                .capacity()
                .saturating_mul(size_of::<ObjectId>())
                .saturating_add(
                    self.attributes
                        .capacity()
                        .saturating_mul(size_of::<(&str, Value)>()),
                ),
            |bytes, (_, value)| bytes.saturating_add(HeapNativeBudget::value_payload_bytes(value)),
        )
    }

    pub(crate) fn copied_buffer_bytes(&self) -> usize {
        self.attributes.iter().fold(
            self.path
                .len()
                .saturating_mul(size_of::<ObjectId>())
                .saturating_add(
                    self.attributes
                        .len()
                        .saturating_mul(size_of::<(&str, Value)>()),
                ),
            |bytes, (_, value)| {
                bytes.saturating_add(match value {
                    Value::String(value) => value.byte_len(),
                    Value::Symbol(value) => {
                        value.description.as_ref().map_or(0, JsString::byte_len)
                    }
                    value => HeapNativeBudget::value_payload_bytes(value),
                })
            },
        )
    }

    pub fn visit_references(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.transfer
            .into_iter()
            .chain(self.current_target)
            .chain(self.path.iter().copied())
            .chain(
                self.attributes
                    .iter()
                    .filter_map(|(_, value)| value.object_id()),
            )
            .for_each(&mut *visit)
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
        let mut valid = Ok(());
        store.visit_references(&mut |id| valid = valid.and_then(|()| self.object(id).map(|_| ())));
        valid?;
        self.ensure_room_protected(
            bytes.saturating_sub(previous),
            ProtectedRoots::DragStore(object, &store),
        )?;
        store.visit_references(&mut |id| self.write_barrier(object, Some(id)));
        let entry = self
            .objects
            .get_mut(&object)
            .expect("protected across collection");
        let ObjectKind::WebDataTransfer(current) = &mut entry.kind else {
            unreachable!("validated drag store")
        };
        **current = store;
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
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
                self.drag_store(store)?
                    .file_ids()
                    .nth(index)
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
            ObjectKind::WebDragFiles(store) => Ok(Some(self.drag_store(store)?.file_ids().count())),
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

    pub(crate) fn begin_event_dispatch(
        &mut self,
        object: ObjectId,
        target: ObjectId,
        trusted: bool,
        path: Vec<ObjectId>,
    ) -> Result<(), HeapError> {
        // Preserve the standalone entry's exact single-target contract.
        if path.as_slice() != [target] {
            return Err(HeapError::InvalidInternalSlot(object));
        }
        self.begin_node_event_dispatch(object, target, trusted, path)
    }

    pub(crate) fn begin_node_event_dispatch(
        &mut self,
        object: ObjectId,
        target: ObjectId,
        trusted: bool,
        path: Vec<ObjectId>,
    ) -> Result<(), HeapError> {
        if path.first() != Some(&target) {
            return Err(HeapError::InvalidInternalSlot(object));
        }
        for id in &path {
            self.object(*id)?;
        }
        let state = self.drag_event(object)?;
        let target_slot = state
            .attributes
            .iter()
            .position(|(name, _)| *name == "target")
            .ok_or(HeapError::InvalidInternalSlot(object))?;
        let trusted_slot = state
            .attributes
            .iter()
            .position(|(name, _)| *name == "isTrusted")
            .ok_or(HeapError::InvalidInternalSlot(object))?;
        let previous = state.bytes();
        let bytes = previous
            .saturating_sub(state.path.capacity().saturating_mul(size_of::<ObjectId>()))
            .saturating_sub(HeapNativeBudget::value_payload_bytes(
                &state.attributes[target_slot].1,
            ))
            .saturating_sub(HeapNativeBudget::value_payload_bytes(
                &state.attributes[trusted_slot].1,
            ))
            .saturating_add(path.capacity().saturating_mul(size_of::<ObjectId>()));
        self.ensure_room_protected(
            bytes.saturating_sub(previous),
            ProtectedRoots::EventPath(object, &path),
        )?;
        for id in &path {
            self.write_barrier(object, Some(*id));
        }
        let entry = self.objects.get_mut(&object).expect("protected event");
        let ObjectKind::WebDragEvent(state) = &mut entry.kind else {
            unreachable!("validated event")
        };
        state.attributes[target_slot].1 = Value::Object(target);
        state.attributes[trusted_slot].1 = Value::Bool(trusted);
        state.current_target = Some(target);
        state.phase = 2;
        state.path = path;
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        self.write_barrier(object, Some(target));
        Ok(())
    }

    pub(crate) fn set_event_phase(
        &mut self,
        object: ObjectId,
        current: ObjectId,
        phase: u8,
    ) -> Result<(), HeapError> {
        if !(1..=3).contains(&phase) || !self.drag_event(object)?.path.contains(&current) {
            return Err(HeapError::InvalidInternalSlot(object));
        }
        self.object(current)?;
        let state = self.drag_event_mut(object)?;
        state.phase = phase;
        state.current_target = Some(current);
        self.write_barrier(object, Some(current));
        Ok(())
    }

    fn drag_event_mut(&mut self, object: ObjectId) -> Result<&mut DragEventState, HeapError> {
        self.drag_event(object)?;
        match &mut self.objects.get_mut(&object).expect("validated event").kind {
            ObjectKind::WebDragEvent(state) => Ok(state),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }

    pub(crate) fn set_event_passive(
        &mut self,
        object: ObjectId,
        passive: bool,
    ) -> Result<(), HeapError> {
        self.drag_event_mut(object)?.passive = passive;
        Ok(())
    }

    pub(crate) fn prevent_event_default(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let state = self.drag_event_mut(object)?;
        if state.cancelable && !state.passive {
            state.default_prevented = true;
        }
        Ok(())
    }

    /// Terminal cleanup adds no edge or allocation. The retained path keeps
    /// its managed capacity charge after its transient references are cleared.
    pub(crate) fn finish_event_dispatch(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let state = self.drag_event_mut(object)?;
        state.phase = 0;
        state.current_target = None;
        state.path.clear();
        state.passive = false;
        state.propagation_stopped = false;
        state.immediate_stopped = false;
        Ok(())
    }

    pub(crate) fn stop_event_propagation(
        &mut self,
        object: ObjectId,
        immediate: bool,
    ) -> Result<(), HeapError> {
        let state = self.drag_event_mut(object)?;
        state.propagation_stopped = true;
        state.immediate_stopped |= immediate;
        Ok(())
    }

    pub(crate) fn set_drag_event_state(
        &mut self,
        object: ObjectId,
        state: DragEventState,
    ) -> Result<(), HeapError> {
        let previous = self.drag_event(object)?.bytes();
        let bytes = state.bytes();
        let mut valid = Ok(());
        state.visit_references(&mut |id| valid = valid.and_then(|()| self.object(id).map(|_| ())));
        valid?;
        self.ensure_room_protected(
            bytes.saturating_sub(previous),
            ProtectedRoots::Event(object, &state),
        )?;
        state.visit_references(&mut |id| self.write_barrier(object, Some(id)));
        let entry = self
            .objects
            .get_mut(&object)
            .expect("protected across collection");
        entry.kind = ObjectKind::WebDragEvent(Box::new(state));
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        Ok(())
    }

    /// Terminal retirement creates no reference or allocation. Retained Vec
    /// slots remain paid, while payloads, type cache and image roots are dropped.
    pub(crate) fn disable_drag_store(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let previous = self.drag_store(object)?.bytes();
        let entry = self.objects.get_mut(&object).expect("validated drag store");
        let ObjectKind::WebDataTransfer(store) = &mut entry.kind else {
            unreachable!("validated drag store")
        };
        store.mode = DragStoreMode::Disabled;
        store.items.clear();
        store.types = None;
        store.image = None;
        let bytes = store.bytes();
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes.sub(previous - bytes);
        self.structure_epoch = self.structure_epoch.wrapping_add(1);
        Ok(())
    }
}
