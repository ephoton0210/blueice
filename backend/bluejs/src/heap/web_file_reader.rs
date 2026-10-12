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
    pub fn references(&self) -> Vec<ObjectId> {
        self.target
            .references()
            .into_iter()
            .chain(self.result.object_id())
            .chain(self.error)
            .chain(self.blob)
            .collect()
    }
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + self.target.bytes()
            + self.encoding.as_ref().map_or(0, JsString::byte_len)
            + match &self.result {
                Value::String(value) => value.byte_len(),
                _ => 0,
            }
    }
}

impl Heap {
    pub(crate) fn alloc_file_reader(&mut self, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebFileReader(Box::default()), Some(prototype))
    }
    pub(crate) fn file_reader(&self, object: ObjectId) -> Result<&FileReaderState, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebFileReader(state) => Ok(state),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    pub(crate) fn set_file_reader(
        &mut self,
        object: ObjectId,
        state: FileReaderState,
    ) -> Result<(), HeapError> {
        let previous = self.file_reader(object)?.bytes();
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
        entry.kind = ObjectKind::WebFileReader(Box::new(state));
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes = self.managed_bytes - previous + bytes;
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }
}
