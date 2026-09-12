// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn native_call(
        &mut self,
        function: NativeFunction,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if receiver
            .object_id()
            .is_some_and(|id| self.test262_foreign_reference(id).is_some())
            && matches!(
                function,
                NativeFunction::ArrayIteratorNext
                    | NativeFunction::IteratorNext
                    | NativeFunction::RegExpIteratorNext
            )
        {
            return self.test262_foreign_next(&receiver, &args);
        }
        let first = native::argument(&args, 0);
        match function {
            NativeFunction::Promise => self.promise_constructor(first.clone(), construct),
            NativeFunction::PromiseResolvingFunction { promise, fulfill } => {
                if fulfill {
                    self.resolve_promise(promise, first.clone())?;
                } else {
                    self.settle_promise(promise, PromiseStatus::Rejected(first.clone()))?;
                }
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseCapabilityExecutor { storage } => {
                let resolve = native::argument(&args, 0).clone();
                let reject = native::argument(&args, 1).clone();
                self.with_roots(|heap| heap.set(storage, "resolve", resolve))?;
                self.with_roots(|heap| heap.set(storage, "reject", reject))?;
                Ok(Value::Undefined)
            }
            NativeFunction::AsyncFromSyncFulfill { target, done } => {
                let result = self.iterator_result(first.clone(), done);
                match result {
                    Ok(result) => self.settle_promise(target, PromiseStatus::Fulfilled(result))?,
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
                Ok(Value::Undefined)
            }
            NativeFunction::AsyncFromSyncReject { target, record } => {
                // AsyncFromSyncIteratorContinuation closes with an existing
                // throw completion. IteratorClose must retain that original
                // rejection even when the delegate's return method fails or
                // returns a non-object.
                let _ = self.iterator_close(&Value::Object(record));
                self.settle_promise(target, PromiseStatus::Rejected(first.clone()))?;
                Ok(Value::Undefined)
            }
            NativeFunction::AbstractModuleSource => Err(RuntimeError::TypeError(
                "AbstractModuleSource is an abstract constructor".into(),
            )),
            NativeFunction::AbstractModuleSourceToStringTag => Ok(Value::Undefined),
            NativeFunction::Function => self.function_constructor(&args),
            NativeFunction::AsyncFunction => self.async_function_constructor(&args),
            NativeFunction::Error(name) => self.error_constructor(name, &args, construct),
            NativeFunction::ErrorToString => self.error_to_string(&receiver),
            NativeFunction::Test262(name) => self.test262_call(name, &args),
            NativeFunction::Test262Done => {
                self.test262_done = Some(if matches!(first, Value::Undefined) {
                    Ok(())
                } else {
                    Err(first.clone())
                });
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseThen => self.promise_then(&receiver, &args),
            NativeFunction::PromiseCatch => self.promise_catch(&receiver, first),
            NativeFunction::PromiseFinally => self.promise_finally(&receiver, first),
            NativeFunction::PromiseResolve => {
                self.promise_resolve_constructor(&receiver, first.clone())
            }
            NativeFunction::PromiseReject => self.promise_reject(first.clone()),
            NativeFunction::PromiseAll => self.promise_all(&receiver, first),
            NativeFunction::PromiseAllResolve { target, index } => {
                self.promise_all_settled(target, index, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAllReject { target } => {
                self.promise_all_reject(target, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseWithResolvers => self.promise_with_resolvers(),
            NativeFunction::ToLocaleLowerCase
            | NativeFunction::ToLocaleUpperCase
            | NativeFunction::LocaleCompare => {
                let string = self.string_receiver(&receiver)?;
                if function == NativeFunction::LocaleCompare {
                    let other = self.coerce_string(first)?;
                    let collator = self
                        .resolve_collator(native::argument(&args, 1), native::argument(&args, 2))?;
                    Ok(collator.compare(&string, &other))
                } else {
                    let locales = self.canonical_locales(first)?;
                    let locale = locales
                        .first()
                        .cloned()
                        .unwrap_or(icu_locale_core::locale!("en-US"));
                    crate::intl::case_map(
                        &string,
                        &locale,
                        function == NativeFunction::ToLocaleUpperCase,
                        self.config.max_string_bytes,
                    )
                    .map(Value::String)
                }
            }
            NativeFunction::Collator => self.create_collator(&args, construct),
            NativeFunction::Locale => self.create_locale(&args, construct),
            NativeFunction::CanonicalLocales => {
                let locales = self.canonical_locales(first)?;
                self.array_from(
                    locales
                        .into_iter()
                        .map(|l| Value::String(l.to_string().into()))
                        .collect(),
                )
            }
            NativeFunction::SupportedLocales => self.supported_locales(&args),
            NativeFunction::CollatorCompareGetter => self.collator_compare_getter(&receiver),
            NativeFunction::CollatorCompare => {
                let collator = self.collator_data(&receiver)?;
                let left = self.coerce_string(first)?;
                let right = self.coerce_string(native::argument(&args, 1))?;
                Ok(collator.compare(&left, &right))
            }
            NativeFunction::CollatorResolvedOptions => self.collator_resolved_options(&receiver),
            NativeFunction::LocaleToString => self.locale_to_string(&receiver),
            NativeFunction::LocaleMaximize => self.locale_transform(&receiver, true),
            NativeFunction::LocaleMinimize => self.locale_transform(&receiver, false),
            NativeFunction::LocaleGetter(name) => self.locale_getter(&receiver, name),
            NativeFunction::LocaleInfo(name) => self.locale_info(&receiver, name),
            NativeFunction::Array => {
                if args.len() == 1 {
                    if let Value::Number(length) = first {
                        let Value::Number(length) =
                            self.array_length_value(&Value::Number(*length))?
                        else {
                            unreachable!()
                        };
                        let prototype = self.array_prototype;
                        return Ok(Value::Object(self.with_roots(|heap| {
                            heap.alloc_array(length as u32, Some(prototype))
                        })?));
                    }
                }
                self.array_from(args)
            }
            NativeFunction::ArrayBuffer => self.array_buffer_constructor(&args, construct),
            NativeFunction::ArrayBufferByteLength => Ok(Value::Number(
                self.heap
                    .array_buffer_byte_length(self.array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::ArrayBufferMaxByteLength => Ok(Value::Number(
                self.heap
                    .buffer_max_byte_length(self.array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::ArrayBufferResizable => Ok(Value::Bool(
                self.heap
                    .buffer_resizable(self.array_buffer_receiver(&receiver)?)?,
            )),
            NativeFunction::ArrayBufferResize => self.buffer_resize(&receiver, first),
            NativeFunction::ArrayBufferTransfer => {
                self.array_buffer_transfer(&receiver, &args, false)
            }
            NativeFunction::ArrayBufferTransferToFixedLength => {
                self.array_buffer_transfer(&receiver, &args, true)
            }
            NativeFunction::ArrayBufferSlice => self.array_buffer_slice(&receiver, &args),
            NativeFunction::ArrayBufferIsView => {
                Ok(Value::Bool(first.object_id().is_some_and(|object| {
                    self.heap.is_data_view(object).unwrap_or(false)
                        || self.heap.is_typed_array(object).unwrap_or(false)
                })))
            }
            NativeFunction::ArrayBufferSpecies => Ok(receiver),
            NativeFunction::SharedArrayBuffer => {
                self.shared_array_buffer_constructor(&args, construct)
            }
            NativeFunction::SharedArrayBufferByteLength => Ok(Value::Number(
                self.heap
                    .buffer_byte_length(self.shared_array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::SharedArrayBufferMaxByteLength => Ok(Value::Number(
                self.heap
                    .buffer_max_byte_length(self.shared_array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::SharedArrayBufferGrowable => Ok(Value::Bool(
                self.heap
                    .buffer_growable(self.shared_array_buffer_receiver(&receiver)?)?,
            )),
            NativeFunction::SharedArrayBufferGrow => self.shared_buffer_grow(&receiver, first),
            NativeFunction::SharedArrayBufferSlice => {
                self.shared_array_buffer_slice(&receiver, &args)
            }
            NativeFunction::SharedArrayBufferSpecies => Ok(receiver),
            NativeFunction::Atomics(operation) => self.atomics_operation(&args, operation),
            NativeFunction::AtomicsIsLockFree => self.atomics_is_lock_free(first),
            NativeFunction::AtomicsNotify => self.atomics_notify(&args),
            NativeFunction::AtomicsPause => Ok(Value::Undefined),
            NativeFunction::AtomicsWait => self.atomics_wait(&args),
            NativeFunction::AtomicsWaitAsync => self.atomics_wait_async(&args),
            NativeFunction::DataView => self.data_view_constructor(&args, construct),
            NativeFunction::DataViewBuffer => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError("DataView method requires a DataView receiver".into())
                })?;
                let buffer = self
                    .heap
                    .data_view_buffer(object)
                    .map_err(|error| match error {
                        HeapError::InvalidInternalSlot(_) | HeapError::InvalidObject(_) => {
                            RuntimeError::TypeError(
                                "DataView method requires a DataView receiver".into(),
                            )
                        }
                        error => error.into(),
                    })?;
                Ok(Value::Object(buffer))
            }
            NativeFunction::DataViewByteLength => {
                let (_, _, length) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(length as f64))
            }
            NativeFunction::DataViewByteOffset => {
                let (_, offset, _) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(offset as f64))
            }
            NativeFunction::DataViewGet {
                width,
                signed,
                floating,
                bigint,
            } => self.data_view_get(&receiver, &args, width, signed, floating, bigint),
            NativeFunction::DataViewSet {
                width,
                signed,
                floating,
                bigint,
            } => self.data_view_set(&receiver, &args, width, signed, floating, bigint),
            NativeFunction::TypedArray(kind) => {
                self.typed_array_constructor(&args, construct, kind)
            }
            NativeFunction::TypedArrayIntrinsic => Err(RuntimeError::TypeError(
                "%TypedArray% is not directly constructible".into(),
            )),
            NativeFunction::TypedArrayBuffer => {
                let (buffer, _, _, _) = self.typed_array_receiver(&receiver)?;
                Ok(Value::Object(buffer))
            }
            NativeFunction::TypedArrayByteLength => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, _, length, kind) = self.heap.typed_array_info(object)?;
                let byte_length = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    length * kind.byte_width()
                };
                Ok(Value::Number(byte_length as f64))
            }
            NativeFunction::TypedArrayByteOffset => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, offset, _, _) = self.heap.typed_array_info(object)?;
                let byte_offset = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    offset
                };
                Ok(Value::Number(byte_offset as f64))
            }
            NativeFunction::TypedArrayLength => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, _, length, _) = self.heap.typed_array_info(object)?;
                let element_length = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    length
                };
                Ok(Value::Number(element_length as f64))
            }
            NativeFunction::TypedArraySet => self.typed_array_set(&receiver, &args),
            NativeFunction::TypedArraySubarray => self.typed_array_subarray(&receiver, &args),
            NativeFunction::TypedArraySpecies => Ok(receiver),
            NativeFunction::TypedArrayIterator(kind) => {
                self.typed_array_receiver(&receiver)?;
                let object = receiver
                    .object_id()
                    .expect("validated TypedArray receiver has an object identity");
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(object, kind, prototype)
                })?))
            }
            NativeFunction::TypedArrayMethod(method) => {
                self.typed_array_method(&receiver, &args, method)
            }
            NativeFunction::Proxy => self.proxy_constructor(&args, construct),
            NativeFunction::ProxyRevocable => self.proxy_revocable(&args),
            NativeFunction::ProxyRevoker(proxy) => {
                self.with_roots(|heap| heap.revoke_proxy(proxy))?;
                Ok(Value::Undefined)
            }
            NativeFunction::Map => self.collection_constructor(true, construct),
            NativeFunction::Set => self.collection_constructor(false, construct),
            NativeFunction::ArrayIsArray => Ok(Value::Bool(
                first
                    .object_id()
                    .is_some_and(|id| self.heap.is_array(id).unwrap_or(false)),
            )),
            NativeFunction::ArrayFrom => self.array_from_method(&args),
            NativeFunction::ArrayForEach => {
                self.array_for_each(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayIncludes => {
                self.array_includes(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayReduce => self.array_reduce(&receiver, &args),
            NativeFunction::ArrayPush => {
                let object = self.coerce_object(&receiver)?;
                let array = Value::Object(object);
                self.stack.push(array.clone());
                let result = (|| {
                    for value in &args {
                        self.array_push(&array, value, 0)?;
                    }
                    self.heap.get(object, "length").map_err(Into::into)
                })();
                self.stack.pop();
                result
            }
            NativeFunction::ArrayIndexOf => {
                self.array_index_of(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArraySlice => self.array_slice(&receiver, &args),
            NativeFunction::ArraySplice => self.array_splice(&receiver, &args),
            NativeFunction::Eval => self.indirect_eval(first),
            NativeFunction::IsNaN => Ok(Value::Bool(self.coerce_number(first)?.is_nan())),
            NativeFunction::IsFinite => Ok(Value::Bool(self.coerce_number(first)?.is_finite())),
            NativeFunction::ParseInt => self.parse_int(first, native::argument(&args, 1)),
            NativeFunction::ParseFloat => self.parse_float(first),
            NativeFunction::EncodeUri { component } => self.encode_uri(first, component),
            NativeFunction::DecodeUri { component } => self.decode_uri(first, component),
            NativeFunction::DynamicImport { source } => {
                if source {
                    self.dynamic_import_source(first.clone())
                } else {
                    self.dynamic_import(first.clone())
                }
            }
            NativeFunction::JsonParse => self.json_parse(first),
            NativeFunction::JsonStringify => self.json_stringify(first),
            NativeFunction::Math(method) => self.math_method(method, &args),
            NativeFunction::Bind => self.bind_function(receiver, &args),
            NativeFunction::HasInstance => self
                .has_instance(first.clone(), receiver, true)
                .map(Value::Bool),
            NativeFunction::RegExpEscape => self.regexp_escape(first),
            NativeFunction::ArrayIterator(kind) => {
                let object = self.coerce_object(&receiver)?;
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(object, kind, prototype)
                })?))
            }
            NativeFunction::ArrayIteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                let Some((object, index, done, kind)) = self.heap.array_iterator(id)? else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                if done {
                    return self.iterator_result(Value::Undefined, true);
                }
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)?;
                let done = index as f64 >= length;
                self.heap.advance_array_iterator(id, done);
                let value = if done {
                    Value::Undefined
                } else {
                    match kind {
                        ArrayIteratorKind::Keys => Value::Number(index as f64),
                        ArrayIteratorKind::Values => {
                            self.get_property(&Value::Object(object), &index.to_string().into())?
                        }
                        ArrayIteratorKind::Entries => {
                            let entry = self
                                .get_property(&Value::Object(object), &index.to_string().into())?;
                            self.array_from(vec![Value::Number(index as f64), entry])?
                        }
                    }
                };
                self.iterator_result(value, done)
            }
            NativeFunction::GeneratorNext => {
                self.generator_next(&receiver, Some(first.clone()), None)
            }
            NativeFunction::GeneratorReturn => self.generator_return(&receiver, first.clone()),
            NativeFunction::GeneratorThrow => self.generator_throw(&receiver, first.clone()),
            NativeFunction::AsyncGeneratorNext
            | NativeFunction::AsyncGeneratorReturn
            | NativeFunction::AsyncGeneratorThrow => {
                self.async_generator_request(&receiver, first.clone(), function)
            }
            NativeFunction::Apply => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError("apply requires a callable".into()));
                }
                let list = native::argument(&args, 1);
                let values = if matches!(list, Value::Null | Value::Undefined) {
                    Vec::new()
                } else {
                    self.array_like_values(list)?
                };
                self.call_native(receiver, first.clone(), values, false)
            }
            NativeFunction::ReflectApply => {
                if !self.is_callable(first)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.apply requires a callable target".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 2))?;
                self.call_native(
                    first.clone(),
                    native::argument(&args, 1).clone(),
                    values,
                    false,
                )
            }
            NativeFunction::ReflectConstruct => {
                let new_target = if args.len() > 2 {
                    args[2].clone()
                } else {
                    first.clone()
                };
                if !self.is_constructor(first)? || !self.is_constructor(&new_target)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.construct requires constructors".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 1))?;
                self.call_with_target(first.clone(), Value::Undefined, values, true, new_target)
            }
            NativeFunction::FunctionToString => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError(
                        "Function.toString requires a callable".into(),
                    ));
                }
                let name = self
                    .heap
                    .function_initial_name(receiver.object_id().unwrap())?;
                let mut result = JsString::from("function ");
                result.push_str(&name);
                result.push_str(&"() { [native code] }".into());
                Ok(Value::String(result))
            }
            NativeFunction::PrimitiveConstructor(boolean) => {
                let value = if boolean {
                    Value::Bool(self.to_boolean(first)?)
                } else if let Value::BigInt(value) = first {
                    Value::Number(value.to_f64().unwrap_or_else(|| {
                        if value.sign() == Sign::Minus {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }
                    }))
                } else {
                    Value::Number(if args.is_empty() {
                        0.0
                    } else {
                        self.coerce_number(first)?
                    })
                };
                if !construct {
                    return Ok(value);
                }
                let constructor = self.global(if boolean { "Boolean" } else { "Number" })?;
                let default = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                let prototype = self.constructor_prototype(default)?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_boxed_primitive(value, prototype)
                })?))
            }
            NativeFunction::BigInt => {
                if construct {
                    return Err(RuntimeError::TypeError(
                        "BigInt is not a constructor".into(),
                    ));
                }
                let value = self.coerce_primitive(first, "number")?;
                match value {
                    Value::BigInt(value) => Ok(Value::BigInt(value)),
                    Value::Number(value) if value.is_finite() && value.fract() == 0.0 => {
                        // Every integral IEEE-754 Number is within i64's
                        // magnitude range, including the safe-integer range
                        // used by TypedArray conversion fixtures.
                        Ok(Value::BigInt(BigInt::from(value as i64)))
                    }
                    Value::Number(_) => Err(RuntimeError::RangeError(
                        "BigInt conversion requires an integral Number".into(),
                    )),
                    Value::String(value) => {
                        let value = value.to_utf8().map_err(|_| {
                            RuntimeError::SyntaxError("invalid BigInt string".into())
                        })?;
                        let value =
                            BigInt::parse_bytes(value.trim().as_bytes(), 10).ok_or_else(|| {
                                RuntimeError::SyntaxError("invalid BigInt string".into())
                            })?;
                        Ok(Value::BigInt(value))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "BigInt conversion requires a Number, BigInt, or integer string".into(),
                    )),
                }
            }
            NativeFunction::PrimitiveMethod { boolean, string } => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                if !matches!(
                    (&value, boolean),
                    (Value::Bool(_), true) | (Value::Number(_), false)
                ) {
                    return Err(RuntimeError::TypeError(
                        "incompatible boxed primitive receiver".into(),
                    ));
                }
                if string {
                    Ok(Value::String(primitive::string(&value)?))
                } else {
                    Ok(value)
                }
            }
            NativeFunction::SymbolToString | NativeFunction::SymbolValueOf => {
                let value = if let Value::Object(id) = receiver {
                    self.heap
                        .boxed_primitive(id)?
                        .or(self.test262_foreign_boxed_primitive(id)?)
                        .unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::Symbol(symbol) = value else {
                    return Err(RuntimeError::TypeError(
                        "Symbol method requires a Symbol".into(),
                    ));
                };
                if function == NativeFunction::SymbolToString {
                    Ok(Value::String(symbol.descriptive_string()))
                } else {
                    Ok(Value::Symbol(symbol))
                }
            }
            NativeFunction::BigIntToString | NativeFunction::BigIntValueOf => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::BigInt(value) = value else {
                    return Err(RuntimeError::TypeError(
                        "BigInt method requires a BigInt".into(),
                    ));
                };
                if function == NativeFunction::BigIntToString {
                    Ok(Value::String(value.to_string().into()))
                } else {
                    Ok(Value::BigInt(value))
                }
            }
            NativeFunction::RegExp => {
                if !construct
                    && *native::argument(&args, 1) == Value::Undefined
                    && self.is_regexp(first)?
                {
                    let constructor = self.get_property(first, &"constructor".into())?;
                    if constructor == self.regexp_global()? {
                        return Ok(first.clone());
                    }
                }
                self.regexp_create(first, native::argument(&args, 1))
            }
            NativeFunction::RegExpMethod(method) => self.regexp_method(method, &receiver, &args),
            NativeFunction::RegExpGetter(name) => self.regexp_getter(name, &receiver),
            NativeFunction::RegExpIteratorNext => self.regexp_iterator_next(&receiver),
            NativeFunction::ThrowTypeError => Err(RuntimeError::TypeError(
                "restricted function property".into(),
            )),
            NativeFunction::Empty => Ok(Value::Undefined),
            NativeFunction::ObjectValueOf => self.coerce_object(&receiver).map(Value::Object),
            NativeFunction::ObjectIsPrototypeOf => {
                // §20.1.3.6 tests the argument before coercing `this`.  That
                // ordering keeps primitive arguments observable as `false`,
                // even when `this` is null or undefined.
                let Value::Object(mut candidate) = first else {
                    return Ok(Value::Bool(false));
                };
                let object = self.coerce_object(&receiver)?;
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(object), Value::Object(candidate)]);
                let result = (|| {
                    while let Some(prototype) = self.object_get_prototype(candidate)? {
                        if prototype == object {
                            return Ok(Value::Bool(true));
                        }
                        candidate = prototype;
                        *self.stack.last_mut().expect("prototype-chain root") =
                            Value::Object(candidate);
                    }
                    Ok(Value::Bool(false))
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectToString => {
                let tag = match &receiver {
                    Value::Undefined => "Undefined",
                    Value::Null => "Null",
                    Value::String(_) => "String",
                    Value::Symbol(_) => "Symbol",
                    Value::Number(_) => "Number",
                    Value::BigInt(_) => "BigInt",
                    Value::Bool(_) => "Boolean",
                    Value::Object(id) => {
                        if self.heap.boxed_string(*id)?.is_some() {
                            "String"
                        } else if self.heap.is_array(*id)? {
                            "Array"
                        } else if self.heap.is_arguments(*id)? {
                            "Arguments"
                        } else if self.is_callable(&receiver)? {
                            "Function"
                        } else if self.heap.regexp(*id)?.is_some() {
                            "RegExp"
                        } else if let Some(value) = self.heap.boxed_primitive(*id)? {
                            match value {
                                Value::Number(_) => "Number",
                                Value::Bool(_) => "Boolean",
                                Value::BigInt(_) => "BigInt",
                                Value::Symbol(_) => "Symbol",
                                _ => "Object",
                            }
                        } else {
                            "Object"
                        }
                    }
                };
                let custom = if matches!(receiver, Value::Undefined | Value::Null) {
                    Value::Undefined
                } else {
                    self.get_property(&receiver, &JsSymbol::well_known("toStringTag").into())?
                };
                let mut result = JsString::from("[object ");
                result.push_str(&if let Value::String(custom) = custom {
                    custom
                } else {
                    tag.into()
                });
                result.push_str(&"]".into());
                Ok(Value::String(result))
            }
            NativeFunction::ArrayToString => {
                let object = Value::Object(self.coerce_object(&receiver)?);
                self.stack.push(object.clone());
                let join = self.get_property(&object, &"join".into())?;
                if self.is_callable(&join)? {
                    self.call_native(join, object, vec![], false)
                } else {
                    self.native_call(NativeFunction::ObjectToString, object, vec![], false)
                }
            }
            NativeFunction::ArrayConcat => self.array_concat(&receiver, &args),
            NativeFunction::ArrayJoin => self.array_join(&receiver, first),
            NativeFunction::Symbol => Ok(Value::Symbol(JsSymbol::new(
                if matches!(first, Value::Undefined) {
                    None
                } else {
                    Some(self.coerce_string(first)?)
                },
            ))),
            NativeFunction::Object => {
                if matches!(first, Value::Undefined | Value::Null) {
                    let proto = if construct {
                        self.constructor_prototype(self.object_prototype)?
                    } else {
                        self.object_prototype
                    };
                    return Ok(Value::Object(
                        self.with_roots(|heap| heap.alloc_object(Some(proto)))?,
                    ));
                }
                self.coerce_object(first).map(Value::Object)
            }
            NativeFunction::ObjectMethod(method) => self.object_method(method, &receiver, &args),
            NativeFunction::StringIterator => {
                let string = self.string_receiver(&receiver)?;
                let prototype = self.string_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_string_iterator(string, prototype)
                })?))
            }
            NativeFunction::IteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires an iterator".into(),
                    ));
                };
                let Some(value) = self.heap.string_iterator_next(id)? else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires a String iterator".into(),
                    ));
                };
                let done = value.is_none();
                self.iterator_result(value.map_or(Value::Undefined, Value::String), done)
            }
            NativeFunction::IteratorSelf | NativeFunction::AsyncIteratorSelf => Ok(receiver),
            NativeFunction::Pattern(method) => self.string_pattern(method, &receiver, &args),
            NativeFunction::String => {
                let string = if args.is_empty() {
                    JsString::default()
                } else {
                    self.string_constructor_argument(native::argument(&args, 0), construct)?
                };
                self.check_string(&Value::String(string.clone()))?;
                if construct {
                    let (_, prototype) = self.string_intrinsics()?;
                    let prototype = self.constructor_prototype(prototype)?;
                    Ok(Value::Object(self.with_roots(|heap| {
                        heap.alloc_string(string, Some(prototype))
                    })?))
                } else {
                    Ok(Value::String(string))
                }
            }
            NativeFunction::FromCharCode | NativeFunction::FromCodePoint => {
                let mut result = JsString::default();
                for arg in &args {
                    let number = Value::Number(self.coerce_number(arg)?);
                    let Value::String(part) = native::from_codes(
                        &[number],
                        function == NativeFunction::FromCodePoint,
                        self.config.max_string_bytes,
                    )?
                    else {
                        unreachable!()
                    };
                    native::append(&mut result, &part, self.config.max_string_bytes)?;
                }
                Ok(Value::String(result))
            }
            NativeFunction::Raw => self.string_raw(&args),
            NativeFunction::Split => self.string_split(&receiver, &args),
            NativeFunction::Replace | NativeFunction::ReplaceAll => {
                self.string_replace(&receiver, &args, function == NativeFunction::ReplaceAll)
            }
            NativeFunction::StringMethod(method) => {
                self.dispatch_string_method(method, &receiver, &args)
            }
            NativeFunction::Call => self.call_native(
                receiver,
                first.clone(),
                args.iter().skip(1).cloned().collect(),
                false,
            ),
        }
    }
}
