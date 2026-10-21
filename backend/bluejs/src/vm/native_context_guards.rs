// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immutable caller identities survive nested neutral realm registrations.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct NativeContextGuards {
    owners: Vec<SharedSchedulingCharge>,
    // Zero-byte account views and their Vec allocation die before its credit.
    storage: Option<SharedSchedulingCharge>,
}

impl NativeContextGuards {
    fn references(&self) -> impl Iterator<Item = &SharedSchedulingCharge> + Clone {
        self.storage.iter().chain(self.owners.iter())
    }

    pub(super) fn validate(&self) -> Result<(), RuntimeError> {
        for owner in self.references() {
            owner.reserve_peer(0)?;
        }
        Ok(())
    }

    pub(super) fn capture(
        owner: Option<&SharedSchedulingCharge>,
        inherited: [Option<&Rc<Self>>; 2],
        additional: [Option<&SharedSchedulingCharge>; 3],
    ) -> Result<Option<Rc<Self>>, RuntimeError> {
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        for guards in inherited.iter().flatten() {
            guards.validate()?;
        }
        for owner in additional.iter().flatten() {
            owner.reserve_peer(0)?;
        }
        let sources = inherited.map(|guards| guards.map(|guards| guards.as_ref()));
        let candidates = sources
            .iter()
            .flatten()
            .flat_map(|guards| guards.references())
            .chain(additional.iter().flatten().copied())
            .filter(|candidate| owner.is_none_or(|owner| !owner.same_owner(candidate)));
        if candidates.clone().next().is_none() {
            return Ok(None);
        }
        for existing in inherited.into_iter().flatten() {
            if candidates.clone().all(|candidate| {
                existing
                    .references()
                    .any(|saved| saved.same_owner(candidate))
            }) {
                return Ok(Some(Rc::clone(existing)));
            }
        }
        let header = std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>();
        let count = candidates.clone().count();
        let fail = || owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail);
        let bytes = count
            .checked_mul(std::mem::size_of::<SharedSchedulingCharge>())
            .and_then(|bytes| bytes.checked_add(header))
            .ok_or_else(fail)?;
        let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
        let mut owners = Vec::new();
        owners.try_reserve_exact(count).map_err(|_| fail())?;
        if let Some(storage) = &mut storage {
            storage.resize(header.saturating_add(host_scheduling::slots(&owners)))?;
        }
        for candidate in candidates {
            if !owners
                .iter()
                .any(|saved: &SharedSchedulingCharge| saved.same_owner(candidate))
            {
                owners.push(candidate.reserve_peer(0)?);
            }
        }
        Ok(Some(Rc::new(Self { owners, storage })))
    }
}
