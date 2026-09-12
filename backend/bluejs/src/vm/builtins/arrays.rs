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
}
