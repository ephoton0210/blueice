// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

static NEXT_HEAP: AtomicU64 = AtomicU64::new(1);

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
