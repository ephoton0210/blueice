// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(super) fn buffer_prototype(&mut self, constructor: &str) -> Result<ObjectId, RuntimeError> {
        let constructor = self.global(constructor)?;
        self.get_property(&constructor, &"prototype".into())?
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("buffer prototype is unavailable".into()))
    }

    /// OrdinaryCreateFromConstructor for the concrete non-shared binary
    /// constructors. The intrinsic prototype remains the fallback when a
    /// custom `newTarget.prototype` is not an object.
    pub(super) fn constructed_buffer_prototype(
        &mut self,
        constructor: &str,
    ) -> Result<ObjectId, RuntimeError> {
        let default = self.buffer_prototype(constructor)?;
        self.constructor_prototype(default)
    }

    /// Lazily creates the non-global `%TypedArray%` constructor and its shared
    /// prototype. Concrete typed-array constructors inherit from this function
    /// and their per-kind prototypes inherit from this object, which is
    /// observable through `Object.getPrototypeOf(Int8Array)`.
    pub(super) fn typed_array_intrinsics(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let Some(intrinsics) = self.typed_array_intrinsics {
            return Ok(intrinsics);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.function_prototype()?;
        let typed_prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(typed_prototype)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(typed_prototype));
        let result = (|| {
            let constructor = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::TypedArrayIntrinsic,
                    "TypedArray",
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(constructor));
            self.define_data(
                constructor,
                "name",
                Value::String("TypedArray".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(0.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(typed_prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                typed_prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            for (name, native) in [
                ("buffer", NativeFunction::TypedArrayBuffer),
                ("byteLength", NativeFunction::TypedArrayByteLength),
                ("byteOffset", NativeFunction::TypedArrayByteOffset),
                ("length", NativeFunction::TypedArrayLength),
            ] {
                self.install_native_getter(typed_prototype, function_prototype, name, native)?;
            }
            self.install_native(
                typed_prototype,
                function_prototype,
                "set",
                1,
                NativeFunction::TypedArraySet,
            )?;
            self.install_native(
                typed_prototype,
                function_prototype,
                "subarray",
                2,
                NativeFunction::TypedArraySubarray,
            )?;
            Ok(constructor)
        })();
        self.stack.truncate(base);
        match result {
            Ok(constructor) => {
                let intrinsics = (constructor, typed_prototype);
                self.typed_array_intrinsics = Some(intrinsics);
                Ok(intrinsics)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    pub(super) fn buffer_index(&mut self, value: &Value) -> Result<usize, RuntimeError> {
        let number = self.coerce_number(value)?;
        if number.is_nan() || number == 0.0 {
            return Ok(0);
        }
        if !number.is_finite() {
            return Err(RuntimeError::RangeError("invalid buffer index".into()));
        }
        let integer = number.trunc();
        // ToIndex applies ToIntegerOrInfinity before rejecting negatives, so
        // a finite value in (-1, 0) becomes -0 and is accepted as zero.
        if integer < 0.0 {
            return Err(RuntimeError::RangeError("invalid buffer index".into()));
        }
        if integer > usize::MAX as f64 {
            return Err(RuntimeError::RangeError("buffer index is too large".into()));
        }
        Ok(integer as usize)
    }

    /// Integer-indexed writes use ToNumber for numeric typed arrays and
    /// ToBigInt for the two BigInt element kinds. Both start with the same
    /// observable ToPrimitive(value, number) step.
    pub(super) fn typed_array_element_value(
        &mut self,
        kind: TypedArrayKind,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let value = self.coerce_primitive(value, "number")?;
        if kind.bigint() {
            return match value {
                Value::BigInt(_) => Ok(value),
                _ => Err(RuntimeError::TypeError(
                    "BigInt typed arrays require a BigInt element value".into(),
                )),
            };
        }
        Ok(Value::Number(primitive::number(&value)?))
    }

    pub(super) fn array_buffer_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer constructor requires 'new'".into(),
            ));
        }
        let length = if args.is_empty() {
            0
        } else {
            self.buffer_index(native::argument(args, 0))?
        };
        let prototype = self.constructed_buffer_prototype("ArrayBuffer")?;
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_array_buffer(length, Some(prototype))
        })?))
    }

    pub(super) fn array_buffer_receiver(&self, receiver: &Value) -> Result<ObjectId, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("ArrayBuffer method requires an ArrayBuffer receiver".into())
        })?;
        if !self.heap.is_array_buffer(object)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer method requires an ArrayBuffer receiver".into(),
            ));
        }
        Ok(object)
    }

    pub(super) fn array_buffer_species_constructor(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let constructor = self.get_property(receiver, &"constructor".into())?;
        if constructor == Value::Undefined {
            return self.global("ArrayBuffer");
        }
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer constructor must be an object".into(),
            ));
        }
        let species = self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
        if matches!(species, Value::Undefined | Value::Null) {
            return self.global("ArrayBuffer");
        }
        if !self.is_constructor(&species)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer species must be a constructor".into(),
            ));
        }
        Ok(species)
    }

    pub(super) fn array_buffer_slice(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let buffer = self.array_buffer_receiver(receiver)?;
        let length = self.heap.array_buffer_byte_length(buffer)?;
        let start = self.relative_buffer_index(native::argument(args, 0), length)?;
        let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let width = end.saturating_sub(start);
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        let constructor = self.array_buffer_species_constructor(receiver)?;
        let result = self.call_with_target(
            constructor.clone(),
            Value::Undefined,
            vec![Value::Number(width as f64)],
            true,
            constructor,
        )?;
        let result_buffer = self.array_buffer_receiver(&result)?;
        if result_buffer == buffer {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer species returned the source buffer".into(),
            ));
        }
        if self.heap.array_buffer_is_detached(buffer)?
            || self.heap.array_buffer_is_detached(result_buffer)?
        {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        if self.heap.array_buffer_byte_length(result_buffer)? < width {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer species result is too small".into(),
            ));
        }
        let bytes = self.heap.array_buffer_copy(buffer, start, width)?;
        self.with_roots(|heap| heap.array_buffer_write(result_buffer, 0, &bytes))?;
        Ok(result)
    }

    pub(super) fn relative_buffer_index(
        &mut self,
        value: &Value,
        length: usize,
    ) -> Result<usize, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(0);
        }
        let number = self.coerce_number(value)?;
        if number.is_nan() {
            return Ok(0);
        }
        if number == f64::INFINITY {
            return Ok(length);
        }
        if number == f64::NEG_INFINITY {
            return Ok(0);
        }
        let integer = number.trunc();
        if integer < 0.0 {
            Ok(length.saturating_sub((-integer) as usize))
        } else {
            Ok((integer as usize).min(length))
        }
    }

    pub(super) fn data_view_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "DataView constructor requires 'new'".into(),
            ));
        }
        let buffer = native::argument(args, 0).object_id().ok_or_else(|| {
            RuntimeError::TypeError("DataView buffer must be an ArrayBuffer".into())
        })?;
        if !self.heap.is_array_buffer(buffer)? {
            return Err(RuntimeError::TypeError(
                "DataView buffer must be an ArrayBuffer".into(),
            ));
        }
        // ToIndex(byteOffset) is observable and precedes the detached-buffer
        // check. A valueOf hook can therefore run even for a detached buffer.
        let offset = if args.len() > 1 {
            self.buffer_index(native::argument(args, 1))?
        } else {
            0
        };
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "DataView buffer is detached".into(),
            ));
        }
        let total = self.heap.array_buffer_byte_length(buffer)?;
        if offset > total {
            return Err(RuntimeError::RangeError(
                "DataView offset is outside its buffer".into(),
            ));
        }
        let length = if args.len() > 2 && native::argument(args, 2) != &Value::Undefined {
            self.buffer_index(native::argument(args, 2))?
        } else {
            total - offset
        };
        let prototype = self.constructed_buffer_prototype("DataView")?;
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_data_view(buffer, offset, length, Some(prototype))
        })?))
    }

    pub(super) fn data_view_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, usize), RuntimeError> {
        let (buffer, offset, length) = self.data_view_raw_receiver(receiver)?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer has been detached".into(),
            ));
        }
        Ok((buffer, offset, length))
    }

    pub(super) fn data_view_raw_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, usize), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("DataView method requires a DataView receiver".into())
        })?;
        self.heap
            .data_view_raw_info(object)
            .map_err(|error| match error {
                HeapError::InvalidObject(_) | HeapError::InvalidInternalSlot(_) => {
                    RuntimeError::TypeError("DataView method requires a DataView receiver".into())
                }
                error => error.into(),
            })
    }

    pub(super) fn data_view_get(
        &mut self,
        receiver: &Value,
        args: &[Value],
        width: usize,
        signed: bool,
        floating: bool,
        bigint: bool,
    ) -> Result<Value, RuntimeError> {
        let (buffer, offset, length) = self.data_view_raw_receiver(receiver)?;
        let index = self.buffer_index(native::argument(args, 0))?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer has been detached".into(),
            ));
        }
        let end = index.checked_add(width).ok_or_else(|| {
            RuntimeError::RangeError("DataView access is outside its view".into())
        })?;
        if end > length {
            return Err(RuntimeError::RangeError(
                "DataView access is outside its view".into(),
            ));
        }
        let little_endian = match args.get(1) {
            Some(value) => self.to_boolean(value)?,
            None => false,
        };
        let bytes = self.heap.array_buffer_copy(buffer, offset + index, width)?;
        Ok(data_view_value(
            &bytes,
            signed,
            floating,
            little_endian,
            bigint,
        ))
    }

    pub(super) fn data_view_set(
        &mut self,
        receiver: &Value,
        args: &[Value],
        width: usize,
        signed: bool,
        floating: bool,
        bigint: bool,
    ) -> Result<Value, RuntimeError> {
        let (buffer, offset, length) = self.data_view_raw_receiver(receiver)?;
        let index = self.buffer_index(native::argument(args, 0))?;
        // SetViewValue converts its value before observing detachment or an
        // out-of-range index. This matters when valueOf throws or detaches.
        let kind = if bigint {
            if signed {
                TypedArrayKind::BigInt64
            } else {
                TypedArrayKind::BigUint64
            }
        } else {
            TypedArrayKind::Float64
        };
        let value = self.typed_array_element_value(kind, native::argument(args, 1))?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer has been detached".into(),
            ));
        }
        let end = index.checked_add(width).ok_or_else(|| {
            RuntimeError::RangeError("DataView access is outside its view".into())
        })?;
        if end > length {
            return Err(RuntimeError::RangeError(
                "DataView access is outside its view".into(),
            ));
        }
        let little_endian = match args.get(2) {
            Some(value) => self.to_boolean(value)?,
            None => false,
        };
        let bytes = data_view_bytes(&value, width, signed, floating, little_endian, bigint);
        self.with_roots(|heap| heap.array_buffer_write(buffer, offset + index, &bytes))?;
        Ok(Value::Undefined)
    }

    pub(super) fn typed_array_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
        kind: TypedArrayKind,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "TypedArray constructor requires 'new'".into(),
            ));
        }
        let input = native::argument(args, 0);
        let (buffer, byte_offset, length, initial_values) = if let Value::Object(buffer) = input {
            if self.heap.is_array_buffer(*buffer)? {
                if self.heap.array_buffer_is_detached(*buffer)? {
                    return Err(RuntimeError::TypeError(
                        "TypedArray buffer is detached".into(),
                    ));
                }
                let bytes = self.heap.array_buffer_byte_length(*buffer)?;
                let offset = if args.len() > 1 {
                    self.buffer_index(native::argument(args, 1))?
                } else {
                    0
                };
                if offset % kind.byte_width() != 0 || offset > bytes {
                    return Err(RuntimeError::RangeError(
                        "invalid TypedArray byte offset".into(),
                    ));
                }
                let length = if args.len() > 2 && native::argument(args, 2) != &Value::Undefined {
                    self.buffer_index(native::argument(args, 2))?
                } else {
                    let remaining = bytes - offset;
                    if remaining % kind.byte_width() != 0 {
                        return Err(RuntimeError::RangeError(
                            "invalid TypedArray buffer length".into(),
                        ));
                    }
                    remaining / kind.byte_width()
                };
                (*buffer, offset, length, None)
            } else if self.heap.is_typed_array(*buffer)? {
                let (source_buffer, _, source_length, _) = self.heap.typed_array_info(*buffer)?;
                if self.heap.array_buffer_is_detached(source_buffer)? {
                    return Err(RuntimeError::TypeError(
                        "TypedArray source is detached".into(),
                    ));
                }
                let values = self.typed_array_values(*buffer, source_length)?;
                let result = self.new_typed_array_buffer(source_length, kind)?;
                (result, 0, source_length, Some(values))
            } else {
                let source = Value::Object(*buffer);
                let base = self.stack.len();
                self.stack.push(source.clone());
                let values = (|| {
                    let iterator =
                        self.get_method(&source, &JsSymbol::well_known("iterator").into())?;
                    if iterator == Value::Undefined {
                        self.array_like_numbers(*buffer, kind)
                    } else {
                        self.iterable_numbers(&source, iterator, kind)
                    }
                })();
                self.stack.truncate(base);
                let values = values?;
                let length = values.len();
                let result = self.new_typed_array_buffer(length, kind)?;
                (result, 0, length, Some(values))
            }
        } else {
            let length = if *input == Value::Undefined {
                0
            } else {
                self.buffer_index(input)?
            };
            let buffer = self.new_typed_array_buffer(length, kind)?;
            (buffer, 0, length, None)
        };
        let base = self.stack.len();
        self.stack.push(Value::Object(buffer));
        let result = (|| {
            let prototype = self.constructed_buffer_prototype(kind.name())?;
            let object = self.with_roots(|heap| {
                heap.alloc_typed_array(buffer, byte_offset, length, kind, Some(prototype))
            })?;
            if let Some(values) = initial_values {
                self.stack.push(Value::Object(object));
                let writes = values
                    .into_iter()
                    .enumerate()
                    .try_for_each(|(index, value)| {
                        let value = self.typed_array_element_value(kind, &value)?;
                        self.with_roots(|heap| heap.typed_array_set_index(object, index, &value))
                            .map(|_| ())
                    });
                self.stack.pop();
                writes?;
            }
            Ok(Value::Object(object))
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn new_typed_array_buffer(
        &mut self,
        length: usize,
        kind: TypedArrayKind,
    ) -> Result<ObjectId, RuntimeError> {
        let bytes = length
            .checked_mul(kind.byte_width())
            .ok_or_else(|| RuntimeError::RangeError("TypedArray length is too large".into()))?;
        if bytes > self.heap.max_array_buffer_byte_length() {
            return Err(RuntimeError::RangeError(
                "TypedArray length is too large".into(),
            ));
        }
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        self.with_roots(|heap| heap.alloc_array_buffer(bytes, Some(prototype)))
    }

    pub(super) fn typed_array_values(
        &mut self,
        source: ObjectId,
        length: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.stack.push(Value::Object(source));
        let result = (|| {
            let mut values = Vec::with_capacity(length);
            for index in 0..length {
                let value = self.get_property(&Value::Object(source), &index.to_string().into())?;
                values.push(value);
            }
            Ok(values)
        })();
        self.stack.pop();
        result
    }

    pub(super) fn array_like_numbers(
        &mut self,
        source: ObjectId,
        kind: TypedArrayKind,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.stack.push(Value::Object(source));
        let result = (|| {
            let length = self.get_property(&Value::Object(source), &"length".into())?;
            let length = self.coerce_length(&length)?;
            if length > (self.heap.max_array_buffer_byte_length() / kind.byte_width()) as f64 {
                return Err(RuntimeError::RangeError(
                    "TypedArray length is too large".into(),
                ));
            }
            let mut values = Vec::with_capacity(length as usize);
            for index in 0..length as usize {
                let value = self.get_property(&Value::Object(source), &index.to_string().into())?;
                values.push(self.typed_array_element_value(kind, &value)?);
            }
            Ok(values)
        })();
        self.stack.pop();
        result
    }

    /// Collect an iterable constructor source before allocating the new view.
    /// The source and iterator record stay on the VM stack for every user-code
    /// call, so a collection triggered by a getter, `next`, or number coercion
    /// cannot reclaim either internal object.
    pub(super) fn iterable_numbers(
        &mut self,
        source: &Value,
        iterator_method: Value,
        kind: TypedArrayKind,
    ) -> Result<Vec<Value>, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(source.clone());
        let result = (|| {
            let record = self.get_iterator_from_method(source, iterator_method)?;
            self.stack.push(record.clone());
            let maximum = self.heap.max_array_buffer_byte_length() / kind.byte_width();
            let mut values = Vec::new();
            while let Some(value) = self.iterator_step(&record, true)? {
                if values.len() == maximum {
                    return Err(RuntimeError::RangeError(
                        "TypedArray length is too large".into(),
                    ));
                }
                values.push(self.typed_array_element_value(kind, &value)?);
            }
            Ok(values)
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn typed_array_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, usize, TypedArrayKind), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("TypedArray method requires a TypedArray receiver".into())
        })?;
        self.heap
            .typed_array_info(object)
            .map_err(|error| match error {
                HeapError::InvalidObject(_) => RuntimeError::TypeError(
                    "TypedArray method requires a TypedArray receiver".into(),
                ),
                error => error.into(),
            })
    }

    pub(super) fn typed_array_set(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (buffer, _, length, kind) = self.typed_array_receiver(receiver)?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "TypedArray buffer is detached".into(),
            ));
        }
        let source = native::argument(args, 0);
        let target_offset = self.buffer_index(native::argument(args, 1))?;
        if target_offset > length {
            return Err(RuntimeError::RangeError(
                "target offset is outside TypedArray".into(),
            ));
        }
        let source = self.coerce_object(source)?;
        self.stack.push(Value::Object(source));
        let result = (|| {
            let source_length_value =
                self.get_property(&Value::Object(source), &"length".into())?;
            let source_length = self.coerce_length(&source_length_value)? as usize;
            if source_length > length - target_offset {
                return Err(RuntimeError::RangeError(
                    "source does not fit in TypedArray".into(),
                ));
            }
            let mut values = Vec::with_capacity(source_length);
            for index in 0..source_length {
                let value = self.get_property(&Value::Object(source), &index.to_string().into())?;
                values.push(self.typed_array_element_value(kind, &value)?);
            }
            for (index, value) in values.into_iter().enumerate() {
                self.with_roots(|heap| {
                    heap.typed_array_set_index(
                        receiver.object_id().expect("validated TypedArray receiver"),
                        target_offset + index,
                        &value,
                    )
                })?;
            }
            Ok(Value::Undefined)
        })();
        self.stack.pop();
        result
    }

    pub(super) fn typed_array_subarray(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (buffer, byte_offset, length, kind) = self.typed_array_receiver(receiver)?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "TypedArray buffer is detached".into(),
            ));
        }
        let start = self.relative_buffer_index(native::argument(args, 0), length)?;
        let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let view_length = end.saturating_sub(start);
        let view_offset = byte_offset
            .checked_add(start * kind.byte_width())
            .ok_or_else(|| RuntimeError::RangeError("TypedArray offset is too large".into()))?;
        let prototype = self.buffer_prototype(kind.name())?;
        let object = self.with_roots(|heap| {
            heap.alloc_typed_array(buffer, view_offset, view_length, kind, Some(prototype))
        })?;
        Ok(Value::Object(object))
    }
}
