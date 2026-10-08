// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypedArrayCallbackMethod {
    Every,
    ForEach,
    Some,
    Find,
    FindIndex,
    FindLast,
    FindLastIndex,
    Map,
    Filter,
}

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/typed_array_boundaries.rs"]
mod boundary_tests;

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
        // A cold concrete constructor can allocate and collect before the
        // view exists. Retain its backing through that lazy initialization.
        let base = self.stack.len();
        self.stack.push(Value::Object(buffer));
        let result = (|| {
            let prototype = self.buffer_prototype(kind.name())?;
            self.with_roots(|heap| {
                heap.alloc_typed_array(buffer, 0, length, false, kind, Some(prototype))
            })
        })();
        self.stack.truncate(base);
        result
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
        // A species constructor reached through the Test262 membrane can be
        // a *forward* facade (a child realm's own constructor, observed
        // from its parent) or, symmetrically, a *reverse* facade (a
        // parent's constructor, observed from inside a child realm that
        // received it as an argument/property value) -- either one
        // produces a construction result that isn't owned by `self`'s own
        // realm, so both must be checked here. Missing the reverse case
        // previously misclassified such a constructor as "local",
        // producing a result whose real TypedArray data lived in another
        // realm's heap while this function's caller read it directly from
        // `self.heap` -- silently empty/zeroed data, not an error.
        //
        // The classification itself must be "is `constructor_id` a facade
        // at all", not "...specifically for a native TypedArray
        // constructor": a species constructor reaching across the membrane
        // can be an ordinary JS *subclass* of a real TypedArray constructor
        // (e.g. Test262's own `sm/non262-TypedArray-shell.js` synthesizes
        // exactly this -- a `class SharedTypedArray extends
        // Object.getPrototypeOf(baseConstructor)`), which has no
        // `NativeFunction::TypedArray` tag of its own even though
        // `[[Construct]]`ing it still produces a value belonging to the
        // other realm. The narrower native-function check missed this,
        // silently falling through to the same-realm construction path and
        // losing every mutation applied to the result afterward (the
        // round-trip identity cache in `test262_import_foreign_value`
        // discards it on the way back out). Validate the actual result's
        // internal slots below, independently of the constructor's brand.
        let is_foreign_constructor = self.test262_foreign_reference(constructor_id).is_some()
            || self.test262_reverse_reference(constructor_id).is_some();
        if !is_foreign_constructor {
            return Ok(None);
        }
        let target = self.call_with_target(
            constructor.clone(),
            Value::Undefined,
            vec![Value::Number(length as f64)],
            true,
            constructor.clone(),
        )?;
        // TypedArrayCreateFromConstructor validates internal slots, without
        // reading an observable length property. A user getter could otherwise
        // disguise a short result or detach a transported snapshot before its
        // copy. Forward facades retain storage in the child; reverse results
        // are concrete local snapshots and use the ordinary validation path.
        let target_id = target
            .object_id()
            .expect("successful membrane [[Construct]] preserves an object result");
        let actual = match self.test262_foreign_typed_array_info(target_id)? {
            Some((actual, _)) => {
                let (realm_id, foreign_target, _, _) = self
                    .test262_foreign_reference(target_id)
                    .expect("the pure shape check validated this retained foreign facade");
                self.test262_realms
                    .get(&realm_id)
                    .expect("the retained facade owns its realm")
                    .vm
                    .reject_immutable_typed_array(&Value::Object(foreign_target))?;
                actual
            }
            None => {
                self.reject_immutable_typed_array(&target)?;
                self.typed_array_receiver(&target)?.2
            }
        };
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

    fn typed_array_element(&self, object: ObjectId, index: usize) -> Value {
        // Indexed TypedArray iteration methods capture their iteration range
        // before invoking user callbacks. If a resizable backing buffer then
        // shrinks, each later missing integer-indexed element is observed as
        // `undefined`, rather than terminating that already-started loop.
        self.heap
            .typed_array_index_value(object, index)
            .expect("the validated and retained TypedArray owns its backing buffer")
            .unwrap_or(Value::Undefined)
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
        method: TypedArrayCallbackMethod,
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
            TypedArrayCallbackMethod::Every => {
                for index in 0..length {
                    let value = self.typed_array_element(object, index);
                    let result =
                        self.typed_array_callback(callback, &this_arg, value, index, object)?;
                    if !self
                        .to_boolean(&result)
                        .expect("callback returns a value validated in this realm")
                    {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            TypedArrayCallbackMethod::ForEach => {
                for index in 0..length {
                    let value = self.typed_array_element(object, index);
                    self.typed_array_callback(callback, &this_arg, value, index, object)?;
                }
                Ok(Value::Undefined)
            }
            TypedArrayCallbackMethod::Some => {
                for index in 0..length {
                    let value = self.typed_array_element(object, index);
                    let result =
                        self.typed_array_callback(callback, &this_arg, value, index, object)?;
                    if self
                        .to_boolean(&result)
                        .expect("callback returns a value validated in this realm")
                    {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            TypedArrayCallbackMethod::Find | TypedArrayCallbackMethod::FindIndex => {
                for index in 0..length {
                    let value = self.typed_array_element(object, index);
                    let result = self.typed_array_callback(
                        callback,
                        &this_arg,
                        value.clone(),
                        index,
                        object,
                    )?;
                    if self
                        .to_boolean(&result)
                        .expect("callback returns a value validated in this realm")
                    {
                        return Ok(if method == TypedArrayCallbackMethod::Find {
                            value
                        } else {
                            Value::Number(index as f64)
                        });
                    }
                }
                Ok(if method == TypedArrayCallbackMethod::Find {
                    Value::Undefined
                } else {
                    Value::Number(-1.0)
                })
            }
            TypedArrayCallbackMethod::FindLast | TypedArrayCallbackMethod::FindLastIndex => {
                for index in (0..length).rev() {
                    let value = self.typed_array_element(object, index);
                    let result = self.typed_array_callback(
                        callback,
                        &this_arg,
                        value.clone(),
                        index,
                        object,
                    )?;
                    if self
                        .to_boolean(&result)
                        .expect("callback returns a value validated in this realm")
                    {
                        return Ok(if method == TypedArrayCallbackMethod::FindLast {
                            value
                        } else {
                            Value::Number(index as f64)
                        });
                    }
                }
                Ok(if method == TypedArrayCallbackMethod::FindLast {
                    Value::Undefined
                } else {
                    Value::Number(-1.0)
                })
            }
            TypedArrayCallbackMethod::Map => {
                let (target, target_kind) =
                    self.typed_array_species_create(receiver, length, kind)?;
                self.stack.push(Value::Object(target));
                for index in 0..length {
                    let value = self.typed_array_element(object, index);
                    let value =
                        self.typed_array_callback(callback, &this_arg, value, index, object)?;
                    self.typed_array_write_values(target, target_kind, index, &[value])?;
                }
                Ok(Value::Object(target))
            }
            TypedArrayCallbackMethod::Filter => {
                let mut selected = Vec::new();
                for index in 0..length {
                    let value = self.typed_array_element(object, index);
                    let result = self.typed_array_callback(
                        callback,
                        &this_arg,
                        value.clone(),
                        index,
                        object,
                    )?;
                    if self
                        .to_boolean(&result)
                        .expect("callback returns a value validated in this realm")
                    {
                        selected.push(value);
                    }
                }
                let (target, target_kind) =
                    self.typed_array_species_create(receiver, selected.len(), kind)?;
                self.stack.push(Value::Object(target));
                self.typed_array_write_values(target, target_kind, 0, &selected)?;
                Ok(Value::Object(target))
            }
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
            if self
                .heap
                .typed_array_index_value(object, index)
                .expect("receiver validation retains the integer-indexed object")
                == Some(search.clone())
            {
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
            if equality(&self.typed_array_element(object, index), search) {
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
            if self
                .heap
                .typed_array_index_value(object, index)
                .expect("receiver validation retains the integer-indexed object")
                == Some(search.clone())
            {
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
            let value = self.typed_array_element(object, index);
            if !matches!(value, Value::Undefined | Value::Null) {
                let value = self.coerce_string(&value)
                    .expect("a decoded TypedArray element is a Number or BigInt; primitive string conversion cannot throw");
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
            self.typed_array_element(object, 0)
        };
        while index < length {
            let value = self.typed_array_element(object, index);
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
        let attempted_cross_realm_construction = foreign_target.is_some();
        let constructed = match foreign_target {
            Some(value) => value,
            None => Value::Object(self.typed_array_create(constructor.clone(), count)?.0),
        };
        let constructed_id = constructed
            .object_id()
            .expect("TypedArray construction returns an object");
        // A species constructor reached through a foreign *or* reverse
        // Test262 facade (`typed_array_create_foreign_target` recognizes
        // both) does not necessarily produce a facade result:
        // `test262_transport_value` eagerly snapshots a TypedArray crossing
        // realms into a genuine local object (see its own doc comment), so
        // a *reverse*-facade constructor's result crosses back as a real
        // local object even though the constructor itself was cross-realm.
        // The copy strategy below must check whether `constructed` really
        // is a live forward facade, not infer it from which construction
        // path was taken.
        let (is_live_foreign_target, target_kind) =
            match self.test262_foreign_typed_array_info(constructed_id).expect(
                "TypedArrayCreate validated this retained result; no JavaScript or allocation intervenes",
            ) {
                Some((_, target_kind)) => (true, target_kind),
                None => {
                    // Both constructor paths already validated brand, bounds,
                    // mutability and length. This pure read chooses a copy
                    // strategy, while invalid user results fail above.
                    let (_, _, _, target_kind) = self.heap.typed_array_info(constructed_id)
                        .expect("TypedArrayCreate validated this retained local result");
                    (false, target_kind)
                }
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
            return Ok(constructed);
        }
        let (source_buffer, source_offset, _, _) = self
            .heap
            .typed_array_info(object)
            .expect("validated TypedArray retains its backing");
        if is_live_foreign_target {
            if kind == target_kind {
                let target_buffer = self
                    .get_property(&constructed, &"buffer".into())?
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
                    .test262_foreign_reference(constructed_id)
                    .expect("foreign TypedArray construction retains its realm");
                self.test262_sync_foreign_buffer_mirrors(realm_id);
            } else {
                let values = self.typed_array_read_values(object, start, copy_count)
                    .expect("slice revalidated this source range after species construction; no callback intervened");
                let source = self.array_from(values)?;
                // Looking up a foreign set method can allocate a facade or
                // invoke a getter that collects this realm. Retain the new
                // argument array until the entire callback has returned.
                let base = self.stack.len();
                self.stack.extend([constructed.clone(), source.clone()]);
                let copied = (|| {
                    let set = self.get_property(&constructed, &"set".into())?;
                    self.call_native(set, constructed.clone(), vec![source], false)
                })();
                self.stack.truncate(base);
                copied?;
            }
            return Ok(constructed);
        }
        if attempted_cross_realm_construction {
            // `constructed` is a genuine local object here (the branch
            // above handles a live facade), but it was reached through a
            // *reverse* facade constructor -- `test262_reverse_call`'s
            // result crossed back into this realm via `test262_transport_
            // value`'s ordinary TypedArray snapshot, registered as a
            // round-trip stand-in for the *real* object in the parent's
            // own `imported_values` (`Test262Realm`, `vm.rs`). Mutating the
            // snapshot itself has no observable effect once this value
            // crosses back out: `test262_import_foreign_value`'s
            // round-trip cache unconditionally returns that real, original
            // object instead (see `TEST262_ANALYSIS_REPORT.md`'s
            // "Reverse-membrane round-trip cache" writeup for the full
            // trace this diagnosis came from). For the common
            // same-element-type case, write the bytes directly into the
            // *real* parent-owned buffer instead of the snapshot --
            // `test262_reverse_write_into_real_construction_result`
            // resolves it via the same `resolve_active` mechanism every
            // other cross-realm call in this membrane uses (sound here
            // because the parent is still registered active: this whole
            // call is nested inside `test262_foreign_typed_array_native_
            // call`'s own `register_active` scope). This also restores the
            // same bitwise, NaN-payload-preserving copy the same-realm and
            // live-foreign-facade branches above use.
            let mut real_write_done = false;
            if kind == target_kind {
                let constructor_wrapper = constructor
                    .object_id()
                    .expect("a cross-realm species constructor is an object");
                let byte_start = source_offset + start * kind.byte_width();
                let byte_length = copy_count * kind.byte_width();
                let bytes = self
                    .heap
                    .array_buffer_copy(source_buffer, byte_start, byte_length)
                    .expect("slice revalidated this source range after species construction; no callback intervened");
                real_write_done = self.test262_reverse_write_into_real_construction_result(
                    constructor_wrapper,
                    constructed_id,
                    0,
                    &bytes,
                )?;
                if real_write_done {
                    // The snapshot itself is still worth populating too --
                    // for a caller that observes `constructed` *before* it
                    // round-trips back out (e.g. more child-realm code
                    // running before this call returns), the snapshot is
                    // what it sees, not the real parent object. `array_
                    // buffer_write` takes the *buffer* object's id, not the
                    // TypedArray view's own id -- resolve it first.
                    let (local_buffer, local_offset, ..) = self
                        .heap
                        .typed_array_info(constructed_id)
                        .expect("local species results are validated or transported TypedArrays");
                    self.with_roots(|heap| {
                        heap.array_buffer_write(local_buffer, local_offset, &bytes)
                    }).expect("the fresh mutable transported snapshot has the validated species range; the real write runs no callbacks");
                }
            }
            if !real_write_done {
                // Either a differing element kind, or (defensively; not
                // expected in practice -- see this function's own doc
                // comment) the constructor wasn't actually a reverse
                // facade after all. Falls back to populating the local
                // snapshot only, which is still correct for any caller
                // that observes it before a round trip, and strictly
                // better than leaving it unpopulated.
                let values = self.typed_array_read_values(object, start, copy_count)
                    .expect("the reverse byte-write helper cannot invoke source callbacks or resize this source");
                for (index, value) in values.into_iter().enumerate() {
                    self.typed_array_write_values(constructed_id, target_kind, index, &[value])?;
                }
            }
            return Ok(constructed);
        }
        let target = constructed_id;
        let (target_buffer, target_offset, _, _) = self
            .heap
            .typed_array_info(target)
            .expect("species construction returned a validated TypedArray");
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
                .array_buffer_copy(source_buffer, byte_start, byte_length)
                .expect("slice revalidated this local source range after species construction");
            self.with_roots(|heap| heap.array_buffer_write(target_buffer, target_offset, &bytes))
                .expect("TypedArrayCreate validated this mutable destination and its minimum length; byte copying invokes no callbacks");
            return Ok(Value::Object(target));
        }
        // Read and write one element at a time. This preserves slice's
        // observable forward byte-copy behavior when a species result shares
        // the source buffer at a different byte offset.
        for index in 0..copy_count {
            let value = self.typed_array_element(object, start + index);
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
            let left = left.as_bigint().expect("BigInt storage decodes as BigInt");
            let right = right.as_bigint().expect("BigInt storage decodes as BigInt");
            return left.cmp(right);
        }
        let left = left.as_number().expect("numeric storage decodes as Number");
        let right = right
            .as_number()
            .expect("numeric storage decodes as Number");
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
        if left == 0.0 && right == 0.0 {
            return match (left.is_sign_negative(), right.is_sign_negative()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            };
        }
        left.partial_cmp(&right)
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
            let (buffer, _, _, _) = self
                .heap
                .typed_array_info(source)
                .expect("sort retains its validated source view");
            if self
                .heap
                .buffer_is_detached(buffer)
                .expect("source view retains its backing buffer")
            {
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
        let values = self
            .typed_array_read_values(object, 0, length)
            .expect("validated length bounds a snapshot without JavaScript");
        let target = self.typed_array_new_same_kind(length, kind)?;
        for (index, value) in values.into_iter().rev().enumerate() {
            self.typed_array_write_values(target, kind, index, &[value])
                .expect("fresh mutable target accepts decoded values");
        }
        Ok(Value::Object(target))
    }

    fn typed_array_to_sorted(
        &mut self,
        receiver: &Value,
        compare: &Value,
    ) -> Result<Value, RuntimeError> {
        let (object, length, kind) = self.typed_array_method_receiver(receiver)?;
        let mut values = self
            .typed_array_read_values(object, 0, length)
            .expect("validated length bounds a snapshot without JavaScript");
        self.typed_array_sort_values(&mut values, compare, kind, None)?;
        let target = self.typed_array_new_same_kind(length, kind)?;
        self.typed_array_write_values(target, kind, 0, &values)
            .expect("fresh mutable target accepts decoded values");
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
                .typed_array_index_value(object, index as usize)
                .expect("value coercion retains the validated receiver")
                .is_none()
        {
            return Err(RuntimeError::RangeError(
                "TypedArray index is outside its bounds".into(),
            ));
        }
        let values = self.typed_array_read_values(object, 0, length)?;
        let target = self.typed_array_new_same_kind(length, kind)?;
        self.typed_array_write_values(target, kind, 0, &values)
            .expect("with copies decoded primitives into its fresh mutable same-kind target without callbacks");
        self.typed_array_write_values(target, kind, index as usize, &[replacement])
            .expect("with converted the replacement before allocating its mutable target");
        Ok(Value::Object(target))
    }

    pub(super) fn typed_array_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
        method: TypedArrayMethod,
    ) -> Result<Value, RuntimeError> {
        match method {
            TypedArrayMethod::Every => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::Every)
            }
            TypedArrayMethod::ForEach => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::ForEach)
            }
            TypedArrayMethod::Some => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::Some)
            }
            TypedArrayMethod::Find => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::Find)
            }
            TypedArrayMethod::FindIndex => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::FindIndex)
            }
            TypedArrayMethod::FindLast => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::FindLast)
            }
            TypedArrayMethod::FindLastIndex => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::FindLastIndex)
            }
            TypedArrayMethod::Map => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::Map)
            }
            TypedArrayMethod::Filter => {
                self.typed_array_callback_method(receiver, args, TypedArrayCallbackMethod::Filter)
            }
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
                Ok(self.typed_array_element(object, index))
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
                let values = self.typed_array_read_values(object, start, count).expect("copyWithin bounds the current readable overlap");
                self.typed_array_write_values(object, kind, target, &values).expect("copyWithin validated mutable storage and decoded values");
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
                    self.with_roots(|heap| heap.typed_array_set_index(object, index, &value))
                        .expect("fill revalidated the mutable receiver after coercion and writes only its decoded primitive");
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
                    self.typed_array_element(object, index)
                };
                while index > 0 {
                    index -= 1;
                    let value = self.typed_array_element(object, index);
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
                let values = self.typed_array_read_values(object, 0, length).expect("validated length bounds the initial snapshot");
                let reversed: Vec<_> = values.into_iter().rev().collect();
                self.typed_array_write_values(object, kind, 0, &reversed).expect("mutable source accepts decoded elements without JavaScript");
                Ok(receiver.clone())
            }
            TypedArrayMethod::Slice => self.typed_array_slice(receiver, args),
            TypedArrayMethod::Sort => {
                let (object, length, kind) = self.typed_array_write_receiver(receiver)?;
                let mut values = self.typed_array_read_values(object, 0, length).expect("validated length bounds the initial snapshot");
                if self.typed_array_sort_values(
                    &mut values,
                    native::argument(args, 0),
                    kind,
                    Some(object),
                )? {
                    self.typed_array_write_values(object, kind, 0, &values).expect("mutable, attached source accepts decoded elements");
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
