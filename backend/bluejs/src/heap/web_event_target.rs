// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Collector-owned listener identities and callback lifetimes.

use super::*;

#[derive(Clone)]
pub(crate) struct WebListener {
    pub serial: u64,
    pub name: JsString,
    pub callback: ObjectId,
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
    pub handler: bool,
    pub signal: Option<ObjectId>,
}

#[derive(Clone, Default)]
pub(crate) struct WebTargetData {
    pub listeners: Vec<WebListener>,
    pub next_serial: u64,
}

impl WebTargetData {
    pub fn visit_references(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.listeners
            .iter()
            .flat_map(|listener| std::iter::once(listener.callback).chain(listener.signal))
            .for_each(&mut *visit)
    }

    pub fn bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.owned_buffer_bytes())
    }

    pub(crate) fn owned_buffer_bytes(&self) -> usize {
        self.listeners.iter().fold(
            self.listeners.capacity() * size_of::<WebListener>(),
            |bytes, listener| {
                bytes.saturating_add(
                    listener
                        .name
                        .owned_heap_payload_bytes()
                        .unwrap_or(usize::MAX),
                )
            },
        )
    }

    pub(crate) fn copied_buffer_bytes(&self) -> usize {
        self.listeners.iter().fold(
            self.listeners.len() * size_of::<WebListener>(),
            |bytes, listener| bytes.saturating_add(listener.name.byte_len()),
        )
    }
}

impl Heap {
    /// Give the existing realm global collector-owned listeners without
    /// replacing its identity, own properties or prototype storage.
    pub(crate) fn initialize_global_event_target(
        &mut self,
        object: ObjectId,
    ) -> Result<(), HeapError> {
        if !matches!(self.object(object)?.kind, ObjectKind::Ordinary) {
            return Err(HeapError::InvalidInternalSlot(object));
        }
        let state = Box::<WebTargetData>::default();
        let bytes = state.bytes();
        self.ensure_room(bytes, &[object])?;
        let entry = self.objects.get_mut(&object).expect("protected global");
        entry.kind = ObjectKind::WebEventTarget(state);
        entry.bytes += bytes;
        self.managed_bytes.add(bytes);
        Ok(())
    }

    pub(crate) fn alloc_web_event_target(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebEventTarget(Box::default()), Some(prototype))
    }

    pub(crate) fn web_event_target(&self, object: ObjectId) -> Result<&WebTargetData, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebEventTarget(state) => Ok(state),
            ObjectKind::WebFileReader(state) => Ok(&state.target),
            ObjectKind::WebAbortSignal(state) => Ok(&state.target),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }

    pub(crate) fn set_web_event_target(
        &mut self,
        object: ObjectId,
        state: WebTargetData,
    ) -> Result<(), HeapError> {
        let previous = self.web_event_target(object)?.bytes();
        let bytes = state.bytes();
        let mut valid = Ok(());
        state.visit_references(&mut |id| {
            valid = valid.and_then(|()| self.object(id).map(|_| ()));
        });
        valid?;
        self.ensure_room_protected(
            bytes.saturating_sub(previous),
            ProtectedRoots::WebTarget(object, &state),
        )?;
        // This borrowed candidate remains live through collection. Record its
        // barriers before the pure move, without an extra reference snapshot.
        state.visit_references(&mut |id| self.write_barrier(object, Some(id)));
        let entry = self
            .objects
            .get_mut(&object)
            .expect("protected across collection");
        match &mut entry.kind {
            ObjectKind::WebEventTarget(target) => **target = state,
            ObjectKind::WebFileReader(reader) => reader.target = state,
            ObjectKind::WebAbortSignal(signal) => signal.target = state,
            _ => unreachable!("validated target"),
        }
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        Ok(())
    }

    /// Once-listener retirement allocates nothing and preserves paid slots.
    pub(crate) fn remove_web_listener(
        &mut self,
        object: ObjectId,
        serial: u64,
    ) -> Result<(), HeapError> {
        let previous = self.web_event_target(object)?.bytes();
        let entry = self.objects.get_mut(&object).expect("validated target");
        let state = match &mut entry.kind {
            ObjectKind::WebEventTarget(state) => state.as_mut(),
            ObjectKind::WebFileReader(state) => &mut state.target,
            ObjectKind::WebAbortSignal(state) => &mut state.target,
            _ => unreachable!("validated target"),
        };
        state.listeners.retain(|listener| listener.serial != serial);
        let bytes = state.bytes();
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes.sub(previous - bytes);
        Ok(())
    }

    /// No guest call separates a new registration from its final root refresh.
    /// A refused publication can therefore retire that exact serial in place.
    pub(crate) fn rollback_web_listener(
        &mut self,
        object: ObjectId,
        serial: u64,
    ) -> Result<(), HeapError> {
        self.remove_web_listener(object, serial)?;
        let entry = self.objects.get_mut(&object).expect("validated target");
        let state = match &mut entry.kind {
            ObjectKind::WebEventTarget(state) => state.as_mut(),
            ObjectKind::WebFileReader(state) => &mut state.target,
            ObjectKind::WebAbortSignal(state) => &mut state.target,
            _ => unreachable!("validated target"),
        };
        if state.next_serial == serial {
            state.next_serial -= 1;
        }
        Ok(())
    }
}
