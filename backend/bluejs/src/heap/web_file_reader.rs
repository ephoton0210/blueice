// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! FileReader state and its retained Blob/result belong to the collector.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileReadKind {
    Text,
    BinaryString,
    ArrayBuffer,
    DataUrl,
}

#[derive(Clone)]
pub(crate) struct FileReaderState {
    pub target: WebTargetData,
    pub ready: u8,
    pub result: Value,
    pub error: Option<ObjectId>,
    pub epoch: u64,
    pub blob: Option<ObjectId>,
    pub kind: FileReadKind,
    pub encoding: Option<JsString>,
    pub loaded: usize,
    pub total: usize,
}

impl Default for FileReaderState {
    fn default() -> Self {
        Self {
            target: WebTargetData::default(),
            ready: 0,
            result: Value::Null,
            error: None,
            epoch: 0,
            blob: None,
            kind: FileReadKind::Text,
            encoding: None,
            loaded: 0,
            total: 0,
        }
    }
}

impl FileReaderState {
    pub fn visit_references(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.target.visit_references(&mut *visit);
        self.result
            .object_id()
            .into_iter()
            .chain(self.error)
            .chain(self.blob)
            .for_each(&mut *visit)
    }
    pub fn bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.owned_buffer_bytes())
    }
    pub(crate) fn owned_buffer_bytes(&self) -> usize {
        self.target
            .owned_buffer_bytes()
            .saturating_add(self.encoding.as_ref().map_or(0, |text| {
                text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
            }))
            .saturating_add(crate::vm::HeapNativeBudget::value_payload_bytes(
                &self.result,
            ))
    }
    pub(crate) fn copied_buffer_bytes(&self) -> usize {
        self.target
            .copied_buffer_bytes()
            .saturating_add(self.encoding.as_ref().map_or(0, JsString::byte_len))
            .saturating_add(match &self.result {
                Value::String(text) => text.byte_len(),
                value => crate::vm::HeapNativeBudget::value_payload_bytes(value),
            })
    }
}

pub(crate) struct PreparedFileReaderUpdate {
    object: ObjectId,
    previous: usize,
    bytes: usize,
    state: Box<FileReaderState>,
}

impl Heap {
    #[cfg(any(test, coverage))]
    pub(crate) fn alloc_file_reader(&mut self, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc_file_reader_owned(Box::default(), prototype)
    }
    pub(crate) fn alloc_file_reader_owned(
        &mut self,
        state: Box<FileReaderState>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebFileReader(state), Some(prototype))
    }
    pub(crate) fn file_reader(&self, object: ObjectId) -> Result<&FileReaderState, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebFileReader(state) => Ok(state),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn set_file_reader(
        &mut self,
        object: ObjectId,
        state: FileReaderState,
    ) -> Result<(), HeapError> {
        let prepared = self.prepare_file_reader_update(object, state)?;
        self.commit_file_reader_update(prepared);
        Ok(())
    }
    /// Admission may collect; the old reader remains installed. After this
    /// returns, commit must precede any further collection or managed allocation.
    #[cfg(any(test, coverage))]
    pub(crate) fn prepare_file_reader_update(
        &mut self,
        object: ObjectId,
        state: FileReaderState,
    ) -> Result<PreparedFileReaderUpdate, HeapError> {
        self.prepare_file_reader_update_owned(object, Box::new(state))
    }
    pub(crate) fn prepare_file_reader_update_owned(
        &mut self,
        object: ObjectId,
        state: Box<FileReaderState>,
    ) -> Result<PreparedFileReaderUpdate, HeapError> {
        let previous = self.file_reader(object)?.bytes();
        let bytes = state.bytes();
        let mut validation = Ok(());
        state.visit_references(&mut |id| {
            if validation.is_ok() {
                validation = self.object(id).map(|_| ());
            }
        });
        validation?;
        let kind = ObjectKind::WebFileReader(state);
        // Allocation protection visits the incoming kind and one extra object,
        // avoiding reference/protection scratch vectors for the receiver update.
        self.ensure_room_protected(
            bytes.saturating_sub(previous),
            ProtectedRoots::Allocation(&kind, Some(object)),
        )?;
        let ObjectKind::WebFileReader(state) = kind else {
            unreachable!()
        };
        // Conservative remembered edges are safe if later preparation refuses.
        // Preparing them here keeps publication free of backing allocation.
        state.visit_references(&mut |reference| self.write_barrier(object, Some(reference)));
        Ok(PreparedFileReaderUpdate {
            object,
            previous,
            bytes,
            state,
        })
    }
    pub(crate) fn commit_file_reader_update(&mut self, prepared: PreparedFileReaderUpdate) {
        let PreparedFileReaderUpdate {
            object,
            previous,
            bytes,
            state,
        } = prepared;
        assert_eq!(
            self.file_reader(object).expect("prepared reader").bytes(),
            previous
        );
        let entry = self
            .objects
            .get_mut(&object)
            .expect("protected across collection");
        entry.kind = ObjectKind::WebFileReader(state);
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
    }
}
