// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Both directions of asynchronous module dependency edges have one owner.

use super::*;

struct ParentEdge {
    dependency: String,
    parent: String,
    key: Option<String>,
    growth: SchedulingCapacity,
    values: Vec<String>,
}

pub(super) fn string_vector_bytes(values: &Vec<String>) -> usize {
    values.iter().fold(
        values
            .capacity()
            .saturating_mul(std::mem::size_of::<String>()),
        |bytes, value| bytes.saturating_add(value.capacity()),
    )
}

pub(super) fn string_queue_bytes(values: &VecDeque<String>) -> usize {
    values.iter().fold(
        values
            .capacity()
            .saturating_mul(std::mem::size_of::<String>()),
        |bytes, value| bytes.saturating_add(value.capacity()),
    )
}

pub(super) fn string_iterator_bytes(capacity: usize, values: &[String]) -> usize {
    values.iter().fold(
        capacity.saturating_mul(std::mem::size_of::<String>()),
        |bytes, value| bytes.saturating_add(value.capacity()),
    )
}

impl Vm {
    pub(super) fn module_dependency_bytes(&self) -> usize {
        let mut bytes = self
            .module_pending_dependencies
            .capacity()
            .saturating_mul(std::mem::size_of::<(String, HashSet<String>)>())
            .saturating_add(
                self.module_async_parents
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(String, Vec<String>)>()),
            );
        for (parent, dependencies) in &self.module_pending_dependencies {
            bytes = bytes.saturating_add(parent.capacity()).saturating_add(
                dependencies
                    .capacity()
                    .saturating_mul(std::mem::size_of::<String>()),
            );
            for dependency in dependencies {
                bytes = bytes.saturating_add(dependency.capacity());
            }
        }
        for (dependency, parents) in &self.module_async_parents {
            bytes = bytes
                .saturating_add(dependency.capacity())
                .saturating_add(string_vector_bytes(parents));
        }
        bytes
    }

    pub(super) fn register_async_module_dependencies(
        &mut self,
        parent: &str,
        mut dependencies: Vec<String>,
    ) -> Result<(), RuntimeError> {
        self.check_scheduling_failure()?;
        let indices = {
            let existing = self.module_pending_dependencies.get(parent);
            let mut seen = HashSet::new();
            dependencies
                .iter()
                .enumerate()
                .filter_map(|(index, dependency)| {
                    (seen.insert(dependency.as_str())
                        && !existing.is_some_and(|set| set.contains(dependency)))
                    .then_some(index)
                })
                .collect::<Vec<_>>()
        };
        if indices.is_empty() {
            return self.synchronize_scheduling_usage();
        }
        let new_parent =
            (!self.module_pending_dependencies.contains_key(parent)).then(|| parent.to_owned());
        let mut edges = Vec::with_capacity(indices.len());
        for index in indices {
            let dependency = std::mem::take(&mut dependencies[index]);
            let existing = self.module_async_parents.get(&dependency);
            let values = Vec::new();
            edges.push(ParentEdge {
                key: existing.is_none().then(|| dependency.clone()),
                growth: SchedulingCapacity::for_vec_push(existing.unwrap_or(&values)),
                dependency,
                parent: parent.to_owned(),
                values,
            });
        }
        let forward = SchedulingMapCapacity::for_additional(
            &self.module_pending_dependencies,
            usize::from(new_parent.is_some()),
            self.host_scheduling.as_ref(),
        )?;
        let reverse = SchedulingMapCapacity::for_additional(
            &self.module_async_parents,
            edges.iter().filter(|edge| edge.key.is_some()).count(),
            self.host_scheduling.as_ref(),
        )?;
        let existing = self.module_pending_dependencies.get(parent);
        let old_capacity = existing.map_or(0, HashSet::capacity);
        let required = existing.map_or(0, HashSet::len).saturating_add(edges.len());
        let mut replacement = if required > old_capacity {
            let mut set = HashSet::new();
            set.try_reserve(required).map_err(|_| {
                self.host_scheduling
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
            })?;
            Some(set)
        } else {
            None
        };
        let set_capacity = replacement.as_ref().map_or(old_capacity, HashSet::capacity);
        let mut data = set_capacity
            .saturating_sub(old_capacity)
            .saturating_mul(std::mem::size_of::<String>())
            .saturating_add(new_parent.as_ref().map_or(0, String::capacity));
        for edge in &edges {
            data = data
                .saturating_add(edge.dependency.capacity())
                .saturating_add(edge.parent.capacity())
                .saturating_add(edge.key.as_ref().map_or(0, String::capacity));
        }
        let extra = HostSchedulingUsage {
            payload_bytes: data,
            ..Default::default()
        }
        .with_map_growth(&forward)
        .with_map_growth(&reverse);
        let mut forecast = extra;
        for edge in &edges {
            forecast = forecast.with_growth(edge.growth);
        }
        self.admit_scheduling_work(forecast)?;
        for edge in &mut edges {
            if edge.key.is_some() {
                edge.growth.reserve_vec(&mut edge.values);
            } else {
                edge.growth
                    .reserve_vec(self.module_async_parents.get_mut(&edge.dependency).unwrap());
            }
        }
        // Existing reverse vectors are now visible to the ledger; new rows
        // and the forward set remain private until this actual-capacity check.
        let mut actual = extra;
        for edge in &edges {
            actual.payload_bytes = actual
                .payload_bytes
                .saturating_add(string_vector_bytes(&edge.values));
        }
        self.admit_scheduling_work(actual)?;
        forward.commit(&mut self.module_pending_dependencies);
        reverse.commit(&mut self.module_async_parents);
        if let Some(key) = new_parent {
            self.module_pending_dependencies.insert(key, HashSet::new());
        }
        let set = self.module_pending_dependencies.get_mut(parent).unwrap();
        if let Some(mut next) = replacement.take() {
            next.extend(set.drain());
            *set = next;
        }
        for mut edge in edges {
            if let Some(key) = edge.key {
                edge.values.push(edge.parent);
                self.module_async_parents.insert(key, edge.values);
            } else {
                self.module_async_parents
                    .get_mut(&edge.dependency)
                    .unwrap()
                    .push(edge.parent);
            }
            self.module_pending_dependencies
                .get_mut(parent)
                .unwrap()
                .insert(edge.dependency);
        }
        self.synchronize_scheduling_usage()
    }

    pub(super) fn remove_module_dependency_parent(&mut self, parent: &str) {
        if let Some(dependencies) = self.module_pending_dependencies.remove(parent) {
            for dependency in dependencies {
                if let Some(parents) = self.module_async_parents.get_mut(&dependency) {
                    parents.retain(|value| value != parent);
                    if parents.is_empty() {
                        self.module_async_parents.remove(&dependency);
                    } else if parents.len().saturating_mul(4) < parents.capacity() {
                        parents.shrink_to_fit();
                    }
                }
            }
        }
        self.reclaim_module_dependency_storage();
    }

    pub(super) fn reclaim_module_dependency_storage(&mut self) {
        if self.module_pending_dependencies.len().saturating_mul(4)
            < self.module_pending_dependencies.capacity()
        {
            self.module_pending_dependencies.shrink_to_fit();
        }
        if self.module_async_parents.len().saturating_mul(4) < self.module_async_parents.capacity()
        {
            self.module_async_parents.shrink_to_fit();
        }
    }
}
