// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(super) fn get_property(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        let result = self.get_property_value(receiver, key);
        self.stack.truncate(base);
        result
    }

    pub(super) fn get_property_value(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        match receiver {
            Value::Object(id) => self.get_object_property(*id, receiver, key),
            Value::String(string) => {
                if let PropertyName::String(key) = key {
                    if let Some(value) = string.own_property(key) {
                        return Ok(value);
                    }
                }
                let (_, prototype) = self.string_intrinsics()?;
                self.get_from_prototype(prototype, receiver, key)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot access a property of null or undefined".into(),
            )),
            _ => {
                let constructor = self.global(match receiver {
                    Value::Symbol(_) => "Symbol",
                    Value::Bool(_) => "Boolean",
                    _ => "Number",
                })?;
                let prototype = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                self.get_from_prototype(prototype, receiver, key)
            }
        }
    }

    /// [[Get]] with the lookup target separated from the receiver supplied to
    /// accessors and Proxy traps.  Ordinary property syntax supplies the same
    /// object for both arguments; Reflect.get and inherited Proxy operations
    /// intentionally do not.
    pub(super) fn get_object_property(
        &mut self,
        target: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        if self.test262_foreign_reference(target).is_some() {
            return self.test262_foreign_get(target, receiver, key);
        }
        self.materialize_global_object_property(target, key)?;
        if let Some(cell) = self.global_property_cell(target, key) {
            return self
                .heap
                .get_own(cell, "value")?
                .ok_or_else(|| RuntimeError::ReferenceError("global binding".into()));
        }
        if self.string_intrinsics.is_none()
            && (key == "toString"
                || key == "valueOf"
                || key == "join"
                || key == "forEach"
                || key == "includes"
                || *key == PropertyName::from(JsSymbol::well_known("iterator")))
        {
            self.string_intrinsics()?;
        }
        if key == "propertyIsEnumerable" {
            self.property_is_enumerable_intrinsic()?;
        }
        if key == "hasOwnProperty" {
            self.has_own_property_intrinsic()?;
        }
        // `%Object.prototype%` has an initial own constructor property.
        // Intrinsics otherwise bootstrap lazily, so make it observable before
        // an ordinary object performs an inherited lookup.
        if key == "constructor"
            && self
                .heap
                .get_own_property_descriptor(self.object_prototype, "constructor")?
                .is_none()
        {
            self.global("Object")?;
        }
        if self.heap.proxy(target)?.is_some() {
            return self.proxy_get(target, receiver, key);
        }
        self.get_from_prototype(target, receiver, key)
    }

    pub(super) fn set_property(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = self.set_property_value(receiver, key, value);
        self.stack.truncate(base);
        result
    }

    pub(super) fn set_property_value(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        if let Value::Object(object) = receiver {
            if self.test262_foreign_reference(*object).is_some() {
                return self.test262_foreign_set(*object, key, value);
            }
            self.materialize_global_object_property(*object, key)?;
            if let Some(cell) = self.global_property_cell(*object, key) {
                let result = self.with_roots(|heap| heap.set(*object, key.clone(), value.clone()));
                return match result {
                    Err(RuntimeError::Heap(HeapError::ReadOnlyProperty)) if self.strict => Err(
                        RuntimeError::TypeError("property cannot be assigned".into()),
                    ),
                    Err(RuntimeError::Heap(HeapError::ReadOnlyProperty)) => Ok(()),
                    Ok(()) => self.with_roots(|heap| heap.set(cell, "value", value.clone())),
                    Err(error) => Err(error),
                };
            }
        }
        // ToObject provides the lookup target; accessors retain the original
        // receiver. `ordinary_set_with_receiver` also routes a Proxy found
        // anywhere in the prototype chain through its [[Set]] trap.
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let succeeded = self.ordinary_set_with_receiver(object, receiver, key, value)?;
        if succeeded {
            Ok(())
        } else if self.strict {
            Err(RuntimeError::TypeError(
                "property cannot be assigned".into(),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn property_reference(&mut self) -> Result<(Value, PropertyName), RuntimeError> {
        let key = self.pop();
        let key = self.coerce_property_key(&key)?;
        match self.pop() {
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot access a property of null or undefined".into(),
            )),
            receiver => Ok((receiver, key)),
        }
    }

    /// Extracts the two stack values used to retain a private Reference and
    /// resolves its owner through the compiler-generated lexical private-name
    /// binding.  Unlike an ordinary property reference, its name is not a
    /// PropertyKey and its receiver is never boxed.
    pub(super) fn private_reference(
        &mut self,
        owner_slot: usize,
    ) -> Result<(Value, ObjectId, JsString), RuntimeError> {
        let name = match self.pop() {
            Value::String(name) => name,
            _ => unreachable!("compiler emits a string private name"),
        };
        let receiver = self.pop();
        let owner = self
            .binding_value(owner_slot)?
            .and_then(|value| value.object_id())
            .ok_or_else(|| {
                RuntimeError::TypeError(
                    "private elements are not available in this function".into(),
                )
            })?;
        Ok((receiver, owner, name))
    }

    pub(super) fn private_receiver(
        &self,
        receiver: &Value,
        owner: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("private fields require an object receiver".into())
        })?;
        if !self.heap.has_private_brand(object, owner)? {
            return Err(RuntimeError::TypeError(
                "receiver does not have the requested private element".into(),
            ));
        }
        Ok(object)
    }

    pub(super) fn private_get(
        &mut self,
        receiver: &Value,
        owner: ObjectId,
        name: &JsString,
    ) -> Result<Value, RuntimeError> {
        let object = self.private_receiver(receiver, owner)?;
        let element = self.heap.private_element(owner, name)?.ok_or_else(|| {
            RuntimeError::TypeError("private element is not declared by this class".into())
        })?;
        match element {
            PrivateElement::Field => {
                self.heap.private_slot(object, owner, name)?.ok_or_else(|| {
                    RuntimeError::TypeError("private field has not been initialized".into())
                })
            }
            PrivateElement::Method(function) => Ok(function),
            PrivateElement::Accessor { get: None, .. } => Ok(Value::Undefined),
            PrivateElement::Accessor {
                get: Some(getter), ..
            } => self.call_native(getter, receiver.clone(), Vec::new(), false),
        }
    }

    pub(super) fn private_set(
        &mut self,
        receiver: &Value,
        owner: ObjectId,
        name: JsString,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let object = self.private_receiver(receiver, owner)?;
        let element = self.heap.private_element(owner, &name)?.ok_or_else(|| {
            RuntimeError::TypeError("private element is not declared by this class".into())
        })?;
        match element {
            PrivateElement::Field => {
                self.with_roots(|heap| heap.set_private_slot(object, owner, name, value))
            }
            PrivateElement::Method(_) => Err(RuntimeError::TypeError(
                "cannot assign to a private method".into(),
            )),
            PrivateElement::Accessor { set: None, .. } => Err(RuntimeError::TypeError(
                "private accessor has no setter".into(),
            )),
            PrivateElement::Accessor {
                set: Some(setter), ..
            } => {
                self.call_native(setter, receiver.clone(), vec![value], false)?;
                Ok(())
            }
        }
    }

    pub(super) fn set_class_heritage(&mut self) -> Result<(), RuntimeError> {
        let base = self.pop();
        let class = self
            .stack
            .last()
            .expect("class closure remains on the stack")
            .object_id()
            .expect("compiler emits a class closure before heritage");
        let prototype = self
            .heap
            .get(class, "prototype")?
            .object_id()
            .expect("class constructors have a prototype object");
        let (constructor_parent, instance_parent) = match &base {
            Value::Null => (None, None),
            Value::Object(base) if self.is_constructor(&Value::Object(*base))? => {
                let instance_parent =
                    match self.get_property(&Value::Object(*base), &"prototype".into())? {
                        Value::Object(prototype) => Some(prototype),
                        Value::Null => None,
                        _ => {
                            return Err(RuntimeError::TypeError(
                                "superclass prototype must be an object or null".into(),
                            ))
                        }
                    };
                (Some(*base), instance_parent)
            }
            _ => {
                return Err(RuntimeError::TypeError(
                    "class extends value is not a constructor or null".into(),
                ))
            }
        };
        self.heap.set_prototype(class, constructor_parent)?;
        self.heap.set_prototype(prototype, instance_parent)?;
        self.with_roots(|heap| heap.set_class_base(class, base))?;
        self.with_roots(|heap| heap.set_closure_home(class, prototype))?;
        Ok(())
    }

    pub(super) fn set_class_home(&mut self) -> Result<(), RuntimeError> {
        let class = self
            .stack
            .last()
            .expect("class closure remains on the stack")
            .object_id()
            .expect("compiler emits a class closure before setting its home object");
        let prototype = self
            .heap
            .get(class, "prototype")?
            .object_id()
            .expect("class constructors have a prototype object");
        self.with_roots(|heap| heap.set_closure_home(class, prototype))?;
        Ok(())
    }

    pub(super) fn super_base(&mut self) -> Result<ObjectId, RuntimeError> {
        let home = self.home_object.ok_or_else(|| {
            RuntimeError::TypeError("super is not available in this function".into())
        })?;
        self.object_get_prototype(home)?
            .ok_or_else(|| RuntimeError::TypeError("superclass is null".into()))
    }

    pub(super) fn super_get(&mut self, key: &PropertyName) -> Result<Value, RuntimeError> {
        let base = self.super_base()?;
        self.get_from_prototype(base, &self.this.clone(), key)
    }

    pub(super) fn super_set(
        &mut self,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let base = self.super_base()?;
        let this = self.this.clone();
        if !matches!(this, Value::Object(_)) {
            return Err(RuntimeError::ReferenceError(
                "this is uninitialized before super()".into(),
            ));
        }
        if self.ordinary_set_with_receiver(base, &this, key, value)? {
            Ok(())
        } else {
            self.super_assignment_failed("super property cannot be assigned")
        }
    }

    pub(super) fn super_assignment_failed(&self, message: &str) -> Result<(), RuntimeError> {
        if self.strict {
            Err(RuntimeError::TypeError(message.into()))
        } else {
            Ok(())
        }
    }

    pub(super) fn super_call(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let constructor = self.class_constructor.ok_or_else(|| {
            RuntimeError::TypeError("super() is not available in this function".into())
        })?;
        let base = self.heap.class_base(constructor)?.ok_or_else(|| {
            RuntimeError::TypeError("super() requires a derived constructor".into())
        })?;
        if matches!(base, Value::Null) {
            return Err(RuntimeError::TypeError("super constructor is null".into()));
        }
        let value =
            self.call_with_target(base, Value::Undefined, args, true, self.new_target.clone())?;
        self.this = value.clone();
        Ok(value)
    }
    /// Implements CopyDataProperties for an object-rest binding.  The
    /// compiler supplies an internal array of already-coerced excluded keys;
    /// getters are read from the original source object and copied as normal
    /// enumerable data properties onto a fresh ordinary object.
    pub(super) fn destructure_object_rest(
        &mut self,
        source: &Value,
        excluded: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let length_value = self.get_property(excluded, &"length".into())?;
            let length = self.coerce_length(&length_value)? as u64;
            let mut excluded_keys = Vec::new();
            for index in 0..length {
                self.charge_step()?;
                let key = self.get_property(excluded, &index.to_string().into())?;
                excluded_keys.push(self.coerce_property_key(&key)?);
            }
            let prototype = self.object_prototype;
            let target = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(target));
            self.copy_data_properties(target, source, &excluded_keys)?;
            self.stack.pop();
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// The enumerable-property portion of CopyDataProperties.  Object spread
    /// skips nullish sources, while object-rest has already performed
    /// RequireObjectCoercible before arriving here.
    pub(super) fn copy_data_properties(
        &mut self,
        target: ObjectId,
        source: &Value,
        excluded: &[PropertyName],
    ) -> Result<(), RuntimeError> {
        if matches!(source, Value::Null | Value::Undefined) {
            return Ok(());
        }
        let source_object = self.coerce_object(source)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(source_object));
        let result = (|| {
            for key in self.object_own_property_keys(source_object)? {
                self.charge_step()?;
                if excluded.iter().any(|excluded| excluded == &key) {
                    continue;
                }
                // A Proxy's ownKeys trap is allowed to report a key for which
                // its getOwnPropertyDescriptor trap returns undefined.  This
                // is therefore a conditional copy, rather than an assertion
                // that every reported key still has a descriptor.
                let Some(descriptor) = self.object_get_own_property(source_object, &key)? else {
                    continue;
                };
                if descriptor.enumerable != Some(true) {
                    continue;
                }
                let value = self.get_property(&Value::Object(source_object), &key)?;
                if !self.object_define_own_property(
                    target,
                    key,
                    PropertyDescriptor::data(value, true, true, true),
                )? {
                    return Err(RuntimeError::TypeError(
                        "cannot define copied property".into(),
                    ));
                }
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    /// Snapshots the enumerable string keys visible through an object's
    /// prototype chain. Non-enumerable own keys still suppress an inherited
    /// key with the same name; symbols never participate in `for-in`.
    pub(super) fn for_in_keys(&mut self, source: &Value) -> Result<Value, RuntimeError> {
        let mut current = Some(self.coerce_object(source)?);
        let base = self.stack.len();
        let mut seen = HashSet::new();
        let mut visited_objects = HashSet::new();
        let mut keys = Vec::new();
        let result = (|| {
            while let Some(object) = current {
                // Keep each traversed object live while Proxy traps execute.
                // A proxy is allowed to return a prototype that is not
                // otherwise reachable from its target or handler.
                self.stack.push(Value::Object(object));
                if !visited_objects.insert(object) {
                    break;
                }
                for key in self.object_own_property_keys(object)? {
                    if !seen.insert(key.clone()) {
                        continue;
                    }
                    if let PropertyName::String(key) = key {
                        if self
                            .object_get_own_property(object, &PropertyName::String(key.clone()))?
                            .is_some_and(|descriptor| descriptor.enumerable == Some(true))
                        {
                            keys.push(Value::String(key));
                        }
                    }
                }
                current = self.object_get_prototype(object)?;
            }
            self.array_from(keys)
        })();
        self.stack.truncate(base);
        result
    }
}
