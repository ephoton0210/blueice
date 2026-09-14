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
        )?;
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
            self.install_symbol_native(
                prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::CollectionIterator { map },
            )?;
            if map {
                self.install_native_getter(
                    prototype,
                    function_prototype,
                    "size",
                    NativeFunction::MapSize,
                )?;
                for (name, length, method) in [
                    ("delete", 1, MapMethod::Delete),
                    ("get", 1, MapMethod::Get),
                    ("has", 1, MapMethod::Has),
                    ("set", 2, MapMethod::Set),
                ] {
                    self.install_native(
                        prototype,
                        function_prototype,
                        name,
                        length,
                        NativeFunction::MapMethod(method),
                    )?;
                }
            } else {
                self.install_native_getter(
                    prototype,
                    function_prototype,
                    "size",
                    NativeFunction::SetSize,
                )?;
                for (name, length, method) in [
                    ("add", 1, SetMethod::Add),
                    ("delete", 1, SetMethod::Delete),
                    ("has", 1, SetMethod::Has),
                ] {
                    self.install_native(
                        prototype,
                        function_prototype,
                        name,
                        length,
                        NativeFunction::SetMethod(method),
                    )?;
                }
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

    fn collection_iterator_prototype(&mut self, map: bool) -> Result<ObjectId, RuntimeError> {
        let cached = if map {
            self.map_iterator_prototype
        } else {
            self.set_iterator_prototype
        };
        if let Some(prototype) = cached {
            return Ok(prototype);
        }
        let base = self.base_iterator_prototype()?;
        let function_prototype = self.function_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::CollectionIteratorNext,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String(if map { "Map Iterator" } else { "Set Iterator" }.into()),
                false,
                false,
                true,
            )?;
            Ok(prototype)
        })();
        match result {
            Ok(prototype) => {
                if map {
                    self.map_iterator_prototype = Some(prototype);
                } else {
                    self.set_iterator_prototype = Some(prototype);
                }
                Ok(prototype)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    pub(in super::super) fn collection_iterator(
        &mut self,
        map: bool,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "collection iterator requires an object receiver".into(),
            ));
        }
        let prototype = self.collection_iterator_prototype(map)?;
        Ok(Value::Object(
            self.with_roots(|heap| heap.alloc_object(Some(prototype)))?,
        ))
    }

    pub(in super::super) fn collection_constructor(
        &mut self,
        map: bool,
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
        self.stack.push(Value::Object(collection));
        self.stack.pop();
        Ok(Value::Object(collection))
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
            MapMethod::Delete => Ok(Value::Bool(self.heap.map_delete(map, key)?)),
            MapMethod::Get => Ok(self.heap.map_get(map, key)?.unwrap_or(Value::Undefined)),
            MapMethod::Has => Ok(Value::Bool(self.heap.map_has(map, key)?)),
            MapMethod::Set => {
                self.with_roots(|heap| {
                    heap.map_set(map, key.clone(), native::argument(args, 1).clone())
                })?;
                Ok(Value::Object(map))
            }
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
            SetMethod::Delete => Ok(Value::Bool(self.heap.set_delete(set, key)?)),
            SetMethod::Has => Ok(Value::Bool(self.heap.set_has(set, key)?)),
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
        let unregister_token = args.get(2).cloned();
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

    pub(in super::super) fn new_promise(&mut self) -> Result<ObjectId, RuntimeError> {
        let prototype = self.promise_prototype()?;
        let promise = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.promises.insert(
            promise,
            PromiseRecord {
                status: PromiseStatus::Pending,
                reactions: Vec::new(),
            },
        );
        Ok(promise)
    }

    /// Create one of the resolving functions belonging to a Promise
    /// capability. Their target is private native-function state rather than
    /// a JavaScript-visible property, which keeps `resolve.call(...)` and
    /// `reject.call(...)` correct.
    pub(in super::super) fn promise_resolving_function(
        &mut self,
        promise: ObjectId,
        fulfill: bool,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let function = NativeFunction::PromiseResolvingFunction { promise, fulfill };
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

    /// Execute `NewPromiseCapability(C)` for a constructor supplied to a
    /// static Promise method. The executor's captured resolve/reject pair is
    /// stored in a heap object because a user constructor receives it through
    /// normal JavaScript invocation rather than a private VM call path.
    pub(in super::super) fn new_promise_capability(
        &mut self,
        constructor: &Value,
    ) -> Result<(Value, Value, Value), RuntimeError> {
        if !self.is_constructor(constructor)? {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be a constructor".into(),
            ));
        }
        let base = self.stack.len();
        let storage = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(storage));
        let result = (|| {
            let prototype = self.function_prototype()?;
            let executor = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::PromiseCapabilityExecutor { storage },
                    "",
                    prototype,
                )
            })?;
            self.stack.push(Value::Object(executor));
            self.define_data(
                executor,
                "name",
                Value::String("".into()),
                false,
                false,
                true,
            )?;
            self.define_data(executor, "length", Value::Number(2.0), false, false, true)?;
            let promise = self.call_native(
                constructor.clone(),
                Value::Undefined,
                vec![Value::Object(executor)],
                true,
            )?;
            let Value::Object(_) = promise else {
                return Err(RuntimeError::TypeError(
                    "Promise constructor must return an object".into(),
                ));
            };
            self.stack.push(promise.clone());
            let resolve = self.get_property(&Value::Object(storage), &"resolve".into())?;
            let reject = self.get_property(&Value::Object(storage), &"reject".into())?;
            if !self.is_callable(&resolve)? || !self.is_callable(&reject)? {
                return Err(RuntimeError::TypeError(
                    "Promise constructor did not provide resolving functions".into(),
                ));
            }
            Ok((promise, resolve, reject))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn promise_resolve_constructor(
        &mut self,
        constructor: &Value,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        if value
            .object_id()
            .is_some_and(|promise| self.promises.contains_key(&promise))
            && self.get_property(&value, &"constructor".into())? == *constructor
        {
            return Ok(value);
        }
        let (promise, resolve, _) = self.new_promise_capability(constructor)?;
        let base = self.stack.len();
        self.stack
            .extend([promise.clone(), resolve.clone(), value.clone()]);
        let result = self.call_native(resolve, Value::Undefined, vec![value], false);
        self.stack.truncate(base);
        result?;
        Ok(promise)
    }

    pub(in super::super) fn promise_constructor(
        &mut self,
        executor: Value,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be called with new".into(),
            ));
        }
        if !self.is_callable(&executor)? {
            return Err(RuntimeError::TypeError(
                "Promise resolver is not a function".into(),
            ));
        }
        let promise = self.new_promise()?;
        // The capability must survive allocations for its resolving functions
        // and for executor invocation.
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let resolve = self.promise_resolving_function(promise, true)?;
            let reject = self.promise_resolving_function(promise, false)?;
            match self.call_native(executor, Value::Undefined, vec![resolve, reject], false) {
                Ok(_) => {}
                Err(RuntimeError::Thrown(value)) => {
                    self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                }
                Err(error) => {
                    let value = self.error_value(error)?;
                    self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                }
            }
            Ok(Value::Object(promise))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn promise_with_resolvers(&mut self) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let promise = self.new_promise()?;
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let resolve = self.promise_resolving_function(promise, true)?;
            let reject = self.promise_resolving_function(promise, false)?;
            self.stack.extend([resolve.clone(), reject.clone()]);
            let object_prototype = self.object_prototype;
            let capability = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(capability));
            self.define_data(
                capability,
                "promise",
                Value::Object(promise),
                true,
                true,
                true,
            )?;
            self.define_data(capability, "resolve", resolve, true, true, true)?;
            self.define_data(capability, "reject", reject, true, true, true)?;
            Ok(Value::Object(capability))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn settle_promise(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
    ) -> Result<(), RuntimeError> {
        let record = self
            .promises
            .get_mut(&promise)
            .ok_or(RuntimeError::TypeError("invalid Promise receiver".into()))?;
        if !matches!(record.status, PromiseStatus::Pending) {
            return Ok(());
        }
        let fulfilled = matches!(status, PromiseStatus::Fulfilled(_));
        let value = match &status {
            PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => value.clone(),
            PromiseStatus::Pending => unreachable!("Promise settlement is final"),
        };
        let reactions = std::mem::take(&mut record.reactions);
        record.status = status;
        self.promise_jobs
            .extend(reactions.into_iter().map(|reaction| match reaction {
                PromiseReaction::Then(reaction) => PromiseJob::Reaction {
                    target: reaction.target,
                    handler: if fulfilled {
                        reaction.on_fulfilled
                    } else {
                        reaction.on_rejected
                    },
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::ModuleAwait { continuation } => PromiseJob::ModuleAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncAwait { continuation } => PromiseJob::AsyncAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                } => PromiseJob::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                } => PromiseJob::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                    value: value.clone(),
                    fulfilled,
                },
            }));
        Ok(())
    }

    /// Resolve a capability from a value returned by user code. Promise
    /// reactions adopt another BlueJS Promise instead of fulfilling with the
    /// Promise object itself, which is essential for `then`, async functions
    /// and top-level await.
    pub(in super::super) fn resolve_promise(
        &mut self,
        promise: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if value.object_id() == Some(promise) {
            let error = self.error_object("TypeError", "Promise resolved with itself".into())?;
            return self.settle_promise(promise, PromiseStatus::Rejected(error));
        }
        let resolution = self.promise_resolve(value)?;
        let Value::Object(resolved) = resolution else {
            unreachable!("Promise.resolve always returns a promise")
        };
        let status = match self.promises.get(&resolved) {
            Some(PromiseRecord {
                status: PromiseStatus::Fulfilled(value),
                ..
            }) => Some((true, value.clone())),
            Some(PromiseRecord {
                status: PromiseStatus::Rejected(value),
                ..
            }) => Some((false, value.clone())),
            Some(PromiseRecord {
                status: PromiseStatus::Pending,
                ..
            }) => None,
            None => unreachable!("Promise.resolve returns a registered promise"),
        };
        if let Some((fulfilled, value)) = status {
            return self.settle_promise(
                promise,
                if fulfilled {
                    PromiseStatus::Fulfilled(value)
                } else {
                    PromiseStatus::Rejected(value)
                },
            );
        }
        self.promises
            .get_mut(&resolved)
            .expect("checked pending promise exists")
            .reactions
            .push(PromiseReaction::Then(PromiseThenReaction {
                target: promise,
                on_fulfilled: Value::Undefined,
                on_rejected: Value::Undefined,
            }));
        Ok(())
    }

    pub(in super::super) fn promise_then(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let promise = receiver
            .object_id()
            .filter(|id| self.promises.contains_key(id))
            .ok_or(RuntimeError::TypeError(
                "Promise.prototype.then receiver".into(),
            ))?;
        let target = self.new_promise()?;
        let reaction = PromiseThenReaction {
            target,
            on_fulfilled: native::argument(args, 0).clone(),
            on_rejected: native::argument(args, 1).clone(),
        };
        let status = {
            let record = self.promises.get_mut(&promise).unwrap();
            match &record.status {
                PromiseStatus::Pending => {
                    record.reactions.push(PromiseReaction::Then(reaction));
                    return Ok(Value::Object(target));
                }
                PromiseStatus::Fulfilled(value) => (true, value.clone()),
                PromiseStatus::Rejected(value) => (false, value.clone()),
            }
        };
        self.promise_jobs.push_back(PromiseJob::Reaction {
            target,
            handler: if status.0 {
                reaction.on_fulfilled
            } else {
                reaction.on_rejected
            },
            value: status.1,
            fulfilled: status.0,
        });
        Ok(Value::Object(target))
    }

    pub(in super::super) fn promise_catch(
        &mut self,
        receiver: &Value,
        reason_handler: &Value,
    ) -> Result<Value, RuntimeError> {
        self.promise_then(receiver, &[Value::Undefined, reason_handler.clone()])
    }

    pub(in super::super) fn promise_finally(
        &mut self,
        receiver: &Value,
        handler: &Value,
    ) -> Result<Value, RuntimeError> {
        // A non-callable handler already passes the original completion
        // through in `promise_then`. Callable handlers run on both paths;
        // reaction adoption is handled by the shared job queue.
        self.promise_then(receiver, &[handler.clone(), handler.clone()])
    }

    /// Implements the settled-value portion of Await. A pending promise needs
    /// a saved interpreter continuation, which remains a separate boundary.
    pub(in super::super) fn await_value(&self, value: Value) -> Result<Value, RuntimeError> {
        let Some(promise) = value.object_id() else {
            return Ok(value);
        };
        let Some(record) = self.promises.get(&promise) else {
            return Ok(value);
        };
        match &record.status {
            PromiseStatus::Pending => Err(RuntimeError::Unsupported("pending await continuation")),
            PromiseStatus::Fulfilled(value) => Ok(value.clone()),
            PromiseStatus::Rejected(value) => Err(RuntimeError::Thrown(value.clone())),
        }
    }

    pub(in super::super) fn promise_resolve(
        &mut self,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        // PromiseResolve(%Promise%, value) may return a native Promise only
        // after it observes `value.constructor`. That lookup is observable
        // (and may throw), including through AsyncFromSyncIteratorContinuation.
        // Keep `value` rooted while lazy intrinsic initialization or thenable
        // lookup can allocate.
        let base = self.stack.len();
        self.stack.push(value.clone());
        let outcome = (|| {
            let constructor = self.global("Promise")?;
            if value
                .object_id()
                .is_some_and(|promise| self.promises.contains_key(&promise))
                && self.get_property(&value, &"constructor".into())? == constructor
            {
                return Ok(value);
            }
            let promise = self.new_promise()?;
            let then = match &value {
                Value::Object(_) => self.get_property(&value, &"then".into()),
                _ => Ok(Value::Undefined),
            };
            match then {
                Ok(then) if self.is_callable(&then)? => {
                    self.promise_jobs.push_back(PromiseJob::Thenable {
                        target: promise,
                        thenable: value,
                        then,
                    });
                }
                Ok(_) => self.settle_promise(promise, PromiseStatus::Fulfilled(value))?,
                Err(error) => {
                    let error = self.error_value(error)?;
                    self.settle_promise(promise, PromiseStatus::Rejected(error))?;
                }
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super) fn promise_reject(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let promise = self.new_promise()?;
        self.settle_promise(promise, PromiseStatus::Rejected(value))?;
        Ok(Value::Object(promise))
    }

    pub(in super::super) fn promise_all_handler(
        &mut self,
        target: ObjectId,
        index: Option<u32>,
    ) -> Result<Value, RuntimeError> {
        let function = match index {
            Some(index) => NativeFunction::PromiseAllResolve { target, index },
            None => NativeFunction::PromiseAllReject { target },
        };
        self.promise_combinator_handler(function)
    }

    fn promise_combinator_handler(
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

    pub(in super::super) fn promise_all_settled(
        &mut self,
        target: ObjectId,
        index: u32,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let complete = {
            let Some(state) = self.promise_all.get_mut(&target) else {
                return Ok(());
            };
            let slot = state
                .values
                .get_mut(index as usize)
                .expect("Promise.all reaction index was allocated");
            if slot.is_some() {
                return Ok(());
            }
            *slot = Some(value);
            state.remaining -= 1;
            (state.remaining == 0).then(|| {
                state
                    .values
                    .iter()
                    .cloned()
                    .map(|value| value.expect("completed Promise.all has every value"))
                    .collect::<Vec<_>>()
            })
        };
        let Some(values) = complete else {
            return Ok(());
        };
        self.promise_all.remove(&target);
        let base = self.stack.len();
        self.stack.extend(values.iter().cloned());
        let values = self.array_from(values);
        self.stack.truncate(base);
        let values = values?;
        self.settle_promise(target, PromiseStatus::Fulfilled(values))
    }

    pub(in super::super) fn promise_all_reject(
        &mut self,
        target: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if self.promise_all.remove(&target).is_some() {
            self.settle_promise(target, PromiseStatus::Rejected(value))?;
        }
        Ok(())
    }

    pub(in super::super) fn promise_all(
        &mut self,
        constructor: &Value,
        values: &Value,
    ) -> Result<Value, RuntimeError> {
        let values = self.array_like_values(values)?;
        let promise = self.new_promise()?;
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        let outcome = (|| {
            // PerformPromiseAll observes `C.resolve` once before consuming
            // inputs. Calling the internal resolver here used to hide a
            // getter throw and leave the returned aggregate pending forever.
            let resolve = self.get_property(constructor, &"resolve".into())?;
            if !self.is_callable(&resolve)? {
                return Err(RuntimeError::TypeError(
                    "Promise.all resolve must be callable".into(),
                ));
            }
            if values.is_empty() {
                let values = self.array_from(Vec::new())?;
                self.settle_promise(promise, PromiseStatus::Fulfilled(values))?;
                return Ok(Value::Object(promise));
            }
            self.promise_all.insert(
                promise,
                PromiseAllState {
                    values: vec![None; values.len()],
                    remaining: values.len(),
                },
            );
            for (index, value) in values.into_iter().enumerate() {
                let input =
                    self.call_native(resolve.clone(), constructor.clone(), vec![value], false)?;
                let fulfilled = self.promise_all_handler(promise, Some(index as u32))?;
                let rejected = self.promise_all_handler(promise, None)?;
                // Invoke rather than internally attaching a reaction: an
                // own `then` getter/method on the resolved value is part of
                // Promise.all's observable error surface.
                let then = self.get_property(&input, &"then".into())?;
                self.call_native(then, input, vec![fulfilled, rejected], false)?;
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                self.promise_all.remove(&promise);
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                Ok(Value::Object(promise))
            }
        }
    }

    pub(in super::super) fn promise_race(
        &mut self,
        constructor: &Value,
        values: &Value,
    ) -> Result<Value, RuntimeError> {
        let values = self.array_like_values(values)?;
        let promise = self.new_promise()?;
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        let outcome = (|| {
            let resolve = self.get_property(constructor, &"resolve".into())?;
            if !self.is_callable(&resolve)? {
                return Err(RuntimeError::TypeError(
                    "Promise.race resolve must be callable".into(),
                ));
            }
            for value in values {
                let input =
                    self.call_native(resolve.clone(), constructor.clone(), vec![value], false)?;
                let fulfilled =
                    self.promise_combinator_handler(NativeFunction::PromiseRaceFulfill {
                        target: promise,
                    })?;
                let rejected =
                    self.promise_combinator_handler(NativeFunction::PromiseRaceReject {
                        target: promise,
                    })?;
                let then = self.get_property(&input, &"then".into())?;
                self.call_native(then, input, vec![fulfilled, rejected], false)?;
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                Ok(Value::Object(promise))
            }
        }
    }

    fn promise_settlement_record(
        &mut self,
        fulfilled: bool,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(record));
        self.stack.push(value.clone());
        let result = (|| {
            self.define_data(
                record,
                "status",
                Value::String(if fulfilled { "fulfilled" } else { "rejected" }.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                record,
                if fulfilled { "value" } else { "reason" },
                value,
                true,
                true,
                true,
            )?;
            Ok(Value::Object(record))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn promise_all_settled_result(
        &mut self,
        target: ObjectId,
        index: u32,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let complete = {
            let Some(state) = self.promise_all_settled.get_mut(&target) else {
                return Ok(());
            };
            let slot = state
                .results
                .get_mut(index as usize)
                .expect("Promise.allSettled reaction index was allocated");
            if slot.is_some() {
                return Ok(());
            }
            *slot = Some((value, fulfilled));
            state.remaining -= 1;
            (state.remaining == 0).then(|| {
                state
                    .results
                    .iter()
                    .cloned()
                    .map(|value| value.expect("completed Promise.allSettled has every result"))
                    .collect::<Vec<_>>()
            })
        };
        let Some(values) = complete else {
            return Ok(());
        };
        let base = self.stack.len();
        let mut records = Vec::with_capacity(values.len());
        for (value, fulfilled) in values {
            let record = self.promise_settlement_record(fulfilled, value)?;
            // The aggregate state retains the input values, not these newly
            // allocated result records. Keep every completed record rooted
            // while creating its siblings and the output array.
            self.stack.push(record.clone());
            records.push(record);
        }
        let records = self.array_from(records);
        self.stack.truncate(base);
        let records = records?;
        self.promise_all_settled.remove(&target);
        self.settle_promise(target, PromiseStatus::Fulfilled(records))
    }

    pub(in super::super) fn promise_all_settled_static(
        &mut self,
        constructor: &Value,
        values: &Value,
    ) -> Result<Value, RuntimeError> {
        let values = self.array_like_values(values)?;
        let promise = self.new_promise()?;
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        let outcome = (|| {
            let resolve = self.get_property(constructor, &"resolve".into())?;
            if !self.is_callable(&resolve)? {
                return Err(RuntimeError::TypeError(
                    "Promise.allSettled resolve must be callable".into(),
                ));
            }
            if values.is_empty() {
                let values = self.array_from(Vec::new())?;
                self.settle_promise(promise, PromiseStatus::Fulfilled(values))?;
                return Ok(Value::Object(promise));
            }
            self.promise_all_settled.insert(
                promise,
                PromiseAllSettledState {
                    results: vec![None; values.len()],
                    remaining: values.len(),
                },
            );
            for (index, value) in values.into_iter().enumerate() {
                let input =
                    self.call_native(resolve.clone(), constructor.clone(), vec![value], false)?;
                let fulfilled =
                    self.promise_combinator_handler(NativeFunction::PromiseAllSettledFulfill {
                        target: promise,
                        index: index as u32,
                    })?;
                let rejected =
                    self.promise_combinator_handler(NativeFunction::PromiseAllSettledReject {
                        target: promise,
                        index: index as u32,
                    })?;
                let then = self.get_property(&input, &"then".into())?;
                self.call_native(then, input, vec![fulfilled, rejected], false)?;
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                self.promise_all_settled.remove(&promise);
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                Ok(Value::Object(promise))
            }
        }
    }

    fn promise_any_aggregate_error(&mut self, errors: Vec<Value>) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend(errors.iter().cloned());
        let result = (|| {
            let errors = self.array_from(errors)?;
            self.stack.push(errors.clone());
            let constructor = self.error_global("AggregateError")?;
            self.call_native(
                constructor,
                Value::Undefined,
                vec![errors, Value::String("All promises were rejected".into())],
                false,
            )
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn promise_any_fulfill(
        &mut self,
        target: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if self.promise_any.remove(&target).is_some() {
            self.settle_promise(target, PromiseStatus::Fulfilled(value))?;
        }
        Ok(())
    }

    pub(in super::super) fn promise_any_reject(
        &mut self,
        target: ObjectId,
        index: u32,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let complete = {
            let Some(state) = self.promise_any.get_mut(&target) else {
                return Ok(());
            };
            let slot = state
                .errors
                .get_mut(index as usize)
                .expect("Promise.any reaction index was allocated");
            if slot.is_some() {
                return Ok(());
            }
            *slot = Some(value);
            state.remaining -= 1;
            (state.remaining == 0).then(|| {
                state
                    .errors
                    .iter()
                    .cloned()
                    .map(|value| value.expect("completed Promise.any has every error"))
                    .collect::<Vec<_>>()
            })
        };
        let Some(errors) = complete else {
            return Ok(());
        };
        let error = self.promise_any_aggregate_error(errors)?;
        self.promise_any.remove(&target);
        self.settle_promise(target, PromiseStatus::Rejected(error))
    }

    pub(in super::super) fn promise_any(
        &mut self,
        constructor: &Value,
        values: &Value,
    ) -> Result<Value, RuntimeError> {
        let values = self.array_like_values(values)?;
        let promise = self.new_promise()?;
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        let outcome = (|| {
            let resolve = self.get_property(constructor, &"resolve".into())?;
            if !self.is_callable(&resolve)? {
                return Err(RuntimeError::TypeError(
                    "Promise.any resolve must be callable".into(),
                ));
            }
            self.promise_any.insert(
                promise,
                PromiseAnyState {
                    errors: vec![None; values.len()],
                    remaining: values.len(),
                },
            );
            if values.is_empty() {
                let error = self.promise_any_aggregate_error(Vec::new())?;
                self.promise_any.remove(&promise);
                self.settle_promise(promise, PromiseStatus::Rejected(error))?;
                return Ok(Value::Object(promise));
            }
            for (index, value) in values.into_iter().enumerate() {
                let input =
                    self.call_native(resolve.clone(), constructor.clone(), vec![value], false)?;
                let fulfilled =
                    self.promise_combinator_handler(NativeFunction::PromiseAnyFulfill {
                        target: promise,
                    })?;
                let rejected =
                    self.promise_combinator_handler(NativeFunction::PromiseAnyReject {
                        target: promise,
                        index: index as u32,
                    })?;
                let then = self.get_property(&input, &"then".into())?;
                self.call_native(then, input, vec![fulfilled, rejected], false)?;
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                self.promise_any.remove(&promise);
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                Ok(Value::Object(promise))
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
        match job {
            PromiseJob::Reaction {
                target,
                handler,
                value,
                fulfilled,
            } => {
                if !self.is_callable(&handler)? {
                    self.settle_promise(
                        target,
                        if fulfilled {
                            PromiseStatus::Fulfilled(value)
                        } else {
                            PromiseStatus::Rejected(value)
                        },
                    )?;
                    return Ok(true);
                }
                let result = self.call_native(handler, Value::Undefined, vec![value], false);
                match result {
                    Ok(value) => self.resolve_promise(target, value)?,
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
            }
            PromiseJob::Thenable {
                target,
                thenable,
                then,
            } => {
                let result = (|| {
                    let resolve = self.promise_resolving_function(target, true)?;
                    let reject = self.promise_resolving_function(target, false)?;
                    self.call_native(then, thenable, vec![resolve, reject], false)
                })();
                if let Err(error) = result {
                    let error = self.error_value(error)?;
                    self.settle_promise(target, PromiseStatus::Rejected(error))?;
                }
            }
            PromiseJob::DynamicImport {
                target,
                referrer,
                specifier,
            } => {
                let result = self.dynamic_import_job(&referrer, &specifier);
                match result {
                    Ok(DynamicImportResult::Fulfilled(namespace)) => {
                        self.settle_promise(target, PromiseStatus::Fulfilled(namespace))?
                    }
                    Ok(DynamicImportResult::Waiting(module)) => {
                        self.module_import_waiters
                            .entry(module)
                            .or_default()
                            .push(target);
                    }
                    // Dynamic import delegates loading and linking to the
                    // host. A host module-resolution failure rejects the
                    // capability with its host error rather than leaking
                    // the static-module SyntaxError classification.
                    Err(RuntimeError::ModuleResolution(message)) => {
                        let error = self.error_object("TypeError", message)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
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
            } => self.finish_async_generator_yield(generator, target, result, value, fulfilled)?,
            PromiseJob::AsyncGeneratorDelegate {
                generator,
                target,
                kind,
                value,
                fulfilled,
            } => self.finish_async_generator_delegate(generator, target, kind, value, fulfilled)?,
            PromiseJob::FinalizationCleanup { callback, holdings } => {
                // Cleanup callbacks are host jobs, not Promise reactions. A
                // throwing callback is reported through this embedding's job
                // runner but cannot resurrect or re-register the consumed
                // cell.
                self.call_native(callback, Value::Undefined, vec![holdings], false)?;
            }
        }
        Ok(true)
    }
}
