// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise jobs retain their own content/account while queue backing stays paid.

use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedDeque;
use super::*;

pub(super) struct QueuedPromiseJob {
    pub(super) job: PromiseJob,
    pub(super) storage: Option<SharedSchedulingCharge>,
}
impl QueuedPromiseJob {
    pub(super) fn owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
}

#[derive(Default)]
pub(super) struct PromiseJobQueue {
    pub(super) data: VecDeque<QueuedPromiseJob>,
    storage: Option<SharedSchedulingCharge>,
}

fn job_data(queued: &QueuedPromiseJob) -> &PromiseJob {
    &queued.job
}

impl std::ops::Deref for PromiseJobQueue {
    type Target = VecDeque<QueuedPromiseJob>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::Index<usize> for PromiseJobQueue {
    type Output = PromiseJob;
    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index].job
    }
}
impl<'a> IntoIterator for &'a PromiseJobQueue {
    type Item = &'a PromiseJob;
    type IntoIter = std::iter::Map<
        std::collections::vec_deque::Iter<'a, QueuedPromiseJob>,
        fn(&'a QueuedPromiseJob) -> &'a PromiseJob,
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter().map(job_data)
    }
}

impl PromiseJobQueue {
    #[cfg(any(test, coverage))]
    pub(super) fn iter(&self) -> impl Iterator<Item = &PromiseJob> {
        self.data.iter().map(job_data)
    }
    #[cfg(any(test, coverage))]
    pub(super) fn front(&self) -> Option<&PromiseJob> {
        self.data.front().map(job_data)
    }
    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
            && self.data.iter().all(|job| {
                job.storage
                    .as_ref()
                    .is_none_or(|owner| owner.belongs_to(scope))
            })
    }
    pub(super) fn backing_bytes(&self) -> usize {
        self.data
            .capacity()
            .saturating_mul(std::mem::size_of::<QueuedPromiseJob>())
    }
    pub(super) fn unowned_payload_bytes(&self) -> usize {
        let backing = if self.storage.is_some() {
            0
        } else {
            self.backing_bytes()
        };
        self.data
            .iter()
            .filter(|job| job.storage.is_none())
            .fold(backing, |bytes, job| {
                bytes.saturating_add(job.job.scheduling_payload_bytes())
            })
    }
    pub(super) fn unowned_microtasks(&self) -> usize {
        self.data.iter().filter(|job| job.storage.is_none()).count()
    }
    pub(super) fn pop_front(&mut self) -> Option<QueuedPromiseJob> {
        let mut job = self.data.pop_front()?;
        if let Some(storage) = &mut job.storage {
            storage.release_microtasks();
        }
        Some(job)
    }
    pub(super) fn shrink_to_fit(&mut self) {
        assert!(self.data.is_empty(), "release only an empty Promise queue");
        self.data = VecDeque::new();
        if let Some(storage) = &mut self.storage {
            storage.release_to(0);
        }
    }
    pub(super) fn push_prepared(
        &mut self,
        job: PromiseJob,
        mut input: Option<SharedSchedulingCharge>,
        count: Option<SharedSchedulingCharge>,
    ) {
        assert!(self.data.len() < self.data.capacity(), "prepared job slot");
        if let Some(count) = count {
            input
                .as_mut()
                .expect("prepared job content owner")
                .absorb(count)
                .expect("prepared job count shares its content account");
        }
        if let Some(input) = &input {
            debug_assert_eq!(input.bytes(), job.scheduling_payload_bytes());
            input.publish_microtasks();
        }
        self.data.push_back(QueuedPromiseJob {
            job,
            storage: input,
        });
    }

    // Heap cleanup keeps its input on refusal, retaining the source cell's
    // managed credit without allocating an error carrier.
    pub(super) fn try_push_cleanup(
        &mut self,
        job: &mut Option<PromiseJob>,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        let prepared = (|| {
            let backing = self.owner_view()?;
            let owner = hint.or(backing.as_ref());
            let input = owner
                .map(|owner| {
                    owner.reserve_peer(
                        job.as_ref()
                            .expect("cleanup input")
                            .scheduling_payload_bytes(),
                    )
                })
                .transpose()?;
            let count = owner.map(|owner| owner.reserve_microtasks(1)).transpose()?;
            let growth = PreparedDeque::for_additional_owned(&self.data, 1, backing.as_ref())?;
            Ok::<_, RuntimeError>((input, count, growth))
        })();
        let (input, count, growth) = prepared?;
        if let Some(backing) = growth.commit_owned(&mut self.data) {
            self.storage = Some(backing);
        }
        self.push_prepared(job.take().expect("prepared cleanup input"), input, count);
        Ok(())
    }
    #[cfg(any(test, coverage))]
    pub(super) fn retain(&mut self, mut predicate: impl FnMut(&PromiseJob) -> bool) {
        self.data.retain(|job| predicate(&job.job));
    }
    #[cfg(any(test, coverage))]
    pub(super) fn clear(&mut self) {
        self.data.clear();
    }
    #[cfg(any(test, coverage))]
    pub(super) fn extend(&mut self, jobs: impl IntoIterator<Item = PromiseJob>) {
        for job in jobs {
            self.try_push_cleanup(&mut Some(job), None)
                .unwrap_or_else(|error| panic!("fixture queue publication: {error:?}"));
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn reserve_exact(&mut self, additional: usize) {
        self.data.reserve_exact(additional);
        let bytes = self.backing_bytes();
        if let Some(storage) = &mut self.storage {
            storage.reconcile_owned_bytes(bytes);
        }
    }
}

pub(super) struct PreparedPromiseJobs {
    growth: PreparedDeque<QueuedPromiseJob>,
    counts: Option<SharedSchedulingCharge>,
}
impl PreparedPromiseJobs {
    pub(super) fn commit(self, queue: &mut PromiseJobQueue) -> Option<SharedSchedulingCharge> {
        if let Some(backing) = self.growth.commit_owned(&mut queue.data) {
            // The previous allocation has been destroyed; job payload leases
            // have moved with their records into the admitted candidate.
            queue.storage = Some(backing);
        }
        self.counts
    }
}

impl Vm {
    pub(super) fn promise_job_owner(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let current = self.argument_list_owner()?;
        let original = self.promise_jobs.owner_view()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.promise_jobs.can_join(scope)
                || hint.is_some_and(|owner| !owner.belongs_to(scope))
            {
                return Err(scope.fail());
            }
        }
        let backing_owner = original.as_ref().or(current.as_ref()).or(hint);
        if let Some(owner) = backing_owner {
            if self.promise_jobs.storage.is_none() || self.promise_jobs.unowned_microtasks() != 0 {
                self.synchronize_scheduling_usage()?;
                let bytes = self.promise_jobs.unowned_payload_bytes();
                let count = self.promise_jobs.unowned_microtasks();
                let mut storage = match &mut self.host_scheduling {
                    Some(scope) => {
                        let mut storage = scope.detach_owned_payload(bytes);
                        storage
                            .absorb(scope.detach_owned_microtasks(count))
                            .expect("published queue shares the document account");
                        storage
                    }
                    None => {
                        let mut storage = owner.reserve_peer(bytes)?;
                        storage.absorb(owner.reserve_microtasks(count)?)?;
                        storage
                    }
                };
                if self.promise_jobs.storage.is_none() {
                    self.promise_jobs.storage =
                        Some(storage.split_off(self.promise_jobs.backing_bytes()));
                }
                for job in &mut self.promise_jobs.data {
                    if job.storage.is_none() {
                        let mut input = storage.split_off(job.job.scheduling_payload_bytes());
                        input
                            .absorb(storage.split_microtasks(1))
                            .expect("adopted job shares its account");
                        input.publish_microtasks();
                        job.storage = Some(input);
                    }
                }
                debug_assert_eq!(storage.bytes(), 0);
            }
        }
        hint.or(current.as_ref())
            .or(original.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(super) fn prepare_promise_jobs(
        &mut self,
        count: usize,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedPromiseJobs, RuntimeError> {
        let owner = self.promise_job_owner(hint)?;
        let backing = self.promise_jobs.owner_view()?;
        let counts = owner
            .as_ref()
            .map(|owner| owner.reserve_microtasks(count))
            .transpose()?;
        let growth =
            PreparedDeque::for_additional_owned(&self.promise_jobs.data, count, backing.as_ref())?;
        Ok(PreparedPromiseJobs { growth, counts })
    }
}
