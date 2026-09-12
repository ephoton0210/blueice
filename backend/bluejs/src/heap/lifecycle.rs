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
        marked
    }

    pub(super) fn minor_gc(&mut self, protected: &[ObjectId]) {
        let marked = self.mark(true, protected);
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
