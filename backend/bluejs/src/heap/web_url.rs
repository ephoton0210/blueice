// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Heap {
    pub(crate) fn alloc_web_url(
        &mut self,
        url: url::Url,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebUrl(Box::new(url)), Some(prototype))
    }
    pub(crate) fn web_url(&self, object: ObjectId) -> Result<&url::Url, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebUrl(url) => Ok(url),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    pub(crate) fn set_web_url(&mut self, object: ObjectId, url: url::Url) -> Result<(), HeapError> {
        let previous = self.web_url(object)?.as_str().len();
        let bytes = url.as_str().len();
        self.ensure_room(bytes.saturating_sub(previous), &[object])?;
        self.objects.get_mut(&object).expect("protected URL").kind =
            ObjectKind::WebUrl(Box::new(url));
        self.managed_bytes = self.managed_bytes - previous + bytes;
        Ok(())
    }
}
