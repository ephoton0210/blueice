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
                // Primitive-string lookup reaches `%Object.prototype%` through
                // `%String.prototype%`, too.  These two Object methods are
                // installed lazily, so materialize them before walking that
                // inherited path just as `get_object_property` does for an
                // ordinary object receiver.  In particular, a primitive
                // receiver supplied to Reflect.set must still support the
                // standard `receiver.hasOwnProperty(key)` observation after
                // [[Set]] correctly returns false.
                if key == "propertyIsEnumerable" {
                    self.property_is_enumerable_intrinsic()?;
                }
                if key == "hasOwnProperty" {
                    self.has_own_property_intrinsic()?;
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
                    Value::BigInt(_) => "BigInt",
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

    /// Built-in prototype methods are installed lazily with the string
    /// intrinsics. Any lookup that names one of them (`[[Get]]` and
    /// `[[HasProperty]]` alike) must materialize that set first, or an
    /// inherited built-in reads as absent (`'push' in []`, a `with` lookup).
    pub(super) fn materialize_string_intrinsics_for_key(
        &mut self,
        key: &PropertyName,
    ) -> Result<(), RuntimeError> {
        if self.string_intrinsics.is_none()
            && (key == "toString"
                || key == "toLocaleString"
                || key == "valueOf"
                || key == "at"
                || key == "fill"
                || key == "copyWithin"
                || key == "flat"
                || key == "flatMap"
                || key == "toReversed"
                || key == "toSorted"
                || key == "toSpliced"
                || key == "with"
                || key == "entries"
                || key == "keys"
                || key == "values"
                || key == "join"
                || key == "forEach"
                || key == "filter"
                || key == "map"
                || key == "find"
                || key == "findIndex"
                || key == "findLast"
                || key == "findLastIndex"
                || key == "every"
                || key == "some"
                || key == "includes"
                || key == "reduce"
                || key == "reduceRight"
                || key == "push"
                || key == "pop"
                || key == "shift"
                || key == "unshift"
                || key == "reverse"
                || key == "indexOf"
                || key == "lastIndexOf"
                || key == "slice"
                || key == "splice"
                || key == "sort"
                || key == "__defineGetter__"
                || key == "__defineSetter__"
                || key == "__lookupGetter__"
                || key == "__lookupSetter__"
                || key == "__proto__"
                || *key == PropertyName::from(JsSymbol::well_known("iterator"))
                || *key == PropertyName::from(JsSymbol::well_known("unscopables")))
        {
            self.string_intrinsics()?;
        }
        Ok(())
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
        if key == "constructor" && self.heap.is_array(target)? {
            // Array instances inherit this property from `%Array.prototype%`.
            // The intrinsic constructor is otherwise lazy, but the inherited
            // lookup may occur before a global `Array` reference in the same
            // expression. Materialize it first so `[].constructor` is not
            // observably absent.
            self.global("Array")?;
        }
        if key == "constructor" && self.is_callable(&Value::Object(target))? {
            // `%Function.prototype%` owns its `constructor` property.
            // Materialize `%Function%` before an inherited lookup on a
            // closure (including one passed through Object(value)) can
            // observe the temporary lazy-intrinsic gap.
            self.global("Function")?;
        }
        // Imported live Proxies have both a membrane record and a local
        // Proxy exotic record.  The latter owns the current execution
        // context, so it must dispatch before ordinary foreign forwarding.
        if self.heap.proxy(target)?.is_some() {
            return self.proxy_get(target, receiver, key);
        }
        if self.test262_foreign_reference(target).is_some() {
            return self.test262_foreign_get(target, receiver, key);
        }
        if self.test262_reverse_reference(target).is_some() {
            return self.test262_reverse_get(target, receiver, key);
        }
        self.materialize_global_object_property(target, key)?;
        if let Some(cell) = self.global_property_cell(target, key) {
            return self
                .heap
                .get_own(cell, "value")?
                .ok_or_else(|| RuntimeError::ReferenceError("global binding".into()));
        }
        self.materialize_string_intrinsics_for_key(key)?;
        if key == "propertyIsEnumerable" {
            self.property_is_enumerable_intrinsic()?;
        }
        if key == "hasOwnProperty" {
            self.has_own_property_intrinsic()?;
        }
        // `%Function.prototype%` is allocated while the first string
        // intrinsic bootstraps, whereas its restricted own properties are
        // installed with the `%Function%` global. Materialize that global
        // before an inherited access can observe the temporary gap.
        if key == "caller" || key == "arguments" {
            self.global("Function")?;
        }
        if key == "__proto__" && self.string_intrinsics.is_none() {
            self.string_intrinsics()?;
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
        // `%Function.prototype%` owns the restricted `caller` and
        // `arguments` accessors.  Globals bootstrap lazily, so an assignment
        // through a bound function must materialize that prototype before the
        // ordinary [[Set]] prototype walk observes it.
        if key == "caller" || key == "arguments" {
            self.global("Function")?;
        }
        if let Value::Object(object) = receiver {
            if self.heap.proxy(*object)?.is_none()
                && self.test262_foreign_reference(*object).is_some()
            {
                return self.test262_foreign_set(*object, key, value);
            }
            if self.heap.proxy(*object)?.is_none()
                && self.test262_reverse_reference(*object).is_some()
            {
                return self.test262_reverse_set(*object, key, value);
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
            PrivateElement::Accessor { get: None, .. } => Err(RuntimeError::TypeError(
                "private accessor has no getter".into(),
            )),
            PrivateElement::Accessor {
                get: Some(getter), ..
            } => self.call_native(getter, receiver.clone(), Vec::new(), false),
        }
    }

    /// SetFunctionName(F, key, prefix) for a function value created by a
    /// property definition whose key is only known at run time. `prefix` is
    /// 0 (none), 1 (`get `) or 2 (`set `). A function that already carries a
    /// non-empty `name` of its own (a class with a static `name` member) is
    /// left alone.
    pub(super) fn set_function_name_from_key(
        &mut self,
        function: &Value,
        key: &Value,
        prefix: u32,
    ) -> Result<(), RuntimeError> {
        let Value::Object(function) = function else {
            return Ok(());
        };
        let prefix_text: JsString = match prefix {
            1 => "get ".into(),
            2 => "set ".into(),
            _ => JsString::default(),
        };
        // The parser leaves an anonymous function's name empty, or (for a
        // computed object-literal accessor) just its prefix.
        match self.heap.get_own(*function, "name")? {
            None => {}
            Some(Value::String(existing))
                if existing.byte_len() == 0 || existing == prefix_text => {}
            Some(_) => return Ok(()),
        }
        let mut name = prefix_text;
        match key {
            Value::Symbol(symbol) => {
                if let Some(description) = &symbol.description {
                    name.push_str(&"[".into());
                    name.push_str(description);
                    name.push_str(&"]".into());
                }
            }
            Value::String(text) => name.push_str(text),
            other => name.push_str(&crate::primitive::string(other)?),
        }
        self.define_data(*function, "name", Value::String(name), false, false, true)
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
                // Assignment updates an initialized field; it never adds one.
                if self.heap.private_slot(object, owner, &name)?.is_none() {
                    return Err(RuntimeError::TypeError(
                        "private field has not been initialized".into(),
                    ));
                }
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
            // `extends null`: the constructor still inherits from
            // %Function.prototype%; only the instance prototype chain ends.
            Value::Null => (Some(self.function_prototype()?), None),
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
        self.with_roots(|heap| heap.set_closure_home(class, prototype))?;
        Ok(())
    }

    /// PrivateFieldAdd: define a private field on `receiver`. It is a
    /// TypeError to add the same field twice, or to add one to an object that
    /// is no longer extensible.
    pub(super) fn private_field_add(
        &mut self,
        receiver: &Value,
        owner: ObjectId,
        name: JsString,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("private fields require an object receiver".into())
        })?;
        if self.heap.private_slot(object, owner, &name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "private field is already defined on this object".into(),
            ));
        }
        if !self.object_is_extensible(object)? {
            return Err(RuntimeError::TypeError(
                "cannot add a private element to a non-extensible object".into(),
            ));
        }
        self.with_roots(|heap| {
            heap.add_private_brand(object, owner)?;
            heap.set_private_slot(object, owner, name, value)
        })
    }

    /// Installs the initializer function on the class constructor beneath it
    /// on the stack (`F, initializer` -> `F`). Its home object is the class
    /// prototype, so `super.x` works in a field initializer.
    pub(super) fn set_class_fields(&mut self) -> Result<(), RuntimeError> {
        let initializer = self
            .stack
            .last()
            .and_then(Value::object_id)
            .expect("compiler emits the class field initializer closure");
        let class = self.stack[self.stack.len() - 2]
            .object_id()
            .expect("compiler emits a class closure before its field initializer");
        let prototype = self
            .heap
            .get(class, "prototype")?
            .object_id()
            .expect("class constructors have a prototype object");
        self.with_roots(|heap| heap.set_closure_home(initializer, prototype))?;
        self.with_roots(|heap| heap.set_class_fields(class, initializer))?;
        self.stack.pop();
        Ok(())
    }

    /// InitializeInstanceElements: run the class's field initializer (if it
    /// has one) with the just-constructed object as `this`.
    pub(super) fn initialize_instance_elements(
        &mut self,
        constructor: &Value,
        instance: &Value,
    ) -> Result<(), RuntimeError> {
        let Some(class) = constructor.object_id() else {
            return Ok(());
        };
        if let Some(initializer) = self.heap.class_fields(class)? {
            self.call_native(
                Value::Object(initializer),
                instance.clone(),
                Vec::new(),
                false,
            )?;
        }
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

    /// GetSuperBase: the home object's [[Prototype]], an object or `null`.
    pub(super) fn super_base(&mut self) -> Result<Value, RuntimeError> {
        let home = self.home_object.ok_or_else(|| {
            RuntimeError::TypeError("super is not available in this function".into())
        })?;
        Ok(self
            .object_get_prototype(home)?
            .map_or(Value::Null, Value::Object))
    }

    /// The ToObject step of GetValue/PutValue on a super Reference: a `null`
    /// base (a class or object whose prototype chain ends) is a TypeError,
    /// raised only once the property is actually used.
    fn super_base_object(base: &Value) -> Result<ObjectId, RuntimeError> {
        base.object_id().ok_or_else(|| {
            RuntimeError::TypeError("cannot access a property through a null super base".into())
        })
    }

    /// GetValue of a super Reference: `base.[[Get]](key, this)`, converting the
    /// key only after the base is known to be an object.
    pub(super) fn super_get(
        &mut self,
        base: &Value,
        key: &Value,
        this: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = Self::super_base_object(base)?;
        let key = self.coerce_property_key(key)?;
        self.get_from_prototype(base, this, &key)
    }

    /// PutValue of a super Reference: `base.[[Set]](key, value, this)`; a
    /// failed [[Set]] is a TypeError in strict code and ignored otherwise.
    pub(super) fn super_set(
        &mut self,
        base: &Value,
        key: &Value,
        value: &Value,
        this: &Value,
    ) -> Result<(), RuntimeError> {
        let base = Self::super_base_object(base)?;
        let key = self.coerce_property_key(key)?;
        if self.ordinary_set_with_receiver(base, this, &key, value)? {
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

    /// The Construct step of `super(...)`: IsConstructor is checked only now,
    /// after the arguments have been evaluated, and the active function's
    /// `new.target` is passed through.
    pub(super) fn super_call(
        &mut self,
        constructor: Value,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        if !self.is_constructor(&constructor)? {
            return Err(RuntimeError::TypeError(
                "super constructor is not a constructor".into(),
            ));
        }
        let new_target = self.new_target.clone();
        self.call_with_target(constructor, Value::Undefined, args, true, new_target)
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
    /// The iterator record of a `for-in` loop (EnumerateObjectProperties,
    /// §14.7.5.9). It is an engine-internal record, not an ECMAScript
    /// iterator: `for_in_step` walks the object and its prototype chain lazily,
    /// so a key deleted (or made non-enumerable) before it is visited is not
    /// produced, and the loop never touches user-visible iterator methods.
    pub(super) fn for_in_iterator(&mut self, source: &Value) -> Result<Value, RuntimeError> {
        // ForIn/OfHeadEvaluation: a `null` or `undefined` subject enumerates
        // nothing instead of failing ToObject.
        let object = if matches!(source, Value::Null | Value::Undefined) {
            Value::Null
        } else {
            Value::Object(self.coerce_object(source)?)
        };
        let base = self.stack.len();
        // The coerced object of a primitive subject is new: keep it reachable
        // while the record's own objects are allocated.
        self.stack.push(object.clone());
        let result = (|| {
            let record = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(record));
            let visited = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(visited));
            let chain = self.array_from(Vec::new())?;
            self.stack.push(chain.clone());
            self.with_roots(|heap| heap.set(record, "forInObject", object))?;
            self.with_roots(|heap| heap.set(record, "forInKeys", Value::Undefined))?;
            self.with_roots(|heap| heap.set(record, "forInIndex", Value::Number(0.0)))?;
            self.with_roots(|heap| heap.set(record, "forInVisited", Value::Object(visited)))?;
            self.with_roots(|heap| heap.set(record, "forInChain", chain))?;
            self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
            Ok(Value::Object(record))
        })();
        self.stack.truncate(base);
        result
    }

    /// Whether `record` is a `for-in` record made by [`Vm::for_in_iterator`].
    pub(super) fn is_for_in_record(&self, record: ObjectId) -> Result<bool, RuntimeError> {
        Ok(self.heap.get_own(record, "forInObject")?.is_some())
    }

    /// The next key of a `for-in` loop, or `None` once every object of the
    /// prototype chain is exhausted. Each own key is checked with
    /// [[GetOwnProperty]] when it is about to be produced: a key that is gone
    /// or no longer enumerable is skipped, and one seen on an object nearer
    /// the receiver (enumerable or not) shadows the same key further up.
    pub(super) fn for_in_step(&mut self, record: ObjectId) -> Result<Option<Value>, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(record));
        let result = self.for_in_next_key(record);
        self.stack.truncate(base);
        if !matches!(result, Ok(Some(_))) {
            self.with_roots(|heap| heap.set(record, "done", Value::Bool(true)))?;
        }
        result
    }

    fn for_in_next_key(&mut self, record: ObjectId) -> Result<Option<Value>, RuntimeError> {
        let visited = self.heap.get_own(record, "forInVisited")?;
        let Some(Value::Object(visited)) = visited else {
            return Ok(None);
        };
        loop {
            let Some(Value::Object(object)) = self.heap.get_own(record, "forInObject")? else {
                return Ok(None);
            };
            self.stack.push(Value::Object(object));
            let keys = match self.heap.get_own(record, "forInKeys")? {
                Some(Value::Object(keys)) => keys,
                _ => {
                    // Entering this object: guard against a cyclic chain
                    // (a Proxy can return one), then list its own keys.
                    let Some(Value::Object(chain)) = self.heap.get_own(record, "forInChain")?
                    else {
                        return Ok(None);
                    };
                    if self.array_contains_object(chain, object)? {
                        self.with_roots(|heap| heap.set(record, "forInObject", Value::Null))?;
                        continue;
                    }
                    let length = match self.heap.get_own(chain, "length")? {
                        Some(Value::Number(length)) => length as usize,
                        _ => 0,
                    };
                    self.with_roots(|heap| {
                        heap.set(chain, length.to_string(), Value::Object(object))
                    })?;
                    let mut names = Vec::new();
                    for key in self.object_own_property_keys(object)? {
                        if let PropertyName::String(key) = key {
                            names.push(Value::String(key));
                        }
                    }
                    let keys = self.array_from(names)?;
                    let Value::Object(keys) = keys else {
                        unreachable!("array_from returns an array object")
                    };
                    self.stack.push(Value::Object(keys));
                    self.with_roots(|heap| heap.set(record, "forInKeys", Value::Object(keys)))?;
                    self.with_roots(|heap| heap.set(record, "forInIndex", Value::Number(0.0)))?;
                    keys
                }
            };
            let length = match self.heap.get_own(keys, "length")? {
                Some(Value::Number(length)) => length as usize,
                _ => 0,
            };
            let mut index = match self.heap.get_own(record, "forInIndex")? {
                Some(Value::Number(index)) => index as usize,
                _ => 0,
            };
            while index < length {
                let key = self.heap.get_own(keys, index.to_string())?;
                index += 1;
                self.with_roots(|heap| {
                    heap.set(record, "forInIndex", Value::Number(index as f64))
                })?;
                let Some(Value::String(key)) = key else {
                    continue;
                };
                let name = PropertyName::String(key.clone());
                if self.heap.get_own(visited, name.clone())?.is_some() {
                    continue;
                }
                let Some(descriptor) = self.object_get_own_property(object, &name)? else {
                    continue;
                };
                self.with_roots(|heap| heap.set(visited, name, Value::Bool(true)))?;
                if descriptor.enumerable == Some(true) {
                    return Ok(Some(Value::String(key)));
                }
            }
            // This object is exhausted: continue with its prototype.
            let prototype = self.object_get_prototype(object)?;
            let next = prototype.map_or(Value::Null, Value::Object);
            self.with_roots(|heap| heap.set(record, "forInObject", next))?;
            self.with_roots(|heap| heap.set(record, "forInKeys", Value::Undefined))?;
        }
    }

    fn array_contains_object(
        &mut self,
        array: ObjectId,
        object: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let length = match self.heap.get_own(array, "length")? {
            Some(Value::Number(length)) => length as usize,
            _ => 0,
        };
        for index in 0..length {
            if self.heap.get_own(array, index.to_string())? == Some(Value::Object(object)) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
