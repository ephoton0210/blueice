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
            pub(super) fn for_additional(
                values: &$container<T>,
                additional: usize,
                scope: Option<&SchedulingScope>,
            ) -> Result<Self, RuntimeError> {
                Self::for_additional_with_owner(values, additional, scope, None)
            }
            fn for_additional_with_owner(
                values: &$container<T>,
                additional: usize,
                scope: Option<&SchedulingScope>,
                owner: Option<&SharedSchedulingCharge>,
            ) -> Result<Self, RuntimeError> {
                let refusal = || owner.map_or_else(|| fail(scope), SharedSchedulingCharge::fail);
                let mut prepared = Self {
                    replacement: None,
                    charge: None,
                    previous: (values.len(), values.capacity()),
                    additional,
                };
                if additional <= values.capacity() - values.len() {
                    return Ok(prepared);
                }
                let minimum = values.len().checked_add(additional).ok_or_else(refusal)?;
                // Match repeated single-entry growth, including non-power-of-
                // two existing capacities, while admitting one full candidate.
                let step = values.capacity().saturating_mul(2).max(4);
                let multiple = minimum
                    .div_ceil(step)
                    .checked_next_power_of_two()
                    .ok_or_else(refusal)?;
                let requested = step.checked_mul(multiple).ok_or_else(refusal)?;
                let bytes = requested.saturating_mul(std::mem::size_of::<T>());
                prepared.charge = match owner {
                    Some(owner) => Some(owner.reserve_peer(bytes)?),
                    None => charge(scope, bytes)?,
                };
                let mut replacement = $container::<T>::new();
                replacement
                    .try_reserve_exact(requested)
                    .map_err(|_| refusal())?;
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
        }
    };
}
prepared_sequence!(PreparedVec, Vec);
prepared_sequence!(PreparedDeque, VecDeque);

impl<T> PreparedVec<T> {
    pub(super) fn for_push(
        values: &Vec<T>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        Self::for_additional(values, 1, scope)
    }
    pub(super) fn commit(self, values: &mut Vec<T>) {
        drop(self.commit_owned(values));
    }
    pub(super) fn for_push_owned(
        values: &Vec<T>,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        Self::for_additional_with_owner(values, 1, None, owner)
    }

    /// A detached owner adopts the complete candidate lease with its buffer.
    pub(super) fn commit_owned(mut self, values: &mut Vec<T>) -> Option<SharedSchedulingCharge> {
        assert_eq!((values.len(), values.capacity()), self.previous);
        if let Some(mut replacement) = self.replacement.take() {
            replacement.append(values);
            *values = replacement;
        }
        assert!(self.additional <= values.capacity() - values.len());
        self.charge.take()
    }
    pub(super) fn staging_bytes(&self) -> usize {
        self.replacement.as_ref().map_or(0, |values| {
            values.capacity().saturating_mul(std::mem::size_of::<T>())
        })
    }
}

impl<T> PreparedDeque<T> {
    pub(super) fn for_additional_owned(
        values: &VecDeque<T>,
        count: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        Self::for_additional_with_owner(values, count, None, owner)
    }
    /// A detached queue adopts the complete candidate lease with its backing.
    pub(super) fn commit_owned(
        mut self,
        values: &mut VecDeque<T>,
    ) -> Option<SharedSchedulingCharge> {
        assert_eq!((values.len(), values.capacity()), self.previous);
        if let Some(mut replacement) = self.replacement.take() {
            replacement.append(values);
            *values = replacement;
        }
        assert!(self.additional <= values.capacity() - values.len());
        self.charge.take()
    }
    pub(super) fn staging_bytes(&self) -> usize {
        self.replacement.as_ref().map_or(0, |values| {
            values.capacity().saturating_mul(std::mem::size_of::<T>())
        })
    }
}

pub(super) trait MapPreparationTarget<K, V> {
    fn map(&self) -> &HashMap<K, V>;
    fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        Ok(None)
    }
}
impl<K, V> MapPreparationTarget<K, V> for HashMap<K, V> {
    fn map(&self) -> &HashMap<K, V> {
        self
    }
}
impl<K: Eq + std::hash::Hash, V> MapPreparationTarget<K, V>
    for retained_control_map::RetainedControlMap<K, V>
{
    fn map(&self) -> &HashMap<K, V> {
        self
    }
    fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.owner_view()
    }
}

pub(super) struct PreparedMap<K, V> {
    replacement: Option<HashMap<K, V>>,
    charge: Option<SharedSchedulingCharge>,
    previous: (usize, usize),
    additional: usize,
    owner: Option<SharedSchedulingCharge>,
}
impl<K: Eq + std::hash::Hash, V> PreparedMap<K, V> {
    pub(super) fn for_insert(
        values: &impl MapPreparationTarget<K, V>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        Self::for_additional(values, 1, scope)
    }
    pub(super) fn for_additional(
        values: &impl MapPreparationTarget<K, V>,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let owner = if let Some(owner) = values.owner_view()? {
            if let Some(scope) = scope {
                if !owner.belongs_to(scope) {
                    return Err(scope.fail());
                }
            }
            Some(owner)
        } else {
            scope.map(|scope| scope.shared_charge(0)).transpose()?
        };
        Self::for_additional_owned(values.map(), additional, owner.as_ref())
    }
    pub(super) fn staging_bytes(&self) -> usize {
        self.replacement
            .as_ref()
            .map_or(0, host_scheduling::map_slots)
    }
    pub(super) fn validate_owner(&self) -> Result<(), RuntimeError> {
        if let Some(owner) = &self.owner {
            owner.reserve_peer(0)?;
        }
        Ok(())
    }

    /// A native loan can retain its original owner without a local realm scope.
    pub(super) fn for_additional_owned(
        values: &HashMap<K, V>,
        additional: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut prepared = Self {
            replacement: None,
            charge: None,
            previous: (values.len(), values.capacity()),
            additional,
            owner: owner.map(|owner| owner.reserve_peer(0)).transpose()?,
        };
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        if additional <= values.capacity() - values.len() {
            return Ok(prepared);
        }
        let fail = || owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail);
        let requested = values.len().checked_add(additional).ok_or_else(fail)?;
        prepared.charge = owner
            .map(|owner| {
                owner.reserve_peer(requested.saturating_mul(std::mem::size_of::<(K, V)>()))
            })
            .transpose()?;
        let mut replacement = HashMap::new();
        replacement.try_reserve(requested).map_err(|_| fail())?;
        if let Some(charge) = &mut prepared.charge {
            charge.resize(host_scheduling::map_slots(&replacement))?;
        }
        prepared.replacement = Some(replacement);
        Ok(prepared)
    }
    pub(super) fn commit(self, values: &mut retained_control_map::RetainedControlMap<K, V>) {
        self.commit_owned(values);
    }

    /// Move a complete candidate lease with independently owned table backing.
    pub(super) fn commit_owned(
        mut self,
        values: &mut retained_control_map::RetainedControlMap<K, V>,
    ) -> Option<SharedSchedulingCharge> {
        assert_eq!((values.len(), values.capacity()), self.previous);
        if let Some(replacement) = self.replacement.take() {
            self.charge = values.replace_prepared_owned(replacement, self.charge.take());
        }
        assert!(
            self.additional <= values.capacity() - values.len(),
            "record slots admitted before publication"
        );
        self.charge.take()
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
        if self.additional <= values.capacity() - values.len() {
            return;
        }
        let replacement = self
            .replacement
            .take()
            .expect("full table has an admitted candidate");
        self.charge = values.replace_prepared_owned(replacement, self.charge.take());
        assert!(
            self.additional <= values.capacity() - values.len(),
            "record slots admitted before publication"
        );
    }
}

enum AwaitQueue {
    Reaction(PreparedVec<PromiseReaction>),
    Job(promise_job_storage::PreparedPromiseJobs),
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

struct AwaitInput {
    work: AwaitRegistration,
    input: Option<SharedSchedulingCharge>,
}

impl AwaitRegistration {
    pub(super) fn prepare(
        self,
        vm: &mut Vm,
        frame_bytes: usize,
        input: Option<SharedSchedulingCharge>,
    ) -> Result<PreparedAwaitRegistration, RuntimeError> {
        let mut incoming = AwaitInput { work: self, input };
        if let Some(input) = &incoming.input {
            input.reserve_peer(0)?;
            if let Some(scope) = &vm.host_scheduling {
                if !input.belongs_to(scope) {
                    return Err(scope.fail());
                }
            }
        }
        let owner = match &incoming.work {
            Self::Reaction { promise, .. } => {
                vm.prepare_promise_record_owner(*promise, incoming.input.as_ref())?
            }
            Self::Job(_) => vm.promise_job_owner(incoming.input.as_ref())?,
        };
        let (counts, bytes) = match &incoming.work {
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
        if let Some(owner) = &owner {
            if incoming
                .input
                .as_ref()
                .is_some_and(|input| input.same_owner(owner))
            {
                incoming.input.as_mut().unwrap().resize(bytes)?;
            } else {
                let input = owner.reserve_peer(bytes)?;
                incoming.input = Some(input);
            }
        } else if let Some(input) = &mut incoming.input {
            input.resize(bytes)?;
        }
        vm.admit_scheduling_work(counts)?;
        if matches!(incoming.work, Self::Reaction { .. }) {
            if let Some(owner) = &owner {
                let count = owner.reserve_reactions(1)?;
                incoming
                    .input
                    .as_mut()
                    .expect("prepared reaction input")
                    .absorb(count)?;
            }
        }
        let frame = charge(vm.host_scheduling.as_ref(), frame_bytes)?;
        let queue = match &incoming.work {
            Self::Reaction { promise, .. } => AwaitQueue::Reaction(PreparedVec::for_push_owned(
                &vm.promises[promise].reactions,
                owner.as_ref(),
            )?),
            Self::Job(_) => AwaitQueue::Job(vm.prepare_promise_jobs(1, owner.as_ref())?),
        };
        Ok(PreparedAwaitRegistration {
            work: incoming.work,
            queue,
            input: incoming.input,
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
        let mut input = input;
        match (work, queue) {
            (AwaitRegistration::Reaction { promise, reaction }, AwaitQueue::Reaction(growth)) => {
                let record = vm
                    .promises
                    .get_mut(&promise)
                    .expect("validated await owner");
                record.publish_reaction(growth, reaction, input.take());
            }
            (AwaitRegistration::Job(job), AwaitQueue::Job(growth)) => {
                let count = growth.commit(&mut vm.promise_jobs);
                vm.promise_jobs.push_prepared(job, input.take(), count);
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
        let counts = match &record.status {
            PromiseStatus::Pending => HostSchedulingUsage {
                pending_reactions: 1,
                ..Default::default()
            },
            PromiseStatus::Fulfilled(_) | PromiseStatus::Rejected(_) => HostSchedulingUsage {
                pending_microtasks: 1,
                ..Default::default()
            },
        };
        self.admit_scheduling_work(counts)?;
        self.prepare_promise_status_copy(promise)
    }

    pub(super) fn prepare_promise_status_copy(
        &mut self,
        promise: ObjectId,
    ) -> Result<(PromiseAwaitStatus, Option<SharedSchedulingCharge>), RuntimeError> {
        let owner = self.prepare_promise_record_owner(promise, None)?;
        let bytes = self.promises[&promise].status_payload_bytes();
        let mut input = owner
            .as_ref()
            .map(|owner| owner.reserve_peer(bytes))
            .transpose()?;
        let status = self.promises[&promise].clone_for_await();
        let actual = match &status {
            PromiseAwaitStatus::Pending => 0,
            PromiseAwaitStatus::Fulfilled(value) => host_scheduling::value_payload_bytes(value),
            PromiseAwaitStatus::Rejected(value, origin) => {
                host_scheduling::value_payload_bytes(value).saturating_add(origin.payload_bytes())
            }
        };
        if let Some(input) = &mut input {
            input.resize(actual)?;
        }
        Ok((status, input))
    }

    pub(super) fn publish_promise_job(&mut self, job: PromiseJob) -> Result<(), RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_microtasks: 1,
            ..Default::default()
        })?;
        let owner = self.promise_job_owner(None)?;
        let input = owner
            .as_ref()
            .map(|owner| owner.reserve_peer(job.scheduling_payload_bytes()))
            .transpose()?;
        let growth = self.prepare_promise_jobs(1, owner.as_ref())?;
        let count = growth.commit(&mut self.promise_jobs);
        self.promise_jobs.push_prepared(job, input, count);
        self.reconcile_scheduling_ownership();
        Ok(())
    }
    #[cfg(any(test, coverage))]
    pub(super) fn publish_promise_reaction(
        &mut self,
        promise: ObjectId,
        reaction: PromiseReaction,
    ) -> Result<(), RuntimeError> {
        AwaitRegistration::Reaction { promise, reaction }
            .prepare(self, 0, None)?
            .publish(self)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_queue_storage.rs"]
mod ownership_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/promise_job_lifetime.rs"]
mod lifetime_tests;
