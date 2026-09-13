// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// [[GetOwnProperty]] dispatch used by descriptor APIs, Proxy invariants,
    /// and receiver-aware [[Set]].  Ordinary heap records stay below this
    /// boundary; every Proxy operation re-enters through the VM so its trap
    /// may call JavaScript while roots remain registered.
    pub(in super::super) fn object_get_own_property(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<PropertyDescriptor>, RuntimeError> {
        // Intrinsic globals are lazily initialized, but reflective descriptor
        // operations must observe the same own properties as ordinary Get.
        self.materialize_global_object_property(object, key)?;
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_get_own_property(object, key);
        }
        self.heap
            .get_own_property_descriptor(object, key)
            .map_err(Into::into)
    }

    pub(in super::super) fn object_define_own_property(
        &mut self,
        object: ObjectId,
        key: PropertyName,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_define_own_property(object, key, descriptor);
        }
        if let Some(numeric) = self.heap.typed_array_numeric_key(object, &key)? {
            return self.typed_array_define_own_property(object, numeric, descriptor);
        }
        self.with_roots(|heap| heap.define_own_property(object, key, descriptor))
    }

    /// IntegerIndexedElementSet and the compatible portion of
    /// IntegerIndexedObject.[[DefineOwnProperty]].  A canonical numeric key
    /// never becomes an ordinary property, even when it is invalid or outside
    /// the fixed view range.
    pub(in super::super) fn typed_array_define_own_property(
        &mut self,
        object: ObjectId,
        numeric: TypedArrayNumericKey,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, RuntimeError> {
        let TypedArrayNumericKey::Index(index) = numeric else {
            return Ok(false);
        };
        let (buffer, _, length, kind) = self.heap.typed_array_info(object)?;
        if self.heap.buffer_is_detached(buffer)? || index >= length {
            return Ok(false);
        }
        if descriptor.accessor()
            || descriptor.configurable == Some(false)
            || descriptor.enumerable == Some(false)
            || descriptor.writable == Some(false)
        {
            return Ok(false);
        }
        let Some(value) = descriptor.value else {
            return Ok(true);
        };
        let value = self.typed_array_element_value(kind, &value)?;
        // IntegerIndexedElementSet converts first. A conversion may detach the
        // backing buffer; in that case the already-valid DefineOwnProperty
        // operation still succeeds without writing a byte.
        if self.heap.buffer_is_detached(buffer)? {
            return Ok(true);
        }
        self.with_roots(|heap| heap.typed_array_set_index(object, index, &value))
    }

    pub(in super::super) fn object_delete(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_delete(object, key);
        }
        self.heap.delete(object, key).map_err(Into::into)
    }

    pub(in super::super) fn object_own_property_keys(
        &mut self,
        object: ObjectId,
    ) -> Result<Vec<PropertyName>, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_own_keys(object);
        }
        self.heap.own_property_keys(object).map_err(Into::into)
    }

    pub(in super::super) fn object_is_extensible(
        &mut self,
        object: ObjectId,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_is_extensible(object);
        }
        self.heap.is_extensible(object).map_err(Into::into)
    }

    pub(in super::super) fn object_get_prototype(
        &mut self,
        object: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        if self.test262_foreign_reference(object).is_some() {
            return self.test262_foreign_get_prototype(object);
        }
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_get_prototype(object);
        }
        self.heap.prototype(object).map_err(Into::into)
    }

    pub(in super::super) fn object_set_prototype(
        &mut self,
        object: ObjectId,
        prototype: Option<ObjectId>,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_set_prototype(object, prototype);
        }
        match self.heap.set_prototype(object, prototype) {
            Ok(()) => Ok(true),
            Err(HeapError::ReadOnlyProperty | HeapError::PrototypeCycle) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub(in super::super) fn object_prevent_extensions(
        &mut self,
        object: ObjectId,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_prevent_extensions(object);
        }
        self.heap.prevent_extensions(object)?;
        Ok(true)
    }

    /// OrdinarySet with an explicit receiver.  It is deliberately expressed
    /// in terms of the object-operation boundary above, so a Proxy can occur
    /// either as the target or in the prototype chain without being skipped.
    pub(in super::super) fn ordinary_set_with_receiver(
        &mut self,
        target: ObjectId,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(target)?.is_some() {
            return self.proxy_set(target, receiver, key, value);
        }
        if let Some(numeric) = self.heap.typed_array_numeric_key(target, key)? {
            let valid = match numeric {
                TypedArrayNumericKey::Index(index) => {
                    self.heap.typed_array_index_value(target, index)?.is_some()
                }
                TypedArrayNumericKey::Invalid => false,
            };
            if receiver == &Value::Object(target) {
                // TypedArraySetElement performs ToNumber before checking
                // IsValidIntegerIndex. Thus an own assignment to `"-0"`, a
                // fractional canonical key, or an out-of-bounds index still
                // observes a throwing value conversion. A different receiver
                // has the separate OrdinarySet path below and must *not*
                // convert an invalid key's value.
                let (_, _, _, kind) = self.heap.typed_array_info(target)?;
                let value = self.typed_array_element_value(kind, value)?;
                if valid {
                    let TypedArrayNumericKey::Index(index) = numeric else {
                        unreachable!("valid TypedArray index has an integer index")
                    };
                    // Conversion can detach the buffer, in which case this
                    // successful [[Set]] performs no byte write.
                    let (buffer, _, _, _) = self.heap.typed_array_info(target)?;
                    if !self.heap.buffer_is_detached(buffer)? {
                        self.with_roots(|heap| heap.typed_array_set_index(target, index, &value))?;
                    }
                }
                return Ok(true);
            }
            // Invalid canonical numeric indices terminate the exotic [[Set]]
            // without coercion. A valid index with a distinct Receiver instead
            // follows OrdinarySet below and stores the original value there.
            if !valid {
                return Ok(true);
            }
        }
        let mut current = Some(target);
        while let Some(object) = current {
            if self.heap.proxy(object)?.is_some() {
                return self.proxy_set(object, receiver, key, value);
            }
            if object != target {
                if let Some(numeric) = self.heap.typed_array_numeric_key(object, key)? {
                    let valid = matches!(numeric, TypedArrayNumericKey::Index(index) if self
                        .heap
                        .typed_array_index_value(object, index)?
                        .is_some());
                    if receiver == &Value::Object(object) {
                        // OrdinarySet reached an Integer-Indexed exotic in
                        // the prototype chain. Its [[Set]] target is this
                        // `object`, not the initial ordinary receiver. The
                        // SameValue branch therefore still performs ToNumber
                        // for any canonical numeric key before validity is
                        // tested.
                        let (_, _, _, kind) = self.heap.typed_array_info(object)?;
                        let value = self.typed_array_element_value(kind, value)?;
                        if valid {
                            let TypedArrayNumericKey::Index(index) = numeric else {
                                unreachable!("valid TypedArray index has an integer index")
                            };
                            let (buffer, _, _, _) = self.heap.typed_array_info(object)?;
                            if !self.heap.buffer_is_detached(buffer)? {
                                self.with_roots(|heap| {
                                    heap.typed_array_set_index(object, index, &value)
                                })?;
                            }
                        }
                        return Ok(true);
                    }
                    if !valid {
                        return Ok(true);
                    }
                }
            }
            if let Some(descriptor) = self.object_get_own_property(object, key)? {
                if descriptor.accessor() {
                    let setter = descriptor.set.unwrap_or(Value::Undefined);
                    if setter == Value::Undefined {
                        return Ok(false);
                    }
                    if !self.is_callable(&setter)? {
                        return Err(RuntimeError::TypeError(
                            "property setter is not callable".into(),
                        ));
                    }
                    self.call_native(setter, receiver.clone(), vec![value.clone()], false)?;
                    return Ok(true);
                }
                if descriptor.writable == Some(false) {
                    return Ok(false);
                }
                break;
            }
            current = self.object_get_prototype(object)?;
        }
        let Value::Object(receiver) = receiver else {
            return Ok(false);
        };
        // OrdinarySetWithOwnDescriptor must first observe Receiver's
        // [[GetOwnProperty]] even when the receiver is a Proxy.  Defining
        // directly skipped that trap and broke descriptor-sensitive proxy
        // forwarding.  Use the shared boundary before choosing the update.
        let own = self.object_get_own_property(*receiver, key)?;
        if let Some(own) = &own {
            if own.accessor() || own.writable == Some(false) {
                return Ok(false);
            }
        }
        let stored = if key == "length" && self.heap.is_array(*receiver)? {
            self.array_length_value(value)?
        } else {
            value.clone()
        };
        let descriptor = if own.is_some() {
            PropertyDescriptor {
                value: Some(stored),
                ..Default::default()
            }
        } else {
            PropertyDescriptor::data(stored, true, true, true)
        };
        if self.heap.proxy(*receiver)?.is_some() {
            return self.proxy_define_own_property(*receiver, key.clone(), descriptor);
        }
        self.object_define_own_property(*receiver, key.clone(), descriptor)
    }

    pub(in super::super) fn get_from_prototype(
        &mut self,
        start: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let mut current = Some(start);
        while let Some(object) = current {
            if self.heap.proxy(object)?.is_some() {
                return self.proxy_get(object, receiver, key);
            }
            if let Some(numeric) = self.heap.typed_array_numeric_key(object, key)? {
                return match numeric {
                    TypedArrayNumericKey::Index(index) => Ok(self
                        .heap
                        .typed_array_index_value(object, index)?
                        .unwrap_or(Value::Undefined)),
                    TypedArrayNumericKey::Invalid => Ok(Value::Undefined),
                };
            }
            if let Some(desc) = self.object_get_own_property(object, key)? {
                if desc.accessor() {
                    let getter = desc.get.unwrap_or(Value::Undefined);
                    return if matches!(getter, Value::Undefined) {
                        Ok(Value::Undefined)
                    } else {
                        self.call_native(getter, receiver.clone(), Vec::new(), false)
                    };
                }
                return Ok(desc.value.unwrap_or(Value::Undefined));
            }
            current = self.object_get_prototype(object)?;
        }
        Ok(Value::Undefined)
    }

    pub(in super::super) fn constructor_prototype(
        &mut self,
        default: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let target = self.new_target.clone();
        let prototype = self.get_property(&target, &"prototype".into())?;
        if let Some(prototype) = prototype.object_id() {
            return Ok(prototype);
        }
        // GetPrototypeFromConstructor obtains the constructor's realm when
        // `prototype` is not an object. Besides selecting the fallback
        // intrinsic, that walk must observe a Proxy revoked by a `prototype`
        // getter. BlueJS stores same-realm intrinsics in this VM, so the
        // caller-supplied default is already the correct fallback after the
        // validation walk completes.
        if let Some(realm) = self.validate_function_realm(target)? {
            let intrinsic = if default == self.object_prototype {
                Some("Object")
            } else if default == self.function_prototype()? {
                Some("Function")
            } else if default == self.buffer_prototype("ArrayBuffer")? {
                Some("ArrayBuffer")
            } else if default == self.buffer_prototype("SharedArrayBuffer")? {
                Some("SharedArrayBuffer")
            } else {
                None
            };
            if let Some(intrinsic) = intrinsic {
                return self.test262_foreign_default_prototype(realm, intrinsic);
            }
        }
        Ok(default)
    }

    /// The validation portion of GetFunctionRealm for constructors owned by
    /// this VM. Bound functions and Proxy exotic objects delegate to their
    /// targets; `Heap::proxy` raises a TypeError when the Proxy has been
    /// revoked. All remaining callable forms belong to this realm and can use
    /// the intrinsic supplied by `constructor_prototype`.
    fn validate_function_realm(
        &self,
        mut function: Value,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        loop {
            let object = function
                .object_id()
                .ok_or_else(|| RuntimeError::TypeError("constructor must be callable".into()))?;
            if let Some((realm, _, _, _)) = self.test262_foreign_reference(object) {
                return Ok(Some(realm));
            }
            if let Some(bound) = self.heap.bound_function(object)? {
                function = Value::Object(bound.target);
                continue;
            }
            if let Some((target, _)) = self.heap.proxy(object)? {
                function = Value::Object(target);
                continue;
            }
            if self.heap.closure(object)?.is_some() || self.heap.native_function(object)?.is_some()
            {
                return Ok(None);
            }
            return Err(RuntimeError::TypeError(
                "constructor must be callable".into(),
            ));
        }
    }

    pub(in super::super) fn is_constructor(&self, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(id) = value else {
            return Ok(false);
        };
        if let Some((_, _, _, constructible)) = self.test262_foreign_reference(*id) {
            return Ok(constructible);
        }
        if let Some((_, constructible)) = self.heap.proxy_capabilities(*id)? {
            return Ok(constructible);
        }
        if let Some(bound) = self.heap.bound_function(*id)? {
            return Ok(bound.constructible);
        }
        if let Some((code, _, _, _, _)) = self.heap.closure(*id)? {
            return Ok(code.constructible);
        }
        Ok(matches!(
            self.heap.native_function(*id)?,
            Some(
                NativeFunction::Function
                    | NativeFunction::String
                    | NativeFunction::Array
                    | NativeFunction::ArrayBuffer
                    | NativeFunction::SharedArrayBuffer
                    | NativeFunction::DataView
                    | NativeFunction::TypedArray(_)
                    | NativeFunction::Proxy
                    | NativeFunction::Map
                    | NativeFunction::Set
                    | NativeFunction::Promise
                    | NativeFunction::Object
                    | NativeFunction::RegExp
                    | NativeFunction::Collator
                    | NativeFunction::Locale
                    | NativeFunction::Error(_)
                    | NativeFunction::PrimitiveConstructor(_)
            )
        ))
    }

    pub(in super::super) fn proxy_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Proxy constructor requires 'new'".into(),
            ));
        }
        let target = native::argument(args, 0)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy target must be an object".into()))?;
        let handler = native::argument(args, 1)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy handler must be an object".into()))?;
        let callable = self.is_callable(&Value::Object(target))?;
        let constructible = self.is_constructor(&Value::Object(target))?;
        // A Proxy's ordinary prototype slot is never consulted by its
        // internal methods; [[GetPrototypeOf]] delegates to the target.
        // Using Object.prototype also allows ProxyCreate to wrap an already
        // revoked Proxy, as required by the specification.
        let prototype = Some(self.object_prototype);
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_proxy(target, handler, prototype, callable, constructible)
        })?))
    }

    pub(in super::super) fn proxy_revocable(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let target = native::argument(args, 0)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy target must be an object".into()))?;
        let handler = native::argument(args, 1)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy handler must be an object".into()))?;
        let callable = self.is_callable(&Value::Object(target))?;
        let constructible = self.is_constructor(&Value::Object(target))?;
        let proxy_prototype = Some(self.object_prototype);
        let proxy = self.with_roots(|heap| {
            heap.alloc_proxy(target, handler, proxy_prototype, callable, constructible)
        })?;
        let base = self.stack.len();
        self.stack.push(Value::Object(proxy));
        let result = (|| {
            let function_prototype = self.function_prototype()?;
            let revoke = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::ProxyRevoker(proxy),
                    "",
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(revoke));
            self.define_data(revoke, "name", Value::String("".into()), false, false, true)?;
            self.define_data(revoke, "length", Value::Number(0.0), false, false, true)?;
            let object_prototype = self.object_prototype;
            let result = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(result));
            self.define_data(result, "proxy", Value::Object(proxy), true, true, true)?;
            self.define_data(result, "revoke", Value::Object(revoke), true, true, true)?;
            Ok(Value::Object(result))
        })();
        self.stack.truncate(base);
        result
    }

    /// Returns a Proxy trap value, preserving the handler as the call
    /// receiver.  Proxy objects retain both target and handler as heap edges,
    /// so the rooted proxy itself keeps the arguments live across this lookup.
    pub(in super::super) fn proxy_trap(
        &mut self,
        handler: ObjectId,
        name: &str,
    ) -> Result<Value, RuntimeError> {
        // GetMethod treats both `undefined` and `null` as absent. All Proxy
        // internal methods share this lookup, so this is the single forwarding
        // boundary for null-valued traps.
        let trap = self.get_property(&Value::Object(handler), &name.into())?;
        Ok(if trap == Value::Null {
            Value::Undefined
        } else {
            trap
        })
    }

    /// Implements Proxy.[[Get]] including the non-configurable-property
    /// invariants.  `receiver` is deliberately separate from `proxy`: this
    /// is what makes inherited Proxy properties and Reflect.get faithful.
    pub(in super::super) fn proxy_get(
        &mut self,
        proxy: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.get_object_property(proxy, receiver, key);
        };
        let trap = self.proxy_trap(handler, "get")?;
        if trap == Value::Undefined {
            return self.get_object_property(target, receiver, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy get trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value(), receiver.clone()],
            false,
        )?;
        if let Some(descriptor) = self.object_get_own_property(target, key)? {
            if descriptor.configurable == Some(false) {
                if descriptor.writable == Some(false)
                    && descriptor
                        .value
                        .as_ref()
                        .is_some_and(|value| !same_value(value, &result))
                {
                    return Err(RuntimeError::TypeError(
                        "Proxy get trap violated a non-writable property invariant".into(),
                    ));
                }
                if descriptor.accessor()
                    && descriptor.get == Some(Value::Undefined)
                    && result != Value::Undefined
                {
                    return Err(RuntimeError::TypeError(
                        "Proxy get trap violated an accessor invariant".into(),
                    ));
                }
            }
        }
        Ok(result)
    }

    /// Implements Proxy.[[HasProperty]] and its non-configurable and
    /// non-extensible target invariants.
    pub(in super::super) fn proxy_has(
        &mut self,
        proxy: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.has_property(proxy, key);
        };
        let trap = self.proxy_trap(handler, "has")?;
        if trap == Value::Undefined {
            return self.has_property(target, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy has trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value()],
            false,
        )?;
        let result = self.to_boolean(&result)?;
        if !result {
            if let Some(descriptor) = self.object_get_own_property(target, key)? {
                if descriptor.configurable == Some(false) || !self.object_is_extensible(target)? {
                    return Err(RuntimeError::TypeError(
                        "Proxy has trap hid a required target property".into(),
                    ));
                }
            }
        }
        Ok(result)
    }

    /// Implements Proxy.[[Set]]. The actual data-property write is shared by
    /// ordinary property assignment and Reflect.set, which keeps the supplied
    /// receiver visible to prototype accessors and Proxy traps.
    pub(in super::super) fn proxy_set(
        &mut self,
        proxy: ObjectId,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.ordinary_set_with_receiver(proxy, receiver, key, value);
        };
        let trap = self.proxy_trap(handler, "set")?;
        if trap == Value::Undefined {
            return self.ordinary_set_with_receiver(target, receiver, key, value);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy set trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![
                Value::Object(target),
                key.value(),
                value.clone(),
                receiver.clone(),
            ],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if let Some(descriptor) = self.object_get_own_property(target, key)? {
            if descriptor.configurable == Some(false)
                && ((descriptor.writable == Some(false)
                    && descriptor
                        .value
                        .as_ref()
                        .is_some_and(|current| !same_value(current, value)))
                    || (descriptor.accessor() && descriptor.set == Some(Value::Undefined)))
            {
                return Err(RuntimeError::TypeError(
                    "Proxy set trap violated a target property invariant".into(),
                ));
            }
        }
        Ok(true)
    }

    pub(in super::super) fn proxy_delete(
        &mut self,
        proxy: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return Ok(self.heap.delete(proxy, key)?);
        };
        let trap = self.proxy_trap(handler, "deleteProperty")?;
        if trap == Value::Undefined {
            return self.object_delete(target, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy deleteProperty trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value()],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if let Some(descriptor) = self.object_get_own_property(target, key)? {
            if descriptor.configurable == Some(false) || !self.object_is_extensible(target)? {
                return Err(RuntimeError::TypeError(
                    "Proxy deleteProperty trap removed a required target property".into(),
                ));
            }
        }
        Ok(true)
    }

    pub(in super::super) fn proxy_own_keys(
        &mut self,
        proxy: ObjectId,
    ) -> Result<Vec<PropertyName>, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.heap.own_property_keys(proxy).map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "ownKeys")?;
        if trap == Value::Undefined {
            return self.object_own_property_keys(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy ownKeys trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        let object = self.coerce_object(&result)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut keys = Vec::new();
        for index in 0..length {
            let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
            let key = match value {
                Value::String(string) => PropertyName::String(string),
                Value::Symbol(symbol) => PropertyName::Symbol(symbol),
                _ => {
                    self.stack.pop();
                    return Err(RuntimeError::TypeError(
                        "Proxy ownKeys trap result contains a non-property key".into(),
                    ));
                }
            };
            if keys.contains(&key) {
                self.stack.pop();
                return Err(RuntimeError::TypeError(
                    "Proxy ownKeys trap returned a duplicate key".into(),
                ));
            }
            keys.push(key);
        }
        self.stack.pop();
        let target_keys = self.object_own_property_keys(target)?;
        let mut non_configurable = Vec::new();
        for key in &target_keys {
            if self
                .object_get_own_property(target, key)?
                .is_some_and(|descriptor| descriptor.configurable == Some(false))
            {
                non_configurable.push(key.clone());
            }
        }
        if non_configurable.iter().any(|key| !keys.contains(key)) {
            return Err(RuntimeError::TypeError(
                "Proxy ownKeys trap omitted a non-configurable key".into(),
            ));
        }
        if !self.object_is_extensible(target)?
            && (keys.len() != target_keys.len()
                || target_keys.iter().any(|key| !keys.contains(key)))
        {
            return Err(RuntimeError::TypeError(
                "Proxy ownKeys trap disagreed with a non-extensible target".into(),
            ));
        }
        Ok(keys)
    }

    pub(in super::super) fn descriptor_object(
        &mut self,
        descriptor: &PropertyDescriptor,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            for (name, value) in [
                ("value", descriptor.value.clone()),
                ("writable", descriptor.writable.map(Value::Bool)),
                ("get", descriptor.get.clone()),
                ("set", descriptor.set.clone()),
                ("enumerable", descriptor.enumerable.map(Value::Bool)),
                ("configurable", descriptor.configurable.map(Value::Bool)),
            ] {
                if let Some(value) = value {
                    self.with_roots(|heap| heap.set(object, name, value))?;
                }
            }
            Ok(Value::Object(object))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn proxy_get_own_property(
        &mut self,
        proxy: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<PropertyDescriptor>, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self
                .heap
                .get_own_property_descriptor(proxy, key)
                .map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "getOwnPropertyDescriptor")?;
        if trap == Value::Undefined {
            return self.object_get_own_property(target, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy getOwnPropertyDescriptor trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value()],
            false,
        )?;
        let target_descriptor = self.object_get_own_property(target, key)?;
        let extensible = self.object_is_extensible(target)?;
        if result == Value::Undefined {
            if target_descriptor
                .as_ref()
                .is_some_and(|descriptor| descriptor.configurable == Some(false))
                || (!extensible && target_descriptor.is_some())
            {
                return Err(RuntimeError::TypeError(
                    "Proxy getOwnPropertyDescriptor trap hid a required target property".into(),
                ));
            }
            return Ok(None);
        }
        let descriptor = complete_property_descriptor(self.read_descriptor(&result)?);
        if !compatible_property_descriptor(extensible, target_descriptor.as_ref(), &descriptor) {
            return Err(RuntimeError::TypeError(
                "Proxy getOwnPropertyDescriptor trap returned an incompatible descriptor".into(),
            ));
        }
        if descriptor.configurable == Some(false)
            && target_descriptor
                .as_ref()
                .is_none_or(|current| current.configurable != Some(false))
        {
            return Err(RuntimeError::TypeError(
                "Proxy getOwnPropertyDescriptor trap reported a new non-configurable property"
                    .into(),
            ));
        }
        Ok(Some(descriptor))
    }

    pub(in super::super) fn proxy_define_own_property(
        &mut self,
        proxy: ObjectId,
        key: PropertyName,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.with_roots(|heap| heap.define_own_property(proxy, key, descriptor));
        };
        let trap = self.proxy_trap(handler, "defineProperty")?;
        if trap == Value::Undefined {
            return self.object_define_own_property(target, key, descriptor);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy defineProperty trap must be callable".into(),
            ));
        }
        let descriptor_value = self.descriptor_object(&descriptor)?;
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value(), descriptor_value],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        let target_descriptor = self.object_get_own_property(target, &key)?;
        let extensible = self.object_is_extensible(target)?;
        if !compatible_property_descriptor(extensible, target_descriptor.as_ref(), &descriptor) {
            return Err(RuntimeError::TypeError(
                "Proxy defineProperty trap reported an incompatible property".into(),
            ));
        }
        if descriptor.configurable == Some(false)
            && target_descriptor
                .as_ref()
                .is_none_or(|current| current.configurable != Some(false))
        {
            return Err(RuntimeError::TypeError(
                "Proxy defineProperty trap reported a new non-configurable property".into(),
            ));
        }
        Ok(true)
    }

    pub(in super::super) fn proxy_is_extensible(
        &mut self,
        proxy: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.heap.is_extensible(proxy).map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "isExtensible")?;
        if trap == Value::Undefined {
            return self.object_is_extensible(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy isExtensible trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        let result = self.to_boolean(&trap_result)?;
        if result != self.object_is_extensible(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy isExtensible trap disagreed with its target".into(),
            ));
        }
        Ok(result)
    }

    pub(in super::super) fn proxy_get_prototype(
        &mut self,
        proxy: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.heap.prototype(proxy).map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "getPrototypeOf")?;
        if trap == Value::Undefined {
            return self.object_get_prototype(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy getPrototypeOf trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        let prototype = match result {
            Value::Null => None,
            Value::Object(object) => Some(object),
            _ => {
                return Err(RuntimeError::TypeError(
                    "Proxy getPrototypeOf trap must return an object or null".into(),
                ))
            }
        };
        if !self.object_is_extensible(target)? && prototype != self.object_get_prototype(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy getPrototypeOf trap disagreed with a non-extensible target".into(),
            ));
        }
        Ok(prototype)
    }

    pub(in super::super) fn proxy_set_prototype(
        &mut self,
        proxy: ObjectId,
        prototype: Option<ObjectId>,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return match self.heap.set_prototype(proxy, prototype) {
                Ok(()) => Ok(true),
                Err(HeapError::ReadOnlyProperty | HeapError::PrototypeCycle) => Ok(false),
                Err(error) => Err(error.into()),
            };
        };
        let trap = self.proxy_trap(handler, "setPrototypeOf")?;
        if trap == Value::Undefined {
            return self.object_set_prototype(target, prototype);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy setPrototypeOf trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![
                Value::Object(target),
                prototype.map_or(Value::Null, Value::Object),
            ],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if !self.object_is_extensible(target)? && prototype != self.object_get_prototype(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy setPrototypeOf trap changed a non-extensible target".into(),
            ));
        }
        Ok(true)
    }

    pub(in super::super) fn proxy_prevent_extensions(
        &mut self,
        proxy: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            self.heap.prevent_extensions(proxy)?;
            return Ok(true);
        };
        let trap = self.proxy_trap(handler, "preventExtensions")?;
        if trap == Value::Undefined {
            return self.object_prevent_extensions(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy preventExtensions trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if self.object_is_extensible(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy preventExtensions trap left its target extensible".into(),
            ));
        }
        Ok(true)
    }

    /// Proxy.[[Call]] and Proxy.[[Construct]].  The outer call frame retains
    /// the Proxy as `newTarget`, so forwarding a construct without a trap
    /// preserves the required allocation prototype.
    pub(in super::super) fn proxy_call(
        &mut self,
        proxy: ObjectId,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return Err(RuntimeError::TypeError(
                "Proxy target is unavailable".into(),
            ));
        };
        let name = if construct { "construct" } else { "apply" };
        let trap = self.proxy_trap(handler, name)?;
        if trap == Value::Undefined {
            return self.dispatch_call(Value::Object(target), receiver, args, construct);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(format!(
                "Proxy {name} trap must be callable"
            )));
        }
        let arguments = self.array_from(args)?;
        let values = if construct {
            vec![Value::Object(target), arguments, self.new_target.clone()]
        } else {
            vec![Value::Object(target), receiver, arguments]
        };
        let result = self.call_native(trap, Value::Object(handler), values, false)?;
        if construct && !matches!(result, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Proxy construct trap must return an object".into(),
            ));
        }
        Ok(result)
    }
}
