// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Each shared-realm owner admits its own weak identity node in its document.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use std::rc::Weak;

struct OwnerNode {
    value: Weak<()>,
    next: RefCell<Option<Rc<OwnerNode>>>,
    // Weak identity and links drop before their allocation credit.
    storage: RefCell<Option<SharedSchedulingCharge>>,
}
#[derive(Clone)]
pub(super) struct RealmOwner {
    value: Rc<()>,
    node: Rc<OwnerNode>,
}
impl RealmOwner {
    pub(super) fn bytes() -> usize {
        std::mem::size_of::<OwnerNode>() + 4 * std::mem::size_of::<usize>()
    }
    fn new(scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let storage = scope
            .map(|scope| scope.shared_charge(Self::bytes()))
            .transpose()?;
        let value = Rc::new(());
        let node = Rc::new(OwnerNode {
            value: Rc::downgrade(&value),
            next: RefCell::new(None),
            storage: RefCell::new(storage),
        });
        Ok(Self { value, node })
    }
    pub(super) fn identity(&self) -> usize {
        Rc::as_ptr(&self.value) as usize
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.node
            .storage
            .borrow()
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }
    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if !self.can_join(scope) {
            return Err(scope.fail());
        }
        let mut owner = self.node.storage.borrow_mut();
        if owner.is_none() {
            *owner = Some(scope.shared_charge(Self::bytes())?);
        }
        Ok(())
    }
}

struct OwnerList {
    head: RefCell<Option<Rc<OwnerNode>>>,
}
impl Drop for OwnerList {
    fn drop(&mut self) {
        // Tokens can outlive the list. Detach every link iteratively so neither
        // teardown nor a later token drop recurses through a long alias chain.
        let mut current = self.head.get_mut().take();
        while let Some(node) = current {
            current = node.next.borrow_mut().take();
        }
    }
}
#[derive(Clone)]
pub(super) struct RealmOwners {
    values: Rc<OwnerList>,
    // The shared head drops before its header credit. Individual nodes may
    // belong to different documents and retain their own independent leases.
    storage: Rc<RefCell<Option<SharedSchedulingCharge>>>,
}
pub(super) struct PreparedOwner {
    owner: RealmOwner,
}
impl RealmOwners {
    fn header_bytes() -> usize {
        std::mem::size_of::<OwnerList>()
            + std::mem::size_of::<RefCell<Option<SharedSchedulingCharge>>>()
            + 4 * std::mem::size_of::<usize>()
    }
    pub(super) fn new(scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let storage = scope
            .map(|scope| scope.shared_charge(Self::header_bytes()))
            .transpose()?;
        Ok(Self {
            values: Rc::new(OwnerList {
                head: RefCell::new(None),
            }),
            storage: Rc::new(RefCell::new(storage)),
        })
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .borrow()
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }
    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if !self.can_join(scope) {
            return Err(scope.fail());
        }
        if self.storage.borrow().is_some() {
            return Ok(());
        }
        self.prune();
        let mut owner = self.storage.borrow_mut();
        if owner.is_none() {
            *owner = Some(scope.shared_charge(Self::header_bytes())?);
        }
        Ok(())
    }
    pub(super) fn prepare_owner(
        &self,
        scope: Option<&SchedulingScope>,
    ) -> Result<PreparedOwner, RuntimeError> {
        // Adding a foreign alias never grows or rebinds the original document's
        // shared allocation. The receiving document admits one complete node.
        Ok(PreparedOwner {
            owner: RealmOwner::new(scope)?,
        })
    }
    pub(super) fn commit_owner(&self, prepared: PreparedOwner) -> RealmOwner {
        let owner = prepared.owner;
        let mut head = self.values.head.borrow_mut();
        assert!(
            owner.node.next.borrow().is_none(),
            "new owner publishes once"
        );
        *owner.node.next.borrow_mut() = head.take();
        *head = Some(owner.node.clone());
        owner
    }
    pub(super) fn has_external_owner(&self, known: &[usize]) -> bool {
        let mut current = self.values.head.borrow().clone();
        while let Some(node) = current {
            if node.value.strong_count() != 0
                && known
                    .binary_search(&(node.value.as_ptr() as usize))
                    .is_err()
            {
                return true;
            }
            current = node.next.borrow().clone();
        }
        false
    }
    pub(super) fn prune(&self) {
        let mut current = self.values.head.borrow_mut().take();
        let mut kept = None;
        while let Some(node) = current {
            current = node.next.borrow_mut().take();
            if node.value.strong_count() != 0 {
                *node.next.borrow_mut() = kept;
                kept = Some(node);
            }
        }
        *self.values.head.borrow_mut() = kept;
    }
    #[cfg(any(test, coverage))]
    pub(super) fn len(&self) -> usize {
        let mut current = self.values.head.borrow().clone();
        let mut len = 0;
        while let Some(node) = current {
            len += 1;
            current = node.next.borrow().clone();
        }
        len
    }
    #[cfg(any(test, coverage))]
    pub(super) fn retained_bytes(&self) -> usize {
        Self::header_bytes() + self.len() * RealmOwner::bytes()
    }
}
#[cfg(any(test, coverage))]
impl PreparedOwner {
    pub(super) fn staging_bytes(&self) -> usize {
        RealmOwner::bytes()
    }
}
