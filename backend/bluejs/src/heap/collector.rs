// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted reusable marking storage and owner-carrying mark results.

use super::*;
use crate::vm::{HeapNativeBudget, HeapNativeCharge};
use std::ops::Deref;

const NO_WAITER: usize = usize::MAX;
struct Waiter {
    target: ObjectId,
    next: usize,
}

#[derive(Clone, Copy)]
pub(super) enum ProtectedRoots<'a> {
    Slice(&'a [ObjectId]),
    Allocation(&'a ObjectKind, Option<ObjectId>),
    Namespace(ObjectId, &'a [(JsString, ObjectId)]),
}
impl ProtectedRoots<'_> {
    fn visit(self, visit: &mut dyn FnMut(ObjectId)) {
        match self {
            Self::Slice(ids) => ids.iter().copied().for_each(visit),
            Self::Allocation(kind, prototype) => {
                prototype.into_iter().for_each(&mut *visit);
                visit_kind_references(kind, visit);
            }
            Self::Namespace(namespace, exports) => {
                visit(namespace);
                exports.iter().map(|(_, cell)| *cell).for_each(visit);
            }
        }
    }
}

pub(crate) struct CollectorMarks {
    heap: u64,
    marked: HashSet<ObjectId>,
    charge: Option<HeapNativeCharge>,
    budget: Option<HeapNativeBudget>,
}
impl Deref for CollectorMarks {
    type Target = HashSet<ObjectId>;
    fn deref(&self) -> &Self::Target {
        &self.marked
    }
}

#[derive(Default)]
pub(super) struct CollectorWorkspace {
    work: Vec<ObjectId>,
    waiters: Vec<Waiter>,
    heads: HashMap<ObjectId, usize>,
    marked: HashSet<ObjectId>,
    budget: Option<HeapNativeBudget>,
    charge: Option<HeapNativeCharge>,
}
fn vec_bytes<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(size_of::<T>())
}
fn vec_capacity<T>(values: &Vec<T>, required: usize) -> Option<usize> {
    (required > values.capacity()).then(|| required.max(values.capacity().saturating_mul(2)).max(4))
}
fn new_vec<T>(capacity: Option<usize>) -> Result<Option<Vec<T>>, HeapError> {
    capacity
        .map(|capacity| {
            let mut values = Vec::new();
            values
                .try_reserve_exact(capacity)
                .map_err(|_| HeapError::CollectorStorageLimit)?;
            Ok(values)
        })
        .transpose()
}
fn new_set<T: Eq + Hash>(capacity: Option<usize>) -> Result<Option<HashSet<T>>, HeapError> {
    capacity
        .map(|capacity| {
            let mut values = HashSet::new();
            values
                .try_reserve(capacity)
                .map_err(|_| HeapError::CollectorStorageLimit)?;
            Ok(values)
        })
        .transpose()
}
fn new_map<K: Eq + Hash, V>(capacity: Option<usize>) -> Result<Option<HashMap<K, V>>, HeapError> {
    capacity
        .map(|capacity| {
            let mut values = HashMap::new();
            values
                .try_reserve(capacity)
                .map_err(|_| HeapError::CollectorStorageLimit)?;
            Ok(values)
        })
        .transpose()
}

impl CollectorWorkspace {
    fn retained_bytes(&self) -> usize {
        vec_bytes(&self.work)
            .saturating_add(vec_bytes(&self.waiters))
            .saturating_add(
                self.heads
                    .capacity()
                    .saturating_mul(size_of::<(ObjectId, usize)>()),
            )
            .saturating_add(self.marked.capacity().saturating_mul(size_of::<ObjectId>()))
    }
    fn fail(&self) -> HeapError {
        if let Some(budget) = &self.budget {
            let _ = budget.fail();
        }
        HeapError::CollectorStorageLimit
    }
    fn clear(&mut self) {
        self.work.clear();
        self.waiters.clear();
        self.heads.clear();
        self.marked.clear();
    }
    fn prepare(&mut self, objects: usize, edges: usize) -> Result<(), HeapError> {
        let work = vec_capacity(&self.work, objects);
        let waiters = vec_capacity(&self.waiters, edges);
        let marked = (objects > self.marked.capacity()).then_some(objects);
        let keys = objects.min(edges);
        let heads = (keys > self.heads.capacity()).then_some(keys);
        if work.is_none() && waiters.is_none() && marked.is_none() && heads.is_none() {
            return Ok(());
        }
        let forecast = work
            .unwrap_or(0)
            .saturating_mul(size_of::<ObjectId>())
            .saturating_add(waiters.unwrap_or(0).saturating_mul(size_of::<Waiter>()))
            .saturating_add(marked.unwrap_or(0).saturating_mul(size_of::<ObjectId>()))
            .saturating_add(
                heads
                    .unwrap_or(0)
                    .saturating_mul(size_of::<(ObjectId, usize)>()),
            );
        let mut staged = self
            .budget
            .as_ref()
            .map(|budget| {
                budget
                    .reserve(forecast)
                    .map_err(|_| HeapError::CollectorStorageLimit)
            })
            .transpose()?;
        let replacements = (|| {
            Ok::<_, HeapError>((
                new_vec::<ObjectId>(work)?,
                new_vec::<Waiter>(waiters)?,
                new_set::<ObjectId>(marked)?,
                new_map::<ObjectId, usize>(heads)?,
            ))
        })();
        let (new_work, new_waiters, new_marked, new_heads) =
            replacements.map_err(|_| self.fail())?;
        let actual = new_work
            .as_ref()
            .map_or(0, vec_bytes)
            .saturating_add(new_waiters.as_ref().map_or(0, vec_bytes))
            .saturating_add(
                new_marked
                    .as_ref()
                    .map_or(0, |values| values.capacity() * size_of::<ObjectId>()),
            )
            .saturating_add(new_heads.as_ref().map_or(0, |values| {
                values.capacity() * size_of::<(ObjectId, usize)>()
            }));
        if let Some(charge) = &mut staged {
            charge
                .resize(actual)
                .map_err(|_| HeapError::CollectorStorageLimit)?;
        }
        if let (Some(charge), Some(staged)) = (&mut self.charge, staged) {
            charge
                .absorb(staged)
                .map_err(|_| HeapError::CollectorStorageLimit)?;
        }
        if let Some(values) = new_work {
            self.work = values;
        }
        if let Some(values) = new_waiters {
            self.waiters = values;
        }
        if let Some(values) = new_marked {
            self.marked = values;
        }
        if let Some(values) = new_heads {
            self.heads = values;
        }
        let bytes = self.retained_bytes();
        if let Some(charge) = &mut self.charge {
            charge.release_to(bytes);
        }
        Ok(())
    }
    fn enqueue(&mut self, objects: &HashMap<ObjectId, Object>, young: bool, id: ObjectId) {
        let Some(object) = objects.get(&id) else {
            return;
        };
        if young && !object.young {
            return;
        }
        // Inserting an existing key into a full hash table can grow it before
        // discovering the duplicate. Check first so marking never reallocates.
        if !self.marked.contains(&id) {
            assert!(self.marked.len() < self.marked.capacity());
            self.marked.insert(id);
            assert!(
                self.work.len() < self.work.capacity(),
                "one work item per heap object"
            );
            self.work.push(id);
        }
    }
    fn key_live(&self, objects: &HashMap<ObjectId, Object>, young: bool, key: ObjectId) -> bool {
        objects
            .get(&key)
            .is_some_and(|object| (young && !object.young) || self.marked.contains(&key))
    }
    fn conditional(
        &mut self,
        objects: &HashMap<ObjectId, Object>,
        young: bool,
        key: ObjectId,
        target: ObjectId,
    ) {
        if !objects.contains_key(&target) {
            return;
        }
        if self.key_live(objects, young, key) {
            self.enqueue(objects, young, target);
        } else if objects.contains_key(&key) {
            assert!(
                self.waiters.len() < self.waiters.capacity(),
                "conditional edge bound prepared before marking"
            );
            let index = self.waiters.len();
            let next = if let Some(head) = self.heads.get_mut(&key) {
                std::mem::replace(head, index)
            } else {
                assert!(self.heads.len() < self.heads.capacity());
                self.heads.insert(key, index);
                NO_WAITER
            };
            self.waiters.push(Waiter { target, next });
        }
    }
    fn activate(
        &mut self,
        objects: &HashMap<ObjectId, Object>,
        young: bool,
        entries: &HashMap<WeakCollectionKey, Value>,
    ) {
        for (key, value) in entries {
            let Some(target) = value.object_id() else {
                continue;
            };
            match key {
                WeakCollectionKey::Object(key) => self.conditional(objects, young, *key, target),
                _ => self.enqueue(objects, young, target),
            }
        }
    }
    fn take_marks(&mut self, heap: u64) -> CollectorMarks {
        let marked = std::mem::take(&mut self.marked);
        let bytes = marked.capacity() * size_of::<ObjectId>();
        let charge = self.charge.as_mut().map(|charge| charge.split_off(bytes));
        CollectorMarks {
            heap,
            marked,
            charge,
            budget: self.budget.clone(),
        }
    }
    fn recycle(&mut self, mut marks: CollectorMarks) -> Result<(), HeapError> {
        if self.marked.capacity() >= marks.marked.capacity() {
            return Ok(());
        }
        if let (Some(owner), Some(incoming)) = (&self.budget, &marks.budget) {
            if !owner.same_document(incoming) {
                return Err(HeapError::InvalidConfig);
            }
        }
        if let (Some(budget), None) = (&self.budget, &marks.charge) {
            marks.charge = Some(
                budget
                    .reserve(marks.marked.capacity() * size_of::<ObjectId>())
                    .map_err(|_| HeapError::CollectorStorageLimit)?,
            );
        }
        marks.marked.clear();
        if let (Some(charge), Some(incoming)) = (&mut self.charge, marks.charge.take()) {
            charge
                .absorb(incoming)
                .map_err(|_| HeapError::CollectorStorageLimit)?;
        }
        self.marked = marks.marked;
        let bytes = self.retained_bytes();
        if let Some(charge) = &mut self.charge {
            charge.release_to(bytes);
        }
        Ok(())
    }
}

impl Heap {
    pub(super) fn attach_code_sources(&self, code: &SharedBytecode) -> Result<(), HeapError> {
        if let Some(budget) = &self.collector.budget {
            code.attach_runtime_sources(budget)
                .map_err(|_| HeapError::CodeSourceStorageLimit)?;
        }
        Ok(())
    }

    pub(crate) fn bind_native_budget(&mut self, budget: HeapNativeBudget) -> Result<(), HeapError> {
        match (
            self.roots.budget(),
            self.collector.budget.as_ref(),
            self.realm_pins.budget(),
            self.pending_finalization.budget(),
        ) {
            (Some(roots), Some(collector), Some(pins), Some(finalization)) => {
                return if roots.same_document(&budget)
                    && collector.same_document(&budget)
                    && pins.same_document(&budget)
                    && finalization.same_document(&budget)
                {
                    Ok(())
                } else {
                    Err(HeapError::InvalidConfig)
                };
            }
            (None, None, None, None) => {}
            _ => return Err(HeapError::InvalidConfig),
        }
        // Closures/generators can outlive an unbounded registry. Bind their code
        // before admitting execution in a newly bounded document.
        for object in self.objects.values() {
            let code = match &object.kind {
                ObjectKind::Closure { code, .. } => Some(code),
                ObjectKind::Generator { state, .. } => match &**state {
                    GeneratorState::Start { code, .. } | GeneratorState::Suspended { code, .. } => {
                        Some(code)
                    }
                    GeneratorState::Running | GeneratorState::Done => None,
                },
                _ => None,
            };
            if let Some(code) = code {
                code.attach_runtime_sources(&budget)
                    .map_err(|_| HeapError::CodeSourceStorageLimit)?;
            }
        }
        // Install no owner until root, collector, pin and ready-index storage fit. Root
        // registrations and handles survive refused activation; only unadmitted
        // collector scratch may be discarded.
        let root_charge = match budget.reserve(self.roots.retained_bytes()) {
            Ok(charge) => charge,
            Err(_) => {
                self.collector = CollectorWorkspace::default();
                return Err(HeapError::RootStorageLimit);
            }
        };
        let collector_charge = match budget.reserve(self.collector.retained_bytes()) {
            Ok(charge) => charge,
            Err(_) => {
                self.collector = CollectorWorkspace::default();
                return Err(HeapError::CollectorStorageLimit);
            }
        };
        let pin_charge = match budget.reserve(self.realm_pins.retained_bytes()) {
            Ok(charge) => charge,
            Err(_) => {
                self.collector = CollectorWorkspace::default();
                return Err(HeapError::RootStorageLimit);
            }
        };
        let finalization_charge = match budget.reserve(self.pending_finalization.retained_bytes()) {
            Ok(charge) => charge,
            Err(_) => {
                self.collector = CollectorWorkspace::default();
                return Err(HeapError::CollectorStorageLimit);
            }
        };
        self.pending_finalization
            .bind_admitted(budget.clone(), finalization_charge);
        self.roots.bind_admitted(budget.clone(), root_charge);
        self.realm_pins.bind_admitted(budget.clone(), pin_charge);
        self.collector.charge = Some(collector_charge);
        self.collector.budget = Some(budget);
        Ok(())
    }

    pub(crate) fn take_collection_error(&mut self) -> Option<HeapError> {
        self.collection_error.take()
    }
    pub(crate) fn recycle_collector_marks(
        &mut self,
        marks: CollectorMarks,
    ) -> Result<(), HeapError> {
        if marks.heap != self.identity {
            return Err(HeapError::InvalidConfig);
        }
        self.collector.recycle(marks)
    }
    #[cfg(any(test, coverage))]
    pub(crate) fn collector_storage_bytes(&self) -> usize {
        self.collector.retained_bytes()
    }

    pub(super) fn mark_without_roots(
        &mut self,
        young: bool,
        protected: ProtectedRoots<'_>,
        ignored: &HashSet<RootId>,
    ) -> Result<CollectorMarks, HeapError> {
        let edges = self
            .scoped_trace
            .as_ref()
            .map_or(0, |batch| batch.edges.len())
            .saturating_add(
                self.objects
                    .values()
                    .map(|object| match &object.kind {
                        ObjectKind::WeakCollection { entries, .. } => entries
                            .values()
                            .filter(|value| value.object_id().is_some())
                            .count(),
                        ObjectKind::WebAbortSignal(state)
                            if !state.aborted() && state.dependent && state.observed() =>
                        {
                            state.sources.len()
                        }
                        _ => 0,
                    })
                    .fold(0, usize::saturating_add),
            );
        let mut workspace = std::mem::take(&mut self.collector);
        if let Err(error) = workspace.prepare(self.objects.len(), edges) {
            self.collector = workspace;
            return Err(error);
        }
        workspace.clear();
        for (root, id) in self.roots.iter() {
            if !ignored.contains(root) && !self.realm_pins.is_released(root) {
                workspace.enqueue(&self.objects, young, *id);
            }
        }
        if let Some(batch) = &self.scoped_trace {
            for id in &batch.roots {
                workspace.enqueue(&self.objects, young, *id);
            }
        }
        protected.visit(&mut |id| workspace.enqueue(&self.objects, young, id));
        if young {
            for id in &self.remembered {
                if let Some(object) = self.objects.get(id) {
                    object.visit_references(&mut |id| workspace.enqueue(&self.objects, young, id));
                }
                if let Some(metadata) = self.closure_metadata.get(id) {
                    metadata
                        .references()
                        .for_each(|id| workspace.enqueue(&self.objects, young, id));
                }
            }
        }
        if let Some(batch) = &self.scoped_trace {
            for &(owner, target) in &batch.edges {
                workspace.conditional(&self.objects, young, owner, target);
            }
        }
        for (id, object) in &self.objects {
            if let ObjectKind::WeakCollection { entries, .. } = &object.kind {
                if young && !object.young {
                    workspace.activate(&self.objects, young, entries);
                }
            }
            if let ObjectKind::WebAbortSignal(state) = &object.kind {
                if !state.aborted() && state.dependent && state.observed() {
                    for source in &state.sources {
                        workspace.conditional(&self.objects, young, *source, *id);
                    }
                }
            }
        }
        while let Some(id) = workspace.work.pop() {
            let object = &self.objects[&id];
            object.visit_references(&mut |id| workspace.enqueue(&self.objects, young, id));
            if let Some(metadata) = self.closure_metadata.get(&id) {
                metadata
                    .references()
                    .for_each(|id| workspace.enqueue(&self.objects, young, id));
            }
            if let ObjectKind::WeakCollection { entries, .. } = &object.kind {
                workspace.activate(&self.objects, young, entries);
            }
            // Keep keys until the cache is cleared: removing entries can leave
            // tombstones and force later new keys to rehash during marking.
            let mut waiter = workspace
                .heads
                .get_mut(&id)
                .map_or(NO_WAITER, |head| std::mem::replace(head, NO_WAITER));
            while waiter != NO_WAITER {
                let slot = &workspace.waiters[waiter];
                let target = slot.target;
                waiter = slot.next;
                workspace.enqueue(&self.objects, young, target);
            }
        }
        let marks = workspace.take_marks(self.identity);
        workspace.clear();
        self.collector = workspace;
        Ok(marks)
    }
}
