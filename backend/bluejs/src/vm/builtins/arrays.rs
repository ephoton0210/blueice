// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn array_at(
        &mut self,
        receiver: &Value,
        index: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)?;
            let index = self.coerce_number(index)?;
            // ToIntegerOrInfinity, followed by the relative-index check.
            // Avoid converting an out-of-range finite double to usize before
            // knowing it is within the bounded array-like length.
            let index = if index.is_nan() || index == 0.0 {
                0.0
            } else {
                index.trunc()
            };
            let index = if index >= 0.0 { index } else { length + index };
            if !index.is_finite() || index < 0.0 || index >= length {
                Ok(Value::Undefined)
            } else {
                self.get_property(&Value::Object(object), &(index as u64).to_string().into())
            }
        })();
        self.stack.pop();
        result
    }

    /// `Array.prototype.fill` performs `Set` for every index in the selected
    /// range. Keeping this at the ordinary property boundary makes it generic
    /// for array-like objects and preserves proxy and inherited-setter
    /// behavior, rather than treating the receiver as dense Array storage.
    pub(in super::super) fn array_fill(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let object_value = Value::Object(object);
        let base = self.stack.len();
        self.stack.push(object_value.clone());
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let length = self.get_property(&object_value, &"length".into())?;
            let length = self.coerce_length(&length)?;
            let start = self.array_fill_index(native::argument(args, 1), length)?;
            let end = if args.get(2).is_some_and(|value| *value != Value::Undefined) {
                self.array_fill_index(native::argument(args, 2), length)?
            } else {
                length as u64
            };
            let value = native::argument(args, 0).clone();
            self.stack.push(value.clone());
            for index in start..end.max(start) {
                self.charge_step()?;
                self.array_set_or_throw(object, index.to_string().into(), &value)?;
            }
            self.stack.pop();
            Ok(object_value)
        })();
        self.stack.truncate(base);
        result
    }

    /// `ToIntegerOrInfinity` followed by Array's relative-index conversion.
    /// `LengthOfArrayLike` is at most `2^53 - 1`, so every finite clamped
    /// result is representable as an unsigned property index here.
    fn array_fill_index(&mut self, value: &Value, length: f64) -> Result<u64, RuntimeError> {
        let number = self.coerce_number(value)?;
        let integer = if number.is_nan() || number == 0.0 {
            0.0
        } else {
            number.trunc()
        };
        let index = if integer == f64::NEG_INFINITY {
            0.0
        } else if integer < 0.0 {
            (length + integer).max(0.0)
        } else {
            integer.min(length)
        };
        Ok(index as u64)
    }

    /// `Array.prototype.push` for a receiver whose `length` is not a plain
    /// Number (an array-like, a proxy, a TypedArray): ToLength(Get(O,
    /// "length")), one strict Set per argument, then a strict Set of the new
    /// `length`. Genuine arrays never get here.
    pub(in super::super) fn array_push_generic(
        &mut self,
        object: ObjectId,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
        let object_value = Value::Object(object);
        let base = self.stack.len();
        self.stack.push(object_value.clone());
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let length = self.get_property(&object_value, &"length".into())?;
            let mut length = self.coerce_length(&length)?;
            if length + args.len() as f64 > MAX_SAFE_INTEGER {
                return Err(RuntimeError::TypeError(
                    "Array.prototype.push would exceed the maximum array-like length".into(),
                ));
            }
            for value in args {
                self.charge_step()?;
                let key: PropertyName = (length as u64).to_string().into();
                self.array_set_or_throw(object, key, value)?;
                length += 1.0;
            }
            let length = Value::Number(length);
            self.array_set_or_throw(object, "length".into(), &length)?;
            Ok(length)
        })();
        self.stack.truncate(base);
        result
    }

    /// `Array.prototype.copyWithin`, built from `HasProperty`, `Get`, `Set`
    /// and `DeletePropertyOrThrow` at the ordinary property boundary, so an
    /// array-like, Proxy or TypedArray receiver observes every step (a
    /// resizable-buffer view that shrank simply reads as a shorter length).
    pub(in super::super) fn array_copy_within(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let object_value = Value::Object(object);
        let base = self.stack.len();
        self.stack.push(object_value.clone());
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let length = self.get_property(&object_value, &"length".into())?;
            let length = self.coerce_length(&length)?;
            let to = self.array_fill_index(native::argument(args, 0), length)? as i64;
            let from = self.array_fill_index(native::argument(args, 1), length)? as i64;
            let end = if args.get(2).is_some_and(|value| *value != Value::Undefined) {
                self.array_fill_index(native::argument(args, 2), length)? as i64
            } else {
                length as i64
            };
            let mut count = (end - from).min(length as i64 - to);
            let (mut from, mut to, step) = if from < to && to < from + count {
                (from + count - 1, to + count - 1, -1)
            } else {
                (from, to, 1)
            };
            while count > 0 {
                self.charge_step()?;
                let from_key: PropertyName = from.to_string().into();
                let to_key: PropertyName = to.to_string().into();
                if self.has_property(object, &from_key)? {
                    let value = self.get_property(&object_value, &from_key)?;
                    self.stack.push(value.clone());
                    let stored = self.array_set_or_throw(object, to_key, &value);
                    self.stack.pop();
                    stored?;
                } else {
                    self.array_delete_or_throw(object, &to_key)?;
                }
                from += step;
                to += step;
                count -= 1;
            }
            Ok(object_value.clone())
        })();
        self.stack.truncate(base);
        result
    }

    /// `Array.prototype.flat`: `depth` defaults to 1, and is otherwise
    /// `ToIntegerOrInfinity(depth)` clamped below at 0.
    pub(in super::super) fn array_flat(
        &mut self,
        receiver: &Value,
        depth: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        self.stack.push(depth.clone());
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            let depth = if *depth == Value::Undefined {
                1.0
            } else {
                let number = self.coerce_number(depth)?;
                let integer = if number.is_nan() { 0.0 } else { number.trunc() };
                integer.max(0.0)
            };
            let target = self.array_species_create(object, 0)?;
            self.stack.push(Value::Object(target));
            self.array_flatten_into(target, object, length, depth, None)?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// `Array.prototype.flatMap`: the mapper's result is flattened one level.
    pub(in super::super) fn array_flat_map(
        &mut self,
        receiver: &Value,
        mapper: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        self.stack.push(mapper.clone());
        self.stack.push(this_arg.clone());
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            if !self.is_callable(mapper)? {
                return Err(RuntimeError::TypeError(
                    "Array.prototype.flatMap callback must be callable".into(),
                ));
            }
            let target = self.array_species_create(object, 0)?;
            self.stack.push(Value::Object(target));
            self.array_flatten_into(target, object, length, 1.0, Some((mapper, this_arg)))?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// `FlattenIntoArray`, run with an explicit frame list instead of Rust
    /// recursion so that deeply nested (or cyclic) input is bounded by
    /// `MAX_FLATTEN_NESTING` and reports a RangeError. Only the outermost
    /// frame applies `mapper`, exactly as the recursive calls in the
    /// specification omit it. Every frame's source stays on the VM stack.
    fn array_flatten_into(
        &mut self,
        target: ObjectId,
        source: ObjectId,
        source_len: u64,
        depth: f64,
        mapper: Option<(&Value, &Value)>,
    ) -> Result<(), RuntimeError> {
        const MAX_FLATTEN_NESTING: usize = 10_000;
        struct Frame {
            source: ObjectId,
            len: u64,
            index: u64,
            depth: f64,
        }
        let base = self.stack.len();
        self.stack.push(Value::Object(source));
        let mut frames = vec![Frame {
            source,
            len: source_len,
            index: 0,
            depth,
        }];
        let mut target_index: u64 = 0;
        let result = (|| {
            while let Some(frame) = frames.last_mut() {
                if frame.index >= frame.len {
                    frames.pop();
                    self.stack.pop();
                    continue;
                }
                self.charge_step()?;
                let (source, index, depth) = (frame.source, frame.index, frame.depth);
                frame.index += 1;
                let key: PropertyName = index.to_string().into();
                if !self.has_property(source, &key)? {
                    continue;
                }
                let step_base = self.stack.len();
                let mut element = self.get_property(&Value::Object(source), &key)?;
                self.stack.push(element.clone());
                if frames.len() == 1 {
                    if let Some((mapper, this_arg)) = mapper {
                        element = self.call_native(
                            mapper.clone(),
                            this_arg.clone(),
                            vec![element, Value::Number(index as f64), Value::Object(source)],
                            false,
                        )?;
                        self.stack.push(element.clone());
                    }
                }
                if depth > 0.0 && self.is_array(&element)? {
                    let length = self.get_property(&element, &"length".into())?;
                    let length = self.coerce_length(&length)? as u64;
                    let Value::Object(inner) = element else {
                        unreachable!("IsArray is only true for objects");
                    };
                    if frames.len() >= MAX_FLATTEN_NESTING {
                        return Err(RuntimeError::RangeError(
                            "Array flattening is nested too deeply".into(),
                        ));
                    }
                    self.stack.truncate(step_base);
                    self.stack.push(Value::Object(inner));
                    frames.push(Frame {
                        source: inner,
                        len: length,
                        index: 0,
                        depth: depth - 1.0,
                    });
                } else {
                    if target_index >= (1u64 << 53) - 1 {
                        return Err(RuntimeError::TypeError(
                            "Array.prototype.flat result is too long".into(),
                        ));
                    }
                    self.array_create_data_property_or_throw(
                        target,
                        target_index.to_string().into(),
                        element,
                    )?;
                    target_index += 1;
                    self.stack.truncate(step_base);
                }
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

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
        // §23.1.3.2 starts by boxing the receiver, then creates its result
        // through ArraySpeciesCreate. A primitive receiver therefore becomes
        // one non-spread element, while an Array Proxy can still select its
        // species and spread through the normal internal-method boundary.
        let original = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(original));
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let target = self.array_species_create(original, 0)?;
            self.stack.push(Value::Object(target));
            let mut index = 0_u64;
            for value in std::iter::once(Value::Object(original)).chain(args.iter().cloned()) {
                self.stack.push(value.clone());
                if self.array_is_concat_spreadable(&value)? {
                    let source = self.coerce_object(&value)?;
                    self.stack.push(Value::Object(source));
                    let length = self.get_property(&Value::Object(source), &"length".into())?;
                    let length = self.coerce_length(&length)? as u64;
                    if index
                        .checked_add(length)
                        .is_none_or(|next| next > 9_007_199_254_740_991)
                    {
                        return Err(RuntimeError::TypeError(
                            "concatenated Array length exceeds the safe integer limit".into(),
                        ));
                    }
                    for source_index in 0..length {
                        self.charge_step()?;
                        let key: PropertyName = source_index.to_string().into();
                        if self.has_property(source, &key)? {
                            let element = self.get_property(&Value::Object(source), &key)?;
                            self.stack.push(element.clone());
                            self.array_create_data_property_or_throw(
                                target,
                                index.to_string().into(),
                                element,
                            )?;
                            self.stack.pop();
                        }
                        index += 1;
                    }
                    self.stack.pop();
                } else {
                    if index >= 9_007_199_254_740_991 {
                        return Err(RuntimeError::TypeError(
                            "concatenated Array length exceeds the safe integer limit".into(),
                        ));
                    }
                    self.array_create_data_property_or_throw(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    index += 1;
                }
                self.stack.pop();
            }
            self.array_set_or_throw(target, "length".into(), &Value::Number(index as f64))?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// ECMAScript `IsConcatSpreadable`. The well-known-symbol lookup is
    /// observable before `IsArray`; a false marker keeps even an Array (or an
    /// Array Proxy) as a single element, while a true marker spreads an
    /// arbitrary array-like object.
    fn array_is_concat_spreadable(&mut self, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(_) = value else {
            return Ok(false);
        };
        let marker =
            self.get_property(value, &JsSymbol::well_known("isConcatSpreadable").into())?;
        if marker != Value::Undefined {
            return self.to_boolean(&marker);
        }
        self.is_array(value)
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

    pub(in super::super) fn array_filter(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Array.prototype.filter callback must be callable".into(),
            ));
        }
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            let target = self.array_species_create(object, 0)?;
            self.stack.push(Value::Object(target));
            let result = (|| {
                let mut target_index = 0usize;
                if let Some(indices) = self.array_own_indices(object, length)? {
                    for index in indices {
                        self.charge_step()?;
                        let value =
                            self.get_property(&Value::Object(object), &index.to_string().into())?;
                        let selected = self.call_native(
                            callback.clone(),
                            this_arg.clone(),
                            vec![
                                value.clone(),
                                Value::Number(index as f64),
                                Value::Object(object),
                            ],
                            false,
                        )?;
                        if self.to_boolean(&selected)? {
                            self.array_create_data_property_or_throw(
                                target,
                                target_index.to_string().into(),
                                value,
                            )?;
                            target_index += 1;
                        }
                    }
                } else {
                    for index in 0..length {
                        self.charge_step()?;
                        let key: PropertyName = index.to_string().into();
                        if !self.has_property(object, &key)? {
                            continue;
                        }
                        let value = self.get_property(&Value::Object(object), &key)?;
                        let selected = self.call_native(
                            callback.clone(),
                            this_arg.clone(),
                            vec![
                                value.clone(),
                                Value::Number(index as f64),
                                Value::Object(object),
                            ],
                            false,
                        )?;
                        if self.to_boolean(&selected)? {
                            self.array_create_data_property_or_throw(
                                target,
                                target_index.to_string().into(),
                                value,
                            )?;
                            target_index += 1;
                        }
                    }
                }
                Ok(Value::Object(target))
            })();
            self.stack.pop();
            result
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn array_map(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            if !self.is_callable(callback)? {
                return Err(RuntimeError::TypeError(
                    "Array.prototype.map callback must be callable".into(),
                ));
            }
            let target = self.array_species_create(object, length as usize)?;
            self.stack.push(Value::Object(target));
            let outcome = (|| {
                for index in 0..length {
                    self.charge_step()?;
                    let key: PropertyName = index.to_string().into();
                    if !self.has_property(object, &key)? {
                        continue;
                    }
                    let value = self.get_property(&Value::Object(object), &key)?;
                    let mapped = self.call_native(
                        callback.clone(),
                        this_arg.clone(),
                        vec![value, Value::Number(index as f64), Value::Object(object)],
                        false,
                    )?;
                    self.array_create_data_property_or_throw(target, key, mapped)?;
                }
                Ok(Value::Object(target))
            })();
            self.stack.pop();
            outcome
        })();
        self.stack.pop();
        result
    }

    fn array_predicate(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
        some: bool,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            if !self.is_callable(callback)? {
                return Err(RuntimeError::TypeError(
                    "Array predicate callback must be callable".into(),
                ));
            }
            for index in 0..length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if !self.has_property(object, &key)? {
                    continue;
                }
                let value = self.get_property(&Value::Object(object), &key)?;
                let selected = self.call_native(
                    callback.clone(),
                    this_arg.clone(),
                    vec![value, Value::Number(index as f64), Value::Object(object)],
                    false,
                )?;
                if self.to_boolean(&selected)? == some {
                    return Ok(Value::Bool(some));
                }
            }
            Ok(Value::Bool(!some))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn array_every(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_predicate(receiver, callback, this_arg, false)
    }

    pub(in super::super) fn array_some(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_predicate(receiver, callback, this_arg, true)
    }

    pub(in super::super) fn array_find(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
        reverse: bool,
        return_index: bool,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            if !self.is_callable(callback)? {
                return Err(RuntimeError::TypeError(
                    "Array.prototype.find callback must be callable".into(),
                ));
            }
            let indices: Box<dyn Iterator<Item = u64>> = if reverse {
                Box::new((0..length).rev())
            } else {
                Box::new(0..length)
            };
            // Unlike map/every/some, find visits holes and passes undefined
            // to the predicate. This is why it must use [[Get]] directly.
            for index in indices {
                self.charge_step()?;
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                let selected = self.call_native(
                    callback.clone(),
                    this_arg.clone(),
                    vec![
                        value.clone(),
                        Value::Number(index as f64),
                        Value::Object(object),
                    ],
                    false,
                )?;
                if self.to_boolean(&selected)? {
                    return Ok(if return_index {
                        Value::Number(index as f64)
                    } else {
                        value
                    });
                }
            }
            Ok(if return_index {
                Value::Number(-1.0)
            } else {
                Value::Undefined
            })
        })();
        self.stack.pop();
        result
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

    pub(in super::super) fn array_reduce_right(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let callback = native::argument(args, 0);
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let mut index = self.coerce_length(&length)? as u64;
            if !self.is_callable(callback)? {
                return Err(RuntimeError::TypeError(
                    "Array.prototype.reduceRight callback must be callable".into(),
                ));
            }
            let mut accumulator = if args.len() > 1 {
                args[1].clone()
            } else {
                loop {
                    if index == 0 {
                        return Err(RuntimeError::TypeError(
                            "reduceRight of empty array with no initial value".into(),
                        ));
                    }
                    index -= 1;
                    let key: PropertyName = index.to_string().into();
                    if self.has_property(object, &key)? {
                        break self.get_property(&Value::Object(object), &key)?;
                    }
                }
            };
            while index > 0 {
                index -= 1;
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

    pub(in super::super) fn array_pop(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            if length == 0 {
                self.array_set_or_throw(object, "length".into(), &Value::Number(0.0))?;
                return Ok(Value::Undefined);
            }
            let key: PropertyName = (length - 1).to_string().into();
            let value = self.get_property(&Value::Object(object), &key)?;
            self.stack.push(value.clone());
            self.array_delete_or_throw(object, &key)?;
            self.array_set_or_throw(object, "length".into(), &Value::Number((length - 1) as f64))?;
            Ok(value)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_shift(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            if length == 0 {
                self.array_set_or_throw(object, "length".into(), &Value::Number(0.0))?;
                return Ok(Value::Undefined);
            }
            let first = self.get_property(&Value::Object(object), &"0".into())?;
            self.stack.push(first.clone());
            for index in 1..length {
                self.charge_step()?;
                let from: PropertyName = index.to_string().into();
                let to: PropertyName = (index - 1).to_string().into();
                if self.has_property(object, &from)? {
                    let value = self.get_property(&Value::Object(object), &from)?;
                    self.stack.push(value.clone());
                    self.array_set_or_throw(object, to, &value)?;
                    self.stack.pop();
                } else {
                    self.array_delete_or_throw(object, &to)?;
                }
            }
            self.array_delete_or_throw(object, &(length - 1).to_string().into())?;
            self.array_set_or_throw(object, "length".into(), &Value::Number((length - 1) as f64))?;
            Ok(first)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_unshift(
        &mut self,
        receiver: &Value,
        items: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        self.stack.extend(items.iter().cloned());
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            let new_length = length
                .checked_add(items.len() as u64)
                .filter(|length| *length <= 9_007_199_254_740_991)
                .ok_or_else(|| RuntimeError::TypeError("invalid Array length".into()))?;
            for index in (0..length).rev() {
                self.charge_step()?;
                let from: PropertyName = index.to_string().into();
                let to: PropertyName = (index + items.len() as u64).to_string().into();
                if self.has_property(object, &from)? {
                    let value = self.get_property(&Value::Object(object), &from)?;
                    self.stack.push(value.clone());
                    self.array_set_or_throw(object, to, &value)?;
                    self.stack.pop();
                } else {
                    self.array_delete_or_throw(object, &to)?;
                }
            }
            for (index, value) in items.iter().enumerate() {
                self.array_set_or_throw(object, index.to_string().into(), value)?;
            }
            self.array_set_or_throw(object, "length".into(), &Value::Number(new_length as f64))?;
            Ok(Value::Number(new_length as f64))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_reverse(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            for lower_index in 0..length / 2 {
                self.charge_step()?;
                let upper_index = length - lower_index - 1;
                let lower: PropertyName = lower_index.to_string().into();
                let upper: PropertyName = upper_index.to_string().into();
                let step_base = self.stack.len();
                let lower_value = if self.has_property(object, &lower)? {
                    let value = self.get_property(&Value::Object(object), &lower)?;
                    self.stack.push(value.clone());
                    Some(value)
                } else {
                    None
                };
                let upper_value = if self.has_property(object, &upper)? {
                    let value = self.get_property(&Value::Object(object), &upper)?;
                    self.stack.push(value.clone());
                    Some(value)
                } else {
                    None
                };
                let swapped: Result<(), RuntimeError> = (|| {
                    if let Some(value) = &upper_value {
                        self.array_set_or_throw(object, lower.clone(), value)?;
                    } else {
                        self.array_delete_or_throw(object, &lower)?;
                    }
                    if let Some(value) = &lower_value {
                        self.array_set_or_throw(object, upper.clone(), value)?;
                    } else {
                        self.array_delete_or_throw(object, &upper)?;
                    }
                    Ok(())
                })();
                self.stack.truncate(step_base);
                swapped?;
            }
            Ok(receiver.clone())
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
        typed_array_method: bool,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            // `%TypedArray%.prototype.toLocaleString` uses the receiver's
            // internal [[ArrayLength]] (and throws for an out-of-bounds
            // view), unlike Array.prototype's generic `Get("length")`
            // algorithm. `typed_array_method` selects that variant; the
            // Array.prototype entry point keeps the ordinary path even when
            // its receiver happens to be a TypedArray.
            let foreign_typed_values = if self.heap.is_typed_array(object)? {
                None
            } else {
                self.test262_foreign_typed_array_values(object)?
            };
            let length = if typed_array_method && self.heap.is_typed_array(object)? {
                let (_, _, length, _) = self.typed_array_receiver(&Value::Object(object))?;
                length as u64
            } else if let Some(values) = &foreign_typed_values {
                values.len() as u64
            } else {
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                self.coerce_length(&length)? as u64
            };
            let mut output = JsString::default();
            for index in 0..length {
                self.charge_step()?;
                if index != 0 {
                    native::append(&mut output, &",".into(), self.config.max_string_bytes)?;
                }
                let value = if let Some(values) = &foreign_typed_values {
                    values[index as usize].clone()
                } else {
                    self.get_property(&Value::Object(object), &index.to_string().into())?
                };
                if matches!(value, Value::Null | Value::Undefined) {
                    continue;
                }
                self.stack.push(value.clone());
                let method = self.get_property(&value, &"toLocaleString".into())?;
                self.stack.push(method.clone());
                if !self.is_callable(&method)? {
                    return Err(RuntimeError::TypeError(
                        "Array element toLocaleString is not callable".into(),
                    ));
                }
                // ECMA-262 forwards both optional arguments, including their
                // `undefined` defaults, to every non-null array element.
                // Supplying the pair explicitly is observable to user-defined
                // `toLocaleString` methods through `arguments.length`.
                let string = self.call_native(
                    method,
                    value,
                    vec![
                        native::argument(args, 0).clone(),
                        native::argument(args, 1).clone(),
                    ],
                    false,
                )?;
                let string = self.coerce_string(&string)?;
                native::append(&mut output, &string, self.config.max_string_bytes)?;
                self.stack.truncate(base + 1);
            }
            Ok(Value::String(output))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_last_index_of(
        &mut self,
        receiver: &Value,
        search: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            if length == 0 {
                return Ok(Value::Number(-1.0));
            }
            let length = length.min(i64::MAX as u64) as i64;
            let mut index = if args.len() < 2 {
                length - 1
            } else {
                let number = self.coerce_number(&args[1])?;
                if number.is_nan() {
                    0
                } else if number.is_sign_positive() && number.is_infinite() {
                    length - 1
                } else if number.is_sign_negative() && number.is_infinite() {
                    -1
                } else {
                    let integer = number.trunc();
                    if integer >= 0.0 {
                        integer.min((length - 1) as f64) as i64
                    } else if integer < -(length as f64) {
                        -1
                    } else {
                        (length as f64 + integer) as i64
                    }
                }
            };
            while index >= 0 {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)?
                    && self.get_property(&Value::Object(object), &key)? == *search
                {
                    return Ok(Value::Number(index as f64));
                }
                index -= 1;
            }
            Ok(Value::Number(-1.0))
        })();
        self.stack.pop();
        result
    }

    /// Array.prototype.sort with the observable comparison path retained for
    /// the host scheduler's report arrays. Undefined values follow sorted
    /// present values and holes remain trailing holes, as required by the
    /// ArraySort collection/write-back steps.
    pub(in super::super) fn array_sort(
        &mut self,
        receiver: &Value,
        compare: &Value,
    ) -> Result<Value, RuntimeError> {
        if *compare != Value::Undefined && !self.is_callable(compare)? {
            return Err(RuntimeError::TypeError(
                "Array sort comparator must be callable".into(),
            ));
        }
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as usize;
            let mut values = Vec::new();
            let mut undefined = 0usize;
            for index in 0..length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)? {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    if value == Value::Undefined {
                        undefined += 1;
                    } else {
                        values.push(value);
                    }
                }
            }
            // Bottom-up merge sorting remains stable while bounding observable
            // user comparator calls to O(n log n). The 513- and 2048-element
            // stable-array-sort conformance cases exercise this exact path.
            let mut scratch = values.to_vec();
            let mut width = 1usize;
            while width < values.len() {
                let mut start = 0usize;
                while start < values.len() {
                    let middle = start.saturating_add(width).min(values.len());
                    let end = middle.saturating_add(width).min(values.len());
                    let (mut left, mut right, mut target) = (start, middle, start);
                    while left < middle && right < end {
                        if self.array_sort_order(compare, &values[right], &values[left])?
                            == std::cmp::Ordering::Less
                        {
                            scratch[target] = values[right].clone();
                            right += 1;
                        } else {
                            scratch[target] = values[left].clone();
                            left += 1;
                        }
                        target += 1;
                    }
                    while left < middle {
                        scratch[target] = values[left].clone();
                        target += 1;
                        left += 1;
                    }
                    while right < end {
                        scratch[target] = values[right].clone();
                        target += 1;
                        right += 1;
                    }
                    values[start..end].clone_from_slice(&scratch[start..end]);
                    start = end;
                }
                width = width.saturating_mul(2);
            }
            let mut index = 0usize;
            for value in values {
                self.set_property_value(&Value::Object(object), &index.to_string().into(), &value)?;
                index += 1;
            }
            for _ in 0..undefined {
                self.set_property_value(
                    &Value::Object(object),
                    &index.to_string().into(),
                    &Value::Undefined,
                )?;
                index += 1;
            }
            while index < length {
                self.object_delete(object, &index.to_string().into())?;
                index += 1;
            }
            Ok(receiver.clone())
        })();
        self.stack.pop();
        result
    }

    fn array_sort_order(
        &mut self,
        compare: &Value,
        left: &Value,
        right: &Value,
    ) -> Result<std::cmp::Ordering, RuntimeError> {
        if *compare == Value::Undefined {
            return Ok(self.coerce_string(left)?.cmp(&self.coerce_string(right)?));
        }
        let value = self.call_native(
            compare.clone(),
            Value::Undefined,
            vec![left.clone(), right.clone()],
            false,
        )?;
        let value = self.coerce_number(&value)?;
        Ok(if value.is_nan() || value == 0.0 {
            std::cmp::Ordering::Equal
        } else if value < 0.0 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        })
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
        if !self.is_array(&Value::Object(original))? {
            return ordinary_array(self);
        }
        let original = Value::Object(original);
        let constructor = self.get_property(&original, &"constructor".into())?;
        if constructor == Value::Undefined {
            return ordinary_array(self);
        }
        if let Some(constructor) = constructor.object_id() {
            if self.test262_foreign_intrinsic_constructor(constructor, "Array")? {
                return ordinary_array(self);
            }
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

    pub(super) fn array_set_or_throw(
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

    pub(super) fn array_create_data_property_or_throw(
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
            // count = max(final - k, 0): an end before the start yields an
            // empty result, never a wrapped-around length.
            let count = end.saturating_sub(start).max(0) as usize;
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
