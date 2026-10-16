// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Foreign facade roots are released without reborrowing an active VM.

use super::*;
use std::cell::RefCell;
use std::rc::Weak;

pub(crate) struct RealmRoot {
    pub(crate) id: RootId,
    releases: Weak<RefCell<Vec<RootId>>>,
}

impl Drop for RealmRoot {
    fn drop(&mut self) {
        if let Some(releases) = self.releases.upgrade() {
            releases.borrow_mut().push(self.id);
        }
    }
}

impl Heap {
    pub(crate) fn realm_root(&mut self, target: ObjectId) -> Result<Rc<RealmRoot>, HeapError> {
        Ok(Rc::new(RealmRoot {
            id: self.root(target)?,
            releases: Rc::downgrade(&self.released_realm_roots),
        }))
    }

    pub(crate) fn release_realm_roots(&mut self) {
        for root in self.released_realm_roots.borrow_mut().drain(..) {
            self.roots.remove(&root);
        }
    }

    pub(crate) fn realm_reachable(
        &self,
        ignored: &HashSet<RootId>,
        extra: &[ObjectId],
    ) -> HashSet<ObjectId> {
        self.mark_without_roots(false, extra, ignored)
    }
}
