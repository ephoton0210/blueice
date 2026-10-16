// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted root segments move through module cleanup without allocation.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

struct RootSegment {
    roots: Vec<RootId>,
    charge: RefCell<Option<SharedSchedulingCharge>>,
    next: Option<Box<RootSegment>>,
}
impl RootSegment {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(host_scheduling::slots(&self.roots))
    }
}

#[derive(Default)]
pub(super) struct ModuleRoots {
    head: Option<Box<RootSegment>>,
    owner: RefCell<Option<SchedulingScope>>,
    len: usize,
    bytes: usize,
}
pub(super) struct RootReservation {
    segment: Option<Box<RootSegment>>,
    expected_len: usize,
}
impl RootReservation {
    pub(super) fn commit(mut self, roots: &mut ModuleRoots, root: RootId) {
        assert_eq!(
            roots.len, self.expected_len,
            "root reservation stays with its list"
        );
        if let Some(mut segment) = self.segment.take() {
            roots.bytes = roots.bytes.saturating_add(segment.bytes());
            segment.next = roots.head.take();
            roots.head = Some(segment);
        }
        let head = roots.head.as_mut().expect("a root slot was reserved");
        assert!(head.roots.len() < head.roots.capacity());
        head.roots.push(root);
        roots.len += 1;
    }
    pub(super) fn root_into(
        self,
        roots: &mut ModuleRoots,
        heap: &mut Heap,
        object: ObjectId,
    ) -> Result<(), RuntimeError> {
        let root = heap.root(object)?;
        self.commit(roots, root);
        Ok(())
    }
}
pub(super) struct ModuleRootIter<'a> {
    segment: Option<&'a RootSegment>,
    index: usize,
}
impl<'a> Iterator for ModuleRootIter<'a> {
    type Item = &'a RootId;
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(segment) = self.segment {
            if let Some(root) = segment.roots.get(self.index) {
                self.index += 1;
                return Some(root);
            }
            self.segment = segment.next.as_deref();
            self.index = 0;
        }
        None
    }
}
pub(super) struct ModuleRootDrain(ModuleRoots);
impl Iterator for ModuleRootDrain {
    type Item = RootId;
    fn next(&mut self) -> Option<RootId> {
        self.0.pop()
    }
}
impl IntoIterator for ModuleRoots {
    type Item = RootId;
    type IntoIter = ModuleRootDrain;
    fn into_iter(self) -> Self::IntoIter {
        ModuleRootDrain(self)
    }
}
impl<'a> IntoIterator for &'a ModuleRoots {
    type Item = &'a RootId;
    type IntoIter = ModuleRootIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl Drop for ModuleRoots {
    fn drop(&mut self) {
        // A sequence of small loads must not build a recursive destructor.
        while let Some(mut segment) = self.head.take() {
            self.head = segment.next.take();
        }
    }
}
impl ModuleRoots {
    pub(super) fn len(&self) -> usize {
        self.len
    }
    #[cfg(any(test, coverage))]
    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[cfg(any(test, coverage))]
    pub(super) fn retained_bytes(&self) -> usize {
        self.bytes
    }
    pub(super) fn iter(&self) -> ModuleRootIter<'_> {
        ModuleRootIter {
            segment: self.head.as_deref(),
            index: 0,
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn snapshot(&self) -> Vec<RootId> {
        self.iter().copied().collect()
    }

    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if let Some(owner) = self.owner.borrow().as_ref() {
            if !owner.same_document(scope) {
                scope.fail();
                return Err(RuntimeError::TypeError(
                    "module roots already have a document owner".into(),
                ));
            }
            return Ok(());
        }
        let mut cursor = self.head.as_deref();
        while let Some(segment) = cursor {
            let mut charge = segment.charge.borrow_mut();
            if let Some(owner) = charge.as_ref() {
                if !owner.belongs_to(scope) {
                    scope.fail();
                    return Err(RuntimeError::TypeError(
                        "module roots already have a document owner".into(),
                    ));
                }
            } else {
                *charge = Some(scope.shared_charge(segment.bytes())?);
            }
            cursor = segment.next.as_deref();
        }
        *self.owner.borrow_mut() = Some(scope.child());
        Ok(())
    }
    pub(super) fn prepare_slot(
        &self,
        scope: Option<&SchedulingScope>,
    ) -> Result<RootReservation, RuntimeError> {
        if let Some(scope) = scope {
            drop(scope.shared_charge(0)?);
            self.attach(scope)?;
        } else {
            if let Some(owner) = self.owner.borrow().as_ref() {
                return Err(owner.fail());
            }
            let mut cursor = self.head.as_deref();
            while let Some(segment) = cursor {
                if let Some(charge) = segment.charge.borrow().as_ref() {
                    return Err(charge.fail());
                }
                cursor = segment.next.as_deref();
            }
        }
        if self
            .head
            .as_ref()
            .is_some_and(|head| head.roots.len() < head.roots.capacity())
        {
            return Ok(RootReservation {
                segment: None,
                expected_len: self.len,
            });
        }
        let capacity = self
            .head
            .as_ref()
            .map_or(4, |head| head.roots.capacity().saturating_mul(2).max(4));
        let minimum = std::mem::size_of::<RootSegment>()
            .saturating_add(capacity.saturating_mul(std::mem::size_of::<RootId>()));
        let mut charge = scope
            .map(|scope| scope.shared_charge(minimum))
            .transpose()?;
        let mut roots = Vec::new();
        roots
            .try_reserve_exact(capacity)
            .map_err(|_| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?;
        let actual =
            std::mem::size_of::<RootSegment>().saturating_add(host_scheduling::slots(&roots));
        if let Some(charge) = &mut charge {
            charge.resize(actual)?;
        }
        Ok(RootReservation {
            segment: Some(Box::new(RootSegment {
                roots,
                charge: RefCell::new(charge),
                next: None,
            })),
            expected_len: self.len,
        })
    }
    pub(super) fn root_object(
        &mut self,
        heap: &mut Heap,
        scope: Option<&SchedulingScope>,
        object: ObjectId,
    ) -> Result<(), RuntimeError> {
        self.prepare_slot(scope)?.root_into(self, heap, object)
    }
    pub(super) fn pop(&mut self) -> Option<RootId> {
        let head = self.head.as_mut()?;
        let root = head.roots.pop()?;
        self.len -= 1;
        if head.roots.is_empty() {
            let mut head = self.head.take().expect("the segment just supplied a root");
            self.bytes -= head.bytes();
            self.head = head.next.take();
        }
        Some(root)
    }
    /// Splice already-owned segments; cleanup performs no reservation or copy.
    pub(super) fn merge(&mut self, mut other: Self) {
        if other.head.is_none() {
            return;
        }
        let owner = self.owner.get_mut();
        let incoming = other.owner.get_mut();
        if self.head.is_none() {
            *owner = incoming.take();
        } else if !owner
            .as_ref()
            .zip(incoming.as_ref())
            .is_some_and(|(a, b)| a.same_document(b))
        {
            *owner = None;
        }
        self.len = self.len.saturating_add(other.len);
        self.bytes = self.bytes.saturating_add(other.bytes);
        let mut tail = other.head.as_mut().expect("checked nonempty segments");
        while let Some(ref mut next) = tail.next {
            tail = next;
        }
        tail.next = self.head.take();
        self.head = other.head.take();
    }
}

impl Vm {
    pub(super) fn release_module_roots_from(&mut self, roots: &mut ModuleRoots, checkpoint: usize) {
        while roots.len() > checkpoint {
            self.release_root(roots.pop().expect("checked module root suffix"));
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn module_root_storage_bytes(&self) -> usize {
        self.module_graph
            .as_ref()
            .map_or(0, |graph| graph.roots.retained_bytes())
            .saturating_add(self.nested_module_roots.retained_bytes())
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_root_ownership.rs"]
mod ownership_tests;
