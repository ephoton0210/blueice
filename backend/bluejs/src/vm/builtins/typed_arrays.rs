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

    /// `ValidateTypedArray(O, ~seq-cst~, ~write~)` for a mutating method:
    /// like `typed_array_method_receiver`, but a view over an immutable
    /// ArrayBuffer is rejected first, before any argument is read.
    fn typed_array_write_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, TypedArrayKind), RuntimeError> {
        self.reject_immutable_typed_array(receiver)?;
        self.typed_array_method_receiver(receiver)
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
        let constructor = self.typed_array_species_constructor(receiver, fallback)?;
        self.typed_array_create(constructor, length)
    }

    /// SpeciesConstructor(exemplar, defaultConstructor), shared by
    /// TypedArraySpeciesCreate's length-based users and `subarray`, whose
    /// constructor argument list is instead a buffer/byte-offset view tuple.
    pub(super) fn typed_array_species_constructor(
        &mut self,
        receiver: &Value,
        fallback: Value,
    ) -> Result<Value, RuntimeError> {
        let constructor = self.get_property(receiver, &"constructor".into())?;
        if constructor == Value::Undefined {
            return Ok(fallback);
        }
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "TypedArray constructor must be an object".into(),
            ));
        }
        let species = self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
        Ok(if matches!(species, Value::Undefined | Value::Null) {
            fallback
        } else {
            species
        })
    }

    /// TypedArrayCreate(constructor, argumentList) for the single-length-
    /// argument case: constructs, then validates the result is a
    /// non-detached TypedArray whose length is at least the requested one.
    /// Every caller (species `map`/`filter`/`slice`, `from`, `of`) creates a
    /// destination it goes on to write, i.e. `TypedArrayCreateFromConstructor`
    /// with `~write~`, so a result over an immutable ArrayBuffer is rejected.
    pub(super) fn typed_array_create(
        &mut self,
        constructor: Value,
        length: usize,
    ) -> Result<(ObjectId, TypedArrayKind), RuntimeError> {
        if !self.is_constructor(&constructor)? {
            return Err(RuntimeError::TypeError(
                "TypedArray constructor must be a constructor".into(),
            ));
        }
        let result = self.call_with_target(
            constructor.clone(),
            Value::Undefined,
            vec![Value::Number(length as f64)],
            true,
            constructor,
        )?;
        self.reject_immutable_typed_array(&result)?;
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

    /// Builds a concrete TypedArray in a foreign Test262 Realm. Generic
    /// `%TypedArray%.from`/`.of` collect and map caller-owned values in the
    /// caller Realm, then write them into this facade one at a time; an
    /// ordinary membrane transport deliberately exposes no source-array
    /// properties to a child VM.
    pub(super) fn typed_array_create_foreign_target(
        &mut self,
        constructor: &Value,
        length: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some(constructor_id) = constructor.object_id() else {
            return Ok(None);
        };
        let Some(NativeFunction::TypedArray(_)) =
            self.test262_foreign_native_function(constructor_id)?
        else {
            return Ok(None);
        };
        let target = self.call_with_target(
            constructor.clone(),
            Value::Undefined,
            vec![Value::Number(length as f64)],
            true,
            constructor.clone(),
        )?;
        let actual = self.get_property(&target, &"length".into())?;
        let actual = self.coerce_length(&actual)? as usize;
        if actual < length {
            return Err(RuntimeError::TypeError(
                "TypedArray species result is too small".into(),
            ));
        }
        Ok(Some(target))
    }

    pub(super) fn typed_array_read_values(
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
        // Indexed TypedArray iteration methods capture their iteration range
        // before invoking user callbacks. If a resizable backing buffer then
        // shrinks, each later missing integer-indexed element is observed as
        // `undefined`, rather than terminating that already-started loop.
        Ok(self
            .heap
            .typed_array_index_value(object, index)?
            .unwrap_or(Value::Undefined))
    }

    pub(super) fn typed_array_write_values(
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
                    let value = self.typed_array_element(object, index)?;
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
                    let value = self.typed_array_element(object, index)?;
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
                    let value = self.typed_array_element(object, index)?;
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
                    let value = self.typed_array_element(object, index)?;
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
                    let value = self.typed_array_element(object, index)?;
                    let value =
                        self.typed_array_callback(callback, &this_arg, value, index, object)?;
                    self.typed_array_write_values(target, target_kind, index, &[value])?;
                }
                Ok(Value::Object(target))
            }
            TypedArrayMethod::Filter => {
                let mut selected = Vec::new();
                for index in 0..length {
                    let value = self.typed_array_element(object, index)?;
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
        let from = if args.len() < 2 {
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
            let integer = if from.is_nan() { 0.0 } else { from.trunc() };
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
            if self.heap.typed_array_index_value(object, index)? == Some(search.clone()) {
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
        if length == 0 {
            return Ok(Value::Bool(false));
        }
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
            let integer = if from.is_nan() { 0.0 } else { from.trunc() };
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
        if length == 0 {
            return Ok(Value::Number(-1.0));
        }
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
            let integer = if from.is_nan() { 0.0 } else { from.trunc() };
            if integer >= 0.0 {
                (integer as usize).min(length)
            } else {
                length.saturating_sub((-integer) as usize)
            }
        };
        for index in start..length {
            self.charge_step()?;
            if self.heap.typed_array_index_value(object, index)? == Some(search.clone()) {
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
            if !matches!(value, Value::Undefined | Value::Null) {
                let value = self.coerce_string(&value)?;
                native::append(&mut result, &value, self.config.max_string_bytes)?;
            }
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
        let fallback = self.global(kind.name())?;
        let constructor = self.typed_array_species_constructor(receiver, fallback)?;
        let foreign_target = self.typed_array_create_foreign_target(&constructor, count)?;
        let (target, target_kind) = if let Some(target) = &foreign_target {
            let target = target
                .object_id()
                .expect("foreign TypedArray construction returns an object");
            let (_, target_kind) = self
                .test262_foreign_typed_array_info(target)?
                .expect("foreign TypedArray construction returns a TypedArray");
            (None, target_kind)
        } else {
            let (target, target_kind) = self.typed_array_create(constructor, count)?;
            (Some(target), target_kind)
        };
        // Species construction can resize the source. Revalidate fixed views
        // before copying; a length-tracking source instead copies its
        // currently available prefix and leaves the already-created target's
        // remaining elements at their initialized zero values.
        let copy_count = if count == 0 {
            0
        } else {
            let (_, current_length, _) = self.typed_array_method_receiver(receiver)?;
            count.min(current_length.saturating_sub(start))
        };
        if copy_count == 0 {
            return Ok(foreign_target.unwrap_or_else(|| {
                Value::Object(target.expect("local TypedArray construction returns an object"))
            }));
        }
        let (source_buffer, source_offset, _, _) = self.heap.typed_array_info(object)?;
        if let Some(foreign_target) = foreign_target {
            if kind == target_kind {
                let target_buffer = self
                    .get_property(&foreign_target, &"buffer".into())?
                    .object_id()
                    .ok_or_else(|| {
                        RuntimeError::TypeError(
                            "foreign TypedArray buffer must be an object".into(),
                        )
                    })?;
                let target_buffer = self
                    .test262_foreign_buffer_clone(target_buffer)?
                    .expect("foreign TypedArray buffer has a foreign backing store");
                let byte_start = source_offset + start * kind.byte_width();
                let byte_length = copy_count * kind.byte_width();
                let bytes = self
                    .heap
                    .array_buffer_copy(source_buffer, byte_start, byte_length)?;
                self.with_roots(|heap| heap.array_buffer_write(target_buffer, 0, &bytes))?;
                let (realm_id, _, _, _) = self
                    .test262_foreign_reference(
                        foreign_target
                            .object_id()
                            .expect("foreign TypedArray construction returns an object"),
                    )
                    .expect("foreign TypedArray construction retains its realm");
                self.test262_sync_foreign_buffer_mirrors(realm_id)?;
            } else {
                let values = self.typed_array_read_values(object, start, copy_count)?;
                let source = self.array_from(values)?;
                let set = self.get_property(&foreign_target, &"set".into())?;
                self.call_native(set, foreign_target.clone(), vec![source], false)?;
            }
            return Ok(foreign_target);
        }
        let target = target.expect("local TypedArray construction returns an object");
        let (target_buffer, target_offset, _, _) = self.heap.typed_array_info(target)?;
        if kind == target_kind && source_buffer != target_buffer {
            // §23.2.3.29 performs a raw byte copy for a same-element-type
            // destination. Going through Number would canonicalize NaN and
            // lose its sign/payload, which is observable through another
            // typed view. A shared backing buffer retains the required
            // forward element-by-element behavior below.
            let byte_start = source_offset + start * kind.byte_width();
            let byte_length = copy_count * kind.byte_width();
            let bytes = self
                .heap
                .array_buffer_copy(source_buffer, byte_start, byte_length)?;
            self.with_roots(|heap| heap.array_buffer_write(target_buffer, target_offset, &bytes))?;
            return Ok(Value::Object(target));
        }
        // Read and write one element at a time. This preserves slice's
        // observable forward byte-copy behavior when a species result shares
        // the source buffer at a different byte offset.
        for index in 0..copy_count {
            let value = self.typed_array_element(object, start + index)?;
            self.typed_array_write_values(target, target_kind, index, &[value])?;
        }
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
        source: Option<ObjectId>,
    ) -> Result<bool, RuntimeError> {
        if *compare != Value::Undefined && !self.is_callable(compare)? {
            return Err(RuntimeError::TypeError(
                "TypedArray sort comparator must be callable".into(),
            ));
        }
        if *compare == Value::Undefined {
            values.sort_by(|left, right| Self::typed_array_default_compare(left, right, kind));
            return Ok(true);
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
                let Some(first_order) = self.typed_array_compare_values(
                    compare,
                    &values[end],
                    &values[end - 1],
                    source,
                )?
                else {
                    return Ok(false);
                };
                let descending = first_order == std::cmp::Ordering::Less;
                end += 1;
                while end < values.len() {
                    let Some(order) = self.typed_array_compare_values(
                        compare,
                        &values[end],
                        &values[end - 1],
                        source,
                    )?
                    else {
                        return Ok(false);
                    };
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
            let mut next_runs = Vec::with_capacity(runs.len().div_ceil(2));
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
                    let Some(order) = self.typed_array_compare_values(
                        compare,
                        &values[right],
                        &values[left],
                        source,
                    )?
                    else {
                        return Ok(false);
                    };
                    if order == std::cmp::Ordering::Less {
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
        Ok(true)
    }

    fn typed_array_compare_values(
        &mut self,
        compare: &Value,
        left: &Value,
        right: &Value,
        source: Option<ObjectId>,
    ) -> Result<Option<std::cmp::Ordering>, RuntimeError> {
        let result = self.call_native(
            compare.clone(),
            Value::Undefined,
            vec![left.clone(), right.clone()],
            false,
        )?;
        let result = self.coerce_number(&result)?;
        if let Some(source) = source {
            let (buffer, _, _, _) = self.heap.typed_array_info(source)?;
            if self.heap.buffer_is_detached(buffer)? {
                return Ok(None);
            }
        }
        Ok(Some(if result.is_nan() || result == 0.0 {
            std::cmp::Ordering::Equal
        } else if result < 0.0 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }))
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
        self.typed_array_sort_values(&mut values, compare, kind, None)?;
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
        // ToIntegerOrInfinity happens before converting `value`; a NaN index
        // is therefore +0, and negative indices remain relative to this
        // operation's initially captured length even if conversion resizes
        // the backing buffer.
        let relative = if index.is_nan() { 0.0 } else { index.trunc() };
        let index = if relative < 0.0 {
            length as f64 + relative
        } else {
            relative
        };
        // TypedArraySetElement performs ToNumber/ToBigInt before its final
        // IsValidIntegerIndex check. A value conversion can grow a resizable
        // buffer and make a formerly out-of-range positive index valid (or
        // shrink one that was initially valid).
        let replacement = self.typed_array_element_value(kind, native::argument(args, 1))?;
        if !index.is_finite()
            || index < 0.0
            || index > usize::MAX as f64
            || self
                .heap
                .typed_array_index_value(object, index as usize)?
                .is_none()
        {
            return Err(RuntimeError::RangeError(
                "TypedArray index is outside its bounds".into(),
            ));
        }
        let values = self.typed_array_read_values(object, 0, length)?;
        let target = self.typed_array_new_same_kind(length, kind)?;
        self.typed_array_write_values(target, kind, 0, &values)?;
        self.typed_array_write_values(target, kind, index as usize, &[replacement])?;
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
                let index = if index.is_nan() {
                    0
                } else if !index.is_finite() {
                    return Ok(Value::Undefined);
                } else if index < 0.0 {
                    let index = length as f64 + index.trunc();
                    if index < 0.0 {
                        return Ok(Value::Undefined);
                    }
                    index as usize
                } else {
                    index.trunc() as usize
                };
                if index >= length {
                    return Ok(Value::Undefined);
                }
                self.typed_array_element(object, index)
            }
            TypedArrayMethod::LastIndexOf => self.typed_array_last_index_of(receiver, args),
            TypedArrayMethod::CopyWithin => {
                let (object, length, kind) = self.typed_array_write_receiver(receiver)?;
                let target = self.relative_buffer_index(native::argument(args, 0), length)?;
                let start = self.relative_buffer_index(native::argument(args, 1), length)?;
                let end = if args.get(2).is_some_and(|value| *value != Value::Undefined) {
                    self.relative_buffer_index(native::argument(args, 2), length)?
                } else {
                    length
                };
                // Coercion can resize the receiver. Fixed views reject an
                // out-of-bounds state here; auto-length views continue with
                // the currently readable/writeable overlap.
                let (_, current_length, _) = self.typed_array_method_receiver(receiver)?;
                let count = end
                    .saturating_sub(start)
                    .min(length.saturating_sub(target))
                    .min(current_length.saturating_sub(target))
                    .min(current_length.saturating_sub(start));
                let values = self.typed_array_read_values(object, start, count)?;
                self.typed_array_write_values(object, kind, target, &values)?;
                Ok(receiver.clone())
            }
            TypedArrayMethod::Fill => {
                let (object, length, kind) = self.typed_array_write_receiver(receiver)?;
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
            TypedArrayMethod::ToLocaleString => {
                // ValidateTypedArray precedes any observable element lookup.
                // The shared array algorithm then forwards both locale
                // arguments to each Number/BigInt element exactly as the
                // TypedArray specification requires.
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError("TypedArray method requires a TypedArray receiver".into())
                })?;
                if !self.heap.is_typed_array(object)?
                    && self.test262_foreign_typed_array_values(object)?.is_none()
                {
                    return Err(RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    ));
                }
                self.array_to_locale_string(receiver, args, true)
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
                let (object, length, kind) = self.typed_array_write_receiver(receiver)?;
                let values = self.typed_array_read_values(object, 0, length)?;
                let reversed: Vec<_> = values.into_iter().rev().collect();
                self.typed_array_write_values(object, kind, 0, &reversed)?;
                Ok(receiver.clone())
            }
            TypedArrayMethod::Slice => self.typed_array_slice(receiver, args),
            TypedArrayMethod::Sort => {
                let (object, length, kind) = self.typed_array_write_receiver(receiver)?;
                let mut values = self.typed_array_read_values(object, 0, length)?;
                if self.typed_array_sort_values(
                    &mut values,
                    native::argument(args, 0),
                    kind,
                    Some(object),
                )? {
                    self.typed_array_write_values(object, kind, 0, &values)?;
                }
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
