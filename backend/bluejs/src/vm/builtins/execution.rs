// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn array_like_values(
        &mut self,
        value: &Value,
    ) -> Result<Vec<Value>, RuntimeError> {
        if !matches!(value, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "argument list must be an object".into(),
            ));
        }
        let length = self.get_property(value, &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut args = Vec::new();
        for index in 0..length {
            self.charge_step()?;
            let value = self.get_property(value, &index.to_string().into())?;
            self.stack.push(value.clone());
            args.push(value);
        }
        Ok(args)
    }

    pub(in super::super) fn base_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_base {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_symbol_native(
                prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::IteratorSelf,
            )?;
            self.install_symbol_native(
                prototype,
                function_prototype,
                "dispose",
                0,
                NativeFunction::IteratorDispose,
            )?;
            for (name, length, native) in [
                (
                    "map",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Map),
                ),
                (
                    "filter",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Filter),
                ),
                (
                    "take",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Take),
                ),
                (
                    "drop",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Drop),
                ),
                (
                    "includes",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Includes),
                ),
                (
                    "join",
                    1,
                    NativeFunction::IteratorHelper(native::IteratorHelperMethod::Join),
                ),
                ("toArray", 0, NativeFunction::IteratorToArray),
                ("forEach", 1, NativeFunction::IteratorForEach),
                ("every", 1, NativeFunction::IteratorEvery),
                ("some", 1, NativeFunction::IteratorSome),
                ("find", 1, NativeFunction::IteratorFind),
                ("reduce", 1, NativeFunction::IteratorReduce),
            ] {
                self.install_native(prototype, function_prototype, name, length, native)?;
            }
            self.install_iterator_to_string_tag_accessor(prototype, function_prototype)
        })();
        if let Err(error) = result {
            self.heap.unroot(root)?;
            return Err(error);
        }
        self.iterator_base = Some(prototype);
        Ok(prototype)
    }

    /// `%Iterator.prototype%` is shared by every iterator, including string
    /// iterators created while String intrinsics bootstrap under a deliberately
    /// tiny heap. Materialize the newest, independent helper on first
    /// observation so ordinary string iteration does not pay its native
    /// function/property allocation cost. Reads and descriptor queries both
    /// route through this boundary, preserving an ordinary data property.
    pub(in super::super) fn materialize_iterator_helper_property(
        &mut self,
        owner: ObjectId,
        key: &PropertyName,
    ) -> Result<(), RuntimeError> {
        if self.iterator_base != Some(owner)
            || self.heap.get_own_property_descriptor(owner, key)?.is_some()
        {
            return Ok(());
        }
        let (name, length, method) = if key == &PropertyName::from("flatMap") {
            ("flatMap", 1, native::IteratorHelperMethod::FlatMap)
        } else if key == &PropertyName::from("chunks") {
            ("chunks", 1, native::IteratorHelperMethod::Chunks)
        } else if key == &PropertyName::from("windows") {
            ("windows", 1, native::IteratorHelperMethod::Windows)
        } else {
            return Ok(());
        };
        let function_prototype = self.function_prototype()?;
        self.install_native(
            owner,
            function_prototype,
            name,
            length,
            NativeFunction::IteratorHelper(method),
        )
    }

    fn install_iterator_to_string_tag_accessor(
        &mut self,
        owner: ObjectId,
        function_prototype: ObjectId,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let getter = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::IteratorToStringTagGetter,
                    "get [Symbol.toStringTag]",
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(getter));
            self.define_data(
                getter,
                "name",
                Value::String("get [Symbol.toStringTag]".into()),
                false,
                false,
                true,
            )?;
            self.define_data(getter, "length", Value::Number(0.0), false, false, true)?;

            let setter = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::IteratorToStringTagSetter,
                    "set [Symbol.toStringTag]",
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(setter));
            self.define_data(
                setter,
                "name",
                Value::String("set [Symbol.toStringTag]".into()),
                false,
                false,
                true,
            )?;
            self.define_data(setter, "length", Value::Number(1.0), false, false, true)?;
            self.with_roots(|heap| {
                heap.define_own_property(
                    owner,
                    JsSymbol::well_known("toStringTag"),
                    PropertyDescriptor {
                        get: Some(Value::Object(getter)),
                        set: Some(Value::Object(setter)),
                        enumerable: Some(false),
                        configurable: Some(true),
                        ..PropertyDescriptor::default()
                    },
                )
            })?;
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn iterator_wrapper_prototype(
        &mut self,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_wrapper_prototype {
            return Ok(prototype);
        }
        let function_prototype = self.function_prototype()?;
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::IteratorWrapperNext,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "return",
                0,
                NativeFunction::IteratorWrapperReturn,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            self.heap.unroot(root)?;
            return Err(error);
        }
        self.iterator_wrapper_prototype = Some(prototype);
        Ok(prototype)
    }

    fn iterator_helper_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_helper_prototype {
            return Ok(prototype);
        }
        let function_prototype = self.function_prototype()?;
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::IteratorHelperNext,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "return",
                0,
                NativeFunction::IteratorHelperReturn,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("Iterator Helper".into()),
                false,
                false,
                true,
            )
        })();
        if let Err(error) = result {
            self.heap.unroot(root)?;
            return Err(error);
        }
        self.iterator_helper_prototype = Some(prototype);
        Ok(prototype)
    }

    pub(in super::super) fn iterator_from(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        if !matches!(value, Value::Object(_) | Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator.from requires an object or String".into(),
            ));
        }
        let iterator_method = self.get_method(value, &JsSymbol::well_known("iterator").into())?;
        let iterator = if iterator_method == Value::Undefined {
            self.coerce_object(value)?
        } else {
            let iterator = self.call_native(iterator_method, value.clone(), Vec::new(), false)?;
            iterator.object_id().ok_or_else(|| {
                RuntimeError::TypeError(
                    "Iterator.from iterator method must return an object".into(),
                )
            })?
        };
        self.stack.push(Value::Object(iterator));
        let result = (|| {
            let next = self.get_property(&Value::Object(iterator), &"next".into())?;
            let base = self.base_iterator_prototype()?;
            let mut prototype = Some(iterator);
            while let Some(current) = prototype {
                if current == base {
                    return Ok(Value::Object(iterator));
                }
                prototype = self.object_get_prototype(current)?;
            }
            let prototype = self.iterator_wrapper_prototype()?;
            Ok(Value::Object(self.with_roots(|heap| {
                heap.alloc_iterator_wrapper(iterator, next, prototype)
            })?))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn iterator_wrapper_next(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(wrapper) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator wrapper next requires an iterator wrapper".into(),
            ));
        };
        let Some((iterator, next)) = self.heap.iterator_wrapper(*wrapper)? else {
            return Err(RuntimeError::TypeError(
                "Iterator wrapper next requires an iterator wrapper".into(),
            ));
        };
        self.stack.push(receiver.clone());
        self.stack.push(next.clone());
        let result = self.call_native(next, Value::Object(iterator), Vec::new(), false);
        self.stack.pop();
        self.stack.pop();
        result
    }

    pub(in super::super) fn iterator_wrapper_return(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(wrapper) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator wrapper return requires an iterator wrapper".into(),
            ));
        };
        let Some((iterator, _)) = self.heap.iterator_wrapper(*wrapper)? else {
            return Err(RuntimeError::TypeError(
                "Iterator wrapper return requires an iterator wrapper".into(),
            ));
        };
        self.stack.push(receiver.clone());
        let result = (|| {
            let return_method = self.get_method(&Value::Object(iterator), &"return".into())?;
            if return_method == Value::Undefined {
                self.iterator_result(Value::Undefined, true)
            } else {
                self.call_native(return_method, Value::Object(iterator), Vec::new(), false)
            }
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn iterator_dispose(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let return_method = self.get_method(receiver, &"return".into())?;
        if return_method != Value::Undefined {
            self.call_native(return_method, receiver.clone(), Vec::new(), false)?;
        }
        Ok(Value::Undefined)
    }

    fn iterator_close_direct(&mut self, iterator: &Value) -> Result<(), RuntimeError> {
        let close = self.get_method(iterator, &"return".into())?;
        if close == Value::Undefined {
            return Ok(());
        }
        let result = self.call_native(close, iterator.clone(), Vec::new(), false)?;
        if !matches!(result, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "iterator return must return an object".into(),
            ));
        }
        Ok(())
    }

    fn direct_iterator_record(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::Object(iterator) = value else {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        };
        self.stack.push(value.clone());
        let result = (|| {
            let next = self.get_property(value, &"next".into())?;
            self.stack.push(next.clone());
            let record = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(record));
            self.with_roots(|heap| heap.set(record, "iterator", Value::Object(*iterator)))?;
            self.with_roots(|heap| heap.set(record, "next", next))?;
            self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
            self.stack.pop();
            self.stack.pop();
            Ok(Value::Object(record))
        })();
        self.stack.pop();
        result
    }

    fn callback_iterator_record(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        }
        if !self.is_callable(callback)? {
            // Terminal helpers validate their callback before GetIteratorDirect.
            // An invalid callback closes the receiver but must not observe
            // the cached `next` property.
            self.iterator_close_direct(receiver)?;
            return Err(RuntimeError::TypeError(
                "Iterator helper callback must be callable".into(),
            ));
        }
        self.direct_iterator_record(receiver)
    }

    fn iterator_flattenable_record(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        if !matches!(value, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator.flatMap mapper must return an object".into(),
            ));
        }
        let method = self.get_method(value, &JsSymbol::well_known("iterator").into())?;
        if method == Value::Undefined {
            return self.direct_iterator_record(value);
        }
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator.flatMap iterator method must return an object".into(),
            ));
        }
        self.direct_iterator_record(&iterator)
    }

    /// `Iterator.concat` observes each argument's iterator method eagerly,
    /// but it opens that iterator only when the result is advanced to that
    /// argument. Keep these private records in the ordinary state object so
    /// their object edges are traced without exposing public slots.
    pub(in super::super) fn iterator_concat(
        &mut self,
        items: &[Value],
    ) -> Result<Value, RuntimeError> {
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(record));
        let result = (|| {
            for (index, item) in items.iter().enumerate() {
                if !matches!(item, Value::Object(_)) {
                    return Err(RuntimeError::TypeError(
                        "Iterator.concat requires object arguments".into(),
                    ));
                }
                self.stack.push(item.clone());
                let open = self.get_method(item, &JsSymbol::well_known("iterator").into())?;
                self.stack.push(open.clone());
                if open == Value::Undefined {
                    return Err(RuntimeError::TypeError(
                        "Iterator.concat requires iterable arguments".into(),
                    ));
                }
                self.with_roots(|heap| {
                    heap.set(record, format!("concatIterable{index}"), item.clone())
                })?;
                self.with_roots(|heap| heap.set(record, format!("concatOpen{index}"), open))?;
                self.stack.pop();
                self.stack.pop();
            }
            self.with_roots(|heap| {
                heap.set(record, "concatLength", Value::Number(items.len() as f64))
            })?;
            self.with_roots(|heap| heap.set(record, "concatCurrent", Value::Undefined))?;
            let prototype = self.iterator_helper_prototype()?;
            self.with_roots(|heap| {
                heap.alloc_iterator_helper(
                    record,
                    Value::Undefined,
                    IteratorHelperKind::Concat,
                    0,
                    prototype,
                )
            })
            .map(Value::Object)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn iterator_zip(
        &mut self,
        iterables: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(iterables, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator.zip iterables must be an object".into(),
            ));
        }
        let (mode, padding_option) = self.iterator_zip_options(options)?;
        let metadata = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(metadata));
        self.stack.push(iterables.clone());
        let result = (|| {
            let outer_method =
                self.get_method(iterables, &JsSymbol::well_known("iterator").into())?;
            if outer_method == Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Iterator.zip iterables must be iterable".into(),
                ));
            }
            self.stack.push(outer_method.clone());
            let outer = self.call_native(outer_method, iterables.clone(), Vec::new(), false)?;
            self.stack.pop();
            if !matches!(outer, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "Iterator.zip iterable method must return an object".into(),
                ));
            }
            self.stack.push(outer.clone());
            let outer_record = self.direct_iterator_record(&outer)?;
            self.stack.pop();
            self.stack.push(outer_record.clone());
            let mut count = 0_u64;
            let stepped = (|| loop {
                let item = match self.iterator_step(&outer_record, true) {
                    Ok(item) => item,
                    // IteratorStepValue's own IfAbruptCloseIterators closes
                    // only the already-opened inner records: the outer
                    // iterables iterator's own step already produced this
                    // completion, so it must not be closed again.
                    Err(error) => {
                        let _ = self.iterator_zip_close_records(metadata, count);
                        return Err(error);
                    }
                };
                let Some(item) = item else { return Ok(()) };
                self.stack.push(item.clone());
                let inner = match self.iterator_flattenable_record(&item) {
                    Ok(inner) => inner,
                    // GetIteratorFlattenable's IfAbruptCloseIterators closes
                    // the already-opened inner records before the outer
                    // iterables iterator itself.
                    Err(error) => {
                        self.stack.pop();
                        let _ = self.iterator_zip_close_records(metadata, count);
                        let _ = self.iterator_close(&outer_record);
                        return Err(error);
                    }
                };
                self.stack.pop();
                self.stack.push(inner.clone());
                self.with_roots(|heap| heap.set(metadata, format!("zipRecord{count}"), inner))?;
                self.stack.pop();
                count += 1;
            })();
            self.stack.pop();
            stepped?;
            if mode == "longest" {
                if let Err(error) =
                    self.iterator_zip_collect_padding(metadata, count, &padding_option)
                {
                    let _ = self.iterator_zip_close_records(metadata, count);
                    return Err(error);
                }
            }
            self.with_roots(|heap| heap.set(metadata, "zipCount", Value::Number(count as f64)))?;
            self.with_roots(|heap| heap.set(metadata, "zipMode", Value::String(mode.into())))?;
            let prototype = self.iterator_helper_prototype()?;
            self.with_roots(|heap| {
                heap.alloc_iterator_helper(
                    metadata,
                    Value::Undefined,
                    IteratorHelperKind::Zip,
                    0,
                    prototype,
                )
            })
            .map(Value::Object)
        })();
        self.stack.truncate(base);
        result
    }

    fn iterator_zip_options(
        &mut self,
        options: &Value,
    ) -> Result<(&'static str, Value), RuntimeError> {
        if *options == Value::Undefined {
            return Ok(("shortest", Value::Undefined));
        }
        if !matches!(options, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator.zip options must be an object".into(),
            ));
        }
        self.stack.push(options.clone());
        let result = (|| {
            let mode = self.get_property(options, &"mode".into())?;
            let mode = match mode {
                Value::Undefined => "shortest",
                Value::String(ref mode) if mode == "shortest" => "shortest",
                Value::String(ref mode) if mode == "longest" => "longest",
                Value::String(ref mode) if mode == "strict" => "strict",
                _ => return Err(RuntimeError::TypeError("invalid Iterator.zip mode".into())),
            };
            let padding = if mode == "longest" {
                let padding = self.get_property(options, &"padding".into())?;
                if !matches!(padding, Value::Undefined | Value::Object(_)) {
                    return Err(RuntimeError::TypeError(
                        "Iterator.zip padding must be an object".into(),
                    ));
                }
                padding
            } else {
                Value::Undefined
            };
            Ok((mode, padding))
        })();
        self.stack.pop();
        result
    }

    /// `Iterator.zipKeyed` snapshots own keys, but obtains each descriptor
    /// immediately before reading the corresponding value. This preserves the
    /// observable [[OwnPropertyKeys]], [[GetOwnProperty]], and [[Get]] order
    /// for ordinary objects and Proxies alike.
    pub(in super::super) fn iterator_zip_keyed(
        &mut self,
        iterables: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(iterables_object) = iterables else {
            return Err(RuntimeError::TypeError(
                "Iterator.zipKeyed iterables must be an object".into(),
            ));
        };
        let (mode, padding_option) = self.iterator_zip_options(options)?;
        let metadata = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(metadata));
        self.stack.push(iterables.clone());
        let result = (|| {
            let mut count = 0_u64;
            let collected = (|| {
                for key in self.object_own_property_keys(*iterables_object)? {
                    let Some(descriptor) = self.object_get_own_property(*iterables_object, &key)?
                    else {
                        continue;
                    };
                    if descriptor.enumerable != Some(true) {
                        continue;
                    }
                    let value = self.get_property(iterables, &key)?;
                    if value == Value::Undefined {
                        continue;
                    }
                    self.stack.push(value.clone());
                    let record = self.iterator_flattenable_record(&value)?;
                    self.stack.pop();
                    self.stack.push(record.clone());
                    self.with_roots(|heap| {
                        heap.set(metadata, format!("zipRecord{count}"), record)
                    })?;
                    count += 1;
                    self.with_roots(|heap| {
                        heap.set(metadata, format!("zipKey{}", count - 1), key.value())
                    })?;
                    self.stack.pop();
                }
                if mode == "longest" {
                    self.iterator_zip_keyed_collect_padding(metadata, count, &padding_option)?;
                }
                self.with_roots(|heap| {
                    heap.set(metadata, "zipCount", Value::Number(count as f64))
                })?;
                self.with_roots(|heap| heap.set(metadata, "zipMode", Value::String(mode.into())))?;
                let prototype = self.iterator_helper_prototype()?;
                self.with_roots(|heap| {
                    heap.alloc_iterator_helper(
                        metadata,
                        Value::Undefined,
                        IteratorHelperKind::ZipKeyed,
                        0,
                        prototype,
                    )
                })
                .map(Value::Object)
            })();
            if let Err(error) = collected {
                // A construction failure is already a throw completion, so
                // IteratorCloseAll observes every remaining record but cannot
                // replace the original error with one raised by `return`.
                let _ = self.iterator_zip_close_records(metadata, count);
                return Err(error);
            }
            collected
        })();
        self.stack.truncate(base);
        result
    }

    fn iterator_zip_keyed_collect_padding(
        &mut self,
        metadata: ObjectId,
        count: u64,
        padding_option: &Value,
    ) -> Result<(), RuntimeError> {
        for index in 0..count {
            let value = if *padding_option == Value::Undefined {
                Value::Undefined
            } else {
                let key = self
                    .heap
                    .get_own(metadata, format!("zipKey{index}"))?
                    .expect("zipKeyed metadata stores every source key");
                let key = self.coerce_property_key(&key)?;
                self.get_property(padding_option, &key)?
            };
            self.stack.push(value.clone());
            self.with_roots(|heap| heap.set(metadata, format!("zipPadding{index}"), value))?;
            self.stack.pop();
        }
        Ok(())
    }

    fn iterator_zip_collect_padding(
        &mut self,
        metadata: ObjectId,
        count: u64,
        padding_option: &Value,
    ) -> Result<(), RuntimeError> {
        if *padding_option == Value::Undefined {
            for index in 0..count {
                self.with_roots(|heap| {
                    heap.set(metadata, format!("zipPadding{index}"), Value::Undefined)
                })?;
            }
            return Ok(());
        }
        self.stack.push(padding_option.clone());
        let result = (|| {
            let method =
                self.get_method(padding_option, &JsSymbol::well_known("iterator").into())?;
            if method == Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Iterator.zip padding must be iterable".into(),
                ));
            }
            self.stack.push(method.clone());
            let iterator = self.call_native(method, padding_option.clone(), Vec::new(), false)?;
            self.stack.pop();
            if !matches!(iterator, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "Iterator.zip padding iterator must return an object".into(),
                ));
            }
            self.stack.push(iterator.clone());
            let record = self.direct_iterator_record(&iterator)?;
            self.stack.pop();
            self.stack.push(record.clone());
            let mut exhausted = false;
            for index in 0..count {
                let value = if exhausted {
                    Value::Undefined
                } else if let Some(value) = self.iterator_step(&record, true)? {
                    value
                } else {
                    exhausted = true;
                    Value::Undefined
                };
                self.stack.push(value.clone());
                self.with_roots(|heap| heap.set(metadata, format!("zipPadding{index}"), value))?;
                self.stack.pop();
            }
            if !exhausted {
                self.iterator_close(&record)?;
            }
            self.stack.pop();
            Ok(())
        })();
        self.stack.pop();
        result
    }

    fn iterator_helper_create(
        &mut self,
        receiver: &Value,
        callback: &Value,
        kind: IteratorHelperKind,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        }
        if !self.is_callable(callback)? {
            // Iterator helpers close an object receiver when argument
            // validation fails, but must not observe its `next` property.
            self.iterator_close_direct(receiver)?;
            return Err(RuntimeError::TypeError(
                "Iterator helper callback must be callable".into(),
            ));
        }
        let record = self.direct_iterator_record(receiver)?;
        let record_id = record
            .object_id()
            .expect("direct iterator records are ordinary objects");
        self.stack.push(record);
        self.stack.push(callback.clone());
        let result = (|| {
            if kind == IteratorHelperKind::FlatMap {
                self.with_roots(|heap| heap.set(record_id, "flatMapInner", Value::Undefined))?;
            }
            let prototype = self.iterator_helper_prototype()?;
            self.with_roots(|heap| {
                heap.alloc_iterator_helper(record_id, callback.clone(), kind, 0, prototype)
            })
            .map(Value::Object)
        })();
        self.stack.pop();
        self.stack.pop();
        result
    }

    pub(in super::super) fn iterator_map(
        &mut self,
        receiver: &Value,
        mapper: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_helper_create(receiver, mapper, IteratorHelperKind::Map)
    }

    pub(in super::super) fn iterator_filter(
        &mut self,
        receiver: &Value,
        predicate: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_helper_create(receiver, predicate, IteratorHelperKind::Filter)
    }

    pub(in super::super) fn iterator_flat_map(
        &mut self,
        receiver: &Value,
        mapper: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_helper_create(receiver, mapper, IteratorHelperKind::FlatMap)
    }

    pub(in super::super) fn iterator_take(
        &mut self,
        receiver: &Value,
        limit: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_count_helper(receiver, limit, IteratorHelperKind::Take)
    }

    pub(in super::super) fn iterator_drop(
        &mut self,
        receiver: &Value,
        limit: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_count_helper(receiver, limit, IteratorHelperKind::Drop)
    }

    pub(in super::super) fn iterator_chunks(
        &mut self,
        receiver: &Value,
        chunk_size: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator.chunks requires an object receiver".into(),
            ));
        }
        self.stack.push(receiver.clone());
        let result = (|| {
            // Chunks accepts an integral Number only; it does not perform
            // ToNumber. Validate before obtaining the direct iterator's
            // observable `next` method.
            let count = match chunk_size {
                Value::Number(value)
                    if value.is_finite() && value.fract() == 0.0 && *value >= 1.0 =>
                {
                    if *value <= f64::from(u32::MAX) {
                        *value as u64
                    } else {
                        return self.close_direct_iterator_on_error(
                            receiver,
                            RuntimeError::RangeError("invalid Iterator.chunks size".into()),
                        );
                    }
                }
                Value::Number(value) if value.is_finite() && *value <= 0.0 => {
                    return self.close_direct_iterator_on_error(
                        receiver,
                        RuntimeError::RangeError("invalid Iterator.chunks size".into()),
                    )
                }
                _ => {
                    return self.close_direct_iterator_on_error(
                        receiver,
                        RuntimeError::TypeError(
                            "Iterator.chunks size must be an integral Number".into(),
                        ),
                    )
                }
            };
            let record = self.direct_iterator_record(receiver)?;
            let record_id = record
                .object_id()
                .expect("direct iterator records are ordinary objects");
            self.stack.push(record);
            let result = (|| {
                let prototype = self.iterator_helper_prototype()?;
                self.with_roots(|heap| {
                    heap.alloc_iterator_helper(
                        record_id,
                        Value::Undefined,
                        IteratorHelperKind::Chunks,
                        count,
                        prototype,
                    )
                })
                .map(Value::Object)
            })();
            self.stack.pop();
            result
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn iterator_windows(
        &mut self,
        receiver: &Value,
        window_size: &Value,
        undersized: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator.windows requires an object receiver".into(),
            ));
        }
        self.stack.push(receiver.clone());
        let result = (|| {
            // Windows has the same Number-only size validation as chunks,
            // and validates its `undersized` mode before reading `next`.
            let count = match window_size {
                Value::Number(value)
                    if value.is_finite() && value.fract() == 0.0 && *value >= 1.0 =>
                {
                    if *value <= f64::from(u32::MAX) {
                        *value as u64
                    } else {
                        return self.close_direct_iterator_on_error(
                            receiver,
                            RuntimeError::RangeError("invalid Iterator.windows size".into()),
                        );
                    }
                }
                Value::Number(value) if value.is_finite() && *value <= 0.0 => {
                    return self.close_direct_iterator_on_error(
                        receiver,
                        RuntimeError::RangeError("invalid Iterator.windows size".into()),
                    )
                }
                _ => {
                    return self.close_direct_iterator_on_error(
                        receiver,
                        RuntimeError::TypeError(
                            "Iterator.windows size must be an integral Number".into(),
                        ),
                    )
                }
            };
            let allow_partial = match undersized {
                Value::Undefined => false,
                Value::String(mode) if mode == "only-full" => false,
                Value::String(mode) if mode == "allow-partial" => true,
                _ => {
                    return self.close_direct_iterator_on_error(
                        receiver,
                        RuntimeError::TypeError(
                            "Iterator.windows undersized mode is invalid".into(),
                        ),
                    )
                }
            };
            let record = self.direct_iterator_record(receiver)?;
            let record_id = record
                .object_id()
                .expect("direct iterator records are ordinary objects");
            self.stack.push(record);
            let result = (|| {
                self.with_roots(|heap| heap.set(record_id, "windowsBuffer", Value::Undefined))?;
                self.with_roots(|heap| {
                    heap.set(record_id, "windowsAllowPartial", Value::Bool(allow_partial))
                })?;
                let prototype = self.iterator_helper_prototype()?;
                self.with_roots(|heap| {
                    heap.alloc_iterator_helper(
                        record_id,
                        Value::Undefined,
                        IteratorHelperKind::Windows,
                        count,
                        prototype,
                    )
                })
                .map(Value::Object)
            })();
            self.stack.pop();
            result
        })();
        self.stack.pop();
        result
    }

    fn iterator_count_helper(
        &mut self,
        receiver: &Value,
        limit: &Value,
        kind: IteratorHelperKind,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        }
        self.stack.push(receiver.clone());
        let result = (|| {
            // `take` deliberately coerces its limit before obtaining `next`.
            // Abrupt limit conversion and every invalid range close the
            // receiver without observing that property.
            let number = match self.coerce_number(limit) {
                Ok(number) => number,
                Err(error) => return self.close_direct_iterator_on_error(receiver, error),
            };
            if number.is_nan() || (number.is_finite() && number > 9_007_199_254_740_991.0) {
                return self.close_direct_iterator_on_error(
                    receiver,
                    RuntimeError::RangeError("invalid Iterator.take limit".into()),
                );
            }
            let integer = if number == 0.0 { 0.0 } else { number.trunc() };
            if integer < 0.0 {
                return self.close_direct_iterator_on_error(
                    receiver,
                    RuntimeError::RangeError("invalid Iterator.take limit".into()),
                );
            }
            // The public finite range ends at MAX_SAFE_INTEGER. `u64::MAX`
            // is therefore an unobservable sentinel for an infinite limit
            // under the VM's finite execution budget, without inflating every
            // heap object's representation for a per-helper Option field.
            let remaining = if integer.is_infinite() {
                u64::MAX
            } else {
                integer as u64
            };
            let record = self.direct_iterator_record(receiver)?;
            let record_id = record
                .object_id()
                .expect("direct iterator records are ordinary objects");
            self.stack.push(record);
            let result = (|| {
                let prototype = self.iterator_helper_prototype()?;
                self.with_roots(|heap| {
                    heap.alloc_iterator_helper(
                        record_id,
                        Value::Undefined,
                        kind,
                        remaining,
                        prototype,
                    )
                })
                .map(Value::Object)
            })();
            self.stack.pop();
            result
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn iterator_helper_next(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(helper) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator helper next requires an iterator helper".into(),
            ));
        };
        let Some(state) = self.heap.iterator_helper(*helper)? else {
            return Err(RuntimeError::TypeError(
                "Iterator helper next requires an iterator helper".into(),
            ));
        };
        if state.kind == IteratorHelperKind::Concat {
            return self.iterator_concat_next(*helper, receiver, &state);
        }
        if matches!(
            state.kind,
            IteratorHelperKind::Zip | IteratorHelperKind::ZipKeyed
        ) {
            return self.iterator_zip_next(*helper, receiver, &state);
        }
        if state.kind == IteratorHelperKind::Chunks {
            return self.iterator_chunks_next(*helper, receiver, &state);
        }
        if state.kind == IteratorHelperKind::Windows {
            return self.iterator_windows_next(*helper, receiver, &state);
        }
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        self.stack.push(receiver.clone());
        self.stack.push(Value::Object(state.record));
        self.stack.push(state.callback.clone());
        let record = Value::Object(state.record);
        let result = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(*helper))?;
            loop {
                if state.kind == IteratorHelperKind::Take && state.index == 0 {
                    self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
                    self.iterator_close(&record)?;
                    return self.iterator_result(Value::Undefined, true);
                }
                if state.kind == IteratorHelperKind::FlatMap {
                    let inner = self.heap.get_own(state.record, "flatMapInner")?;
                    if let Some(inner @ Value::Object(_)) = inner {
                        self.stack.push(inner.clone());
                        let next = self.iterator_step(&inner, true);
                        self.stack.pop();
                        if let Some(value) = next? {
                            self.stack.push(value.clone());
                            let result = self.iterator_result(value, false);
                            self.stack.pop();
                            return result;
                        }
                        self.with_roots(|heap| {
                            heap.set(state.record, "flatMapInner", Value::Undefined)
                        })?;
                    }
                }
                let Some(value) = self.iterator_step(&record, true)? else {
                    self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
                    return self.iterator_result(Value::Undefined, true);
                };
                match state.kind {
                    IteratorHelperKind::Concat => {
                        unreachable!("concat has a dedicated state machine")
                    }
                    IteratorHelperKind::Zip | IteratorHelperKind::ZipKeyed => {
                        unreachable!("zip has a dedicated state machine")
                    }
                    IteratorHelperKind::Chunks => {
                        unreachable!("chunks has a dedicated state machine")
                    }
                    IteratorHelperKind::Windows => {
                        unreachable!("windows has a dedicated state machine")
                    }
                    IteratorHelperKind::Take => {
                        self.with_roots(|heap| heap.consume_iterator_helper_take(*helper))?;
                        self.stack.push(value.clone());
                        let result = self.iterator_result(value, false);
                        self.stack.pop();
                        return result;
                    }
                    IteratorHelperKind::Drop => {
                        let remaining = self
                            .heap
                            .iterator_helper(*helper)?
                            .expect("iterator helper state remains live")
                            .index;
                        if remaining > 0 {
                            self.with_roots(|heap| heap.consume_iterator_helper_take(*helper))?;
                            continue;
                        }
                        self.stack.push(value.clone());
                        let result = self.iterator_result(value, false);
                        self.stack.pop();
                        return result;
                    }
                    IteratorHelperKind::Map => {
                        let callback_result = self.iterator_helper_callback(*helper, &value)?;
                        self.stack.pop();
                        self.stack.push(callback_result.clone());
                        let result = self.iterator_result(callback_result, false);
                        self.stack.pop();
                        return result;
                    }
                    IteratorHelperKind::Filter => {
                        let callback_result = self.iterator_helper_callback(*helper, &value)?;
                        let selected = match self.to_boolean(&callback_result) {
                            Ok(selected) => selected,
                            Err(error) => {
                                self.stack.pop();
                                return Err(error);
                            }
                        };
                        if selected {
                            let result = self.iterator_result(value, false);
                            self.stack.pop();
                            return result;
                        }
                        self.stack.pop();
                    }
                    IteratorHelperKind::FlatMap => {
                        let mapped = self.iterator_helper_callback(*helper, &value)?;
                        self.stack.pop();
                        self.stack.push(mapped.clone());
                        let inner = self.iterator_flattenable_record(&mapped);
                        self.stack.pop();
                        let inner = inner?;
                        self.stack.push(inner.clone());
                        self.with_roots(|heap| heap.set(state.record, "flatMapInner", inner))?;
                        self.stack.pop();
                    }
                }
            }
        })();
        if result.is_err() {
            self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
        } else if !self
            .heap
            .iterator_helper(*helper)?
            .is_some_and(|state| state.done)
        {
            self.with_roots(|heap| heap.leave_iterator_helper(*helper))?;
        }
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    fn iterator_concat_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            loop {
                if let Some(current @ Value::Object(_)) =
                    self.heap.get_own(state.record, "concatCurrent")?
                {
                    self.stack.push(current.clone());
                    let next = self.iterator_step(&current, true);
                    self.stack.pop();
                    if let Some(value) = next? {
                        self.stack.push(value.clone());
                        let result = self.iterator_result(value, false);
                        self.stack.pop();
                        return result;
                    }
                    self.with_roots(|heap| {
                        heap.set(state.record, "concatCurrent", Value::Undefined)
                    })?;
                    continue;
                }

                let current_state = self
                    .heap
                    .iterator_helper(helper)?
                    .expect("concat helper state remains live");
                let length = self
                    .heap
                    .get_own(state.record, "concatLength")?
                    .and_then(|value| match value {
                        Value::Number(value) => Some(value as u64),
                        _ => None,
                    })
                    .expect("concat state stores its argument count");
                if current_state.index >= length {
                    self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                    return self.iterator_result(Value::Undefined, true);
                }
                let index = current_state.index;
                let iterable = self
                    .heap
                    .get_own(state.record, format!("concatIterable{index}"))?
                    .expect("concat state stores every iterable");
                let open = self
                    .heap
                    .get_own(state.record, format!("concatOpen{index}"))?
                    .expect("concat state stores every iterator method");
                self.stack.push(iterable.clone());
                self.stack.push(open.clone());
                let opened = self.call_native(open, iterable, Vec::new(), false)?;
                self.stack.pop();
                self.stack.pop();
                if !matches!(opened, Value::Object(_)) {
                    return Err(RuntimeError::TypeError(
                        "Iterator.concat iterator method must return an object".into(),
                    ));
                }
                self.stack.push(opened.clone());
                let direct = self.direct_iterator_record(&opened)?;
                self.stack.pop();
                self.stack.push(direct.clone());
                self.with_roots(|heap| heap.set(state.record, "concatCurrent", direct))?;
                self.with_roots(|heap| heap.advance_iterator_helper(helper))?;
                self.stack.pop();
            }
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.close_concat_on_error(&metadata, error)
            }
        };
        self.stack.truncate(base);
        outcome
    }

    fn iterator_zip_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            let count = self
                .heap
                .get_own(state.record, "zipCount")?
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as u64),
                    _ => None,
                })
                .expect("zip metadata stores its source count");
            let mode = self
                .heap
                .get_own(state.record, "zipMode")?
                .expect("zip metadata stores its mode");
            let Value::String(mode) = mode else {
                unreachable!("zip mode is a string")
            };
            if count == 0 {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                return self.iterator_result(Value::Undefined, true);
            }
            let values_base = self.stack.len();
            let result = (|| {
                let mut values = Vec::new();
                let mut any_done = false;
                let mut all_done = true;
                for index in 0..count {
                    let record = self
                        .heap
                        .get_own(state.record, format!("zipRecord{index}"))?
                        .expect("zip metadata stores every source record");
                    self.stack.push(record.clone());
                    let step = self.iterator_step(&record, true);
                    self.stack.pop();
                    match step? {
                        Some(value) => {
                            all_done = false;
                            self.stack.push(value.clone());
                            values.push(Some(value));
                        }
                        None => {
                            any_done = true;
                            if mode == "strict" {
                                if index != 0 {
                                    self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                                    let _ = self.iterator_zip_close_records(state.record, count);
                                    return Err(RuntimeError::TypeError(
                                        "Iterator.zip strict sources have different lengths".into(),
                                    ));
                                }
                                for remaining in 1..count {
                                    let record = self
                                        .heap
                                        .get_own(state.record, format!("zipRecord{remaining}"))?
                                        .expect("zip metadata stores every source record");
                                    self.stack.push(record.clone());
                                    let step = self.iterator_step(&record, true);
                                    self.stack.pop();
                                    if step?.is_some() {
                                        self.with_roots(|heap| {
                                            heap.finish_iterator_helper(helper)
                                        })?;
                                        let _ =
                                            self.iterator_zip_close_records(state.record, count);
                                        return Err(RuntimeError::TypeError(
                                            "Iterator.zip strict sources have different lengths"
                                                .into(),
                                        ));
                                    }
                                }
                                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                                return self.iterator_result(Value::Undefined, true);
                            }
                            values.push(None);
                        }
                    }
                    if mode == "shortest" && any_done {
                        self.iterator_zip_close_records(state.record, count)?;
                        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                        return self.iterator_result(Value::Undefined, true);
                    }
                }
                if mode == "longest" {
                    if all_done {
                        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                        return self.iterator_result(Value::Undefined, true);
                    }
                    for (index, value) in values.iter_mut().enumerate() {
                        if value.is_none() {
                            let padding = self
                                .heap
                                .get_own(state.record, format!("zipPadding{index}"))?
                                .expect("zip metadata stores every padding value");
                            self.stack.push(padding.clone());
                            *value = Some(padding);
                        }
                    }
                }
                let values = values
                    .into_iter()
                    .map(|value| value.expect("non-longest modes have no completed source"))
                    .collect();
                let values = if state.kind == IteratorHelperKind::Zip {
                    self.array_from(values)?
                } else {
                    self.iterator_zip_keyed_results(state.record, values)?
                };
                self.with_roots(|heap| heap.set(state.record, "zipStarted", Value::Bool(true)))?;
                self.stack.push(values.clone());
                let result = self.iterator_result(values, false);
                self.stack.pop();
                result
            })();
            self.stack.truncate(values_base);
            result
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                let _ = self.iterator_zip_close_records(
                    state.record,
                    self.iterator_zip_count(state.record)?,
                );
                Err(error)
            }
        };
        self.stack.truncate(base);
        outcome
    }

    fn iterator_zip_keyed_results(
        &mut self,
        metadata: ObjectId,
        values: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let result = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(result));
        let outcome = (|| {
            for (index, value) in values.into_iter().enumerate() {
                let key = self
                    .heap
                    .get_own(metadata, format!("zipKey{index}"))?
                    .expect("zipKeyed metadata stores every source key");
                let key = self.coerce_property_key(&key)?;
                self.stack.push(value.clone());
                let defined = self.object_define_own_property(
                    result,
                    key,
                    PropertyDescriptor::data(value, true, true, true),
                )?;
                self.stack.pop();
                if !defined {
                    return Err(RuntimeError::TypeError(
                        "cannot define Iterator.zipKeyed result property".into(),
                    ));
                }
            }
            Ok(Value::Object(result))
        })();
        self.stack.truncate(base);
        outcome
    }

    fn iterator_zip_count(&self, metadata: ObjectId) -> Result<u64, RuntimeError> {
        self.heap
            .get_own(metadata, "zipCount")?
            .and_then(|value| match value {
                Value::Number(value) => Some(value as u64),
                _ => None,
            })
            .ok_or_else(|| RuntimeError::TypeError("invalid Iterator.zip state".into()))
    }

    fn iterator_zip_close_records(
        &mut self,
        metadata: ObjectId,
        count: u64,
    ) -> Result<(), RuntimeError> {
        let mut completion = None;
        for index in (0..count).rev() {
            let record = self
                .heap
                .get_own(metadata, format!("zipRecord{index}"))?
                .expect("zip metadata stores every source record");
            self.stack.push(record.clone());
            let close = self.iterator_close(&record);
            self.stack.pop();
            if let Err(error) = close {
                // IteratorCloseAll continues after an abrupt `return`. The
                // first such error becomes the completion; later close errors
                // cannot replace it, but their `return` methods still run.
                completion.get_or_insert(error);
            }
        }
        if let Some(error) = completion {
            return Err(error);
        }
        Ok(())
    }

    fn iterator_chunks_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let record = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(record.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            let values_base = self.stack.len();
            let chunk = (|| {
                let mut values = Vec::new();
                for _ in 0..state.index {
                    let Some(value) = self.iterator_step(&record, true)? else {
                        break;
                    };
                    self.stack.push(value.clone());
                    values.push(value);
                }
                if values.is_empty() {
                    return Ok(None);
                }
                self.array_from(values).map(Some)
            })();
            self.stack.truncate(values_base);
            let Some(chunk) = chunk? else {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                return self.iterator_result(Value::Undefined, true);
            };
            self.stack.push(chunk.clone());
            let result = self.iterator_result(chunk, false);
            self.stack.pop();
            result
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.close_iterator_on_error(&record, Err(error))
            }
        };
        self.stack.truncate(base);
        outcome
    }

    fn iterator_windows_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let record = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(record.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            let values_base = self.stack.len();
            let window = (|| {
                let allow_partial = matches!(
                    self.heap.get_own(state.record, "windowsAllowPartial")?,
                    Some(Value::Bool(true))
                );
                let prior = self.heap.get_own(state.record, "windowsBuffer")?;
                let mut values = Vec::new();
                if let Some(Value::Object(buffer)) = prior {
                    // A full prior window slides by one element. Its private
                    // buffer is ordinary traced storage, but every value is
                    // rooted while the next pull can allocate or invoke JS.
                    for index in 1..state.index {
                        let value = self
                            .heap
                            .get_own(buffer, index.to_string())?
                            .expect("window buffer has the requested length");
                        self.stack.push(value.clone());
                        values.push(value);
                    }
                    let Some(value) = self.iterator_step(&record, true)? else {
                        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                        return Ok(None);
                    };
                    self.stack.push(value.clone());
                    values.push(value);
                } else {
                    for _ in 0..state.index {
                        let Some(value) = self.iterator_step(&record, true)? else {
                            self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                            if allow_partial && !values.is_empty() {
                                return self.array_from(values).map(Some);
                            }
                            return Ok(None);
                        };
                        self.stack.push(value.clone());
                        values.push(value);
                    }
                }
                // A yielded full window becomes the next private sliding
                // buffer, while the caller receives a distinct Array.
                let buffer = self.array_from(values.clone())?;
                self.stack.push(buffer.clone());
                self.with_roots(|heap| heap.set(state.record, "windowsBuffer", buffer))?;
                self.stack.pop();
                self.array_from(values).map(Some)
            })();
            let Some(window) = window? else {
                self.stack.truncate(values_base);
                return self.iterator_result(Value::Undefined, true);
            };
            self.stack.push(window.clone());
            let result = self.iterator_result(window, false);
            self.stack.pop();
            self.stack.truncate(values_base);
            result
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.close_iterator_on_error(&record, Err(error))
            }
        };
        self.stack.truncate(base);
        outcome
    }

    fn close_concat_on_error<T>(
        &mut self,
        metadata: &Value,
        error: RuntimeError,
    ) -> Result<T, RuntimeError> {
        let Value::Object(metadata) = metadata else {
            unreachable!("concat metadata is an ordinary object")
        };
        let current = self.heap.get_own(*metadata, "concatCurrent")?;
        if let Some(current @ Value::Object(_)) = current {
            self.stack.push(current.clone());
            let result = self.close_iterator_on_error(&current, Err(error));
            self.stack.pop();
            result
        } else {
            Err(error)
        }
    }

    fn iterator_helper_callback(
        &mut self,
        helper: ObjectId,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let state = self
            .heap
            .iterator_helper(helper)?
            .expect("iterator helper state remains live");
        if state.index >= 9_007_199_254_740_991 {
            return Err(RuntimeError::RangeError(
                "Iterator helper index exceeds the safe integer range".into(),
            ));
        }
        self.stack.push(value.clone());
        let callback_result = self.call_native(
            state.callback.clone(),
            Value::Undefined,
            vec![value.clone(), Value::Number(state.index as f64)],
            false,
        );
        let callback_result = match callback_result {
            Ok(result) => result,
            Err(error) => {
                self.stack.pop();
                return Err(error);
            }
        };
        self.with_roots(|heap| heap.advance_iterator_helper(helper))?;
        Ok(callback_result)
    }

    fn close_direct_iterator_on_error<T>(
        &mut self,
        iterator: &Value,
        error: RuntimeError,
    ) -> Result<T, RuntimeError> {
        let base = self.stack.len();
        if let RuntimeError::Thrown(value) = &error {
            self.stack.push(value.clone());
        }
        let _ = self.iterator_close_direct(iterator);
        self.stack.truncate(base);
        Err(error)
    }

    pub(in super::super) fn iterator_includes(
        &mut self,
        receiver: &Value,
        search_element: &Value,
        skipped_elements: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        }
        self.stack.push(receiver.clone());
        self.stack.push(search_element.clone());
        let result = (|| {
            // `includes` intentionally does not apply ToNumber to its
            // optional skip count. It accepts only integral Numbers and
            // infinities, and validates before reading `next`.
            let to_skip = match skipped_elements {
                Value::Undefined => 0.0,
                Value::Number(number)
                    if !number.is_nan() && (number.is_infinite() || number.fract() == 0.0) =>
                {
                    *number
                }
                _ => {
                    return self.close_direct_iterator_on_error(
                        receiver,
                        RuntimeError::TypeError(
                            "Iterator.includes skippedElements must be an integral Number".into(),
                        ),
                    )
                }
            };
            if to_skip < 0.0 {
                return self.close_direct_iterator_on_error(
                    receiver,
                    RuntimeError::RangeError(
                        "Iterator.includes skippedElements must not be negative".into(),
                    ),
                );
            }
            if to_skip.is_finite() && to_skip > 9_007_199_254_740_991.0 {
                return self.close_direct_iterator_on_error(
                    receiver,
                    RuntimeError::RangeError(
                        "Iterator.includes skippedElements exceeds MAX_SAFE_INTEGER".into(),
                    ),
                );
            }
            let mut skipped = to_skip;
            let record = self.direct_iterator_record(receiver)?;
            self.stack.push(record.clone());
            let result = (|| {
                while let Some(value) = self.iterator_step(&record, true)? {
                    if skipped > 0.0 {
                        skipped -= 1.0;
                        continue;
                    }
                    if Self::same_value_zero(&value, search_element) {
                        self.iterator_close(&record)?;
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            })();
            self.stack.pop();
            self.close_iterator_on_error(&record, result)
        })();
        self.stack.pop();
        self.stack.pop();
        result
    }

    fn same_value_zero(left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Number(left), Value::Number(right)) => {
                left == right || (left.is_nan() && right.is_nan())
            }
            _ => left == right,
        }
    }

    pub(in super::super) fn iterator_join(
        &mut self,
        receiver: &Value,
        separator: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        }
        self.stack.push(receiver.clone());
        let result = (|| {
            // The separator is coerced before `next` is fetched. Its abrupt
            // conversion closes the direct iterator record without observing
            // that property.
            let separator = if *separator == Value::Undefined {
                ",".into()
            } else {
                match self.coerce_string(separator) {
                    Ok(separator) => separator,
                    Err(error) => return self.close_direct_iterator_on_error(receiver, error),
                }
            };
            let record = self.direct_iterator_record(receiver)?;
            self.stack.push(record.clone());
            let result = (|| {
                let mut result = JsString::default();
                let mut first = true;
                while let Some(value) = self.iterator_step(&record, true)? {
                    if !first {
                        native::append(&mut result, &separator, self.config.max_string_bytes)?;
                    }
                    first = false;
                    if !matches!(value, Value::Null | Value::Undefined) {
                        self.stack.push(value.clone());
                        let text = self.coerce_string(&value);
                        self.stack.pop();
                        let text = text?;
                        native::append(&mut result, &text, self.config.max_string_bytes)?;
                    }
                }
                Ok(Value::String(result))
            })();
            self.stack.pop();
            self.close_iterator_on_error(&record, result)
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn iterator_helper_return(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(helper) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator helper return requires an iterator helper".into(),
            ));
        };
        let Some(state) = self.heap.iterator_helper(*helper)? else {
            return Err(RuntimeError::TypeError(
                "Iterator helper return requires an iterator helper".into(),
            ));
        };
        if state.kind == IteratorHelperKind::Concat {
            return self.iterator_concat_return(*helper, receiver, &state);
        }
        if matches!(
            state.kind,
            IteratorHelperKind::Zip | IteratorHelperKind::ZipKeyed
        ) {
            return self.iterator_zip_return(*helper, receiver, &state);
        }
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        self.stack.push(receiver.clone());
        self.stack.push(Value::Object(state.record));
        let record = Value::Object(state.record);
        let result = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(*helper))?;
            if state.kind == IteratorHelperKind::FlatMap {
                if let Some(inner @ Value::Object(_)) =
                    self.heap.get_own(state.record, "flatMapInner")?
                {
                    self.stack.push(inner.clone());
                    let inner_result = self.iterator_close(&inner);
                    self.stack.pop();
                    if let Err(error) = inner_result {
                        return self.close_iterator_on_error(&record, Err(error));
                    }
                }
            }
            self.iterator_close(&record)?;
            self.iterator_result(Value::Undefined, true)
        })();
        self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
        self.stack.pop();
        self.stack.pop();
        result
    }

    fn iterator_concat_return(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata.clone());
        let result = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            if let Some(current @ Value::Object(_)) =
                self.heap.get_own(state.record, "concatCurrent")?
            {
                self.stack.push(current.clone());
                let close = self.iterator_close(&current);
                self.stack.pop();
                close?;
            }
            self.iterator_result(Value::Undefined, true)
        })();
        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
        self.stack.truncate(base);
        result
    }

    fn iterator_zip_return(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata);
        let result = (|| {
            let started = matches!(
                self.heap.get_own(state.record, "zipStarted")?,
                Some(Value::Bool(true))
            );
            if started {
                // Resuming a suspended yield runs the close handlers while
                // the helper is executing, so their re-entrant `next`/return
                // calls see the GeneratorValidate TypeError.
                self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
                let close = self.iterator_zip_close_records(
                    state.record,
                    self.iterator_zip_count(state.record)?,
                );
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                close?;
            } else {
                // GeneratorClose from suspended-start completes the helper
                // before the close handlers run. Re-entrant `next` therefore
                // returns the completed iterator result.
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.iterator_zip_close_records(
                    state.record,
                    self.iterator_zip_count(state.record)?,
                )?;
            }
            self.iterator_result(Value::Undefined, true)
        })();
        self.stack.truncate(base);
        result
    }

    fn iterator_callback(
        &mut self,
        callback: &Value,
        value: Value,
        index: u64,
    ) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Iterator helper callback must be callable".into(),
            ));
        }
        self.call_native(
            callback.clone(),
            Value::Undefined,
            vec![value, Value::Number(index as f64)],
            false,
        )
    }

    fn close_iterator_on_error<T>(
        &mut self,
        record: &Value,
        result: Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                // IteratorClose preserves an active iterator's observable
                // cleanup side effect. When a completion is already abrupt,
                // a later `return` failure cannot replace it.
                let base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                let _ = self.iterator_close(record);
                self.stack.truncate(base);
                Err(error)
            }
        }
    }

    pub(in super::super) fn iterator_to_array(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.direct_iterator_record(receiver)?;
        self.stack.push(record.clone());
        let result = (|| {
            let array = self.array_from(Vec::new())?;
            self.stack.push(array.clone());
            while let Some(value) = self.iterator_step(&record, true)? {
                self.array_push(&array, &value, 0)?;
            }
            self.stack.pop();
            Ok(array)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super) fn iterator_for_each(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                self.iterator_callback(callback, value, index)?;
                index += 1;
            }
            Ok(Value::Undefined)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super) fn iterator_every(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                let predicate = self.iterator_callback(callback, value, index)?;
                if !self.to_boolean(&predicate)? {
                    self.iterator_close(&record)?;
                    return Ok(Value::Bool(false));
                }
                index += 1;
            }
            Ok(Value::Bool(true))
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super) fn iterator_some(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                let predicate = self.iterator_callback(callback, value, index)?;
                if self.to_boolean(&predicate)? {
                    self.iterator_close(&record)?;
                    return Ok(Value::Bool(true));
                }
                index += 1;
            }
            Ok(Value::Bool(false))
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super) fn iterator_find(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                let predicate = self.iterator_callback(callback, value.clone(), index)?;
                if self.to_boolean(&predicate)? {
                    self.iterator_close(&record)?;
                    return Ok(value);
                }
                index += 1;
            }
            Ok(Value::Undefined)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super) fn iterator_reduce(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let callback = native::argument(args, 0);
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            if !self.is_callable(callback)? {
                return Err(RuntimeError::TypeError(
                    "Iterator helper callback must be callable".into(),
                ));
            }
            let mut index = 0_u64;
            let mut accumulator = if args.len() > 1 {
                args[1].clone()
            } else {
                let Some(value) = self.iterator_step(&record, true)? else {
                    return Err(RuntimeError::TypeError(
                        "cannot reduce an empty iterator without an initial value".into(),
                    ));
                };
                index = 1;
                value
            };
            while let Some(value) = self.iterator_step(&record, true)? {
                accumulator = self.call_native(
                    callback.clone(),
                    Value::Undefined,
                    vec![accumulator, value, Value::Number(index as f64)],
                    false,
                )?;
                index += 1;
            }
            Ok(accumulator)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super) fn iterator_to_string_tag_setter(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(receiver) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator prototype tag setter requires an object receiver".into(),
            ));
        };
        let base = self.base_iterator_prototype()?;
        if *receiver == base {
            return Err(RuntimeError::TypeError(
                "cannot assign Iterator.prototype Symbol.toStringTag".into(),
            ));
        }
        let key: PropertyName = JsSymbol::well_known("toStringTag").into();
        if self
            .heap
            .get_own_property_descriptor(*receiver, &key)?
            .is_none()
        {
            self.define_data(*receiver, key, value.clone(), true, true, true)?;
        } else {
            self.set_property(&Value::Object(*receiver), &key, value)?;
        }
        Ok(Value::Undefined)
    }

    pub(in super::super) fn iterator_constructor_setter(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(receiver) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator prototype constructor setter requires an object receiver".into(),
            ));
        };
        let base = self.base_iterator_prototype()?;
        if *receiver == base {
            return Err(RuntimeError::TypeError(
                "cannot assign Iterator.prototype constructor".into(),
            ));
        }
        let key: PropertyName = "constructor".into();
        if self
            .heap
            .get_own_property_descriptor(*receiver, &key)?
            .is_none()
        {
            // SetterThatIgnoresPrototypeProperties creates an own data
            // property instead of recursively invoking the accessor found on
            // %Iterator.prototype%.
            self.define_data(*receiver, key, value.clone(), true, true, true)?;
        } else {
            self.set_property(&Value::Object(*receiver), &key, value)?;
        }
        Ok(Value::Undefined)
    }

    pub(in super::super) fn template_object(
        &mut self,
        site: &crate::bytecode::TemplateSite,
    ) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.templates.get(&site.id) {
            return Ok(Value::Object(id));
        }
        let base = self.stack.len();
        let result = (|| {
            for string in site.raw.iter().chain(site.cooked.iter().flatten()) {
                self.check_string(&Value::String(string.clone()))?;
            }
            let raw = self.array_from(site.raw.iter().cloned().map(Value::String).collect())?;
            self.stack.push(raw.clone());
            let cooked = self.array_from(
                site.cooked
                    .iter()
                    .cloned()
                    .map(|s| s.map_or(Value::Undefined, Value::String))
                    .collect(),
            )?;
            self.stack.push(cooked.clone());
            self.define_data(
                cooked.object_id().unwrap(),
                "raw",
                raw.clone(),
                false,
                false,
                false,
            )?;
            for array in [&raw, &cooked] {
                let id = array.object_id().unwrap();
                for key in self.heap.own_property_keys(id)? {
                    let mut desc = self.heap.get_own_property_descriptor(id, &key)?.unwrap();
                    desc.writable = Some(false);
                    desc.configurable = Some(false);
                    self.with_roots(|heap| heap.define_own_property(id, key, desc))?;
                }
                self.heap.prevent_extensions(id)?;
            }
            self.heap.root(cooked.object_id().unwrap())?;
            self.templates.insert(site.id, cooked.object_id().unwrap());
            Ok(cooked)
        })();
        self.stack.truncate(base);
        result
    }
    pub(in super::super) fn get_iterator(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let method = self.get_method(value, &JsSymbol::well_known("iterator").into())?;
        self.get_iterator_from_method(value, method)
    }

    pub(in super::super) fn get_iterator_from_method(
        &mut self,
        value: &Value,
        method: Value,
    ) -> Result<Value, RuntimeError> {
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError("iterator must be an object".into()));
        }
        self.stack.push(iterator.clone());
        let next = self.get_property(&iterator, &"next".into())?;
        self.stack.push(next.clone());
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(record));
        self.with_roots(|heap| heap.set(record, "iterator", iterator))?;
        self.with_roots(|heap| heap.set(record, "next", next))?;
        self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(record))
    }

    /// GetAsyncIterator first observes @@asyncIterator and uses the ordinary
    /// iterator protocol as an AsyncFromSync fallback. The caller awaits the
    /// returned `.next()` result, so both paths share one record shape.
    pub(in super::super) fn get_async_iterator(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let method = self.get_method(value, &JsSymbol::well_known("asyncIterator").into())?;
        if method == Value::Undefined {
            let record = self.get_iterator(value)?;
            let Value::Object(record_id) = record else {
                unreachable!("GetIterator creates an iterator record")
            };
            self.stack.push(Value::Object(record_id));
            let result =
                self.with_roots(|heap| heap.set(record_id, "asyncFromSync", Value::Bool(true)));
            self.stack.pop();
            result?;
            return Ok(Value::Object(record_id));
        }
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "async iterator must be an object".into(),
            ));
        }
        self.stack.push(iterator.clone());
        let next = self.get_property(&iterator, &"next".into())?;
        self.stack.push(next.clone());
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(record));
        self.with_roots(|heap| heap.set(record, "iterator", iterator))?;
        self.with_roots(|heap| heap.set(record, "next", next))?;
        self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(record))
    }

    pub(in super::super) fn async_iterator_next(
        &mut self,
        record: &Value,
        argument: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return self.iterator_result(Value::Undefined, true);
        }
        let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
        let next = self.get_property(&Value::Object(*record), &"next".into())?;
        let result = self.call_native(next, iterator, argument.into_iter().collect(), false);
        if !matches!(
            self.heap.get_own(*record, "asyncFromSync")?,
            Some(Value::Bool(true))
        ) {
            return result;
        }
        match result {
            Ok(result) => self.async_from_sync_continue(*record, result),
            Err(error) => {
                let error = self.error_value(error)?;
                self.promise_reject(error)
            }
        }
    }

    pub(in super::super) fn async_from_sync_handler(
        &mut self,
        function: NativeFunction,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let id = self.with_roots(|heap| heap.alloc_native_function(function, "", prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String("".into()), false, false, true)?;
            self.define_data(id, "length", Value::Number(1.0), false, false, true)?;
            Ok(Value::Object(id))
        })();
        self.stack.pop();
        result
    }

    /// AsyncFromSyncIteratorContinuation. A synchronous iterator result's
    /// `value` is adopted through PromiseResolve before a for-await loop sees
    /// it; rejection closes the original iterator and rejects the public
    /// `next()` capability.
    pub(in super::super) fn async_from_sync_continue(
        &mut self,
        record: ObjectId,
        result: Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let outcome = (|| {
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator result must be an object".into(),
                ));
            }
            let done = self.get_property(&result, &"done".into())?;
            let value = self.get_property(&result, &"value".into())?;
            let value_wrapper = self.promise_resolve(value)?;
            let target = self.new_promise()?;
            self.stack
                .extend([value_wrapper.clone(), Value::Object(target)]);
            let fulfilled = self.async_from_sync_handler(NativeFunction::AsyncFromSyncFulfill {
                target,
                done: self.to_boolean(&done)?,
            })?;
            let rejected = self
                .async_from_sync_handler(NativeFunction::AsyncFromSyncReject { target, record })?;
            self.promise_then(&value_wrapper, &[fulfilled, rejected])?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                let error = self.error_value(error)?;
                self.promise_reject(error)
            }
        }
    }

    /// Invoke an ordinary iterator's `next` method while retaining the raw
    /// iterator result for synchronous `yield*`. The following bytecode
    /// instruction validates `done`/`value`, matching the split async path.
    pub(in super::super) fn iterator_next(
        &mut self,
        record: &Value,
        argument: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return self.iterator_result(Value::Undefined, true);
        }
        let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
        let next = self.get_property(&Value::Object(*record), &"next".into())?;
        self.call_native(next, iterator, argument.into_iter().collect(), false)
    }

    pub(in super::super) fn async_iterator_step(
        &mut self,
        record: &Value,
        result: &Value,
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        let outcome = (|| {
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "async iterator result must be an object".into(),
                ));
            }
            let done = self.get_property(result, &"done".into())?;
            if self.to_boolean(&done)? {
                Ok(None)
            } else {
                self.get_property(result, &"value".into()).map(Some)
            }
        })();
        if !matches!(&outcome, Ok(Some(_))) {
            let base = self.stack.len();
            if let Err(RuntimeError::Thrown(value)) = &outcome {
                self.stack.push(value.clone());
            }
            let marked = self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)));
            self.stack.truncate(base);
            marked?;
        }
        outcome
    }

    /// IteratorStepValue, or IteratorStep without IteratorValue for elisions.
    /// Iterator-origin errors complete this record before outer unwinding.
    pub(in super::super) fn iterator_step(
        &mut self,
        record: &Value,
        read_value: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return Ok(None);
        }
        let outcome = (|| {
            let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
            let next = self.get_property(&Value::Object(*record), &"next".into())?;
            let result = self.call_native(next, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator result must be an object".into(),
                ));
            }
            self.stack.push(result.clone());
            let done = self.get_property(&result, &"done".into())?;
            let value = if self.to_boolean(&done)? {
                None
            } else if read_value {
                Some(self.get_property(&result, &"value".into())?)
            } else {
                Some(Value::Undefined)
            };
            self.stack.pop();
            Ok(value)
        })();
        if !matches!(&outcome, Ok(Some(_))) {
            let base = self.stack.len();
            if let Err(RuntimeError::Thrown(value)) = &outcome {
                self.stack.push(value.clone());
            }
            let marked = self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)));
            self.stack.truncate(base);
            marked?;
        }
        outcome
    }

    pub(in super::super) fn iterator_close(&mut self, record: &Value) -> Result<(), RuntimeError> {
        let id = record
            .object_id()
            .expect("compiler only emits iterator records");
        if matches!(self.heap.get_own(id, "done")?, Some(Value::Bool(true))) {
            return Ok(());
        }
        self.with_roots(|heap| heap.set(id, "done", Value::Bool(true)))?;
        let iterator = self.get_property(record, &"iterator".into())?;
        let close = self.get_method(&iterator, &"return".into())?;
        if close != Value::Undefined {
            let result = self.call_native(close, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator return must return an object".into(),
                ));
            }
        }
        Ok(())
    }
    pub(in super::super) fn binding_value(
        &mut self,
        slot: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        if let Some(cell) = self.cells.get(&slot) {
            Ok(self.heap.get_own(*cell, "value")?)
        } else {
            Ok(self.bindings[slot].clone())
        }
    }

    pub(in super::super) fn store_binding(
        &mut self,
        slot: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            self.store_global_cell(cell, value)?;
        } else {
            self.bindings[slot] = Some(value);
        }
        Ok(())
    }

    pub(in super::super) fn capture(&mut self, slot: usize) -> Result<ObjectId, RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            return Ok(cell);
        }
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        self.cells.insert(slot, cell);
        if let Some(value) = self.bindings[slot].take() {
            self.with_roots(|heap| heap.set(cell, "value", value))?;
        }
        Ok(cell)
    }

    pub(in super::super) fn call_closure(
        &mut self,
        call: ClosureCall,
    ) -> Result<Value, RuntimeError> {
        let ClosureCall {
            code,
            captures,
            callee,
            receiver,
            args,
            construct,
            home,
            class_base,
        } = call;
        if code.class_constructor && !construct {
            // §10.2.1.1's class-constructor rejection is created in the
            // function's Realm. Materialize it before a Test262 membrane can
            // return from this VM, rather than letting the caller allocate a
            // same-named error in its own Realm.
            let error = self.error_value(RuntimeError::TypeError(
                "class constructor cannot be invoked without new".into(),
            ))?;
            return Err(RuntimeError::Thrown(error));
        }
        if construct && !code.constructible {
            return Err(RuntimeError::TypeError(
                "arrow function is not a constructor".into(),
            ));
        }
        // Async generator requests own their own Promise capabilities. Do not
        // allocate an ordinary async-function capability before entering the
        // generator branch, where it would be unreachable and leak state.
        let async_function = code.async_function && !code.generator;
        // AsyncFunctionStart creates the promise capability before executing
        // the body.  If the body reaches await, that same promise owns the
        // saved continuation; otherwise its immediate completion settles it.
        let async_promise = async_function.then(|| self.new_promise()).transpose()?;
        let receiver = if construct && code.derived_constructor {
            Value::Undefined
        } else if construct {
            let prototype = self.constructor_prototype(self.object_prototype)?;
            Value::Object(self.with_roots(|heap| heap.alloc_object(Some(prototype)))?)
        } else if code.arrow || code.strict {
            receiver
        } else if matches!(receiver, Value::Null | Value::Undefined) {
            self.global("globalThis")?
        } else {
            Value::Object(self.coerce_object(&receiver)?)
        };
        if code.generator {
            let async_generator = code.async_function;
            let default_prototype = if code.async_function {
                self.async_generator_prototype()?
            } else {
                self.generator_prototype()?
            };
            let prototype = self
                .get_property(&callee, &"prototype".into())?
                .object_id()
                .unwrap_or(default_prototype);
            if !code.generator_initializes_parameters {
                let state = GeneratorState::Start {
                    code,
                    captures,
                    callee,
                    receiver,
                    args,
                    home,
                };
                let generator = self.with_roots(|heap| heap.alloc_generator(state, prototype))?;
                if async_generator {
                    self.heap.enable_async_generator(generator)?;
                }
                return Ok(Value::Object(generator));
            }
            // Install a temporary state first so the generator owns every
            // captured edge while the entry phase may allocate. The actual
            // parameter frame replaces it below before the object escapes.
            let generator = self.with_roots(|heap| {
                heap.alloc_generator(
                    GeneratorState::Start {
                        code: code.clone(),
                        captures: captures.clone(),
                        callee: callee.clone(),
                        receiver: receiver.clone(),
                        args: args.clone(),
                        home,
                    },
                    prototype,
                )
            })?;
            if async_generator {
                self.heap.enable_async_generator(generator)?;
            }
            let base = self.stack.len();
            self.stack.push(Value::Object(generator));
            let state =
                self.initialize_generator(code, captures, callee.clone(), receiver, args, home);
            self.stack.truncate(base);
            let state = state?;
            // FunctionDeclarationInstantiation is observable to a parameter
            // initializer. Read `.prototype` again after it completes: a
            // default such as `(g.prototype = null)` must affect the freshly
            // created generator object's [[Prototype]].
            let prototype = self
                .get_property(&callee, &"prototype".into())?
                .object_id()
                .unwrap_or(default_prototype);
            self.heap.set_prototype(generator, Some(prototype))?;
            self.heap.set_generator_state(generator, state)?;
            return Ok(Value::Object(generator));
        }
        self.stack.push(receiver.clone());
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let frame_base = self.stack.len();
        let mut frame_bindings = vec![None; code.bindings.len()];
        if let Some(slot) = code.self_slot {
            frame_bindings[slot as usize] = Some(callee.clone());
        }
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let dynamic_eval_bindings = std::mem::take(&mut self.dynamic_eval_bindings);
        let mut dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        dynamic_eval_outer_bindings.push(dynamic_eval_bindings);
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let variable_scope = std::mem::replace(&mut self.variable_scope, code.variable_scope);
        let variable_scope_lexicals = std::mem::replace(
            &mut self.variable_scope_lexicals,
            code.scopes
                .get(code.variable_scope as usize)
                .into_iter()
                .flat_map(|scope| scope.iter())
                .filter_map(|slot| {
                    let binding = &code.bindings[*slot as usize];
                    binding.lexical.then(|| binding.name.clone())
                })
                .collect(),
        );
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        let frame_callee = std::mem::replace(&mut self.callee, callee.clone());
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, home);
        let next_field_initializer_depth = if code.arrow {
            self.class_field_initializer_depth
        } else {
            0
        };
        let class_field_initializer_depth = std::mem::replace(
            &mut self.class_field_initializer_depth,
            next_field_initializer_depth,
        );
        let derived_constructor_arrow = code.arrow && class_base.is_some();
        let class_constructor = std::mem::replace(
            &mut self.class_constructor,
            (code.class_constructor || derived_constructor_arrow).then(|| {
                callee
                    .object_id()
                    .expect("class and arrow closures are objects")
            }),
        );
        let pending_completions = self.pending_completions.clone();
        let completion_saves = self.completion_saves.clone();
        let with_objects = self.with_objects.clone();
        let frame_dynamic_eval_outer_bindings = self.dynamic_eval_outer_bindings.clone();
        let top_level_module = self.top_level_module;
        let remaining_instructions = self.remaining_instructions;
        let new_target = self.new_target.clone();
        let new_target_allowed = self.new_target_allowed;
        let active_module_name = self.active_module_name.clone();
        let result_root = self.result_root.take();
        let mut suspended_parent_stack = None;
        let mut suspended_async = None;
        let result = if async_function {
            let mut iterators = Vec::new();
            match self.interpret(&code, &mut iterators, 0, None, None, None) {
                Ok(InterpreterExit::Return(value)) => Ok(value),
                Ok(InterpreterExit::Await {
                    promise,
                    pc,
                    handlers,
                }) => {
                    let stack = self.stack.split_off(frame_base);
                    let mut execution = self.suspend_module_execution();
                    let parent_stack = std::mem::replace(&mut execution.stack, stack);
                    let templates = execution.templates.clone();
                    self.templates = templates;
                    let state = AsyncContinuation {
                        generator: None,
                        target: async_promise.expect("async function has a promise"),
                        code: code.clone(),
                        pc,
                        execution,
                        iterators,
                        handlers,
                        call_depth: self.call_depth,
                    };
                    suspended_parent_stack = Some(parent_stack);
                    suspended_async = Some((state, promise));
                    Ok(Value::Undefined)
                }
                Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                    "yield requires an async generator function".into(),
                )),
                Ok(InterpreterExit::Suspend { .. }) => {
                    unreachable!("ordinary async functions have no entry suspend")
                }
                Err(error) => {
                    let base = self.stack.len();
                    if let RuntimeError::Thrown(value) = &error {
                        self.stack.push(value.clone());
                    }
                    self.stack.extend(iterators.iter().cloned());
                    for record in iterators.into_iter().rev() {
                        let _ = self.iterator_close(&record);
                    }
                    self.stack.truncate(base);
                    Err(error)
                }
            }
        } else {
            self.run(&code)
        };
        let constructed = self.this.clone();
        let suspended = suspended_parent_stack.is_some();
        if let Some(stack) = suspended_parent_stack {
            self.stack = stack;
            self.result_root = result_root;
            self.pending_completions = pending_completions;
            self.completion_saves = completion_saves;
            self.with_objects = with_objects;
            self.dynamic_eval_outer_bindings = frame_dynamic_eval_outer_bindings;
            self.top_level_module = top_level_module;
            self.remaining_instructions = remaining_instructions;
            self.new_target = new_target;
            self.new_target_allowed = new_target_allowed;
            self.active_module_name = active_module_name;
        } else {
            self.result_root = result_root;
        }
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        let mut dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        self.dynamic_eval_bindings = dynamic_eval_outer_bindings
            .pop()
            .expect("callee inherits its caller dynamic environment");
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.script_global_slots = script_global_slots;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        // `super()` in a derived-constructor arrow initializes the enclosing
        // constructor's lexical `this` binding. Nested arrows propagate that
        // initialized receiver one frame at a time on return.
        self.this = if derived_constructor_arrow && matches!(constructed, Value::Object(_)) {
            constructed.clone()
        } else {
            this
        };
        self.arguments = arguments;
        self.callee = frame_callee;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.home_object = home_object;
        self.class_constructor = class_constructor;
        self.class_field_initializer_depth = class_field_initializer_depth;
        self.stack.truncate(base - 1);
        if let Some((state, awaited)) = suspended_async {
            self.suspend_async_await(state, awaited)?;
        }
        let result = result.and_then(|value| {
            if construct && !matches!(value, Value::Object(_)) {
                if code.derived_constructor && value != Value::Undefined {
                    return Err(RuntimeError::TypeError(
                        "derived constructor returned a non-object value".into(),
                    ));
                }
                if matches!(constructed, Value::Object(_)) {
                    Ok(constructed)
                } else {
                    Err(RuntimeError::ReferenceError(
                        "derived constructor did not call super()".into(),
                    ))
                }
            } else {
                Ok(value)
            }
        });
        if !async_function {
            return result;
        }

        let promise = async_promise.expect("async function has a promise");
        match result {
            // `AsyncFunctionStart` resolves rather than directly fulfills so
            // `return somePromise` adopts its eventual settlement.
            Ok(value) if !suspended => self.resolve_promise(promise, value)?,
            Ok(_) => {}
            Err(error) => {
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
            }
        }
        Ok(Value::Object(promise))
    }
}
