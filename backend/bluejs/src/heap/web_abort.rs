// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Abort data is collector-owned. Dependency graph edges are weak; active
//! dependent signals are traced conditionally from independently live sources.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AbortHook {
    Web { target: ObjectId, serial: u64 },
    Host { serial: u64 },
}
#[derive(Clone)]
pub(crate) struct AbortData {
    pub target: WebTargetData,
    pub reason: Value,
    pub dependent: bool,
    pub sources: Vec<ObjectId>,
    pub dependents: Vec<ObjectId>,
    pub hooks: Vec<AbortHook>,
}
impl Default for AbortData {
    fn default() -> Self {
        Self {
            target: WebTargetData::default(),
            reason: Value::Undefined,
            dependent: false,
            sources: Vec::new(),
            dependents: Vec::new(),
            hooks: Vec::new(),
        }
    }
}
impl AbortData {
    pub fn aborted(&self) -> bool {
        self.reason != Value::Undefined
    }
    pub fn observed(&self) -> bool {
        !self.hooks.is_empty()
            || self
                .target
                .listeners
                .iter()
                .any(|listener| listener.name == "abort")
    }
    pub fn references(&self) -> Vec<ObjectId> {
        self.target
            .references()
            .into_iter()
            .chain(self.reason.object_id())
            .chain(self.hooks.iter().filter_map(|hook| match hook {
                AbortHook::Web { target, .. } => Some(*target),
                AbortHook::Host { .. } => None,
            }))
            .collect()
    }
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + self.target.bytes()
            + match &self.reason {
                Value::String(value) => value.byte_len(),
                _ => 0,
            }
            + (self.sources.len() + self.dependents.len()) * size_of::<ObjectId>()
            + self.hooks.len() * size_of::<AbortHook>()
    }
}
impl Heap {
    pub(crate) fn alloc_abort_signal(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebAbortSignal(Box::default()), Some(prototype))
    }
    pub(crate) fn alloc_abort_controller(
        &mut self,
        signal: ObjectId,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.abort_signal(signal)?;
        self.alloc(ObjectKind::WebAbortController(signal), Some(prototype))
    }
    pub(crate) fn abort_controller(&self, object: ObjectId) -> Result<ObjectId, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebAbortController(signal) => Ok(*signal),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    pub(crate) fn abort_signal(&self, object: ObjectId) -> Result<&AbortData, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebAbortSignal(state) => Ok(state),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    pub(crate) fn set_abort_signal(
        &mut self,
        object: ObjectId,
        state: AbortData,
    ) -> Result<(), HeapError> {
        let previous = self.abort_signal(object)?.bytes();
        let bytes = state.bytes();
        let references = state.references();
        for reference in &references {
            self.object(*reference)?;
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(bytes.saturating_sub(previous), &protected)?;
        let entry = self.objects.get_mut(&object).expect("protected signal");
        entry.kind = ObjectKind::WebAbortSignal(Box::new(state));
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes = self.managed_bytes - previous + bytes;
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }
    pub(super) fn clean_abort_weak_edges(&mut self) {
        let alive: HashSet<_> = self.objects.keys().copied().collect();
        let mut released = 0;
        for entry in self.objects.values_mut() {
            if let ObjectKind::WebAbortSignal(state) = &mut entry.kind {
                let previous = state.bytes();
                state.sources.retain(|source| alive.contains(source));
                state
                    .dependents
                    .retain(|dependent| alive.contains(dependent));
                let removed = previous - state.bytes();
                entry.bytes -= removed;
                released += removed;
            }
        }
        self.managed_bytes -= released;
    }
}
