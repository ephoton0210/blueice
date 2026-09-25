// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn promise_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.promise_prototype {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        // Building the methods below allocates native function objects. Keep
        // the freshly-created prototype rooted until its cache entry makes it
        // permanently reachable from the Realm.
        let base = self.stack.len();
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            let function_prototype = self.function_prototype()?;
            self.install_native(
                prototype,
                function_prototype,
                "then",
                2,
                NativeFunction::PromiseThen,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "catch",
                1,
                NativeFunction::PromiseCatch,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "finally",
                1,
                NativeFunction::PromiseFinally,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("Promise".into()),
                false,
                false,
                true,
            )
        })();
        self.stack.truncate(base);
        result?;
        self.promise_prototype = Some(prototype);
        Ok(prototype)
    }

    /// Map and Set have distinct ordinary prototypes and keyed-entry cores.
    pub(in super::super) fn collection_prototype(
        &mut self,
        map: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let cached = if map {
            self.map_prototype
        } else {
            self.set_prototype
        };
        if let Some(prototype) = cached {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String(if map { "Map" } else { "Set" }.into()),
                false,
                false,
                true,
            )?;
            if map {
                self.install_native_getter(
                    prototype,
                    function_prototype,
                    "size",
                    NativeFunction::MapSize,
                )?;
                for (name, length, method) in [
                    ("clear", 0, MapMethod::Clear),
                    ("delete", 1, MapMethod::Delete),
                    ("entries", 0, MapMethod::Entries),
                    ("forEach", 1, MapMethod::ForEach),
                    ("get", 1, MapMethod::Get),
                    ("getOrInsert", 2, MapMethod::GetOrInsert),
                    ("getOrInsertComputed", 2, MapMethod::GetOrInsertComputed),
                    ("has", 1, MapMethod::Has),
                    ("keys", 0, MapMethod::Keys),
                    ("set", 2, MapMethod::Set),
                    ("values", 0, MapMethod::Values),
                ] {
                    self.install_native(
                        prototype,
                        function_prototype,
                        name,
                        length,
                        NativeFunction::MapMethod(method),
                    )?;
                }
                // `Map.prototype[@@iterator]` is the same function object as
                // `Map.prototype.entries`.
                let entries = self.heap.get(prototype, "entries")?;
                self.define_data(
                    prototype,
                    JsSymbol::well_known("iterator"),
                    entries,
                    true,
                    false,
                    true,
                )?;
            } else {
                self.install_native_getter(
                    prototype,
                    function_prototype,
                    "size",
                    NativeFunction::SetSize,
                )?;
                for (name, length, method) in [
                    ("add", 1, SetMethod::Add),
                    ("clear", 0, SetMethod::Clear),
                    ("delete", 1, SetMethod::Delete),
                    ("entries", 0, SetMethod::Entries),
                    ("forEach", 1, SetMethod::ForEach),
                    ("has", 1, SetMethod::Has),
                    ("values", 0, SetMethod::Values),
                    ("union", 1, SetMethod::Union),
                    ("intersection", 1, SetMethod::Intersection),
                    ("difference", 1, SetMethod::Difference),
                    ("symmetricDifference", 1, SetMethod::SymmetricDifference),
                    ("isSubsetOf", 1, SetMethod::IsSubsetOf),
                    ("isSupersetOf", 1, SetMethod::IsSupersetOf),
                    ("isDisjointFrom", 1, SetMethod::IsDisjointFrom),
                ] {
                    self.install_native(
                        prototype,
                        function_prototype,
                        name,
                        length,
                        NativeFunction::SetMethod(method),
                    )?;
                }
                // `Set.prototype.keys` and `Set.prototype[@@iterator]` are the
                // same function object as `Set.prototype.values`.
                let values = self.heap.get(prototype, "values")?;
                self.define_data(prototype, "keys", values.clone(), true, false, true)?;
                self.define_data(
                    prototype,
                    JsSymbol::well_known("iterator"),
                    values,
                    true,
                    false,
                    true,
                )?;
            }
            Ok(())
        })();
        self.stack.pop();
        result?;
        if map {
            self.map_prototype = Some(prototype);
        } else {
            self.set_prototype = Some(prototype);
        }
        Ok(prototype)
    }

    pub(in super::super) fn collection_constructor(
        &mut self,
        map: bool,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                if map {
                    "Map constructor must be called with new"
                } else {
                    "Set constructor must be called with new"
                }
                .into(),
            ));
        }
        let default = self.collection_prototype(map)?;
        let prototype = self.constructor_prototype(default)?;
        let collection = if map {
            self.with_roots(|heap| heap.alloc_map(Some(prototype)))?
        } else {
            self.with_roots(|heap| heap.alloc_set(Some(prototype)))?
        };
        let base = self.stack.len();
        self.stack.push(Value::Object(collection));
        let result = (|| {
            let source = native::argument(args, 0).clone();
            if matches!(source, Value::Undefined | Value::Null) {
                return Ok(Value::Object(collection));
            }

            // Like WeakMap and WeakSet, obtain the adder after construction
            // and invoke that exact callable for every iterator value. Apart
            // from supporting the standard iterable constructor argument,
            // this keeps subclass and observable-adder semantics aligned with
            // the collection constructor algorithms.
            let adder = self.get_property(
                &Value::Object(collection),
                &(if map { "set" } else { "add" }).into(),
            )?;
            self.stack.push(adder.clone());
            if !self.is_callable(&adder)? {
                return Err(RuntimeError::TypeError(
                    "collection adder must be callable".into(),
                ));
            }

            self.stack.push(source.clone());
            let record = self.get_iterator(&source)?;
            self.stack.push(record.clone());
            let outcome = (|| {
                while let Some(entry) = self.iterator_step(&record, true)? {
                    let entry_base = self.stack.len();
                    let call_args = if map {
                        let Value::Object(entry) = entry else {
                            return Err(RuntimeError::TypeError(
                                "Map constructor entry must be an object".into(),
                            ));
                        };
                        let entry = Value::Object(entry);
                        self.stack.push(entry.clone());
                        let key = self.get_property(&entry, &"0".into())?;
                        self.stack.push(key.clone());
                        let value = self.get_property(&entry, &"1".into())?;
                        self.stack.push(value.clone());
                        vec![key, value]
                    } else {
                        self.stack.push(entry.clone());
                        vec![entry]
                    };
                    self.call_native(adder.clone(), Value::Object(collection), call_args, false)?;
                    self.stack.truncate(entry_base);
                }
                Ok(Value::Object(collection))
            })();
            if outcome.is_err() {
                let error_base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &outcome {
                    self.stack.push(value.clone());
                }
                let _ = self.iterator_close(&record);
                self.stack.truncate(error_base);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn map_method(
        &mut self,
        method: MapMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let Some(map) = receiver.object_id() else {
            return Err(RuntimeError::TypeError(
                "Map method requires a Map receiver".into(),
            ));
        };
        if !self.heap.is_map(map)? {
            return Err(RuntimeError::TypeError(
                "Map method requires a Map receiver".into(),
            ));
        }
        let key = native::argument(args, 0);
        match method {
            MapMethod::Clear => {
                self.heap.collection_clear(map)?;
                Ok(Value::Undefined)
            }
            MapMethod::Delete => Ok(Value::Bool(self.heap.map_delete(map, key)?)),
            MapMethod::Entries => self.collection_iterator(map, true, ArrayIteratorKind::Entries),
            MapMethod::ForEach => self.collection_for_each(map, true, args),
            MapMethod::Get => Ok(self.heap.map_get(map, key)?.unwrap_or(Value::Undefined)),
            MapMethod::GetOrInsert => {
                if let Some(value) = self.heap.map_get(map, key)? {
                    return Ok(value);
                }
                let value = native::argument(args, 1).clone();
                self.with_roots(|heap| heap.map_set(map, key.clone(), value.clone()))?;
                Ok(value)
            }
            MapMethod::GetOrInsertComputed => {
                let callback = native::argument(args, 1).clone();
                if !self.is_callable(&callback)? {
                    return Err(RuntimeError::TypeError(
                        "Map getOrInsertComputed callback must be callable".into(),
                    ));
                }
                if let Some(value) = self.heap.map_get(map, key)? {
                    return Ok(value);
                }
                // CanonicalizeKeyedCollectionKey: the callback observes +0
                // where the caller passed -0.
                let key = match key {
                    Value::Number(number) if *number == 0.0 => Value::Number(0.0),
                    other => other.clone(),
                };
                // The callback may allocate or mutate this very map. Keep
                // every input rooted, then overwrite whatever the callback
                // stored under the same key, as the upsert algorithm requires.
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(map), key.clone(), callback.clone()]);
                let value = self.call_native(callback, Value::Undefined, vec![key.clone()], false);
                let stored = value.and_then(|value| {
                    self.stack.push(value.clone());
                    self.with_roots(|heap| heap.map_set(map, key, value.clone()))?;
                    Ok(value)
                });
                self.stack.truncate(base);
                stored
            }
            MapMethod::Has => Ok(Value::Bool(self.heap.map_has(map, key)?)),
            MapMethod::Keys => self.collection_iterator(map, true, ArrayIteratorKind::Keys),
            MapMethod::Set => {
                self.with_roots(|heap| {
                    heap.map_set(map, key.clone(), native::argument(args, 1).clone())
                })?;
                Ok(Value::Object(map))
            }
            MapMethod::Values => self.collection_iterator(map, true, ArrayIteratorKind::Values),
        }
    }

    pub(in super::super) fn set_method(
        &mut self,
        method: SetMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let Some(set) = receiver.object_id() else {
            return Err(RuntimeError::TypeError(
                "Set method requires a Set receiver".into(),
            ));
        };
        if !self.heap.is_set(set)? {
            return Err(RuntimeError::TypeError(
                "Set method requires a Set receiver".into(),
            ));
        }
        let key = native::argument(args, 0);
        match method {
            SetMethod::Add => {
                self.with_roots(|heap| heap.set_add(set, key.clone()))?;
                Ok(Value::Object(set))
            }
            SetMethod::Clear => {
                self.heap.collection_clear(set)?;
                Ok(Value::Undefined)
            }
            SetMethod::Delete => Ok(Value::Bool(self.heap.set_delete(set, key)?)),
            SetMethod::Entries => self.collection_iterator(set, false, ArrayIteratorKind::Entries),
            SetMethod::ForEach => self.collection_for_each(set, false, args),
            SetMethod::Has => Ok(Value::Bool(self.heap.set_has(set, key)?)),
            SetMethod::Values => self.collection_iterator(set, false, ArrayIteratorKind::Values),
            SetMethod::Union
            | SetMethod::Intersection
            | SetMethod::Difference
            | SetMethod::SymmetricDifference
            | SetMethod::IsSubsetOf
            | SetMethod::IsSupersetOf
            | SetMethod::IsDisjointFrom => self.set_algebra_method(method, set, key),
        }
    }

    pub(in super::super) fn weak_ref_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.weak_ref_prototype {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("WeakRef".into()),
                false,
                false,
                true,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "deref",
                0,
                NativeFunction::WeakRefDeref,
            )?;
            Ok(())
        })();
        self.stack.pop();
        result?;
        self.weak_ref_prototype = Some(prototype);
        Ok(prototype)
    }

    pub(in super::super) fn weak_ref_constructor(
        &mut self,
        target: Value,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "WeakRef constructor must be called with new".into(),
            ));
        }
        if !self.can_hold_weakly(&target) {
            return Err(RuntimeError::TypeError(
                "WeakRef target cannot be held weakly".into(),
            ));
        }
        // `constructor_prototype` can invoke a user getter. Root the target
        // before that observable step, then keep it through this entire job
        // once construction has succeeded.
        let base = self.stack.len();
        self.stack.push(target.clone());
        let result = (|| {
            let default = self.weak_ref_prototype()?;
            let prototype = self.constructor_prototype(default)?;
            let weak_ref =
                self.with_roots(|heap| heap.alloc_weak_ref(target.clone(), Some(prototype)))?;
            self.keep_weak_target(&target);
            Ok(Value::Object(weak_ref))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn weak_ref_deref(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Some(weak_ref) = receiver.object_id() else {
            return Err(RuntimeError::TypeError(
                "WeakRef deref requires a WeakRef receiver".into(),
            ));
        };
        let target = self.heap.weak_ref_target(weak_ref).map_err(|_| {
            RuntimeError::TypeError("WeakRef deref requires a WeakRef receiver".into())
        })?;
        if let Some(target) = &target {
            self.keep_weak_target(target);
        }
        Ok(target.unwrap_or(Value::Undefined))
    }

    pub(in super::super) fn finalization_registry_prototype(
        &mut self,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.finalization_registry_prototype {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("FinalizationRegistry".into()),
                false,
                false,
                true,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "register",
                2,
                NativeFunction::FinalizationRegistryRegister,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "unregister",
                1,
                NativeFunction::FinalizationRegistryUnregister,
            )?;
            Ok(())
        })();
        self.stack.pop();
        result?;
        self.finalization_registry_prototype = Some(prototype);
        Ok(prototype)
    }

    pub(in super::super) fn finalization_registry_constructor(
        &mut self,
        cleanup_callback: Value,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "FinalizationRegistry constructor must be called with new".into(),
            ));
        }
        if !self.is_callable(&cleanup_callback)? {
            return Err(RuntimeError::TypeError(
                "FinalizationRegistry cleanup callback must be callable".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.push(cleanup_callback.clone());
        let result = (|| {
            let default = self.finalization_registry_prototype()?;
            let prototype = self.constructor_prototype(default)?;
            Ok(Value::Object(self.with_roots(|heap| {
                heap.alloc_finalization_registry(cleanup_callback, Some(prototype))
            })?))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn finalization_registry_register(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let registry = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "FinalizationRegistry register requires a registry receiver".into(),
            )
        })?;
        if !self.heap.is_finalization_registry(registry)? {
            return Err(RuntimeError::TypeError(
                "FinalizationRegistry register requires a registry receiver".into(),
            ));
        }
        let target = native::argument(args, 0).clone();
        let holdings = native::argument(args, 1).clone();
        // An explicit `undefined` unregister token means "none", exactly like
        // an omitted one; only another value is checked for weak holdability.
        let unregister_token = args
            .get(2)
            .cloned()
            .filter(|token| *token != Value::Undefined);
        if !self.can_hold_weakly(&target)
            || unregister_token
                .as_ref()
                .is_some_and(|token| !self.can_hold_weakly(token))
        {
            return Err(RuntimeError::TypeError(
                "FinalizationRegistry target or unregister token cannot be held weakly".into(),
            ));
        }
        if target == holdings {
            return Err(RuntimeError::TypeError(
                "FinalizationRegistry target and holdings must differ".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.push(Value::Object(registry));
        self.stack.push(target.clone());
        self.stack.push(holdings.clone());
        if let Some(token) = &unregister_token {
            self.stack.push(token.clone());
        }
        let result = self.with_roots(|heap| {
            heap.finalization_registry_register(registry, target, holdings, unregister_token)
        });
        self.stack.truncate(base);
        result?;
        Ok(Value::Undefined)
    }

    pub(in super::super) fn finalization_registry_unregister(
        &mut self,
        receiver: &Value,
        unregister_token: Value,
    ) -> Result<Value, RuntimeError> {
        let registry = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "FinalizationRegistry unregister requires a registry receiver".into(),
            )
        })?;
        if !self.heap.is_finalization_registry(registry)? {
            return Err(RuntimeError::TypeError(
                "FinalizationRegistry unregister requires a registry receiver".into(),
            ));
        }
        if !self.can_hold_weakly(&unregister_token) {
            return Err(RuntimeError::TypeError(
                "FinalizationRegistry unregister token cannot be held weakly".into(),
            ));
        }
        Ok(Value::Bool(self.with_roots(|heap| {
            heap.finalization_registry_unregister(registry, unregister_token)
        })?))
    }

    pub(in super::super) fn weak_collection_prototype(
        &mut self,
        map: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let cached = if map {
            self.weak_map_prototype
        } else {
            self.weak_set_prototype
        };
        if let Some(prototype) = cached {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String(if map { "WeakMap" } else { "WeakSet" }.into()),
                false,
                false,
                true,
            )?;
            let methods: &[(&str, u32, WeakCollectionMethod)] = if map {
                &[
                    ("delete", 1, WeakCollectionMethod::Delete),
                    ("get", 1, WeakCollectionMethod::Get),
                    ("getOrInsert", 2, WeakCollectionMethod::GetOrInsert),
                    (
                        "getOrInsertComputed",
                        2,
                        WeakCollectionMethod::GetOrInsertComputed,
                    ),
                    ("has", 1, WeakCollectionMethod::Has),
                    ("set", 2, WeakCollectionMethod::Set),
                ]
            } else {
                &[
                    ("add", 1, WeakCollectionMethod::Add),
                    ("delete", 1, WeakCollectionMethod::Delete),
                    ("has", 1, WeakCollectionMethod::Has),
                ]
            };
            for &(name, length, method) in methods {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::WeakCollectionMethod { map, method },
                )?;
            }
            Ok(())
        })();
        self.stack.pop();
        result?;
        if map {
            self.weak_map_prototype = Some(prototype);
        } else {
            self.weak_set_prototype = Some(prototype);
        }
        Ok(prototype)
    }

    pub(in super::super) fn weak_collection_constructor(
        &mut self,
        map: bool,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                if map {
                    "WeakMap constructor must be called with new"
                } else {
                    "WeakSet constructor must be called with new"
                }
                .into(),
            ));
        }
        let default = self.weak_collection_prototype(map)?;
        let prototype = self.constructor_prototype(default)?;
        let collection =
            self.with_roots(|heap| heap.alloc_weak_collection(map, Some(prototype)))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(collection));
        let result = (|| {
            let source = native::argument(args, 0).clone();
            if matches!(source, Value::Undefined | Value::Null) {
                return Ok(Value::Object(collection));
            }
            // The adder is observable: the constructor must obtain it once
            // from the freshly created collection, then call that exact
            // function for every iterator item. This preserves overridden
            // `set`/`add` methods and their abrupt-completion behaviour.
            let adder = self.get_property(
                &Value::Object(collection),
                &(if map { "set" } else { "add" }).into(),
            )?;
            self.stack.push(adder.clone());
            if !self.is_callable(&adder)? {
                return Err(RuntimeError::TypeError(
                    "weak collection adder must be callable".into(),
                ));
            }
            self.stack.push(source.clone());
            let record = self.get_iterator(&source)?;
            self.stack.push(record.clone());
            let outcome = (|| {
                while let Some(entry) = self.iterator_step(&record, true)? {
                    let entry_base = self.stack.len();
                    let call_args = if map {
                        let Value::Object(entry) = entry else {
                            return Err(RuntimeError::TypeError(
                                "weak collection entry must be an object".into(),
                            ));
                        };
                        let entry = Value::Object(entry);
                        self.stack.push(entry.clone());
                        let key = self.get_property(&entry, &"0".into())?;
                        self.stack.push(key.clone());
                        let value = self.get_property(&entry, &"1".into())?;
                        self.stack.push(value.clone());
                        vec![key, value]
                    } else {
                        self.stack.push(entry.clone());
                        vec![entry]
                    };
                    self.call_native(adder.clone(), Value::Object(collection), call_args, false)?;
                    self.stack.truncate(entry_base);
                }
                Ok(Value::Object(collection))
            })();
            if outcome.is_err() {
                let error_base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &outcome {
                    self.stack.push(value.clone());
                }
                let _ = self.iterator_close(&record);
                self.stack.truncate(error_base);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    fn can_hold_weakly(&self, key: &Value) -> bool {
        match key {
            Value::Object(_) => true,
            Value::Symbol(symbol) => !self
                .symbol_registry
                .borrow()
                .values()
                .any(|registered| registered == symbol),
            _ => false,
        }
    }

    fn keep_weak_target(&mut self, target: &Value) {
        if let Some(target) = target.object_id() {
            if !self.kept_weak_objects.contains(&target) {
                self.kept_weak_objects.push(target);
            }
        }
    }

    pub(in super::super) fn weak_collection_method(
        &mut self,
        map: bool,
        method: WeakCollectionMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let Some(collection) = receiver.object_id() else {
            return Err(RuntimeError::TypeError(
                "Weak collection method requires a Weak collection receiver".into(),
            ));
        };
        if !self.heap.is_weak_collection(collection, map)? {
            return Err(RuntimeError::TypeError(
                "Weak collection method requires a matching receiver".into(),
            ));
        }
        let key = native::argument(args, 0);
        match method {
            WeakCollectionMethod::Set | WeakCollectionMethod::Add => {
                if !self.can_hold_weakly(key) {
                    return Err(RuntimeError::TypeError(
                        "Weak collection key cannot be held weakly".into(),
                    ));
                }
                let value = if map {
                    native::argument(args, 1).clone()
                } else {
                    Value::Undefined
                };
                self.with_roots(|heap| heap.weak_collection_set(collection, key.clone(), value))?;
                Ok(Value::Object(collection))
            }
            WeakCollectionMethod::Get => {
                if !self.can_hold_weakly(key) {
                    return Ok(Value::Undefined);
                }
                Ok(self
                    .heap
                    .weak_collection_get(collection, key)?
                    .unwrap_or(Value::Undefined))
            }
            WeakCollectionMethod::GetOrInsert => {
                if !self.can_hold_weakly(key) {
                    return Err(RuntimeError::TypeError(
                        "Weak collection key cannot be held weakly".into(),
                    ));
                }
                if let Some(value) = self.heap.weak_collection_get(collection, key)? {
                    return Ok(value);
                }
                let value = native::argument(args, 1).clone();
                self.with_roots(|heap| {
                    heap.weak_collection_set(collection, key.clone(), value.clone())
                })?;
                Ok(value)
            }
            WeakCollectionMethod::GetOrInsertComputed => {
                let callback = native::argument(args, 1).clone();
                if !self.is_callable(&callback)? {
                    return Err(RuntimeError::TypeError(
                        "WeakMap getOrInsertComputed callback must be callable".into(),
                    ));
                }
                if !self.can_hold_weakly(key) {
                    return Err(RuntimeError::TypeError(
                        "Weak collection key cannot be held weakly".into(),
                    ));
                }
                if let Some(value) = self.heap.weak_collection_get(collection, key)? {
                    return Ok(value);
                }
                // The callback may allocate or mutate this very map. Keep
                // every observable input rooted, then overwrite a mutation
                // made for the same key as required by the upsert algorithm.
                let base = self.stack.len();
                self.stack.push(Value::Object(collection));
                self.stack.push(key.clone());
                self.stack.push(callback.clone());
                let value = self.call_native(callback, Value::Undefined, vec![key.clone()], false);
                let value = match value {
                    Ok(value) => value,
                    Err(error) => {
                        self.stack.truncate(base);
                        return Err(error);
                    }
                };
                let stored = self.with_roots(|heap| {
                    heap.weak_collection_set(collection, key.clone(), value.clone())
                });
                self.stack.truncate(base);
                stored?;
                Ok(value)
            }
            WeakCollectionMethod::Has => {
                if !self.can_hold_weakly(key) {
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(
                    self.heap.weak_collection_get(collection, key)?.is_some(),
                ))
            }
            WeakCollectionMethod::Delete => {
                if !self.can_hold_weakly(key) {
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(
                    self.heap.weak_collection_delete(collection, key)?,
                ))
            }
        }
    }

    pub fn run_promise_jobs(&mut self) -> Result<(), RuntimeError> {
        while !self.promise_jobs.is_empty() {
            // Each queued Promise reaction has its own execution context and
            // therefore its own instruction budget. A module continuation
            // deliberately leaves the ambient interpreter with zero fuel
            // while suspended; it must not starve the next job turn.
            self.remaining_instructions = self.config.instruction_budget;
            self.run_next_promise_job()?;
        }
        Ok(())
    }

    /// Runs a finite microtask checkpoint for an embedding-host task. A job
    /// may enqueue another job, so the caller's fixed bound limits total
    /// work rather than just the queue length at checkpoint entry.
    pub fn run_promise_jobs_bounded(&mut self, max_jobs: usize) -> Result<(), RuntimeError> {
        for _ in 0..max_jobs {
            if !self.run_next_promise_job()? {
                return Ok(());
            }
        }
        if self.promise_jobs.is_empty() {
            Ok(())
        } else {
            Err(RuntimeError::InstructionLimit)
        }
    }

    /// Execute exactly one Promise job.  Async functions and top-level await
    /// resume from a queued continuation, rather than draining later turns
    /// in the same checkpoint, so callers that need an await boundary can
    /// advance the queue one observable turn at a time.
    pub(in super::super) fn run_next_promise_job(&mut self) -> Result<bool, RuntimeError> {
        let Some(job) = self.promise_jobs.pop_front() else {
            return Ok(false);
        };
        // Promise jobs always execute in a new ECMAScript execution context.
        // A preceding top-level-await continuation can leave the ambient
        // interpreter with no fuel, but that must not turn the next queued
        // reaction into an instruction-limit failure.
        self.remaining_instructions = self.config.instruction_budget;
        // The queue no longer owns this job after pop_front. Keep every
        // heap edge in the active job visible to allocation safepoints until
        // it has either settled its target or scheduled its successor.
        let root_base = self.stack.len();
        match &job {
            PromiseJob::Reaction {
                target,
                handler,
                value,
                ..
            } => {
                match target {
                    ReactionTarget::Native(promise) => self.stack.push(Value::Object(*promise)),
                    ReactionTarget::Capability(capability) => self.stack.extend([
                        capability.promise.clone(),
                        capability.resolve.clone(),
                        capability.reject.clone(),
                    ]),
                }
                self.stack.push(handler.clone());
                self.stack.push(value.clone());
            }
            PromiseJob::FinalizationCleanup { callback, holdings } => {
                self.stack.push(callback.clone());
                self.stack.push(holdings.clone());
            }
            PromiseJob::Thenable {
                target,
                thenable,
                then,
            } => {
                self.stack.push(Value::Object(*target));
                self.stack.push(thenable.clone());
                self.stack.push(then.clone());
            }
            PromiseJob::DynamicImport { target, .. } => {
                self.stack.push(Value::Object(*target));
            }
            PromiseJob::ModuleAwait { value, .. } | PromiseJob::AsyncAwait { value, .. } => {
                self.stack.push(value.clone());
            }
            PromiseJob::AsyncGeneratorYield {
                generator,
                target,
                result,
                value,
                ..
            } => {
                self.stack.push(Value::Object(*generator));
                self.stack.push(Value::Object(*target));
                self.stack.push(Value::Object(*result));
                self.stack.push(value.clone());
            }
            PromiseJob::AsyncGeneratorDelegate {
                generator,
                target,
                value,
                ..
            } => {
                self.stack.push(Value::Object(*generator));
                self.stack.push(Value::Object(*target));
                self.stack.push(value.clone());
            }
        }
        let result: Result<(), RuntimeError> = (|| {
            match job {
                PromiseJob::Reaction {
                    target,
                    handler,
                    value,
                    fulfilled,
                } => {
                    // NewPromiseReactionJob: a missing handler passes the
                    // settlement through; either way the outcome is delivered
                    // through the reaction's resolve/reject.
                    let outcome = if self.is_callable(&handler)? {
                        self.call_native(handler, Value::Undefined, vec![value], false)
                    } else if fulfilled {
                        Ok(value)
                    } else {
                        Err(RuntimeError::Thrown(value))
                    };
                    match (target, outcome) {
                        (ReactionTarget::Native(promise), Ok(value)) => {
                            self.resolve_promise(promise, value)?
                        }
                        (ReactionTarget::Native(promise), Err(error)) => {
                            let error = self.error_value(error)?;
                            self.settle_promise(promise, PromiseStatus::Rejected(error))?;
                        }
                        (ReactionTarget::Capability(capability), Ok(value)) => {
                            self.stack.push(value.clone());
                            self.call_native(
                                capability.resolve,
                                Value::Undefined,
                                vec![value],
                                false,
                            )?;
                        }
                        (ReactionTarget::Capability(capability), Err(error)) => {
                            let error = self.error_value(error)?;
                            self.stack.push(error.clone());
                            self.call_native(
                                capability.reject,
                                Value::Undefined,
                                vec![error],
                                false,
                            )?;
                        }
                    }
                }
                PromiseJob::Thenable {
                    target,
                    thenable,
                    then,
                } => {
                    // NewPromiseResolveThenableJob: a fresh resolving pair
                    // (with its own [[AlreadyResolved]]) is handed to `then`;
                    // if that throws, the pair's reject function decides
                    // whether the rejection still counts.
                    let base = self.stack.len();
                    let outcome: Result<(), RuntimeError> = (|| {
                        let (resolve, reject) = self.promise_resolving_functions(target)?;
                        self.stack.extend([resolve.clone(), reject.clone()]);
                        if let Err(error) =
                            self.call_native(then, thenable, vec![resolve, reject.clone()], false)
                        {
                            let error = self.error_value(error)?;
                            self.stack.push(error.clone());
                            self.call_native(reject, Value::Undefined, vec![error], false)?;
                        }
                        Ok(())
                    })();
                    self.stack.truncate(base);
                    outcome?;
                }
                PromiseJob::DynamicImport {
                    target,
                    referrer,
                    specifier,
                    module_type,
                    phase,
                } => {
                    let result = self.dynamic_import_job(&referrer, &specifier, module_type, phase);
                    match result {
                        Ok(DynamicImportResult::Fulfilled(namespace)) => {
                            self.settle_promise(target, PromiseStatus::Fulfilled(namespace))?
                        }
                        Ok(DynamicImportResult::WaitingDeferred { namespace, modules }) => {
                            self.deferred_import_waiters.push(DeferredImportWaiter {
                                promise: target,
                                namespace,
                                pending: modules.into_iter().collect(),
                            });
                        }
                        Ok(DynamicImportResult::Waiting(module)) => {
                            self.module_import_waiters
                                .entry(module)
                                .or_default()
                                .push(target);
                        }
                        Err(error) => {
                            let error = self.error_value(error)?;
                            self.settle_promise(target, PromiseStatus::Rejected(error))?;
                        }
                    }
                }
                PromiseJob::ModuleAwait {
                    continuation,
                    value,
                    fulfilled,
                } => self.resume_module_await(continuation, value, fulfilled)?,
                PromiseJob::AsyncAwait {
                    continuation,
                    value,
                    fulfilled,
                } => self.resume_async_await(continuation, value, fulfilled)?,
                PromiseJob::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                    value,
                    fulfilled,
                } => {
                    self.finish_async_generator_yield(generator, target, result, value, fulfilled)?
                }
                PromiseJob::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                    value,
                    fulfilled,
                } => {
                    self.finish_async_generator_delegate(generator, target, kind, value, fulfilled)?
                }
                PromiseJob::FinalizationCleanup { callback, holdings } => {
                    // Cleanup callbacks are host jobs, not Promise reactions. A
                    // throwing callback is reported through this embedding's job
                    // runner but cannot resurrect or re-register the consumed
                    // cell.
                    self.call_native(callback, Value::Undefined, vec![holdings], false)?;
                }
            }
            Ok(())
        })();
        self.stack.truncate(root_base);
        result?;
        Ok(true)
    }
}
