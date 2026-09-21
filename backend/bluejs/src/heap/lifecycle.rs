// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Object prototype lifecycle, extensibility, and generational collection.

use super::*;

impl Heap {
    pub fn prototype(&self, object: ObjectId) -> Result<Option<ObjectId>, HeapError> {
        Ok(self.object(object)?.prototype)
    }

    pub fn prevent_extensions(&mut self, object: ObjectId) -> Result<(), HeapError> {
        self.objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .extensible = false;
        Ok(())
    }

    pub fn is_extensible(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(self.object(object)?.extensible)
    }

    /// Changes `[[Prototype]]` without collecting. Cycles and foreign/stale
    /// handles are rejected before changing either the object or barrier.
    pub fn set_prototype(
        &mut self,
        object: ObjectId,
        prototype: Option<ObjectId>,
    ) -> Result<(), HeapError> {
        self.object(object)?;
        if !self.object(object)?.extensible && self.object(object)?.prototype != prototype {
            return Err(HeapError::ReadOnlyProperty);
        }
        let mut current = prototype;
        while let Some(id) = current {
            if id == object {
                return Err(HeapError::PrototypeCycle);
            }
            current = self.object(id)?.prototype;
        }
        self.write_barrier(object, prototype);
        self.structure_epoch += 1;
        self.objects
            .get_mut(&object)
            .expect("validated receiver")
            .prototype = prototype;
        Ok(())
    }

    pub fn stats(&self) -> HeapStats {
        HeapStats {
            nursery_objects: self.nursery.len(),
            tenured_objects: self.objects.len() - self.nursery.len(),
            managed_bytes: self.managed_bytes,
            next_major_bytes: self.next_major_bytes,
            minor_collections: self.minor_collections,
            major_collections: self.major_collections,
            root_registrations: self.root_registrations,
        }
    }

    /// Reclaims unreachable young objects and promotes every survivor.
    /// Old objects are not reclaimed here; their current young edges are
    /// conservatively retained until a major collection proves them dead.
    pub fn collect_minor(&mut self) {
        self.minor_gc(&[]);
    }

    /// Reclaims unreachable objects in both generations, including cycles.
    /// This is also the future process-pressure monitor's collection hook.
    pub fn collect_major(&mut self) {
        self.major_gc(&[]);
    }

    pub(super) fn object(&self, id: ObjectId) -> Result<&Object, HeapError> {
        self.objects.get(&id).ok_or(HeapError::InvalidObject(id))
    }

    pub(super) fn write_barrier(&mut self, owner: ObjectId, target: Option<ObjectId>) {
        if let Some(target) = target {
            if !self.objects[&owner].young && self.objects[&target].young {
                self.remembered.insert(owner);
            }
        }
    }

    /// Registers a whole safepoint's temporary roots as one batch, which the
    /// next `pop_scoped_roots` releases. Unlike `root`, this does no per-object
    /// table work; an id that is not (or no longer) in this heap is ignored
    /// when marking, and is still reported by whichever operation uses it.
    pub(crate) fn push_scoped_roots(&mut self, roots: Vec<ObjectId>) {
        self.scoped_roots.push(roots);
    }

    pub(crate) fn pop_scoped_roots(&mut self) {
        self.scoped_roots.pop();
    }

    pub(super) fn ensure_room(
        &mut self,
        additional: usize,
        protected: &[ObjectId],
    ) -> Result<(), HeapError> {
        if additional == 0 {
            return Ok(());
        }
        let proposed = self.managed_bytes.checked_add(additional);
        if proposed.is_none_or(|bytes| bytes >= self.next_major_bytes) {
            self.major_gc(protected);
        }
        if self
            .managed_bytes
            .checked_add(additional)
            .is_none_or(|bytes| bytes > self.config.max_heap_bytes)
        {
            return Err(HeapError::HeapLimitExceeded {
                limit: self.config.max_heap_bytes,
            });
        }
        Ok(())
    }

    // Mark and sweep are separate phases, with an explicit worklist so
    // a deep user-created graph never turns into recursive Rust calls.
    pub(super) fn mark(&self, young_only: bool, protected: &[ObjectId]) -> HashSet<ObjectId> {
        let mut work: Vec<_> = self
            .roots
            .values()
            .copied()
            .chain(
                self.scoped_roots
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|id| self.objects.contains_key(id)),
            )
            .chain(protected.iter().copied())
            .collect();
        if young_only {
            for id in &self.remembered {
                work.extend(self.objects[id].references());
                if let Some(metadata) = self.closure_metadata.get(id) {
                    work.extend(metadata.references());
                }
            }
        }
        let mut marked = HashSet::new();
        while let Some(id) = work.pop() {
            let obj = &self.objects[&id];
            if (young_only && !obj.young) || !marked.insert(id) {
                continue;
            }
            work.extend(obj.references());
            if let Some(metadata) = self.closure_metadata.get(&id) {
                work.extend(metadata.references());
            }
        }
        // WeakMap/WeakSet entries are ephemerons, rather than ordinary
        // object edges. Once a live key has independently been marked, its
        // value joins the mark worklist; repeat to a fixed point because that
        // value may in turn make another key reachable. During a minor
        // collection old objects are retained conservatively, so old tables
        // and old keys participate even though only young objects are marked.
        loop {
            let mut ephemeron_work = Vec::new();
            for (owner, object) in &self.objects {
                let table_live = if young_only {
                    !object.young || marked.contains(owner)
                } else {
                    marked.contains(owner)
                };
                if !table_live {
                    continue;
                }
                let ObjectKind::WeakCollection { entries, .. } = &object.kind else {
                    continue;
                };
                for (key, value) in entries {
                    let key_live = match key {
                        WeakCollectionKey::Object(key) if young_only => self
                            .objects
                            .get(key)
                            .is_some_and(|key_object| !key_object.young || marked.contains(key)),
                        WeakCollectionKey::Object(key) => marked.contains(key),
                        // Symbols have identity but no heap record; a
                        // non-registered symbol key is therefore live while
                        // the table itself is reachable.
                        WeakCollectionKey::Symbol(_) => true,
                    };
                    if key_live {
                        if let Some(value) = value
                            .object_id()
                            .filter(|value| self.objects.contains_key(value))
                        {
                            ephemeron_work.push(value);
                        }
                    }
                }
            }
            let mut added = false;
            while let Some(id) = ephemeron_work.pop() {
                let obj = &self.objects[&id];
                if (young_only && !obj.young) || !marked.insert(id) {
                    continue;
                }
                added = true;
                ephemeron_work.extend(obj.references());
                if let Some(metadata) = self.closure_metadata.get(&id) {
                    ephemeron_work.extend(metadata.references());
                }
            }
            if !added {
                break;
            }
        }
        marked
    }

    pub(super) fn minor_gc(&mut self, protected: &[ObjectId]) {
        let marked = self.mark(true, protected);
        let young: HashSet<_> = self.nursery.iter().copied().collect();
        let mut released_weak_entries: usize = 0;
        for object in self.objects.values_mut() {
            if let ObjectKind::WeakCollection { entries, .. } = &mut object.kind {
                let mut released: usize = 0;
                entries.retain(|key, value| {
                    let keep = match key {
                        WeakCollectionKey::Object(key) => {
                            !young.contains(key) || marked.contains(key)
                        }
                        WeakCollectionKey::Symbol(_) => true,
                    };
                    if !keep {
                        released = released.saturating_add(weak_collection_entry_bytes(key, value));
                    }
                    keep
                });
                object.bytes -= released;
                released_weak_entries = released_weak_entries.saturating_add(released);
            }
            if let ObjectKind::WeakRef { target } = &mut object.kind {
                if target.as_ref().is_some_and(|target| {
                    matches!(target, WeakCollectionKey::Object(key) if young.contains(key) && !marked.contains(key))
                }) {
                    *target = None;
                }
            }
            if let ObjectKind::FinalizationRegistry { cells, .. } = &mut object.kind {
                for cell in cells {
                    if cell.target.as_ref().is_some_and(|target| {
                        matches!(target, WeakCollectionKey::Object(key) if young.contains(key) && !marked.contains(key))
                    }) {
                        cell.target = None;
                    }
                }
            }
        }
        self.managed_bytes -= released_weak_entries;
        for id in self.nursery.drain(..) {
            if marked.contains(&id) {
                self.objects
                    .get_mut(&id)
                    .expect("nursery handle is live")
                    .young = false;
            } else {
                self.managed_bytes -= self
                    .objects
                    .remove(&id)
                    .expect("nursery handle is live")
                    .bytes;
                if self.closure_metadata.remove(&id).is_some() {
                    self.managed_bytes -= CLOSURE_METADATA_BYTES;
                }
            }
        }
        self.remembered.clear();
        self.minor_collections += 1;
    }

    pub(super) fn major_gc(&mut self, protected: &[ObjectId]) {
        let marked = self.mark(false, protected);
        let mut released_weak_entries: usize = 0;
        for object in self.objects.values_mut() {
            if let ObjectKind::WeakCollection { entries, .. } = &mut object.kind {
                let mut released: usize = 0;
                entries.retain(|key, value| {
                    let keep = match key {
                        WeakCollectionKey::Object(key) => marked.contains(key),
                        WeakCollectionKey::Symbol(_) => true,
                    };
                    if !keep {
                        released = released.saturating_add(weak_collection_entry_bytes(key, value));
                    }
                    keep
                });
                object.bytes -= released;
                released_weak_entries = released_weak_entries.saturating_add(released);
            }
            if let ObjectKind::WeakRef { target } = &mut object.kind {
                if target.as_ref().is_some_and(
                    |target| matches!(target, WeakCollectionKey::Object(key) if !marked.contains(key)),
                ) {
                    *target = None;
                }
            }
            if let ObjectKind::FinalizationRegistry { cells, .. } = &mut object.kind {
                for cell in cells {
                    if cell.target.as_ref().is_some_and(
                        |target| matches!(target, WeakCollectionKey::Object(key) if !marked.contains(key)),
                    ) {
                        cell.target = None;
                    }
                }
            }
        }
        self.managed_bytes -= released_weak_entries;
        let reclaimed_metadata = self
            .closure_metadata
            .keys()
            .filter(|id| !marked.contains(id))
            .count();
        self.objects.retain(|id, obj| {
            if marked.contains(id) {
                obj.young = false;
                true
            } else {
                self.managed_bytes -= obj.bytes;
                false
            }
        });
        self.closure_metadata.retain(|id, _| marked.contains(id));
        self.managed_bytes -= reclaimed_metadata * CLOSURE_METADATA_BYTES;
        self.nursery.clear();
        self.remembered.clear();
        self.next_major_bytes = self
            .managed_bytes
            .saturating_mul(2)
            .max(self.config.major_threshold_bytes)
            .min(self.config.max_heap_bytes);
        self.major_collections += 1;
    }
}
