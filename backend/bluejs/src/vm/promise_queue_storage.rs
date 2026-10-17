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
            additional: usize,
        }
        impl<T> $name<T> {
            pub(super) fn for_push(
                values: &$container<T>,
                scope: Option<&SchedulingScope>,
            ) -> Result<Self, RuntimeError> {
                Self::for_additional(values, 1, scope)
            }
            pub(super) fn for_additional(
                values: &$container<T>,
                additional: usize,
                scope: Option<&SchedulingScope>,
            ) -> Result<Self, RuntimeError> {
                let mut prepared = Self {
                    replacement: None,
                    charge: None,
                    previous: (values.len(), values.capacity()),
                    additional,
                };
                if additional <= values.capacity() - values.len() {
                    return Ok(prepared);
                }
                let minimum = values
                    .len()
                    .checked_add(additional)
                    .ok_or_else(|| fail(scope))?;
                // Match repeated single-entry growth, including non-power-of-
                // two existing capacities, while admitting one full candidate.
                let step = values.capacity().saturating_mul(2).max(4);
                let multiple = minimum
                    .div_ceil(step)
                    .checked_next_power_of_two()
                    .ok_or_else(|| fail(scope))?;
                let requested = step.checked_mul(multiple).ok_or_else(|| fail(scope))?;
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
                    self.additional <= values.capacity() - values.len(),
                    "queue slots admitted before publication"
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
    pub(super) fn commit(mut self, values: &mut retained_control_map::RetainedControlMap<K, V>) {
        assert_eq!((values.len(), values.capacity()), self.previous);
        if let Some(replacement) = self.replacement.take() {
            values.replace_prepared(replacement);
        }
        assert!(
            values.len() < values.capacity(),
            "continuation slot admitted before publication"
        );
    }

    /// Heap-only allocation may prune records while retaining their backing.
    /// Reuse a slot freed by collection, or move survivors into the admitted
    /// candidate. Preparation's caller prevents intervening table shrinkage.
    pub(super) fn commit_after_pruning(
        mut self,
        values: &mut retained_control_map::RetainedControlMap<K, V>,
    ) {
        assert!(
            values.len() <= self.previous.0,
            "collection only removes old records"
        );
        if values.len() < values.capacity() {
            return;
        }
        let replacement = self
            .replacement
            .take()
            .expect("full table has an admitted candidate");
        values.replace_prepared(replacement);
        assert!(
            values.len() < values.capacity(),
            "record slot admitted before publication"
        );
    }
}

enum AwaitQueue {
    Reaction(PreparedVec<PromiseReaction>),
    Job(PreparedDeque<PromiseJob>),
}

/// Scalar continuation metadata; the caller retains its existing frame/control
/// until both the saved owner and its prepared resumption work can publish.
#[derive(Clone, Copy)]
pub(super) enum AwaitContinuation {
    Module {
        continuation: u64,
    },
    Async {
        continuation: u64,
    },
    GeneratorYield {
        generator: ObjectId,
        target: ObjectId,
        result: ObjectId,
    },
    GeneratorDelegate {
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
    },
}
impl AwaitContinuation {
    fn reaction(self) -> PromiseReaction {
        match self {
            Self::Module { continuation } => PromiseReaction::ModuleAwait { continuation },
            Self::Async { continuation } => PromiseReaction::AsyncAwait { continuation },
            Self::GeneratorYield {
                generator,
                target,
                result,
            } => PromiseReaction::AsyncGeneratorYield {
                generator,
                target,
                result,
            },
            Self::GeneratorDelegate {
                generator,
                target,
                kind,
            } => PromiseReaction::AsyncGeneratorDelegate {
                generator,
                target,
                kind,
            },
        }
    }
    fn job(self, value: Value, fulfilled: bool, origin: ThrowOrigin) -> PromiseJob {
        match self {
            Self::Module { continuation } => PromiseJob::ModuleAwait {
                continuation,
                value,
                fulfilled,
                origin,
            },
            Self::Async { continuation } => PromiseJob::AsyncAwait {
                continuation,
                value,
                fulfilled,
                origin,
            },
            Self::GeneratorYield {
                generator,
                target,
                result,
            } => PromiseJob::AsyncGeneratorYield {
                generator,
                target,
                result,
                value,
                fulfilled,
                origin,
            },
            Self::GeneratorDelegate {
                generator,
                target,
                kind,
            } => PromiseJob::AsyncGeneratorDelegate {
                generator,
                target,
                kind,
                value,
                fulfilled,
                origin,
            },
        }
    }
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
    pub(super) fn publish_and_handle(
        self,
        vm: &mut Vm,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        let result = self.publish(vm);
        // The queue is already committed even if its final budget check failed.
        // Notification cleanup/follow-up is a separate committed producer.
        vm.mark_promise_handled(promise);
        result?;
        vm.check_scheduling_failure()
    }

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
    pub(super) fn prepare_continuation_await(
        &mut self,
        promise: ObjectId,
        continuation: AwaitContinuation,
        frame_bytes: usize,
    ) -> Result<PreparedAwaitRegistration, RuntimeError> {
        // Count and copy admission happen before the retained value/source clone.
        let (status, input) = self.prepare_promise_await_status(promise)?;
        let work = match status {
            PromiseAwaitStatus::Pending => AwaitRegistration::Reaction {
                promise,
                reaction: continuation.reaction(),
            },
            PromiseAwaitStatus::Fulfilled(value) => {
                AwaitRegistration::Job(continuation.job(value, true, ThrowOrigin::default()))
            }
            PromiseAwaitStatus::Rejected(value, origin) => {
                AwaitRegistration::Job(continuation.job(value, false, origin))
            }
        };
        work.prepare(self, frame_bytes, input)
    }

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
    #[cfg(any(test, coverage))]
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
