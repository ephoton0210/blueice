// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Disposal capabilities own records conditionally; frames own lexical scopes.

use super::*;

impl Vm {
    pub(super) fn disposal_storage_bytes(&self) -> usize {
        let mut bytes = host_scheduling::map_slots(&self.disposable_stacks)
            .saturating_add(host_scheduling::map_slots(&self.async_disposable_stacks))
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

    pub(super) fn insert_disposal_state(
        &mut self,
        id: ObjectId,
        state: DisposeCapabilityState,
        is_async: bool,
    ) -> Result<(), RuntimeError> {
        let map = if is_async {
            &self.async_disposable_stacks
        } else {
            &self.disposable_stacks
        };
        let growth = SchedulingMapCapacity::for_insert(map, self.host_scheduling.as_ref())?;
        self.admit_scheduling_work(HostSchedulingUsage::default().with_map_growth(&growth))?;
        let map = if is_async {
            &mut self.async_disposable_stacks
        } else {
            &mut self.disposable_stacks
        };
        growth.commit(map);
        map.insert(id, state);
        self.synchronize_scheduling_usage()
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
            if map.len().saturating_mul(4) < map.capacity() {
                map.shrink_to_fit();
            }
        }
    }
}
