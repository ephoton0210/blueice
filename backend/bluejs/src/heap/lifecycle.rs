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
        let entry = self.object(object)?;
        if !entry.extensible && entry.prototype != prototype {
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
            managed_bytes: self.managed_bytes.get(),
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
        if let Err(error) = self.try_collect_minor() {
            self.collection_error = Some(error);
        }
    }

    /// Reclaims unreachable objects in both generations, including cycles.
    /// This is also the future process-pressure monitor's collection hook.
    pub fn collect_major(&mut self) {
        if let Err(error) = self.try_collect_major() {
            self.collection_error = Some(error);
        }
    }

    /// Reclaims the nursery after admitting all native marking storage.
    pub fn try_collect_minor(&mut self) -> Result<(), HeapError> {
        self.minor_gc(ProtectedRoots::Slice(&[]))
    }

    /// Reclaims both generations after admitting all native marking storage.
    pub fn try_collect_major(&mut self) -> Result<(), HeapError> {
        self.major_gc(ProtectedRoots::Slice(&[]))
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
    pub(crate) fn push_scoped_roots(&mut self, batch: GcTraceBatch) {
        assert!(
            self.scoped_trace.is_none(),
            "heap operations cannot reenter the VM"
        );
        self.scoped_trace = Some(batch);
    }

    pub(crate) fn pop_scoped_roots(&mut self) -> GcTraceBatch {
        self.scoped_trace
            .take()
            .expect("the safepoint batch is still attached")
    }

    pub(super) fn ensure_room(
        &mut self,
        additional: usize,
        protected: &[ObjectId],
    ) -> Result<(), HeapError> {
        self.ensure_room_protected(additional, ProtectedRoots::Slice(protected))
    }

    pub(super) fn ensure_room_protected(
        &mut self,
        additional: usize,
        protected: ProtectedRoots<'_>,
    ) -> Result<(), HeapError> {
        if let Some(error) = &self.collection_error {
            return Err(*error);
        }
        if additional == 0 {
            self.managed_bytes.check_room(0)?;
            return Ok(());
        }
        let proposed = self.managed_bytes.get().checked_add(additional);
        if proposed.is_none_or(|bytes| bytes >= self.next_major_bytes)
            || self.managed_bytes.would_exceed(additional)
        {
            self.major_gc(protected)?;
        }
        self.managed_bytes.check_room(additional)?;
        let required = self.managed_bytes.get().checked_add(additional);
        if required.is_none_or(|bytes| bytes > self.config.max_heap_bytes) {
            #[cfg(any(test, coverage))]
            self.first_failed_allocation
                .get_or_insert(required.unwrap_or(usize::MAX));
            return Err(HeapError::HeapLimitExceeded {
                limit: self.config.max_heap_bytes,
            });
        }
        Ok(())
    }

    pub(super) fn minor_gc(&mut self, protected: ProtectedRoots<'_>) -> Result<(), HeapError> {
        let marked = self.mark_without_roots(true, protected, &HashSet::new())?;
        self.release_realm_roots();
        // Nursery identities are appended in serial order and drained together.
        // Borrow that index instead of allocating a second set during collection.
        let nursery = &self.nursery;
        let identity = self.identity;
        let is_young = |id: &ObjectId| {
            id.heap == identity
                && nursery
                    .binary_search_by_key(&id.serial, |id| id.serial)
                    .is_ok()
        };
        let mut released_weak_entries: usize = 0;
        for (id, object) in &mut self.objects {
            if let ObjectKind::WeakCollection { entries, .. } = &mut object.kind {
                let mut released: usize = 0;
                entries.retain(|key, value| {
                    let keep = match key {
                        WeakCollectionKey::Object(key) => !is_young(key) || marked.contains(key),
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
                    matches!(target, WeakCollectionKey::Object(key) if is_young(key) && !marked.contains(key))
                }) {
                    *target = None;
                }
            }
            if let ObjectKind::FinalizationRegistry { cells, .. } = &mut object.kind {
                for cell in cells.iter_mut() {
                    if cell.target.as_ref().is_some_and(|target| {
                        matches!(target, WeakCollectionKey::Object(key) if is_young(key) && !marked.contains(key))
                    }) {
                        cell.target = None;
                    }
                }
                if let Some(index) = cells.iter().position(|cell| cell.target.is_none()) {
                    self.pending_finalization.insert(*id, index);
                } else {
                    self.pending_finalization.remove(id);
                }
            }
        }
        self.managed_bytes.sub(released_weak_entries);
        for id in self.nursery.drain(..) {
            if marked.contains(&id) {
                self.objects
                    .get_mut(&id)
                    .expect("nursery handle is live")
                    .young = false;
            } else {
                self.managed_bytes.sub(
                    self.objects
                        .remove(&id)
                        .expect("nursery handle is live")
                        .bytes,
                );
                if self.closure_metadata.remove(&id).is_some() {
                    self.managed_bytes.sub(CLOSURE_METADATA_BYTES);
                }
            }
        }
        self.remembered.clear();
        self.clean_abort_weak_edges();
        self.prune_finalization_cleanup_index();
        self.minor_collections += 1;
        self.recycle_collector_marks(marked)
    }

    pub(super) fn major_gc(&mut self, protected: ProtectedRoots<'_>) -> Result<(), HeapError> {
        let marked = self.mark_without_roots(false, protected, &HashSet::new())?;
        self.release_realm_roots();
        let mut released_weak_entries: usize = 0;
        for (id, object) in &mut self.objects {
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
                for cell in cells.iter_mut() {
                    if cell.target.as_ref().is_some_and(
                        |target| matches!(target, WeakCollectionKey::Object(key) if !marked.contains(key)),
                    ) {
                        cell.target = None;
                    }
                }
                if let Some(index) = cells.iter().position(|cell| cell.target.is_none()) {
                    self.pending_finalization.insert(*id, index);
                } else {
                    self.pending_finalization.remove(id);
                }
            }
        }
        self.managed_bytes.sub(released_weak_entries);
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
                self.managed_bytes.sub(obj.bytes);
                false
            }
        });
        self.closure_metadata.retain(|id, _| marked.contains(id));
        self.managed_bytes
            .sub(reclaimed_metadata * CLOSURE_METADATA_BYTES);
        self.nursery.clear();
        self.remembered.clear();
        self.next_major_bytes = self
            .managed_bytes
            .get()
            .saturating_mul(2)
            .max(self.config.major_threshold_bytes)
            .min(self.config.max_heap_bytes);
        self.clean_abort_weak_edges();
        self.prune_finalization_cleanup_index();
        self.major_collections += 1;
        self.recycle_collector_marks(marked)
    }

    fn prune_finalization_cleanup_index(&mut self) {
        self.pending_finalization
            .retain(|id, _| self.objects.contains_key(id));
        if self.pending_finalization.len().saturating_mul(4) < self.pending_finalization.capacity()
        {
            self.pending_finalization.shrink_to_fit();
        }
    }
}
