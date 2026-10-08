// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Heap {
    pub(crate) fn alloc_host_selection_event(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::HostSelectionEvent {
                current_target: None,
            },
            Some(prototype),
        )
    }
    pub(crate) fn host_selection_current_target(
        &self,
        object: ObjectId,
    ) -> Result<Option<ObjectId>, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::HostSelectionEvent { current_target } => Ok(*current_target),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    pub(crate) fn set_host_selection_current_target(
        &mut self,
        object: ObjectId,
        target: Option<ObjectId>,
    ) -> Result<(), HeapError> {
        self.host_selection_current_target(object)?;
        if let Some(target) = target {
            self.object(target)?;
        }
        self.write_barrier(object, target);
        let ObjectKind::HostSelectionEvent { current_target } =
            &mut self.objects.get_mut(&object).expect("validated above").kind
        else {
            unreachable!()
        };
        *current_target = target;
        Ok(())
    }
    pub(crate) fn alloc_web_file_list(
        &mut self,
        files: Vec<ObjectId>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebFileList(files), Some(prototype))
    }

    pub(crate) fn web_file_list(&self, object: ObjectId) -> Result<&[ObjectId], HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebFileList(files) => Ok(files),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    pub(crate) fn alloc_web_blob(
        &mut self,
        data: WebBlobData,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        if data.bytes.len() > self.max_array_buffer_byte_length() {
            return Err(HeapError::InvalidBufferRange);
        }
        self.alloc(ObjectKind::WebBlob(data), Some(prototype))
    }

    pub(crate) fn web_blob(&self, object: ObjectId) -> Result<&WebBlobData, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebBlob(data) => Ok(data),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
}
