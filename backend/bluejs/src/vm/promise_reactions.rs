// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise reaction registration and scheduling, independent of species and
//! resolving functions. Installing a reaction never calls JavaScript or allocates
//! in the managed heap; executing a queued handler remains a separate boundary.
//! Tracked records are inserted with their promises, never removed during VM
//! execution, and included in every VM allocation root batch. Callers that
//! validate membership before species/allocation may rely on that membership;
//! this entry point keeps its own invalid-receiver check for other callers.

use super::*;

impl PromiseRecord {
    fn add_then_reaction(&mut self, reaction: PromiseThenReaction) -> Option<PromiseJob> {
        let (fulfilled, value) = match &self.status {
            PromiseStatus::Pending => {
                self.reactions.push(PromiseReaction::Then(reaction));
                return None;
            }
            PromiseStatus::Fulfilled(value) => (true, value.clone()),
            PromiseStatus::Rejected(value) => (false, value.clone()),
        };
        Some(PromiseJob::Reaction {
            target: reaction.target,
            handler: if fulfilled {
                reaction.on_fulfilled
            } else {
                reaction.on_rejected
            },
            value,
            fulfilled,
        })
    }
}

impl Vm {
    pub(super) fn perform_promise_then(
        &mut self,
        promise: ObjectId,
        on_fulfilled: Value,
        on_rejected: Value,
        target: ReactionTarget,
    ) -> Result<(), RuntimeError> {
        let record = self
            .promises
            .get_mut(&promise)
            .ok_or(RuntimeError::TypeError("invalid Promise receiver".into()))?;
        if let Some(job) = record.add_then_reaction(PromiseThenReaction {
            target,
            on_fulfilled,
            on_rejected,
        }) {
            self.promise_jobs.push_back(job);
        }
        Ok(())
    }
}
