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

    /// OrdinaryCreateFromConstructor for concrete binary constructors. The
    /// intrinsic prototype remains the fallback when a custom
    /// `newTarget.prototype` is not an object.
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
            for (name, kind) in [
                ("entries", ArrayIteratorKind::Entries),
                ("keys", ArrayIteratorKind::Keys),
                ("values", ArrayIteratorKind::Values),
            ] {
                self.install_native(
                    typed_prototype,
                    function_prototype,
                    name,
                    0,
                    NativeFunction::TypedArrayIterator(kind),
                )?;
            }
            for (name, length, method) in [
                ("at", 1, TypedArrayMethod::At),
                ("copyWithin", 2, TypedArrayMethod::CopyWithin),
                ("every", 1, TypedArrayMethod::Every),
                ("fill", 1, TypedArrayMethod::Fill),
                ("filter", 1, TypedArrayMethod::Filter),
                ("find", 1, TypedArrayMethod::Find),
                ("findIndex", 1, TypedArrayMethod::FindIndex),
                ("findLast", 1, TypedArrayMethod::FindLast),
                ("findLastIndex", 1, TypedArrayMethod::FindLastIndex),
                ("map", 1, TypedArrayMethod::Map),
                ("lastIndexOf", 1, TypedArrayMethod::LastIndexOf),
                ("forEach", 1, TypedArrayMethod::ForEach),
                ("includes", 1, TypedArrayMethod::Includes),
                ("indexOf", 1, TypedArrayMethod::IndexOf),
                ("join", 1, TypedArrayMethod::Join),
                ("reduce", 1, TypedArrayMethod::Reduce),
                ("toString", 0, TypedArrayMethod::ToString),
                ("reduceRight", 1, TypedArrayMethod::ReduceRight),
                ("reverse", 0, TypedArrayMethod::Reverse),
                ("slice", 2, TypedArrayMethod::Slice),
                ("some", 1, TypedArrayMethod::Some),
                ("sort", 1, TypedArrayMethod::Sort),
                ("toReversed", 0, TypedArrayMethod::ToReversed),
                ("toSorted", 1, TypedArrayMethod::ToSorted),
                ("with", 2, TypedArrayMethod::With),
            ] {
                self.install_native(
                    typed_prototype,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::TypedArrayMethod(method),
                )?;
            }
            self.install_getter(
                constructor,
                function_prototype,
                JsSymbol::well_known("species").into(),
                "get [Symbol.species]",
                NativeFunction::TypedArraySpecies,
            )?;
            let values = self
                .heap
                .get(typed_prototype, "values")
                .expect("installed TypedArray values method");
            self.define_data(
                typed_prototype,
                JsSymbol::well_known("iterator"),
                values,
                true,
                false,
                true,
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
        let maximum = self.buffer_max_byte_length_option(args)?;
        if maximum.is_some_and(|maximum| length > maximum) {
            return Err(RuntimeError::RangeError(
                "ArrayBuffer length exceeds maxByteLength".into(),
            ));
        }
        let prototype = self.constructed_buffer_prototype("ArrayBuffer")?;
        Ok(Value::Object(match maximum {
            Some(maximum) => self.with_roots(|heap| {
                heap.alloc_resizable_array_buffer(length, maximum, Some(prototype))
            })?,
            None => self.with_roots(|heap| heap.alloc_array_buffer(length, Some(prototype)))?,
        }))
    }

    pub(super) fn shared_array_buffer_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "SharedArrayBuffer constructor requires 'new'".into(),
            ));
        }
        let length = if args.is_empty() {
            0
        } else {
            self.buffer_index(native::argument(args, 0))?
        };
        let maximum = self.buffer_max_byte_length_option(args)?;
        if maximum.is_some_and(|maximum| length > maximum) {
            return Err(RuntimeError::RangeError(
                "SharedArrayBuffer length exceeds maxByteLength".into(),
            ));
        }
        let prototype = self.constructed_buffer_prototype("SharedArrayBuffer")?;
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_shared_array_buffer(length, maximum, Some(prototype))
        })?))
    }

    pub(super) fn buffer_max_byte_length_option(
        &mut self,
        args: &[Value],
    ) -> Result<Option<usize>, RuntimeError> {
        let Some(options) = args.get(1) else {
            return Ok(None);
        };
        if !matches!(options, Value::Object(_)) {
            return Ok(None);
        }
        let maximum = self.get_property(options, &"maxByteLength".into())?;
        if maximum == Value::Undefined {
            return Ok(None);
        }
        Ok(Some(self.buffer_index(&maximum)?))
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

    pub(super) fn shared_array_buffer_receiver(
        &self,
        receiver: &Value,
    ) -> Result<ObjectId, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "SharedArrayBuffer method requires a SharedArrayBuffer receiver".into(),
            )
        })?;
        if !self.heap.is_shared_array_buffer(object)? {
            return Err(RuntimeError::TypeError(
                "SharedArrayBuffer method requires a SharedArrayBuffer receiver".into(),
            ));
        }
        Ok(object)
    }

    pub(super) fn buffer_resize(
        &mut self,
        receiver: &Value,
        length: &Value,
    ) -> Result<Value, RuntimeError> {
        let buffer = self.array_buffer_receiver(receiver)?;
        let length = self.buffer_index(length)?;
        if !self.heap.buffer_resizable(buffer)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer is not resizable".into(),
            ));
        }
        self.with_roots(|heap| heap.resize_array_buffer(buffer, length))?;
        Ok(Value::Undefined)
    }

    /// Implements ArrayBuffer.prototype.transfer and transferToFixedLength.
    /// A transfer never observes `constructor` or species: it allocates the
    /// intrinsic ArrayBuffer, copies the bounded prefix, and detaches the
    /// source only after every fallible conversion and allocation succeeds.
    pub(super) fn array_buffer_transfer(
        &mut self,
        receiver: &Value,
        args: &[Value],
        fixed_length: bool,
    ) -> Result<Value, RuntimeError> {
        let source = self.array_buffer_receiver(receiver)?;
        if self.heap.buffer_is_detached(source)? {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        let source_length = self.heap.buffer_byte_length(source)?;
        let length = if args.is_empty() || args[0] == Value::Undefined {
            source_length
        } else {
            self.buffer_index(native::argument(args, 0))?
        };
        let resizable = !fixed_length && self.heap.buffer_resizable(source)?;
        let maximum = if resizable {
            self.heap.buffer_max_byte_length(source)?
        } else {
            length
        };
        if length > maximum {
            return Err(RuntimeError::RangeError(
                "transfer length exceeds ArrayBuffer maxByteLength".into(),
            ));
        }
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        let target = self.with_roots(|heap| {
            if resizable {
                heap.alloc_resizable_array_buffer(length, maximum, Some(prototype))
            } else {
                heap.alloc_array_buffer(length, Some(prototype))
            }
        })?;
        let copy_length = source_length.min(length);
        let bytes = self.heap.array_buffer_copy(source, 0, copy_length)?;
        self.with_roots(|heap| heap.array_buffer_write(target, 0, &bytes))?;
        self.with_roots(|heap| heap.detach_array_buffer(source))?;
        Ok(Value::Object(target))
    }

    pub(super) fn shared_buffer_grow(
        &mut self,
        receiver: &Value,
        length: &Value,
    ) -> Result<Value, RuntimeError> {
        let buffer = self.shared_array_buffer_receiver(receiver)?;
        let length = self.buffer_index(length)?;
        if !self.heap.buffer_growable(buffer)? {
            return Err(RuntimeError::TypeError(
                "SharedArrayBuffer is not growable".into(),
            ));
        }
        self.with_roots(|heap| heap.grow_shared_array_buffer(buffer, length))?;
        Ok(Value::Undefined)
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

    pub(super) fn shared_array_buffer_species_constructor(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let constructor = self.get_property(receiver, &"constructor".into())?;
        if constructor == Value::Undefined {
            return self.global("SharedArrayBuffer");
        }
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "SharedArrayBuffer constructor must be an object".into(),
            ));
        }
        let species = self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
        if matches!(species, Value::Undefined | Value::Null) {
            return self.global("SharedArrayBuffer");
        }
        if !self.is_constructor(&species)? {
            return Err(RuntimeError::TypeError(
                "SharedArrayBuffer species must be a constructor".into(),
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

    pub(super) fn shared_array_buffer_slice(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let buffer = self.shared_array_buffer_receiver(receiver)?;
        let length = self.heap.buffer_byte_length(buffer)?;
        let start = self.relative_buffer_index(native::argument(args, 0), length)?;
        let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let width = end.saturating_sub(start);
        let constructor = self.shared_array_buffer_species_constructor(receiver)?;
        let result = self.call_with_target(
            constructor.clone(),
            Value::Undefined,
            vec![Value::Number(width as f64)],
            true,
            constructor,
        )?;
        let result_buffer = self.shared_array_buffer_receiver(&result)?;
        if result_buffer == buffer {
            return Err(RuntimeError::TypeError(
                "SharedArrayBuffer species returned the source buffer".into(),
            ));
        }
        if self.heap.buffer_byte_length(result_buffer)? < width {
            return Err(RuntimeError::TypeError(
                "SharedArrayBuffer species result is too small".into(),
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
        if !self.heap.is_buffer(buffer)? {
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
        if self.heap.buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "DataView buffer is detached".into(),
            ));
        }
        let total = self.heap.buffer_byte_length(buffer)?;
        if offset > total {
            return Err(RuntimeError::RangeError(
                "DataView offset is outside its buffer".into(),
            ));
        }
        let length_tracking = args.len() <= 2 || native::argument(args, 2) == &Value::Undefined;
        let length = if !length_tracking {
            self.buffer_index(native::argument(args, 2))?
        } else {
            total - offset
        };
        let prototype = self.constructed_buffer_prototype("DataView")?;
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_data_view(buffer, offset, length, length_tracking, Some(prototype))
        })?))
    }

    pub(super) fn data_view_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, usize), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("DataView method requires a DataView receiver".into())
        })?;
        self.heap
            .data_view_current_info(object)
            .map_err(|_| RuntimeError::TypeError("DataView is out of bounds".into()))
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
        self.data_view_raw_receiver(receiver)?;
        let index = self.buffer_index(native::argument(args, 0))?;
        let (buffer, offset, length) = self.data_view_receiver(receiver)?;
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
        self.data_view_raw_receiver(receiver)?;
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
        let (buffer, offset, length) = self.data_view_receiver(receiver)?;
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

    fn atomics_access(
        &mut self,
        args: &[Value],
        waitable: bool,
    ) -> Result<(ObjectId, usize, TypedArrayKind), RuntimeError> {
        let object = native::argument(args, 0).object_id().ok_or_else(|| {
            RuntimeError::TypeError("Atomics requires an integer TypedArray".into())
        })?;
        if !self.heap.is_typed_array(object)? {
            return Err(RuntimeError::TypeError(
                "Atomics requires an integer TypedArray".into(),
            ));
        }
        let (buffer, _, length, kind) = self.heap.typed_array_info(object)?;
        if !self.heap.buffer_is_shared(buffer)? || !kind.atomic() || (waitable && !kind.waitable())
        {
            return Err(RuntimeError::TypeError(
                "Atomics requires a shared integer TypedArray".into(),
            ));
        }
        if self.heap.typed_array_is_out_of_bounds(object)? {
            return Err(RuntimeError::TypeError(
                "TypedArray is out of bounds".into(),
            ));
        }
        let index = self.buffer_index(native::argument(args, 1))?;
        if index >= length {
            return Err(RuntimeError::RangeError(
                "Atomics index is outside TypedArray".into(),
            ));
        }
        Ok((object, index, kind))
    }

    fn atomics_element_value(
        &mut self,
        kind: TypedArrayKind,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let value = self.typed_array_element_value(kind, value)?;
        Ok(self.heap.typed_array_normalize_value(kind, &value))
    }

    fn atomics_read(&self, object: ObjectId, index: usize) -> Result<Value, RuntimeError> {
        self.heap
            .typed_array_index_value(object, index)?
            .ok_or_else(|| RuntimeError::TypeError("TypedArray is out of bounds".into()))
    }

    fn atomics_write(
        &mut self,
        object: ObjectId,
        index: usize,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        self.with_roots(|heap| heap.typed_array_set_index(object, index, value))?;
        self.atomics_read(object, index)
    }

    fn atomics_binary_value(
        kind: TypedArrayKind,
        old: &Value,
        value: &Value,
        operation: AtomicOp,
    ) -> Value {
        if kind.bigint() {
            let (Value::BigInt(old), Value::BigInt(value)) = (old, value) else {
                unreachable!("BigInt atomic operations have BigInt operands");
            };
            return Value::BigInt(match operation {
                AtomicOp::Add => old + value,
                AtomicOp::And => old & value,
                AtomicOp::Or => old | value,
                AtomicOp::Sub => old - value,
                AtomicOp::Xor => old ^ value,
                _ => unreachable!("only read-modify-write operations reach this helper"),
            });
        }
        let (Value::Number(old), Value::Number(value)) = (old, value) else {
            unreachable!("numeric atomic operations have Number operands");
        };
        let (old, value) = (*old as i64, *value as i64);
        Value::Number(match operation {
            AtomicOp::Add => old.wrapping_add(value),
            AtomicOp::And => old & value,
            AtomicOp::Or => old | value,
            AtomicOp::Sub => old.wrapping_sub(value),
            AtomicOp::Xor => old ^ value,
            _ => unreachable!("only read-modify-write operations reach this helper"),
        } as f64)
    }

    pub(super) fn atomics_operation(
        &mut self,
        args: &[Value],
        operation: AtomicOp,
    ) -> Result<Value, RuntimeError> {
        let (object, index, kind) = self.atomics_access(args, false)?;
        let old = self.atomics_read(object, index)?;
        match operation {
            AtomicOp::Load => Ok(old),
            AtomicOp::Store => {
                let value = self.atomics_element_value(kind, native::argument(args, 2))?;
                self.atomics_write(object, index, &value)
            }
            AtomicOp::CompareExchange => {
                let expected = self.atomics_element_value(kind, native::argument(args, 2))?;
                let replacement = self.atomics_element_value(kind, native::argument(args, 3))?;
                if old == expected {
                    self.atomics_write(object, index, &replacement)?;
                }
                Ok(old)
            }
            AtomicOp::Add | AtomicOp::And | AtomicOp::Or | AtomicOp::Sub | AtomicOp::Xor => {
                let value = self.atomics_element_value(kind, native::argument(args, 2))?;
                let value = Self::atomics_binary_value(kind, &old, &value, operation);
                self.atomics_write(object, index, &value)?;
                Ok(old)
            }
            AtomicOp::Exchange => {
                let value = self.atomics_element_value(kind, native::argument(args, 2))?;
                self.atomics_write(object, index, &value)?;
                Ok(old)
            }
        }
    }

    pub(super) fn atomics_is_lock_free(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let size = self.coerce_number(value)?;
        let size = if size.is_finite() { size.trunc() } else { 0.0 };
        Ok(Value::Bool(matches!(size as i64, 1 | 2 | 4 | 8)))
    }

    pub(super) fn atomics_notify(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        self.atomics_access(args, true)?;
        // BlueJS has one VM thread today. There can be no blocked agents to
        // awaken, but validation and the observable conversions above follow
        // the ordinary Atomics path.
        Ok(Value::Number(0.0))
    }

    fn atomics_wait_status(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let (object, index, kind) = self.atomics_access(args, true)?;
        let expected = self.atomics_element_value(kind, native::argument(args, 2))?;
        let observed = self.atomics_read(object, index)?;
        if observed != expected {
            return Ok(Value::String("not-equal".into()));
        }
        if let Some(timeout) = args.get(3) {
            self.coerce_number(timeout)?;
        }
        // A waiter queue needs the P1.6 agent scheduler. Until that host
        // boundary is present, a matching wait completes as a timed-out
        // single-agent operation rather than blocking the VM thread.
        Ok(Value::String("timed-out".into()))
    }

    pub(super) fn atomics_wait(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        self.atomics_wait_status(args)
    }

    pub(super) fn atomics_wait_async(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let value = self.atomics_wait_status(args)?;
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.define_data(result, "async", Value::Bool(false), true, true, true)?;
        self.define_data(result, "value", value, true, true, true)?;
        Ok(Value::Object(result))
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
        let (buffer, byte_offset, length, length_tracking, initial_values) =
            if let Value::Object(buffer) = input {
                if self.heap.is_buffer(*buffer)? {
                    if self.heap.buffer_is_detached(*buffer)? {
                        return Err(RuntimeError::TypeError(
                            "TypedArray buffer is detached".into(),
                        ));
                    }
                    let bytes = self.heap.buffer_byte_length(*buffer)?;
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
                    let length_tracking =
                        args.len() <= 2 || native::argument(args, 2) == &Value::Undefined;
                    let length = if !length_tracking {
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
                    (*buffer, offset, length, length_tracking, None)
                } else if self.heap.is_typed_array(*buffer)? {
                    let (source_buffer, _, source_length, _) =
                        self.heap.typed_array_info(*buffer)?;
                    if self.heap.buffer_is_detached(source_buffer)? {
                        return Err(RuntimeError::TypeError(
                            "TypedArray source is detached".into(),
                        ));
                    }
                    let values = self.typed_array_values(*buffer, source_length)?;
                    let result = self.new_typed_array_buffer(source_length, kind)?;
                    (result, 0, source_length, false, Some(values))
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
                    (result, 0, length, false, Some(values))
                }
            } else {
                let length = if *input == Value::Undefined {
                    0
                } else {
                    self.buffer_index(input)?
                };
                let buffer = self.new_typed_array_buffer(length, kind)?;
                (buffer, 0, length, false, None)
            };
        let base = self.stack.len();
        self.stack.push(Value::Object(buffer));
        let result = (|| {
            let prototype = self.constructed_buffer_prototype(kind.name())?;
            let object = self.with_roots(|heap| {
                heap.alloc_typed_array(
                    buffer,
                    byte_offset,
                    length,
                    length_tracking,
                    kind,
                    Some(prototype),
                )
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
        if self.heap.typed_array_is_out_of_bounds(object)? {
            return Err(RuntimeError::TypeError(
                "TypedArray is out of bounds".into(),
            ));
        }
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
        let (_, _, length, kind) = self.typed_array_receiver(receiver)?;
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
        let start = self.relative_buffer_index(native::argument(args, 0), length)?;
        let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let length_tracking = args.get(1).is_none_or(|value| *value == Value::Undefined)
            && self.heap.typed_array_is_length_tracking(
                receiver.object_id().expect("validated TypedArray"),
            )?;
        let view_length = end.saturating_sub(start);
        let view_offset = byte_offset
            .checked_add(start * kind.byte_width())
            .ok_or_else(|| RuntimeError::RangeError("TypedArray offset is too large".into()))?;
        let prototype = self.buffer_prototype(kind.name())?;
        let object = self.with_roots(|heap| {
            heap.alloc_typed_array(
                buffer,
                view_offset,
                view_length,
                length_tracking,
                kind,
                Some(prototype),
            )
        })?;
        Ok(Value::Object(object))
    }
}
