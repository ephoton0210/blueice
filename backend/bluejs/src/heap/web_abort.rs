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
    pub fn visit_references(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.target.visit_references(&mut *visit);
        self.reason
            .object_id()
            .into_iter()
            .chain(self.hooks.iter().filter_map(|hook| match hook {
                AbortHook::Web { target, .. } => Some(*target),
                AbortHook::Host { .. } => None,
            }))
            .for_each(&mut *visit)
    }
    pub fn bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.owned_buffer_bytes())
    }

    pub(crate) fn owned_buffer_bytes(&self) -> usize {
        self.target
            .owned_buffer_bytes()
            .saturating_add(HeapNativeBudget::value_payload_bytes(&self.reason))
            .saturating_add(
                (self.sources.capacity() + self.dependents.capacity()) * size_of::<ObjectId>(),
            )
            .saturating_add(self.hooks.capacity() * size_of::<AbortHook>())
    }

    pub(crate) fn copied_buffer_bytes(&self) -> usize {
        let reason = match &self.reason {
            Value::String(value) => value.byte_len(),
            Value::Symbol(value) => value.description.as_ref().map_or(0, JsString::byte_len),
            value => HeapNativeBudget::value_payload_bytes(value),
        };
        self.target
            .copied_buffer_bytes()
            .saturating_add(reason)
            .saturating_add((self.sources.len() + self.dependents.len()) * size_of::<ObjectId>())
            .saturating_add(self.hooks.len() * size_of::<AbortHook>())
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
        let mut valid = Ok(());
        state.visit_references(&mut |id| {
            valid = valid.and_then(|()| self.object(id).map(|_| ()));
        });
        valid?;
        self.ensure_room_protected(
            bytes.saturating_sub(previous),
            ProtectedRoots::AbortSignal(object, &state),
        )?;
        state.visit_references(&mut |id| self.write_barrier(object, Some(id)));
        let entry = self.objects.get_mut(&object).expect("protected signal");
        entry.kind = ObjectKind::WebAbortSignal(Box::new(state));
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        Ok(())
    }

    /// Removing Copy hooks creates no edge or allocation. Retained slots keep
    /// the same managed charge, including during terminal document cleanup.
    pub(crate) fn remove_abort_hook(
        &mut self,
        object: ObjectId,
        hook: AbortHook,
    ) -> Result<(), HeapError> {
        self.abort_signal(object)?;
        let entry = self.objects.get_mut(&object).expect("validated signal");
        let ObjectKind::WebAbortSignal(state) = &mut entry.kind else {
            unreachable!("validated signal")
        };
        state.hooks.retain(|candidate| *candidate != hook);
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
        self.managed_bytes.sub(released);
    }
}
