// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    fn typed_array_method_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, TypedArrayKind), RuntimeError> {
        let (_, _, length, kind) = self.typed_array_receiver(receiver)?;
        let object = receiver
            .object_id()
            .expect("TypedArray receiver has an object identity");
        Ok((object, length, kind))
    }

    fn typed_array_new_same_kind(
        &mut self,
        length: usize,
        kind: TypedArrayKind,
    ) -> Result<ObjectId, RuntimeError> {
        let buffer = self.new_typed_array_buffer(length, kind)?;
        let prototype = self.buffer_prototype(kind.name())?;
        self.with_roots(|heap| {
            heap.alloc_typed_array(buffer, 0, length, false, kind, Some(prototype))
        })
    }

    /// TypedArraySpeciesCreate for algorithms which intentionally preserve a
    /// subclass's species. The copying `to*` methods use
    /// `typed_array_new_same_kind` instead: ES2023 made those methods ignore
    /// `constructor` and `Symbol.species`.
    fn typed_array_species_create(
        &mut self,
        receiver: &Value,
        length: usize,
        fallback_kind: TypedArrayKind,
    ) -> Result<(ObjectId, TypedArrayKind), RuntimeError> {
        let fallback = self.global(fallback_kind.name())?;
        let constructor = self.get_property(receiver, &"constructor".into())?;
        let constructor = if constructor == Value::Undefined {
            fallback
        } else {
            if !matches!(constructor, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "TypedArray constructor must be an object".into(),
                ));
            }
            let species =
                self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
            if matches!(species, Value::Undefined | Value::Null) {
                fallback
            } else {
                species
            }
        };
        if !self.is_constructor(&constructor)? {
            return Err(RuntimeError::TypeError(
                "TypedArray species must be a constructor".into(),
            ));
        }
        let result = self.call_with_target(
            constructor.clone(),
            Value::Undefined,
            vec![Value::Number(length as f64)],
            true,
            constructor,
        )?;
        let (_, _, result_length, result_kind) = self.typed_array_receiver(&result)?;
        if result_length < length {
            return Err(RuntimeError::TypeError(
                "TypedArray species result is too small".into(),
            ));
        }
        Ok((
            result
                .object_id()
                .expect("validated TypedArray result has an object identity"),
            result_kind,
        ))
    }

    fn typed_array_read_values(
        &self,
        object: ObjectId,
        start: usize,
        length: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let end = start
            .checked_add(length)
            .ok_or_else(|| RuntimeError::RangeError("TypedArray range is too large".into()))?;
        (start..end)
            .map(|index| {
                self.heap
                    .typed_array_index_value(object, index)?
                    .ok_or_else(|| RuntimeError::TypeError("TypedArray is out of bounds".into()))
            })
            .collect()
    }

    fn typed_array_element(&self, object: ObjectId, index: usize) -> Result<Value, RuntimeError> {
        self.heap
            .typed_array_index_value(object, index)?
            .ok_or_else(|| RuntimeError::TypeError("TypedArray is out of bounds".into()))
    }

    fn typed_array_write_values(
        &mut self,
        object: ObjectId,
        kind: TypedArrayKind,
        start: usize,
        values: &[Value],
    ) -> Result<(), RuntimeError> {
        for (index, value) in values.iter().enumerate() {
            let value = self.typed_array_element_value(kind, value)?;
            self.with_roots(|heap| heap.typed_array_set_index(object, start + index, &value))?;
        }
        Ok(())
    }

    fn typed_array_callback(
        &mut self,
        callback: &Value,
        this_arg: &Value,
        value: Value,
        index: usize,
        object: ObjectId,
    ) -> Result<Value, RuntimeError> {
        self.call_native(
            callback.clone(),
            this_arg.clone(),
            vec![value, Value::Number(index as f64), Value::Object(object)],
            false,
        )
    }

    fn typed_array_callback_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
        method: TypedArrayMethod,
    ) -> Result<Value, RuntimeError> {
        let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
        let callback = native::argument(args, 0);
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "TypedArray callback must be callable".into(),
            ));
        }
        let this_arg = native::argument(args, 1).clone();
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| match method {
            TypedArrayMethod::Every => {
                for index in 0..length {
                    let value = self
                        .heap
                        .typed_array_index_value(object, index)?
                        .ok_or_else(|| {
                            RuntimeError::TypeError("TypedArray is out of bounds".into())
                        })?;
                    let result =
                        self.typed_array_callback(callback, &this_arg, value, index, object)?;
                    if !self.to_boolean(&result)? {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            TypedArrayMethod::ForEach => {
                for index in 0..length {
                    let value = self.typed_array_element(object, index)?;
                    self.typed_array_callback(callback, &this_arg, value, index, object)?;
                }
                Ok(Value::Undefined)
            }
            TypedArrayMethod::Some => {
                for index in 0..length {
                    let value = self
                        .heap
                        .typed_array_index_value(object, index)?
                        .ok_or_else(|| {
                            RuntimeError::TypeError("TypedArray is out of bounds".into())
                        })?;
                    let result =
                        self.typed_array_callback(callback, &this_arg, value, index, object)?;
                    if self.to_boolean(&result)? {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            TypedArrayMethod::Find | TypedArrayMethod::FindIndex => {
                for index in 0..length {
                    let value = self
                        .heap
                        .typed_array_index_value(object, index)?
                        .ok_or_else(|| {
                            RuntimeError::TypeError("TypedArray is out of bounds".into())
                        })?;
                    let result = self.typed_array_callback(
                        callback,
                        &this_arg,
                        value.clone(),
                        index,
                        object,
                    )?;
                    if self.to_boolean(&result)? {
                        return Ok(if method == TypedArrayMethod::Find {
                            value
                        } else {
                            Value::Number(index as f64)
                        });
                    }
                }
                Ok(if method == TypedArrayMethod::Find {
                    Value::Undefined
                } else {
                    Value::Number(-1.0)
                })
            }
            TypedArrayMethod::FindLast | TypedArrayMethod::FindLastIndex => {
                for index in (0..length).rev() {
                    let value = self
                        .heap
                        .typed_array_index_value(object, index)?
                        .ok_or_else(|| {
                            RuntimeError::TypeError("TypedArray is out of bounds".into())
                        })?;
                    let result = self.typed_array_callback(
                        callback,
                        &this_arg,
                        value.clone(),
                        index,
                        object,
                    )?;
                    if self.to_boolean(&result)? {
                        return Ok(if method == TypedArrayMethod::FindLast {
                            value
                        } else {
                            Value::Number(index as f64)
                        });
                    }
                }
                Ok(if method == TypedArrayMethod::FindLast {
                    Value::Undefined
                } else {
                    Value::Number(-1.0)
                })
            }
            TypedArrayMethod::Map => {
                let (target, target_kind) =
                    self.typed_array_species_create(receiver, length, kind)?;
                self.stack.push(Value::Object(target));
                for index in 0..length {
                    let value = self
                        .heap
                        .typed_array_index_value(object, index)?
                        .ok_or_else(|| {
                            RuntimeError::TypeError("TypedArray is out of bounds".into())
                        })?;
                    let value =
                        self.typed_array_callback(callback, &this_arg, value, index, object)?;
                    self.typed_array_write_values(target, target_kind, index, &[value])?;
                }
                Ok(Value::Object(target))
            }
            TypedArrayMethod::Filter => {
                let mut selected = Vec::new();
                for index in 0..length {
                    let value = self
                        .heap
                        .typed_array_index_value(object, index)?
                        .ok_or_else(|| {
                            RuntimeError::TypeError("TypedArray is out of bounds".into())
                        })?;
                    let result = self.typed_array_callback(
                        callback,
                        &this_arg,
                        value.clone(),
                        index,
                        object,
                    )?;
                    if self.to_boolean(&result)? {
                        selected.push(value);
                    }
                }
                let (target, target_kind) =
                    self.typed_array_species_create(receiver, selected.len(), kind)?;
                self.stack.push(Value::Object(target));
                self.typed_array_write_values(target, target_kind, 0, &selected)?;
                Ok(Value::Object(target))
            }
            _ => unreachable!("only callback TypedArray methods use this helper"),
        })();
        self.stack.truncate(base);
        result
    }

    fn typed_array_last_index_of(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (object, length, _) = self.typed_array_method_receiver(receiver)?;
        if length == 0 {
            return Ok(Value::Number(-1.0));
        }
        let search = native::argument(args, 0);
        let from = if args.len() < 2 || args[1] == Value::Undefined {
            length as f64 - 1.0
        } else {
            self.coerce_number(native::argument(args, 1))?
        };
        if from == f64::NEG_INFINITY {
            return Ok(Value::Number(-1.0));
        }
        let mut index = if from == f64::INFINITY {
            length - 1
        } else {
            let integer = from.trunc();
            if integer >= 0.0 {
                (integer as usize).min(length - 1)
            } else {
                let magnitude = (-integer) as usize;
                if magnitude > length {
                    return Ok(Value::Number(-1.0));
                }
                length - magnitude
            }
        };
        loop {
            self.charge_step()?;
            if self.typed_array_element(object, index)? == *search {
                return Ok(Value::Number(index as f64));
            }
            if index == 0 {
                return Ok(Value::Number(-1.0));
            }
            index -= 1;
        }
    }

    fn typed_array_includes(
        &mut self,
        receiver: &Value,
        args: &[Value],
        equality: fn(&Value, &Value) -> bool,
    ) -> Result<Value, RuntimeError> {
        let (object, length, _) = self.typed_array_method_receiver(receiver)?;
        let search = native::argument(args, 0);
        let from = if args.len() < 2 || args[1] == Value::Undefined {
            0.0
        } else {
            self.coerce_number(native::argument(args, 1))?
        };
        if from == f64::INFINITY {
            return Ok(Value::Bool(false));
        }
        let start = if from == f64::NEG_INFINITY {
            0
        } else {
            let integer = from.trunc();
            if integer >= 0.0 {
                (integer as usize).min(length)
            } else {
                length.saturating_sub((-integer) as usize)
            }
        };
        for index in start..length {
            self.charge_step()?;
            if equality(&self.typed_array_element(object, index)?, search) {
                return Ok(Value::Bool(true));
            }
        }
        Ok(Value::Bool(false))
    }

    fn typed_array_index_of(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (object, length, _) = self.typed_array_method_receiver(receiver)?;
        let search = native::argument(args, 0);
        let from = if args.len() < 2 || args[1] == Value::Undefined {
            0.0
        } else {
            self.coerce_number(native::argument(args, 1))?
        };
        if from == f64::INFINITY {
            return Ok(Value::Number(-1.0));
        }
        let start = if from == f64::NEG_INFINITY {
            0
        } else {
            let integer = from.trunc();
            if integer >= 0.0 {
                (integer as usize).min(length)
            } else {
                length.saturating_sub((-integer) as usize)
            }
        };
        for index in start..length {
            self.charge_step()?;
            if self.typed_array_element(object, index)? == *search {
                return Ok(Value::Number(index as f64));
            }
        }
        Ok(Value::Number(-1.0))
    }

    fn typed_array_join(
        &mut self,
        receiver: &Value,
        separator: &Value,
    ) -> Result<Value, RuntimeError> {
        let (object, length, _) = self.typed_array_method_receiver(receiver)?;
        let separator = if *separator == Value::Undefined {
            ",".into()
        } else {
            self.coerce_string(separator)?
        };
        let mut result = JsString::default();
        for index in 0..length {
            self.charge_step()?;
            if index > 0 {
                native::append(&mut result, &separator, self.config.max_string_bytes)?;
            }
            let value = self.typed_array_element(object, index)?;
            let value = self.coerce_string(&value)?;
            native::append(&mut result, &value, self.config.max_string_bytes)?;
        }
        Ok(Value::String(result))
    }

    fn typed_array_reduce(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (object, length, _) = self.typed_array_method_receiver(receiver)?;
        let callback = native::argument(args, 0);
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "TypedArray callback must be callable".into(),
            ));
        }
        let mut index = 0;
        let mut accumulator = if args.len() > 1 {
            args[1].clone()
        } else {
            if length == 0 {
                return Err(RuntimeError::TypeError(
                    "reduce of empty TypedArray with no initial value".into(),
                ));
            }
            index = 1;
            self.typed_array_element(object, 0)?
        };
        while index < length {
            let value = self.typed_array_element(object, index)?;
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
            index += 1;
        }
        Ok(accumulator)
    }

    fn typed_array_slice(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
        let start = self.relative_buffer_index(native::argument(args, 0), length)?;
        let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let count = end.saturating_sub(start);
        let (target, target_kind) = self.typed_array_species_create(receiver, count, kind)?;
        let values = self.typed_array_read_values(object, start, count)?;
        self.typed_array_write_values(target, target_kind, 0, &values)?;
        Ok(Value::Object(target))
    }

    fn typed_array_default_compare(
        left: &Value,
        right: &Value,
        kind: TypedArrayKind,
    ) -> std::cmp::Ordering {
        if kind.bigint() {
            let (Value::BigInt(left), Value::BigInt(right)) = (left, right) else {
                unreachable!("BigInt typed array values are BigInt");
            };
            return left.cmp(right);
        }
        let (Value::Number(left), Value::Number(right)) = (left, right) else {
            unreachable!("numeric typed array values are Number");
        };
        if left.is_nan() {
            return if right.is_nan() {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Greater
            };
        }
        if right.is_nan() {
            return std::cmp::Ordering::Less;
        }
        if *left == 0.0 && *right == 0.0 {
            return match (left.is_sign_negative(), right.is_sign_negative()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            };
        }
        left.partial_cmp(right)
            .expect("non-NaN numbers are comparable")
    }

    fn typed_array_sort_values(
        &mut self,
        values: &mut [Value],
        compare: &Value,
        kind: TypedArrayKind,
    ) -> Result<(), RuntimeError> {
        if *compare != Value::Undefined && !self.is_callable(compare)? {
            return Err(RuntimeError::TypeError(
                "TypedArray sort comparator must be callable".into(),
            ));
        }
        if *compare == Value::Undefined {
            values.sort_by(|left, right| Self::typed_array_default_compare(left, right, kind));
            return Ok(());
        }

        // The values are snapshotted before sorting. A comparator may mutate
        // the receiver, but must not turn an O(n log n) sort into O(n²)
        // interpreter calls. Detecting natural runs avoids needlessly calling
        // an observable comparator O(n log n) times for already sorted input
        // (including Test262's descending TypedArray cases), then stable
        // merging handles the general case and propagates abrupt completions.
        let mut scratch = values.to_vec();
        let mut runs = Vec::new();
        let mut start = 0usize;
        while start < values.len() {
            let mut end = start + 1;
            if end < values.len() {
                let descending =
                    self.typed_array_compare_values(compare, &values[end], &values[end - 1])?
                        == std::cmp::Ordering::Less;
                end += 1;
                while end < values.len() {
                    let order =
                        self.typed_array_compare_values(compare, &values[end], &values[end - 1])?;
                    if (descending && order != std::cmp::Ordering::Less)
                        || (!descending && order == std::cmp::Ordering::Less)
                    {
                        break;
                    }
                    end += 1;
                }
                // Only a strictly descending run reaches here, so reversing
                // it cannot disturb the comparator's stable equal elements.
                if descending {
                    values[start..end].reverse();
                }
            }
            runs.push((start, end));
            start = end;
        }
        while runs.len() > 1 {
            let mut next_runs = Vec::with_capacity((runs.len() + 1) / 2);
            let mut index = 0usize;
            while index < runs.len() {
                let (start, middle) = runs[index];
                let Some(&(right_start, end)) = runs.get(index + 1) else {
                    next_runs.push((start, middle));
                    break;
                };
                debug_assert_eq!(middle, right_start);
                let (mut left, mut right, mut target) = (start, middle, start);
                while left < middle && right < end {
                    if self.typed_array_compare_values(compare, &values[right], &values[left])?
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
                next_runs.push((start, end));
                index += 2;
            }
            runs = next_runs;
        }
        Ok(())
    }

    fn typed_array_compare_values(
        &mut self,
        compare: &Value,
        left: &Value,
        right: &Value,
    ) -> Result<std::cmp::Ordering, RuntimeError> {
        let result = self.call_native(
            compare.clone(),
            Value::Undefined,
            vec![left.clone(), right.clone()],
            false,
        )?;
        let result = self.coerce_number(&result)?;
        Ok(if result.is_nan() || result == 0.0 {
            std::cmp::Ordering::Equal
        } else if result < 0.0 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        })
    }

    fn typed_array_to_reversed(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
        let values = self.typed_array_read_values(object, 0, length)?;
        let target = self.typed_array_new_same_kind(length, kind)?;
        for (index, value) in values.into_iter().rev().enumerate() {
            self.typed_array_write_values(target, kind, index, &[value])?;
        }
        Ok(Value::Object(target))
    }

    fn typed_array_to_sorted(
        &mut self,
        receiver: &Value,
        compare: &Value,
    ) -> Result<Value, RuntimeError> {
        let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
        let mut values = self.typed_array_read_values(object, 0, length)?;
        self.typed_array_sort_values(&mut values, compare, kind)?;
        let target = self.typed_array_new_same_kind(length, kind)?;
        self.typed_array_write_values(target, kind, 0, &values)?;
        Ok(Value::Object(target))
    }

    fn typed_array_with(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
        let index = self.coerce_number(native::argument(args, 0))?;
        if !index.is_finite() {
            return Err(RuntimeError::RangeError(
                "TypedArray index is outside its bounds".into(),
            ));
        }
        let index = index.trunc();
        let index = if index < 0.0 {
            let magnitude = (-index) as usize;
            if magnitude > length {
                return Err(RuntimeError::RangeError(
                    "TypedArray index is outside its bounds".into(),
                ));
            }
            length - magnitude
        } else {
            index as usize
        };
        if index >= length {
            return Err(RuntimeError::RangeError(
                "TypedArray index is outside its bounds".into(),
            ));
        }
        let replacement = self.typed_array_element_value(kind, native::argument(args, 1))?;
        let values = self.typed_array_read_values(object, 0, length)?;
        let target = self.typed_array_new_same_kind(length, kind)?;
        self.typed_array_write_values(target, kind, 0, &values)?;
        self.typed_array_write_values(target, kind, index, &[replacement])?;
        Ok(Value::Object(target))
    }

    pub(super) fn typed_array_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
        method: TypedArrayMethod,
    ) -> Result<Value, RuntimeError> {
        match method {
            TypedArrayMethod::Every
            | TypedArrayMethod::ForEach
            | TypedArrayMethod::Some
            | TypedArrayMethod::Find
            | TypedArrayMethod::FindIndex
            | TypedArrayMethod::FindLast
            | TypedArrayMethod::FindLastIndex
            | TypedArrayMethod::Map
            | TypedArrayMethod::Filter => self.typed_array_callback_method(receiver, args, method),
            TypedArrayMethod::At => {
                let (object, length, _) = self.typed_array_method_receiver(receiver)?;
                let index = self.coerce_number(native::argument(args, 0))?;
                let index = if !index.is_finite() {
                    return Ok(Value::Undefined);
                } else if index < 0.0 {
                    length.saturating_sub((-index.trunc()) as usize)
                } else {
                    index.trunc() as usize
                };
                if index >= length {
                    return Ok(Value::Undefined);
                }
                self.heap
                    .typed_array_index_value(object, index)?
                    .ok_or_else(|| RuntimeError::TypeError("TypedArray is out of bounds".into()))
            }
            TypedArrayMethod::LastIndexOf => self.typed_array_last_index_of(receiver, args),
            TypedArrayMethod::CopyWithin => {
                let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
                let target = self.relative_buffer_index(native::argument(args, 0), length)?;
                let start = self.relative_buffer_index(native::argument(args, 1), length)?;
                let end = if args.get(2).is_some_and(|value| *value != Value::Undefined) {
                    self.relative_buffer_index(native::argument(args, 2), length)?
                } else {
                    length
                };
                let count = end.saturating_sub(start).min(length.saturating_sub(target));
                let values = self.typed_array_read_values(object, start, count)?;
                self.typed_array_write_values(object, kind, target, &values)?;
                Ok(receiver.clone())
            }
            TypedArrayMethod::Fill => {
                let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
                let start = self.relative_buffer_index(native::argument(args, 1), length)?;
                let end = if args.get(2).is_some_and(|value| *value != Value::Undefined) {
                    self.relative_buffer_index(native::argument(args, 2), length)?
                } else {
                    length
                };
                let value = self.typed_array_element_value(kind, native::argument(args, 0))?;
                // A user-defined conversion can resize a resizable backing
                // buffer.  Fixed-length views must reject their newly
                // out-of-bounds state before the first indexed write.
                self.typed_array_receiver(receiver)?;
                for index in start..end {
                    self.with_roots(|heap| heap.typed_array_set_index(object, index, &value))?;
                }
                Ok(receiver.clone())
            }
            TypedArrayMethod::Includes => self.typed_array_includes(receiver, args, |left, right| {
                left == right
                    || matches!((left, right), (Value::Number(left), Value::Number(right)) if left.is_nan() && right.is_nan())
            }),
            TypedArrayMethod::IndexOf => self.typed_array_index_of(receiver, args),
            TypedArrayMethod::Join => self.typed_array_join(receiver, native::argument(args, 0)),
            TypedArrayMethod::Reduce => self.typed_array_reduce(receiver, args),
            TypedArrayMethod::ToString => {
                self.typed_array_method_receiver(receiver)?;
                let join = self.get_property(receiver, &"join".into())?;
                if self.is_callable(&join)? {
                    self.call_native(join, receiver.clone(), vec![], false)
                } else {
                    self.native_call(NativeFunction::ObjectToString, receiver.clone(), vec![], false)
                }
            }
            TypedArrayMethod::ReduceRight => {
                let (object, length, _) = self.typed_array_method_receiver(receiver)?;
                let callback = native::argument(args, 0);
                if !self.is_callable(callback)? {
                    return Err(RuntimeError::TypeError(
                        "TypedArray callback must be callable".into(),
                    ));
                }
                let mut index = length;
                let mut accumulator = if args.len() > 1 {
                    args[1].clone()
                } else {
                    if index == 0 {
                        return Err(RuntimeError::TypeError(
                            "reduce of empty TypedArray with no initial value".into(),
                        ));
                    }
                    index -= 1;
                    self.typed_array_element(object, index)?
                };
                while index > 0 {
                    index -= 1;
                    let value = self.typed_array_element(object, index)?;
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
                Ok(accumulator)
            }
            TypedArrayMethod::Reverse => {
                let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
                let values = self.typed_array_read_values(object, 0, length)?;
                let reversed: Vec<_> = values.into_iter().rev().collect();
                self.typed_array_write_values(object, kind, 0, &reversed)?;
                Ok(receiver.clone())
            }
            TypedArrayMethod::Slice => self.typed_array_slice(receiver, args),
            TypedArrayMethod::Sort => {
                let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
                let mut values = self.typed_array_read_values(object, 0, length)?;
                self.typed_array_sort_values(&mut values, native::argument(args, 0), kind)?;
                self.typed_array_write_values(object, kind, 0, &values)?;
                Ok(receiver.clone())
            }
            TypedArrayMethod::ToReversed => self.typed_array_to_reversed(receiver),
            TypedArrayMethod::ToSorted => {
                self.typed_array_to_sorted(receiver, native::argument(args, 0))
            }
            TypedArrayMethod::With => self.typed_array_with(receiver, args),
        }
    }
}
