// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted realm discovery and flat collection metadata precede heap effects.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

struct MetadataStorage {
    scope: Option<SchedulingScope>,
    charge: Option<SharedSchedulingCharge>,
    bytes: usize,
    allow_growth: bool,
    needs_growth: bool,
}

impl MetadataStorage {
    fn new(scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        Ok(Self {
            scope: scope.map(SchedulingScope::child),
            charge: scope.map(|scope| scope.shared_charge(0)).transpose()?,
            bytes: 0,
            allow_growth: true,
            needs_growth: false,
        })
    }
    fn fail(&self) -> RuntimeError {
        self.scope
            .as_ref()
            .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
    }
    fn stage(&self, bytes: usize) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.scope
            .as_ref()
            .map(|scope| scope.shared_charge(bytes))
            .transpose()
    }
    fn adopt(
        &mut self,
        staged: Option<SharedSchedulingCharge>,
        old: usize,
        actual: usize,
    ) -> Result<(), RuntimeError> {
        if let (Some(charge), Some(staged)) = (&mut self.charge, staged) {
            charge.absorb(staged)?;
        }
        self.bytes = self.bytes.saturating_add(actual).saturating_sub(old);
        if let Some(charge) = &mut self.charge {
            charge.release_to(self.bytes);
        }
        Ok(())
    }
    fn reserve_vec<T>(
        &mut self,
        values: &mut Vec<T>,
        additional: usize,
    ) -> Result<(), RuntimeError> {
        let required = values.len().saturating_add(additional);
        if required <= values.capacity() {
            return Ok(());
        }
        if !self.allow_growth {
            self.needs_growth = true;
            return Err(RuntimeError::SchedulingLimit);
        }
        let capacity = required.max(values.capacity().saturating_mul(2)).max(4);
        let mut staged = self.stage(capacity.saturating_mul(std::mem::size_of::<T>()))?;
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(capacity)
            .map_err(|_| self.fail())?;
        let actual = host_scheduling::slots(&replacement);
        if let Some(charge) = &mut staged {
            charge.resize(actual)?;
        }
        let old = host_scheduling::slots(values);
        replacement.append(values);
        *values = replacement;
        self.adopt(staged, old, actual)
    }
    fn reserve_set<T: Eq + std::hash::Hash>(
        &mut self,
        values: &mut HashSet<T>,
        additional: usize,
    ) -> Result<(), RuntimeError> {
        let required = values.len().saturating_add(additional);
        if required <= values.capacity() {
            return Ok(());
        }
        if !self.allow_growth {
            self.needs_growth = true;
            return Err(RuntimeError::SchedulingLimit);
        }
        let mut staged = self.stage(required.saturating_mul(std::mem::size_of::<T>()))?;
        let mut replacement = HashSet::new();
        replacement.try_reserve(required).map_err(|_| self.fail())?;
        let actual = replacement
            .capacity()
            .saturating_mul(std::mem::size_of::<T>());
        if let Some(charge) = &mut staged {
            charge.resize(actual)?;
        }
        let old = values.capacity().saturating_mul(std::mem::size_of::<T>());
        replacement.extend(values.drain());
        *values = replacement;
        self.adopt(staged, old, actual)
    }
}

#[derive(Clone, Copy)]
pub(super) struct RealmInstance {
    pub(super) object: ObjectId,
    pub(super) child: u64,
    owner: usize,
}
#[derive(Clone, Copy)]
pub(super) struct RealmWrapper {
    pub(super) object: ObjectId,
    pub(super) home: u64,
    pub(super) target: ObjectId,
    root: RootId,
    pub(super) followed: bool,
}
pub(super) struct CollectionRealm {
    pub(super) heap: u64,
    pub(super) record: Option<ShadowRealmRecord>,
    pub(super) instances: Vec<RealmInstance>,
    pub(super) wrappers: Vec<RealmWrapper>,
    pub(super) ignored: HashSet<RootId>,
    pub(super) extra: Vec<ObjectId>,
    incoming: usize,
    pub(super) active: bool,
    // Collector-produced sets have a separate ownership boundary. Metadata
    // accounting here covers their inline headers, not their mark buckets.
    pub(super) live: Option<HashSet<ObjectId>>,
}
impl CollectionRealm {
    fn new(heap: u64, record: Option<ShadowRealmRecord>) -> Self {
        Self {
            heap,
            record,
            instances: Vec::new(),
            wrappers: Vec::new(),
            ignored: HashSet::new(),
            extra: Vec::new(),
            incoming: 0,
            active: false,
            live: None,
        }
    }
}

pub(super) struct RealmCollection {
    pub(super) nodes: Vec<CollectionRealm>,
    pub(super) len: usize,
    indices: Vec<(u64, usize)>,
    known_owners: Vec<usize>,
    seen: HashSet<u64>,
    // Drop all snapshot records and buffers before refunding their owner.
    storage: MetadataStorage,
}
impl RealmCollection {
    #[cfg(any(test, coverage))]
    pub(super) fn retained_bytes(&self) -> usize {
        self.storage.bytes
    }
    fn new(vm: &Vm) -> Result<Self, RuntimeError> {
        Ok(Self {
            nodes: Vec::new(),
            len: 0,
            indices: Vec::new(),
            known_owners: Vec::new(),
            seen: HashSet::new(),
            storage: MetadataStorage::new(vm.host_scheduling.as_ref())?,
        })
    }
    fn clear(&mut self) {
        for node in &mut self.nodes {
            node.record = None;
            node.instances.clear();
            node.wrappers.clear();
            node.ignored.clear();
            node.extra.clear();
            node.live = None;
            node.incoming = 0;
            node.active = false;
        }
        self.len = 0;
        self.indices.clear();
        self.known_owners.clear();
        self.seen.clear();
        self.storage.needs_growth = false;
    }
    fn push_node(
        &mut self,
        heap: u64,
        record: Option<ShadowRealmRecord>,
    ) -> Result<(), RuntimeError> {
        if self.len == self.nodes.len() {
            self.storage.reserve_vec(&mut self.nodes, 1)?;
            self.nodes.push(CollectionRealm::new(heap, record));
        } else {
            self.nodes[self.len].heap = heap;
            self.nodes[self.len].record = record;
        }
        self.len += 1;
        Ok(())
    }
    fn begin(&mut self, heap: u64) -> Result<(), RuntimeError> {
        self.storage.reserve_set(&mut self.seen, 1)?;
        self.push_node(heap, None)?;
        self.seen.insert(heap);
        Ok(())
    }
    fn add_record(&mut self, record: &ShadowRealmRecord) -> Result<(), RuntimeError> {
        if self.seen.contains(&record.heap) {
            return Ok(());
        }
        self.storage.reserve_set(&mut self.seen, 1)?;
        self.push_node(record.heap, Some(record.clone()))?;
        self.seen.insert(record.heap);
        Ok(())
    }
    fn snapshot(&mut self, index: usize, vm: &Vm) -> Result<(), RuntimeError> {
        let node = &mut self.nodes[index];
        self.storage
            .reserve_vec(&mut node.instances, vm.shadow_realms.len())?;
        self.storage
            .reserve_vec(&mut node.wrappers, vm.shadow_wrapped_functions.len())?;
        node.instances.extend(
            vm.shadow_realms
                .iter()
                .map(|(&object, record)| RealmInstance {
                    object,
                    child: record.heap,
                    owner: Rc::as_ptr(&record.owner) as usize,
                }),
        );
        node.wrappers.extend(
            vm.shadow_wrapped_functions
                .iter()
                .map(|(&object, wrapper)| RealmWrapper {
                    object,
                    home: wrapper.home_heap,
                    target: wrapper.target,
                    root: wrapper.target_root.id,
                    followed: false,
                }),
        );
        node.active = index == 0 || vm.shadow_has_work();
        Ok(())
    }
    pub(super) fn index_of(&self, heap: u64) -> Option<usize> {
        self.indices
            .binary_search_by_key(&heap, |&(heap, _)| heap)
            .ok()
            .map(|index| self.indices[index].1)
    }
    pub(super) fn is_active(&self, heap: u64) -> bool {
        self.index_of(heap)
            .is_some_and(|index| self.nodes[index].active)
    }
    fn finish_metadata(&mut self) -> Result<(), RuntimeError> {
        self.storage.reserve_vec(&mut self.indices, self.len)?;
        self.indices.extend(
            self.nodes[..self.len]
                .iter()
                .enumerate()
                .map(|(index, node)| (node.heap, index)),
        );
        self.indices.sort_unstable_by_key(|&(heap, _)| heap);
        let owners = self.nodes[..self.len]
            .iter()
            .map(|node| node.instances.len())
            .sum();
        self.storage.reserve_vec(&mut self.known_owners, owners)?;
        self.known_owners.extend(
            self.nodes[..self.len]
                .iter()
                .flat_map(|node| node.instances.iter().map(|instance| instance.owner)),
        );
        self.known_owners.sort_unstable();
        self.known_owners.dedup();
        for index in 0..self.len {
            for wrapper in 0..self.nodes[index].wrappers.len() {
                let home = self.nodes[index].wrappers[wrapper].home;
                if let Some(home) = self.index_of(home) {
                    self.nodes[home].incoming += 1;
                }
            }
        }
        for node in &mut self.nodes[..self.len] {
            self.storage.reserve_vec(&mut node.extra, node.incoming)?;
            self.storage.reserve_set(&mut node.ignored, node.incoming)?;
            if let Some(record) = &node.record {
                if record
                    .owners
                    .borrow()
                    .iter()
                    .filter(|owner| owner.strong_count() != 0)
                    .any(|owner| {
                        self.known_owners
                            .binary_search(&(owner.as_ptr() as usize))
                            .is_err()
                    })
                {
                    node.active = true;
                }
            }
        }
        for index in 0..self.len {
            for wrapper in 0..self.nodes[index].wrappers.len() {
                let wrapper = self.nodes[index].wrappers[wrapper];
                if let Some(home) = self.index_of(wrapper.home) {
                    self.nodes[home].ignored.insert(wrapper.root);
                }
            }
        }
        Ok(())
    }
}

impl Vm {
    fn fill_realm_collection(&self, plan: &mut RealmCollection) -> Result<bool, RuntimeError> {
        plan.begin(self.object_prototype.heap)?;
        for record in self.shadow_realms.values() {
            plan.add_record(record)?;
        }
        let mut index = 1;
        while index < plan.len {
            let record = plan.nodes[index]
                .record
                .as_ref()
                .expect("discovered child record")
                .clone();
            let Ok(vm) = record.vm.try_borrow_mut() else {
                return Ok(false);
            };
            for child in vm.shadow_realms.values() {
                plan.add_record(child)?;
            }
            plan.snapshot(index, &vm)?;
            index += 1;
        }
        plan.snapshot(0, self)?;
        plan.finish_metadata()?;
        Ok(true)
    }

    pub(super) fn recycle_realm_collection(&mut self, mut plan: RealmCollection) {
        plan.clear();
        // Empty graphs and host-neutral contexts release discovery storage.
        if self.host_scheduling.is_some()
            && (!self.shadow_realms.is_empty() || !self.shadow_wrapped_functions.is_empty())
        {
            self.realm_collection_cache = Some(plan);
        }
    }

    pub(super) fn prepare_realm_collection(
        &mut self,
    ) -> Result<Option<RealmCollection>, RuntimeError> {
        if self
            .shadow_realms
            .values()
            .filter(|record| record.heap != self.object_prototype.heap)
            .any(|record| record.vm.try_borrow_mut().is_err())
        {
            return Ok(None);
        }
        if self.check_scheduling_failure().is_ok() {
            self.synchronize_scheduling_usage()?;
        } else {
            self.reconcile_scheduling_ownership();
        }
        let mut old = self.realm_collection_cache.take();
        let reusable = old.as_ref().is_some_and(|plan| {
            plan.storage
                .scope
                .as_ref()
                .zip(self.host_scheduling.as_ref())
                .is_some_and(|(owner, current)| owner.same_document(current))
        });
        if reusable {
            let mut plan = old.take().expect("checked cached storage");
            plan.storage.allow_growth = false;
            let result = self.fill_realm_collection(&mut plan);
            let needs_growth = plan.storage.needs_growth;
            match result {
                Ok(true) => return Ok(Some(plan)),
                Ok(false) => {
                    self.recycle_realm_collection(plan);
                    return Ok(None);
                }
                Err(error) if !needs_growth => {
                    self.recycle_realm_collection(plan);
                    return Err(error);
                }
                Err(_) => {
                    plan.clear();
                    old = Some(plan);
                }
            }
        }
        // A replacement coexists with the old cache until the complete plan
        // succeeds. Any discovery/admission failure refunds only the candidate.
        let prepared = (|| {
            let mut plan = RealmCollection::new(self)?;
            if !self.fill_realm_collection(&mut plan)? {
                return Ok(None);
            }
            Ok(Some(plan))
        })();
        match prepared {
            Ok(Some(plan)) => {
                drop(old);
                Ok(Some(plan))
            }
            Ok(None) => {
                self.realm_collection_cache = old;
                Ok(None)
            }
            Err(error) => {
                self.realm_collection_cache = old;
                Err(error)
            }
        }
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/realm_collection.rs"]
mod ownership_tests;
