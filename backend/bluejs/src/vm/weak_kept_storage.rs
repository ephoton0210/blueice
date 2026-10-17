// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Kept WeakRef targets carry their native buffer owner through displacement.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[derive(Default)]
pub(super) struct KeptWeakObjects {
    values: Vec<ObjectId>,
    owner: Option<SharedSchedulingCharge>,
}

impl std::ops::Deref for KeptWeakObjects {
    type Target = [ObjectId];
    fn deref(&self) -> &[ObjectId] {
        &self.values
    }
}

impl KeptWeakObjects {
    pub(super) fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if let Some(owner) = &self.owner {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        } else {
            let bytes = host_scheduling::slots(&self.values);
            if bytes != 0 {
                self.owner = Some(scope.shared_charge(bytes)?);
            }
        }
        if !self.values.is_empty() {
            scope.request_checkpoint();
        }
        Ok(())
    }
    pub(super) fn keep(
        &mut self,
        target: ObjectId,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = scope {
            self.attach(scope)?;
            let _admission = scope.shared_charge(0)?;
        } else if self.owner.is_some() {
            return Err(RuntimeError::Unsupported(
                "owned weak roots require their document scope",
            ));
        }
        if self.values.contains(&target) {
            return Ok(());
        }
        let prepared = promise_queue_storage::PreparedVec::for_push(&self.values, scope)?;
        if let Some(owner) = prepared.commit_owned(&mut self.values) {
            self.owner = Some(owner);
        }
        self.values.push(target);
        if let Some(scope) = scope {
            scope.request_checkpoint();
        }
        Ok(())
    }
    pub(super) fn clear(&mut self) {
        self.values = Vec::new();
        self.owner = None;
    }
    #[cfg(any(test, coverage))]
    pub(super) fn retained_bytes(&self) -> usize {
        host_scheduling::slots(&self.values)
    }
}
