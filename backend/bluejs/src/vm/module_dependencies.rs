// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Both directions of asynchronous module dependency edges have one owner.

use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/dependency_storage.rs"]
mod storage_tests;

use super::host_scheduling::SharedSchedulingCharge;
use super::import_waiter_storage::ImportDependencies;
use super::promise_queue_storage::{PreparedMap, PreparedVec};
use super::retained_control_map::RetainedControlMap;

pub(super) type PendingDependencyMap = RetainedControlMap<String, ImportDependencies>;
pub(super) type AsyncParentMap = RetainedControlMap<String, Vec<String>>;

struct ParentEdge {
    dependency: String,
    parent: String,
    key: Option<String>,
    growth: PreparedVec<String>,
}

struct PreparedDependencySet {
    replacement: Option<HashSet<String>>,
    charge: Option<SharedSchedulingCharge>,
    previous: (usize, usize),
    additional: usize,
}
impl PreparedDependencySet {
    fn prepare(
        values: Option<&ImportDependencies>,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let previous = values.map_or((0, 0), |set| (set.len(), set.capacity()));
        let mut prepared = Self {
            replacement: None,
            charge: None,
            previous,
            additional,
        };
        if additional <= previous.1 - previous.0 {
            return Ok(prepared);
        }
        let requested = previous
            .0
            .checked_add(additional)
            .ok_or_else(|| fail(scope))?;
        prepared.charge = charge(
            scope,
            requested.saturating_mul(std::mem::size_of::<String>()),
        )?;
        let mut data = HashSet::new();
        data.try_reserve(requested).map_err(|_| fail(scope))?;
        if let Some(charge) = &mut prepared.charge {
            charge.resize(data.capacity() * std::mem::size_of::<String>())?;
        }
        prepared.replacement = Some(data);
        Ok(prepared)
    }
    fn staging_bytes(&self) -> usize {
        self.replacement
            .as_ref()
            .map_or(0, |set| set.capacity() * std::mem::size_of::<String>())
    }
    fn commit(mut self, values: &mut ImportDependencies) {
        assert_eq!((values.len(), values.capacity()), self.previous);
        if let Some(data) = self.replacement.take() {
            values.replace_prepared(data);
        }
        assert!(
            self.additional <= values.capacity() - values.len(),
            "dependency slots are prepared"
        );
    }
}
fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
fn charge(
    scope: Option<&SchedulingScope>,
    bytes: usize,
) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
    scope.map(|scope| scope.shared_charge(bytes)).transpose()
}

/// All graph data precedes its leases so abandoned staging refunds after drop.
pub(super) struct PreparedDependencyBatch {
    input: Option<ImportDependencies>,
    edges: Vec<ParentEdge>,
    parent_key: Option<String>,
    forward: Option<PreparedMap<String, ImportDependencies>>,
    reverse: Option<PreparedMap<String, Vec<String>>>,
    set: Option<PreparedDependencySet>,
    input_charge: Option<SharedSchedulingCharge>,
    charge: Option<SharedSchedulingCharge>,
    pending_names: usize,
    copied_names: usize,
    vector_bytes: usize,
    pub(super) _peak_bytes: usize,
}
impl PreparedDependencyBatch {
    fn observe_peak(&mut self) {
        let bytes = self
            .input_charge
            .as_ref()
            .map_or(0, SharedSchedulingCharge::bytes)
            .saturating_add(
                self.charge
                    .as_ref()
                    .map_or(0, SharedSchedulingCharge::bytes),
            )
            .saturating_add(self.vector_bytes)
            .saturating_add(self.forward.as_ref().map_or(0, PreparedMap::staging_bytes))
            .saturating_add(self.reverse.as_ref().map_or(0, PreparedMap::staging_bytes))
            .saturating_add(
                self.set
                    .as_ref()
                    .map_or(0, PreparedDependencySet::staging_bytes),
            );
        self._peak_bytes = self._peak_bytes.max(bytes);
    }
    fn refresh_names(&mut self) -> Result<(), RuntimeError> {
        if let Some(charge) = &mut self.charge {
            charge.resize(
                host_scheduling::slots(&self.edges)
                    .saturating_add(self.pending_names)
                    .saturating_add(self.copied_names),
            )?;
        }
        self.observe_peak();
        Ok(())
    }
    fn copy_name(
        &mut self,
        name: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<String, RuntimeError> {
        let copy = module_referrers::copy_referrer(name, scope)?;
        self.pending_names -= name.len();
        self.copied_names = self.copied_names.saturating_add(copy.capacity());
        self.refresh_names()?;
        Ok(copy)
    }
    pub(super) fn publish(mut self, vm: &mut Vm, parent: &str) {
        self.forward
            .take()
            .expect("forward table prepared")
            .commit(&mut vm.module_pending_dependencies);
        self.reverse
            .take()
            .expect("reverse table prepared")
            .commit(&mut vm.module_async_parents);
        if let Some(key) = self.parent_key.take() {
            vm.module_pending_dependencies
                .insert(key, ImportDependencies::default());
        }
        self.set.take().expect("dependency set prepared").commit(
            vm.module_pending_dependencies
                .get_mut(parent)
                .expect("parent row exists"),
        );
        for edge in self.edges.drain(..) {
            let ParentEdge {
                dependency,
                parent: name,
                key,
                growth,
            } = edge;
            if let Some(key) = key {
                vm.module_async_parents.insert(key, Vec::new());
            }
            let values = vm
                .module_async_parents
                .get_mut(&dependency)
                .expect("reverse row exists");
            growth.commit(values);
            values.push(name);
            vm.module_pending_dependencies
                .get_mut(parent)
                .expect("parent row exists")
                .insert(dependency);
        }
        // Every field has adopted its data. Refund staging directly before the
        // synchronous ownership update, with no callback or allocation between.
        drop(self);
        vm.reconcile_scheduling_ownership();
    }
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
            .retained_bytes()
            .saturating_add(self.module_async_parents.retained_bytes());
        for (parent, dependencies) in self.module_pending_dependencies.iter() {
            bytes = bytes
                .saturating_add(parent.capacity())
                .saturating_add(dependencies.retained_bytes());
        }
        for (dependency, parents) in self.module_async_parents.iter() {
            bytes = bytes
                .saturating_add(dependency.capacity())
                .saturating_add(string_vector_bytes(parents));
        }
        bytes
    }

    pub(super) fn register_async_module_dependencies(
        &mut self,
        parent: &str,
        dependencies: Vec<String>,
    ) -> Result<(), RuntimeError> {
        let result = (|| {
            if let Some(prepared) = self.prepare_async_module_dependencies(parent, dependencies)? {
                prepared.publish(self, parent);
            }
            self.check_scheduling_failure()
        })();
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    pub(super) fn prepare_async_module_dependencies(
        &mut self,
        parent: &str,
        dependencies: Vec<String>,
    ) -> Result<Option<PreparedDependencyBatch>, RuntimeError> {
        // Consume a redundant batch without allocating scratch or retaining its
        // incoming buffers. No-op registration remains possible at full quota.
        if dependencies.iter().all(|name| {
            self.module_pending_dependencies
                .get(parent)
                .is_some_and(|set| set.contains(name))
        }) {
            drop(dependencies);
            self.synchronize_scheduling_usage()?;
            return Ok(None);
        }
        self.synchronize_scheduling_usage()?;
        let input_bytes = string_vector_bytes(&dependencies);
        let incoming = self.prepare_import_dependencies(dependencies)?;
        let (mut input, mut input_charge) = incoming.into_owned();
        let initial_peak = input_bytes.saturating_add(input.backing_bytes());
        input.retain(|name| {
            !self
                .module_pending_dependencies
                .get(parent)
                .is_some_and(|set| set.contains(name))
        });
        if let Some(charge) = &mut input_charge {
            charge.release_to(input.retained_bytes());
        }
        let count = input.len();
        let new_parent = !self.module_pending_dependencies.contains_key(parent);
        let new_reverse = input
            .iter()
            .filter(|name| !self.module_async_parents.contains_key(name.as_str()))
            .count();
        let names = count
            .saturating_add(usize::from(new_parent))
            .saturating_mul(parent.len())
            .saturating_add(
                input
                    .iter()
                    .filter(|name| !self.module_async_parents.contains_key(name.as_str()))
                    .map(String::len)
                    .fold(0usize, usize::saturating_add),
            );
        let scope = self.host_scheduling.as_ref();
        let mut batch = PreparedDependencyBatch {
            input: Some(input),
            edges: Vec::new(),
            parent_key: None,
            forward: None,
            reverse: None,
            set: None,
            input_charge,
            charge: None,
            pending_names: names,
            copied_names: 0,
            vector_bytes: 0,
            _peak_bytes: initial_peak,
        };
        batch.charge = charge(
            scope,
            count
                .saturating_mul(std::mem::size_of::<ParentEdge>())
                .saturating_add(names),
        )?;
        batch.observe_peak();
        batch
            .edges
            .try_reserve_exact(count)
            .map_err(|_| fail(scope))?;
        batch.refresh_names()?;
        if new_parent {
            let key = batch.copy_name(parent, scope)?;
            batch.parent_key = Some(key);
        }
        for dependency in batch.input.take().expect("input names prepared") {
            let key = if self.module_async_parents.contains_key(&dependency) {
                None
            } else {
                Some(batch.copy_name(&dependency, scope)?)
            };
            let name = batch.copy_name(parent, scope)?;
            let empty = Vec::new();
            let values = self.module_async_parents.get(&dependency).unwrap_or(&empty);
            let growth = PreparedVec::for_push(values, scope)?;
            batch.vector_bytes = batch.vector_bytes.saturating_add(growth.staging_bytes());
            batch.edges.push(ParentEdge {
                dependency,
                parent: name,
                key,
                growth,
            });
            batch.observe_peak();
        }
        // The input set/iterator is gone; only the moved dependency names remain.
        let names = batch
            .edges
            .iter()
            .map(|edge| edge.dependency.capacity())
            .fold(0usize, usize::saturating_add);
        if let Some(charge) = &mut batch.input_charge {
            charge.release_to(names);
        }
        batch.forward = Some(PreparedMap::for_additional(
            &self.module_pending_dependencies,
            usize::from(new_parent),
            scope,
        )?);
        batch.observe_peak();
        batch.reverse = Some(PreparedMap::for_additional(
            &self.module_async_parents,
            new_reverse,
            scope,
        )?);
        batch.observe_peak();
        batch.set = Some(PreparedDependencySet::prepare(
            self.module_pending_dependencies.get(parent),
            count,
            scope,
        )?);
        batch.observe_peak();
        Ok(Some(batch))
    }

    pub(super) fn remove_module_dependency_parent(&mut self, parent: &str) {
        if let Some(dependencies) = self.module_pending_dependencies.remove(parent) {
            for dependency in dependencies {
                if let Some(parents) = self.module_async_parents.get_mut(&dependency) {
                    parents.retain(|value| value != parent);
                    if parents.is_empty() {
                        self.module_async_parents.remove(&dependency);
                    }
                }
            }
        }
        self.reclaim_module_dependency_storage();
    }

    pub(super) fn reclaim_module_dependency_storage(&mut self) {
        self.reconcile_scheduling_ownership();
        if self.preserve_promise_table_capacity {
            return;
        }
        let mut scope = self.host_scheduling.as_mut();
        for set in self.module_pending_dependencies.values_mut() {
            let before = set.retained_bytes();
            let charge = set.reclaim(scope.as_deref());
            if let Some(scope) = scope.as_deref_mut() {
                scope.release_owned_payload(before - set.retained_bytes());
            }
            drop(charge);
        }
        for values in self.module_async_parents.values_mut() {
            let before = string_vector_bytes(values);
            let charge = reclaim_parent_vector(values, scope.as_deref());
            if let Some(scope) = scope.as_deref_mut() {
                scope.release_owned_payload(before - string_vector_bytes(values));
            }
            drop(charge);
        }
        let before = self.module_pending_dependencies.retained_bytes();
        let charge = self.module_pending_dependencies.reclaim(scope.as_deref());
        if let Some(scope) = scope.as_deref_mut() {
            scope.release_owned_payload(before - self.module_pending_dependencies.retained_bytes());
        }
        drop(charge);
        let before = self.module_async_parents.retained_bytes();
        let charge = self.module_async_parents.reclaim(scope.as_deref());
        if let Some(scope) = scope {
            scope.release_owned_payload(before - self.module_async_parents.retained_bytes());
        }
        drop(charge);
    }
}

fn reclaim_parent_vector(
    values: &mut Vec<String>,
    scope: Option<&SchedulingScope>,
) -> Option<SharedSchedulingCharge> {
    if values.is_empty() {
        *values = Vec::new();
        return None;
    }
    let old = host_scheduling::slots(values);
    let minimum = values.len().saturating_mul(std::mem::size_of::<String>());
    if minimum.saturating_mul(4) >= old {
        return None;
    }
    let mut charge = match scope {
        Some(scope) => Some(scope.try_reclamation_charge(minimum)?),
        None => None,
    };
    let mut replacement = Vec::new();
    replacement.try_reserve_exact(values.len()).ok()?;
    let actual = host_scheduling::slots(&replacement);
    if actual >= old {
        return None;
    }
    if let Some(charge) = &mut charge {
        if !charge.try_resize_for_reclamation(actual) {
            return None;
        }
    }
    replacement.append(values);
    *values = replacement;
    charge
}
