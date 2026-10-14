// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise reaction registration and scheduling, independent of species and
//! resolving functions. Installing a reaction never calls JavaScript or allocates
//! in the managed heap; executing a queued handler remains a separate boundary.
//! Tracked records are traced from their heap owners and swept after a
//! collection. Callers retain a validated receiver across species/allocation
//! before relying on its membership;
//! this entry point keeps its own invalid-receiver check for other callers.

use super::*;

impl PromiseRecord {
    fn add_then_reaction(&mut self, reaction: PromiseThenReaction) -> Option<PromiseJob> {
        let (fulfilled, value) = match &self.status {
            PromiseStatus::Pending => {
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
        if self
            .promises
            .get(&promise)
            .is_some_and(|record| matches!(record.status, PromiseStatus::Pending))
        {
            self.enqueue_promise_reaction(
                promise,
                PromiseReaction::Then(PromiseThenReaction {
                    target,
                    on_fulfilled,
                    on_rejected,
                }),
            );
            return Ok(());
        }
        let record = self
            .promises
            .get_mut(&promise)
            .ok_or(RuntimeError::TypeError("invalid Promise receiver".into()))?;
        if let Some(job) = record.add_then_reaction(PromiseThenReaction {
            target,
            on_fulfilled,
            on_rejected,
        }) {
            self.enqueue_promise_job(job);
        }
        Ok(())
    }
}
