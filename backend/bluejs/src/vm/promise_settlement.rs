// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise settlement prepares complete native ownership before publication.

use super::*;
use host_scheduling::{capability_bytes, slots, value_payload_bytes, SharedSchedulingCharge};
use promise_queue_storage::PreparedDeque;

struct PreparedPromiseSettlement {
    promise: ObjectId,
    status: PromiseStatus,
    origin: ThrowOrigin,
    jobs: Vec<PromiseJob>,
    queue: PreparedDeque<PromiseJob>,
    charge: Option<SharedSchedulingCharge>,
    input: Option<SharedSchedulingCharge>,
}

impl PromiseReaction {
    fn settlement_copy_bytes(&self, fulfilled: bool) -> usize {
        match self {
            Self::Then(reaction) => capability_bytes(&reaction.target).saturating_add(
                value_payload_bytes(if fulfilled {
                    &reaction.on_fulfilled
                } else {
                    &reaction.on_rejected
                }),
            ),
            _ => 0,
        }
    }

    fn prepare_settlement_job(
        &self,
        fulfilled: bool,
        value: &Value,
        origin: &ThrowOrigin,
    ) -> PromiseJob {
        match self {
            Self::Then(reaction) => PromiseJob::Reaction {
                target: reaction.target.clone(),
                handler: if fulfilled {
                    reaction.on_fulfilled.clone()
                } else {
                    reaction.on_rejected.clone()
                },
                value: value.clone(),
                fulfilled,
                origin: origin.clone(),
            },
            Self::ModuleAwait { continuation } => PromiseJob::ModuleAwait {
                continuation: *continuation,
                value: value.clone(),
                fulfilled,
                origin: origin.clone(),
            },
            Self::AsyncAwait { continuation } => PromiseJob::AsyncAwait {
                continuation: *continuation,
                value: value.clone(),
                fulfilled,
                origin: origin.clone(),
            },
            Self::AsyncGeneratorYield {
                generator,
                target,
                result,
            } => PromiseJob::AsyncGeneratorYield {
                generator: *generator,
                target: *target,
                result: *result,
                value: value.clone(),
                fulfilled,
                origin: origin.clone(),
            },
            Self::AsyncGeneratorDelegate {
                generator,
                target,
                kind,
            } => PromiseJob::AsyncGeneratorDelegate {
                generator: *generator,
                target: *target,
                kind: *kind,
                value: value.clone(),
                fulfilled,
                origin: origin.clone(),
            },
        }
    }
}

impl Vm {
    fn prepare_promise_settlement(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
        mut input: Option<SharedSchedulingCharge>,
    ) -> Result<Option<PreparedPromiseSettlement>, RuntimeError> {
        let record = self
            .promises
            .get(&promise)
            .ok_or_else(|| RuntimeError::TypeError("invalid Promise receiver".into()))?;
        if !matches!(record.status, PromiseStatus::Pending) {
            return Ok(None);
        }
        let (fulfilled, value) = match &status {
            PromiseStatus::Fulfilled(value) => (true, value),
            PromiseStatus::Rejected(value) => (false, value),
            PromiseStatus::Pending => {
                return Err(RuntimeError::TypeError(
                    "a Promise settles as fulfilled or rejected, never as pending".into(),
                ));
            }
        };
        let count = record.reactions.len();
        let copied_reactions = record.reactions.iter().fold(0usize, |bytes, reaction| {
            bytes.saturating_add(reaction.settlement_copy_bytes(fulfilled))
        });
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_microtasks: count,
            ..Default::default()
        })?;
        let value_bytes = value_payload_bytes(value);
        if let Some(input) = &mut input {
            if self
                .host_scheduling
                .as_ref()
                .is_some_and(|scope| !input.belongs_to(scope))
            {
                return Err(self.host_scheduling.as_ref().unwrap().fail());
            }
            input.resize(value_bytes)?;
        } else {
            input = self
                .host_scheduling
                .as_ref()
                .map(|scope| scope.shared_charge(value_bytes))
                .transpose()?;
        }
        // Borrow the source before admitting its first clone. An override
        // with unknown source still wins over the active diagnostic frame.
        let origin_bytes = if fulfilled {
            0
        } else {
            self.current_rejection_origin_payload_bytes()
        };
        let payload = |origin_bytes: usize| {
            copied_reactions
                .saturating_add(count.saturating_mul(value_bytes))
                .saturating_add(count.saturating_add(1).saturating_mul(origin_bytes))
        };
        let scope = self.host_scheduling.as_ref();
        let mut charge = scope
            .map(|scope| {
                scope.shared_charge(
                    count
                        .saturating_mul(std::mem::size_of::<PromiseJob>())
                        .saturating_add(payload(origin_bytes)),
                )
            })
            .transpose()?;
        let mut jobs = Vec::new();
        jobs.try_reserve_exact(count)
            .map_err(|_| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?;
        if let Some(charge) = &mut charge {
            charge.resize(slots(&jobs).saturating_add(payload(origin_bytes)))?;
        }
        let origin = if fulfilled {
            ThrowOrigin::default()
        } else {
            self.current_rejection_origin()
        };
        if let Some(charge) = &mut charge {
            charge.resize(slots(&jobs).saturating_add(payload(origin.payload_bytes())))?;
        }
        let mut actual_jobs = origin.payload_bytes();
        let mut remaining = copied_reactions.saturating_add(
            count.saturating_mul(value_bytes.saturating_add(origin.payload_bytes())),
        );
        for reaction in &self.promises[&promise].reactions {
            remaining = remaining.saturating_sub(
                reaction
                    .settlement_copy_bytes(fulfilled)
                    .saturating_add(value_bytes)
                    .saturating_add(origin.payload_bytes()),
            );
            jobs.push(reaction.prepare_settlement_job(fulfilled, value, &origin));
            actual_jobs = actual_jobs.saturating_add(
                jobs.last()
                    .expect("prepared job")
                    .scheduling_payload_bytes(),
            );
            if let Some(charge) = &mut charge {
                // Observe actual clone storage before preparing the next job.
                charge.resize(
                    slots(&jobs)
                        .saturating_add(actual_jobs)
                        .saturating_add(remaining),
                )?;
            }
        }
        let queue = PreparedDeque::for_additional(&self.promise_jobs, count, scope)?;
        Ok(Some(PreparedPromiseSettlement {
            promise,
            status,
            origin,
            jobs,
            queue,
            charge,
            input,
        }))
    }

    pub(super) fn settle_promise_with_input_charge(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
        input: Option<SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        match self.prepare_promise_settlement(promise, status, input) {
            Ok(Some(prepared)) => prepared.publish(self),
            Ok(None) => {}
            Err(RuntimeError::SchedulingLimit | RuntimeError::Heap(_)) => {
                // Native resolvers are infallible for valid receivers. Refusal
                // retains pending work and latches the document abort instead.
                if let Some(scope) = &self.host_scheduling {
                    scope.fail();
                }
                self.reconcile_scheduling_ownership();
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }
}

impl PreparedPromiseSettlement {
    fn publish(self, vm: &mut Vm) {
        let Self {
            promise,
            status,
            origin,
            mut jobs,
            queue,
            charge,
            input,
        } = self;
        queue.commit(&mut vm.promise_jobs);
        let rejected = matches!(status, PromiseStatus::Rejected(_));
        let record = vm
            .promises
            .get_mut(&promise)
            .expect("prepared Promise owner");
        record.status = status;
        record.rejection_origin = origin;
        record.reactions = Vec::new();
        let notify = rejected && !record.handled;
        vm.promise_jobs.extend(jobs.drain(..));
        drop(jobs);
        drop(input);
        drop(charge);
        vm.reconcile_scheduling_ownership();
        if notify {
            vm.track_host_rejection(promise);
        }
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_settlement.rs"]
mod ownership_tests;
