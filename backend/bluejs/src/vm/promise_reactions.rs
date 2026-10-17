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

impl Vm {
    fn prepare_then_registration(
        &mut self,
        promise: ObjectId,
        on_fulfilled: Value,
        on_rejected: Value,
        target: ReactionTarget,
    ) -> Result<promise_queue_storage::PreparedAwaitRegistration, RuntimeError> {
        let record = self
            .promises
            .get(&promise)
            .ok_or_else(|| RuntimeError::TypeError("invalid Promise receiver".into()))?;
        let (pending, copied_bytes) = match &record.status {
            PromiseStatus::Pending => (true, 0),
            PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => (
                false,
                host_scheduling::value_payload_bytes(value)
                    .saturating_add(record.rejection_origin.payload_bytes()),
            ),
        };
        let work = if pending {
            AwaitRegistration::Reaction {
                promise,
                reaction: PromiseReaction::Then(PromiseThenReaction {
                    target,
                    on_fulfilled,
                    on_rejected,
                }),
            }
        } else {
            let fulfilled = matches!(record.status, PromiseStatus::Fulfilled(_));
            // Move the selected handler and discard the other before admission.
            let handler = if fulfilled {
                drop(on_rejected);
                on_fulfilled
            } else {
                drop(on_fulfilled);
                on_rejected
            };
            AwaitRegistration::Job(PromiseJob::Reaction {
                target,
                handler,
                value: Value::Undefined,
                fulfilled,
                origin: ThrowOrigin::default(),
            })
        };
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_reactions: usize::from(pending),
            pending_microtasks: usize::from(!pending),
            ..Default::default()
        })?;
        let incoming = match &work {
            AwaitRegistration::Reaction { reaction, .. } => {
                host_scheduling::reaction_bytes(reaction)
            }
            AwaitRegistration::Job(job) => job.scheduling_payload_bytes(),
        };
        let input = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(incoming.saturating_add(copied_bytes)))
            .transpose()?;
        let mut work = work;
        if let AwaitRegistration::Job(PromiseJob::Reaction { value, origin, .. }) = &mut work {
            let record = &self.promises[&promise];
            let retained = match &record.status {
                PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => value,
                PromiseStatus::Pending => unreachable!("native preparation cannot change status"),
            };
            *value = retained.clone();
            *origin = record.rejection_origin.clone();
        }
        // This existing gate checks actual input ownership and prepares a
        // complete queue replacement without collection or JavaScript.
        work.prepare(self, 0, input)
    }

    pub(super) fn perform_promise_then(
        &mut self,
        promise: ObjectId,
        on_fulfilled: Value,
        on_rejected: Value,
        target: ReactionTarget,
    ) -> Result<(), RuntimeError> {
        let result =
            match self.prepare_then_registration(promise, on_fulfilled, on_rejected, target) {
                Ok(prepared) => {
                    let result = prepared.publish(self);
                    // Queue publication commits the registration. Notification
                    // cleanup/follow-up is a separate committed producer.
                    self.mark_promise_handled(promise);
                    result
                }
                Err(error) => Err(error),
            };
        match result {
            Err(RuntimeError::SchedulingLimit | RuntimeError::Heap(_)) => {
                if let Some(scope) = &self.host_scheduling {
                    scope.fail();
                }
                self.reconcile_scheduling_ownership();
                Ok(())
            }
            other => other,
        }
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_reaction_storage.rs"]
mod storage_tests;
