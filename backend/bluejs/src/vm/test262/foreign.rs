// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::TypedArrayKind;

impl Vm {
    pub(in super::super) fn test262_foreign_reference(
        &self,
        wrapper: ObjectId,
    ) -> Option<(ObjectId, ObjectId, bool, bool)> {
        self.test262_foreign_values.get(&wrapper).map(|value| {
            (
                value.realm,
                value.target,
                value.callable,
                value.constructible,
            )
        })
    }

    /// Checks whether a facade denotes the named intrinsic constructor in its
    /// own Realm. `ArraySpeciesCreate` needs this exact identity check before
    /// reading `@@species`: a foreign `%Array%` is replaced by the current
    /// Realm's default Array constructor rather than observing mutable
    /// properties on the foreign intrinsic.
    pub(in super::super) fn test262_foreign_intrinsic_constructor(
        &mut self,
        wrapper: ObjectId,
        intrinsic: &str,
    ) -> Result<bool, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(false);
        };
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        let intrinsic = realm.vm.global(intrinsic)?.object_id();
        Ok(intrinsic == Some(target))
    }

    /// Return a facade's child-VM builtin tag without invoking it. Internal
    /// algorithms use this to recognize intrinsic hooks such as
    /// `Function.prototype[@@hasInstance]`; calling that hook in the child
    /// would otherwise receive an opaque stand-in for a caller-realm value.
    pub(in super::super) fn test262_foreign_native_function(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<NativeFunction>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        self.test262_realms
            .get(&realm_id)
            .ok_or_else(|| {
                RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
            })?
            .vm
            .heap
            .native_function(target)
            .map_err(Into::into)
    }

    pub(in super::super) fn test262_foreign_regexp_data(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<(JsString, String)>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let realm = self.test262_realms.get(&realm_id).ok_or_else(|| {
            RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
        })?;
        Ok(realm
            .vm
            .heap
            .regexp(target)?
            .map(|regexp| (regexp.source.clone(), regexp.flags.clone())))
    }

    /// The [[DateValue]] of a Date that lives in another Test262 Realm, or
    /// `None` when `wrapper` is not a facade for a Date.
    pub(in super::super) fn test262_foreign_date_value(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<f64>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let realm = self.test262_realms.get(&realm_id).ok_or_else(|| {
            RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
        })?;
        Ok(realm.vm.heap.date_value(target).ok())
    }

    pub(in super::super) fn test262_foreign_boxed_primitive(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let realm = self.test262_realms.get(&realm_id).ok_or_else(|| {
            RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
        })?;
        realm.vm.heap.boxed_primitive(target).map_err(Into::into)
    }

    pub(in super::super) fn test262_import_foreign_value(
        &mut self,
        realm_id: ObjectId,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(target) = value else {
            return Ok(value);
        };
        if let Some(value) = self
            .test262_realms
            .get(&realm_id)
            .and_then(|realm| realm.imported_values.get(&target))
        {
            return Ok(value.value.clone());
        }
        if let Some(wrapper) = self
            .test262_realms
            .get(&realm_id)
            .and_then(|realm| realm.wrappers.get(&target))
        {
            return Ok(Value::Object(*wrapper));
        }
        let (callable, constructible, proxy_parts, target_root) = {
            let realm = self.test262_realms.get_mut(&realm_id).ok_or_else(|| {
                RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
            })?;
            let value = Value::Object(target);
            let callable = realm.vm.is_callable(&value)?;
            let constructible = realm.vm.is_constructor(&value)?;
            // A live foreign Proxy needs a local exotic facade, rather than
            // the ordinary wrapper used for other foreign values. Its
            // internal methods create argument and descriptor objects in the
            // *current* Realm before calling a possibly foreign trap. An
            // ordinary facade would instead run the complete Proxy operation
            // in the child Realm, leaking that Realm through those objects.
            // A revoked proxy remains an ordinary foreign facade: observing
            // it immediately throws and there are no live slots to mirror.
            let proxy_parts = realm.vm.heap.proxy(target).ok().flatten();
            let root = realm.vm.heap.root(target)?;
            (callable, constructible, proxy_parts, root)
        };
        let prototype = self.object_prototype;
        let wrapper = match (|| {
            if let Some((target, handler)) = proxy_parts {
                let target = self
                    .test262_import_foreign_value(realm_id, Value::Object(target))?
                    .object_id()
                    .expect("foreign proxy target is an object");
                let handler = self
                    .test262_import_foreign_value(realm_id, Value::Object(handler))?
                    .object_id()
                    .expect("foreign proxy handler is an object");
                self.with_roots(|heap| {
                    heap.alloc_proxy(target, handler, Some(prototype), callable, constructible)
                })
            } else {
                self.with_roots(|heap| heap.alloc_object(Some(prototype)))
            }
        })() {
            Ok(wrapper) => wrapper,
            Err(error) => {
                self.test262_realms
                    .get_mut(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .unroot(target_root)?;
                return Err(error);
            }
        };
        let wrapper_root = match self.heap.root(wrapper) {
            Ok(root) => root,
            Err(error) => {
                self.test262_realms
                    .get_mut(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .unroot(target_root)?;
                return Err(error.into());
            }
        };
        self.test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live")
            .wrappers
            .insert(target, wrapper);
        self.test262_foreign_values.insert(
            wrapper,
            Test262ForeignValue {
                realm: realm_id,
                target,
                callable,
                constructible,
                prototype_override: None,
                _wrapper_root: wrapper_root,
                _target_root: target_root,
            },
        );
        Ok(Value::Object(wrapper))
    }

    pub(in super::super) fn test262_import_foreign_result(
        &mut self,
        realm_id: ObjectId,
        result: Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        match result {
            Ok(value) => self.test262_import_foreign_value(realm_id, value),
            Err(RuntimeError::Thrown(value)) => Err(RuntimeError::Thrown(
                self.test262_import_foreign_value(realm_id, value)?,
            )),
            Err(error) => Err(error),
        }
    }

    pub(in super::super) fn test262_foreign_get_prototype(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        let (realm_id, target, override_prototype) = {
            let value = self
                .test262_foreign_values
                .get(&wrapper)
                .expect("foreign prototype has a membrane record");
            (value.realm, value.target, value.prototype_override)
        };
        if override_prototype.is_some() {
            return Ok(override_prototype);
        }
        let prototype = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            realm.vm.object_get_prototype(target)?
        };
        prototype
            .map(|prototype| self.test262_import_foreign_value(realm_id, Value::Object(prototype)))
            .transpose()
            .map(|prototype| prototype.and_then(|prototype| prototype.object_id()))
    }

    /// The `%Intrinsic.prototype%` of a foreign Realm, as a facade in this
    /// Realm: what `GetPrototypeFromConstructor` yields when the new target
    /// belongs to that Realm and its `prototype` is not an object.
    pub(in super::super) fn test262_foreign_default_prototype(
        &mut self,
        realm_id: ObjectId,
        intrinsic: &str,
    ) -> Result<ObjectId, RuntimeError> {
        let prototype = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.intrinsic_prototype(intrinsic)?
        };
        self.test262_import_foreign_value(realm_id, prototype)?
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("intrinsic prototype must be an object".into()))
    }

    pub(in super::super) fn test262_set_foreign_prototype_override(
        &mut self,
        wrapper: ObjectId,
        prototype: ObjectId,
    ) {
        self.test262_foreign_values
            .get_mut(&wrapper)
            .expect("foreign result has a membrane record")
            .prototype_override = Some(prototype);
    }

    pub(in super::super) fn test262_export_foreign_value(
        &mut self,
        realm_id: ObjectId,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(source) = value else {
            return Ok(value.clone());
        };
        let mut candidate = *source;
        loop {
            if let Some((value_realm, target, _, _)) = self.test262_foreign_reference(candidate) {
                if value_realm == realm_id {
                    return Ok(Value::Object(target));
                }
                // `target` belongs to a different Test262 realm than the
                // one we are exporting into. If it is itself a
                // `ShadowRealm` instance there, re-export that exact same
                // child realm into `realm_id` too, rather than falling
                // through to an opaque, brand-less stand-in that could
                // never again be recognized as a `ShadowRealm`.
                if let Some(value) =
                    self.export_foreign_shadow_realm(value_realm, target, realm_id)?
                {
                    return Ok(value);
                }
                break;
            }
            let Some((target, _)) = self.heap.proxy(candidate)? else {
                break;
            };
            candidate = target;
        }
        self.test262_transport_value(realm_id, value.clone())
    }

    /// If `target` (an object living in the Test262 realm `source_realm_id`'s
    /// own `Vm`) is itself a `ShadowRealm` instance, registers that exact
    /// same child realm under a fresh instance object in `realm_id`'s own
    /// `Vm` too (a cheap `Rc` clone -- see `ShadowRealmRecord`'s doc
    /// comment) and returns that new instance's identity. Reaching
    /// `evaluate`/`importValue`/a wrapped-function call through it then
    /// observes the exact same realm -- same `globalThis`, same prior
    /// `evaluate()` side effects -- as the original, which Test262's
    /// ordinary opaque, brand-less transport (`test262_transport_value`)
    /// cannot represent at all. Returns `Ok(None)` when `target` is not a
    /// `ShadowRealm`, letting the caller fall back to that ordinary
    /// transport as before.
    pub(in super::super) fn export_foreign_shadow_realm(
        &mut self,
        source_realm_id: ObjectId,
        target: ObjectId,
        realm_id: ObjectId,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some(source_record) = self
            .test262_realms
            .get(&source_realm_id)
            .and_then(|realm| realm.vm.shadow_realm_record(target))
        else {
            return Ok(None);
        };
        // Reuse an earlier re-export of this exact target into this exact
        // destination realm, rather than minting a fresh `ShadowRealm`
        // instance object every time the same value crosses again.
        if let Some(&existing) = self
            .test262_realms
            .get(&realm_id)
            .and_then(|realm| realm.shadow_realm_reexports.get(&target))
        {
            return Ok(Some(Value::Object(existing)));
        }
        let destination = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        let default = destination.vm.shadow_realm_prototype()?;
        let instance = destination
            .vm
            .with_roots(|heap| heap.alloc_object(Some(default)))?;
        destination.vm.adopt_shadow_realm(instance, source_record);
        destination.shadow_realm_reexports.insert(target, instance);
        Ok(Some(Value::Object(instance)))
    }

    /// Creates an identity-preserving, child-heap stand-in for a parent value.
    ///
    /// Arrays, TypedArrays, and ArrayBuffers are the exceptions: when either
    /// crosses into a foreign built-in as an argument, its indexed values or
    /// backing bytes must remain available to that built-in. For example,
    /// `new foreign.Int8Array([1, 2])` performs the ordinary Array-like read,
    /// while `new foreign.Int8Array(localTypedArray)` reads the latter's
    /// TypedArray internal slots. Child-heap snapshots preserve these input
    /// contracts without pretending that the two VMs have a general,
    /// resumable property-forwarding membrane. Returning the stand-in still
    /// restores the exact original parent value.
    pub(in super::super) fn test262_transport_value(
        &mut self,
        realm_id: ObjectId,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let source = value
            .object_id()
            .expect("only objects require Test262 membrane transport");
        if let Some(target) = self
            .test262_realms
            .get(&realm_id)
            .and_then(|realm| realm.imported_sources.get(&source))
        {
            return Ok(Value::Object(*target));
        }
        let array_values = if self.heap.is_array(source)? {
            self.array_like_values(&value)?
                .iter()
                .map(|value| self.test262_export_foreign_value(realm_id, value))
                .collect::<Result<Vec<_>, _>>()
                .map(Some)?
        } else {
            None
        };
        let typed_array_values = if self.heap.is_typed_array(source)? {
            let (buffer, _, length, kind) = self.heap.typed_array_info(source)?;
            if self.heap.buffer_is_detached(buffer)?
                || self.heap.typed_array_is_out_of_bounds(source)?
            {
                return Err(RuntimeError::TypeError(
                    "TypedArray source is detached or out of bounds".into(),
                ));
            }
            let values = (0..length)
                .map(|index| {
                    self.heap
                        .typed_array_index_value(source, index)?
                        .ok_or_else(|| {
                            RuntimeError::TypeError("TypedArray source is out of bounds".into())
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Some((kind, values))
        } else {
            None
        };
        // A child heap cannot retain a parent ArrayBuffer object identity.
        // Allocate an equivalent child buffer and remember the pair so
        // direct indexed writes and host detachment remain observable in
        // both realms. Shared buffers may retain their Arc backing directly;
        // ordinary buffers use a synchronized byte mirror.
        let buffer_transport = if self.heap.is_buffer(source)? {
            let shared = self.heap.buffer_is_shared(source)?;
            let detached = self.heap.buffer_is_detached(source)?;
            let byte_length = self.heap.buffer_byte_length(source)?;
            let maximum = self.heap.buffer_max_byte_length(source)?;
            let bytes = (!shared && !detached)
                .then(|| self.heap.array_buffer_copy(source, 0, byte_length))
                .transpose()?;
            let backing = shared
                .then(|| self.heap.shared_buffer_backing(source))
                .transpose()?;
            Some((shared, detached, byte_length, maximum, bytes, backing))
        } else {
            None
        };
        let ordinary_buffer = buffer_transport
            .as_ref()
            .is_some_and(|(shared, ..)| !shared);
        let immutable_buffer = ordinary_buffer && self.heap.buffer_is_immutable(source)?;
        // Only opaque ordinary objects receive write-back support. Arrays,
        // TypedArrays, and buffers have purpose-built transport snapshots or
        // backing-store mirrors, whose indexed state must not be mistaken for
        // ordinary own data properties at a later call boundary.
        let property_forwarding =
            array_values.is_none() && typed_array_values.is_none() && buffer_transport.is_none();
        // A ShadowRealm boundary is permitted to receive a callable object,
        // and must manufacture a WrappedFunction with the target realm's
        // %Function.prototype%.  The Test262 membrane keeps the object
        // opaque, but it must retain this one observable capability locally
        // or ShadowRealm rejects it before it can create that wrapper.
        let callable = self.is_callable(&Value::Object(source))?;
        let source_root = self.heap.root(source)?;
        let result = (|| {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            let target = if let Some((shared, detached, byte_length, maximum, bytes, backing)) =
                &buffer_transport
            {
                let prototype = if *shared {
                    realm.vm.buffer_prototype("SharedArrayBuffer")?
                } else {
                    realm.vm.buffer_prototype("ArrayBuffer")?
                };
                let buffer = if let Some(backing) = backing.clone() {
                    realm.vm.with_roots(|heap| {
                        heap.alloc_shared_array_buffer_backing(
                            backing,
                            (*maximum != *byte_length).then_some(*maximum),
                            Some(prototype),
                        )
                    })?
                } else if immutable_buffer {
                    // An immutable buffer stays immutable in the child realm;
                    // its bytes are supplied once, at allocation.
                    let bytes = bytes.clone().unwrap_or_default();
                    realm.vm.with_roots(|heap| {
                        heap.alloc_immutable_array_buffer(bytes, Some(prototype))
                    })?
                } else if *maximum != *byte_length {
                    realm.vm.with_roots(|heap| {
                        heap.alloc_resizable_array_buffer(*byte_length, *maximum, Some(prototype))
                    })?
                } else {
                    realm
                        .vm
                        .with_roots(|heap| heap.alloc_array_buffer(*byte_length, Some(prototype)))?
                };
                if *detached {
                    realm
                        .vm
                        .with_roots(|heap| heap.detach_array_buffer(buffer))?;
                } else if let (Some(bytes), false) = (bytes, immutable_buffer) {
                    realm
                        .vm
                        .with_roots(|heap| heap.array_buffer_write(buffer, 0, bytes))?;
                }
                buffer
            } else if let Some(values) = array_values {
                realm
                    .vm
                    .array_from(values)?
                    .object_id()
                    .expect("array construction returns an object")
            } else if let Some((kind, values)) = typed_array_values {
                let constructor = realm.vm.global(kind.name())?;
                let target = realm
                    .vm
                    .call_native(
                        constructor,
                        Value::Undefined,
                        vec![Value::Number(values.len() as f64)],
                        true,
                    )?
                    .object_id()
                    .expect("TypedArray construction returns an object");
                for (index, value) in values.iter().enumerate() {
                    realm
                        .vm
                        .with_roots(|heap| heap.typed_array_set_index(target, index, value))?;
                }
                target
            } else {
                let prototype = realm.vm.object_prototype;
                realm
                    .vm
                    .with_roots(|heap| heap.alloc_object(Some(prototype)))?
            };
            let target_root = realm.vm.heap.root(target)?;
            if callable {
                realm.vm.test262_imported_callables.insert(target);
            }
            realm.imported_sources.insert(source, target);
            realm.imported_values.insert(
                target,
                Test262ImportedValue {
                    value,
                    property_forwarding,
                    _source_root: source_root,
                    _target_root: target_root,
                },
            );
            Ok(Value::Object(target))
        })();
        match result.as_ref() {
            Err(_) => {
                self.heap.unroot(source_root)?;
            }
            Ok(value) if ordinary_buffer => {
                let target = value
                    .object_id()
                    .expect("successful transport returns a child object");
                self.test262_foreign_buffer_mirrors.insert(
                    (source, realm_id),
                    Test262ForeignBufferMirror {
                        realm: realm_id,
                        target,
                        facade: None,
                        _buffer_root: None,
                    },
                );
            }
            Ok(_) => {}
        }
        result
    }

    pub(in super::super) fn test262_foreign_get(
        &mut self,
        wrapper: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign get has a membrane record");
        let receiver = self.test262_export_foreign_value(realm_id, receiver)?;
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        let result = realm.vm.get_object_property(target, &receiver, key);
        self.test262_import_foreign_result(realm_id, result)
    }

    /// Forwards a foreign facade's [[OwnPropertyKeys]] into its Realm. Keys
    /// are primitives, so no wrapper allocation is needed; agent-wide
    /// registered Symbols already retain their identity across the boundary.
    pub(in super::super) fn test262_foreign_own_property_keys(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Vec<PropertyName>, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign ownKeys has a membrane record");
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        realm.vm.object_own_property_keys(target)
    }

    /// Runs a foreign facade's [[Set]] in its owning Realm. `Reflect.set`
    /// exposes its explicit receiver to accessors and Proxy traps, so both
    /// receiver and value must cross the membrane before the internal method
    /// begins rather than being applied to the local wrapper.
    pub(in super::super) fn test262_foreign_set_with_receiver(
        &mut self,
        wrapper: ObjectId,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign set has a membrane record");
        let receiver = self.test262_export_foreign_value(realm_id, receiver)?;
        let value = self.test262_export_foreign_value(realm_id, value)?;
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        realm
            .vm
            .ordinary_set_with_receiver(target, &receiver, key, &value)
    }

    pub(in super::super) fn test262_foreign_set(
        &mut self,
        wrapper: ObjectId,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign set has a membrane record");
        let typed_buffer = self
            .test262_realms
            .get(&realm_id)
            .and_then(|realm| realm.vm.heap.typed_array_info(target).ok())
            .map(|(buffer, _, _, _)| buffer);
        if typed_buffer.is_some() {
            self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        }
        // Test262 harness helpers are installed independently in every
        // Realm. A fixture may explicitly copy (for example)
        // `assert.sameValue` into a child Realm; preserving that child's
        // equivalent native helper keeps the call boundary functional rather
        // than replacing it with the deliberately opaque ordinary-object
        // transport used for arbitrary parent objects.
        let native = value
            .object_id()
            .map(|object| self.heap.native_function(object))
            .transpose()?
            .flatten();
        let equivalent_native = if let Some(native) = native {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            let current = realm
                .vm
                .get_object_property(target, &Value::Object(target), key)?;
            current
                .object_id()
                .filter(|current| {
                    realm.vm.heap.native_function(*current).ok() == Some(Some(native))
                })
                .map(Value::Object)
        } else {
            None
        };
        let value = match equivalent_native {
            Some(value) => value,
            None => self.test262_export_foreign_value(realm_id, value)?,
        };
        let result = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            realm.vm.set_property(&Value::Object(target), key, &value)
        };
        if let Some(buffer) = typed_buffer {
            self.test262_refresh_foreign_buffer_mirrors(realm_id, buffer)?;
        }
        result
    }

    /// Runs one of `%TypedArray%`'s generic native methods against a facade
    /// that denotes a TypedArray in another Test262 Realm. The function
    /// object may belong to this Realm (for example,
    /// `Uint8Array.prototype.entries.call(foreignArray)`), but the receiver's
    /// internal slots belong to the child VM and must never be inspected in
    /// the facade's ordinary-object heap record.
    pub(in super::super) fn test262_foreign_typed_array_native_call(
        &mut self,
        function: NativeFunction,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let wrapper = receiver
            .object_id()
            .expect("foreign TypedArray receiver has an object identity");
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign TypedArray receiver has a membrane record");
        let source_buffer = self
            .test262_realms
            .get(&realm_id)
            .expect("foreign realm remains live")
            .vm
            .heap
            .typed_array_info(target)?
            .0;
        self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let result = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            let result = realm
                .vm
                .native_call(function, Value::Object(target), args, construct);
            match result {
                Ok(value) => Ok(value),
                Err(error) => realm
                    .vm
                    .error_value(error)
                    .and_then(|error| Err(RuntimeError::Thrown(error))),
            }
        };
        self.test262_refresh_foreign_buffer_mirrors(realm_id, source_buffer)?;
        self.test262_import_foreign_result(realm_id, result)
    }

    /// Runs an `Atomics` function whose first argument is a facade denoting a
    /// TypedArray in another Test262 Realm. Atomics validates and accesses
    /// that argument's internal slots, which live in the child VM's heap, so
    /// the whole operation executes there against the real array. Unlike the
    /// `%TypedArray%` methods, an `Atomics` function is not a member of the
    /// array's Realm and its errors belong to the calling Realm: a Rust-level
    /// TypeError or RangeError is returned unconverted for this Realm to
    /// materialize, and only a thrown JavaScript value crosses the membrane.
    /// Returns `Ok(None)` when the first argument is not such a facade, so
    /// the caller continues with the ordinary same-Realm validation.
    pub(in super::super) fn test262_foreign_atomics_call(
        &mut self,
        function: NativeFunction,
        args: &[Value],
    ) -> Result<Option<Value>, RuntimeError> {
        let Some(wrapper) = args.first().and_then(Value::object_id) else {
            return Ok(None);
        };
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let source_buffer = {
            let heap = &self
                .test262_realms
                .get(&realm_id)
                .expect("foreign realm remains live")
                .vm
                .heap;
            if !heap.is_typed_array(target)? {
                return Ok(None);
            }
            heap.typed_array_info(target)?.0
        };
        // The index, value and timeout arguments are always converted with
        // ToPrimitive(number) (via ToIndex, ToNumber or ToBigInt), which runs
        // user code that belongs to this Realm. An object crossing the
        // membrane is an opaque stand-in whose hooks the child could not
        // call, so perform that step here and pass the child the primitive.
        let mut converted = Vec::with_capacity(args.len());
        converted.push(args[0].clone());
        for value in &args[1..] {
            converted.push(self.coerce_primitive(value, "number")?);
        }
        self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        let args = converted
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let result = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            realm
                .vm
                .native_call(function, Value::Undefined, args, false)
        };
        self.test262_refresh_foreign_buffer_mirrors(realm_id, source_buffer)?;
        self.test262_import_foreign_result(realm_id, result)
            .map(Some)
    }

    /// Snapshots the elements of a foreign TypedArray through its internal
    /// slots. Algorithms such as `%TypedArray%.prototype.set` must not read
    /// an observable own `length` property from a cross-Realm source; they
    /// use its [[ArrayLength]] and validate detached/out-of-bounds state.
    pub(in super::super) fn test262_foreign_typed_array_values(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Option<Vec<Value>>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        if !realm.vm.heap.is_typed_array(target)? {
            return Ok(None);
        }
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        if realm.vm.heap.typed_array_is_out_of_bounds(target)? {
            return Err(RuntimeError::TypeError(
                "foreign TypedArray is detached or out of bounds".into(),
            ));
        }
        let (_, _, length, _) = realm.vm.heap.typed_array_info(target)?;
        (0..length)
            .map(|index| {
                realm
                    .vm
                    .heap
                    .typed_array_index_value(target, index)?
                    .ok_or_else(|| {
                        RuntimeError::TypeError("foreign TypedArray is out of bounds".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    /// Creates a local backing buffer for a foreign ArrayBuffer supplied to a
    /// local TypedArray constructor. A child heap cannot be stored directly
    /// in a parent TypedArray's [[ViewedArrayBuffer]], so ordinary buffers
    /// take a byte snapshot and retain a detachment mirror. Shared buffers
    /// already have an agent-safe shared backing and use that directly.
    pub(in super::super) fn test262_foreign_buffer_clone(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let (shared, detached, byte_length, maximum, bytes, backing) = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            if !realm.vm.heap.is_buffer(target)? {
                return Ok(None);
            }
            let shared = realm.vm.heap.buffer_is_shared(target)?;
            let detached = realm.vm.heap.buffer_is_detached(target)?;
            let byte_length = realm.vm.heap.buffer_byte_length(target)?;
            let maximum = realm.vm.heap.buffer_max_byte_length(target)?;
            let bytes = (!shared && !detached)
                .then(|| realm.vm.heap.array_buffer_copy(target, 0, byte_length))
                .transpose()?;
            let backing = shared
                .then(|| realm.vm.heap.shared_buffer_backing(target))
                .transpose()?;
            (shared, detached, byte_length, maximum, bytes, backing)
        };
        let immutable = !shared
            && self
                .test262_realms
                .get(&realm_id)
                .expect("foreign realm remains live")
                .vm
                .heap
                .buffer_is_immutable(target)?;
        let prototype = if shared {
            self.buffer_prototype("SharedArrayBuffer")?
        } else {
            self.buffer_prototype("ArrayBuffer")?
        };
        let buffer = if let Some(backing) = backing {
            self.with_roots(|heap| {
                heap.alloc_shared_array_buffer_backing(
                    backing,
                    (maximum != byte_length).then_some(maximum),
                    Some(prototype),
                )
            })?
        } else if immutable {
            let bytes = bytes.clone().unwrap_or_default();
            self.with_roots(|heap| heap.alloc_immutable_array_buffer(bytes, Some(prototype)))?
        } else if maximum != byte_length {
            self.with_roots(|heap| {
                heap.alloc_resizable_array_buffer(byte_length, maximum, Some(prototype))
            })?
        } else {
            self.with_roots(|heap| heap.alloc_array_buffer(byte_length, Some(prototype)))?
        };
        if detached {
            self.with_roots(|heap| heap.detach_array_buffer(buffer))?;
        }
        if let Some(bytes) = bytes {
            if !immutable {
                self.with_roots(|heap| heap.array_buffer_write(buffer, 0, &bytes))?;
            }
            let buffer_root = self.heap.root(buffer)?;
            self.test262_foreign_buffer_mirrors.insert(
                (buffer, realm_id),
                Test262ForeignBufferMirror {
                    realm: realm_id,
                    target,
                    facade: Some(wrapper),
                    _buffer_root: Some(buffer_root),
                },
            );
        }
        Ok(Some(buffer))
    }

    /// A TypedArray view backed by an ordinary foreign ArrayBuffer exposes
    /// that source's facade from `.buffer`, rather than leaking the bridge's
    /// local mirror object.
    pub(in super::super) fn test262_foreign_buffer_facade(
        &self,
        buffer: ObjectId,
    ) -> Option<Value> {
        self.test262_foreign_buffer_mirrors
            .iter()
            .find_map(|((local, _), mirror)| {
                (*local == buffer)
                    .then_some(mirror.facade)
                    .flatten()
                    .map(Value::Object)
            })
    }

    /// Flush byte changes made through a local mirror before a native
    /// TypedArray operation re-enters the owner Realm. This keeps direct
    /// indexed writes through a locally-created cross-Realm view observable
    /// to the next operation on the foreign TypedArray.
    pub(in super::super) fn test262_sync_foreign_buffer_mirrors(
        &mut self,
        realm_id: ObjectId,
    ) -> Result<(), RuntimeError> {
        let mirrors = self
            .test262_foreign_buffer_mirrors
            .iter()
            .filter_map(|((buffer, _), mirror)| {
                (mirror.realm == realm_id).then_some((*buffer, mirror.target))
            })
            .collect::<Vec<_>>();
        for (buffer, target) in mirrors {
            if !self.heap.is_buffer(buffer)?
                || self.heap.buffer_is_detached(buffer)?
                || self.heap.buffer_is_immutable(buffer)?
            {
                continue;
            }
            let byte_length = self.heap.buffer_byte_length(buffer)?;
            let bytes = self.heap.array_buffer_copy(buffer, 0, byte_length)?;
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            if !realm.vm.heap.buffer_is_detached(target)?
                && !realm.vm.heap.buffer_is_immutable(target)?
                && realm.vm.heap.buffer_byte_length(target)? == byte_length
            {
                realm.vm.heap.array_buffer_write(target, 0, &bytes)?;
            }
        }
        Ok(())
    }

    /// Reads a foreign TypedArray's unobservable shape from its owning heap.
    /// Cross-Realm algorithms must not substitute an own `length` property
    /// for these internal slots.
    pub(in super::super) fn test262_foreign_typed_array_info(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Option<(usize, TypedArrayKind)>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        if !realm.vm.heap.is_typed_array(target)? {
            return Ok(None);
        }
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        if realm.vm.heap.typed_array_is_out_of_bounds(target)? {
            return Err(RuntimeError::TypeError(
                "foreign TypedArray is detached or out of bounds".into(),
            ));
        }
        let (_, _, length, kind) = realm.vm.heap.typed_array_info(target)?;
        Ok(Some((length, kind)))
    }

    pub(in super::super) fn test262_detach_foreign_buffer_mirrors(
        &mut self,
        realm_id: ObjectId,
        target: ObjectId,
    ) -> Result<(), RuntimeError> {
        let mirrors = self
            .test262_foreign_buffer_mirrors
            .iter()
            .filter_map(|((buffer, _), mirror)| {
                (mirror.realm == realm_id && mirror.target == target).then_some(*buffer)
            })
            .collect::<Vec<_>>();
        for buffer in mirrors {
            match self.heap.is_buffer(buffer) {
                Ok(true) if !self.heap.buffer_is_detached(buffer)? => {
                    self.with_roots(|heap| heap.detach_array_buffer(buffer))?;
                }
                Ok(_) | Err(HeapError::InvalidObject(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Propagates a parent-side host detach to all child buffers that mirror
    /// this local ordinary ArrayBuffer. A single parent buffer may have been
    /// imported into more than one Test262 Realm, so every dependency is
    /// detached rather than only the most recently created view.
    pub(in super::super) fn test262_detach_local_buffer_mirrors(
        &mut self,
        buffer: ObjectId,
    ) -> Result<(), RuntimeError> {
        let dependencies = self
            .test262_foreign_buffer_mirrors
            .iter()
            .filter_map(|((local, realm), mirror)| {
                (*local == buffer).then_some((*realm, mirror.target))
            })
            .collect::<Vec<_>>();
        for (realm_id, target) in dependencies {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            if realm.vm.heap.is_buffer(target)? && !realm.vm.heap.buffer_is_detached(target)? {
                realm.vm.heap.detach_array_buffer(target)?;
            }
        }
        Ok(())
    }

    /// Pull bytes back after an operation performed through a foreign
    /// TypedArray facade. A later local-mirror flush must not overwrite that
    /// just-completed foreign indexed write with stale bytes.
    fn test262_refresh_foreign_buffer_mirrors(
        &mut self,
        realm_id: ObjectId,
        target: ObjectId,
    ) -> Result<(), RuntimeError> {
        let (byte_length, bytes) = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            if realm.vm.heap.buffer_is_detached(target)?
                || realm.vm.heap.buffer_is_immutable(target)?
            {
                return Ok(());
            }
            let byte_length = realm.vm.heap.buffer_byte_length(target)?;
            let bytes = realm.vm.heap.array_buffer_copy(target, 0, byte_length)?;
            (byte_length, bytes)
        };
        let mirrors = self
            .test262_foreign_buffer_mirrors
            .iter()
            .filter_map(|((buffer, _), mirror)| {
                (mirror.realm == realm_id && mirror.target == target).then_some(*buffer)
            })
            .collect::<Vec<_>>();
        for buffer in mirrors {
            if self.heap.is_buffer(buffer)?
                && !self.heap.buffer_is_detached(buffer)?
                && self.heap.buffer_byte_length(buffer)? == byte_length
            {
                self.with_roots(|heap| heap.array_buffer_write(buffer, 0, &bytes))?;
            }
        }
        Ok(())
    }

    /// Synchronizes data properties created on opaque ordinary-object
    /// transports while executing a foreign call. The child heap never holds
    /// a parent object ID; this explicit boundary operation re-imports every
    /// value before using the parent's ordinary [[Set]] semantics.
    fn test262_sync_imported_data_properties(
        &mut self,
        realm_id: ObjectId,
    ) -> Result<(), RuntimeError> {
        let writes = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            let imports = realm
                .imported_values
                .iter()
                .filter(|(_, imported)| imported.property_forwarding)
                .map(|(target, imported)| (*target, imported.value.clone()))
                .collect::<Vec<_>>();
            let mut writes = Vec::new();
            for (target, parent) in imports {
                for key in realm.vm.object_own_property_keys(target)? {
                    let Some(PropertyDescriptor {
                        value: Some(value), ..
                    }) = realm.vm.object_get_own_property(target, &key)?
                    else {
                        continue;
                    };
                    writes.push((parent.clone(), key, value));
                }
            }
            writes
        };
        for (parent, key, value) in writes {
            let value = self.test262_import_foreign_value(realm_id, value)?;
            self.set_property(&parent, &key, &value)?;
        }
        Ok(())
    }

    pub(in super::super) fn test262_foreign_call(
        &mut self,
        wrapper: ObjectId,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let (realm_id, target, _, constructible) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign call has a membrane record");
        // The IsConstructor check belongs to the caller (`new`,
        // Reflect.construct), so its TypeError is created in this Realm and
        // must not be delegated to the callee's.
        if construct && !constructible {
            return Err(RuntimeError::TypeError("value is not a constructor".into()));
        }
        // Proxy.revocable does not capture a realm-specific intrinsic in its
        // result; its proxy must instead retain the supplied target and
        // handler. Those values belong to the caller VM and cannot be copied
        // into an isolated Test262 child heap. Create the record locally so
        // its traps, revocation, callability, and construction all retain the
        // caller's real objects.
        let foreign_native = self
            .test262_realms
            .get(&realm_id)
            .expect("foreign realm remains live")
            .vm
            .heap
            .native_function(target)?;
        if matches!(
            foreign_native,
            Some(
                NativeFunction::TypedArrayBuffer
                    | NativeFunction::TypedArrayByteLength
                    | NativeFunction::TypedArrayByteOffset
                    | NativeFunction::TypedArrayLength
                    | NativeFunction::TypedArraySet
                    | NativeFunction::TypedArraySubarray
                    | NativeFunction::TypedArraySpecies
                    | NativeFunction::TypedArrayToStringTag
                    | NativeFunction::TypedArrayIterator(_)
                    | NativeFunction::TypedArrayMethod(_)
            )
        ) {
            self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        }
        // `%ArrayIteratorPrototype%.next` is generic across Realms: the
        // function's Realm must not decide where the receiver's
        // [[IteratedObject]] internal slot lives. Transporting a local
        // iterator as an ordinary child object would erase that slot, so run
        // the same native algorithm against the local iterator directly.
        if !construct
            && foreign_native == Some(NativeFunction::ArrayIteratorNext)
            && receiver
                .object_id()
                .is_some_and(|object| self.heap.array_iterator(object).ok().flatten().is_some())
        {
            return self.native_call(NativeFunction::ArrayIteratorNext, receiver, args, false);
        }
        // A local TypedArray may have been constructed over a foreign
        // ArrayBuffer through a mirror backing store. Keep the host detach
        // operation coherent on both sides before the ordinary membrane
        // transport would turn the local view's `.buffer` facade into an
        // opaque child object.
        if !construct && foreign_native == Some(NativeFunction::Test262("detachArrayBuffer")) {
            if let Some((buffer_realm, buffer_target, _, _)) = args
                .first()
                .and_then(Value::object_id)
                .and_then(|buffer| self.test262_foreign_reference(buffer))
            {
                if buffer_realm == realm_id {
                    let realm = self
                        .test262_realms
                        .get_mut(&realm_id)
                        .expect("foreign realm remains live");
                    realm.vm.heap.detach_array_buffer(buffer_target)?;
                    self.test262_detach_foreign_buffer_mirrors(realm_id, buffer_target)?;
                    return Ok(Value::Undefined);
                }
            }
            if let Some(buffer) = args.first().and_then(Value::object_id).filter(|buffer| {
                self.test262_foreign_buffer_mirrors
                    .contains_key(&(*buffer, realm_id))
            }) {
                // The foreign host hook received a parent-owned buffer which
                // was transported into this Realm. Detach its child mirror
                // and its parent identity together, exactly as a shared
                // cross-Realm ArrayBuffer reference would behave.
                self.test262_detach_local_buffer_mirrors(buffer)?;
                self.with_roots(|heap| heap.detach_array_buffer(buffer))?;
                return Ok(Value::Undefined);
            }
        }
        // A foreign facade obtains `Function.prototype.call` from its own
        // Realm, so `foreignTypedArrayMethod.call(localTypedArray, ...)`
        // first reaches this branch as a foreign `Call`, not as the method
        // itself. Unwrap that one level before transport: TypedArray methods
        // are generic and their local receiver (and callbacks) must stay in
        // this VM.
        let call_target_native = receiver
            .object_id()
            .and_then(|receiver| self.test262_foreign_reference(receiver))
            .filter(|(receiver_realm, _, _, _)| *receiver_realm == realm_id)
            .and_then(|(_, receiver, _, _)| {
                self.test262_realms
                    .get(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .native_function(receiver)
                    .ok()
                    .flatten()
            });
        if !construct
            && foreign_native == Some(NativeFunction::Call)
            && args
                .first()
                .and_then(Value::object_id)
                .is_none_or(|receiver| self.test262_foreign_reference(receiver).is_none())
            && matches!(
                call_target_native,
                Some(
                    NativeFunction::TypedArrayBuffer
                        | NativeFunction::TypedArrayByteLength
                        | NativeFunction::TypedArrayByteOffset
                        | NativeFunction::TypedArrayLength
                        | NativeFunction::TypedArraySet
                        | NativeFunction::TypedArraySubarray
                        | NativeFunction::TypedArraySpecies
                        | NativeFunction::TypedArrayToStringTag
                        | NativeFunction::TypedArrayIterator(_)
                        | NativeFunction::TypedArrayMethod(_)
                        | NativeFunction::TypedArrayFrom
                        | NativeFunction::TypedArrayOf
                )
            )
        {
            return self.native_call(
                call_target_native.expect("matched TypedArray native function"),
                args.first().cloned().unwrap_or(Value::Undefined),
                args.get(1..).unwrap_or_default().to_vec(),
                false,
            );
        }
        // `%TypedArray%.from` and `.of` collect source values and invoke a
        // possible mapping callback in the caller's execution context. Keep
        // those algorithms in this VM even when their constructor receiver
        // is foreign; `typed_array_from`/`typed_array_of` create and fill the
        // resulting foreign TypedArray through the membrane.
        if !construct
            && matches!(
                foreign_native,
                Some(NativeFunction::TypedArrayFrom | NativeFunction::TypedArrayOf)
            )
        {
            return self.native_call(
                foreign_native.expect("matched TypedArray static native function"),
                receiver,
                args,
                false,
            );
        }
        // TypedArray methods are intentionally generic. When a method object
        // from another Realm is called with a local receiver, execute its
        // shared native algorithm locally: its receiver and any callback
        // arguments then retain their actual local internal slots and
        // callability instead of becoming opaque child-VM transports.
        if !construct
            && receiver
                .object_id()
                .is_none_or(|receiver| self.test262_foreign_reference(receiver).is_none())
            && matches!(
                foreign_native,
                Some(
                    NativeFunction::TypedArrayBuffer
                        | NativeFunction::TypedArrayByteLength
                        | NativeFunction::TypedArrayByteOffset
                        | NativeFunction::TypedArrayLength
                        | NativeFunction::TypedArraySet
                        | NativeFunction::TypedArraySubarray
                        | NativeFunction::TypedArraySpecies
                        | NativeFunction::TypedArrayToStringTag
                        | NativeFunction::TypedArrayIterator(_)
                        | NativeFunction::TypedArrayMethod(_)
                )
            )
        {
            return self.native_call(
                foreign_native.expect("matched TypedArray native function"),
                receiver,
                args,
                false,
            );
        }
        // `%Object%` has no child-heap internal slots beyond the ordinary
        // result it creates. Running its construct path in the parent keeps
        // the caller's real `newTarget`, including a bound function whose
        // target lives in a third Test262 realm. `constructor_prototype`
        // then selects that target realm's `%Object.prototype%` normally.
        if construct && foreign_native == Some(NativeFunction::Object) {
            return self.native_call(NativeFunction::Object, receiver, args, true);
        }
        // Array construction is similarly Realm-sensitive through
        // GetPrototypeFromConstructor. Preserve the caller's `newTarget`
        // facade rather than replacing it with the foreign Array itself in a
        // child `call_native` frame.
        if construct && foreign_native == Some(NativeFunction::Array) {
            return self.native_call(NativeFunction::Array, receiver, args, true);
        }
        if construct
            && matches!(
                foreign_native,
                Some(NativeFunction::PrimitiveConstructor(_))
            )
        {
            return self.native_call(
                foreign_native.expect("matched primitive constructor"),
                receiver,
                args,
                true,
            );
        }
        // `%Proxy%` stores its supplied target and handler in the new proxy's
        // internal slots.  Transporting either object into the child VM would
        // replace it with an opaque stand-in, so later traps would lose both
        // identity and the caller Realm in which they execute.  ProxyCreate
        // itself does not capture the constructor's Realm; create the proxy
        // locally with the original objects and let normal proxy dispatch
        // choose the target function's Realm where required.
        if foreign_native == Some(NativeFunction::Proxy) {
            return self.proxy_constructor(&args, construct);
        }
        if foreign_native == Some(NativeFunction::ProxyRevocable) {
            return self.proxy_revocable(&args);
        }
        // `Function.prototype.bind.call(foreignTarget, ...)` creates a bound
        // function whose [[BoundTargetFunction]] keeps the target's Realm.
        // Keeping that record in the parent VM lets the normal bound-function
        // and GetFunctionRealm paths retain a foreign facade, instead of
        // placing an opaque child stand-in in a second child heap.
        let receiver_foreign_native = receiver
            .object_id()
            .and_then(|receiver| self.test262_foreign_reference(receiver))
            .filter(|(receiver_realm, _, _, _)| *receiver_realm == realm_id)
            .and_then(|(_, receiver, _, _)| {
                self.test262_realms
                    .get(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .native_function(receiver)
                    .ok()
                    .flatten()
            });
        if foreign_native == Some(NativeFunction::Call)
            && receiver_foreign_native == Some(NativeFunction::Bind)
        {
            return self.bind_function(
                args.first().cloned().unwrap_or(Value::Undefined),
                args.get(1..).unwrap_or_default(),
            );
        }
        // Function.prototype.call forwards its receiver as the `this` value
        // of the target function.  If that target is this realm's `apply`,
        // Apply's IsCallable check runs before it can observe the remaining
        // arguments.  Preserve that ordering at the membrane: a local object
        // in the unobserved argArray must not block the foreign TypeError.
        let receiver_is_foreign_apply = receiver
            .object_id()
            .and_then(|receiver| self.test262_foreign_reference(receiver))
            .filter(|(receiver_realm, _, _, _)| *receiver_realm == realm_id)
            .is_some_and(|(_, receiver, _, _)| {
                self.test262_realms
                    .get(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .native_function(receiver)
                    .ok()
                    == Some(Some(NativeFunction::Apply))
            });
        if foreign_native == Some(NativeFunction::Call)
            && receiver_is_foreign_apply
            && !self.is_callable(args.first().unwrap_or(&Value::Undefined))?
        {
            let error = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live")
                .vm
                .error_value(RuntimeError::TypeError("apply requires a callable".into()))?;
            return Err(RuntimeError::Thrown(
                self.test262_import_foreign_value(realm_id, error)?,
            ));
        }
        let receiver = self.test262_export_foreign_value(realm_id, &receiver)?;
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        realm.vm.construct_completion_check_failed = false;
        let result = realm
            .vm
            .call_native(Value::Object(target), receiver, args, construct);
        let result = match result {
            Ok(value) => Ok(value),
            // [[Construct]]'s completion checks (a derived constructor's
            // return value and `this` binding) run after the callee's
            // execution context is removed, so their error is created in
            // this Realm: leave it unmaterialized for the caller.
            Err(error @ (RuntimeError::TypeError(_) | RuntimeError::ReferenceError(_)))
                if construct
                    && realm.vm.construct_completion_check_failed
                    && matches!(realm.vm.heap.closure(target), Ok(Some(_))) =>
            {
                Err(error)
            }
            // RuntimeError represents spec throws until they cross a VM
            // boundary. Materialize every ordinary abrupt completion in the
            // child before import so a foreign closure, Proxy, or builtin
            // exposes the Error object from the Realm that created it.
            Err(error) => match realm.vm.error_value(error) {
                Ok(error) => Err(RuntimeError::Thrown(error)),
                Err(error) => Err(error),
            },
        };
        self.test262_sync_imported_data_properties(realm_id)?;
        let result = self.test262_import_foreign_result(realm_id, result)?;
        // OrdinaryCreateFromConstructor consulted `newTarget` only through
        // its `prototype`, and the child ran with the callee itself as
        // `newTarget`. When the caller's new target is another function, the
        // created object's [[Prototype]] must come from that function (or
        // from its Realm's matching intrinsic), so preserve that edge on the
        // caller-side facade. For CreateDynamicFunction the body and own
        // `prototype` object stay in the callee realm.
        if construct && self.new_target != Value::Object(wrapper) {
            if let (Some(intrinsic), Some(created)) = (
                foreign_native.and_then(foreign_constructor_intrinsic),
                result
                    .object_id()
                    .filter(|created| self.test262_foreign_reference(*created).is_some()),
            ) {
                // `default` is this Realm's intrinsic of the same name: the
                // right fallback when the new target belongs to this Realm.
                let default = self
                    .intrinsic_prototype(intrinsic)?
                    .object_id()
                    .expect("intrinsic prototype is an object");
                let prototype = self.constructor_prototype_for(default, Some(intrinsic))?;
                self.test262_set_foreign_prototype_override(created, prototype);
            }
        }
        Ok(result)
    }

    pub(in super::super) fn test262_foreign_next(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let Value::Object(wrapper) = receiver else {
            unreachable!("foreign next receiver is an object")
        };
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(*wrapper)
            .expect("foreign next has a membrane record");
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let result = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            let receiver = Value::Object(target);
            let next = realm.vm.get_property(&receiver, &"next".into())?;
            realm.vm.call_native(next, receiver, args, false)
        };
        self.test262_import_foreign_result(realm_id, result)
    }

    /// Test262's realm hook needs the callee's realm even when `eval` is
    /// detached from the foreign global. Each facade therefore owns a native
    /// function tagged with its realm identity rather than borrowing the
    /// caller's current global environment.
    pub(in super::super) fn test262_create_realm(&mut self) -> Result<Value, RuntimeError> {
        let mut realm = Box::new(Vm::new(self.config)?);
        // Realms created by one Test262 host execute in the same agent. The
        // GlobalSymbolRegistry is agent-wide even though every Realm keeps
        // its own global object and intrinsics.
        realm.symbol_registry = Rc::clone(&self.symbol_registry);
        // A Test262 realm exposes the same host interface as its creator.
        // In particular, the record returned by createRealm must provide an
        // evalScript function that evaluates in this child Realm.
        realm.install_test262_harness()?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            let global = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(global));
            let foreign_global = realm.global("globalThis")?.object_id().unwrap();
            let target_root = realm.heap.root(foreign_global)?;
            let wrapper_root = self.heap.root(global)?;

            let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(record));
            self.define_data(record, "global", Value::Object(global), true, true, true)?;
            self.test262_realms.insert(
                global,
                Test262Realm {
                    vm: realm,
                    wrappers: HashMap::from([(foreign_global, global)]),
                    imported_sources: HashMap::new(),
                    imported_values: HashMap::new(),
                    shadow_realm_reexports: HashMap::new(),
                },
            );
            self.test262_foreign_values.insert(
                global,
                Test262ForeignValue {
                    realm: global,
                    target: foreign_global,
                    callable: false,
                    constructible: false,
                    prototype_override: None,
                    _wrapper_root: wrapper_root,
                    _target_root: target_root,
                },
            );
            let host = self.test262_foreign_get(global, &Value::Object(global), &"$262".into())?;
            let eval_script = self.get_property(&host, &"evalScript".into())?;
            self.define_data(record, "evalScript", eval_script, true, true, true)?;
            Ok(Value::Object(record))
        })();
        self.stack.truncate(base);
        result
    }
}

/// The intrinsic whose `%X.prototype%` a built-in constructor consults, through
/// `OrdinaryCreateFromConstructor`, when `newTarget.prototype` is not an
/// object. Constructors whose result does not depend on a new target's
/// prototype (Symbol, BigInt, Proxy, ...), and those the membrane already
/// runs in the caller's Realm, have none.
fn foreign_constructor_intrinsic(function: NativeFunction) -> Option<&'static str> {
    Some(match function {
        NativeFunction::Function => "Function",
        NativeFunction::AsyncFunction => "AsyncFunction",
        NativeFunction::GeneratorFunction => "GeneratorFunction",
        NativeFunction::AsyncGeneratorFunction => "AsyncGeneratorFunction",
        NativeFunction::Date => "Date",
        NativeFunction::Promise => "Promise",
        NativeFunction::RegExp => "RegExp",
        NativeFunction::Map => "Map",
        NativeFunction::Set => "Set",
        NativeFunction::WeakMap => "WeakMap",
        NativeFunction::WeakSet => "WeakSet",
        NativeFunction::WeakRef => "WeakRef",
        NativeFunction::FinalizationRegistry => "FinalizationRegistry",
        NativeFunction::ArrayBuffer => "ArrayBuffer",
        NativeFunction::SharedArrayBuffer => "SharedArrayBuffer",
        NativeFunction::DataView => "DataView",
        NativeFunction::TypedArray(kind) => kind.name(),
        NativeFunction::Error(name) => name,
        _ => return None,
    })
}
