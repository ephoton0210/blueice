// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::super::argument_list_storage::ArgumentValues;
use super::super::native_value_storage::NativeValue;
use super::super::realm_call_storage::RealmCallRoots;
use super::super::transport_buffer_storage::TransportVector;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/array_callback_storage.rs"]
mod callback_storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/array_flatten_storage.rs"]
mod flatten_storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/array_copy_storage.rs"]
mod copy_storage_tests;

#[derive(Clone, Copy)]
struct ArrayFlattenFrame {
    source: ObjectId,
    len: u64,
    index: u64,
    depth: f64,
}

#[derive(Clone, Copy)]
enum ArrayCallbackMethod {
    ForEach,
    Filter,
    Map,
    Every,
    Some,
    Find { reverse: bool, return_index: bool },
}

impl Vm {
    fn array_flatten_frames(
        &self,
        source: ObjectId,
        len: u64,
        depth: f64,
    ) -> Result<TransportVector<ArrayFlattenFrame>, RuntimeError> {
        let owner = self.argument_list_owner()?;
        TransportVector::capture(
            std::iter::once(ArrayFlattenFrame {
                source,
                len,
                index: 0,
                depth,
            }),
            owner.as_ref(),
        )
    }

    fn array_callback_arguments(
        &self,
        value: NativeValue,
        index: u64,
        object: ObjectId,
    ) -> Result<TransportVector<Value>, RuntimeError> {
        value.validate_owner()?;
        let owner = self.argument_list_owner()?.or(value.owner_view()?);
        let mut arguments = TransportVector::with_capacity(3, owner.as_ref())?;
        arguments.push_owned(value)?;
        arguments.push_owned(NativeValue::adopt(
            Value::Number(index as f64),
            owner.as_ref(),
        )?)?;
        arguments.push_owned(NativeValue::adopt(Value::Object(object), owner.as_ref())?)?;
        Ok(arguments)
    }

    fn array_callback(
        &mut self,
        callback: &Value,
        this_arg: &Value,
        value: NativeValue,
        index: u64,
        object: ObjectId,
    ) -> Result<NativeValue, RuntimeError> {
        let arguments = self.array_callback_arguments(value, index, object)?;
        let owner = arguments.owner_view()?;
        let value = arguments.with_arguments(self, |vm, arguments| {
            arguments.call_native_borrowed(vm, callback, this_arg)
        })?;
        NativeValue::adopt(value, owner.as_ref())
    }

    /// ECMAScript `IsArray`, including the transparent Proxy recursion.
    pub(in super::super) fn is_array(&self, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(mut object) = value else {
            return Ok(false);
        };
        loop {
            if self.heap.is_array(object)? {
                return Ok(true);
            }
            let Some((target, _)) = self.heap.proxy(object)? else {
                return Ok(false);
            };
            object = target;
        }
    }

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

    /// Whether `Array.prototype.push` may store `count` elements at a real
    /// array's end with the raw element/length stores. `Set(O, key, E, true)`
    /// consults the prototype chain: an inherited setter, a read-only inherited
    /// data property, or a Proxy/exotic prototype (all of which can run user
    /// code or refuse the store) must be observed, so any of them selects the
    /// generic algorithm instead. Without one, nothing observable can happen
    /// between the stores, and the direct path is equivalent.
    pub(in super::super) fn array_push_is_unobservable(
        &mut self,
        object: ObjectId,
        count: usize,
    ) -> bool {
        // The caller has checked that `object` is an Array, so its `length` is
        // a Number, and an object's prototype chain consists of live objects.
        let live = "the receiver and its prototypes are live";
        let length = primitive::number(&self.heap.get(object, "length").expect(live))
            .expect("an Array's length is a Number");
        // Multiple arguments must all be stored before an overflowing length
        // write. A single append preserves that order when it returns the error.
        if count > 1 && length + count as f64 > f64::from(u32::MAX) {
            return false;
        }
        let mut current = self.heap.prototype(object).expect(live);
        while let Some(prototype) = current {
            if !matches!(self.heap.proxy(prototype), Ok(None))
                || self.test262_foreign_reference(prototype).is_some()
                || self.test262_reverse_reference(prototype).is_some()
                || self.heap.is_module_namespace(prototype).expect(live)
            {
                return false;
            }
            for offset in 0..count {
                let key: PropertyName = ((length as u64) + offset as u64).to_string().into();
                if self
                    .heap
                    .typed_array_numeric_key(prototype, &key)
                    .expect(live)
                    .is_some()
                    || self
                        .heap
                        .get_own_property_descriptor(prototype, key)
                        .expect(live)
                        .is_some()
                {
                    return false;
                }
            }
            current = self.heap.prototype(prototype).expect(live);
        }
        true
    }

    /// `Array.prototype.push` for a receiver whose `length` is not a plain
    /// Number (an array-like, a proxy, a TypedArray), or an array whose stores
    /// cannot take the direct path: ToLength(Get(O, "length")), one strict Set
    /// per argument, then a strict Set of the new `length`.
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
        self.array_flatten_method(receiver, depth, None)
    }

    /// `Array.prototype.flatMap`: the mapper's result is flattened one level.
    pub(in super::super) fn array_flat_map(
        &mut self,
        receiver: &Value,
        mapper: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_flatten_method(receiver, &Value::Number(1.0), Some((mapper, this_arg)))
    }

    fn array_flatten_method(
        &mut self,
        receiver: &Value,
        depth: &Value,
        mapper: Option<(&Value, &Value)>,
    ) -> Result<Value, RuntimeError> {
        let owner = self.argument_list_owner()?;
        let roots = RealmCallRoots::new(
            2 + usize::from(depth.object_id().is_some())
                + mapper.map_or(0, |(mapper, receiver)| {
                    usize::from(mapper.object_id().is_some())
                        + usize::from(receiver.object_id().is_some())
                }),
            owner.as_ref(),
        )?;
        if let Some(receiver) = receiver.object_id() {
            roots.push(&self.heap, receiver)?;
        }
        if let Some(depth) = depth.object_id() {
            roots.push(&self.heap, depth)?;
        }
        if let Some((mapper, this_arg)) = mapper {
            if let Some(mapper) = mapper.object_id() {
                roots.push(&self.heap, mapper)?;
            }
            if let Some(receiver) = this_arg.object_id() {
                roots.push(&self.heap, receiver)?;
            }
        }
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let object = vm.coerce_object(receiver)?;
            if receiver.object_id().is_none() {
                roots.push(&vm.heap, object)?;
            }
            let length = vm.get_property(&Value::Object(object), &"length".into())?;
            let length = NativeValue::adopt(length, owner.as_ref())?;
            let count = length.with_value(vm, |vm, value| vm.coerce_length(value))? as u64;
            drop(length);
            let depth = if let Some((mapper, _)) = mapper {
                if !vm.is_callable(mapper)? {
                    return Err(RuntimeError::TypeError(
                        "Array.prototype.flatMap callback must be callable".into(),
                    ));
                }
                1.0
            } else if *depth == Value::Undefined {
                1.0
            } else {
                let number = vm.coerce_number(depth)?;
                let integer = if number.is_nan() { 0.0 } else { number.trunc() };
                integer.max(0.0)
            };
            // A neutral Vm can perform the coercion under a document loan.
            // Validate that same document before creating the species result.
            if let Some(owner) = &owner {
                owner.reserve_peer(0)?;
            }
            let target = vm.array_species_create(object, 0)?;
            roots.push(&vm.heap, target)?;
            vm.array_flatten_into(target, object, count, depth, mapper)?;
            Ok(Value::Object(target))
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    /// Explicit paid frames replace Rust recursion. Only the outermost frame
    /// applies the mapper; active source IDs have a parallel scoped root list.
    fn array_flatten_into(
        &mut self,
        target: ObjectId,
        source: ObjectId,
        source_len: u64,
        depth: f64,
        mapper: Option<(&Value, &Value)>,
    ) -> Result<(), RuntimeError> {
        const MAX_FLATTEN_NESTING: usize = 10_000;
        let owner = self.argument_list_owner()?;
        let root_base = 1 + mapper.map_or(0, |(mapper, receiver)| {
            usize::from(mapper.object_id().is_some()) + usize::from(receiver.object_id().is_some())
        });
        let roots = RealmCallRoots::new(root_base + 1, owner.as_ref())?;
        roots.push(&self.heap, target)?;
        if let Some((mapper, this_arg)) = mapper {
            if let Some(mapper) = mapper.object_id() {
                roots.push(&self.heap, mapper)?;
            }
            if let Some(receiver) = this_arg.object_id() {
                roots.push(&self.heap, receiver)?;
            }
        }
        roots.push(&self.heap, source)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let mut frames = vm.array_flatten_frames(source, source_len, depth)?;
            let mut target_index: u64 = 0;
            loop {
                frames.validate_owner()?;
                let Some(frame) = frames.last_mut() else {
                    break;
                };
                if frame.index >= frame.len {
                    frames.truncate(frames.len() - 1);
                    roots.truncate(root_base + frames.len());
                    continue;
                }
                vm.charge_step()?;
                let (source, index, depth) = (frame.source, frame.index, frame.depth);
                frame.index += 1;
                let key: PropertyName = index.to_string().into();
                let present = vm.has_property(source, &key)?;
                frames.validate_owner()?;
                if !present {
                    continue;
                }
                let element = vm.get_property(&Value::Object(source), &key)?;
                let mut element = NativeValue::adopt(element, owner.as_ref())?;
                if frames.len() == 1 {
                    if let Some((mapper, this_arg)) = mapper {
                        element = vm.array_callback(mapper, this_arg, element, index, source)?;
                    }
                }
                if depth > 0.0 && vm.is_array(element.value())? {
                    element.with_owned(vm, |vm, element| {
                        let length = vm.get_property(element.value(), &"length".into())?;
                        let length = NativeValue::adopt(length, owner.as_ref())?;
                        let count =
                            length.with_value(vm, |vm, value| vm.coerce_length(value))? as u64;
                        drop(length);
                        let inner = element
                            .value()
                            .object_id()
                            .expect("IsArray is only true for objects");
                        if frames.len() >= MAX_FLATTEN_NESTING {
                            return Err(RuntimeError::RangeError(
                                "Array flattening is nested too deeply".into(),
                            ));
                        }
                        // No guest call or safepoint separates frame admission
                        // and publishing its source edge. Failure unwinds both.
                        frames.push(ArrayFlattenFrame {
                            source: inner,
                            len: count,
                            index: 0,
                            depth: depth - 1.0,
                        })?;
                        roots.push_admitted(&vm.heap, inner)?;
                        Ok(())
                    })?;
                } else {
                    vm.array_create_data_property_or_throw_owned(
                        target,
                        target_index.to_string().into(),
                        element,
                    )?;
                    target_index += 1;
                }
            }
            Ok(())
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    pub(in super::super) fn array_join(
        &mut self,
        receiver: &Value,
        separator: &Value,
    ) -> Result<Value, RuntimeError> {
        let owner = self.argument_list_owner()?;
        let roots = RealmCallRoots::new(2, owner.as_ref())?;
        [receiver, separator]
            .into_iter()
            .filter_map(Value::object_id)
            .try_for_each(|id| roots.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let object = vm.coerce_object(receiver)?;
            if receiver.object_id().is_none() {
                roots.push(&vm.heap, object)?;
            }
            let length = vm.get_property(&Value::Object(object), &"length".into())?;
            let length = NativeValue::adopt(length, owner.as_ref())?;
            let count = length.with_value(vm, |vm, value| vm.coerce_length(value))? as u64;
            drop(length);
            let separator = if *separator == Value::Undefined {
                ",".into()
            } else {
                vm.coerce_string(separator)?
            };
            let separator = NativeValue::adopt(Value::String(separator), owner.as_ref())?;
            if vm.joining.contains(&object) {
                return Ok(Value::String(JsString::default()));
            }
            vm.begin_array_join(object)?;
            let result = (|| {
                let mut result = TransportVector::<u16>::with_capacity(0, owner.as_ref())?;
                for index in 0..count {
                    vm.charge_step()?;
                    if index > 0 {
                        let Value::String(separator) = separator.value() else {
                            unreachable!("the join separator was converted to a String")
                        };
                        result.append_string(separator, vm.config.max_string_bytes)?;
                    }
                    let value =
                        vm.get_property(&Value::Object(object), &index.to_string().into())?;
                    let value = NativeValue::adopt(value, owner.as_ref())?;
                    if !matches!(value.value(), Value::Null | Value::Undefined) {
                        let text = value.with_value(vm, |vm, value| vm.coerce_string(value))?;
                        drop(value);
                        let text = NativeValue::adopt(Value::String(text), owner.as_ref())?;
                        let Value::String(text) = text.value() else {
                            unreachable!("the join element was converted to a String")
                        };
                        result.append_string(text, vm.config.max_string_bytes)?;
                    }
                }
                result.into_string()
            })();
            vm.end_array_join(object);
            result
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    pub(in super::super) fn array_concat(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let owner = self.argument_list_owner()?;
        let roots = RealmCallRoots::new(
            2 + args.iter().filter_map(Value::object_id).count(),
            owner.as_ref(),
        )?;
        if let Some(receiver) = receiver.object_id() {
            roots.push(&self.heap, receiver)?;
        }
        args.iter()
            .filter_map(Value::object_id)
            .try_for_each(|id| roots.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let original = vm.coerce_object(receiver)?;
            if receiver.object_id().is_none() {
                roots.push(&vm.heap, original)?;
            }
            let target = vm.array_species_create(original, 0)?;
            roots.push(&vm.heap, target)?;
            let original = Value::Object(original);
            let mut index = 0_u64;
            // All object inputs are already pinned. Borrow primitive inputs
            // until the leaf consumer admits its one required copy.
            for value in std::iter::once(&original).chain(args.iter()) {
                let spreadable = vm.array_is_concat_spreadable(value)?;
                let length = if spreadable {
                    let length = vm.get_property(value, &"length".into())?;
                    let length = NativeValue::adopt(length, owner.as_ref())?;
                    let count = length.with_value(vm, |vm, value| vm.coerce_length(value))? as u64;
                    drop(length);
                    count
                } else {
                    1
                };
                if index
                    .checked_add(length)
                    .is_none_or(|next| next > 9_007_199_254_740_991)
                {
                    return Err(RuntimeError::TypeError(
                        "concatenated Array length exceeds the safe integer limit".into(),
                    ));
                }
                if spreadable {
                    let source = value.object_id().expect("only objects are spreadable");
                    for source_index in 0..length {
                        vm.charge_step()?;
                        let key: PropertyName = source_index.to_string().into();
                        let present = vm.has_property(source, &key)?;
                        if let Some(owner) = &owner {
                            owner.reserve_peer(0)?;
                        }
                        if present {
                            let element = vm.get_property(value, &key)?;
                            let element = NativeValue::adopt(element, owner.as_ref())?;
                            vm.array_create_data_property_or_throw_owned(
                                target,
                                index.to_string().into(),
                                element,
                            )?;
                        }
                        index += 1;
                    }
                } else {
                    let value = NativeValue::copied(value, owner.as_ref())?;
                    vm.array_create_data_property_or_throw_owned(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    index += 1;
                }
            }
            vm.array_set_or_throw(target, "length".into(), &Value::Number(index as f64))?;
            Ok(Value::Object(target))
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    /// The marker getter runs before IsArray. Retain its scalar result and
    /// original owner before inspecting it or entering a length getter.
    fn array_is_concat_spreadable(&mut self, value: &Value) -> Result<bool, RuntimeError> {
        let owner = self.argument_list_owner()?;
        let Value::Object(_) = value else {
            return Ok(false);
        };
        let marker =
            self.get_property(value, &JsSymbol::well_known("isConcatSpreadable").into())?;
        let marker = NativeValue::adopt(marker, owner.as_ref())?;
        if *marker.value() != Value::Undefined {
            return self.to_boolean(marker.value());
        }
        self.is_array(value)
    }

    pub(in super::super) fn array_for_each(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_callback_method(receiver, callback, this_arg, ArrayCallbackMethod::ForEach)
    }

    pub(in super::super) fn array_filter(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_callback_method(receiver, callback, this_arg, ArrayCallbackMethod::Filter)
    }

    pub(in super::super) fn array_map(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_callback_method(receiver, callback, this_arg, ArrayCallbackMethod::Map)
    }

    pub(in super::super) fn array_every(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_callback_method(receiver, callback, this_arg, ArrayCallbackMethod::Every)
    }

    pub(in super::super) fn array_some(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        self.array_callback_method(receiver, callback, this_arg, ArrayCallbackMethod::Some)
    }

    pub(in super::super) fn array_find(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
        reverse: bool,
        return_index: bool,
    ) -> Result<Value, RuntimeError> {
        self.array_callback_method(
            receiver,
            callback,
            this_arg,
            ArrayCallbackMethod::Find {
                reverse,
                return_index,
            },
        )
    }

    fn array_callback_method(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
        method: ArrayCallbackMethod,
    ) -> Result<Value, RuntimeError> {
        let owner = self.argument_list_owner()?;
        let target_needed = matches!(
            method,
            ArrayCallbackMethod::Map | ArrayCallbackMethod::Filter
        );
        // Length conversion and species getters can run guest code before the
        // first callback. Pin all borrowed object inputs before either step.
        let roots = RealmCallRoots::new(
            1 + usize::from(callback.object_id().is_some())
                + usize::from(this_arg.object_id().is_some())
                + usize::from(target_needed),
            owner.as_ref(),
        )?;
        if let Some(object) = receiver.object_id() {
            roots.push(&self.heap, object)?;
        }
        if let Some(callback) = callback.object_id() {
            roots.push(&self.heap, callback)?;
        }
        if let Some(receiver) = this_arg.object_id() {
            roots.push(&self.heap, receiver)?;
        }
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let object = vm.coerce_object(receiver)?;
            if receiver.object_id().is_none() {
                roots.push(&vm.heap, object)?;
            }
            let length = vm.get_property(&Value::Object(object), &"length".into())?;
            let length = NativeValue::adopt(length, owner.as_ref())?;
            let count = length.with_value(vm, |vm, value| vm.coerce_length(value))? as u64;
            drop(length);
            if !vm.is_callable(callback)? {
                let message = match method {
                    ArrayCallbackMethod::ForEach => {
                        "Array.prototype.forEach callback must be callable"
                    }
                    ArrayCallbackMethod::Filter => {
                        "Array.prototype.filter callback must be callable"
                    }
                    ArrayCallbackMethod::Map => "Array.prototype.map callback must be callable",
                    ArrayCallbackMethod::Every | ArrayCallbackMethod::Some => {
                        "Array predicate callback must be callable"
                    }
                    ArrayCallbackMethod::Find { .. } => {
                        "Array.prototype.find callback must be callable"
                    }
                };
                return Err(RuntimeError::TypeError(message.into()));
            }
            let result_value = |value| NativeValue::adopt(value, owner.as_ref());
            if let ArrayCallbackMethod::Find {
                reverse,
                return_index,
            } = method
            {
                // Find visits holes through Get. Direction arithmetic needs no
                // boxed iterator or extra unowned native control allocation.
                for step in 0..count {
                    let index = if reverse { count - 1 - step } else { step };
                    vm.charge_step()?;
                    let value =
                        vm.get_property(&Value::Object(object), &index.to_string().into())?;
                    let value = NativeValue::adopt(value, owner.as_ref())?;
                    if return_index {
                        let selected =
                            vm.array_callback(callback, this_arg, value, index, object)?;
                        if vm.to_boolean(selected.value())? {
                            return result_value(Value::Number(index as f64));
                        }
                    } else {
                        let (selected, value) = value.with_owned(vm, |vm, value| {
                            // Find returns the original Get result. Its copy
                            // enters the predicate under a separate paid lease.
                            let argument = NativeValue::copied(value.value(), owner.as_ref())?;
                            let selected =
                                vm.array_callback(callback, this_arg, argument, index, object)?;
                            Ok((vm.to_boolean(selected.value())?, value))
                        })?;
                        if selected {
                            return Ok(value);
                        }
                    }
                }
                return result_value(if return_index {
                    Value::Number(-1.0)
                } else {
                    Value::Undefined
                });
            }
            let target = if target_needed {
                let length = if matches!(method, ArrayCallbackMethod::Map) {
                    count as usize
                } else {
                    0
                };
                let target = vm.array_species_create(object, length)?;
                roots.push(&vm.heap, target)?;
                Some(target)
            } else {
                None
            };
            let mut scan = vm.index_scan(object, count)?;
            let mut from = 0;
            let mut target_index = 0usize;
            while let Some((index, value)) = vm.array_next_present(&mut scan, object, from)? {
                let value = NativeValue::adopt(value, owner.as_ref())?;
                match method {
                    ArrayCallbackMethod::ForEach => {
                        vm.array_callback(callback, this_arg, value, index, object)?;
                    }
                    ArrayCallbackMethod::Every | ArrayCallbackMethod::Some => {
                        let some = matches!(method, ArrayCallbackMethod::Some);
                        let selected =
                            vm.array_callback(callback, this_arg, value, index, object)?;
                        if vm.to_boolean(selected.value())? == some {
                            return result_value(Value::Bool(some));
                        }
                    }
                    ArrayCallbackMethod::Map => {
                        let mapped = vm.array_callback(callback, this_arg, value, index, object)?;
                        vm.array_create_data_property_or_throw_owned(
                            target.unwrap(),
                            index.to_string().into(),
                            mapped,
                        )?;
                    }
                    ArrayCallbackMethod::Filter => {
                        let selected = value.with_owned(vm, |vm, value| {
                            let argument = NativeValue::copied(value.value(), owner.as_ref())?;
                            let selected =
                                vm.array_callback(callback, this_arg, argument, index, object)?;
                            let keep = vm.to_boolean(selected.value())?;
                            drop(selected);
                            if keep {
                                vm.array_create_data_property_or_throw_owned(
                                    target.unwrap(),
                                    target_index.to_string().into(),
                                    value,
                                )?;
                            }
                            Ok(keep)
                        })?;
                        if selected {
                            target_index += 1;
                        }
                    }
                    ArrayCallbackMethod::Find { .. } => unreachable!(),
                }
                from = index + 1;
            }
            result_value(match method {
                ArrayCallbackMethod::ForEach => Value::Undefined,
                ArrayCallbackMethod::Every => Value::Bool(true),
                ArrayCallbackMethod::Some => Value::Bool(false),
                ArrayCallbackMethod::Filter | ArrayCallbackMethod::Map => {
                    Value::Object(target.unwrap())
                }
                ArrayCallbackMethod::Find { .. } => unreachable!(),
            })
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result.map(NativeValue::into_value)
    }

    pub(in super::super) fn array_reduce(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.array_reduce_direction(receiver, args, false)
    }

    pub(in super::super) fn array_reduce_right(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.array_reduce_direction(receiver, args, true)
    }

    fn array_reduce_direction(
        &mut self,
        receiver: &Value,
        args: &[Value],
        reverse: bool,
    ) -> Result<Value, RuntimeError> {
        let callback = native::argument(args, 0);
        let owner = self.argument_list_owner()?;
        let roots = RealmCallRoots::new(3, owner.as_ref())?;
        if let Some(callback) = callback.object_id() {
            roots.push(&self.heap, callback)?;
        }
        if let Some(initial) = args.get(1).and_then(Value::object_id) {
            roots.push(&self.heap, initial)?;
        }
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let object = vm.coerce_object(receiver)?;
            roots.push(&vm.heap, object)?;
            let length = vm.array_like_length(object)?;
            if !vm.is_callable(callback)? {
                return Err(RuntimeError::TypeError(
                    if reverse {
                        "Array.prototype.reduceRight callback must be callable"
                    } else {
                        "Array.prototype.reduce callback must be callable"
                    }
                    .into(),
                ));
            }
            let mut scan = vm.index_scan(object, length)?;
            let mut boundary = if reverse { length } else { 0 };
            let mut accumulator = if let Some(initial) = args.get(1) {
                NativeValue::copied(initial, owner.as_ref())?
            } else {
                let first = if reverse {
                    vm.array_previous_present(&mut scan, object, boundary)?
                } else {
                    vm.array_next_present(&mut scan, object, boundary)?
                };
                let Some((index, value)) = first else {
                    return Err(RuntimeError::TypeError(
                        if reverse {
                            "reduceRight of empty array with no initial value"
                        } else {
                            "reduce of empty array with no initial value"
                        }
                        .into(),
                    ));
                };
                boundary = if reverse { index } else { index + 1 };
                NativeValue::adopt(value, owner.as_ref())?
            };
            while if reverse {
                boundary > 0
            } else {
                boundary < length
            } {
                // A getter can collect before the accumulator moves into the
                // callback arguments. Root only its object ID through both.
                let (next_boundary, next) = accumulator.with_owned(vm, |vm, accumulator| {
                    let next = if reverse {
                        vm.array_previous_present(&mut scan, object, boundary)?
                    } else {
                        vm.array_next_present(&mut scan, object, boundary)?
                    };
                    let Some((index, value)) = next else {
                        return Ok((None, accumulator));
                    };
                    let value = NativeValue::adopt(value, owner.as_ref())?;
                    let next =
                        vm.call_reducer(callback, accumulator, value, index as f64, object)?;
                    Ok((Some(if reverse { index } else { index + 1 }), next))
                })?;
                accumulator = next;
                match next_boundary {
                    Some(next) => boundary = next,
                    None => break,
                }
            }
            Ok(accumulator)
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result.map(NativeValue::into_value)
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
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.array_like_length(object)?;
            if length == 0 {
                return Ok(Value::Bool(false));
            }
            let start = self.array_start_index(from_index, length as i64)? as u64;
            let mut scan = self.index_scan(object, length)?;
            let mut from = start;
            loop {
                let next = self.scan_next(&mut scan, object, from)?;
                // Indices the scan skips are absent everywhere on the chain,
                // and `Get` reads an absent index as `undefined`.
                if *search == Value::Undefined && next.map_or(from < length, |next| next > from) {
                    return Ok(Value::Bool(true));
                }
                let Some(index) = next else {
                    return Ok(Value::Bool(false));
                };
                self.charge_step()?;
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                if same_value_zero(&value, search) {
                    return Ok(Value::Bool(true));
                }
                from = index + 1;
            }
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_index_of(
        &mut self,
        receiver: &Value,
        search: &Value,
        from_index: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.array_like_length(object)?;
            if length == 0 {
                return Ok(Value::Number(-1.0));
            }
            let start = self.array_start_index(from_index, length as i64)? as u64;
            let mut scan = self.index_scan(object, length)?;
            let mut from = start;
            while let Some((index, value)) = self.array_next_present(&mut scan, object, from)? {
                if value == *search {
                    return Ok(Value::Number(index as f64));
                }
                from = index + 1;
            }
            Ok(Value::Number(-1.0))
        })();
        self.stack.truncate(base);
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
            // With nothing to insert no element moves, however long the receiver is.
            let moved = if items.is_empty() { 0 } else { length };
            for index in (0..moved).rev() {
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
                if lower_value.is_none() && upper_value.is_none() {
                    continue;
                }
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
            Ok(Value::Object(object))
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
            let live = "the receiver is live";
            let foreign_typed_values = if self.heap.is_typed_array(object).expect(live) {
                None
            } else {
                self.test262_foreign_typed_array_values(object)?
            };
            let length = if typed_array_method && self.heap.is_typed_array(object).expect(live) {
                let (_, _, length, _) = self.typed_array_receiver(&Value::Object(object))?;
                length as u64
            } else if let Some(values) = &foreign_typed_values {
                values.len() as u64
            } else {
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                self.coerce_length(&length)? as u64
            };
            let result = (|| {
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
                    if !self
                        .is_callable(&method)
                        .expect("Get returned a live local value or an imported facade")
                    {
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
            if let Some(values) = &foreign_typed_values {
                values.validate_owner()?;
            }
            result
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
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.array_like_length(object)?;
            if length == 0 {
                return Ok(Value::Number(-1.0));
            }
            let length = length.min(i64::MAX as u64) as i64;
            let index = if args.len() < 2 {
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
            if index < 0 {
                return Ok(Value::Number(-1.0));
            }
            let mut scan = self.index_scan(object, length as u64)?;
            let mut end = index as u64 + 1;
            while let Some((index, value)) = self.array_previous_present(&mut scan, object, end)? {
                if value == *search {
                    return Ok(Value::Number(index as f64));
                }
                end = index;
            }
            Ok(Value::Number(-1.0))
        })();
        self.stack.truncate(base);
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
        let owner = self.argument_list_owner()?;
        let inputs = RealmCallRoots::new(2, owner.as_ref())?;
        [receiver, compare]
            .into_iter()
            .filter_map(Value::object_id)
            .try_for_each(|id| inputs.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(inputs.clone(), |vm| {
            if *compare != Value::Undefined && !vm.is_callable(compare)? {
                return Err(RuntimeError::TypeError(
                    "Array sort comparator must be callable".into(),
                ));
            }
            let object = vm.coerce_object(receiver)?;
            if receiver.object_id().is_none() {
                inputs.push(&vm.heap, object)?;
            }
            let length = vm.get_property(&Value::Object(object), &"length".into())?;
            let length = NativeValue::adopt(length, owner.as_ref())?;
            let count = length.with_value(vm, |vm, value| vm.coerce_length(value))? as usize;
            drop(length);
            let collected = RealmCallRoots::new(0, owner.as_ref())?;
            vm.with_realm_call_roots(collected.clone(), |vm| {
                let mut values = vm.array_sort_input()?;
                let mut undefined = 0usize;
                for index in 0..count {
                    vm.charge_step()?;
                    let key = Self::array_sort_key(index, owner.as_ref())?;
                    let present = vm.has_property(object, &key.name)?;
                    if let Some(owner) = &owner {
                        owner.reserve_peer(0)?;
                    }
                    if present {
                        let value = vm.get_property(&Value::Object(object), &key.name)?;
                        if let Some(owner) = &owner {
                            owner.reserve_peer(0)?;
                        }
                        if value == Value::Undefined {
                            undefined += 1;
                        } else {
                            if let Some(object) = value.object_id() {
                                collected.push_admitted(&vm.heap, object)?;
                            }
                            vm.push_array_sort_input(&mut values, value)?;
                        }
                    }
                }
                let values = vm.array_sort_values(values, compare)?;
                // Rebuild in reverse write order so each completed consumer
                // can retire its own edge without retaining discarded values.
                collected.truncate(0);
                let mut remaining = 0usize;
                for object in values.iter().rev().filter_map(Value::object_id) {
                    collected.push(&vm.heap, object)?;
                    remaining += 1;
                }
                let mut values = values.into_owned_iter()?;
                let mut index = 0usize;
                while let Some(value) = values.next_owned()? {
                    let object_value = value.value().object_id().is_some();
                    let key = Self::array_sort_key(index, owner.as_ref())?;
                    value.with_value(vm, |vm, value| {
                        vm.array_set_or_throw(object, key.name, value)
                    })?;
                    if let Some(owner) = &owner {
                        owner.reserve_peer(0)?;
                    }
                    if object_value {
                        remaining -= 1;
                        collected.truncate(remaining);
                    }
                    index += 1;
                }
                drop(values);
                for _ in 0..undefined {
                    let key = Self::array_sort_key(index, owner.as_ref())?;
                    vm.array_set_or_throw(object, key.name, &Value::Undefined)?;
                    if let Some(owner) = &owner {
                        owner.reserve_peer(0)?;
                    }
                    index += 1;
                }
                while index < count {
                    let key = Self::array_sort_key(index, owner.as_ref())?;
                    vm.object_delete(object, &key.name)?;
                    if let Some(owner) = &owner {
                        owner.reserve_peer(0)?;
                    }
                    index += 1;
                }
                Ok(Value::Object(object))
            })
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
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
            let prototype = vm.array_create_prototype()?;
            vm.with_roots(|heap| heap.alloc_array(length, Some(prototype)))
        };
        if !self.is_array(&Value::Object(original))? {
            return ordinary_array(self);
        }
        let owner = self.argument_list_owner()?;
        let constructor = self.get_property(&Value::Object(original), &"constructor".into())?;
        let constructor = NativeValue::adopt(constructor, owner.as_ref())?;
        constructor.with_value(self, |vm, constructor| {
            if *constructor == Value::Undefined {
                return ordinary_array(vm);
            }
            if let Some(constructor) = constructor.object_id() {
                if vm.test262_foreign_intrinsic_constructor(constructor, "Array")? {
                    return ordinary_array(vm);
                }
            }
            if constructor.object_id().is_none() {
                return Err(RuntimeError::TypeError(
                    "Array constructor must be an object".into(),
                ));
            }
            let species = vm.get_property(constructor, &JsSymbol::well_known("species").into())?;
            let species = NativeValue::adopt(species, owner.as_ref())?;
            species.with_value(vm, |vm, species| {
                if matches!(species, Value::Undefined | Value::Null) {
                    return ordinary_array(vm);
                }
                if !vm.is_constructor(species)? {
                    return Err(RuntimeError::TypeError(
                        "Array species must be a constructor".into(),
                    ));
                }
                let arguments =
                    ArgumentValues::single(vm, Value::Number(length as f64), owner.as_ref())?;
                Ok(arguments
                    .call(vm, species.clone(), Value::Undefined, true, species.clone())?
                    .object_id()
                    .expect("Construct returns an object"))
            })
        })
    }

    pub(super) fn array_create_data_property_or_throw_owned(
        &mut self,
        object: ObjectId,
        key: PropertyName,
        value: NativeValue,
    ) -> Result<(), RuntimeError> {
        if value.into_property_definition(key).define(self, object)? {
            Ok(())
        } else {
            Err(RuntimeError::TypeError(
                "cannot create Array property".into(),
            ))
        }
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
        let owner = self.argument_list_owner()?;
        let roots = RealmCallRoots::new(
            2 + args.iter().take(2).filter_map(Value::object_id).count(),
            owner.as_ref(),
        )?;
        if let Some(receiver) = receiver.object_id() {
            roots.push(&self.heap, receiver)?;
        }
        args.iter()
            .take(2)
            .filter_map(Value::object_id)
            .try_for_each(|id| roots.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let object = vm.coerce_object(receiver)?;
            if receiver.object_id().is_none() {
                roots.push(&vm.heap, object)?;
            }
            let length = vm.get_property(&Value::Object(object), &"length".into())?;
            let length = NativeValue::adopt(length, owner.as_ref())?;
            let count = length.with_value(vm, |vm, value| vm.coerce_length(value))? as i64;
            drop(length);
            let start = vm.array_start_index(native::argument(args, 0), count)?;
            if let Some(owner) = &owner {
                owner.reserve_peer(0)?;
            }
            let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
                let end = vm.array_start_index(native::argument(args, 1), count)?;
                if let Some(owner) = &owner {
                    owner.reserve_peer(0)?;
                }
                end
            } else {
                count
            };
            let count = end.saturating_sub(start).max(0) as usize;
            let target = vm.array_species_create(object, count)?;
            roots.push(&vm.heap, target)?;
            for (result_index, index) in (start..end.max(start)).enumerate() {
                vm.charge_step()?;
                let key: PropertyName = (index as u64).to_string().into();
                let present = vm.has_property(object, &key)?;
                if let Some(owner) = &owner {
                    owner.reserve_peer(0)?;
                }
                if present {
                    let value = vm.get_property(&Value::Object(object), &key)?;
                    let value = NativeValue::adopt(value, owner.as_ref())?;
                    vm.array_create_data_property_or_throw_owned(
                        target,
                        result_index.to_string().into(),
                        value,
                    )?;
                }
            }
            vm.array_set_or_throw(target, "length".into(), &Value::Number(count as f64))?;
            Ok(Value::Object(target))
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
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
            let delete_count = if args.is_empty() {
                0
            } else if args.len() < 2 {
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
