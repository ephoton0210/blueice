// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super::super) fn array_like_values(
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

    pub(in super::super::super) fn base_iterator_prototype(
        &mut self,
    ) -> Result<ObjectId, RuntimeError> {
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
    pub(in super::super::super) fn materialize_iterator_helper_property(
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

    pub(in super::super::super) fn install_iterator_to_string_tag_accessor(
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

    pub(in super::super::super) fn iterator_wrapper_prototype(
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

    pub(in super::super::super) fn iterator_helper_prototype(
        &mut self,
    ) -> Result<ObjectId, RuntimeError> {
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

    pub(in super::super::super) fn iterator_from(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
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

    pub(in super::super::super) fn iterator_wrapper_next(
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

    pub(in super::super::super) fn iterator_wrapper_return(
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

    pub(in super::super::super) fn iterator_dispose(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let return_method = self.get_method(receiver, &"return".into())?;
        if return_method != Value::Undefined {
            self.call_native(return_method, receiver.clone(), Vec::new(), false)?;
        }
        Ok(Value::Undefined)
    }

    pub(in super::super::super) fn iterator_close_direct(
        &mut self,
        iterator: &Value,
    ) -> Result<(), RuntimeError> {
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

    pub(in super::super::super) fn direct_iterator_record(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
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

    pub(in super::super::super) fn callback_iterator_record(
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

    pub(in super::super::super) fn iterator_flattenable_record(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
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
    pub(in super::super::super) fn iterator_concat(
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

    pub(in super::super::super) fn iterator_zip(
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

    pub(in super::super::super) fn iterator_zip_options(
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
    pub(in super::super::super) fn iterator_zip_keyed(
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

    pub(in super::super::super) fn iterator_zip_keyed_collect_padding(
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

    pub(in super::super::super) fn iterator_zip_collect_padding(
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

    pub(in super::super::super) fn iterator_helper_create(
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

    pub(in super::super::super) fn iterator_map(
        &mut self,
        receiver: &Value,
        mapper: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_helper_create(receiver, mapper, IteratorHelperKind::Map)
    }

    pub(in super::super::super) fn iterator_filter(
        &mut self,
        receiver: &Value,
        predicate: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_helper_create(receiver, predicate, IteratorHelperKind::Filter)
    }

    pub(in super::super::super) fn iterator_flat_map(
        &mut self,
        receiver: &Value,
        mapper: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_helper_create(receiver, mapper, IteratorHelperKind::FlatMap)
    }

    pub(in super::super::super) fn iterator_take(
        &mut self,
        receiver: &Value,
        limit: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_count_helper(receiver, limit, IteratorHelperKind::Take)
    }

    pub(in super::super::super) fn iterator_drop(
        &mut self,
        receiver: &Value,
        limit: &Value,
    ) -> Result<Value, RuntimeError> {
        self.iterator_count_helper(receiver, limit, IteratorHelperKind::Drop)
    }

    pub(in super::super::super) fn iterator_chunks(
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

    pub(in super::super::super) fn iterator_windows(
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

    pub(in super::super::super) fn iterator_count_helper(
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
}
