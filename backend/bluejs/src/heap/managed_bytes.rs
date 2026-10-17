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
    native: Option<crate::vm::NativeDocumentIdentity>,
}

/// Opaque shared managed account; holding it owns no heap or managed bytes.
#[derive(Clone)]
pub(crate) struct DocumentHeapBudget {
    document: Rc<RefCell<DocumentHeap>>,
}
impl DocumentHeapBudget {
    pub(crate) fn empty(limit: usize) -> Self {
        Self {
            document: Rc::new(RefCell::new(DocumentHeap {
                used: 0,
                limit,
                failed: false,
                native: None,
            })),
        }
    }
    pub(crate) fn same_document(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.document, &other.document)
    }
    pub(crate) fn is_failed(&self) -> bool {
        self.document.borrow().failed
    }
    pub(crate) fn accepts_native(&self, budget: &crate::vm::HeapNativeBudget) -> bool {
        self.document
            .borrow()
            .native
            .as_ref()
            .is_none_or(|identity| budget.has_identity(identity))
    }
    pub(crate) fn bind_native(
        &self,
        budget: &crate::vm::HeapNativeBudget,
    ) -> Result<(), HeapError> {
        if !self.accepts_native(budget) {
            return Err(HeapError::InvalidConfig);
        }
        self.document.borrow_mut().native = Some(budget.identity());
        Ok(())
    }
    pub(crate) fn check_room(&self, additional: usize) -> Result<(), HeapError> {
        let mut document = self.document.borrow_mut();
        if document
            .used
            .checked_add(additional)
            .is_none_or(|used| used > document.limit)
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
    #[cfg(any(test, coverage))]
    pub(crate) fn fail_for_test(&self) {
        self.document.borrow_mut().failed = true;
    }
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
    #[cfg(any(test, coverage))]
    pub(crate) fn begin_document_heap_budget(&mut self) {
        if self.managed_bytes.document.is_none() {
            let budget = DocumentHeapBudget::empty(self.config.max_heap_bytes);
            // The fixture creates an account for an already existing heap.
            budget.document.borrow_mut().used = self.managed_bytes.value;
            self.managed_bytes.document = Some(budget.document);
        }
    }

    #[cfg(any(test, coverage))]
    pub(crate) fn inherit_document_heap_budget(&mut self, owner: &Heap) -> Result<(), HeapError> {
        if let Some(budget) = owner.document_heap_budget_handle() {
            self.join_document_heap_budget(&budget)?;
        }
        Ok(())
    }

    pub(crate) fn document_heap_budget_handle(&self) -> Option<DocumentHeapBudget> {
        self.managed_bytes
            .document
            .as_ref()
            .map(|document| DocumentHeapBudget {
                document: document.clone(),
            })
    }
    pub(crate) fn can_join_document_heap_budget(&self, budget: &DocumentHeapBudget) -> bool {
        self.managed_bytes
            .document
            .as_ref()
            .is_none_or(|existing| Rc::ptr_eq(existing, &budget.document))
    }
    pub(crate) fn join_document_heap_budget(
        &mut self,
        budget: &DocumentHeapBudget,
    ) -> Result<(), HeapError> {
        if !self.can_join_document_heap_budget(budget) {
            return Err(HeapError::InvalidConfig);
        }
        if self.managed_bytes.document.is_some() {
            return budget.check_room(0);
        }
        budget.check_room(self.managed_bytes.value)?;
        budget.document.borrow_mut().used += self.managed_bytes.value;
        self.managed_bytes.document = Some(budget.document.clone());
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
