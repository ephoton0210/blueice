// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

static NEXT_HEAP: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
impl Heap {
    /// Test hook: from now on at most `extra` further bytes may be managed, and
    /// no major collection runs to make room, so every later allocation counts
    /// in full and each in turn is the first that no longer fits. Returns the
    /// resulting limit.
    pub(crate) fn allow_only(&mut self, extra: usize) -> usize {
        let limit = self.managed_bytes + extra;
        self.config.max_heap_bytes = limit;
        self.next_major_bytes = usize::MAX;
        limit
    }
}

impl Heap {
    pub fn new(config: HeapConfig) -> Result<Self, HeapError> {
        Self::new_with_identity_source(config, &NEXT_HEAP)
    }

    pub(super) fn new_with_identity_source(
        config: HeapConfig,
        identities: &AtomicU64,
    ) -> Result<Self, HeapError> {
        if config.nursery_capacity == 0
            || config.major_threshold_bytes == 0
            || config.max_heap_bytes < OBJECT_BYTES
            || config.major_threshold_bytes > config.max_heap_bytes
        {
            return Err(HeapError::InvalidConfig);
        }
        let identity = identities
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| HeapError::IdExhausted)?;
        Ok(Self {
            identity,
            next_object: 1,
            next_root: 1,
            config,
            objects: HashMap::new(),
            closure_metadata: HashMap::new(),
            nursery: Vec::new(),
            remembered: HashSet::new(),
            roots: HashMap::new(),
            scoped_roots: Vec::new(),
            managed_bytes: 0,
            next_major_bytes: config.major_threshold_bytes,
            minor_collections: 0,
            major_collections: 0,
            root_registrations: 0,
            structure_epoch: 0,
        })
    }

    /// Allocates a blank object, with `None` meaning a null prototype.
    /// A future runtime supplies its built-in Object.prototype explicitly.
    /// May collect, protecting `prototype` and everything reachable from it.
    /// The returned object is unrooted until registered or attached to a root.
    pub fn alloc_object(&mut self, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Ordinary, prototype)
    }

    /// Allocates the branded object used by `JSON.rawJSON`. The caller defines
    /// and freezes its observable `rawJSON` property before exposing it.
    pub(crate) fn alloc_raw_json(&mut self) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::RawJson, None)
    }

    /// Reports the presence of the `[[IsRawJSON]]` internal slot. A Proxy is
    /// deliberately not unwrapped: it does not itself carry its target's
    /// internal slots.
    pub(crate) fn is_raw_json(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(self.object(object)?.kind, ObjectKind::RawJson))
    }

    pub(crate) fn alloc_weak_collection(
        &mut self,
        map: bool,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::WeakCollection {
                map,
                entries: HashMap::new(),
            },
            prototype,
        )
    }

    pub(crate) fn alloc_map(&mut self, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::Map {
                entries: OrderedCollection::default(),
            },
            prototype,
        )
    }

    pub(crate) fn alloc_set(&mut self, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::Set {
                entries: OrderedCollection::default(),
            },
            prototype,
        )
    }

    pub(crate) fn is_map(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(self.object(object)?.kind, ObjectKind::Map { .. }))
    }

    pub(crate) fn is_set(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(self.object(object)?.kind, ObjectKind::Set { .. }))
    }

    pub(crate) fn map_size(&self, object: ObjectId) -> Result<usize, HeapError> {
        let ObjectKind::Map { entries } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(entries.len())
    }

    pub(crate) fn map_get(
        &self,
        object: ObjectId,
        key: &Value,
    ) -> Result<Option<Value>, HeapError> {
        let ObjectKind::Map { entries } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(entries.get(key).cloned())
    }

    pub(crate) fn map_has(&self, object: ObjectId, key: &Value) -> Result<bool, HeapError> {
        let ObjectKind::Map { entries } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(entries.has(key))
    }

    pub(crate) fn map_set(
        &mut self,
        object: ObjectId,
        key: Value,
        value: Value,
    ) -> Result<(), HeapError> {
        let key = normalize_collection_key(key);
        if let Some(reference) = key.object_id() {
            self.object(reference)?;
        }
        if let Some(reference) = value.object_id() {
            self.object(reference)?;
        }
        let old_bytes = match &self.object(object)?.kind {
            ObjectKind::Map { entries } => entries
                .get(&key)
                .map_or(0, |old_value| map_entry_bytes(&key, old_value)),
            _ => return Err(HeapError::InvalidInternalSlot(object)),
        };
        let new_bytes = map_entry_bytes(&key, &value);
        let protected: Vec<_> = std::iter::once(object)
            .chain(key.object_id())
            .chain(value.object_id())
            .collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        let (entries, entry_bytes) = self
            .ordered_collection_mut(object, true)
            .expect("Map was protected across collection");
        entries.set(key.clone(), value.clone());
        *entry_bytes = *entry_bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        self.write_barrier(object, key.object_id());
        self.write_barrier(object, value.object_id());
        Ok(())
    }

    pub(crate) fn map_delete(&mut self, object: ObjectId, key: &Value) -> Result<bool, HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::Map { entries } = &mut entry.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        let Some((stored_key, stored_value)) = entries.delete(key) else {
            return Ok(false);
        };
        let bytes = map_entry_bytes(&stored_key, &stored_value);
        entry.bytes -= bytes;
        self.managed_bytes -= bytes;
        Ok(true)
    }

    pub(crate) fn set_size(&self, object: ObjectId) -> Result<usize, HeapError> {
        let ObjectKind::Set { entries } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(entries.len())
    }

    pub(crate) fn set_has(&self, object: ObjectId, key: &Value) -> Result<bool, HeapError> {
        let ObjectKind::Set { entries } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(entries.has(key))
    }

    pub(crate) fn set_add(&mut self, object: ObjectId, key: Value) -> Result<(), HeapError> {
        let key = normalize_collection_key(key);
        if let Some(reference) = key.object_id() {
            self.object(reference)?;
        }
        let exists = match &self.object(object)?.kind {
            ObjectKind::Set { entries } => entries.has(&key),
            _ => return Err(HeapError::InvalidInternalSlot(object)),
        };
        let bytes = map_entry_bytes(&key, &Value::Undefined);
        if !exists {
            let protected: Vec<_> = std::iter::once(object).chain(key.object_id()).collect();
            self.ensure_room(bytes, &protected)?;
        }
        let (entries, entry_bytes) = self
            .ordered_collection_mut(object, false)
            .expect("Set was protected across collection");
        if entries.set(key.clone(), Value::Undefined).is_none() {
            *entry_bytes += bytes;
            self.managed_bytes += bytes;
        }
        self.write_barrier(object, key.object_id());
        Ok(())
    }

    pub(crate) fn set_delete(&mut self, object: ObjectId, key: &Value) -> Result<bool, HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::Set { entries } = &mut entry.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        let Some((stored_key, stored_value)) = entries.delete(key) else {
            return Ok(false);
        };
        let bytes = map_entry_bytes(&stored_key, &stored_value);
        entry.bytes -= bytes;
        self.managed_bytes -= bytes;
        Ok(true)
    }

    pub(super) fn ordered_collection_mut(
        &mut self,
        object: ObjectId,
        map: bool,
    ) -> Result<(&mut OrderedCollection, &mut usize), HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let entries = match (&mut entry.kind, map) {
            (ObjectKind::Map { entries }, true) | (ObjectKind::Set { entries }, false) => entries,
            _ => return Err(HeapError::InvalidInternalSlot(object)),
        };
        Ok((entries, &mut entry.bytes))
    }

    pub(crate) fn alloc_weak_ref(
        &mut self,
        target: Value,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        let target = WeakCollectionKey::from_value(&target).ok_or(HeapError::InvalidWeakTarget)?;
        if let WeakCollectionKey::Object(object) = &target {
            self.object(*object)?;
        }
        self.alloc(
            ObjectKind::WeakRef {
                target: Some(target),
            },
            prototype,
        )
    }

    pub(crate) fn alloc_finalization_registry(
        &mut self,
        cleanup_callback: Value,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::FinalizationRegistry {
                cleanup_callback,
                cells: Vec::new(),
            },
            prototype,
        )
    }

    pub(crate) fn is_finalization_registry(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::FinalizationRegistry { .. }
        ))
    }

    pub(crate) fn finalization_registry_register(
        &mut self,
        registry: ObjectId,
        target: Value,
        holdings: Value,
        unregister_token: Option<Value>,
    ) -> Result<(), HeapError> {
        let target = WeakCollectionKey::from_value(&target).ok_or(HeapError::InvalidWeakTarget)?;
        let unregister_token = unregister_token
            .map(|token| WeakCollectionKey::from_value(&token).ok_or(HeapError::InvalidWeakTarget))
            .transpose()?;
        if let WeakCollectionKey::Object(target) = &target {
            self.object(*target)?;
        }
        if let Some(WeakCollectionKey::Object(token)) = &unregister_token {
            self.object(*token)?;
        }
        let bytes = finalization_cell_bytes(&target, &holdings, &unregister_token);
        let protected: Vec<_> = std::iter::once(registry)
            .chain(match &target {
                WeakCollectionKey::Object(target) => Some(*target),
                WeakCollectionKey::Symbol(_) => None,
            })
            .chain(holdings.object_id())
            .chain(match &unregister_token {
                Some(WeakCollectionKey::Object(token)) => Some(*token),
                _ => None,
            })
            .collect();
        self.ensure_room(bytes, &protected)?;
        self.write_barrier(registry, holdings.object_id());
        let object = self
            .objects
            .get_mut(&registry)
            .expect("FinalizationRegistry is protected across collection");
        let ObjectKind::FinalizationRegistry { cells, .. } = &mut object.kind else {
            return Err(HeapError::InvalidInternalSlot(registry));
        };
        cells.push(FinalizationCell {
            target: Some(target),
            holdings,
            unregister_token,
        });
        object.bytes += bytes;
        self.managed_bytes += bytes;
        Ok(())
    }

    pub(crate) fn finalization_registry_unregister(
        &mut self,
        registry: ObjectId,
        unregister_token: Value,
    ) -> Result<bool, HeapError> {
        let unregister_token =
            WeakCollectionKey::from_value(&unregister_token).ok_or(HeapError::InvalidWeakTarget)?;
        let (removed, released) = {
            let object = self
                .objects
                .get_mut(&registry)
                .ok_or(HeapError::InvalidObject(registry))?;
            let ObjectKind::FinalizationRegistry { cells, .. } = &mut object.kind else {
                return Err(HeapError::InvalidInternalSlot(registry));
            };
            let mut released: usize = 0;
            let previous_len = cells.len();
            cells.retain(|cell| {
                let remove = cell.unregister_token.as_ref() == Some(&unregister_token);
                if remove {
                    released = released
                        .saturating_add(size_of::<FinalizationCell>())
                        .saturating_add(cell.target.as_ref().map_or(0, weak_collection_key_bytes))
                        .saturating_add(cell.holdings.payload_bytes())
                        .saturating_add(
                            cell.unregister_token
                                .as_ref()
                                .map_or(0, weak_collection_key_bytes),
                        );
                }
                !remove
            });
            object.bytes -= released;
            (cells.len() != previous_len, released)
        };
        self.managed_bytes -= released;
        Ok(removed)
    }

    /// Transfers cells whose weak target was cleared by collection to the VM
    /// job queue. The callback and holdings stay strongly reachable through
    /// the returned job until the host invokes the callback.
    pub(crate) fn take_finalization_registry_cleanup_jobs(&mut self) -> Vec<(Value, Value)> {
        let mut jobs = Vec::new();
        let mut released: usize = 0;
        for object in self.objects.values_mut() {
            let ObjectKind::FinalizationRegistry {
                cleanup_callback,
                cells,
            } = &mut object.kind
            else {
                continue;
            };
            let mut registry_released: usize = 0;
            let mut live = Vec::with_capacity(cells.len());
            for cell in std::mem::take(cells) {
                if cell.target.is_none() {
                    // `target` is cleared by GC, so only its key identity is
                    // needed here; the entry still carries the holdings and
                    // unregister token payload that were charged at register.
                    registry_released =
                        registry_released.saturating_add(size_of::<FinalizationCell>());
                    registry_released =
                        registry_released.saturating_add(cell.holdings.payload_bytes());
                    registry_released = registry_released.saturating_add(
                        cell.unregister_token
                            .as_ref()
                            .map_or(0, weak_collection_key_bytes),
                    );
                    jobs.push((cleanup_callback.clone(), cell.holdings));
                } else {
                    live.push(cell);
                }
            }
            *cells = live;
            object.bytes -= registry_released;
            released = released.saturating_add(registry_released);
        }
        self.managed_bytes -= released;
        jobs
    }

    pub(crate) fn weak_ref_target(&self, object: ObjectId) -> Result<Option<Value>, HeapError> {
        let ObjectKind::WeakRef { target } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(match target {
            Some(WeakCollectionKey::Object(target)) if self.objects.contains_key(target) => {
                Some(Value::Object(*target))
            }
            Some(WeakCollectionKey::Symbol(target)) => Some(Value::Symbol(target.clone())),
            Some(WeakCollectionKey::Object(_)) | None => None,
        })
    }

    pub(crate) fn is_weak_collection(
        &self,
        object: ObjectId,
        map: bool,
    ) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::WeakCollection { map: actual, .. } if actual == map
        ))
    }

    pub(crate) fn weak_collection_get(
        &self,
        object: ObjectId,
        key: &Value,
    ) -> Result<Option<Value>, HeapError> {
        let ObjectKind::WeakCollection { entries, .. } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(WeakCollectionKey::from_value(key).and_then(|key| entries.get(&key).cloned()))
    }

    pub(crate) fn weak_collection_set(
        &mut self,
        object: ObjectId,
        key: Value,
        value: Value,
    ) -> Result<(), HeapError> {
        let key =
            WeakCollectionKey::from_value(&key).ok_or(HeapError::InvalidInternalSlot(object))?;
        if let WeakCollectionKey::Object(key) = &key {
            self.object(*key)?;
        }
        let old_bytes = match &self.object(object)?.kind {
            ObjectKind::WeakCollection { entries, .. } => entries
                .get(&key)
                .map(|value| weak_collection_entry_bytes(&key, value))
                .unwrap_or(0),
            _ => return Err(HeapError::InvalidInternalSlot(object)),
        };
        let new_bytes = weak_collection_entry_bytes(&key, &value);
        let protected: Vec<_> = std::iter::once(object)
            .chain(match &key {
                WeakCollectionKey::Object(key) => Some(*key),
                WeakCollectionKey::Symbol(_) => None,
            })
            .chain(value.object_id())
            .collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        let (entries, entry_bytes) = self
            .weak_collection_mut(object)
            .expect("WeakCollection is protected across collection");
        entries.insert(key, value);
        *entry_bytes = *entry_bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        self.remembered.insert(object);
        Ok(())
    }

    pub(crate) fn weak_collection_delete(
        &mut self,
        object: ObjectId,
        key: &Value,
    ) -> Result<bool, HeapError> {
        let Some(key) = WeakCollectionKey::from_value(key) else {
            return Ok(false);
        };
        let object_id = object;
        let (deleted, released) = {
            let object = self
                .objects
                .get_mut(&object_id)
                .ok_or(HeapError::InvalidObject(object_id))?;
            let ObjectKind::WeakCollection { entries, .. } = &mut object.kind else {
                return Err(HeapError::InvalidInternalSlot(object_id));
            };
            let Some(value) = entries.remove(&key) else {
                return Ok(false);
            };
            let released = weak_collection_entry_bytes(&key, &value);
            object.bytes -= released;
            (true, released)
        };
        self.managed_bytes -= released;
        Ok(deleted)
    }

    pub(super) fn weak_collection_mut(
        &mut self,
        object: ObjectId,
    ) -> Result<(&mut HashMap<WeakCollectionKey, Value>, &mut usize), HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::WeakCollection { entries, .. } = &mut entry.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok((entries, &mut entry.bytes))
    }

    pub(super) fn ensure_private_data(
        &mut self,
        object: ObjectId,
        protected: &[ObjectId],
    ) -> Result<(), HeapError> {
        self.object(object)?;
        if self.objects[&object].private.is_some() {
            return Ok(());
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(protected.iter().copied())
            .collect();
        self.ensure_room(PRIVATE_DATA_BYTES, &protected)?;
        let object = self
            .objects
            .get_mut(&object)
            .expect("private owner is protected across collection");
        object.private = Some(Box::default());
        object.bytes += PRIVATE_DATA_BYTES;
        self.managed_bytes += PRIVATE_DATA_BYTES;
        Ok(())
    }

    pub(crate) fn define_private_field(
        &mut self,
        owner: ObjectId,
        name: JsString,
    ) -> Result<(), HeapError> {
        self.define_private_element(owner, name, PrivateElement::Field)
    }

    pub(crate) fn define_private_method(
        &mut self,
        owner: ObjectId,
        name: JsString,
        function: Value,
    ) -> Result<(), HeapError> {
        self.define_private_element(owner, name, PrivateElement::Method(function))
    }

    pub(crate) fn define_private_accessor(
        &mut self,
        owner: ObjectId,
        name: JsString,
        function: Value,
        setter: bool,
    ) -> Result<(), HeapError> {
        let existing = self
            .object(owner)?
            .private
            .as_ref()
            .and_then(|private| private.elements.get(&name))
            .cloned();
        let element = match existing {
            None if setter => PrivateElement::Accessor {
                get: None,
                set: Some(function),
            },
            None => PrivateElement::Accessor {
                get: Some(function),
                set: None,
            },
            Some(PrivateElement::Accessor { mut get, mut set }) => {
                if setter {
                    set = Some(function);
                } else {
                    get = Some(function);
                }
                PrivateElement::Accessor { get, set }
            }
            Some(_) => return Err(HeapError::ReadOnlyProperty),
        };
        self.define_private_element(owner, name, element)
    }

    fn define_private_element(
        &mut self,
        owner: ObjectId,
        name: JsString,
        element: PrivateElement,
    ) -> Result<(), HeapError> {
        self.object(owner)?;
        let references: Vec<_> = element.references().collect();
        for reference in &references {
            self.object(*reference)?;
        }
        self.ensure_private_data(owner, &references)?;
        let old_bytes = self.objects[&owner]
            .private
            .as_ref()
            .expect("private data was installed")
            .elements
            .get(&name)
            .map_or(0, |old| private_element_bytes(&name, old));
        let new_bytes = private_element_bytes(&name, &element);
        let protected: Vec<_> = std::iter::once(owner)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        let object = self
            .objects
            .get_mut(&owner)
            .expect("private owner is protected across collection");
        object
            .private
            .as_mut()
            .expect("private data was installed")
            .elements
            .insert(name, element);
        object.bytes = object.bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        for reference in references {
            self.write_barrier(owner, Some(reference));
        }
        Ok(())
    }

    pub(crate) fn add_private_brand(
        &mut self,
        receiver: ObjectId,
        owner: ObjectId,
    ) -> Result<(), HeapError> {
        self.object(receiver)?;
        self.object(owner)?;
        if self.objects[&receiver]
            .private
            .as_ref()
            .is_some_and(|private| private.brands.contains(&owner))
        {
            return Ok(());
        }
        self.ensure_private_data(receiver, &[owner])?;
        let bytes = size_of::<ObjectId>();
        self.ensure_room(bytes, &[receiver, owner])?;
        let receiver_object = self
            .objects
            .get_mut(&receiver)
            .expect("private receiver is protected across collection");
        receiver_object
            .private
            .as_mut()
            .expect("private data was installed")
            .brands
            .insert(owner);
        receiver_object.bytes += bytes;
        self.managed_bytes += bytes;
        self.write_barrier(receiver, Some(owner));
        Ok(())
    }

    pub(crate) fn has_private_brand(
        &self,
        receiver: ObjectId,
        owner: ObjectId,
    ) -> Result<bool, HeapError> {
        Ok(self
            .object(receiver)?
            .private
            .as_ref()
            .is_some_and(|private| private.brands.contains(&owner)))
    }

    pub(crate) fn private_element(
        &self,
        owner: ObjectId,
        name: &JsString,
    ) -> Result<Option<PrivateElement>, HeapError> {
        Ok(self
            .object(owner)?
            .private
            .as_ref()
            .and_then(|private| private.elements.get(name))
            .cloned())
    }

    pub(crate) fn private_slot(
        &self,
        receiver: ObjectId,
        owner: ObjectId,
        name: &JsString,
    ) -> Result<Option<Value>, HeapError> {
        Ok(self
            .object(receiver)?
            .private
            .as_ref()
            .and_then(|private| private.slots.get(&(owner, name.clone())))
            .cloned())
    }

    pub(crate) fn set_private_slot(
        &mut self,
        receiver: ObjectId,
        owner: ObjectId,
        name: JsString,
        value: Value,
    ) -> Result<(), HeapError> {
        self.object(receiver)?;
        self.object(owner)?;
        if let Some(reference) = value.object_id() {
            self.object(reference)?;
        }
        let protected: Vec<_> = std::iter::once(owner).chain(value.object_id()).collect();
        self.ensure_private_data(receiver, &protected)?;
        let key = (owner, name.clone());
        let old_bytes = self.objects[&receiver]
            .private
            .as_ref()
            .expect("private data was installed")
            .slots
            .get(&key)
            .map_or(0, |old| private_slot_bytes(&name, old));
        let new_bytes = private_slot_bytes(&name, &value);
        let protected: Vec<_> = std::iter::once(receiver)
            .chain(std::iter::once(owner))
            .chain(value.object_id())
            .collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        let receiver_object = self
            .objects
            .get_mut(&receiver)
            .expect("private receiver is protected across collection");
        receiver_object
            .private
            .as_mut()
            .expect("private data was installed")
            .slots
            .insert(key, value.clone());
        receiver_object.bytes = receiver_object.bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        self.write_barrier(receiver, Some(owner));
        self.write_barrier(receiver, value.object_id());
        Ok(())
    }

    /// Creates a non-extensible Module Namespace Exotic Object.  Its string
    /// exports are retained as binding cells; reading a namespace property
    /// therefore observes the current exporter value rather than a snapshot.
    pub(crate) fn alloc_module_namespace(
        &mut self,
        mut exports: Vec<(JsString, ObjectId)>,
        deferred: bool,
    ) -> Result<ObjectId, HeapError> {
        for (_, cell) in &exports {
            self.object(*cell)?;
        }
        exports.sort_by(|(left, _), (right, _)| left.as_code_units().cmp(right.as_code_units()));
        let namespace = self.alloc(ObjectKind::ModuleNamespace { exports }, None)?;
        self.define_own_property(
            namespace,
            JsSymbol::well_known("toStringTag"),
            PropertyDescriptor::data(
                Value::String(
                    if deferred {
                        "Deferred Module"
                    } else {
                        "Module"
                    }
                    .into(),
                ),
                false,
                false,
                false,
            ),
        )?;
        self.prevent_extensions(namespace)
            .expect("newly allocated namespace remains live");
        Ok(namespace)
    }

    /// Completes a namespace allocated with an empty export list.  Namespace
    /// exports can themselves name the namespace currently under
    /// construction (`export * as self from "./self.js"`), so the VM first
    /// publishes an identity-stable placeholder and fills its private export
    /// cells once recursive namespace resolution returns.  This is an
    /// internal construction operation; JavaScript still observes a
    /// non-extensible Module Namespace Exotic Object throughout.
    pub(crate) fn initialize_module_namespace(
        &mut self,
        namespace: ObjectId,
        mut exports: Vec<(JsString, ObjectId)>,
    ) -> Result<(), HeapError> {
        for (_, cell) in &exports {
            self.object(*cell)?;
        }
        exports.sort_by(|(left, _), (right, _)| left.as_code_units().cmp(right.as_code_units()));
        let additional = exports
            .iter()
            .map(|(name, _)| name.byte_len() + size_of::<(JsString, ObjectId)>())
            .sum();
        self.ensure_room(additional, &[namespace])?;
        for (_, cell) in &exports {
            self.write_barrier(namespace, Some(*cell));
        }
        let object = self
            .objects
            .get_mut(&namespace)
            .ok_or(HeapError::InvalidObject(namespace))?;
        let ObjectKind::ModuleNamespace { exports: existing } = &mut object.kind else {
            return Err(HeapError::InvalidObject(namespace));
        };
        if !existing.is_empty() {
            return Err(HeapError::InvalidObject(namespace));
        }
        *existing = exports;
        self.managed_bytes += additional;
        Ok(())
    }

    /// Allocates a host-defined exotic with the Annex B `[[IsHTMLDDA]]` slot.
    /// It is a callable (`function` is what a call runs), like the document.all
    /// object it models. Only the Test262 host creates one; ordinary
    /// JavaScript cannot.
    pub(crate) fn alloc_html_dda_object(
        &mut self,
        function: NativeFunction,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        let id = self.alloc_native_function(function, "", prototype)?;
        self.objects
            .get_mut(&id)
            .expect("freshly allocated object is present")
            .is_html_dda = true;
        Ok(id)
    }

    pub(crate) fn is_html_dda(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(self.object(object)?.is_html_dda)
    }

    /// Allocates a sparse array of holes; even a length of u32::MAX costs
    /// only one object record. The caller supplies its prototype, exactly
    /// as for alloc_object. May collect, protecting that prototype graph.
    pub fn alloc_array(
        &mut self,
        length: u32,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Array { length }, prototype)
    }

    pub(crate) fn alloc_date(
        &mut self,
        time: f64,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Date { time }, prototype)
    }

    pub(crate) fn alloc_error(
        &mut self,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Error, prototype)
    }

    pub(crate) fn is_error(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(self.object(object)?.kind, ObjectKind::Error))
    }

    pub(crate) fn is_date(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(self.object(object)?.kind, ObjectKind::Date { .. }))
    }

    pub(crate) fn date_value(&self, object: ObjectId) -> Result<f64, HeapError> {
        let ObjectKind::Date { time } = self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(time)
    }

    pub(crate) fn set_date_value(&mut self, object: ObjectId, time: f64) -> Result<(), HeapError> {
        let id = object;
        let object = self
            .objects
            .get_mut(&id)
            .ok_or(HeapError::InvalidObject(id))?;
        let ObjectKind::Date { time: slot } = &mut object.kind else {
            return Err(HeapError::InvalidInternalSlot(id));
        };
        *slot = time;
        Ok(())
    }

    pub(crate) fn alloc_temporal(
        &mut self,
        value: TemporalValue,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Temporal(Box::new(value)), prototype)
    }

    /// The Temporal type of `object`'s internal slot, without cloning the
    /// value (the receiver brand check runs before every prototype member).
    pub(crate) fn temporal_kind(
        &self,
        object: ObjectId,
    ) -> Result<Option<TemporalKind>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Temporal(value) => Some(value.kind),
            _ => None,
        })
    }

    pub(crate) fn temporal_value(
        &self,
        object: ObjectId,
    ) -> Result<Option<TemporalValue>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Temporal(value) => Some((**value).clone()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An object that belongs to a different heap, so it names nothing here.
    fn foreign() -> ObjectId {
        Heap::default().alloc_object(None).unwrap()
    }

    fn failure<T>(result: Result<T, HeapError>) -> Option<HeapError> {
        result.err()
    }

    fn temporal_value() -> TemporalValue {
        TemporalValue {
            kind: TemporalKind::Instant,
            duration: None,
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            microsecond: 0,
            nanosecond: 0,
            epoch_nanoseconds: BigInt::from(0),
            calendar: String::new(),
            time_zone: String::new(),
        }
    }

    #[test]
    fn operations_on_an_object_of_another_heap_are_invalid() {
        let mut heap = Heap::default();
        let other = foreign();
        let bad = Some(HeapError::InvalidObject(other));
        let name = || JsString::from("p");
        let cell = Value::Object(other);
        assert_eq!(failure(heap.is_raw_json(other)), bad);
        assert_eq!(failure(heap.is_map(other)), bad);
        assert_eq!(failure(heap.is_set(other)), bad);
        assert_eq!(failure(heap.map_size(other)), bad);
        assert_eq!(failure(heap.map_get(other, &Value::Null)), bad);
        assert_eq!(failure(heap.map_has(other, &Value::Null)), bad);
        assert_eq!(failure(heap.map_delete(other, &Value::Null)), bad);
        assert_eq!(failure(heap.set_size(other)), bad);
        assert_eq!(failure(heap.set_has(other, &Value::Null)), bad);
        assert_eq!(failure(heap.set_delete(other, &Value::Null)), bad);
        assert_eq!(failure(heap.map_set(other, Value::Null, Value::Null)), bad);
        assert_eq!(failure(heap.set_add(other, Value::Null)), bad);
        assert_eq!(failure(heap.is_finalization_registry(other)), bad);
        assert_eq!(failure(heap.weak_ref_target(other)), bad);
        assert_eq!(failure(heap.is_weak_collection(other, true)), bad);
        assert_eq!(failure(heap.weak_collection_get(other, &Value::Null)), bad);
        assert_eq!(failure(heap.weak_collection_delete(other, &cell)), bad);
        assert_eq!(failure(heap.is_html_dda(other)), bad);
        assert_eq!(failure(heap.is_error(other)), bad);
        assert_eq!(failure(heap.is_date(other)), bad);
        assert_eq!(failure(heap.date_value(other)), bad);
        assert_eq!(failure(heap.set_date_value(other, 0.0)), bad);
        assert_eq!(failure(heap.temporal_kind(other)), bad);
        assert_eq!(failure(heap.temporal_value(other)), bad);
        assert_eq!(failure(heap.define_private_field(other, name())), bad);
        assert_eq!(
            failure(heap.define_private_accessor(other, name(), Value::Null, true)),
            bad
        );
        assert_eq!(failure(heap.add_private_brand(other, other)), bad);
        assert_eq!(failure(heap.has_private_brand(other, other)), bad);
        assert_eq!(failure(heap.private_element(other, &name())), bad);
        assert_eq!(failure(heap.private_slot(other, other, &name())), bad);
        assert_eq!(
            failure(heap.set_private_slot(other, other, name(), Value::Null)),
            bad
        );
        assert_eq!(
            failure(heap.finalization_registry_unregister(other, cell.clone())),
            bad
        );
        assert_eq!(failure(heap.alloc_weak_ref(cell.clone(), None)), bad);
        assert_eq!(
            failure(heap.alloc_module_namespace(vec![(name(), other)], false)),
            bad
        );
        assert_eq!(
            failure(heap.initialize_module_namespace(other, Vec::new())),
            bad
        );
        assert_eq!(
            failure(heap.initialize_module_namespace(other, vec![(name(), other)])),
            bad
        );
        assert_eq!(failure(heap.alloc_map(Some(other))), bad);
    }

    #[test]
    fn references_to_objects_of_another_heap_are_invalid() {
        let mut heap = Heap::default();
        let other = foreign();
        let bad = Some(HeapError::InvalidObject(other));
        let foreign_value = || Value::Object(other);
        let map = heap.alloc_map(None).unwrap();
        let set = heap.alloc_set(None).unwrap();
        let registry = heap.alloc_finalization_registry(Value::Null, None).unwrap();
        let weak = heap.alloc_weak_collection(true, None).unwrap();
        let object = heap.alloc_object(None).unwrap();
        assert_eq!(
            failure(heap.map_set(map, foreign_value(), Value::Null)),
            bad
        );
        assert_eq!(
            failure(heap.map_set(map, Value::Null, foreign_value())),
            bad
        );
        assert_eq!(failure(heap.set_add(set, foreign_value())), bad);
        assert_eq!(
            failure(heap.finalization_registry_register(
                registry,
                foreign_value(),
                Value::Null,
                None
            )),
            bad
        );
        assert_eq!(
            failure(heap.finalization_registry_register(
                registry,
                Value::Object(object),
                Value::Null,
                Some(foreign_value())
            )),
            bad
        );
        assert_eq!(
            failure(heap.weak_collection_set(weak, foreign_value(), Value::Null)),
            bad
        );
        assert_eq!(
            failure(heap.define_private_method(object, "m".into(), foreign_value())),
            bad
        );
        assert_eq!(failure(heap.add_private_brand(object, other)), bad);
        assert_eq!(failure(heap.add_private_brand(other, object)), bad);
        assert_eq!(
            failure(heap.set_private_slot(object, other, "p".into(), Value::Null)),
            bad
        );
        assert_eq!(
            failure(heap.set_private_slot(object, object, "p".into(), foreign_value())),
            bad
        );
    }

    #[test]
    fn operations_on_the_wrong_kind_of_object_lack_the_internal_slot() {
        let mut heap = Heap::default();
        let plain = heap.alloc_object(None).unwrap();
        let map = heap.alloc_map(None).unwrap();
        let set = heap.alloc_set(None).unwrap();
        let registry = heap.alloc_finalization_registry(Value::Null, None).unwrap();
        let weak_map = heap.alloc_weak_collection(true, None).unwrap();
        let weak_ref = heap.alloc_weak_ref(Value::Object(plain), None).unwrap();
        let bad = |id| Some(HeapError::InvalidInternalSlot(id));
        let key = Value::Object(plain);
        assert_eq!(failure(heap.map_size(set)), bad(set));
        assert_eq!(failure(heap.map_get(set, &key)), bad(set));
        assert_eq!(failure(heap.map_has(set, &key)), bad(set));
        assert_eq!(
            failure(heap.map_set(set, key.clone(), key.clone())),
            bad(set)
        );
        assert_eq!(failure(heap.map_delete(set, &key)), bad(set));
        assert_eq!(failure(heap.set_size(map)), bad(map));
        assert_eq!(failure(heap.set_has(map, &key)), bad(map));
        assert_eq!(failure(heap.set_add(map, key.clone())), bad(map));
        assert_eq!(failure(heap.set_delete(map, &key)), bad(map));
        assert_eq!(
            failure(heap.finalization_registry_register(map, key.clone(), Value::Null, None)),
            bad(map)
        );
        assert_eq!(
            failure(heap.finalization_registry_unregister(map, key.clone())),
            bad(map)
        );
        assert_eq!(failure(heap.weak_ref_target(plain)), bad(plain));
        assert_eq!(failure(heap.weak_collection_get(plain, &key)), bad(plain));
        assert_eq!(
            failure(heap.weak_collection_set(plain, key.clone(), Value::Null)),
            bad(plain)
        );
        assert_eq!(
            failure(heap.weak_collection_delete(plain, &key)),
            bad(plain)
        );
        // A weak collection key must be an object or symbol.
        assert_eq!(
            failure(heap.weak_collection_set(weak_map, Value::Number(1.0), Value::Null)),
            bad(weak_map)
        );
        assert_eq!(failure(heap.date_value(plain)), bad(plain));
        assert_eq!(failure(heap.set_date_value(plain, 0.0)), bad(plain));
        assert_eq!(
            failure(heap.initialize_module_namespace(plain, Vec::new())),
            Some(HeapError::InvalidObject(plain))
        );
        // Neither a Date nor a weak collection of the other kind.
        assert_eq!(heap.is_date(plain), Ok(false));
        assert_eq!(heap.is_weak_collection(weak_map, false), Ok(false));
        assert_eq!(heap.is_weak_collection(weak_map, true), Ok(true));
        assert_eq!(heap.is_finalization_registry(registry), Ok(true));
        assert_eq!(heap.is_finalization_registry(plain), Ok(false));
        assert_eq!(
            heap.weak_ref_target(weak_ref),
            Ok(Some(Value::Object(plain)))
        );
    }

    #[test]
    fn weak_targets_must_be_objects_or_symbols() {
        let mut heap = Heap::default();
        let registry = heap.alloc_finalization_registry(Value::Null, None).unwrap();
        let weak_map = heap.alloc_weak_collection(true, None).unwrap();
        let invalid = Some(HeapError::InvalidWeakTarget);
        assert_eq!(failure(heap.alloc_weak_ref(Value::Null, None)), invalid);
        assert_eq!(
            failure(heap.finalization_registry_unregister(registry, Value::Number(1.0))),
            invalid
        );
        assert_eq!(
            failure(heap.finalization_registry_register(
                registry,
                Value::Object(weak_map),
                Value::Null,
                Some(Value::Number(1.0))
            )),
            invalid
        );
        // Looking up or deleting a non-weak key finds nothing rather than failing.
        assert_eq!(
            heap.weak_collection_get(weak_map, &Value::Number(1.0)),
            Ok(None)
        );
        assert_eq!(
            heap.weak_collection_delete(weak_map, &Value::Number(1.0)),
            Ok(false)
        );
    }

    #[test]
    fn deleting_an_absent_entry_reports_false() {
        let mut heap = Heap::default();
        let map = heap.alloc_map(None).unwrap();
        let set = heap.alloc_set(None).unwrap();
        let registry = heap.alloc_finalization_registry(Value::Null, None).unwrap();
        let weak_map = heap.alloc_weak_collection(true, None).unwrap();
        let key = heap.alloc_object(None).unwrap();
        assert_eq!(heap.map_delete(map, &Value::Number(1.0)), Ok(false));
        assert_eq!(heap.set_delete(set, &Value::Number(1.0)), Ok(false));
        assert_eq!(
            heap.weak_collection_delete(weak_map, &Value::Object(key)),
            Ok(false)
        );
        assert_eq!(
            heap.finalization_registry_unregister(registry, Value::Object(key)),
            Ok(false)
        );
        heap.map_set(map, Value::Number(1.0), Value::Null).unwrap();
        heap.set_add(set, Value::Number(1.0)).unwrap();
        heap.weak_collection_set(weak_map, Value::Object(key), Value::Null)
            .unwrap();
        assert_eq!(heap.map_delete(map, &Value::Number(1.0)), Ok(true));
        assert_eq!(heap.set_delete(set, &Value::Number(1.0)), Ok(true));
        assert_eq!(
            heap.weak_collection_delete(weak_map, &Value::Object(key)),
            Ok(true)
        );
        assert_eq!(heap.map_size(map), Ok(0));
        assert_eq!(heap.set_size(set), Ok(0));
        assert_eq!(heap.map_get(map, &Value::Number(1.0)), Ok(None));
        assert_eq!(heap.map_has(map, &Value::Number(1.0)), Ok(false));
        assert_eq!(heap.set_has(set, &Value::Number(1.0)), Ok(false));
    }

    #[test]
    fn brands_dates_temporal_values_and_flags_report_their_kind() {
        let mut heap = Heap::default();
        let owner = heap.alloc_object(None).unwrap();
        let receiver = heap.alloc_object(None).unwrap();
        let json = heap.alloc_raw_json().unwrap();
        let date = heap.alloc_date(5.0, None).unwrap();
        let error = heap.alloc_error(None).unwrap();
        let temporal = heap.alloc_temporal(temporal_value(), None).unwrap();
        let map = heap.alloc_map(None).unwrap();
        let set = heap.alloc_set(None).unwrap();
        assert_eq!(heap.has_private_brand(receiver, owner), Ok(false));
        heap.add_private_brand(receiver, owner).unwrap();
        // Adding the same brand again changes nothing.
        heap.add_private_brand(receiver, owner).unwrap();
        assert_eq!(heap.has_private_brand(receiver, owner), Ok(true));
        assert_eq!(heap.is_raw_json(json), Ok(true));
        assert_eq!(heap.is_raw_json(owner), Ok(false));
        assert_eq!(heap.is_map(map), Ok(true));
        assert_eq!(heap.is_map(set), Ok(false));
        assert_eq!(heap.is_set(set), Ok(true));
        assert_eq!(heap.is_set(map), Ok(false));
        assert_eq!(heap.is_date(date), Ok(true));
        assert_eq!(heap.date_value(date), Ok(5.0));
        heap.set_date_value(date, 6.0).unwrap();
        assert_eq!(heap.date_value(date), Ok(6.0));
        assert_eq!(heap.is_error(error), Ok(true));
        assert_eq!(heap.is_error(date), Ok(false));
        assert_eq!(heap.is_html_dda(owner), Ok(false));
        assert_eq!(
            heap.temporal_kind(temporal),
            Ok(Some(TemporalKind::Instant))
        );
        assert_eq!(heap.temporal_kind(owner), Ok(None));
        assert!(heap.temporal_value(temporal).unwrap().is_some());
        assert!(heap.temporal_value(owner).unwrap().is_none());
    }

    #[test]
    fn private_accessors_merge_and_reject_other_element_kinds() {
        let mut heap = Heap::default();
        let owner = heap.alloc_object(None).unwrap();
        let getter = Value::Object(heap.alloc_object(None).unwrap());
        let setter = Value::Object(heap.alloc_object(None).unwrap());
        heap.define_private_accessor(owner, "a".into(), getter, false)
            .unwrap();
        heap.define_private_accessor(owner, "a".into(), setter, true)
            .unwrap();
        heap.define_private_field(owner, "f".into()).unwrap();
        assert_eq!(
            failure(heap.define_private_accessor(owner, "f".into(), Value::Null, false)),
            Some(HeapError::ReadOnlyProperty)
        );
    }

    #[test]
    fn module_namespaces_are_completed_exactly_once() {
        let mut heap = Heap::default();
        let cell = heap.alloc_object(None).unwrap();
        let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
        heap.initialize_module_namespace(namespace, vec![("x".into(), cell)])
            .unwrap();
        // A namespace that already has exports cannot be initialized again.
        assert_eq!(
            failure(heap.initialize_module_namespace(namespace, vec![("y".into(), cell)])),
            Some(HeapError::InvalidObject(namespace))
        );
    }

    /// Prepares a heap with `prepare`, then gives `operation` a budget that
    /// grows a little at a time from "no room at all" until it succeeds; every
    /// failure on the way must be the heap limit.
    fn exhausts_then_succeeds(
        prepare: fn(&mut Heap) -> [ObjectId; 3],
        operation: fn(&mut Heap, [ObjectId; 3]) -> Result<(), HeapError>,
    ) {
        let mut extra = 0;
        loop {
            let mut heap = Heap::default();
            let objects = prepare(&mut heap);
            let limit = heap.stats().managed_bytes + extra;
            heap.config.max_heap_bytes = limit;
            let result = operation(&mut heap, objects);
            if result.is_ok() {
                return;
            }
            assert_eq!(result, Err(HeapError::HeapLimitExceeded { limit }));
            extra += 8;
        }
    }

    /// Three rooted plain objects.
    fn three_objects(heap: &mut Heap) -> [ObjectId; 3] {
        [(); 3].map(|()| {
            let object = heap.alloc_object(None).unwrap();
            heap.root(object).unwrap();
            object
        })
    }

    #[test]
    fn a_full_heap_fails_map_and_set_growth() {
        exhausts_then_succeeds(
            |heap| {
                let objects = three_objects(heap);
                let map = heap.alloc_map(None).unwrap();
                heap.root(map).unwrap();
                [map, objects[1], objects[2]]
            },
            |heap, [map, ..]| {
                heap.map_set(map, Value::Number(1.0), Value::String("payload".into()))
            },
        );
        exhausts_then_succeeds(
            |heap| {
                let objects = three_objects(heap);
                let set = heap.alloc_set(None).unwrap();
                heap.root(set).unwrap();
                [set, objects[1], objects[2]]
            },
            |heap, [set, ..]| heap.set_add(set, Value::Number(1.0)),
        );
    }

    #[test]
    fn a_full_heap_fails_weak_collection_and_finalization_growth() {
        exhausts_then_succeeds(
            |heap| {
                let objects = three_objects(heap);
                let weak = heap.alloc_weak_collection(true, None).unwrap();
                heap.root(weak).unwrap();
                [weak, objects[1], objects[2]]
            },
            |heap, [weak, key, _]| heap.weak_collection_set(weak, Value::Object(key), Value::Null),
        );
        exhausts_then_succeeds(
            |heap| {
                let objects = three_objects(heap);
                let registry = heap.alloc_finalization_registry(Value::Null, None).unwrap();
                heap.root(registry).unwrap();
                [registry, objects[1], objects[2]]
            },
            |heap, [registry, target, _]| {
                heap.finalization_registry_register(
                    registry,
                    Value::Object(target),
                    Value::String("held".into()),
                    Some(Value::Object(target)),
                )
            },
        );
    }

    #[test]
    fn a_full_heap_fails_private_state_growth() {
        exhausts_then_succeeds(three_objects, |heap, [owner, ..]| {
            heap.define_private_field(owner, "f".into())
        });
        exhausts_then_succeeds(three_objects, |heap, [owner, method, _]| {
            heap.define_private_method(owner, "m".into(), Value::Object(method))
        });
        exhausts_then_succeeds(three_objects, |heap, [owner, getter, _]| {
            heap.define_private_accessor(owner, "a".into(), Value::Object(getter), false)
        });
        exhausts_then_succeeds(three_objects, |heap, [owner, receiver, _]| {
            heap.add_private_brand(receiver, owner)
        });
        exhausts_then_succeeds(three_objects, |heap, [owner, receiver, _]| {
            heap.set_private_slot(receiver, owner, "p".into(), Value::String("v".into()))
        });
    }

    #[test]
    fn a_full_heap_fails_module_namespace_construction() {
        exhausts_then_succeeds(three_objects, |heap, [first, second, _]| {
            // Exports are kept sorted by name.
            heap.alloc_module_namespace(vec![("b".into(), first), ("a".into(), second)], true)
                .map(|_| ())
        });
        exhausts_then_succeeds(
            |heap| {
                let objects = three_objects(heap);
                let namespace = heap.alloc_module_namespace(Vec::new(), false).unwrap();
                heap.root(namespace).unwrap();
                [namespace, objects[1], objects[2]]
            },
            |heap, [namespace, first, second]| {
                heap.initialize_module_namespace(
                    namespace,
                    vec![("b".into(), first), ("a".into(), second)],
                )
            },
        );
        exhausts_then_succeeds(three_objects, |heap, [prototype, ..]| {
            heap.alloc_html_dda_object(NativeFunction::Test262("gc"), prototype)
                .map(|_| ())
        });
    }

    #[test]
    fn weak_collection_keys_and_registry_targets_are_checked_before_use() {
        let mut heap = Heap::default();
        let other = foreign();
        let live = heap.alloc_object(None).unwrap();
        let registry = heap.alloc_finalization_registry(Value::Null, None).unwrap();
        let weak = heap.alloc_weak_collection(true, None).unwrap();
        assert_eq!(
            failure(heap.finalization_registry_register(
                registry,
                Value::Number(1.0),
                Value::Null,
                None
            )),
            Some(HeapError::InvalidWeakTarget)
        );
        assert_eq!(
            failure(heap.weak_collection_set(other, Value::Object(live), Value::Null)),
            Some(HeapError::InvalidObject(other))
        );
        assert_eq!(
            heap.weak_collection_set(weak, Value::Object(live), Value::Null),
            Ok(())
        );
    }

    #[test]
    fn re_adding_and_symbol_targets_take_the_non_growing_paths() {
        let mut heap = Heap::default();
        let set = heap.alloc_set(None).unwrap();
        heap.set_add(set, Value::Number(1.0)).unwrap();
        // An element that is already present neither grows nor charges.
        heap.set_add(set, Value::Number(1.0)).unwrap();
        assert_eq!(heap.set_size(set), Ok(1));

        // Symbols can be weak targets too.
        let symbol = Value::Symbol(JsSymbol::well_known("iterator"));
        let weak_ref = heap.alloc_weak_ref(symbol.clone(), None).unwrap();
        assert_eq!(heap.weak_ref_target(weak_ref), Ok(Some(symbol.clone())));
        let registry = heap.alloc_finalization_registry(Value::Null, None).unwrap();
        assert_eq!(
            heap.finalization_registry_register(registry, symbol, Value::Null, None),
            Ok(())
        );
    }

    #[test]
    fn a_weak_reference_to_a_collected_object_has_no_target() {
        let mut heap = Heap::default();
        let target = heap.alloc_object(None).unwrap();
        let weak_ref = heap.alloc_weak_ref(Value::Object(target), None).unwrap();
        heap.root(weak_ref).unwrap();
        assert_eq!(
            heap.weak_ref_target(weak_ref),
            Ok(Some(Value::Object(target)))
        );
        // Nothing but the weak reference itself names the target.
        heap.collect_major();
        assert_eq!(heap.weak_ref_target(weak_ref), Ok(None));
    }

    #[test]
    fn a_private_setter_may_come_before_its_getter() {
        let mut heap = Heap::default();
        let owner = heap.alloc_object(None).unwrap();
        let setter = Value::Object(heap.alloc_object(None).unwrap());
        let getter = Value::Object(heap.alloc_object(None).unwrap());
        heap.define_private_accessor(owner, "a".into(), setter, true)
            .unwrap();
        heap.define_private_accessor(owner, "a".into(), getter, false)
            .unwrap();
    }
}
