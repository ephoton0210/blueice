// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property descriptors, ordinary object storage, and array length updates.

use super::*;

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
                        return Ok(Some(PropertyDescriptor::data(value, true, true, true)));
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
        let obj = self.object(object)?;
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
        let length = key == "length"
            && matches!(&obj.kind, ObjectKind::Array { .. } | ObjectKind::String(_));
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
        let mapped_cell = self.arguments_parameter_cell(object, &key)?;
        let old = self.get_own_property_descriptor(object, &key)?;
        if old.is_none() && !self.object(object)?.extensible {
            return Ok(false);
        }
        let obj = self.object(object)?;
        if let ObjectKind::Array { length } = obj.kind {
            if array_index(&key).is_some_and(|index| index >= length)
                && obj
                    .attributes
                    .get(&"length".into())
                    .is_some_and(|d| d.writable == Some(false))
            {
                return Ok(false);
            }
        }
        if let Some(old) = &old {
            if old.configurable == Some(false) {
                if descriptor.configurable == Some(true)
                    || descriptor
                        .enumerable
                        .is_some_and(|v| Some(v) != old.enumerable)
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
                    if descriptor
                        .get
                        .as_ref()
                        .is_some_and(|v| !same_value(v, old.get.as_ref().unwrap()))
                        || descriptor
                            .set
                            .as_ref()
                            .is_some_and(|v| !same_value(v, old.set.as_ref().unwrap()))
                    {
                        return Ok(false);
                    }
                } else if old.writable == Some(false)
                    && (descriptor.writable == Some(true)
                        || descriptor
                            .value
                            .as_ref()
                            .is_some_and(|v| !same_value(v, old.value.as_ref().unwrap())))
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
        let obj = self.object(object)?;
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
        if !length_failed {
            if let Some(cell) = mapped_cell {
                if descriptor_is_accessor || descriptor_non_writable {
                    self.unmap_arguments_property(object, &key)?;
                } else if let Some(value) = descriptor_value {
                    self.set(cell, "value", value)?;
                }
            }
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

    pub(super) fn is_module_namespace(&self, object: ObjectId) -> Result<bool, HeapError> {
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
                ObjectKind::RegExp(regexp) => {
                    regexp.source.byte_len()
                        + regexp.flags.len()
                        + regexp
                            .capture_names
                            .iter()
                            .map(|(name, _)| name.len() + size_of::<(String, usize)>())
                            .sum::<usize>()
                }
                ObjectKind::Collator { data, .. } => data.bytes(),
                ObjectKind::IntlLocale(data) => data.bytes(),
                ObjectKind::BoxedPrimitive(value) => value.payload_bytes(),
                ObjectKind::NativeFunction { initial_name, .. } => initial_name.byte_len(),
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
                ObjectKind::ModuleNamespace { exports } => exports
                    .iter()
                    .map(|(name, _)| name.byte_len() + size_of::<(JsString, ObjectId)>())
                    .sum(),
                _ => 0,
            };
        if self
            .managed_bytes
            .checked_add(bytes)
            .is_none_or(|total| total >= self.next_major_bytes)
        {
            let protected =
                protected.get_or_insert_with(|| allocation_references(&kind, prototype));
            self.ensure_room(bytes, protected)?;
        } else {
            self.ensure_room(bytes, &[])?;
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
        let key = key.into();
        if let Some(numeric) = self.typed_array_numeric_key(object, &key)? {
            return match numeric {
                TypedArrayNumericKey::Index(index) => self.typed_array_index_value(object, index),
                TypedArrayNumericKey::Invalid => Ok(None),
            };
        }
        if let Some(index) = key.index() {
            if let Some(value) = self.typed_array_index_value(object, index)? {
                return Ok(Some(value));
            }
        }
        if let Some(cell) = self.module_namespace_export_cell(object, &key)? {
            return self
                .get_own(cell, "value")?
                .map(Some)
                .ok_or(HeapError::UninitializedModuleExport);
        }
        Ok(self.object(object)?.own_property(&key))
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
            if let Some(index) = key.index() {
                if let Some(value) = self.typed_array_index_value(id, index)? {
                    return Ok(value);
                }
            }
            if let Some(cell) = self.module_namespace_export_cell(id, &key)? {
                return self
                    .get_own(cell, "value")?
                    .ok_or(HeapError::UninitializedModuleExport);
            }
            if let Some(cell) = self.arguments_parameter_cell(id, &key)? {
                return Ok(self.get_own(cell, "value")?.unwrap_or(Value::Undefined));
            }
            let obj = self.object(id)?;
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
        if self.module_namespace_export_cell(object, &key)?.is_some() {
            return Err(HeapError::ReadOnlyProperty);
        }
        let mapped_cell = self.arguments_parameter_cell(object, &key)?;
        let obj = self.object(object)?;
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
            if array_index(&key).is_some_and(|index| index >= length)
                && obj
                    .attributes
                    .get(&"length".into())
                    .is_some_and(|d| d.writable == Some(false))
            {
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
        let mapped_value = mapped_cell.map(|_| value.clone());
        {
            let obj = self
                .objects
                .get_mut(&object)
                .expect("the receiver is protected across collection");
            if !obj.properties.contains_key(&key) {
                obj.order.push(key.clone());
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
            return Ok(self.buffer_is_detached(buffer)? || index >= length);
        }
        if self.module_namespace_export_cell(object, &key)?.is_some() {
            return Ok(false);
        }
        if self.is_module_namespace(object)? && matches!(key, PropertyName::String(_)) {
            return Ok(true);
        }
        let mapped_cell = self.arguments_parameter_cell(object, &key)?;
        let obj = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
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
            let bytes = property_bytes(&key, &value)
                + obj
                    .attributes
                    .remove(&key)
                    .map_or(0, |d| attribute_bytes(&key, &d));
            obj.order.retain(|name| name != &key);
            obj.bytes -= bytes;
            self.managed_bytes -= bytes;
        }
        if mapped_cell.is_some() {
            self.unmap_arguments_property(object, &key)?;
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
            let mut keys: Vec<_> = exports
                .iter()
                .map(|(export, _)| PropertyName::String(export.clone()))
                .collect();
            keys.extend(
                obj.order
                    .iter()
                    .filter(|key| matches!(key, PropertyName::Symbol(_)))
                    .cloned(),
            );
            return Ok(keys);
        }
        let mut indices = Vec::new();
        let mut strings = Vec::new();
        let mut symbols = Vec::new();
        if let ObjectKind::String(string) = &obj.kind {
            for index in 0..string.len() {
                indices.push((index, index.to_string().into()));
            }
        }
        if let ObjectKind::TypedArray { buffer, .. } = &obj.kind {
            if !self.buffer_is_detached(*buffer)? {
                let (_, _, length, _) = self.typed_array_info(object)?;
                for index in 0..length {
                    indices.push((index, index.to_string().into()));
                }
            }
        }
        if matches!(obj.kind, ObjectKind::Array { .. } | ObjectKind::String(_)) {
            strings.push("length".into());
        }
        for key in &obj.order {
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
        Ok(keys)
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
        Ok(self
            .own_keys(object)?
            .into_iter()
            .filter(|key| {
                (!virtual_length || key != "length")
                    && self.objects[&object]
                        .attributes
                        .get(&PropertyName::from(key))
                        .is_none_or(|d| d.enumerable == Some(true))
            })
            .collect())
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
            .expect("validated array receiver");
        let ObjectKind::Array { length } = &mut obj.kind else {
            unreachable!("length dispatch checks object kind")
        };
        let old_length = *length;
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
                if !self.delete(object, &key)? {
                    if let ObjectKind::Array { length } =
                        &mut self.objects.get_mut(&object).unwrap().kind
                    {
                        *length = index + 1;
                    }
                    return Err(HeapError::ReadOnlyProperty);
                }
            }
        }
        if let ObjectKind::Array { length } = &mut self.objects.get_mut(&object).unwrap().kind {
            *length = new_length;
        }
        Ok(())
    }
}
