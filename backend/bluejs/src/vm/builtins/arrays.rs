// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn array_join(
        &mut self,
        receiver: &Value,
        separator: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let separator = if *separator == Value::Undefined {
            ",".into()
        } else {
            self.coerce_string(separator)?
        };
        if self.joining.contains(&object) {
            return Ok(Value::String(JsString::default()));
        }
        self.joining.push(object);
        let result = (|| {
            let mut result = JsString::default();
            for index in 0..length {
                self.charge_step()?;
                if index > 0 {
                    native::append(&mut result, &separator, self.config.max_string_bytes)?;
                }
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                if !matches!(value, Value::Null | Value::Undefined) {
                    let value = self.coerce_string(&value)?;
                    native::append(&mut result, &value, self.config.max_string_bytes)?;
                }
            }
            Ok(Value::String(result))
        })();
        self.joining.pop();
        result
    }

    pub(in super::super) fn array_concat(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let mut values = Vec::new();
        for value in std::iter::once(receiver).chain(args) {
            if let Some(object) = value
                .object_id()
                .filter(|id| self.heap.is_array(*id).unwrap_or(false))
            {
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)? as u64;
                for index in 0..length {
                    self.charge_step()?;
                    values.push(
                        self.get_property(&Value::Object(object), &index.to_string().into())?,
                    );
                }
            } else {
                values.push(value.clone());
            }
        }
        self.array_from(values)
    }

    pub(in super::super) fn array_for_each(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Array.prototype.forEach callback must be callable".into(),
            ));
        }
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        if let Some(indices) = self.array_own_indices(object, length)? {
            for index in indices {
                self.charge_step()?;
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                self.call_native(
                    callback.clone(),
                    this_arg.clone(),
                    vec![value, Value::Number(index as f64), Value::Object(object)],
                    false,
                )?;
            }
        } else {
            for index in 0..length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)? {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    self.call_native(
                        callback.clone(),
                        this_arg.clone(),
                        vec![value, Value::Number(index as f64), Value::Object(object)],
                        false,
                    )?;
                }
            }
        }
        self.stack.pop();
        Ok(Value::Undefined)
    }

    pub(in super::super) fn array_reduce(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let callback = native::argument(args, 0);
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Array.prototype.reduce callback must be callable".into(),
            ));
        }
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            let mut index = 0;
            let mut accumulator = if args.len() > 1 {
                args[1].clone()
            } else {
                loop {
                    if index >= length {
                        return Err(RuntimeError::TypeError(
                            "reduce of empty array with no initial value".into(),
                        ));
                    }
                    let key: PropertyName = index.to_string().into();
                    if self.has_property(object, &key)? {
                        let value = self.get_property(&Value::Object(object), &key)?;
                        index += 1;
                        break value;
                    }
                    index += 1;
                }
            };
            while index < length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)? {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    accumulator = self.call_native(
                        callback.clone(),
                        Value::Undefined,
                        vec![
                            accumulator,
                            value,
                            Value::Number(index as f64),
                            Value::Object(object),
                        ],
                        false,
                    )?;
                }
                index += 1;
            }
            Ok(accumulator)
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn array_own_indices(
        &mut self,
        object: ObjectId,
        length: u64,
    ) -> Result<Option<Vec<u32>>, RuntimeError> {
        // Scanning ordinary arrays preserves properties added by callbacks.  This
        // shortcut is only for the large sparse arrays that would otherwise turn
        // a bounded operation into millions of empty property lookups.
        if length < 65_536 || !self.heap.is_array(object)? || self.heap.proxy(object)?.is_some() {
            return Ok(None);
        }
        let mut prototype = self.heap.prototype(object)?;
        while let Some(id) = prototype {
            // The optimized path does not invoke [[HasProperty]]. A Proxy
            // prototype can observe that operation, so retain the normal path.
            if self.heap.proxy(id)?.is_some() {
                return Ok(None);
            }
            if self
                .heap
                .own_property_keys(id)?
                .iter()
                .any(|key| array_index_below_length(key, length).is_some())
            {
                return Ok(None);
            }
            prototype = self.heap.prototype(id)?;
        }
        let mut indices = Vec::new();
        for key in self.heap.own_property_keys(object)? {
            let Some(index) = array_index_below_length(&key, length) else {
                continue;
            };
            // Accessors can add or remove later indexed properties while the
            // method scans. Keep the ordinary path for that observable case.
            if self
                .heap
                .get_own_property_descriptor(object, &key)?
                .is_some_and(|descriptor| descriptor.accessor())
            {
                return Ok(None);
            }
            indices.push(index);
        }
        indices.sort_unstable();
        Ok(Some(indices))
    }

    pub(in super::super) fn array_start_index(
        &mut self,
        from_index: &Value,
        length: i64,
    ) -> Result<i64, RuntimeError> {
        let from_index = if *from_index == Value::Undefined {
            0
        } else {
            self.coerce_number(from_index)? as i64
        };
        Ok(if from_index < 0 {
            (length + from_index).max(0)
        } else {
            from_index.min(length)
        })
    }

    pub(in super::super) fn array_includes(
        &mut self,
        receiver: &Value,
        search: &Value,
        from_index: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as i64;
            let start = self.array_start_index(from_index, length)?;
            if let Some(indices) = self.array_own_indices(object, length as u64)? {
                let mut index = start as u64;
                for present in indices
                    .into_iter()
                    .filter(|present| i64::from(*present) >= start)
                {
                    // With no inherited indexed properties, the first omitted
                    // element is an observable `undefined` for includes.
                    if *search == Value::Undefined && index < u64::from(present) {
                        return Ok(Value::Bool(true));
                    }
                    self.charge_step()?;
                    let value = self.get_property(
                        &Value::Object(object),
                        &u64::from(present).to_string().into(),
                    )?;
                    if same_value_zero(&value, search) {
                        return Ok(Value::Bool(true));
                    }
                    index = u64::from(present) + 1;
                }
                return Ok(Value::Bool(
                    *search == Value::Undefined && index < length as u64,
                ));
            }
            for index in start..length {
                self.charge_step()?;
                let value =
                    self.get_property(&Value::Object(object), &(index as u64).to_string().into())?;
                if same_value_zero(&value, search) {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn array_index_of(
        &mut self,
        receiver: &Value,
        search: &Value,
        from_index: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as i64;
            let start = self.array_start_index(from_index, length)?;
            if let Some(indices) = self.array_own_indices(object, length as u64)? {
                for present in indices {
                    if i64::from(present) < start {
                        continue;
                    }
                    self.charge_step()?;
                    let key: PropertyName = present.to_string().into();
                    if self.get_property(&Value::Object(object), &key)? == *search {
                        return Ok(Value::Number(present as f64));
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            let mut index = start;
            while index < length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)?
                    && self.get_property(&Value::Object(object), &key)? == *search
                {
                    return Ok(Value::Number(index as f64));
                }
                index += 1;
            }
            Ok(Value::Number(-1.0))
        })();
        self.stack.pop();
        result
    }

    fn array_species_create(
        &mut self,
        original: ObjectId,
        length: usize,
    ) -> Result<ObjectId, RuntimeError> {
        let ordinary_array = |vm: &mut Self| {
            let length = u32::try_from(length)
                .map_err(|_| RuntimeError::RangeError("invalid Array length".into()))?;
            let prototype = vm.array_prototype;
            vm.with_roots(|heap| heap.alloc_array(length, Some(prototype)))
        };
        if !self.heap.is_array(original)? {
            return ordinary_array(self);
        }
        let original = Value::Object(original);
        let constructor = self.get_property(&original, &"constructor".into())?;
        if constructor == Value::Undefined {
            return ordinary_array(self);
        }
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Array constructor must be an object".into(),
            ));
        }
        let species = self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
        if matches!(species, Value::Undefined | Value::Null) {
            return ordinary_array(self);
        }
        if !self.is_constructor(&species)? {
            return Err(RuntimeError::TypeError(
                "Array species must be a constructor".into(),
            ));
        }
        self.call_with_target(
            species.clone(),
            Value::Undefined,
            vec![Value::Number(length as f64)],
            true,
            species,
        )?
        .object_id()
        .ok_or_else(|| RuntimeError::TypeError("Array species must return an object".into()))
    }

    fn array_set_or_throw(
        &mut self,
        object: ObjectId,
        key: PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        if self.ordinary_set_with_receiver(object, &Value::Object(object), &key, value)? {
            Ok(())
        } else {
            Err(RuntimeError::TypeError(
                "cannot assign Array property".into(),
            ))
        }
    }

    fn array_create_data_property_or_throw(
        &mut self,
        object: ObjectId,
        key: PropertyName,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if self.object_define_own_property(
            object,
            key,
            PropertyDescriptor::data(value, true, true, true),
        )? {
            Ok(())
        } else {
            Err(RuntimeError::TypeError(
                "cannot create Array property".into(),
            ))
        }
    }

    fn array_delete_or_throw(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<(), RuntimeError> {
        if self.object_delete(object, key)? {
            Ok(())
        } else {
            Err(RuntimeError::TypeError(
                "cannot delete Array property".into(),
            ))
        }
    }

    /// Array.prototype.slice, including ArraySpeciesCreate and sparse source
    /// property preservation. TypedArray has its own integer-indexed slice.
    pub(in super::super) fn array_slice(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as i64;
            let start = self.array_start_index(native::argument(args, 0), length)?;
            let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
                self.array_start_index(native::argument(args, 1), length)?
            } else {
                length
            };
            let count = end.saturating_sub(start) as usize;
            let target = self.array_species_create(object, count)?;
            self.stack.push(Value::Object(target));
            for (result_index, index) in (start..end.max(start)).enumerate() {
                self.charge_step()?;
                let key: PropertyName = (index as u64).to_string().into();
                if self.has_property(object, &key)? {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    self.array_create_data_property_or_throw(
                        target,
                        result_index.to_string().into(),
                        value,
                    )?;
                }
            }
            self.array_set_or_throw(target, "length".into(), &Value::Number(count as f64))?;
            Ok(Value::Object(target))
        })();
        self.stack.pop();
        result
    }

    /// Array.prototype.splice, with species result creation and sparse source
    /// moves expressed through the object internal-method boundary.
    pub(in super::super) fn array_splice(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as i64;
            let start = self.array_start_index(native::argument(args, 0), length)?;
            let delete_count = if args.len() < 2 {
                length - start
            } else {
                self.coerce_length(native::argument(args, 1))? as i64
            }
            .clamp(0, length - start);
            let items = args.get(2..).unwrap_or_default();
            let new_length = length
                .checked_add(items.len() as i64)
                .and_then(|value| value.checked_sub(delete_count))
                .filter(|value| *value <= 9_007_199_254_740_991)
                .ok_or_else(|| RuntimeError::TypeError("invalid Array length".into()))?;
            let target = self.array_species_create(object, delete_count as usize)?;
            self.stack.push(Value::Object(target));
            for index in start..start + delete_count {
                self.charge_step()?;
                let key: PropertyName = (index as u64).to_string().into();
                if self.has_property(object, &key)? {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    self.array_create_data_property_or_throw(
                        target,
                        (index - start).to_string().into(),
                        value,
                    )?;
                }
            }
            self.array_set_or_throw(target, "length".into(), &Value::Number(delete_count as f64))?;
            let delta = items.len() as i64 - delete_count;
            if delta < 0 {
                for index in start..length - delete_count {
                    let from: PropertyName = ((index + delete_count) as u64).to_string().into();
                    let to: PropertyName = ((index + items.len() as i64) as u64).to_string().into();
                    if self.has_property(object, &from)? {
                        let value = self.get_property(&Value::Object(object), &from)?;
                        self.array_set_or_throw(object, to, &value)?;
                    } else {
                        self.array_delete_or_throw(object, &to)?;
                    }
                }
                for index in (length + delta)..length {
                    self.array_delete_or_throw(object, &(index as u64).to_string().into())?;
                }
            } else if delta > 0 {
                for index in (start..length - delete_count).rev() {
                    let from: PropertyName = ((index + delete_count) as u64).to_string().into();
                    let to: PropertyName = ((index + items.len() as i64) as u64).to_string().into();
                    if self.has_property(object, &from)? {
                        let value = self.get_property(&Value::Object(object), &from)?;
                        self.array_set_or_throw(object, to, &value)?;
                    } else {
                        self.array_delete_or_throw(object, &to)?;
                    }
                }
            }
            for (offset, value) in items.iter().enumerate() {
                self.array_set_or_throw(object, (start + offset as i64).to_string().into(), value)?;
            }
            self.array_set_or_throw(object, "length".into(), &Value::Number(new_length as f64))?;
            Ok(Value::Object(target))
        })();
        self.stack.pop();
        result
    }
}
