// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser queue backing and task content retain their original native owners.

use super::browser_tasks::BrowserTask;
use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedDeque;
use super::*;

pub(super) struct QueuedBrowserTask {
    pub(super) task: BrowserTask,
    // Content is destroyed before its credit, including during cancellation.
    pub(super) storage: Option<SharedSchedulingCharge>,
}
impl QueuedBrowserTask {
    pub(super) fn owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
}

#[derive(Default)]
pub(super) struct BrowserTaskQueue {
    data: VecDeque<QueuedBrowserTask>,
    storage: Option<SharedSchedulingCharge>,
}
fn task_data(queued: &QueuedBrowserTask) -> &BrowserTask {
    &queued.task
}
fn task_data_mut(queued: &mut QueuedBrowserTask) -> &mut BrowserTask {
    &mut queued.task
}
impl std::ops::Deref for BrowserTaskQueue {
    type Target = VecDeque<QueuedBrowserTask>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::Index<usize> for BrowserTaskQueue {
    type Output = BrowserTask;
    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index].task
    }
}
#[cfg(any(test, coverage))]
impl std::ops::IndexMut<usize> for BrowserTaskQueue {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index].task
    }
}
impl<'a> IntoIterator for &'a BrowserTaskQueue {
    type Item = &'a BrowserTask;
    type IntoIter = std::iter::Map<
        std::collections::vec_deque::Iter<'a, QueuedBrowserTask>,
        fn(&'a QueuedBrowserTask) -> &'a BrowserTask,
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter().map(task_data)
    }
}
impl<'a> IntoIterator for &'a mut BrowserTaskQueue {
    type Item = &'a mut BrowserTask;
    type IntoIter = std::iter::Map<
        std::collections::vec_deque::IterMut<'a, QueuedBrowserTask>,
        fn(&'a mut QueuedBrowserTask) -> &'a mut BrowserTask,
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter_mut().map(task_data_mut)
    }
}
impl BrowserTaskQueue {
    pub(super) fn iter(&self) -> impl Iterator<Item = &BrowserTask> {
        self.data.iter().map(task_data)
    }
    pub(super) fn get(&self, index: usize) -> Option<&BrowserTask> {
        self.data.get(index).map(task_data)
    }
    #[cfg(any(test, coverage))]
    pub(super) fn front(&self) -> Option<&BrowserTask> {
        self.data.front().map(task_data)
    }
    pub(super) fn set_serial(&mut self, index: usize, serial: u64) {
        self.data[index].task.set_serial(serial);
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
            && self.data.iter().all(|task| {
                task.storage
                    .as_ref()
                    .is_none_or(|owner| owner.belongs_to(scope))
            })
    }
    pub(super) fn backing_bytes(&self) -> usize {
        self.data
            .capacity()
            .saturating_mul(std::mem::size_of::<QueuedBrowserTask>())
    }
    pub(super) fn unowned_usage(&self) -> HostSchedulingUsage {
        let mut usage = HostSchedulingUsage {
            payload_bytes: if self.storage.is_none() {
                self.backing_bytes()
            } else {
                0
            },
            ..Default::default()
        };
        for task in self.data.iter().filter(|task| task.storage.is_none()) {
            usage.pending_browser_tasks += 1;
            usage.pending_reactions = usage
                .pending_reactions
                .saturating_add(task.task.reaction_count());
            usage.payload_bytes = usage
                .payload_bytes
                .saturating_add(task.task.payload_bytes());
        }
        usage
    }
    pub(super) fn remove_owned(&mut self, index: usize) -> Option<QueuedBrowserTask> {
        let mut queued = self.data.remove(index)?;
        if let Some(storage) = &mut queued.storage {
            storage.release_browser_tasks();
        }
        Some(queued)
    }
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&BrowserTask) -> bool) {
        self.data.retain(|task| keep(&task.task));
    }
    pub(super) fn shrink_to_fit(&mut self) {
        // A nonempty queue retains its paid capacity. Releasing an empty
        // allocation needs no temporary replacement and preserves identity.
        if self.data.is_empty() {
            self.data = VecDeque::new();
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
        }
    }
}

pub(super) struct PreparedBrowserTask {
    queue: PreparedDeque<QueuedBrowserTask>,
    incoming: Option<SharedSchedulingCharge>,
}
impl PreparedBrowserTask {
    pub(super) fn commit_pending_rejections(
        mut self,
        vm: &mut Vm,
        serial: u64,
        mut input: host_rejections::PendingRejectionTransfer,
    ) {
        match (&mut self.incoming, &mut input.storage) {
            (Some(destination), Some(source)) => destination
                .absorb_from(source)
                .expect("pending notifications keep their original account"),
            (None, None) => {}
            _ => panic!("pending notification ownership was prepared before transfer"),
        }
        self.commit(
            vm,
            BrowserTask::PromiseRejections {
                serial,
                entries: input.entries,
                handled: false,
            },
        );
    }
    /// A producer moves its existing primitive lease into the prepared task.
    pub(super) fn absorb_payload(
        &mut self,
        source: &mut Option<SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        match (&mut self.incoming, source) {
            (Some(destination), Some(source)) => destination.absorb_from(source),
            (None, None) => Ok(()),
            _ => Err(RuntimeError::SchedulingLimit),
        }
    }
    pub(super) fn resize_payload(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if let Some(charge) = &mut self.incoming {
            charge.resize(bytes)?;
        }
        Ok(())
    }
    /// No queue producer or guest call may run between preparation and commit.
    pub(super) fn commit(self, vm: &mut Vm, task: BrowserTask) {
        if let Some(backing) = self.queue.commit_owned(&mut vm.browser_tasks.data) {
            vm.browser_tasks.storage = Some(backing);
        }
        if let Some(storage) = &self.incoming {
            debug_assert_eq!(storage.bytes(), task.payload_bytes());
            debug_assert_eq!(storage.reaction_count(), task.reaction_count());
        }
        vm.browser_tasks.data.push_back(QueuedBrowserTask {
            task,
            storage: self.incoming,
        });
        vm.reconcile_scheduling_ownership();
    }
}
impl Vm {
    pub(super) fn browser_queue_owner(
        &mut self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let current = self.argument_list_owner()?;
        let original = self.browser_tasks.owner_view()?;
        if let Some(scope) = &self.host_scheduling {
            if !self.browser_tasks.can_join(scope)
                || hint.is_some_and(|owner| !owner.belongs_to(scope))
            {
                return Err(scope.fail());
            }
        }
        let backing_owner = original.as_ref().or(current.as_ref()).or(hint);
        if let Some(owner) = backing_owner {
            if self.browser_tasks.storage.is_none()
                || self.browser_tasks.unowned_usage().pending_browser_tasks != 0
            {
                self.synchronize_scheduling_usage()?;
                let usage = self.browser_tasks.unowned_usage();
                let mut storage = match &mut self.host_scheduling {
                    Some(scope) => {
                        let mut storage = scope.detach_owned_payload(usage.payload_bytes);
                        storage
                            .absorb(scope.detach_owned_browser_tasks(usage.pending_browser_tasks))
                            .expect("same document");
                        storage
                            .absorb(scope.detach_owned_reactions(usage.pending_reactions))
                            .expect("same document");
                        storage
                    }
                    None => {
                        let mut storage = owner.reserve_peer(usage.payload_bytes)?;
                        storage
                            .absorb(owner.reserve_browser_tasks(usage.pending_browser_tasks)?)?;
                        storage.absorb(owner.reserve_reactions(usage.pending_reactions)?)?;
                        storage
                    }
                };
                if self.browser_tasks.storage.is_none() {
                    self.browser_tasks.storage =
                        Some(storage.split_off(self.browser_tasks.backing_bytes()));
                }
                for task in &mut self.browser_tasks.data {
                    if task.storage.is_none() {
                        let mut input = storage.split_off(task.task.payload_bytes());
                        input
                            .absorb(storage.split_browser_tasks(1))
                            .expect("same document");
                        input
                            .absorb(storage.split_reactions(task.task.reaction_count()))
                            .expect("same document");
                        task.storage = Some(input);
                    }
                }
            }
        }
        hint.or(current.as_ref())
            .or(original.as_ref())
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn browser_task_owner(
        &mut self,
        index: usize,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        if let Some(BrowserTask::HostPromise { token, .. }) = self.browser_tasks.get(index) {
            // A failed request owner retires its external operation and roots.
            // Preserve that cleanup before a failed queue lease can return.
            self.check_host_promise_owner(*token)?;
        }
        self.browser_queue_owner(None)?;
        self.browser_tasks.data[index].owner()
    }
    pub(super) fn prepare_browser_task(
        &mut self,
        extra: HostSchedulingUsage,
    ) -> Result<PreparedBrowserTask, RuntimeError> {
        self.prepare_browser_task_with_owner(extra, None)
    }
    pub(super) fn prepare_browser_task_with_owner(
        &mut self,
        mut extra: HostSchedulingUsage,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedBrowserTask, RuntimeError> {
        if self.host_scheduling.is_none() && self.browser_tasks.len() >= MAX_PENDING_BROWSER_TASKS {
            return Err(RuntimeError::RangeError(
                "Browser task queue limit exceeded".into(),
            ));
        }
        let owner = self.browser_queue_owner(hint)?;
        let bytes = std::mem::take(&mut extra.payload_bytes);
        // Pending task/rejection counts acquire independent credit before
        // backing allocation or guest-visible state publication.
        let incoming = owner
            .as_ref()
            .map(|owner| {
                let mut input = owner.reserve_peer(bytes)?;
                input.absorb(owner.reserve_browser_tasks(1)?)?;
                input.absorb(owner.reserve_reactions(extra.pending_reactions)?)?;
                Ok::<_, RuntimeError>(input)
            })
            .transpose()?;
        extra.pending_reactions = 0;
        self.admit_scheduling_work(extra)?;
        let backing = self.browser_tasks.owner_view()?;
        let queue =
            PreparedDeque::for_additional_owned(&self.browser_tasks.data, 1, backing.as_ref())?;
        Ok(PreparedBrowserTask { queue, incoming })
    }
    pub(super) fn prepare_browser_rejection_transfer(
        &mut self,
    ) -> Result<PreparedBrowserTask, RuntimeError> {
        let owner = self.pending_rejection_owner()?;
        self.prepare_browser_task_with_owner(HostSchedulingUsage::default(), owner.as_ref())
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/browser_task_storage.rs"]
mod ownership_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/browser_task_lifetime.rs"]
mod lifetime_tests;
