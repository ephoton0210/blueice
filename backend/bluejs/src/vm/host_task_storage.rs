// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted host-turn snapshots retain native ownership across callbacks.

use super::realm_collection::MetadataStorage;
use super::*;

pub(super) struct TaskVector<T> {
    values: Vec<T>,
    // Buffers and any nested owners drop before their slot lease.
    storage: MetadataStorage,
}
impl<T> TaskVector<T> {
    pub(super) fn new(scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        Ok(Self {
            values: Vec::new(),
            storage: MetadataStorage::new(scope)?,
        })
    }
    pub(super) fn reserve(&mut self, additional: usize) -> Result<(), RuntimeError> {
        self.storage.reserve_vec(&mut self.values, additional)
    }
    pub(super) fn push(&mut self, value: T) -> Result<(), RuntimeError> {
        self.reserve(1)?;
        self.values.push(value);
        Ok(())
    }
    pub(super) fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        self.values.retain(keep);
    }
    #[cfg(any(test, coverage))]
    fn retained_bytes(&self) -> usize {
        host_scheduling::slots(&self.values)
    }
}
impl<T> std::ops::Deref for TaskVector<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> std::ops::DerefMut for TaskVector<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}

pub(super) struct TimerSnapshot {
    pub(super) callbacks: TaskVector<(u64, u32)>,
    pub(super) aborts: TaskVector<u64>,
}
impl TimerSnapshot {
    pub(super) fn is_empty(&self) -> bool {
        self.callbacks.is_empty() && self.aborts.is_empty()
    }
}

pub(super) struct TimerSnapshots {
    entries: TaskVector<(u64, TimerSnapshot)>,
}
impl TimerSnapshots {
    pub(super) fn new(scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        Ok(Self {
            entries: TaskVector::new(scope)?,
        })
    }
    pub(super) fn insert(
        &mut self,
        heap: u64,
        snapshot: TimerSnapshot,
    ) -> Result<(), RuntimeError> {
        if !snapshot.is_empty() {
            self.entries.push((heap, snapshot))?;
        }
        Ok(())
    }
    pub(super) fn sort(&mut self) {
        self.entries.sort_unstable_by_key(|(heap, _)| *heap);
    }
    pub(super) fn get(&self, heap: &u64) -> Option<&TimerSnapshot> {
        self.entries
            .binary_search_by_key(heap, |(heap, _)| *heap)
            .ok()
            .map(|index| &self.entries[index].1)
    }
    pub(super) fn get_mut(&mut self, heap: &u64) -> Option<&mut TimerSnapshot> {
        self.entries
            .binary_search_by_key(heap, |(heap, _)| *heap)
            .ok()
            .map(|index| &mut self.entries[index].1)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_task_storage.rs"]
mod ownership_tests;
