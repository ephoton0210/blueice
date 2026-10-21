// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A fixed intrinsic-installation state has no native heap backing to grow.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use crate::native::IteratorHelperMethod;

#[derive(Clone, Copy)]
pub(super) enum LateIteratorHelper {
    FlatMap,
    Chunks,
    Windows,
}

impl LateIteratorHelper {
    pub(super) fn for_key(key: &PropertyName) -> Option<Self> {
        [Self::FlatMap, Self::Chunks, Self::Windows]
            .into_iter()
            .find(|helper| key == helper.name())
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::FlatMap => "flatMap",
            Self::Chunks => "chunks",
            Self::Windows => "windows",
        }
    }

    pub(super) fn native(self) -> IteratorHelperMethod {
        match self {
            Self::FlatMap => IteratorHelperMethod::FlatMap,
            Self::Chunks => IteratorHelperMethod::Chunks,
            Self::Windows => IteratorHelperMethod::Windows,
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::FlatMap => 1,
            Self::Chunks => 2,
            Self::Windows => 4,
        }
    }
}

#[derive(Default)]
pub(super) struct IteratorHelperFlags {
    bits: u8,
    // Only document identity is retained: the flags live inline in Vm, whose
    // allocation is handled by its owner, and need no separate backing lease.
    owner: Option<SharedSchedulingCharge>,
}

impl IteratorHelperFlags {
    pub(super) fn contains(&self, helper: LateIteratorHelper) -> bool {
        self.bits & helper.bit() != 0
    }

    #[cfg(any(test, coverage))]
    fn len(&self) -> usize {
        self.bits.count_ones() as usize
    }

    #[cfg(any(test, coverage))]
    fn is_empty(&self) -> bool {
        self.bits == 0
    }

    pub(super) fn validate_owner(&self) -> Result<(), RuntimeError> {
        if let Some(owner) = &self.owner {
            owner.reserve_peer(0)?;
        }
        Ok(())
    }

    fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.owner
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }

    fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if !self.can_join(scope) {
            return Err(scope.fail());
        }
        let owner = scope.shared_charge(0)?;
        if self.bits != 0 && self.owner.is_none() {
            self.owner = Some(owner);
        }
        Ok(())
    }
}

pub(super) struct PreparedIteratorHelper {
    owner: Option<SharedSchedulingCharge>,
}

impl PreparedIteratorHelper {
    pub(super) fn validate_origin(&self) -> Result<(), RuntimeError> {
        if let Some(owner) = &self.owner {
            owner.reserve_peer(0)?;
        }
        Ok(())
    }
}

impl Vm {
    pub(super) fn prepare_iterator_helper_installation(
        &mut self,
    ) -> Result<PreparedIteratorHelper, RuntimeError> {
        self.iterator_helpers_installed.validate_owner()?;
        // Validate both the existing origin and the current context. Keep the
        // original origin when an unprofiled VM is borrowed by another caller.
        let current = self.argument_list_owner()?;
        let owner = self
            .iterator_helpers_installed
            .owner
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()?
            .or(current);
        Ok(PreparedIteratorHelper { owner })
    }

    pub(super) fn commit_iterator_helper_installation(
        &mut self,
        helper: LateIteratorHelper,
        prepared: PreparedIteratorHelper,
    ) {
        if self.iterator_helpers_installed.owner.is_none() {
            self.iterator_helpers_installed.owner = prepared.owner;
        }
        // The property is now published. Updating fixed bits cannot allocate
        // or encounter a second admission failure after that publication.
        self.iterator_helpers_installed.bits |= helper.bit();
    }

    pub(super) fn intrinsic_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.iterator_helpers_installed.can_join(scope)
    }

    pub(super) fn attach_intrinsic_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        self.iterator_helpers_installed.attach(scope)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/intrinsic_storage.rs"]
mod ownership_tests;
