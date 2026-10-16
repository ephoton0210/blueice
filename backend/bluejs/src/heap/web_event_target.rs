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
    pub fn references(&self) -> Vec<ObjectId> {
        let mut ids = Vec::new();
        self.visit_references(&mut |id| ids.push(id));
        ids
    }
    pub fn visit_references(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.listeners
            .iter()
            .flat_map(|listener| std::iter::once(listener.callback).chain(listener.signal))
            .for_each(&mut *visit)
    }

    pub fn bytes(&self) -> usize {
        self.listeners
            .iter()
            .fold(size_of::<Self>(), |bytes, listener| {
                bytes
                    .saturating_add(size_of::<WebListener>())
                    .saturating_add(listener.name.byte_len())
            })
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
        match &mut entry.kind {
            ObjectKind::WebEventTarget(target) => **target = state,
            ObjectKind::WebFileReader(reader) => reader.target = state,
            ObjectKind::WebAbortSignal(signal) => signal.target = state,
            _ => unreachable!("validated target"),
        }
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }
}
