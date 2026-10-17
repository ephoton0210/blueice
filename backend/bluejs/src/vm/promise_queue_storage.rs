// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise queue and reaction replacements retain admitted native ownership.

use super::host_scheduling::{reaction_bytes, SharedSchedulingCharge};
use super::*;

fn charge(
    scope: Option<&SchedulingScope>,
    bytes: usize,
) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
    scope.map(|scope| scope.shared_charge(bytes)).transpose()
}
fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}

macro_rules! prepared_sequence {
    ($name:ident, $container:ident) => {
        pub(super) struct $name<T> {
            replacement: Option<$container<T>>,
            charge: Option<SharedSchedulingCharge>,
            previous: (usize, usize),
        }
        impl<T> $name<T> {
            pub(super) fn for_push(
                values: &$container<T>,
                scope: Option<&SchedulingScope>,
            ) -> Result<Self, RuntimeError> {
                let mut prepared = Self {
                    replacement: None,
                    charge: None,
                    previous: (values.len(), values.capacity()),
                };
                if values.len() < values.capacity() {
                    return Ok(prepared);
                }
                let requested = values
                    .len()
                    .saturating_add(1)
                    .max(values.capacity().saturating_mul(2))
                    .max(4);
                prepared.charge =
                    charge(scope, requested.saturating_mul(std::mem::size_of::<T>()))?;
                let mut replacement = $container::<T>::new();
                replacement
                    .try_reserve_exact(requested)
                    .map_err(|_| fail(scope))?;
                if let Some(charge) = &mut prepared.charge {
                    charge.resize(
                        replacement
                            .capacity()
                            .saturating_mul(std::mem::size_of::<T>()),
                    )?;
                }
                prepared.replacement = Some(replacement);
                Ok(prepared)
            }
            pub(super) fn commit(mut self, values: &mut $container<T>) {
                assert_eq!((values.len(), values.capacity()), self.previous);
                if let Some(mut replacement) = self.replacement.take() {
                    replacement.append(values);
                    *values = replacement;
                }
                assert!(
                    values.len() < values.capacity(),
                    "queue slot admitted before publication"
                );
            }
        }
    };
}
prepared_sequence!(PreparedVec, Vec);
prepared_sequence!(PreparedDeque, VecDeque);

pub(super) struct PreparedMap<K, V> {
    replacement: Option<HashMap<K, V>>,
    charge: Option<SharedSchedulingCharge>,
    previous: (usize, usize),
}
impl<K: Eq + std::hash::Hash, V> PreparedMap<K, V> {
    pub(super) fn for_insert(
        values: &HashMap<K, V>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut prepared = Self {
            replacement: None,
            charge: None,
            previous: (values.len(), values.capacity()),
        };
        if values.len() < values.capacity() {
            return Ok(prepared);
        }
        let requested = values.len().saturating_add(1);
        prepared.charge = charge(
            scope,
            requested.saturating_mul(std::mem::size_of::<(K, V)>()),
        )?;
        let mut replacement = HashMap::new();
        replacement
            .try_reserve(requested)
            .map_err(|_| fail(scope))?;
        if let Some(charge) = &mut prepared.charge {
            charge.resize(host_scheduling::map_slots(&replacement))?;
        }
        prepared.replacement = Some(replacement);
        Ok(prepared)
    }
    pub(super) fn commit(mut self, values: &mut HashMap<K, V>) {
        assert_eq!((values.len(), values.capacity()), self.previous);
        if let Some(mut replacement) = self.replacement.take() {
            replacement.extend(values.drain());
            *values = replacement;
        }
        assert!(
            values.len() < values.capacity(),
            "continuation slot admitted before publication"
        );
    }
}

enum AwaitQueue {
    Reaction(PreparedVec<PromiseReaction>),
    Job(PreparedDeque<PromiseJob>),
}
pub(super) struct PreparedAwaitRegistration {
    work: AwaitRegistration,
    queue: AwaitQueue,
    input: Option<SharedSchedulingCharge>,
    frame: Option<SharedSchedulingCharge>,
}
impl AwaitRegistration {
    pub(super) fn prepare(
        self,
        vm: &mut Vm,
        frame_bytes: usize,
        mut input: Option<SharedSchedulingCharge>,
    ) -> Result<PreparedAwaitRegistration, RuntimeError> {
        let (counts, bytes) = match &self {
            Self::Reaction { reaction, .. } => (
                HostSchedulingUsage {
                    pending_reactions: 1,
                    ..Default::default()
                },
                reaction_bytes(reaction),
            ),
            Self::Job(job) => (
                HostSchedulingUsage {
                    pending_microtasks: 1,
                    ..Default::default()
                },
                job.scheduling_payload_bytes(),
            ),
        };
        if let Some(input) = &mut input {
            input.resize(bytes)?;
        }
        vm.admit_scheduling_work(counts)?;
        let frame = charge(vm.host_scheduling.as_ref(), frame_bytes)?;
        let queue = match &self {
            Self::Reaction { promise, .. } => AwaitQueue::Reaction(PreparedVec::for_push(
                &vm.promises[promise].reactions,
                vm.host_scheduling.as_ref(),
            )?),
            Self::Job(_) => AwaitQueue::Job(PreparedDeque::for_push(
                &vm.promise_jobs,
                vm.host_scheduling.as_ref(),
            )?),
        };
        Ok(PreparedAwaitRegistration {
            work: self,
            queue,
            input,
            frame,
        })
    }
}
impl PreparedAwaitRegistration {
    pub(super) fn publish(self, vm: &mut Vm) -> Result<(), RuntimeError> {
        let Self {
            work,
            queue,
            input,
            frame,
        } = self;
        match (work, queue) {
            (AwaitRegistration::Reaction { promise, reaction }, AwaitQueue::Reaction(growth)) => {
                let reactions = &mut vm
                    .promises
                    .get_mut(&promise)
                    .expect("validated await owner")
                    .reactions;
                growth.commit(reactions);
                reactions.push(reaction);
            }
            (AwaitRegistration::Job(job), AwaitQueue::Job(growth)) => {
                growth.commit(&mut vm.promise_jobs);
                vm.promise_jobs.push_back(job);
            }
            _ => unreachable!("prepared queue matches its await work"),
        }
        drop(input);
        drop(frame);
        vm.reconcile_scheduling_ownership();
        vm.check_scheduling_failure()
    }
}

impl Vm {
    pub(super) fn prepare_promise_await_status(
        &mut self,
        promise: ObjectId,
    ) -> Result<(PromiseAwaitStatus, Option<SharedSchedulingCharge>), RuntimeError> {
        let record = self
            .promises
            .get(&promise)
            .ok_or_else(|| RuntimeError::TypeError("invalid await Promise".into()))?;
        let (counts, bytes) = match &record.status {
            PromiseStatus::Pending => (
                HostSchedulingUsage {
                    pending_reactions: 1,
                    ..Default::default()
                },
                0,
            ),
            PromiseStatus::Fulfilled(value) => (
                HostSchedulingUsage {
                    pending_microtasks: 1,
                    ..Default::default()
                },
                host_scheduling::value_payload_bytes(value),
            ),
            PromiseStatus::Rejected(value) => (
                HostSchedulingUsage {
                    pending_microtasks: 1,
                    ..Default::default()
                },
                host_scheduling::value_payload_bytes(value)
                    .saturating_add(record.rejection_origin.payload_bytes()),
            ),
        };
        self.admit_scheduling_work(counts)?;
        let input = charge(self.host_scheduling.as_ref(), bytes)?;
        Ok((self.promises[&promise].clone_for_await(), input))
    }

    pub(super) fn publish_promise_job(&mut self, job: PromiseJob) -> Result<(), RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_microtasks: 1,
            ..Default::default()
        })?;
        let input = charge(
            self.host_scheduling.as_ref(),
            job.scheduling_payload_bytes(),
        )?;
        let growth = PreparedDeque::for_push(&self.promise_jobs, self.host_scheduling.as_ref())?;
        growth.commit(&mut self.promise_jobs);
        self.promise_jobs.push_back(job);
        drop(input);
        self.reconcile_scheduling_ownership();
        Ok(())
    }
    pub(super) fn publish_promise_reaction(
        &mut self,
        promise: ObjectId,
        reaction: PromiseReaction,
    ) -> Result<(), RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_reactions: 1,
            ..Default::default()
        })?;
        let input = charge(self.host_scheduling.as_ref(), reaction_bytes(&reaction))?;
        let growth = PreparedVec::for_push(
            &self.promises[&promise].reactions,
            self.host_scheduling.as_ref(),
        )?;
        let reactions = &mut self
            .promises
            .get_mut(&promise)
            .expect("validated reaction owner")
            .reactions;
        growth.commit(reactions);
        reactions.push(reaction);
        drop(input);
        self.reconcile_scheduling_ownership();
        Ok(())
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_queue_storage.rs"]
mod ownership_tests;
