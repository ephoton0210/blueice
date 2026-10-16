// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Managed storage is charged to its heap and, for pages, its document.
//! Updating or dropping the counter releases the same document's charge.

use super::*;
use std::cell::RefCell;

struct DocumentHeap {
    used: usize,
    limit: usize,
    failed: bool,
}

#[derive(Default)]
pub(super) struct ManagedBytes {
    value: usize,
    document: Option<Rc<RefCell<DocumentHeap>>>,
}

impl ManagedBytes {
    pub(super) fn get(&self) -> usize {
        self.value
    }

    pub(super) fn set(&mut self, value: usize) {
        if let Some(document) = &self.document {
            let mut document = document.borrow_mut();
            document.used = document.used - self.value + value;
            document.failed |= document.used > document.limit;
        }
        self.value = value;
    }

    pub(super) fn add(&mut self, bytes: usize) {
        self.set(self.value + bytes);
    }
    pub(super) fn sub(&mut self, bytes: usize) {
        self.set(self.value - bytes);
    }

    pub(super) fn would_exceed(&self, additional: usize) -> bool {
        self.document.as_ref().is_some_and(|document| {
            let document = document.borrow();
            document.failed
                || document
                    .used
                    .checked_add(additional)
                    .is_none_or(|n| n > document.limit)
        })
    }

    pub(super) fn check_room(&self, additional: usize) -> Result<(), HeapError> {
        let Some(document) = &self.document else {
            return Ok(());
        };
        let mut document = document.borrow_mut();
        if document
            .used
            .checked_add(additional)
            .is_none_or(|n| n > document.limit)
        {
            document.failed = true;
        }
        if document.failed {
            Err(HeapError::HeapLimitExceeded {
                limit: document.limit,
            })
        } else {
            Ok(())
        }
    }
}

impl Drop for ManagedBytes {
    fn drop(&mut self) {
        if let Some(document) = &self.document {
            document.borrow_mut().used -= self.value;
        }
    }
}

impl Heap {
    pub(crate) fn begin_document_heap_budget(&mut self) {
        if self.managed_bytes.document.is_none() {
            self.managed_bytes.document = Some(Rc::new(RefCell::new(DocumentHeap {
                used: self.managed_bytes.value,
                limit: self.config.max_heap_bytes,
                failed: false,
            })));
        }
    }

    pub(crate) fn inherit_document_heap_budget(&mut self, owner: &Heap) -> Result<(), HeapError> {
        let Some(document) = &owner.managed_bytes.document else {
            return Ok(());
        };
        if let Some(existing) = &self.managed_bytes.document {
            return if Rc::ptr_eq(existing, document) {
                Ok(())
            } else {
                Err(HeapError::InvalidConfig)
            };
        }
        owner.managed_bytes.check_room(self.managed_bytes.value)?;
        document.borrow_mut().used += self.managed_bytes.value;
        self.managed_bytes.document = Some(document.clone());
        Ok(())
    }

    pub(crate) fn document_heap_budget(&self) -> Option<(usize, usize)> {
        self.managed_bytes.document.as_ref().map(|document| {
            let document = document.borrow();
            (document.used, document.limit)
        })
    }

    pub(crate) fn document_heap_bytes(&self) -> usize {
        self.document_heap_budget()
            .map_or(self.managed_bytes.value, |(used, _)| used)
    }

    pub(crate) fn check_document_heap_budget(&self) -> Result<(), HeapError> {
        self.managed_bytes.check_room(0)
    }
}
