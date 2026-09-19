// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
            let constructor = match intrinsic {
                "Intl.Collator" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.Collator%"])
                }
                "Intl.DateTimeFormat" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.DateTimeFormat%"])
                }
                "Intl.NumberFormat" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.NumberFormat%"])
                }
                "Intl.Locale" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.Locale%"])
                }
                "Intl.DisplayNames" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.DisplayNames%"])
                }
                "Intl.DurationFormat" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.DurationFormat%"])
                }
                "Intl.ListFormat" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.ListFormat%"])
                }
                "Intl.PluralRules" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.PluralRules%"])
                }
                "Intl.RelativeTimeFormat" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.RelativeTimeFormat%"])
                }
                "Intl.Segmenter" => {
                    realm.vm.intl_global()?;
                    Value::Object(realm.vm.globals["%Intl.Segmenter%"])
                }
                _ => realm.vm.global(intrinsic)?,
            };
            realm.vm.get_property(&constructor, &"prototype".into())?
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
    /// The stand-in is deliberately an ordinary object: property forwarding
    /// needs a resumable cross-VM operation and is not implied by passing an
    /// otherwise opaque argument through a foreign call. Returning the
    /// stand-in restores the exact original parent value.
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
        let source_root = self.heap.root(source)?;
        let result = (|| {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            let prototype = realm.vm.object_prototype;
            let target = realm
                .vm
                .with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            let target_root = realm.vm.heap.root(target)?;
            realm.imported_sources.insert(source, target);
            realm.imported_values.insert(
                target,
                Test262ImportedValue {
                    value,
                    _source_root: source_root,
                    _target_root: target_root,
                },
            );
            Ok(Value::Object(target))
        })();
        if result.is_err() {
            self.heap.unroot(source_root)?;
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
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        realm.vm.set_property(&Value::Object(target), key, &value)
    }

    pub(in super::super) fn test262_foreign_call(
        &mut self,
        wrapper: ObjectId,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign call has a membrane record");
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
        let result = realm
            .vm
            .call_native(Value::Object(target), receiver, args, construct);
        let result = match result {
            Ok(value) => Ok(value),
            // RuntimeError represents spec throws until they cross a VM
            // boundary. Materialize every ordinary abrupt completion in the
            // child before import so a foreign closure, Proxy, or builtin
            // exposes the Error object from the Realm that created it.
            Err(error) => match realm.vm.error_value(error) {
                Ok(error) => Err(RuntimeError::Thrown(error)),
                Err(error) => Err(error),
            },
        };
        let result = self.test262_import_foreign_result(realm_id, result)?;
        if construct && foreign_native == Some(NativeFunction::Function) {
            // CreateDynamicFunction uses `newTarget` only to select the
            // function object's [[Prototype]].  Its body and own
            // `prototype` object remain in the callee realm.  Preserve that
            // cross-realm edge on the caller-side facade.
            let default = self.function_prototype()?;
            let prototype = self.constructor_prototype(default)?;
            if let Some(wrapper) = result.object_id() {
                self.test262_set_foreign_prototype_override(wrapper, prototype);
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
