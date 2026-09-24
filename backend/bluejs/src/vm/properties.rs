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
                    .get_property(&constructor, &"prototype".into())
                    .expect("an intrinsic constructor's prototype is a data property")
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
        if key == "constructor" && self.heap.is_array(target).expect(Self::LIVE_OBJECT) {
            // Array instances inherit this property from `%Array.prototype%`.
            // The intrinsic constructor is otherwise lazy, but the inherited
            // lookup may occur before a global `Array` reference in the same
            // expression. Materialize it first so `[].constructor` is not
            // observably absent.
            self.global("Array")?;
        }
        if key == "constructor" && self.callable(&Value::Object(target)) {
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
            return Ok(self
                .record_get(cell, "value")
                .expect("a global property cell always holds its value"));
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
        // `%Object.prototype%` has an initial own constructor property.
        // Intrinsics otherwise bootstrap lazily, so make it observable before
        // an ordinary object performs an inherited lookup.
        if key == "constructor"
            && self
                .heap
                .get_own_property_descriptor(self.object_prototype, "constructor")
                .expect(Self::LIVE_OBJECT)
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
            let ordinary = self.heap.proxy(*object)?.is_none();
            if ordinary && self.test262_foreign_reference(*object).is_some() {
                return self.test262_foreign_set(*object, key, value);
            }
            if ordinary && self.test262_reverse_reference(*object).is_some() {
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
        let name = primitive::string(&self.pop()).expect("compiler emits a string private name");
        let receiver = self.pop();
        // An owner binding is made by the compiler for the class scope, never
        // by `eval`, so reading it cannot take the eval-variable lookup that
        // is the only fallible path of `binding_value`.
        let owner = Self::private_owner(
            self.binding_value(owner_slot)
                .expect("a private owner binding is never eval-created"),
        )?;
        Ok((receiver, owner, name))
    }

    /// The object standing for a class's private-name environment, from the
    /// value of the owner binding a private access resolved to.
    fn private_owner(owner: Option<Value>) -> Result<ObjectId, RuntimeError> {
        owner.and_then(|value| value.object_id()).ok_or_else(|| {
            RuntimeError::TypeError("private elements are not available in this function".into())
        })
    }

    pub(super) fn private_receiver(
        &self,
        receiver: &Value,
        owner: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("private fields require an object receiver".into())
        })?;
        if !self
            .heap
            .has_private_brand(object, owner)
            .expect(Self::LIVE_OBJECT)
        {
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
        let element = self
            .heap
            .private_element(owner, name)
            .expect(Self::LIVE_OBJECT)
            .expect("the compiler declares every private name its class uses");
        match element {
            PrivateElement::Field => self
                .heap
                .private_slot(object, owner, name)
                .expect(Self::LIVE_OBJECT)
                .ok_or_else(|| {
                    RuntimeError::TypeError("private field has not been initialized".into())
                }),
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
        // The compiler emits this only right after an anonymous function
        // definition, so the value is always a function object.
        let function = function
            .object_id()
            .expect("SetFunctionName follows a function definition");
        let prefix_text: JsString = match prefix {
            1 => "get ".into(),
            2 => "set ".into(),
            _ => JsString::default(),
        };
        // The parser leaves an anonymous function's name empty, or (for a
        // computed object-literal accessor) just its prefix.
        let anonymous = self.record_get(function, "name").is_none_or(|name| {
            matches!(name, Value::String(ref existing)
                if existing.byte_len() == 0 || *existing == prefix_text)
        });
        if !anonymous {
            return Ok(());
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
            // The key has already been through ToPropertyKey.
            other => name.push_str(
                &crate::primitive::string(other)
                    .expect("a property key that is not a symbol is a string"),
            ),
        }
        self.define_data(function, "name", Value::String(name), false, false, true)
    }

    pub(super) fn private_set(
        &mut self,
        receiver: &Value,
        owner: ObjectId,
        name: JsString,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let object = self.private_receiver(receiver, owner)?;
        let element = self
            .heap
            .private_element(owner, &name)
            .expect(Self::LIVE_OBJECT)
            .expect("the compiler declares every private name its class uses");
        match element {
            PrivateElement::Field => {
                // Assignment updates an initialized field; it never adds one.
                if self
                    .heap
                    .private_slot(object, owner, &name)
                    .expect(Self::LIVE_OBJECT)
                    .is_none()
                {
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
            .get(class, "prototype")
            .expect(Self::LIVE_OBJECT)
            .object_id()
            .expect("class constructors have a prototype object");
        let (constructor_parent, instance_parent) = match &base {
            // `extends null`: the constructor still inherits from
            // %Function.prototype%; only the instance prototype chain ends.
            Value::Null => (
                Some(
                    self.function_prototype()
                        .expect("the class constructor was made with the string intrinsics"),
                ),
                None,
            ),
            Value::Object(base) if self.constructible(&Value::Object(*base)) => {
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
        // The class and its prototype are fresh, so neither can be part of a
        // chain that would make the new links cyclic.
        self.heap
            .set_prototype(class, constructor_parent)
            .expect("a fresh class cannot form a prototype cycle");
        self.heap
            .set_prototype(prototype, instance_parent)
            .expect("a fresh class prototype cannot form a prototype cycle");
        // SetClassHome already gave the class its closure metadata, so this
        // second write allocates nothing and only an invalid handle fails.
        self.with_roots(|heap| heap.set_closure_home(class, prototype))
            .expect("the class closure already has metadata");
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
        // Fields are only added to the object `this` (or an override) names.
        let object = receiver
            .object_id()
            .expect("private fields are added to an object");
        if self
            .heap
            .private_slot(object, owner, &name)
            .expect(Self::LIVE_OBJECT)
            .is_some()
        {
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
            .get(class, "prototype")
            .expect(Self::LIVE_OBJECT)
            .object_id()
            .expect("class constructors have a prototype object");
        self.with_roots(|heap| heap.set_closure_home(initializer, prototype))?;
        // The class closure's metadata was made by SetClassHome, so storing
        // its `[[Fields]]` allocates nothing and only an invalid handle fails.
        self.with_roots(|heap| heap.set_class_fields(class, initializer))
            .expect("the class closure already has metadata");
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
        let class = constructor
            .object_id()
            .expect("instance elements are initialized for a class constructor");
        if let Some(initializer) = self
            .heap
            .class_fields(class)
            .expect("instance elements are initialized for a class closure")
        {
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
            .get(class, "prototype")
            .expect(Self::LIVE_OBJECT)
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
        // The home object is an object literal or a class (or its prototype),
        // all ordinary, so reading its prototype cannot run a Proxy trap.
        Ok(self
            .object_get_prototype(home)
            .expect("a home object is ordinary")
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
        if !self.constructible(&constructor) {
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
            // `excluded` is the compiler's own array of property keys.
            let excluded = excluded
                .object_id()
                .expect("the compiler passes an array of excluded keys");
            let length = self.record_count(excluded, "length");
            let mut excluded_keys = Vec::new();
            for index in 0..length {
                self.charge_step()?;
                let key = self
                    .record_get(excluded, index.to_string())
                    .expect("the compiler stores every excluded key");
                excluded_keys.push(
                    self.coerce_property_key(&key)
                        .expect("an excluded key is already a property key"),
                );
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
                // `target` is an object literal or fresh rest object, so
                // defining a plain data property on it always succeeds.
                self.define_data(target, key, value, true, true, true)?;
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
    pub(super) fn is_for_in_record(&self, record: ObjectId) -> bool {
        self.record_get(record, "forInObject").is_some()
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
            self.record_overwrite(record, "done", Value::Bool(true));
        }
        result
    }

    fn for_in_next_key(&mut self, record: ObjectId) -> Result<Option<Value>, RuntimeError> {
        let visited = self
            .record_get(record, "forInVisited")
            .and_then(|value| value.object_id())
            .expect("a for-in record keeps its visited set");
        loop {
            // `forInObject` is `null` once the whole chain is exhausted.
            let Some(object) = self
                .record_get(record, "forInObject")
                .and_then(|value| value.object_id())
            else {
                return Ok(None);
            };
            self.stack.push(Value::Object(object));
            let keys = match self.record_get(record, "forInKeys") {
                Some(Value::Object(keys)) => keys,
                _ => {
                    // Entering this object: guard against a cyclic chain
                    // (a Proxy can return one), then list its own keys.
                    let chain = self
                        .record_get(record, "forInChain")
                        .and_then(|value| value.object_id())
                        .expect("a for-in record keeps its chain");
                    if self.array_contains_object(chain, object) {
                        self.record_overwrite(record, "forInObject", Value::Null);
                        continue;
                    }
                    let length = self.record_count(chain, "length");
                    self.with_roots(|heap| {
                        heap.set(chain, length.to_string(), Value::Object(object))
                    })?;
                    let mut names = Vec::new();
                    for key in self.object_own_property_keys(object)? {
                        if let PropertyName::String(key) = key {
                            names.push(Value::String(key));
                        }
                    }
                    let keys = self
                        .array_from(names)?
                        .object_id()
                        .expect("array_from returns an array object");
                    self.stack.push(Value::Object(keys));
                    self.record_overwrite(record, "forInKeys", Value::Object(keys));
                    self.record_overwrite(record, "forInIndex", Value::Number(0.0));
                    keys
                }
            };
            let length = self.record_count(keys, "length") as usize;
            let mut index = self.record_count(record, "forInIndex") as usize;
            while index < length {
                let key = primitive::string(
                    &self
                        .record_get(keys, index.to_string())
                        .expect("the key list has an entry below its length"),
                )
                .expect("the key list holds strings");
                index += 1;
                self.record_overwrite(record, "forInIndex", Value::Number(index as f64));
                let name = PropertyName::String(key.clone());
                if self.record_get(visited, name.clone()).is_some() {
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
            self.record_overwrite(record, "forInObject", next);
            self.record_overwrite(record, "forInKeys", Value::Undefined);
        }
    }

    fn array_contains_object(&self, array: ObjectId, object: ObjectId) -> bool {
        let length = self.record_count(array, "length");
        (0..length)
            .any(|index| self.record_get(array, index.to_string()) == Some(Value::Object(object)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_reference_resolves_to_its_bound_owner_object() {
        let mut vm = Vm::default();
        let owner = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
        vm.bindings = vec![Some(Value::Object(owner))];
        vm.stack
            .extend([Value::Number(3.0), Value::String("#x".into())]);
        assert_eq!(
            vm.private_reference(0),
            Ok((Value::Number(3.0), owner, JsString::from("#x")))
        );
    }

    #[test]
    fn a_private_reference_needs_a_bound_owner_object() {
        let mut vm = Vm::default();
        vm.bindings = vec![None];
        vm.stack
            .extend([Value::Undefined, Value::String("#x".into())]);
        assert_eq!(
            vm.private_reference(0),
            Err(RuntimeError::TypeError(
                "private elements are not available in this function".into()
            ))
        );
    }

    #[test]
    fn a_private_owner_binding_must_hold_an_object() {
        let owner = ObjectId { heap: 3, serial: 9 };
        assert_eq!(Vm::private_owner(Some(Value::Object(owner))), Ok(owner));
        for value in [None, Some(Value::Undefined), Some(Value::Number(1.0))] {
            assert_eq!(
                Vm::private_owner(value),
                Err(RuntimeError::TypeError(
                    "private elements are not available in this function".into()
                ))
            );
        }
    }
}
