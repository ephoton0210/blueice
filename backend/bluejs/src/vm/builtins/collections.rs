// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// Object.groupBy consumes its source as an iterator, rather than using
    /// array-like indexing. The callback result is converted to a property
    /// key before a group is created, and every callback/key/append abrupt
    /// completion closes the still-live iterator while preserving that
    /// original completion.
    pub(in super::super) fn object_group_by_method(
        &mut self,
        items: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Object.groupBy callback must be callable".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.extend([items.clone(), callback.clone()]);
        let result = (|| {
            let record = self.get_iterator(items)?;
            self.stack.push(record.clone());
            let groups = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(groups));
            let outcome = (|| {
                let mut index = 0u64;
                while let Some(value) = self.iterator_step(&record, true)? {
                    if index >= 9_007_199_254_740_991 {
                        return Err(RuntimeError::TypeError(
                            "Object.groupBy iterator is too large".into(),
                        ));
                    }
                    let item_base = self.stack.len();
                    self.stack.push(value.clone());
                    let key_value = self.call_native(
                        callback.clone(),
                        Value::Undefined,
                        vec![value, Value::Number(index as f64)],
                        false,
                    )?;
                    self.stack.push(key_value.clone());
                    let key = self.coerce_property_key(&key_value)?;
                    let group =
                        if let Some(descriptor) = self.object_get_own_property(groups, &key)? {
                            descriptor.value.ok_or_else(|| {
                                RuntimeError::TypeError("Object.groupBy group is not data".into())
                            })?
                        } else {
                            let group = self.array_from(Vec::new())?;
                            self.stack.push(group.clone());
                            let defined = self.object_define_own_property(
                                groups,
                                key,
                                PropertyDescriptor::data(group.clone(), true, true, true),
                            )?;
                            self.stack.pop();
                            if !defined {
                                return Err(RuntimeError::TypeError(
                                    "cannot create Object.groupBy group".into(),
                                ));
                            }
                            group
                        };
                    self.stack.push(group.clone());
                    let value = self.stack[item_base].clone();
                    self.array_push(&group, &value, 0)?;
                    self.stack.truncate(item_base);
                    index += 1;
                }
                Ok(Value::Object(groups))
            })();
            if outcome.is_err() {
                let error_base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &outcome {
                    self.stack.push(value.clone());
                }
                // An IteratorStep abrupt completion has already marked the
                // record done; IteratorClose consequently becomes a no-op in
                // that case and preserves its original error.
                let _ = self.iterator_close(&record);
                self.stack.truncate(error_base);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    /// `Map.groupBy` (GroupBy with zero key coercion): like `Object.groupBy`
    /// it consumes an iterator and closes it on any abrupt completion, but
    /// keys keep their identity (only `-0` becomes `+0`) and the groups land
    /// in a fresh `%Map%` in first-seen order. Grouping straight into that
    /// Map is equivalent to the spec's separate group list: the Map is not
    /// observable until it is returned, and its SameValueZero lookup is the
    /// spec's SameValue on the zero-normalized keys.
    pub(in super::super) fn map_group_by_method(
        &mut self,
        items: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        if matches!(items, Value::Undefined | Value::Null) {
            return Err(RuntimeError::TypeError(
                "Map.groupBy items must not be null or undefined".into(),
            ));
        }
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Map.groupBy callback must be callable".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.extend([items.clone(), callback.clone()]);
        let result = (|| {
            let record = self.get_iterator(items)?;
            self.stack.push(record.clone());
            let prototype = self.collection_prototype(true)?;
            let groups = self.with_roots(|heap| heap.alloc_map(Some(prototype)))?;
            self.stack.push(Value::Object(groups));
            let outcome = (|| {
                let mut index = 0u64;
                while let Some(value) = self.iterator_step(&record, true)? {
                    if index >= 9_007_199_254_740_991 {
                        return Err(RuntimeError::TypeError(
                            "Map.groupBy iterator is too large".into(),
                        ));
                    }
                    let item_base = self.stack.len();
                    self.stack.push(value.clone());
                    let key = self.call_native(
                        callback.clone(),
                        Value::Undefined,
                        vec![value.clone(), Value::Number(index as f64)],
                        false,
                    )?;
                    self.stack.push(key.clone());
                    let group = match self.heap.map_get(groups, &key)? {
                        Some(group) => group,
                        None => {
                            let group = self.array_from(Vec::new())?;
                            self.stack.push(group.clone());
                            self.with_roots(|heap| heap.map_set(groups, key, group.clone()))?;
                            group
                        }
                    };
                    self.stack.push(group.clone());
                    self.array_push(&group, &value, 0)?;
                    self.stack.truncate(item_base);
                    index += 1;
                }
                Ok(Value::Object(groups))
            })();
            if outcome.is_err() {
                let error_base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &outcome {
                    self.stack.push(value.clone());
                }
                let _ = self.iterator_close(&record);
                self.stack.truncate(error_base);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn object_from_entries_method(
        &mut self,
        source: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(source.clone());
        let result = (|| {
            let record = self.get_iterator(source)?;
            self.stack.push(record.clone());
            let prototype = self.object_prototype;
            let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(object));
            let outcome = (|| {
                while let Some(entry) = self.iterator_step(&record, true)? {
                    let Value::Object(entry) = entry else {
                        return Err(RuntimeError::TypeError(
                            "Object.fromEntries entry must be an object".into(),
                        ));
                    };
                    let entry_base = self.stack.len();
                    self.stack.push(Value::Object(entry));
                    let key_value = self.get_property(&Value::Object(entry), &"0".into())?;
                    self.stack.push(key_value.clone());
                    let value = self.get_property(&Value::Object(entry), &"1".into())?;
                    self.stack.push(value.clone());
                    let key = self.coerce_property_key(&key_value)?;
                    let defined = self.object_define_own_property(
                        object,
                        key,
                        PropertyDescriptor::data(value, true, true, true),
                    )?;
                    self.stack.truncate(entry_base);
                    if !defined {
                        return Err(RuntimeError::TypeError(
                            "cannot define Object.fromEntries property".into(),
                        ));
                    }
                }
                Ok(Value::Object(object))
            })();
            if outcome.is_err() {
                let error_base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &outcome {
                    self.stack.push(value.clone());
                }
                // IteratorClose is required for the side effect, but an
                // existing abrupt completion wins over a close failure.
                let _ = self.iterator_close(&record);
                self.stack.truncate(error_base);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    /// `Array.from(items, mapfn, thisArg)` with `this` as the constructor `C`
    /// (`Array.from` step 1). The iterator method is read before `C` is
    /// constructed, elements are defined with `CreateDataPropertyOrThrow`, and
    /// `length` is set at the end. A non-constructor `this` builds a plain
    /// Array, exactly as `Array.of` does.
    pub(in super::super) fn array_from_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
        let source = native::argument(args, 0).clone();
        if matches!(source, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Array.from requires an object".into(),
            ));
        }
        let mapper = native::argument(args, 1).clone();
        if mapper != Value::Undefined && !self.is_callable(&mapper)? {
            return Err(RuntimeError::TypeError(
                "Array.from mapper must be callable".into(),
            ));
        }
        let this_arg = native::argument(args, 2).clone();
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(source.clone());
        if mapper != Value::Undefined {
            self.stack.push(mapper.clone());
            self.stack.push(this_arg.clone());
        }
        let result = (|| {
            let iterator = self.get_method(&source, &JsSymbol::well_known("iterator").into())?;
            // A getter may have produced the method just now, so nothing but
            // this local holds it while `C` is constructed below.
            self.stack.push(iterator.clone());
            let constructor = self.is_constructor(receiver)?;
            if iterator == Value::Undefined {
                // Each element is read, mapped and stored before the next
                // one is read (spec order), so a mapper result is reachable
                // from the rooted result array before any later mapper call
                // can allocate and collect.
                let object = self.coerce_object(&source)?;
                let object_value = Value::Object(object);
                self.stack.push(object_value.clone());
                let length = self.get_property(&object_value, &"length".into())?;
                let length = self.coerce_length(&length)?;
                let target = self.array_from_target(receiver, constructor, Some(length))?;
                self.stack.push(Value::Object(target));
                let mark = self.stack.len();
                for index in 0..length as u64 {
                    self.charge_step()?;
                    let value = self.get_property(&object_value, &index.to_string().into())?;
                    let value = if mapper == Value::Undefined {
                        value
                    } else {
                        self.stack.push(value.clone());
                        self.call_native(
                            mapper.clone(),
                            this_arg.clone(),
                            vec![value, Value::Number(index as f64)],
                            false,
                        )?
                    };
                    self.stack.push(value.clone());
                    self.array_create_data_property_or_throw(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    self.stack.truncate(mark);
                }
                self.array_set_or_throw(target, "length".into(), &Value::Number(length))?;
                return Ok(Value::Object(target));
            }

            // Array.from maps one iterator value at a time. Collecting the
            // iterator first makes an infinite source consume its resource
            // budget before an abrupt mapper can close it, which is both
            // observably wrong and turns finite conformance checks into
            // timeouts.
            let target = self.array_from_target(receiver, constructor, None)?;
            self.stack.push(Value::Object(target));
            let record = self.get_iterator_from_method(&source, iterator)?;
            self.stack.push(record.clone());
            let outcome = (|| {
                let mut index = 0u64;
                loop {
                    if index >= MAX_SAFE_INTEGER {
                        return Err(RuntimeError::TypeError(
                            "Array.from result length is too large".into(),
                        ));
                    }
                    let Some(value) = self.iterator_step(&record, true)? else {
                        break;
                    };
                    let mark = self.stack.len();
                    let value = if mapper == Value::Undefined {
                        value
                    } else {
                        self.stack.push(value.clone());
                        self.call_native(
                            mapper.clone(),
                            this_arg.clone(),
                            vec![value, Value::Number(index as f64)],
                            false,
                        )?
                    };
                    self.stack.push(value.clone());
                    self.array_create_data_property_or_throw(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    self.stack.truncate(mark);
                    index += 1;
                }
                self.array_set_or_throw(target, "length".into(), &Value::Number(index as f64))?;
                Ok(Value::Object(target))
            })();
            if outcome.is_err() {
                // IteratorClose retains an existing abrupt completion. The
                // original mapper/iterator error must win over a return()
                // failure, so close only for its required side effect here.
                // The thrown value is only reachable from `outcome`, and
                // return() runs user code that can allocate and collect.
                // A finished or failed iterator is already marked done, which
                // makes the close a no-op (spec: no close after IteratorStep
                // or the final length Set fails).
                let error_base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &outcome {
                    self.stack.push(value.clone());
                }
                let _ = self.iterator_close(&record);
                self.stack.truncate(error_base);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    /// The result object of `Array.from`: `Construct(C)` (or `Construct(C,
    /// «len»)` for an array-like) when `this` is a constructor, otherwise a
    /// plain Array of the current Realm.
    pub(in super::super) fn array_from_target(
        &mut self,
        receiver: &Value,
        constructor: bool,
        length: Option<f64>,
    ) -> Result<ObjectId, RuntimeError> {
        if !constructor {
            return self.array_create_exact(length.unwrap_or(0.0));
        }
        let args = length.map(Value::Number).into_iter().collect();
        self.call_with_target(
            receiver.clone(),
            Value::Undefined,
            args,
            true,
            receiver.clone(),
        )?
        .object_id()
        .ok_or_else(|| {
            RuntimeError::TypeError("Array.from constructor returned a primitive".into())
        })
    }

    pub(in super::super) fn array_of_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let array = if self.is_constructor(receiver)? {
                self.call_with_target(
                    receiver.clone(),
                    Value::Undefined,
                    vec![Value::Number(args.len() as f64)],
                    true,
                    receiver.clone(),
                )?
                .object_id()
                .ok_or_else(|| {
                    RuntimeError::TypeError("Array.of constructor returned a primitive".into())
                })?
            } else {
                let prototype = self.array_prototype;
                self.with_roots(|heap| heap.alloc_array(0, Some(prototype)))?
            };
            self.stack.push(Value::Object(array));
            for (index, value) in args.iter().cloned().enumerate() {
                self.array_create_data_property_or_throw(array, index.to_string().into(), value)?;
            }
            self.array_set_or_throw(array, "length".into(), &Value::Number(args.len() as f64))?;
            Ok(Value::Object(array))
        })();
        self.stack.truncate(base);
        result
    }
}
