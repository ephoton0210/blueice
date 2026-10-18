// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::super::realm_call_storage::{RealmCallInput, RealmCallRoots};
use super::super::realm_reentrancy::{prepare_active_registration, ActiveRegistration};
use super::*;
use crate::heap::TypedArrayKind;

impl Vm {
    pub(in super::super) fn test262_realm_handle(
        &self,
        realm_id: ObjectId,
    ) -> Result<realm_cells::RealmHandle, RuntimeError> {
        self.test262_realms
            .get(&realm_id)
            .map(|realm| realm.vm.clone())
            .ok_or_else(|| {
                RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
            })
    }

    fn test262_realm_native(
        &self,
        realm_id: ObjectId,
        target: ObjectId,
    ) -> Result<Option<NativeFunction>, RuntimeError> {
        self.test262_realm_handle(realm_id)?
            .read(self.object_prototype.heap, |vm| {
                vm.heap.native_function(target).map_err(Into::into)
            })
    }

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
        self.test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| {
                vm.remaining_instructions = vm.config.instruction_budget;
                let intrinsic = vm.global(intrinsic)?.object_id();
                Ok(intrinsic == Some(target))
            })
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
        self.test262_realm_native(realm_id, target)
    }

    pub(in super::super) fn test262_foreign_regexp_data(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<(JsString, String)>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        self.test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| {
                Ok(vm
                    .heap
                    .regexp(target)
                    .expect("the membrane owns a root for its live foreign target")
                    .map(|regexp| (regexp.source.clone(), regexp.flags.clone())))
            })
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
        self.test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| {
                Ok(vm.heap.date_value(target).ok())
            })
    }

    pub(in super::super) fn test262_foreign_boxed_primitive(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        self.test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| {
                vm.heap.boxed_primitive(target).map_err(Into::into)
            })
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
        let child = self.test262_realm_handle(realm_id)?;
        let caller = self.object_prototype.heap;
        let (callable, constructible, proxy_parts, target_root) = child.edit(caller, |vm| {
            let value = Value::Object(target);
            let callable = vm.is_callable(&value)?;
            let constructible = vm
                .is_constructor(&value)
                .expect("the preceding callability check validated this live child-heap value");
            // A live foreign Proxy needs a local exotic facade, rather than
            // the ordinary wrapper used for other foreign values. Its
            // internal methods create argument and descriptor objects in the
            // *current* Realm before calling a possibly foreign trap. An
            // ordinary facade would instead run the complete Proxy operation
            // in the child Realm, leaking that Realm through those objects.
            // A revoked proxy remains an ordinary foreign facade: observing
            // it immediately throws and there are no live slots to mirror.
            let proxy_parts = vm.heap.proxy(target).ok().flatten();
            let root = vm.heap.root(target)?;
            Ok((callable, constructible, proxy_parts, root))
        })?;
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
                child.edit(caller, |vm| {
                    vm.heap
                        .unroot(target_root)
                        .expect("this failed membrane construction still owns its target root");
                    Ok(())
                })?;
                return Err(error);
            }
        };
        let wrapper_root = match self.heap.root(wrapper) {
            Ok(root) => root,
            Err(error) => {
                child.edit(caller, |vm| {
                    vm.heap
                        .unroot(target_root)
                        .expect("this failed membrane construction still owns its target root");
                    Ok(())
                })?;
                return Err(error.into());
            }
        };
        let prepared = (|| {
            let wrappers = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live")
                .wrappers
                .prepare_insert(self.host_scheduling.as_ref())?;
            let values = self
                .test262_foreign_values
                .prepare_insert(self.host_scheduling.as_ref())?;
            Ok::<_, RuntimeError>((wrappers, values))
        })();
        let (wrappers, values) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.release_root(wrapper_root);
                child.edit(caller, |vm| {
                    vm.release_root(target_root);
                    Ok(())
                })?;
                return Err(error);
            }
        };
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.wrappers.commit(wrappers);
        self.test262_foreign_values.commit(values);
        realm.wrappers.insert(target, wrapper);
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
        let prototype =
            self.test262_with_foreign_vm(realm_id, |vm| vm.object_get_prototype(target))?;
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
        let prototype = self
            .test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| {
                vm.intrinsic_prototype(intrinsic)
            })?;
        // Callers name a standard intrinsic. Its cached constructor identity
        // survives replacement of the corresponding global property, and its
        // own prototype property is a non-writable, non-configurable object.
        Ok(self
            .test262_import_foreign_value(realm_id, prototype)?
            .object_id()
            .expect("a standard intrinsic prototype remains an object across the membrane"))
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

    /// An owned primitive crosses without allocating another backing buffer.
    fn test262_export_foreign_value_owned(
        &mut self,
        realm_id: ObjectId,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        if value.object_id().is_some() {
            self.test262_export_foreign_value(realm_id, &value)
        } else {
            Ok(value)
        }
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
            .test262_realm_handle(source_realm_id)?
            .read(self.object_prototype.heap, |vm| {
                Ok(vm.shadow_realm_record(target))
            })?
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
        // Intrinsic setup can allocate and collect. Prepare only after it has
        // finished, while the unpublished instance has an explicit root.
        let child = self.test262_realm_handle(realm_id)?;
        let caller = self.object_prototype.heap;
        let (instance, root) = child.edit(caller, |vm| {
            let default = vm.shadow_realm_prototype()?;
            let instance = vm.with_roots(|heap| heap.alloc_object(Some(default)))?;
            let root = vm.heap.root(instance)?;
            Ok((instance, root))
        })?;
        let prepared = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live")
            .shadow_realm_reexports
            .prepare_insert(self.host_scheduling.as_ref());
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                child.edit(caller, |vm| {
                    vm.release_root(root);
                    Ok(())
                })?;
                return Err(error);
            }
        };
        child.edit(caller, |vm| {
            let adopted = vm.adopt_shadow_realm(instance, source_record);
            vm.release_root(root);
            adopted
        })?;
        let cache = &mut self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live")
            .shadow_realm_reexports;
        cache.commit(prepared);
        cache.insert(target, instance);
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
        let typed_array_values =
            if self
                .heap
                .is_typed_array(source)
                .expect("the source was validated before the array snapshot")
            {
                let (buffer, _, length, kind) = self.heap.typed_array_info(source).expect(
                    "validated transport metadata is read without JavaScript or collection",
                );
                if self
                    .heap
                    .buffer_is_detached(buffer)
                    .expect("validated transport metadata is read without JavaScript or collection")
                    || self.heap.typed_array_is_out_of_bounds(source).expect(
                        "validated transport metadata is read without JavaScript or collection",
                    )
                {
                    return Err(RuntimeError::TypeError(
                        "TypedArray source is detached or out of bounds".into(),
                    ));
                }
                let values = (0..length)
                    .map(|index| {
                        // This snapshot runs no JavaScript and does not collect.
                        // The validated buffer cannot shrink between these reads;
                        // a shared backing may only grow on another agent.
                        self.heap
                            .typed_array_index_value(source, index)
                            .expect("the validated TypedArray remains live during its snapshot")
                            .expect("each snapshot index is below the validated array length")
                    })
                    .collect::<Vec<_>>();
                Some((kind, values))
            } else {
                None
            };
        // A child heap cannot retain a parent ArrayBuffer object identity.
        // Allocate an equivalent child buffer and remember the pair so
        // direct indexed writes and host detachment remain observable in
        // both realms. Shared buffers may retain their Arc backing directly;
        // ordinary buffers use a synchronized byte mirror.
        let buffer_transport =
            if self
                .heap
                .is_buffer(source)
                .expect("the source kind cannot change during membrane transport")
            {
                let shared = self.heap.buffer_is_shared(source).expect(
                    "validated transport metadata is read without JavaScript or collection",
                );
                let detached = self.heap.buffer_is_detached(source).expect(
                    "validated transport metadata is read without JavaScript or collection",
                );
                let byte_length = self.heap.buffer_byte_length(source).expect(
                    "validated transport metadata is read without JavaScript or collection",
                );
                let maximum = self.heap.buffer_max_byte_length(source).expect(
                    "validated transport metadata is read without JavaScript or collection",
                );
                let flexible = self.heap.buffer_resizable(source).expect(
                    "validated transport metadata is read without JavaScript or collection",
                ) || self.heap.buffer_growable(source).expect(
                    "validated transport metadata is read without JavaScript or collection",
                );
                let bytes = (!shared && !detached).then(|| {
                    self.heap.array_buffer_copy(source, 0, byte_length).expect(
                        "the snapshot copies the validated live buffer within its current length",
                    )
                });
                let backing = shared.then(|| {
                    self.heap
                        .shared_buffer_backing(source)
                        .expect("the validated shared buffer retains its backing")
                });
                Some((
                    shared,
                    detached,
                    byte_length,
                    maximum,
                    flexible,
                    bytes,
                    backing,
                ))
            } else {
                None
            };
        let ordinary_buffer = buffer_transport
            .as_ref()
            .is_some_and(|(shared, ..)| !shared);
        let immutable_buffer = ordinary_buffer
            && self
                .heap
                .buffer_is_immutable(source)
                .expect("validated transport metadata is read without JavaScript or collection");
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
        let callable = self
            .is_callable(&Value::Object(source))
            .expect("the transport source was validated without running JavaScript");
        // The reverse-membrane facade this function may allocate below (the
        // opaque, property-forwarding case) needs its own constructible bit,
        // exactly like the forward direction's `test262_import_foreign_value`
        // already computes for its own facades.
        let constructible = self
            .is_constructor(&Value::Object(source))
            .expect("the transport source was validated without running JavaScript");
        let child = self.test262_realm_handle(realm_id)?;
        let home_heap = self.object_prototype.heap;
        let source_root = self.heap.root(source)?;
        // A second, independent root for the same `source` object, held by
        // the reverse-membrane record itself (see `Test262ReverseValue`).
        // Only ever created for the opaque, property-forwarding case (the
        // final `else` branch below): Arrays/TypedArrays/ArrayBuffers keep
        // their existing eager-snapshot transport instead, so `property_
        // forwarding`'s value -- already known here -- decides this before
        // the closure below borrows `self` through `realm` and makes a
        // second `self.heap.root` call impossible.
        let reverse_target_root = property_forwarding
            .then(|| self.heap.root(source))
            .transpose()
            .map_err(|error| {
                self.release_root(source_root);
                RuntimeError::from(error)
            })?;
        let result = child.edit(self.object_prototype.heap, |vm| {
            let target =
                if let Some((shared, detached, byte_length, maximum, flexible, bytes, backing)) =
                    &buffer_transport
                {
                    let prototype = if *shared {
                        vm.buffer_prototype("SharedArrayBuffer")?
                    } else {
                        vm.buffer_prototype("ArrayBuffer")?
                    };
                    let buffer = if let Some(backing) = backing.clone() {
                        vm.with_roots(|heap| {
                            heap.alloc_shared_array_buffer_backing(
                                backing,
                                flexible.then_some(*maximum),
                                Some(prototype),
                            )
                        })?
                    } else if immutable_buffer {
                        // An immutable buffer stays immutable in the child realm;
                        // its bytes are supplied once, at allocation.
                        let bytes = bytes.clone().unwrap_or_default();
                        vm.with_roots(|heap| {
                            heap.alloc_immutable_array_buffer(bytes, Some(prototype))
                        })?
                    } else if *flexible {
                        vm.with_roots(|heap| {
                            heap.alloc_resizable_array_buffer(
                                *byte_length,
                                *maximum,
                                Some(prototype),
                            )
                        })?
                    } else {
                        vm.with_roots(|heap| {
                            heap.alloc_array_buffer(*byte_length, Some(prototype))
                        })?
                    };
                    if *detached {
                        vm.heap.detach_array_buffer(buffer).expect(
                            "the fresh ordinary snapshot has not been exposed to JavaScript",
                        );
                    } else if let (Some(bytes), false) = (bytes, immutable_buffer) {
                        vm.heap.array_buffer_write(buffer, 0, bytes).expect(
                            "the fresh writable snapshot has exactly the copied byte length",
                        );
                    }
                    buffer
                } else if let Some(values) = array_values {
                    vm.array_from(values)?
                        .object_id()
                        .expect("array construction returns an object")
                } else if let Some((kind, values)) = typed_array_values {
                    let constructor = vm.global(kind.name())?;
                    let target = vm
                        .call_native(
                            constructor,
                            Value::Undefined,
                            vec![Value::Number(values.len() as f64)],
                            true,
                        )?
                        .object_id()
                        .expect("TypedArray construction returns an object");
                    for (index, value) in values.iter().enumerate() {
                        vm.heap
                            .typed_array_set_index(target, index, value)
                            .expect("the intrinsic snapshot has the source kind and length");
                    }
                    target
                } else {
                    let prototype = vm.object_prototype;
                    vm.with_roots(|heap| heap.alloc_object(Some(prototype)))?
                };
            let target_root = vm.heap.root(target)?;
            let wrapper_root = if property_forwarding {
                Some(vm.heap.root(target).map_err(|error| {
                    vm.release_root(target_root);
                    RuntimeError::from(error)
                })?)
            } else {
                None
            };
            // All guest constructors and heap allocation have finished. Admit
            // every table in both heaps before publishing either root pair.
            let prepared = (|| {
                let realm = self
                    .test262_realms
                    .get_mut(&realm_id)
                    .expect("foreign realm remains live");
                let sources = realm
                    .imported_sources
                    .prepare_insert(self.host_scheduling.as_ref())?;
                let values = realm
                    .imported_values
                    .prepare_insert(self.host_scheduling.as_ref())?;
                let mirrors = if ordinary_buffer {
                    Some(
                        self.test262_foreign_buffer_mirrors
                            .prepare_insert(self.host_scheduling.as_ref())?,
                    )
                } else {
                    None
                };
                let reverse = if property_forwarding {
                    Some(
                        vm.test262_reverse_values
                            .prepare_insert(vm.host_scheduling.as_ref())?,
                    )
                } else {
                    None
                };
                Ok::<_, RuntimeError>((sources, values, mirrors, reverse))
            })();
            let (sources, values, mirrors, reverse) = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    vm.release_root(target_root);
                    if let Some(root) = wrapper_root {
                        vm.release_root(root);
                    }
                    return Err(error);
                }
            };
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.imported_sources.commit(sources);
            realm.imported_values.commit(values);
            if let Some(prepared) = mirrors {
                self.test262_foreign_buffer_mirrors.commit(prepared);
            }
            if let Some(prepared) = reverse {
                vm.test262_reverse_values.commit(prepared);
                vm.test262_reverse_values.insert(
                    target,
                    Test262ReverseValue {
                        home_heap,
                        target: source,
                        callable,
                        constructible,
                        prototype_override: None,
                        _wrapper_root: wrapper_root.expect("forwarding facade owns its root"),
                        _target_root: reverse_target_root.expect("forwarding source owns its root"),
                    },
                );
            }
            realm.imported_sources.insert(source, target);
            realm.imported_values.insert(
                target,
                Test262ImportedValue {
                    value,
                    _source_root: source_root,
                    _target_root: target_root,
                },
            );
            if ordinary_buffer {
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
            Ok(Value::Object(target))
        });
        if result.is_err() {
            self.release_root(source_root);
            if let Some(root) = reverse_target_root {
                self.release_root(root);
            }
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
        let result = self.test262_with_foreign_vm(realm_id, |vm| {
            vm.get_object_property(target, &receiver, key)
        });
        let value = self.test262_foreign_completion(realm_id, result)?;
        self.test262_import_foreign_value(realm_id, value)
    }

    /// Forwards a foreign facade's [[DefineOwnProperty]] into its Realm, with
    /// the descriptor's value / accessor functions exported across the
    /// membrane (a facaded TypedArray keeps its buffer mirrors in step, as
    /// [[Set]] does), so a detached or out-of-range element is judged by the
    /// owning realm's internal method rather than by the empty local record.
    pub(in super::super) fn test262_foreign_define_own_property(
        &mut self,
        wrapper: ObjectId,
        key: PropertyName,
        mut descriptor: PropertyDescriptor,
    ) -> Result<bool, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign defineOwnProperty has a membrane record");
        let typed_buffer = self
            .test262_realm_handle(realm_id)?
            .read(self.object_prototype.heap, |vm| {
                Ok(vm.heap.typed_array_info(target).ok())
            })?
            .map(|(buffer, _, _, _)| buffer);
        if typed_buffer.is_some() {
            self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        }
        for field in [
            &mut descriptor.value,
            &mut descriptor.get,
            &mut descriptor.set,
        ] {
            if let Some(value) = field.take() {
                *field = Some(self.test262_export_foreign_value(realm_id, &value)?);
            }
        }
        let result = self.test262_with_foreign_vm(realm_id, |vm| {
            vm.object_define_own_property(target, key, descriptor)
        });
        if let Some(buffer) = typed_buffer {
            self.test262_refresh_foreign_buffer_mirrors(realm_id, buffer)?;
        }
        self.test262_foreign_completion(realm_id, result)
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
        let result =
            self.test262_with_foreign_vm(realm_id, |vm| vm.object_own_property_keys(target));
        self.test262_foreign_completion(realm_id, result)
    }

    /// Materializes an abrupt completion of an operation run in a foreign
    /// Realm as that Realm's own Error object and imports it, so the caller
    /// observes an error from the Realm whose code failed. Resource errors
    /// pass through unchanged.
    fn test262_foreign_completion<T>(
        &mut self,
        realm_id: ObjectId,
        result: Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        match result {
            Ok(value) => Ok(value),
            Err(RuntimeError::Thrown(value)) => Err(RuntimeError::Thrown(
                self.test262_import_foreign_value(realm_id, value)?,
            )),
            Err(error) => Err(self.test262_create_error_in_realm(realm_id, error)?),
        }
    }

    /// An unmaterialized language error (`TypeError`, `RangeError`,
    /// `ReferenceError`, `SyntaxError`) becomes the corresponding Error
    /// object of the foreign Realm `realm_id`, imported here. Every other
    /// completion, resource errors and already thrown values included, is
    /// returned unchanged.
    fn test262_create_error_in_realm(
        &mut self,
        realm_id: ObjectId,
        error: RuntimeError,
    ) -> Result<RuntimeError, RuntimeError> {
        if !matches!(
            error,
            RuntimeError::TypeError(_)
                | RuntimeError::RangeError(_)
                | RuntimeError::ReferenceError(_)
                | RuntimeError::SyntaxError(_)
        ) {
            return Ok(error);
        }
        let error = self
            .test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| vm.error_value(error))?;
        Ok(RuntimeError::Thrown(
            self.test262_import_foreign_value(realm_id, error)?,
        ))
    }

    fn prepare_foreign_active_registration(
        &mut self,
        realm_id: ObjectId,
    ) -> Result<ActiveRegistration, RuntimeError> {
        let child = self.test262_realm_handle(realm_id)?;
        let owner = child.read(self.object_prototype.heap, |vm| {
            Ok(vm.host_scheduling.as_ref().map(SchedulingScope::child))
        })?;
        prepare_active_registration(self, owner.as_ref())
    }

    /// Internal methods can execute accessors, coercions and Proxy traps.
    /// Keep the parent reachable through the reverse membrane for their
    /// complete dynamic extent, without retaining a borrow of the realm map.
    fn test262_with_foreign_vm<T>(
        &mut self,
        realm_id: ObjectId,
        operation: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        self.test262_with_foreign_vm_mode(realm_id, true, operation)
    }

    /// Host task delivery establishes ancestor reentrancy without changing fuel.
    pub(in super::super) fn test262_with_host_realm<T>(
        &mut self,
        realm_id: ObjectId,
        operation: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        self.test262_with_foreign_vm_mode(realm_id, false, operation)
    }

    fn test262_with_foreign_vm_mode<T>(
        &mut self,
        realm_id: ObjectId,
        reset_fuel: bool,
        operation: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let registration = self.prepare_foreign_active_registration(realm_id)?;
        let child = self.test262_realm_handle(realm_id)?;
        let caller = self.object_prototype.heap;
        let guard = registration.publish(self);
        let result = child.edit(caller, |vm| {
            if reset_fuel {
                vm.remaining_instructions = vm.config.instruction_budget;
            }
            operation(vm)
        });
        drop(guard);
        result
    }

    /// The owning Realm and target behind an internal-method facade.
    fn test262_foreign_target(&self, wrapper: ObjectId) -> (ObjectId, ObjectId) {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign internal method has a membrane record");
        (realm_id, target)
    }

    /// [[Delete]] of a foreign facade, performed in its Realm.
    pub(in super::super) fn test262_foreign_delete(
        &mut self,
        wrapper: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        let (realm_id, target) = self.test262_foreign_target(wrapper);
        let result = self.test262_with_foreign_vm(realm_id, |vm| vm.object_delete(target, key));
        self.test262_foreign_completion(realm_id, result)
    }

    /// [[IsExtensible]] of a foreign facade, performed in its Realm.
    pub(in super::super) fn test262_foreign_is_extensible(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let (realm_id, target) = self.test262_foreign_target(wrapper);
        let result = self.test262_with_foreign_vm(realm_id, |vm| vm.object_is_extensible(target));
        self.test262_foreign_completion(realm_id, result)
    }

    /// [[PreventExtensions]] of a foreign facade, performed in its Realm.
    pub(in super::super) fn test262_foreign_prevent_extensions(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let (realm_id, target) = self.test262_foreign_target(wrapper);
        let result =
            self.test262_with_foreign_vm(realm_id, |vm| vm.object_prevent_extensions(target));
        self.test262_foreign_completion(realm_id, result)
    }

    /// [[SetPrototypeOf]] of a foreign facade, performed in its Realm. A
    /// prototype belonging to another Realm crosses the membrane like any
    /// other value, and replaces the prototype recorded by construction.
    pub(in super::super) fn test262_foreign_set_prototype(
        &mut self,
        wrapper: ObjectId,
        prototype: Option<ObjectId>,
    ) -> Result<bool, RuntimeError> {
        let (realm_id, _) = self.test262_foreign_target(wrapper);
        let prototype = prototype
            .map(|prototype| {
                self.test262_export_foreign_value(realm_id, &Value::Object(prototype))
                    .map(|value| {
                        value
                            .object_id()
                            .expect("an exported object stays an object")
                    })
            })
            .transpose()?;
        let (_, target) = self.test262_foreign_target(wrapper);
        let result =
            self.test262_with_foreign_vm(realm_id, |vm| vm.object_set_prototype(target, prototype));
        let succeeded = self.test262_foreign_completion(realm_id, result)?;
        if succeeded {
            self.test262_foreign_values
                .get_mut(&wrapper)
                .expect("foreign facade keeps its membrane record")
                .prototype_override = None;
        }
        Ok(succeeded)
    }

    /// Runs a foreign facade's [[GetOwnProperty]] in its owning Realm.
    /// `Object.keys`/`getOwnPropertyDescriptor`/`hasOwnProperty`/`in`/
    /// spread/`for-in`/`JSON.stringify` all ultimately reach this internal
    /// method, so without it every one of those observes a foreign facade
    /// as permanently empty regardless of `test262_foreign_own_property_
    /// keys`' own already-correct key list.
    pub(in super::super) fn test262_foreign_get_own_property(
        &mut self,
        wrapper: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<PropertyDescriptor>, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign own-property has a membrane record");
        let result =
            self.test262_with_foreign_vm(realm_id, |vm| vm.object_get_own_property(target, key));
        let descriptor = self.test262_foreign_completion(realm_id, result)?;
        let Some(descriptor) = descriptor else {
            return Ok(None);
        };
        Ok(Some(PropertyDescriptor {
            value: descriptor
                .value
                .map(|value| self.test262_import_foreign_value(realm_id, value))
                .transpose()?,
            writable: descriptor.writable,
            get: descriptor
                .get
                .map(|value| self.test262_import_foreign_value(realm_id, value))
                .transpose()?,
            set: descriptor
                .set
                .map(|value| self.test262_import_foreign_value(realm_id, value))
                .transpose()?,
            enumerable: descriptor.enumerable,
            configurable: descriptor.configurable,
        }))
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
        let result = self.test262_with_foreign_vm(realm_id, |vm| {
            vm.ordinary_set_with_receiver(target, &receiver, key, &value)
        });
        self.test262_foreign_completion(realm_id, result)
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
            .test262_realm_handle(realm_id)?
            .read(self.object_prototype.heap, |vm| {
                Ok(vm.heap.typed_array_info(target).ok())
            })?
            .map(|(buffer, _, _, _)| buffer);
        if typed_buffer.is_some() {
            self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        }
        // Test262 harness helpers are installed independently in every
        // Realm. A fixture may explicitly copy (for example)
        // `assert.sameValue` into a child Realm; preserving that child's
        // equivalent native helper keeps the call boundary functional rather
        // than replacing it with the deliberately opaque ordinary-object
        // transport used for arbitrary parent objects. Only harness helpers
        // are interchangeable: any other built-in (`RegExp`, `Object`, ...)
        // of this Realm is a distinct function from its namesake there.
        let native = value
            .object_id()
            .map(|object| self.heap.native_function(object))
            .transpose()?
            .flatten()
            .filter(|native| matches!(native, NativeFunction::Test262(_)));
        let equivalent_native = if let Some(native) = native {
            let result = self.test262_with_foreign_vm(realm_id, |vm| {
                let current = vm.get_object_property(target, &Value::Object(target), key)?;
                Ok(current
                    .object_id()
                    .filter(|current| vm.heap.native_function(*current).ok() == Some(Some(native)))
                    .map(Value::Object))
            });
            self.test262_foreign_completion(realm_id, result)?
        } else {
            None
        };
        let value = match equivalent_native {
            Some(value) => value,
            None => self.test262_export_foreign_value(realm_id, value)?,
        };
        let result = self.test262_with_foreign_vm(realm_id, |vm| {
            vm.set_property(&Value::Object(target), key, &value)
        });
        if let Some(buffer) = typed_buffer {
            self.test262_refresh_foreign_buffer_mirrors(realm_id, buffer)?;
        }
        self.test262_foreign_completion(realm_id, result)
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
            .test262_realm_handle(realm_id)?
            .read(self.object_prototype.heap, |vm| {
                Ok(vm.heap.typed_array_info(target)?.0)
            })?;
        let registration = self.prepare_foreign_active_registration(realm_id)?;
        self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let child = self.test262_realm_handle(realm_id)?;
        let caller = self.object_prototype.heap;
        let _guard = registration.publish(self);
        let result = child.edit(caller, |child| {
            child.remaining_instructions = child.config.instruction_budget;
            let result = child.native_call(function, Value::Object(target), args, construct);
            match result {
                Ok(value) => Ok(value),
                Err(error) => child
                    .error_value(error)
                    .and_then(|error| Err(RuntimeError::Thrown(error))),
            }
        });
        drop(_guard);
        self.test262_refresh_foreign_buffer_mirrors(realm_id, source_buffer)?;
        self.test262_import_foreign_result(realm_id, result)
    }

    /// Runs an `ArrayBuffer`/`SharedArrayBuffer` `slice`-family method
    /// directly against the real buffer object in its owning Test262 Realm,
    /// the same shape as `test262_foreign_typed_array_native_call` above.
    /// Unlike a TypedArray view, the receiver here *is* the buffer, so the
    /// membrane target doubles as the buffer-mirror sync key instead of one
    /// reached through `typed_array_info`. Running in the owning Realm
    /// (rather than re-fetching `slice` as a property in the caller's Realm,
    /// `test262_foreign_next`'s approach) is required so
    /// `%ArrayBuffer.prototype.slice%`'s SpeciesConstructor lookup reads
    /// that Realm's own `constructor`/`@@species` off the real receiver,
    /// matching `slice-species.js`'s
    /// `ArrayBuffer.prototype.slice.call(g.a, 8, 16)` cross-realm case
    /// (`b.constructor === g.ArrayBuffer`).
    pub(in super::super) fn test262_foreign_array_buffer_native_call(
        &mut self,
        function: NativeFunction,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let wrapper = receiver
            .object_id()
            .expect("foreign ArrayBuffer receiver has an object identity");
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign ArrayBuffer receiver has a membrane record");
        let registration = self.prepare_foreign_active_registration(realm_id)?;
        // A local TypedArray may have been constructed over this same
        // ArrayBuffer's mirror (`test262_foreign_buffer_clone`); flush any
        // bytes written through it before `slice` reads from the owning
        // Realm's real target, exactly as the TypedArray-method path above
        // does before re-entering that Realm.
        self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let child = self.test262_realm_handle(realm_id)?;
        let caller = self.object_prototype.heap;
        let _guard = registration.publish(self);
        let result = child.edit(caller, |child| {
            child.remaining_instructions = child.config.instruction_budget;
            let result = child.native_call(function, Value::Object(target), args, construct);
            match result {
                Ok(value) => Ok(value),
                Err(error) => child
                    .error_value(error)
                    .and_then(|error| Err(RuntimeError::Thrown(error))),
            }
        });
        drop(_guard);
        // None of this family mutates its source buffer's bytes (`slice`
        // and `sliceToImmutable` only read from `target` and allocate a new
        // buffer), so no local mirror ever goes stale here. Refreshing
        // anyway keeps this call symmetric with the TypedArray-method path
        // and stays correct if a future family member (e.g. an in-place
        // resize) is ever added to this branch.
        self.test262_refresh_foreign_buffer_mirrors(realm_id, target)?;
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
        let Some(source_buffer) =
            self.test262_realm_handle(realm_id)?
                .read(self.object_prototype.heap, |vm| {
                    if !vm.heap.is_typed_array(target)? {
                        return Ok(None);
                    }
                    Ok(Some(vm.heap.typed_array_info(target)?.0))
                })?
        else {
            return Ok(None);
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
            .collect::<Result<Vec<_>, _>>()
            .expect("the first argument already belongs to this realm; coercion made every other argument primitive");
        let result = self.test262_with_foreign_vm(realm_id, |vm| {
            vm.native_call(function, Value::Undefined, args, false)
        });
        self.test262_refresh_foreign_buffer_mirrors(realm_id, source_buffer)
            .expect(
                "Atomics retains a validated shared backing, which has no mutable ordinary mirrors",
            );
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
        self.test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| {
                if !vm.heap.is_typed_array(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                ) {
                    return Ok(None);
                }
                vm.remaining_instructions = vm.config.instruction_budget;
                if vm
                    .heap
                    .typed_array_is_out_of_bounds(target)
                    .expect("the foreign TypedArray brand was checked without allocation")
                {
                    return Err(RuntimeError::TypeError(
                        "foreign TypedArray is detached or out of bounds".into(),
                    ));
                }
                let (_, _, length, _) = vm
                    .heap
                    .typed_array_info(target)
                    .expect("the foreign TypedArray remains live during this pure snapshot");
                Ok(Some(
                    (0..length)
                        .map(|index| {
                            vm
                        .heap
                        .typed_array_index_value(target, index)
                        .expect("the validated foreign TypedArray remains live during its snapshot")
                        .expect("a snapshot without JavaScript cannot invalidate a checked index")
                        })
                        .collect::<Vec<_>>(),
                ))
            })
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
        // Views over the same foreign buffer must share one local backing
        // store. Independent snapshots would overwrite one another when
        // flushed and would hide writes made through a sibling view.
        if let Some(buffer) =
            self.test262_foreign_buffer_mirrors
                .iter()
                .find_map(|((buffer, _), mirror)| {
                    (mirror.realm == realm_id && mirror.target == target && mirror.facade.is_some())
                        .then_some(*buffer)
                })
        {
            return Ok(Some(buffer));
        }
        let Some((shared, detached, byte_length, maximum, flexible, bytes, backing)) = self
            .test262_realm_handle(realm_id)?
            .read(self.object_prototype.heap, |vm| {
                if !vm.heap.is_buffer(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                ) {
                    return Ok(None);
                }
                let shared = vm.heap.buffer_is_shared(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                );
                let detached = vm.heap.buffer_is_detached(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                );
                let byte_length = vm.heap.buffer_byte_length(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                );
                let maximum = vm.heap.buffer_max_byte_length(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                );
                let flexible = vm.heap.buffer_resizable(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                ) || vm.heap.buffer_growable(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                );
                let bytes = (!shared && !detached).then(|| {
                    vm.heap.array_buffer_copy(target, 0, byte_length).expect(
                        "the rooted buffer snapshot uses the checked byte range without JavaScript",
                    )
                });
                let backing = shared.then(|| {
                    vm.heap.shared_buffer_backing(target).expect(
                        "the rooted buffer snapshot uses the checked byte range without JavaScript",
                    )
                });
                Ok(Some((
                    shared,
                    detached,
                    byte_length,
                    maximum,
                    flexible,
                    bytes,
                    backing,
                )))
            })?
        else {
            return Ok(None);
        };
        let immutable = !shared
            && self
                .test262_realm_handle(realm_id)?
                .read(self.object_prototype.heap, |vm| {
                    vm.heap.buffer_is_immutable(target).map_err(Into::into)
                })?;
        let prototype = if shared {
            self.buffer_prototype("SharedArrayBuffer")?
        } else {
            self.buffer_prototype("ArrayBuffer")?
        };
        let buffer = if let Some(backing) = backing {
            self.with_roots(|heap| {
                heap.alloc_shared_array_buffer_backing(
                    backing,
                    flexible.then_some(maximum),
                    Some(prototype),
                )
            })?
        } else if immutable {
            let bytes = bytes.clone().unwrap_or_default();
            self.with_roots(|heap| heap.alloc_immutable_array_buffer(bytes, Some(prototype)))?
        } else if flexible {
            self.with_roots(|heap| {
                heap.alloc_resizable_array_buffer(byte_length, maximum, Some(prototype))
            })?
        } else {
            self.with_roots(|heap| heap.alloc_array_buffer(byte_length, Some(prototype)))?
        };
        if detached {
            self.heap
                .detach_array_buffer(buffer)
                .expect("the newly allocated local mirror is an ordinary buffer");
        }
        if let Some(bytes) = bytes {
            if !immutable {
                self.heap
                    .array_buffer_write(buffer, 0, &bytes)
                    .expect("the fresh mutable mirror has the exact snapshotted byte length");
            }
            let buffer_root = self.heap.root(buffer)?;
            let prepared = match self
                .test262_foreign_buffer_mirrors
                .prepare_insert(self.host_scheduling.as_ref())
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.release_root(buffer_root);
                    return Err(error);
                }
            };
            self.test262_foreign_buffer_mirrors.commit(prepared);
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
            // Imported transport records retain the parent source and child
            // target; child-to-parent mirrors retain their local buffer root.
            if self
                .heap
                .buffer_is_detached(buffer)
                .expect("the retained membrane mirror owns this live buffer and checked byte range")
                || self.heap.buffer_is_immutable(buffer).expect(
                    "the retained membrane mirror owns this live buffer and checked byte range",
                )
            {
                continue;
            }
            let byte_length = self.heap.buffer_byte_length(buffer).expect(
                "the retained membrane mirror owns this live buffer and checked byte range",
            );
            let bytes = self.heap.array_buffer_copy(buffer, 0, byte_length).expect(
                "the retained membrane mirror owns this live buffer and checked byte range",
            );
            self.test262_realm_handle(realm_id)?
                .edit(self.object_prototype.heap, |vm| {
                    if !vm.heap.buffer_is_detached(target).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    ) && !vm.heap.buffer_is_immutable(target).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    ) && vm.heap.buffer_byte_length(target).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    ) == byte_length
                    {
                        vm.heap.array_buffer_write(target, 0, &bytes).expect(
                    "the retained membrane mirror owns this live buffer and checked byte range",
                );
                    }
                    Ok(())
                })?;
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
        self.test262_realm_handle(realm_id)?
            .edit(self.object_prototype.heap, |vm| {
                if !vm.heap.is_typed_array(target).expect(
                    "the retained membrane record owns this live target and its immutable brand",
                ) {
                    return Ok(None);
                }
                vm.remaining_instructions = vm.config.instruction_budget;
                if vm
                    .heap
                    .typed_array_is_out_of_bounds(target)
                    .expect("the foreign TypedArray brand was checked without allocation")
                {
                    return Err(RuntimeError::TypeError(
                        "foreign TypedArray is detached or out of bounds".into(),
                    ));
                }
                let (_, _, length, kind) = vm
                    .heap
                    .typed_array_info(target)
                    .expect("the foreign TypedArray remains live during this pure snapshot");
                Ok(Some((length, kind)))
            })
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
            // Weak mirror metadata may outlive a collected local object.
            // Validate membership first; all following queries are pure.
            if self.heap.contains(buffer)
                && self
                    .heap
                    .is_buffer(buffer)
                    .expect("membership was checked without collection")
                && !self
                    .heap
                    .buffer_is_detached(buffer)
                    .expect("the live mirror was checked to be a buffer")
            {
                self.heap
                    .detach_array_buffer(buffer)
                    .expect("ordinary buffer mirrors support detachment without allocation");
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
            self.test262_realm_handle(realm_id)?
                .edit(self.object_prototype.heap, |vm| {
                    if vm.heap.is_buffer(target).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    ) && !vm.heap.buffer_is_detached(target).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    ) {
                        // Host detachment also reaches immutable buffers. Liveness
                        // and brand do not make their legitimate refusal infallible.
                        vm.heap.detach_array_buffer(target)?;
                    }
                    Ok(())
                })?;
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
        let detached = self
            .test262_realm_handle(realm_id)?
            .read(self.object_prototype.heap, |vm| {
                vm.heap.buffer_is_detached(target).map_err(Into::into)
            })?;
        if detached {
            return self.test262_detach_foreign_buffer_mirrors(realm_id, target);
        }
        let Some((byte_length, bytes)) =
            self.test262_realm_handle(realm_id)?
                .read(self.object_prototype.heap, |vm| {
                    if vm.heap.buffer_is_immutable(target).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    ) {
                        return Ok(None);
                    }
                    let byte_length = vm.heap.buffer_byte_length(target).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    );
                    let bytes = vm.heap.array_buffer_copy(target, 0, byte_length).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    );
                    Ok(Some((byte_length, bytes)))
                })?
        else {
            return Ok(());
        };
        let mirrors = self
            .test262_foreign_buffer_mirrors
            .iter()
            .filter_map(|((buffer, _), mirror)| {
                (mirror.realm == realm_id && mirror.target == target).then_some(*buffer)
            })
            .collect::<Vec<_>>();
        for buffer in mirrors {
            if !self
                .heap
                .buffer_is_detached(buffer)
                .expect("the retained membrane mirror owns this live buffer and checked byte range")
            {
                if self.heap.buffer_byte_length(buffer).expect(
                    "the retained membrane mirror owns this live buffer and checked byte range",
                ) != byte_length
                    && self.heap.buffer_resizable(buffer).expect(
                        "the retained membrane mirror owns this live buffer and checked byte range",
                    )
                {
                    self.with_roots(|heap| heap.resize_array_buffer(buffer, byte_length))?;
                }
                // Both mirror producers preserve resizability and the source
                // length. A fixed source cannot change length; a resizable
                // mirror either resized above or returned its refusal. No
                // JavaScript runs between that resize and this byte copy.
                self.heap
                    .array_buffer_write(buffer, 0, &bytes)
                    .expect("the mutable mirror has the checked matching byte length");
            }
        }
        Ok(())
    }

    #[cfg(any(test, coverage))]
    pub(in super::super) fn test262_foreign_call(
        &mut self,
        wrapper: ObjectId,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let entry = self.call_entry_charge(&Value::Object(wrapper), &receiver, &args);
        self.test262_foreign_call_owned(wrapper, receiver, args, construct, entry)
    }

    pub(in super::super) fn test262_foreign_call_owned(
        &mut self,
        wrapper: ObjectId,
        receiver: Value,
        mut args: Vec<Value>,
        construct: bool,
        entry: execution_payload::CallEntryCharge,
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
        let foreign_native = self.test262_realm_native(realm_id, target)?;
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
                // The host hook accepts a buffer from any live Realm. Its
                // function's Realm does not own the receiver's buffer slots.
                self.test262_realm_handle(buffer_realm)?.edit(
                    self.object_prototype.heap,
                    |vm| {
                        vm.heap
                            .detach_array_buffer(buffer_target)
                            .map_err(Into::into)
                    },
                )?;
                self.test262_detach_foreign_buffer_mirrors(buffer_realm, buffer_target)
                    .expect("detaching foreign buffer mirrors checks weak membership and cannot allocate");
                return Ok(Value::Undefined);
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
                self.with_roots(|heap| heap.detach_array_buffer(buffer))
                    .expect("the retained local mirror has the same buffer mutability as its successfully detached child");
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
            .map(|(_, receiver, _, _)| self.test262_realm_native(realm_id, receiver))
            .transpose()?
            .flatten();
        // A built-in that only reads and writes its operands through ordinary
        // property access and shared internal slots (an Array method, an
        // Iterator helper, the `stack` accessors) applied to an object that
        // is not the callee Realm's own must see that object, not the opaque
        // stand-in the membrane would transport: run its algorithm here, on
        // behalf of the callee's Realm. `method.call(receiver, ...)` reaches
        // the membrane as a foreign `Call`, so look through it.
        if !construct {
            // Materialize an Atomics apply argument list in its caller once.
            // Its eventual native call chooses the TypedArray's owning VM;
            // transporting the list first would snapshot that array's bytes.
            if foreign_native == Some(NativeFunction::Apply)
                && matches!(call_target_native, Some(NativeFunction::Atomics(_)))
            {
                return self.test262_run_native_for_realm(
                    realm_id,
                    NativeFunction::Apply,
                    receiver,
                    args,
                );
            }
            let method_call = if foreign_native == Some(NativeFunction::Call) {
                call_target_native.map(|method| {
                    (
                        method,
                        native::argument(&args, 0),
                        args.get(1..).unwrap_or_default(),
                    )
                })
            } else {
                foreign_native.map(|method| (method, &receiver, args.as_slice()))
            };
            if let Some((method, this_value, arguments)) = method_call {
                let foreign_to_callee = |vm: &Self, value: &Value| {
                    value.object_id().is_some_and(|object| {
                        vm.test262_foreign_reference(object)
                            .is_none_or(|(object_realm, _, _, _)| object_realm != realm_id)
                    })
                };
                let runs_here = match runs_where_its_operands_live(method) {
                    None => false,
                    Some(OperandPolicy::Receiver) => foreign_to_callee(self, this_value),
                    Some(OperandPolicy::FirstArgument) => arguments
                        .first()
                        .is_some_and(|value| foreign_to_callee(self, value)),
                    Some(OperandPolicy::AnyOperand) => {
                        foreign_to_callee(self, this_value)
                            || arguments.iter().any(|value| foreign_to_callee(self, value))
                    }
                };
                if runs_here {
                    let (this_value, arguments) = if foreign_native == Some(NativeFunction::Call) {
                        let this_value = if args.is_empty() {
                            Value::Undefined
                        } else {
                            args.remove(0)
                        };
                        (this_value, args)
                    } else {
                        (receiver, args)
                    };
                    return self
                        .test262_run_native_for_realm(realm_id, method, this_value, arguments);
                }
            }
        }
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
            let this_value = if args.is_empty() {
                Value::Undefined
            } else {
                args.remove(0)
            };
            return self.native_call(
                call_target_native.expect("matched TypedArray native function"),
                this_value,
                args,
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
            .map(|(_, receiver, _, _)| self.test262_realm_native(realm_id, receiver))
            .transpose()?
            .flatten();
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
            .map(|(_, receiver, _, _)| self.test262_realm_native(realm_id, receiver))
            .transpose()?
            .flatten()
            == Some(NativeFunction::Apply);
        if foreign_native == Some(NativeFunction::Call)
            && receiver_is_foreign_apply
            && !self.is_callable(args.first().unwrap_or(&Value::Undefined))?
        {
            let error = self
                .test262_realm_handle(realm_id)?
                .edit(self.object_prototype.heap, |vm| {
                    vm.error_value(RuntimeError::TypeError("apply requires a callable".into()))
                })?;
            return Err(RuntimeError::Thrown(
                self.test262_import_foreign_value(realm_id, error)?,
            ));
        }
        let child = self.test262_realm_handle(realm_id)?;
        let owner = child.read(self.object_prototype.heap, |child| {
            self.realm_call_native_owner(child)
        })?;
        let mut input = RealmCallInput {
            callee: Value::Object(wrapper),
            receiver,
            arguments: args,
            entry,
        };
        input.entry.retain_native(
            self,
            &input.callee,
            &input.receiver,
            &input.arguments,
            owner.as_ref(),
        )?;
        let roots = RealmCallRoots::new(input.objects().count(), owner.as_ref())?;
        input.objects().for_each(|id| roots.push(id));
        // Original source operands keep their conditional membrane edges live
        // while conversion creates destination objects. No active VM window moves.
        let result = self.with_realm_call_roots(roots, |source| {
            source.test262_foreign_call_input(
                realm_id,
                target,
                wrapper,
                foreign_native,
                input,
                construct,
            )
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    fn test262_foreign_call_input(
        &mut self,
        realm_id: ObjectId,
        target: ObjectId,
        wrapper: ObjectId,
        foreign_native: Option<NativeFunction>,
        mut input: RealmCallInput,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let registration = self.prepare_foreign_active_registration(realm_id)?;
        // Any foreign function can mutate or transfer a mirrored buffer,
        // including closures and Function.prototype.call/apply. Flush before
        // entry rather than relying on the outer function's native identity.
        self.test262_sync_foreign_buffer_mirrors(realm_id)?;
        input.receiver = self.test262_export_foreign_value_owned(
            realm_id,
            std::mem::replace(&mut input.receiver, Value::Undefined),
        )?;
        for slot in &mut input.arguments {
            let value = std::mem::replace(slot, Value::Undefined);
            *slot = self.test262_export_foreign_value_owned(realm_id, value)?;
        }
        let child = self.test262_realm_handle(realm_id)?;
        let caller = self.object_prototype.heap;
        let _guard = registration.publish(self);
        let result = child.edit(caller, |child| {
            child.remaining_instructions = child.config.instruction_budget;
            child.construct_completion_check_failed = false;
            let result = input.invoke(child, target, construct);
            match result {
                Ok(value) => Ok(value),
                // [[Construct]]'s completion checks (a derived constructor's
                // return value and `this` binding) run after the callee's
                // execution context is removed, so their error is created in
                // this Realm: leave it unmaterialized for the caller.
                Err(error @ (RuntimeError::TypeError(_) | RuntimeError::ReferenceError(_)))
                    if construct
                        && child.construct_completion_check_failed
                        && matches!(child.heap.closure(target), Ok(Some(_))) =>
                {
                    Err(error)
                }
                // RuntimeError represents spec throws until they cross a VM
                // boundary. Materialize every ordinary abrupt completion in the
                // child before import so a foreign closure, Proxy, or builtin
                // exposes the Error object from the Realm that created it.
                Err(error) => match child.error_value(error) {
                    Ok(error) => Err(RuntimeError::Thrown(error)),
                    Err(error) => Err(error),
                },
            }
        });
        drop(_guard);
        let buffers = self
            .test262_foreign_buffer_mirrors
            .values()
            .filter(|mirror| mirror.realm == realm_id)
            .map(|mirror| mirror.target)
            .collect::<HashSet<_>>();
        for buffer in buffers {
            self.test262_refresh_foreign_buffer_mirrors(realm_id, buffer)?;
        }
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
                // The returned facade is still a Rust temporary. A prototype
                // getter can collect the whole agent before the caller stores it.
                let root = self.heap.root(created)?;
                let prototype = (|| {
                    let default = self
                        .intrinsic_prototype(intrinsic)?
                        .object_id()
                        .expect("intrinsic prototype is an object");
                    self.constructor_prototype_for(default, Some(intrinsic))
                })();
                self.release_root(root);
                self.test262_set_foreign_prototype_override(created, prototype?);
            }
        }
        Ok(result)
    }

    /// Runs the built-in `function` of the foreign Realm `realm_id` in this
    /// Realm's `Vm`, where its receiver and arguments live. Objects it creates
    /// directly (`acting_realm`) and errors it raises belong to `realm_id`;
    /// values thrown by callbacks are left alone.
    fn test262_run_native_for_realm(
        &mut self,
        realm_id: ObjectId,
        function: NativeFunction,
        receiver: Value,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let previous = self.acting_realm.replace(realm_id);
        let result = self.native_call(function, receiver, args, false);
        self.acting_realm = previous;
        result.map_err(
            |error| match self.test262_create_error_in_realm(realm_id, error) {
                Ok(error) | Err(error) => error,
            },
        )
    }

    /// Re-fetches `property` (`"next"` or `"return"`) on the real receiver in
    /// its owning Test262 Realm and calls it there. Used both for the
    /// pre-existing iterator-protocol natives (`ArrayIteratorNext`/
    /// `IteratorNext`/`RegExpIteratorNext`, always `"next"`) and for
    /// `%WrapForValidIteratorPrototype%`'s `IteratorWrapperNext`/
    /// `IteratorWrapperReturn`, whose local implementation reads a captured
    /// internal-slot method rather than re-reading a property — but since
    /// that slot's method is always the owning Realm's own shared prototype
    /// method, re-fetching `property` by name on the real (child-Realm)
    /// receiver and calling it there reaches the identical function.
    pub(in super::super) fn test262_foreign_next(
        &mut self,
        receiver: &Value,
        property: &str,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let wrapper = receiver
            .object_id()
            .expect("native dispatch selects a live foreign object receiver");
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign next has a membrane record");
        let registration = self.prepare_foreign_active_registration(realm_id)?;
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let child = self.test262_realm_handle(realm_id)?;
        let caller = self.object_prototype.heap;
        let _guard = registration.publish(self);
        let result = child.edit(caller, |child| {
            child.remaining_instructions = child.config.instruction_budget;
            let receiver = Value::Object(target);
            let method = child.get_property(&receiver, &property.into())?;
            child.call_native(method, receiver, args, false)
        });
        drop(_guard);
        // This is the local `next` applied to a foreign receiver, so its own
        // errors belong to this Realm: they are imported but not recreated.
        self.test262_import_foreign_result(realm_id, result)
    }

    /// Test262's realm hook needs the callee's realm even when `eval` is
    /// detached from the foreign global. Each facade therefore owns a native
    /// function tagged with its realm identity rather than borrowing the
    /// caller's current global environment.
    pub(in super::super) fn test262_create_realm(&mut self) -> Result<Value, RuntimeError> {
        let mut realm = Vm::new(self.config)?;
        // Realms created by one Test262 host execute in the same agent. The
        // GlobalSymbolRegistry is agent-wide even though every Realm keeps
        // its own global object and intrinsics.
        realm.symbol_registry = Rc::clone(&self.symbol_registry);
        realm.inherit_document_policy(self)?;
        // A Test262 realm exposes the same host interface as its creator.
        // In particular, the record returned by createRealm must provide an
        // evalScript function that evaluates in this child Realm.
        realm.install_test262_harness()?;
        realm.synchronize_scheduling_usage()?;
        let realm = realm_cells::RealmHandle::new(realm)?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let mut published_realm = None;
        let result = (|| {
            let global = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(global));
            let foreign_global = *realm.borrow().globals.get("globalThis").expect(
                "install_test262_harness initialized and retained this fresh realm's global",
            );
            let target_root = realm.borrow_mut().heap.root(foreign_global)?;

            let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(record));
            self.define_data(record, "global", Value::Object(global), true, true, true)?;
            let mut wrappers = MembraneMap::default();
            let wrapper_slot = wrappers.prepare_insert(self.host_scheduling.as_ref())?;
            let realm_slot = self
                .test262_realms
                .prepare_insert(self.host_scheduling.as_ref())?;
            let value_slot = self
                .test262_foreign_values
                .prepare_insert(self.host_scheduling.as_ref())?;
            // Acquire the parent root immediately before publication. Earlier
            // allocations can fail while both objects are still stack roots.
            let wrapper_root = self.heap.root(global)?;
            wrappers.commit(wrapper_slot);
            wrappers.insert(foreign_global, global);
            self.test262_realms.commit(realm_slot);
            self.test262_foreign_values.commit(value_slot);
            self.test262_realms.insert(
                global,
                Test262Realm {
                    vm: realm,
                    wrappers,
                    imported_sources: MembraneMap::default(),
                    imported_values: MembraneMap::default(),
                    shadow_realm_reexports: MembraneMap::default(),
                },
            );
            published_realm = Some(global);
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
        if result.is_err() {
            if let Some(realm_id) = published_realm {
                let wrappers: Vec<_> = self
                    .test262_foreign_values
                    .iter()
                    .filter_map(|(wrapper, value)| (value.realm == realm_id).then_some(*wrapper))
                    .collect();
                for wrapper in wrappers {
                    let value = self
                        .test262_foreign_values
                        .remove(&wrapper)
                        .expect("the new realm owns this unpublished facade");
                    self.release_root(value._wrapper_root);
                }
                // Every child root belongs to this private heap. No user code
                // has received the failed realm or one of its facades.
                self.test262_realms.remove(&realm_id);
                self.test262_realms.release_empty();
                self.test262_foreign_values.release_empty();
            }
        }
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
        NativeFunction::String => "String",
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
        NativeFunction::DisposableStack { is_async: false } => "DisposableStack",
        NativeFunction::DisposableStack { is_async: true } => "AsyncDisposableStack",
        NativeFunction::ShadowRealm => "ShadowRealm",
        NativeFunction::Collator => "Intl.Collator",
        NativeFunction::Locale => "Intl.Locale",
        NativeFunction::IntlService(service) => match service {
            native::IntlService::Number => "Intl.NumberFormat",
            native::IntlService::DateTime => "Intl.DateTimeFormat",
            native::IntlService::DisplayNames => "Intl.DisplayNames",
            native::IntlService::Duration => "Intl.DurationFormat",
            native::IntlService::List => "Intl.ListFormat",
            native::IntlService::Plural => "Intl.PluralRules",
            native::IntlService::RelativeTime => "Intl.RelativeTimeFormat",
            native::IntlService::Segmenter => "Intl.Segmenter",
        },
        NativeFunction::TemporalConstructor(kind) => match kind {
            crate::heap::TemporalKind::Duration => "Temporal.Duration",
            crate::heap::TemporalKind::Instant => "Temporal.Instant",
            crate::heap::TemporalKind::PlainDate => "Temporal.PlainDate",
            crate::heap::TemporalKind::PlainDateTime => "Temporal.PlainDateTime",
            crate::heap::TemporalKind::PlainMonthDay => "Temporal.PlainMonthDay",
            crate::heap::TemporalKind::PlainTime => "Temporal.PlainTime",
            crate::heap::TemporalKind::PlainYearMonth => "Temporal.PlainYearMonth",
            crate::heap::TemporalKind::ZonedDateTime => "Temporal.ZonedDateTime",
        },
        _ => return None,
    })
}

/// Which operands of a built-in of another Realm decide that it must run in
/// the calling Realm's `Vm` (see `runs_where_its_operands_live`).
enum OperandPolicy {
    /// The function checks its receiver's internal slots or accessors, so it
    /// runs here only when the receiver is not the callee Realm's own.
    Receiver,
    /// Atomics arithmetic accesses the first argument's backing storage and
    /// returns a primitive; the receiver does not select that storage.
    FirstArgument,
    /// The function is generic over everything it is given (and may call
    /// callbacks), so it runs here when the receiver or any argument object
    /// is not the callee Realm's own.
    AnyOperand,
}

/// Built-in functions whose algorithm only reads and writes its operands
/// through ordinary property access and internal slots that every Realm's
/// objects share. Applied to another Realm's objects they must run on those
/// objects, not on a transported copy. Objects and errors they create belong
/// to the callee's Realm.
fn runs_where_its_operands_live(function: NativeFunction) -> Option<OperandPolicy> {
    Some(match function {
        NativeFunction::Atomics(_) => OperandPolicy::FirstArgument,
        NativeFunction::ArrayAt
        | NativeFunction::ArrayFill
        | NativeFunction::ArrayCopyWithin
        | NativeFunction::ArrayToReversed
        | NativeFunction::ArrayToSorted
        | NativeFunction::ArrayToSpliced
        | NativeFunction::ArrayWith
        | NativeFunction::ArrayFlat
        | NativeFunction::ArrayFlatMap
        | NativeFunction::ArrayOf
        | NativeFunction::ArrayFrom
        | NativeFunction::ArrayForEach
        | NativeFunction::ArrayFilter
        | NativeFunction::ArrayMap
        | NativeFunction::ArrayFind
        | NativeFunction::ArrayFindIndex
        | NativeFunction::ArrayFindLast
        | NativeFunction::ArrayFindLastIndex
        | NativeFunction::ArrayEvery
        | NativeFunction::ArraySome
        | NativeFunction::ArrayIncludes
        | NativeFunction::ArrayReduce
        | NativeFunction::ArrayReduceRight
        | NativeFunction::ArrayPush
        | NativeFunction::ArrayPop
        | NativeFunction::ArrayShift
        | NativeFunction::ArrayUnshift
        | NativeFunction::ArrayReverse
        | NativeFunction::ArrayIndexOf
        | NativeFunction::ArrayLastIndexOf
        | NativeFunction::ArraySlice
        | NativeFunction::ArraySplice
        | NativeFunction::ArraySort
        | NativeFunction::ArrayToLocaleString
        | NativeFunction::IteratorHelper(_)
        | NativeFunction::IteratorToArray
        | NativeFunction::IteratorForEach
        | NativeFunction::IteratorEvery
        | NativeFunction::IteratorSome
        | NativeFunction::IteratorFind
        | NativeFunction::IteratorReduce => OperandPolicy::AnyOperand,
        NativeFunction::IteratorHelperNext
        | NativeFunction::IteratorHelperReturn
        | NativeFunction::IteratorWrapperNext
        | NativeFunction::IteratorWrapperReturn
        | NativeFunction::PromiseThen
        | NativeFunction::ErrorStackGetter
        | NativeFunction::ErrorStackSetter => OperandPolicy::Receiver,
        _ => return None,
    })
}

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/foreign_realm_boundaries.rs"]
mod boundary_contracts;
