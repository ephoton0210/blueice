// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property descriptors, ordinary object storage, and array length updates.

use super::*;

/// Apply OrdinaryOwnPropertyKeys ordering to stored keys, after any exotic
/// virtual indices and strings have been supplied by the caller.
pub(super) fn ordered_stored_property_keys(
    order: &[PropertyName],
    mut indices: Vec<(usize, PropertyName)>,
    mut strings: Vec<PropertyName>,
) -> Vec<PropertyName> {
    let mut symbols = Vec::new();
    for key in order {
        if matches!(key, PropertyName::Symbol(_)) {
            symbols.push(key.clone());
            continue;
        }
        match array_index(key) {
            Some(index) => indices.push((index as usize, key.clone())),
            None => strings.push(key.clone()),
        }
    }
    indices.sort_unstable_by_key(|(index, _)| *index);
    let mut keys = Vec::with_capacity(indices.len() + strings.len() + symbols.len());
    for (_, key) in indices {
        keys.push(key);
    }
    keys.extend(strings);
    keys.extend(symbols);
    keys
}

fn array_length_mut(object: &mut Object) -> Option<&mut u32> {
    match &mut object.kind {
        ObjectKind::Array { length } => Some(length),
        _ => None,
    }
}

impl Heap {
    pub(super) fn get_own_property_descriptor_key(
        &self,
        object: ObjectId,
        key: PropertyName,
    ) -> Result<Option<PropertyDescriptor>, HeapError> {
        if let Some(numeric) = self.typed_array_numeric_key(object, &key)? {
            match numeric {
                TypedArrayNumericKey::Index(index) => {
                    if let Some(value) = self.typed_array_index_value(object, index)? {
                        // An element of an immutable-buffer view is neither
                        // writable nor configurable.
                        let mutable = !self
                            .typed_array_is_immutable(object)
                            .expect("the element read validated its backing buffer");
                        return Ok(Some(PropertyDescriptor::data(
                            value, mutable, true, mutable,
                        )));
                    }
                }
                TypedArrayNumericKey::Invalid => return Ok(None),
            }
            return Ok(None);
        }
        if let Some(cell) = self.module_namespace_export_cell(object, &key)? {
            let value = self
                .get_own(cell, "value")?
                .ok_or(HeapError::UninitializedModuleExport)?;
            return Ok(Some(PropertyDescriptor::data(value, true, true, false)));
        }
        let mapped_cell = self.arguments_parameter_cell(object, &key)?;
        let obj = &self.objects[&object];
        if let Some(descriptor) = obj.attributes.get(&key) {
            let mut descriptor = descriptor.clone();
            if !descriptor.accessor() {
                descriptor.value = match mapped_cell {
                    Some(cell) => self.get_own(cell, "value")?,
                    None => obj.own_property(&key),
                };
            }
            return Ok(Some(descriptor));
        }
        let Some(value) = obj.own_property(&key) else {
            return Ok(None);
        };
        let value = match mapped_cell {
            Some(cell) => self.get_own(cell, "value")?.unwrap_or(value),
            None => value,
        };
        let string_virtual =
            matches!(&obj.kind, ObjectKind::String(s) if string_property(s, &key).is_some());
        let length = match &obj.kind {
            ObjectKind::Array { .. } | ObjectKind::String(_) => key == "length",
            _ => false,
        };
        Ok(Some(PropertyDescriptor::data(
            value,
            !string_virtual,
            !length,
            !string_virtual && !length,
        )))
    }

    pub fn define_own_property(
        &mut self,
        object: ObjectId,
        key: impl Into<PropertyName>,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, HeapError> {
        self.define_own_property_key(object, key.into(), descriptor)
    }

    pub(super) fn define_own_property_key(
        &mut self,
        object: ObjectId,
        key: PropertyName,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, HeapError> {
        if descriptor.accessor() && (descriptor.value.is_some() || descriptor.writable.is_some()) {
            return Ok(false);
        }
        if let Some(cell) = self.module_namespace_export_cell(object, &key)? {
            let current = self
                .get_own(cell, "value")?
                .ok_or(HeapError::UninitializedModuleExport)?;
            if descriptor.configurable == Some(true)
                || descriptor.enumerable == Some(false)
                || descriptor.accessor()
                || descriptor.writable == Some(false)
            {
                return Ok(false);
            }
            return Ok(descriptor
                .value
                .as_ref()
                .is_none_or(|value| same_value(value, &current)));
        }
        if self.is_module_namespace(object)? && matches!(key, PropertyName::String(_)) {
            return Ok(false);
        }
        let mapped_cell = self
            .arguments_parameter_cell(object, &key)
            .expect("the receiver was validated above");
        let old = self.get_own_property_descriptor(object, &key)?;
        if old.is_none() && !self.objects[&object].extensible {
            return Ok(false);
        }
        let obj = &self.objects[&object];
        if let ObjectKind::Array { length } = obj.kind {
            let length_read_only = match obj.attributes.get(&"length".into()) {
                Some(attributes) => attributes.writable == Some(false),
                None => false,
            };
            if array_index(&key).is_some_and(|index| index >= length) && length_read_only {
                return Ok(false);
            }
        }
        if let Some(old) = &old {
            if old.configurable == Some(false) {
                if descriptor.configurable == Some(true)
                    || matches!(descriptor.enumerable, Some(value) if Some(value) != old.enumerable)
                {
                    return Ok(false);
                }
                let changes_kind = if descriptor.accessor() {
                    !old.accessor()
                } else {
                    (descriptor.value.is_some() || descriptor.writable.is_some()) && old.accessor()
                };
                if changes_kind {
                    return Ok(false);
                }
                if old.accessor() {
                    if matches!(descriptor.get.as_ref(), Some(value) if !same_value(value, old.get.as_ref().unwrap()))
                        || matches!(descriptor.set.as_ref(), Some(value) if !same_value(value, old.set.as_ref().unwrap()))
                    {
                        return Ok(false);
                    }
                } else if old.writable == Some(false)
                    && (descriptor.writable == Some(true)
                        || matches!(descriptor.value.as_ref(), Some(value) if !same_value(value, old.value.as_ref().unwrap())))
                {
                    return Ok(false);
                }
            }
        }
        let descriptor_is_accessor = descriptor.accessor();
        let descriptor_non_writable = descriptor.writable == Some(false);
        let descriptor_value = descriptor.value.clone();
        let mut merged = old
            .clone()
            .unwrap_or_else(|| PropertyDescriptor::data(Value::Undefined, false, false, false));
        if descriptor.accessor() && !merged.accessor() {
            merged.value = None;
            merged.writable = None;
            merged.get = Some(Value::Undefined);
            merged.set = Some(Value::Undefined);
        } else if merged.accessor() && (descriptor.value.is_some() || descriptor.writable.is_some())
        {
            merged.get = None;
            merged.set = None;
            merged.value = Some(Value::Undefined);
            merged.writable = Some(false);
        }
        macro_rules! merge { ($($field:ident),*) => { $(if descriptor.$field.is_some() { merged.$field = descriptor.$field; })* }; }
        merge!(value, writable, get, set, enumerable, configurable);
        let obj = &self.objects[&object];
        if matches!(&obj.kind, ObjectKind::String(s) if string_property(s, &key).is_some()) {
            return Ok(true);
        }
        let virtual_length = key == "length" && matches!(obj.kind, ObjectKind::Array { .. });
        let old_attributes = obj
            .attributes
            .get(&key)
            .map_or(0, |d| attribute_bytes(&key, d));
        let old_property = obj
            .properties
            .get(&key)
            .map_or(0, |v| property_bytes(&key, v));
        let value = merged.value.take().unwrap_or(Value::Undefined);
        let new_property = if virtual_length {
            0
        } else {
            property_bytes(&key, &value)
        };
        let new_attributes = attribute_bytes(&key, &merged);
        let protected: Vec<_> = std::iter::once(object)
            .chain(value.object_id())
            .chain(
                merged
                    .get
                    .iter()
                    .chain(merged.set.iter())
                    .filter_map(Value::object_id),
            )
            .collect();
        for &id in &protected {
            self.object(id)?;
        }
        self.ensure_room(
            (new_property + new_attributes).saturating_sub(old_property + old_attributes),
            &protected,
        )?;
        let length_failed = if virtual_length {
            match self.set_array_length(object, value.clone()) {
                Ok(()) => false,
                Err(HeapError::ReadOnlyProperty) => true,
                Err(error) => return Err(error),
            }
        } else {
            false
        };
        for &target in protected.iter().skip(1) {
            self.write_barrier(object, Some(target));
        }
        let obj = self.objects.get_mut(&object).unwrap();
        if !virtual_length {
            if !obj.properties.contains_key(&key) {
                obj.order.push(key.clone());
                self.structure_epoch += 1;
            }
            if let ObjectKind::Array { length } = &mut obj.kind {
                if let Some(index) = array_index(&key) {
                    *length = (*length).max(index + 1);
                }
            }
            obj.properties.insert(key.clone(), value);
        }
        obj.attributes.insert(key.clone(), merged);
        obj.bytes = obj.bytes - old_property - old_attributes + new_property + new_attributes;
        self.managed_bytes =
            self.managed_bytes - old_property - old_attributes + new_property + new_attributes;
        let mapped_update = if length_failed || descriptor_is_accessor {
            None
        } else {
            mapped_cell.zip(descriptor_value)
        };
        mapped_update
            .map(|(cell, value)| self.set(cell, "value", value))
            .transpose()?;
        if !length_failed
            && mapped_cell.is_some()
            && (descriptor_is_accessor || descriptor_non_writable)
        {
            self.unmap_arguments_property(object, &key)
                .expect("the receiver was protected across collection");
        }
        Ok(!length_failed)
    }

    pub fn is_array(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::Array { .. }
        ))
    }

    pub(crate) fn is_arguments(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::Arguments { .. }
        ))
    }

    pub(super) fn arguments_parameter_cell(
        &self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<ObjectId>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Arguments { parameter_map } => parameter_map.get(key).copied(),
            _ => None,
        })
    }

    pub(crate) fn is_module_namespace(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::ModuleNamespace { .. }
        ))
    }

    pub(super) fn module_namespace_export_cell(
        &self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<ObjectId>, HeapError> {
        let PropertyName::String(name) = key else {
            return Ok(None);
        };
        Ok(match &self.object(object)?.kind {
            ObjectKind::ModuleNamespace { exports } => exports
                .iter()
                .find_map(|(export, cell)| (export == name).then_some(*cell)),
            _ => None,
        })
    }

    pub(super) fn unmap_arguments_property(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<(), HeapError> {
        let obj = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        if let ObjectKind::Arguments { parameter_map } = &mut obj.kind {
            parameter_map.remove(key);
        }
        Ok(())
    }

    pub(super) fn alloc(
        &mut self,
        kind: ObjectKind,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        if let Some(id) = prototype {
            self.object(id)?;
        }
        let next = self
            .next_object
            .checked_add(1)
            .ok_or(HeapError::IdExhausted)?;
        let mut protected = None;
        if self.nursery.len() >= self.config.nursery_capacity {
            protected = Some(allocation_references(&kind, prototype));
            self.minor_gc(protected.as_deref().expect("allocated above"));
        }
        let bytes = OBJECT_BYTES
            + match &kind {
                ObjectKind::String(string)
                | ObjectKind::StringIterator { string, .. }
                | ObjectKind::RegExpIterator { string, .. } => string.byte_len(),
                ObjectKind::ArrayBuffer { bytes, .. } => bytes.len(),
                ObjectKind::WebFileList(files) => files.len().saturating_mul(size_of::<ObjectId>()),
                ObjectKind::WebBlob(blob) => blob
                    .bytes
                    .len()
                    .saturating_add(blob.media_type.byte_len())
                    .saturating_add(blob.file.as_ref().map_or(0, |(name, _)| name.byte_len())),
                ObjectKind::RegExp(regexp) => exotic::regexp_bytes(regexp),
                ObjectKind::Collator { data, .. } => data.bytes(),
                ObjectKind::NumberFormat { data, .. } => data.bytes(),
                ObjectKind::DateTimeFormat { data, .. } => data.bytes(),
                ObjectKind::DisplayNames(data) => data.bytes(),
                ObjectKind::DurationFormat(data) => data.bytes(),
                ObjectKind::ListFormat(data) => data.bytes(),
                ObjectKind::PluralRules(data) => data.bytes(),
                ObjectKind::RelativeTimeFormat(data) => data.bytes(),
                ObjectKind::Segmenter(data) => data.bytes(),
                ObjectKind::Segments(data) => data.bytes(),
                ObjectKind::SegmentIterator { .. } => 0,
                ObjectKind::IntlLocale(data) => data.bytes(),
                ObjectKind::Temporal(value) => value.bytes(),
                ObjectKind::BoxedPrimitive(value) => value.payload_bytes(),
                ObjectKind::NativeFunction { initial_name, .. } => {
                    // Preserve the same managed UTF-16 charge as JsString.
                    initial_name.encode_utf16().count() * size_of::<u16>()
                }
                ObjectKind::Closure { captures, this, .. } => {
                    captures.len() * size_of::<ObjectId>() + this.payload_bytes()
                }
                ObjectKind::Generator { state_bytes, .. } => *state_bytes,
                ObjectKind::BoundFunction(bound) => {
                    bound.this.payload_bytes()
                        + bound.args.len() * size_of::<Value>()
                        + bound.args.iter().map(Value::payload_bytes).sum::<usize>()
                }
                ObjectKind::Arguments { parameter_map } => {
                    parameter_map.len() * size_of::<(PropertyName, ObjectId)>()
                }
                ObjectKind::ModuleNamespace { exports } => {
                    let mut bytes = 0;
                    for (name, _) in exports {
                        bytes += name.byte_len() + size_of::<(JsString, ObjectId)>();
                    }
                    bytes
                }
                _ => 0,
            };
        // Below the major-collection trigger (which never exceeds the heap
        // limit) there is nothing to collect and nothing to refuse.
        if self
            .managed_bytes
            .checked_add(bytes)
            .is_none_or(|total| total >= self.next_major_bytes)
        {
            let protected =
                protected.get_or_insert_with(|| allocation_references(&kind, prototype));
            self.ensure_room(bytes, protected)?;
        }
        let id = ObjectId {
            heap: self.identity,
            serial: self.next_object,
        };
        self.next_object = next;
        self.objects.insert(
            id,
            Object {
                kind,
                is_html_dda: false,
                properties: HashMap::new(),
                order: Vec::new(),
                attributes: HashMap::new(),
                private: None,
                extensible: true,
                prototype,
                young: true,
                bytes,
            },
        );
        self.nursery.push(id);
        self.managed_bytes += bytes;
        Ok(id)
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects.contains_key(&id)
    }

    /// Registers an independent root. Does not collect. The caller is
    /// responsible for releasing it once the interpreter/host no longer
    /// needs this value; a copied ObjectId alone is not a GC root.
    pub fn root(&mut self, object: ObjectId) -> Result<RootId, HeapError> {
        self.object(object)?;
        let next = self
            .next_root
            .checked_add(1)
            .ok_or(HeapError::IdExhausted)?;
        let id = RootId {
            heap: self.identity,
            serial: self.next_root,
        };
        self.next_root = next;
        self.roots.insert(id, object);
        self.root_registrations += 1;
        Ok(id)
    }

    /// Releases exactly this registration, without collecting immediately.
    pub fn unroot(&mut self, root: RootId) -> Result<ObjectId, HeapError> {
        self.roots.remove(&root).ok_or(HeapError::InvalidRoot(root))
    }

    /// `None` means absent, distinct from a present `Value::Undefined`.
    pub fn get_own(
        &self,
        object: ObjectId,
        key: impl Into<PropertyName>,
    ) -> Result<Option<Value>, HeapError> {
        self.get_own_key(object, key.into())
    }

    fn get_own_key(&self, object: ObjectId, key: PropertyName) -> Result<Option<Value>, HeapError> {
        if let Some(numeric) = self.typed_array_numeric_key(object, &key)? {
            return match numeric {
                TypedArrayNumericKey::Index(index) => self.typed_array_index_value(object, index),
                TypedArrayNumericKey::Invalid => Ok(None),
            };
        }
        if let Some(cell) = self
            .module_namespace_export_cell(object, &key)
            .expect("the receiver was validated above")
        {
            return self
                .get_own(cell, "value")?
                .map(Some)
                .ok_or(HeapError::UninitializedModuleExport);
        }
        Ok(self.objects[&object].own_property(&key))
    }

    /// Ordinary data-property lookup through the prototype chain.
    pub fn get(&self, object: ObjectId, key: impl Into<PropertyName>) -> Result<Value, HeapError> {
        self.get_key(object, key.into())
    }

    pub(super) fn get_key(&self, object: ObjectId, key: PropertyName) -> Result<Value, HeapError> {
        let mut current = Some(object);
        while let Some(id) = current {
            if let Some(numeric) = self.typed_array_numeric_key(id, &key)? {
                return match numeric {
                    TypedArrayNumericKey::Index(index) => Ok(self
                        .typed_array_index_value(id, index)?
                        .unwrap_or(Value::Undefined)),
                    TypedArrayNumericKey::Invalid => Ok(Value::Undefined),
                };
            }
            if let Some(cell) = self
                .module_namespace_export_cell(id, &key)
                .expect("the receiver was validated above")
            {
                return self
                    .get_own(cell, "value")?
                    .ok_or(HeapError::UninitializedModuleExport);
            }
            if let Some(cell) = self
                .arguments_parameter_cell(id, &key)
                .expect("the receiver was validated above")
            {
                return Ok(self.get_own(cell, "value")?.unwrap_or(Value::Undefined));
            }
            let obj = &self.objects[&id];
            if let Some(value) = obj.own_property(&key) {
                return Ok(value);
            }
            current = obj.prototype;
        }
        Ok(Value::Undefined)
    }

    /// Creates or replaces an own data property. Inputs are protected
    /// across a pressure collection; a budget error leaves the property
    /// and insertion order unchanged. It may still reclaim unrelated garbage.
    /// Array length writes require a pre-coerced Number, validated as an
    /// integer in 0..=u32::MAX. Truncation visits present properties only;
    /// holes cost no storage, and successful index stores grow length.
    pub fn set(
        &mut self,
        object: ObjectId,
        key: impl Into<PropertyName>,
        value: Value,
    ) -> Result<(), HeapError> {
        self.set_key(object, key.into(), value)
    }

    pub(super) fn set_key(
        &mut self,
        object: ObjectId,
        key: PropertyName,
        value: Value,
    ) -> Result<(), HeapError> {
        if self.typed_array_numeric_key(object, &key)?.is_some() {
            return Err(HeapError::ReadOnlyProperty);
        }
        if self
            .module_namespace_export_cell(object, &key)
            .expect("the receiver was validated above")
            .is_some()
        {
            return Err(HeapError::ReadOnlyProperty);
        }
        let mapped_cell = self
            .arguments_parameter_cell(object, &key)
            .expect("the receiver was validated above");
        let obj = &self.objects[&object];
        if obj
            .attributes
            .get(&key)
            .is_some_and(|d| d.accessor() || d.writable == Some(false))
        {
            return Err(HeapError::ReadOnlyProperty);
        }
        if !obj.extensible && obj.own_property(&key).is_none() {
            return Err(HeapError::ReadOnlyProperty);
        }
        if let ObjectKind::Array { length } = obj.kind {
            let length_read_only = match obj.attributes.get(&"length".into()) {
                Some(attributes) => attributes.writable == Some(false),
                None => false,
            };
            if array_index(&key).is_some_and(|index| index >= length) && length_read_only {
                return Err(HeapError::ReadOnlyProperty);
            }
        }
        if matches!(&obj.kind, ObjectKind::String(string) if string_property(string, &key).is_some())
        {
            return Err(HeapError::ReadOnlyProperty);
        }
        let old_bytes = obj
            .properties
            .get(&key)
            .map_or(0, |old| property_bytes(&key, old));
        let value_id = value.object_id();
        if let Some(id) = value_id {
            self.object(id)?;
        }
        if key == "length" && matches!(obj.kind, ObjectKind::Array { .. }) {
            return self.set_array_length(object, value);
        }
        let new_bytes = property_bytes(&key, &value);
        let protected: Vec<_> = std::iter::once(object).chain(value_id).collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        self.write_barrier(object, value_id);
        let mapped_value = if mapped_cell.is_some() {
            Some(value.clone())
        } else {
            None
        };
        {
            let obj = self
                .objects
                .get_mut(&object)
                .expect("the receiver is protected across collection");
            if !obj.properties.contains_key(&key) {
                obj.order.push(key.clone());
                self.structure_epoch += 1;
            }
            if let ObjectKind::Array { length } = &mut obj.kind {
                if let Some(index) = array_index(&key) {
                    *length = (*length).max(index + 1);
                }
            }
            obj.properties.insert(key, value);
            obj.bytes = obj.bytes - old_bytes + new_bytes;
        }
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        if let (Some(cell), Some(value)) = (mapped_cell, mapped_value) {
            self.set(cell, "value", value)?;
        }
        Ok(())
    }

    /// Deleting a missing property succeeds, as it does for JS ordinary
    /// objects. Array length and boxed String indices/length cannot be
    /// deleted; deleting an array index does not change length.
    pub fn delete(
        &mut self,
        object: ObjectId,
        key: impl Into<PropertyName>,
    ) -> Result<bool, HeapError> {
        self.delete_key(object, key.into())
    }

    pub(super) fn delete_key(
        &mut self,
        object: ObjectId,
        key: PropertyName,
    ) -> Result<bool, HeapError> {
        if let Some(numeric) = self.typed_array_numeric_key(object, &key)? {
            let TypedArrayNumericKey::Index(index) = numeric else {
                return Ok(true);
            };
            let (buffer, _, length, _) = self.typed_array_info(object)?;
            return Ok(self
                .buffer_is_detached(buffer)
                .expect("typed_array_info validated the backing buffer")
                || index >= length);
        }
        if self
            .module_namespace_export_cell(object, &key)
            .expect("the receiver was validated above")
            .is_some()
        {
            return Ok(false);
        }
        if self
            .is_module_namespace(object)
            .expect("the receiver was validated above")
            && matches!(key, PropertyName::String(_))
        {
            return Ok(true);
        }
        let mapped_cell = self
            .arguments_parameter_cell(object, &key)
            .expect("the receiver was validated above");
        let obj = self
            .objects
            .get_mut(&object)
            .expect("the receiver was validated above");
        if obj
            .attributes
            .get(&key)
            .is_some_and(|d| d.configurable == Some(false))
        {
            return Ok(false);
        }
        if matches!(&obj.kind, ObjectKind::String(string) if string_property(string, &key).is_some())
        {
            return Ok(false);
        }
        if key == "length" && matches!(obj.kind, ObjectKind::Array { .. }) {
            return Ok(false);
        }
        if let Some(value) = obj.properties.remove(&key) {
            let removed_attributes = match obj.attributes.remove(&key) {
                Some(descriptor) => attribute_bytes(&key, &descriptor),
                None => 0,
            };
            let bytes = property_bytes(&key, &value) + removed_attributes;
            obj.order.retain(|name| name != &key);
            obj.bytes -= bytes;
            self.managed_bytes -= bytes;
            self.structure_epoch += 1;
        }
        if mapped_cell.is_some() {
            self.unmap_arguments_property(object, &key)
                .expect("the receiver was validated above");
        }
        Ok(true)
    }

    /// ECMAScript OrdinaryOwnPropertyKeys: array indices first, then other
    /// strings in creation order, then Symbols in creation order.
    /// Includes non-enumerable virtual length, created before stored strings.
    /// 2^32-1 and noncanonical spellings are not indices.
    pub fn own_property_keys(&self, object: ObjectId) -> Result<Vec<PropertyName>, HeapError> {
        let obj = self.object(object)?;
        if let ObjectKind::ModuleNamespace { exports } = &obj.kind {
            let mut keys = Vec::with_capacity(exports.len() + obj.order.len());
            for (export, _) in exports {
                keys.push(PropertyName::String(export.clone()));
            }
            // Namespace string keys live in `exports`; its own stored keys
            // are symbols, and the namespace is non-extensible after setup.
            keys.extend(obj.order.iter().cloned());
            return Ok(keys);
        }
        let mut indices = Vec::new();
        let mut strings = Vec::new();
        if let ObjectKind::String(string) = &obj.kind {
            for index in 0..string.len() {
                indices.push((index, index.to_string().into()));
            }
        }
        if let ObjectKind::TypedArray { buffer, .. } = &obj.kind {
            if !self.buffer_is_detached(*buffer)? {
                let (_, _, length, _) = self
                    .typed_array_info(object)
                    .expect("the backing buffer was validated above");
                for index in 0..length {
                    indices.push((index, index.to_string().into()));
                }
            }
        }
        if matches!(obj.kind, ObjectKind::Array { .. } | ObjectKind::String(_)) {
            strings.push("length".into());
        }
        Ok(ordered_stored_property_keys(&obj.order, indices, strings))
    }

    /// Ascending canonical integer-index keys (`"0"`, `"1"`, ...) of every own
    /// string property stored on `object`, or `None` when an index can exist
    /// without being stored (Proxy traps, String and TypedArray elements,
    /// module namespace exports) and the stored keys therefore say nothing
    /// about which indices an `[[HasProperty]]` probe could find.
    pub(crate) fn own_integer_keys(&self, object: ObjectId) -> Result<Option<Vec<u64>>, HeapError> {
        let obj = self.object(object)?;
        if matches!(
            obj.kind,
            ObjectKind::Proxy { .. }
                | ObjectKind::String(_)
                | ObjectKind::TypedArray { .. }
                | ObjectKind::ModuleNamespace { .. }
        ) {
            return Ok(None);
        }
        let mut keys: Vec<u64> = obj
            .order
            .iter()
            .filter_map(|key| key.index().map(|index| index as u64))
            .collect();
        keys.sort_unstable();
        Ok(Some(keys))
    }

    pub(crate) fn structure_epoch(&self) -> u64 {
        self.structure_epoch
    }

    pub fn own_keys(&self, object: ObjectId) -> Result<Vec<JsString>, HeapError> {
        Ok(self
            .own_property_keys(object)?
            .into_iter()
            .filter_map(|key| match key {
                PropertyName::String(s) => Some(s),
                _ => None,
            })
            .collect())
    }

    /// The enumerable subset of own_keys, excluding virtual array length.
    /// Inherited properties are not returned by either key enumeration API.
    pub fn enumerable_own_keys(&self, object: ObjectId) -> Result<Vec<JsString>, HeapError> {
        let virtual_length = matches!(
            self.object(object)?.kind,
            ObjectKind::Array { .. } | ObjectKind::String(_)
        );
        let mut keys = Vec::new();
        for key in self.own_keys(object)? {
            if virtual_length && key == "length" {
                continue;
            }
            let enumerable = match self.objects[&object]
                .attributes
                .get(&PropertyName::from(&key))
            {
                Some(descriptor) => descriptor.enumerable == Some(true),
                None => true,
            };
            if enumerable {
                keys.push(key);
            }
        }
        Ok(keys)
    }

    pub(super) fn set_array_length(
        &mut self,
        object: ObjectId,
        value: Value,
    ) -> Result<(), HeapError> {
        let Value::Number(number) = value else {
            return Err(HeapError::InvalidArrayLength);
        };
        if !(0.0..=f64::from(u32::MAX)).contains(&number) || number.fract() != 0.0 {
            return Err(HeapError::InvalidArrayLength);
        }
        let new_length = number as u32;
        let obj = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let old_length = *array_length_mut(obj).ok_or(HeapError::InvalidInternalSlot(object))?;
        if new_length < old_length {
            let mut indices = Vec::new();
            for key in &obj.order {
                if let Some(index) = array_index(key) {
                    if index >= new_length {
                        indices.push((index, key.clone()));
                    }
                }
            }
            indices.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
            for (index, key) in indices {
                if !self
                    .delete(object, &key)
                    .expect("the array remains live while truncating its properties")
                {
                    *array_length_mut(self.objects.get_mut(&object).unwrap())
                        .expect("the receiver remains an array") = index + 1;
                    return Err(HeapError::ReadOnlyProperty);
                }
            }
        }
        *array_length_mut(self.objects.get_mut(&object).unwrap())
            .expect("the receiver remains an array") = new_length;
        Ok(())
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    fn heap() -> Heap {
        Heap::new(HeapConfig::default()).unwrap()
    }

    fn number(value: f64) -> Value {
        Value::Number(value)
    }

    /// An id this heap once handed out and has since reclaimed.
    fn stale(heap: &mut Heap) -> ObjectId {
        let id = heap.alloc_object(None).unwrap();
        heap.collect_major();
        assert!(!heap.contains(id));
        id
    }

    /// A rooted object holding `value` under `"value"`, like a binding cell.
    fn cell(heap: &mut Heap, value: Option<Value>) -> ObjectId {
        let cell = heap.alloc_object(None).unwrap();
        heap.root(cell).unwrap();
        if let Some(value) = value {
            heap.set(cell, "value", value).unwrap();
        }
        cell
    }

    /// A rooted arguments object whose parameter `"0"` is mapped to a cell.
    fn arguments_object(heap: &mut Heap) -> (ObjectId, ObjectId) {
        let prototype = heap.alloc_object(None).unwrap();
        heap.root(prototype).unwrap();
        let cell = cell(heap, Some(number(1.0)));
        let arguments = heap
            .alloc_arguments(HashMap::from([("0".into(), cell)]), prototype)
            .unwrap();
        heap.root(arguments).unwrap();
        heap.set(arguments, "0", number(1.0)).unwrap();
        (arguments, cell)
    }

    /// A rooted Uint16 view of four elements over a rooted eight-byte buffer.
    fn typed_array(heap: &mut Heap) -> (ObjectId, ObjectId) {
        let buffer = heap.alloc_array_buffer(8, None).unwrap();
        heap.root(buffer).unwrap();
        let array = heap
            .alloc_typed_array(buffer, 0, 4, false, TypedArrayKind::Uint16, None)
            .unwrap();
        heap.root(array).unwrap();
        (array, buffer)
    }

    /// A rooted namespace exporting `a` (7) and `b` (an uninitialized binding).
    fn namespace(heap: &mut Heap) -> ObjectId {
        let ready = cell(heap, Some(number(7.0)));
        let uninitialized = cell(heap, None);
        let namespace = heap
            .alloc_module_namespace(
                vec![("b".into(), uninitialized), ("a".into(), ready)],
                false,
            )
            .unwrap();
        heap.root(namespace).unwrap();
        namespace
    }

    #[test]
    fn every_entry_point_rejects_a_reclaimed_object() {
        let mut heap = heap();
        let gone = stale(&mut heap);
        let invalid = Some(HeapError::InvalidObject(gone));
        let key = PropertyName::from("k");
        assert_eq!(heap.is_array(gone).err(), invalid);
        assert_eq!(heap.is_arguments(gone).err(), invalid);
        assert_eq!(heap.is_module_namespace(gone).err(), invalid);
        assert_eq!(heap.arguments_parameter_cell(gone, &key).err(), invalid);
        assert_eq!(heap.module_namespace_export_cell(gone, &key).err(), invalid);
        assert_eq!(heap.unmap_arguments_property(gone, &key).err(), invalid);
        assert_eq!(
            heap.get_own_property_descriptor_key(gone, key.clone())
                .err(),
            invalid
        );
        assert_eq!(heap.get_own_key(gone, key.clone()).err(), invalid);
        assert_eq!(heap.get_key(gone, key.clone()).err(), invalid);
        assert_eq!(heap.set_key(gone, key.clone(), number(1.0)).err(), invalid);
        assert_eq!(heap.delete_key(gone, key.clone()).err(), invalid);
        assert_eq!(heap.own_property_keys(gone).err(), invalid);
        assert_eq!(heap.own_integer_keys(gone).err(), invalid);
        assert_eq!(heap.own_keys(gone).err(), invalid);
        assert_eq!(heap.enumerable_own_keys(gone).err(), invalid);
        assert_eq!(heap.root(gone).err(), invalid);
        assert_eq!(heap.alloc_object(Some(gone)).err(), invalid);
    }

    #[test]
    fn the_kind_of_an_object_is_told_apart() {
        let mut heap = heap();
        let plain = heap.alloc_object(None).unwrap();
        let array = heap.alloc_array(0, None).unwrap();
        let (arguments, cell) = arguments_object(&mut heap);
        let namespace = namespace(&mut heap);
        let key = PropertyName::from("0");
        assert_eq!(heap.is_array(array), Ok(true));
        assert_eq!(heap.is_array(plain), Ok(false));
        assert_eq!(heap.is_arguments(arguments), Ok(true));
        assert_eq!(heap.is_arguments(plain), Ok(false));
        assert_eq!(heap.is_module_namespace(namespace), Ok(true));
        assert_eq!(heap.is_module_namespace(plain), Ok(false));
        assert_eq!(
            heap.arguments_parameter_cell(arguments, &key),
            Ok(Some(cell))
        );
        assert_eq!(heap.arguments_parameter_cell(plain, &key), Ok(None));
        assert_eq!(
            heap.module_namespace_export_cell(namespace, &"zzz".into()),
            Ok(None)
        );
        assert_eq!(
            heap.module_namespace_export_cell(
                namespace,
                &JsSymbol::well_known("toStringTag").into()
            ),
            Ok(None)
        );
        assert_eq!(
            heap.module_namespace_export_cell(plain, &"a".into()),
            Ok(None)
        );
        assert!(heap
            .module_namespace_export_cell(namespace, &"a".into())
            .unwrap()
            .is_some());
        // Unmapping removes the parameter from the arguments object only.
        assert_eq!(heap.unmap_arguments_property(plain, &key), Ok(()));
        assert_eq!(heap.unmap_arguments_property(arguments, &key), Ok(()));
        assert_eq!(heap.arguments_parameter_cell(arguments, &key), Ok(None));
    }

    #[test]
    fn an_allocation_that_does_not_fit_reports_the_limit() {
        let mut heap = Heap::new(HeapConfig {
            nursery_capacity: 16,
            major_threshold_bytes: 1024,
            max_heap_bytes: 4096,
        })
        .unwrap();
        assert_eq!(
            heap.alloc_string("x".repeat(8192).into(), None),
            Err(HeapError::HeapLimitExceeded { limit: 4096 })
        );
    }

    #[test]
    fn a_namespace_reads_through_its_export_cells() {
        let mut heap = heap();
        let namespace = namespace(&mut heap);
        let a = PropertyName::from("a");
        let b = PropertyName::from("b");
        let symbol = PropertyName::from(JsSymbol::well_known("toStringTag"));
        assert_eq!(
            heap.get_own_key(namespace, a.clone()),
            Ok(Some(number(7.0)))
        );
        assert_eq!(
            heap.get_own_key(namespace, b.clone()),
            Err(HeapError::UninitializedModuleExport)
        );
        assert_eq!(heap.get_key(namespace, a.clone()), Ok(number(7.0)));
        assert_eq!(
            heap.get_key(namespace, b.clone()),
            Err(HeapError::UninitializedModuleExport)
        );
        assert_eq!(
            format!(
                "{:?}",
                heap.get_own_property_descriptor_key(namespace, a.clone())
            ),
            format!(
                "{:?}",
                Ok::<_, HeapError>(Some(PropertyDescriptor::data(
                    number(7.0),
                    true,
                    true,
                    false
                )))
            )
        );
        assert_eq!(
            heap.get_own_property_descriptor_key(namespace, b.clone())
                .err(),
            Some(HeapError::UninitializedModuleExport)
        );
        // Every write is refused, and only a compatible redefinition succeeds.
        assert_eq!(
            heap.set_key(namespace, a.clone(), number(1.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        assert_eq!(heap.delete_key(namespace, a.clone()), Ok(false));
        assert_eq!(heap.delete_key(namespace, "zzz".into()), Ok(true));
        assert_eq!(heap.delete_key(namespace, symbol.clone()), Ok(false));
        let same = PropertyDescriptor::data(number(7.0), true, true, false);
        assert_eq!(
            heap.define_own_property_key(namespace, a.clone(), same),
            Ok(true)
        );
        assert_eq!(
            heap.define_own_property_key(
                namespace,
                a.clone(),
                PropertyDescriptor::data(number(8.0), true, true, false)
            ),
            Ok(false)
        );
        assert_eq!(
            heap.define_own_property_key(
                namespace,
                b,
                PropertyDescriptor::data(number(8.0), true, true, false)
            ),
            Err(HeapError::UninitializedModuleExport)
        );
        assert_eq!(
            heap.define_own_property_key(
                namespace,
                "zzz".into(),
                PropertyDescriptor::data(number(8.0), true, true, true)
            ),
            Ok(false)
        );
        assert_eq!(
            heap.own_property_keys(namespace),
            Ok(vec!["a".into(), "b".into(), symbol])
        );
        assert_eq!(heap.own_integer_keys(namespace), Ok(None));
    }

    #[test]
    fn a_typed_array_owns_only_canonical_numeric_keys() {
        let mut heap = heap();
        let (array, buffer) = typed_array(&mut heap);
        let value = |key: &str| heap.get_own_key(array, key.into());
        assert_eq!(value("1"), Ok(Some(number(0.0))));
        assert_eq!(value("9"), Ok(None));
        assert_eq!(value("-0"), Ok(None));
        assert_eq!(value("name"), Ok(None));
        assert_eq!(heap.get_key(array, "1".into()), Ok(number(0.0)));
        assert_eq!(heap.get_key(array, "9".into()), Ok(Value::Undefined));
        assert_eq!(heap.get_key(array, "-0".into()), Ok(Value::Undefined));
        assert_eq!(
            heap.set_key(array, "1".into(), number(3.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        assert_eq!(
            heap.set_key(array, "-0".into(), number(3.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        assert_eq!(
            format!(
                "{:?}",
                heap.get_own_property_descriptor_key(array, "2".into())
            ),
            format!(
                "{:?}",
                Ok::<_, HeapError>(Some(PropertyDescriptor::data(
                    number(0.0),
                    true,
                    true,
                    true
                )))
            )
        );
        assert_eq!(
            heap.get_own_property_descriptor_key(array, "9".into())
                .map(|d| d.is_none()),
            Ok(true)
        );
        assert_eq!(
            heap.get_own_property_descriptor_key(array, "-0".into())
                .map(|d| d.is_none()),
            Ok(true)
        );
        assert_eq!(
            heap.own_property_keys(array),
            Ok(vec!["0".into(), "1".into(), "2".into(), "3".into()])
        );
        // Deleting an element that exists fails; anything else succeeds.
        assert_eq!(heap.delete_key(array, "1".into()), Ok(false));
        assert_eq!(heap.delete_key(array, "9".into()), Ok(true));
        assert_eq!(heap.delete_key(array, "-0".into()), Ok(true));
        assert_eq!(heap.delete_key(array, "name".into()), Ok(true));
        heap.detach_array_buffer(buffer).unwrap();
        assert_eq!(heap.delete_key(array, "1".into()), Ok(true));
        assert_eq!(heap.own_property_keys(array), Ok(Vec::new()));
    }

    #[test]
    fn an_arguments_object_writes_through_to_its_parameter_cell() {
        let mut heap = heap();
        let (arguments, cell) = arguments_object(&mut heap);
        let key = PropertyName::from("0");
        let read = |heap: &Heap| heap.get_own_key(cell, "value".into()).unwrap();
        heap.set_key(arguments, key.clone(), number(2.0)).unwrap();
        assert_eq!(read(&heap), Some(number(2.0)));
        // A cell write is seen through the arguments object.
        heap.set(cell, "value", number(3.0)).unwrap();
        assert_eq!(heap.get_key(arguments, key.clone()), Ok(number(3.0)));
        assert_eq!(
            heap.get_own_key(arguments, key.clone()),
            Ok(Some(number(2.0)))
        );
        assert_eq!(
            heap.get_own_property_descriptor_key(arguments, key.clone())
                .unwrap()
                .unwrap()
                .value,
            Some(number(3.0))
        );
        // A descriptor without a value keeps the mapping.
        let enumerable = PropertyDescriptor {
            value: None,
            writable: None,
            get: None,
            set: None,
            enumerable: Some(false),
            configurable: None,
        };
        assert_eq!(
            heap.define_own_property_key(arguments, key.clone(), enumerable),
            Ok(true)
        );
        assert_eq!(
            heap.arguments_parameter_cell(arguments, &key),
            Ok(Some(cell))
        );
        assert_eq!(
            heap.get_own_property_descriptor_key(arguments, key.clone())
                .unwrap()
                .unwrap()
                .value,
            Some(number(3.0))
        );
        assert_eq!(heap.get_key(arguments, key.clone()), Ok(number(3.0)));
        // A value is written through, then a read-only descriptor unmaps.
        assert_eq!(
            heap.define_own_property_key(
                arguments,
                key.clone(),
                PropertyDescriptor::data(number(4.0), true, false, true)
            ),
            Ok(true)
        );
        assert_eq!(read(&heap), Some(number(4.0)));
        let read_only = PropertyDescriptor {
            value: None,
            writable: Some(false),
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        };
        assert_eq!(
            heap.define_own_property_key(arguments, key.clone(), read_only),
            Ok(true)
        );
        assert_eq!(heap.arguments_parameter_cell(arguments, &key), Ok(None));
    }

    #[test]
    fn an_accessor_or_a_delete_unmaps_an_arguments_parameter() {
        let mut heap = heap();
        let getter = heap.alloc_object(None).unwrap();
        heap.root(getter).unwrap();
        let accessor = PropertyDescriptor {
            value: None,
            writable: None,
            get: Some(Value::Object(getter)),
            set: None,
            enumerable: None,
            configurable: None,
        };
        let (arguments, _) = arguments_object(&mut heap);
        assert_eq!(
            heap.define_own_property_key(arguments, "0".into(), accessor),
            Ok(true)
        );
        assert_eq!(
            heap.arguments_parameter_cell(arguments, &"0".into()),
            Ok(None)
        );
        let (arguments, _) = arguments_object(&mut heap);
        assert_eq!(heap.delete_key(arguments, "0".into()), Ok(true));
        assert_eq!(
            heap.arguments_parameter_cell(arguments, &"0".into()),
            Ok(None)
        );
        assert_eq!(heap.get_own_key(arguments, "0".into()), Ok(None));
    }

    #[test]
    fn a_parameter_cell_that_cannot_grow_reports_the_limit() {
        let mut failures = 0;
        for extra in 0..600 {
            let mut heap = heap();
            let (arguments, cell) = arguments_object(&mut heap);
            let value = Value::String("x".repeat(200).into());
            heap.config.max_heap_bytes = heap.managed_bytes + extra;
            let write = heap.set_key(arguments, "0".into(), value.clone());
            let define = heap.define_own_property_key(
                arguments,
                "0".into(),
                PropertyDescriptor::data(value, true, true, true),
            );
            failures += usize::from(write.is_err()) + usize::from(define.is_err());
            // A failed write never leaves the cell and the arguments disagreeing.
            if write.is_err() {
                assert_eq!(
                    heap.get_own_key(cell, "value".into()),
                    Ok(Some(number(1.0)))
                );
            }
        }
        assert!(failures > 100, "{failures}");
    }

    #[test]
    fn a_stale_value_or_accessor_is_rejected_before_it_is_stored() {
        let mut heap = heap();
        let gone = stale(&mut heap);
        let object = heap.alloc_object(None).unwrap();
        assert_eq!(
            heap.set_key(object, "k".into(), Value::Object(gone)),
            Err(HeapError::InvalidObject(gone))
        );
        assert_eq!(
            heap.define_own_property_key(
                object,
                "k".into(),
                PropertyDescriptor::data(Value::Object(gone), true, true, true)
            ),
            Err(HeapError::InvalidObject(gone))
        );
        assert_eq!(heap.get_own_key(object, "k".into()), Ok(None));
    }

    #[test]
    fn set_refuses_what_an_object_forbids() {
        let mut heap = heap();
        let readonly = PropertyDescriptor::data(number(1.0), false, true, false);
        let object = heap.alloc_object(None).unwrap();
        heap.define_own_property_key(object, "r".into(), readonly)
            .unwrap();
        assert_eq!(
            heap.set_key(object, "r".into(), number(2.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        heap.set_key(object, "plain".into(), number(1.0)).unwrap();
        heap.prevent_extensions(object).unwrap();
        assert_eq!(
            heap.set_key(object, "added".into(), number(1.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        heap.set_key(object, "plain".into(), number(2.0)).unwrap();
        assert_eq!(heap.get_key(object, "plain".into()), Ok(number(2.0)));

        // A frozen length keeps an array from growing by index.
        let array = heap.alloc_array(1, None).unwrap();
        let frozen_length = PropertyDescriptor {
            value: None,
            writable: Some(false),
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        };
        heap.define_own_property_key(array, "length".into(), frozen_length.clone())
            .unwrap();
        assert_eq!(
            heap.set_key(array, "5".into(), number(1.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        assert_eq!(heap.set_key(array, "0".into(), number(1.0)), Ok(()));
        assert_eq!(
            heap.define_own_property_key(
                array,
                "5".into(),
                PropertyDescriptor::data(number(1.0), true, true, true)
            ),
            Ok(false)
        );

        // A boxed string's own indices are read-only.
        let string = heap.alloc_string("ab".into(), None).unwrap();
        assert_eq!(
            heap.set_key(string, "0".into(), number(1.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        assert_eq!(heap.delete_key(string, "0".into()), Ok(false));
        assert_eq!(heap.delete_key(string, "length".into()), Ok(false));
        assert_eq!(
            heap.own_property_keys(string),
            Ok(vec!["0".into(), "1".into(), "length".into()])
        );
    }

    #[test]
    fn array_lengths_are_validated_and_truncation_stops_at_a_fixed_element() {
        let mut heap = heap();
        let array = heap.alloc_array(0, None).unwrap();
        heap.root(array).unwrap();
        for index in 0..4 {
            heap.set_key(array, index.to_string().into(), number(f64::from(index)))
                .unwrap();
        }
        assert_eq!(
            heap.set_key(array, "length".into(), Value::Null),
            Err(HeapError::InvalidArrayLength)
        );
        for length in [-1.0, 1.5, 4294967296.0, f64::NAN] {
            assert_eq!(
                heap.set_key(array, "length".into(), number(length)),
                Err(HeapError::InvalidArrayLength)
            );
        }
        // Element 1 cannot be deleted, so truncation stops just above it.
        heap.define_own_property_key(
            array,
            "1".into(),
            PropertyDescriptor::data(number(1.0), true, true, false),
        )
        .unwrap();
        assert_eq!(
            heap.set_key(array, "length".into(), number(0.0)),
            Err(HeapError::ReadOnlyProperty)
        );
        assert_eq!(heap.get_key(array, "length".into()), Ok(number(2.0)));
        assert_eq!(heap.set_key(array, "length".into(), number(9.0)), Ok(()));
        assert_eq!(heap.get_key(array, "length".into()), Ok(number(9.0)));
        // Only arrays have a length to set.
        let plain = heap.alloc_object(None).unwrap();
        assert_eq!(
            heap.set_array_length(plain, number(1.0)),
            Err(HeapError::InvalidInternalSlot(plain))
        );
    }

    #[test]
    fn a_length_stored_on_an_ordinary_object_is_an_ordinary_property() {
        let mut heap = heap();
        let object = heap.alloc_object(None).unwrap();
        heap.set(object, "length", number(3.0)).unwrap();
        assert_eq!(
            format!(
                "{:?}",
                heap.get_own_property_descriptor_key(object, "length".into())
            ),
            format!(
                "{:?}",
                Ok::<_, HeapError>(Some(PropertyDescriptor::data(
                    number(3.0),
                    true,
                    true,
                    true
                )))
            )
        );
        let array = heap.alloc_array(2, None).unwrap();
        assert_eq!(
            format!(
                "{:?}",
                heap.get_own_property_descriptor_key(array, "length".into())
            ),
            format!(
                "{:?}",
                Ok::<_, HeapError>(Some(PropertyDescriptor::data(
                    number(2.0),
                    true,
                    false,
                    false
                )))
            )
        );
        let string = heap.alloc_string("abc".into(), None).unwrap();
        assert_eq!(
            format!(
                "{:?}",
                heap.get_own_property_descriptor_key(string, "length".into())
            ),
            format!(
                "{:?}",
                Ok::<_, HeapError>(Some(PropertyDescriptor::data(
                    number(3.0),
                    false,
                    false,
                    false
                )))
            )
        );
    }

    #[test]
    fn a_lookup_walks_the_prototype_chain() {
        let mut heap = heap();
        let base = heap.alloc_object(None).unwrap();
        heap.root(base).unwrap();
        heap.set(base, "inherited", number(1.0)).unwrap();
        let middle = heap.alloc_object(Some(base)).unwrap();
        heap.root(middle).unwrap();
        let object = heap.alloc_object(Some(middle)).unwrap();
        heap.root(object).unwrap();
        heap.set(object, "own", number(2.0)).unwrap();
        assert_eq!(heap.get_key(object, "own".into()), Ok(number(2.0)));
        assert_eq!(heap.get_key(object, "inherited".into()), Ok(number(1.0)));
        assert_eq!(heap.get_key(object, "absent".into()), Ok(Value::Undefined));
        assert_eq!(heap.get_own_key(object, "inherited".into()), Ok(None));
    }

    #[test]
    fn an_array_stores_named_and_indexed_properties_and_grows_by_index() {
        let mut heap = heap();
        let array = heap.alloc_array(0, None).unwrap();
        heap.root(array).unwrap();
        heap.set_key(array, "named".into(), number(1.0)).unwrap();
        heap.set_key(array, "3".into(), number(2.0)).unwrap();
        heap.set_key(array, "1".into(), number(3.0)).unwrap();
        heap.set_key(array, "named".into(), number(4.0)).unwrap();
        assert_eq!(heap.get_key(array, "length".into()), Ok(number(4.0)));
        assert_eq!(heap.get_key(array, "named".into()), Ok(number(4.0)));
        assert_eq!(
            heap.own_property_keys(array),
            Ok(vec![
                "1".into(),
                "3".into(),
                "length".into(),
                "named".into()
            ])
        );
        assert_eq!(
            heap.enumerable_own_keys(array),
            Ok(vec!["1".into(), "3".into(), "named".into()])
        );
        assert_eq!(heap.delete_key(array, "named".into()), Ok(true));
        assert_eq!(heap.delete_key(array, "missing".into()), Ok(true));
    }
}
