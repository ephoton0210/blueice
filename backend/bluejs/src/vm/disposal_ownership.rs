// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Disposal capabilities own records conditionally; frames own lexical scopes.

use super::*;

impl Vm {
    pub(super) fn disposal_storage_bytes(&self) -> usize {
        let mut bytes = self
            .disposable_stacks
            .retained_bytes()
            .saturating_add(self.async_disposable_stacks.retained_bytes())
            .saturating_add(self.disposables.unleased_bytes())
            .saturating_add(self.dispose_marks.unleased_bytes());
        for state in self
            .disposable_stacks
            .values()
            .chain(self.async_disposable_stacks.values())
        {
            bytes = bytes.saturating_add(state.resources.unleased_bytes());
        }
        bytes
    }

    pub(super) fn push_disposal_mark(&mut self) -> Result<(), RuntimeError> {
        let mut marks = std::mem::take(&mut self.dispose_marks);
        let result = marks.try_push(self, self.disposables.len());
        self.dispose_marks = marks;
        result
    }

    pub(super) fn take_disposal_resources(&mut self) -> Result<ResourceRecords, RuntimeError> {
        let mark = self
            .dispose_marks
            .pop()
            .expect("compiler pairs each disposal scope");
        let mut records = std::mem::take(&mut self.disposables);
        let result = records.try_split_off(self, mark);
        self.disposables = records;
        result
    }

    /// After observable prototype resolution, prepare a fresh brand record or
    /// transfer an already-validated source through one native publication.
    pub(super) fn allocate_disposal_capability(
        &mut self,
        prototype: ObjectId,
        is_async: bool,
        source: Option<ObjectId>,
    ) -> Result<ObjectId, RuntimeError> {
        let before = self.heap.stats();
        let result = self.allocate_disposal_capability_inner(prototype, is_async, source);
        // All table candidates have committed or dropped before deferred GC
        // reclamation. No managed collection runs in this shared finisher.
        self.finish_promise_record_attempt(before);
        result
    }

    fn allocate_disposal_capability_inner(
        &mut self,
        prototype: ObjectId,
        is_async: bool,
        source: Option<ObjectId>,
    ) -> Result<ObjectId, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let map = if is_async {
            &self.async_disposable_stacks
        } else {
            &self.disposable_stacks
        };
        let mut growth = Some(promise_queue_storage::PreparedMap::for_insert(
            map,
            self.host_scheduling.as_ref(),
        )?);
        // A move source and its resource edges must survive allocation even
        // when an internal caller has not installed ordinary call-entry roots.
        let root_base = self.async_frame_roots.len();
        self.extend_async_frame_roots(source.into_iter())?;
        let previous = std::mem::replace(&mut self.preserve_promise_table_capacity, true);
        let result = (|| {
            let id = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.check_scheduling_failure()?;
            let map = if is_async {
                &mut self.async_disposable_stacks
            } else {
                &mut self.disposable_stacks
            };
            growth
                .take()
                .expect("native table storage was prepared")
                .commit_after_pruning(map);
            let state = if let Some(source) = source {
                let state = map
                    .get_mut(&source)
                    .expect("the move source was rooted across allocation");
                let resources = std::mem::take(&mut state.resources);
                state.disposed = true;
                DisposeCapabilityState {
                    resources,
                    disposed: false,
                }
            } else {
                DisposeCapabilityState::default()
            };
            map.insert(id, state);
            Ok(id)
        })();
        drop(growth);
        self.async_frame_roots.truncate(root_base);
        self.preserve_promise_table_capacity = previous;
        result
    }

    pub(super) fn append_disposal_resource(
        &mut self,
        id: ObjectId,
        resource: DisposableResource,
        is_async: bool,
    ) -> Result<(), RuntimeError> {
        let map = if is_async {
            &mut self.async_disposable_stacks
        } else {
            &mut self.disposable_stacks
        };
        let mut records = std::mem::take(
            &mut map
                .get_mut(&id)
                .expect("validated disposal capability")
                .resources,
        );
        let result = records.try_push(self, resource);
        let map = if is_async {
            &mut self.async_disposable_stacks
        } else {
            &mut self.disposable_stacks
        };
        map.get_mut(&id)
            .expect("native admission runs no JavaScript")
            .resources = records;
        result
    }

    pub(super) fn disposal_owned_edges(&self) -> impl Iterator<Item = (ObjectId, ObjectId)> + '_ {
        self.disposable_stacks
            .iter()
            .chain(self.async_disposable_stacks.iter())
            .flat_map(|(&id, state)| {
                state
                    .resources
                    .iter()
                    .flat_map(DisposableResource::values)
                    .filter_map(Value::object_id)
                    .map(move |reference| (id, reference))
            })
    }

    pub(super) fn prune_disposal_state(&mut self) {
        for map in [
            &mut self.disposable_stacks,
            &mut self.async_disposable_stacks,
        ] {
            map.retain(|id, _| self.heap.contains(*id));
        }
    }
}
