// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Internal name of the wrapper declaration `dynamic_function_constructor`
/// compiles; it is not a valid identifier, so no source can refer to it.
const DYNAMIC_FUNCTION_BINDING: &str = "*anonymous*";

impl Vm {
    /// ECMA-262 Function constructor. Dynamic function source is compiled in
    /// the realm's global environment rather than inheriting the native
    /// caller's active lexical bindings.
    pub(in super::super) fn function_constructor(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.dynamic_function_constructor(args, false)
    }

    /// Shared constructor path for `%Function%` and `%AsyncFunction%`. Dynamic
    /// functions compile against the realm global environment; the async form
    /// then takes the same Promise/continuation path as a source async
    /// function. Generators have a separate constructor family and are not
    /// conflated with this operation.
    pub(in super::super) fn async_function_constructor(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.dynamic_function_constructor(args, true)
    }

    pub(in super::super) fn dynamic_function_constructor(
        &mut self,
        args: &[Value],
        async_function: bool,
    ) -> Result<Value, RuntimeError> {
        let mut parameters = String::new();
        for (index, argument) in args.iter().take(args.len().saturating_sub(1)).enumerate() {
            if index != 0 {
                parameters.push(',');
            }
            parameters.push_str(&self.coerce_string(argument)?.to_utf8().map_err(|_| {
                RuntimeError::SyntaxError(
                    "Function parameter contains an unpaired surrogate".into(),
                )
            })?);
        }
        let mut source = String::from(if async_function {
            "async function anonymous("
        } else {
            "function anonymous("
        });
        source.push_str(&strip_dynamic_function_html_comments(&parameters));
        // Dynamic parameter text is parsed as its own grammar production.
        // Preserve that boundary in the generated wrapper: a trailing
        // single-line comment belongs to the parameters, not to the closing
        // parenthesis that follows them.
        source.push_str("\n) {\n");
        if let Some(body) = args.last() {
            source.push_str(&self.coerce_string(body)?.to_utf8().map_err(|_| {
                RuntimeError::SyntaxError("Function body contains an unpaired surrogate".into())
            })?);
        }
        source.push_str("\n}");

        let mut program =
            crate::parse(&source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        // The wrapper declaration only gives the function its `name` property.
        // CreateDynamicFunction binds no such name in the function's scope, so
        // rename the declaration to an internal identifier no source can spell:
        // `anonymous` in the body then resolves like any free identifier.
        if let Some(crate::ast::Stmt::FunctionDecl(function)) = program.body.first_mut() {
            function.name = Some(DYNAMIC_FUNCTION_BINDING.into());
        }
        let code = crate::compile(&program)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let child = code
            .functions
            .first()
            .cloned()
            .expect("Function wrapper compiles one function declaration");

        debug_assert_eq!(child.async_function, async_function);
        let default_prototype = if async_function {
            self.async_function_prototype()?
        } else {
            self.function_prototype()?
        };
        // CreateDynamicFunction selects its function object's prototype with
        // GetPrototypeFromConstructor for every dynamic function kind. In
        // particular, `%AsyncFunction%` must observe a revoked Proxy
        // newTarget rather than silently using its default prototype.
        let function_prototype = if self.new_target != Value::Undefined {
            self.constructor_prototype(default_prototype)?
        } else {
            default_prototype
        };
        // Compiling the wrapper declaration produces a single capture for
        // its declaration name. It is an implementation detail of using the
        // ordinary compiler, not a capture of the Function caller.
        let stack_base = self.stack.len();
        let function: Result<ObjectId, RuntimeError> = (|| {
            let mut captures = Vec::with_capacity(child.captures.len());
            for _ in &child.captures {
                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                self.stack.push(Value::Object(cell));
                captures.push(cell);
            }
            let function = self.with_roots(|heap| {
                heap.alloc_closure(
                    child.clone(),
                    captures.clone(),
                    Value::Undefined,
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(function));
            for (&slot, &cell) in child.captures.iter().zip(&captures) {
                let value = if code.bindings[slot as usize].name == DYNAMIC_FUNCTION_BINDING {
                    Value::Object(function)
                } else {
                    Value::Undefined
                };
                self.with_roots(|heap| heap.set(cell, "value", value))?;
            }
            Ok(function)
        })();
        self.stack.truncate(stack_base);
        let function = function?;
        self.stack.push(Value::Object(function));
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                function,
                "name",
                Value::String("anonymous".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                function,
                "length",
                Value::Number(child.function_length as f64),
                false,
                false,
                true,
            )?;
            if child.constructible
                && !child.strict
                && !child.arrow
                && !child.generator
                && !child.async_function
            {
                self.install_legacy_function_properties(function)?;
            }
            if child.constructible {
                let object_prototype = self.object_prototype;
                let prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(
                    function,
                    "prototype",
                    Value::Object(prototype),
                    true,
                    false,
                    false,
                )?;
                self.define_data(
                    prototype,
                    "constructor",
                    Value::Object(function),
                    true,
                    false,
                    true,
                )?;
            }
            Ok(())
        })();
        self.stack.truncate(stack_base);
        result?;
        Ok(Value::Object(function))
    }

    pub(in super::super) fn coerce_object(
        &mut self,
        value: &Value,
    ) -> Result<ObjectId, RuntimeError> {
        match value {
            Value::Object(id) => Ok(*id),
            Value::String(s) => {
                let (_, prototype) = self.string_intrinsics()?;
                Ok(self.with_roots(|heap| heap.alloc_string(s.clone(), Some(prototype)))?)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot convert null or undefined to Object".into(),
            )),
            _ => {
                let constructor = self.global(match value {
                    Value::Symbol(_) => "Symbol",
                    Value::Bool(_) => "Boolean",
                    Value::BigInt(_) => "BigInt",
                    _ => "Number",
                })?;
                let prototype = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                Ok(self.with_roots(|heap| heap.alloc_boxed_primitive(value.clone(), prototype))?)
            }
        }
    }

    pub(in super::super) fn object_method(
        &mut self,
        method: ObjectMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        use ObjectMethod::*;
        let first = native::argument(args, 0);
        if method == IsExtensible && !matches!(first, Value::Object(_)) {
            return Ok(Value::Bool(false));
        }
        if matches!(method, IsSealed | IsFrozen) && !matches!(first, Value::Object(_)) {
            return Ok(Value::Bool(true));
        }
        if matches!(method, PreventExtensions | Seal | Freeze) && !matches!(first, Value::Object(_))
        {
            return Ok(first.clone());
        }
        if matches!(
            method,
            DefineProperty
                | DefineProperties
                | OwnKeys
                | ReflectGet
                | ReflectGetOwnPropertyDescriptor
                | ReflectDefineProperty
                | ReflectSet
                | ReflectDeleteProperty
                | ReflectPreventExtensions
                | ReflectGetPrototypeOf
                | ReflectSetPrototypeOf
                | ReflectIsExtensible
                | ReflectHas
        ) && !matches!(first, Value::Object(_))
        {
            return Err(RuntimeError::TypeError(
                "operation requires an object".into(),
            ));
        }
        // Object.prototype.hasOwnProperty and propertyIsEnumerable perform
        // ToPropertyKey before ToObject(this).  The conversion may call
        // user code, so its abrupt completion must win over a nullish
        // receiver error.
        let property_query_key = if matches!(method, PropertyIsEnumerable | HasOwnProperty) {
            Some(self.coerce_property_key(first)?)
        } else {
            None
        };
        if method == Is {
            return Ok(Value::Bool(same_value(
                native::argument(args, 0),
                native::argument(args, 1),
            )));
        }
        if method == FromEntries {
            return self.object_from_entries_method(first);
        }
        if method == GroupBy {
            return self.object_group_by_method(first, native::argument(args, 1));
        }
        let object = if matches!(method, PropertyIsEnumerable | HasOwnProperty) {
            self.coerce_object(receiver)?
        } else if method == Create {
            let prototype = match first {
                Value::Null => None,
                Value::Object(id) => Some(*id),
                _ => {
                    return Err(RuntimeError::TypeError(
                        "Object.create prototype must be object or null".into(),
                    ))
                }
            };
            self.with_roots(|heap| heap.alloc_object(prototype))?
        } else {
            self.coerce_object(first)?
        };
        self.stack.push(Value::Object(object));
        match method {
            Is => unreachable!("Object.is returns before object coercion"),
            FromEntries => unreachable!("Object.fromEntries creates its result before coercion"),
            GroupBy => unreachable!("Object.groupBy creates its result before object coercion"),
            Assign => {
                let base = self.stack.len();
                let result = (|| {
                    for source in args.iter().skip(1) {
                        if matches!(source, Value::Undefined | Value::Null) {
                            continue;
                        }
                        let source = self.coerce_object(source)?;
                        self.stack.push(Value::Object(source));
                        for key in self.object_own_property_keys(source)? {
                            let Some(descriptor) = self.object_get_own_property(source, &key)?
                            else {
                                continue;
                            };
                            if descriptor.enumerable != Some(true) {
                                continue;
                            }
                            let value = self.get_property(&Value::Object(source), &key)?;
                            self.stack.push(value.clone());
                            if !self.ordinary_set_with_receiver(
                                object,
                                &Value::Object(object),
                                &key,
                                &value,
                            )? {
                                return Err(RuntimeError::TypeError(
                                    "cannot assign property".into(),
                                ));
                            }
                            self.stack.pop();
                        }
                        self.stack.pop();
                    }
                    Ok(Value::Object(object))
                })();
                self.stack.truncate(base);
                result
            }
            HasOwn => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                Ok(Value::Bool(
                    self.object_get_own_property(object, &key)?.is_some(),
                ))
            }
            GetOwnPropertyDescriptors => {
                // Object.getOwnPropertyDescriptors observes the object's
                // internal methods directly.  In particular, it does not
                // call the public Object.getOwnPropertyDescriptor property:
                // replacing that property must not affect this operation.
                // Keeping the walk at this boundary also preserves Proxy
                // trap order and lets primitive inputs use their ordinary
                // temporary wrapper without exposing it to JavaScript.
                let prototype = self.object_prototype;
                let descriptors = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(descriptors));
                for key in self.object_own_property_keys(object)? {
                    let Some(descriptor) = self.object_get_own_property(object, &key)? else {
                        continue;
                    };
                    let descriptor_object =
                        self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                    self.stack.push(Value::Object(descriptor_object));
                    for (name, value) in [
                        ("value", descriptor.value),
                        ("writable", descriptor.writable.map(Value::Bool)),
                        ("get", descriptor.get),
                        ("set", descriptor.set),
                        ("enumerable", descriptor.enumerable.map(Value::Bool)),
                        ("configurable", descriptor.configurable.map(Value::Bool)),
                    ] {
                        if let Some(value) = value {
                            self.with_roots(|heap| heap.set(descriptor_object, name, value))?;
                        }
                    }
                    let defined = self.object_define_own_property(
                        descriptors,
                        key,
                        PropertyDescriptor::data(
                            Value::Object(descriptor_object),
                            true,
                            true,
                            true,
                        ),
                    )?;
                    if !defined {
                        return Err(RuntimeError::TypeError(
                            "cannot define descriptor property".into(),
                        ));
                    }
                    self.stack.pop();
                }
                Ok(Value::Object(descriptors))
            }
            DefineProperties => {
                let properties = self.coerce_object(native::argument(args, 1))?;
                self.stack.push(Value::Object(properties));
                let mut descriptors = Vec::new();
                for key in self.object_own_property_keys(properties)? {
                    if self
                        .object_get_own_property(properties, &key)?
                        .is_none_or(|descriptor| descriptor.enumerable != Some(true))
                    {
                        continue;
                    }
                    let descriptor_object = self.get_property(&Value::Object(properties), &key)?;
                    self.stack.push(descriptor_object.clone());
                    descriptors.push((key, self.read_descriptor(&descriptor_object)?));
                }
                for (key, mut descriptor) in descriptors {
                    if key == "length" && self.heap.is_array(object)? {
                        if let Some(value) = &descriptor.value {
                            descriptor.value = Some(self.array_length_value(value)?);
                        }
                    }
                    if !self.object_define_own_property(object, key, descriptor)? {
                        return Err(RuntimeError::TypeError("cannot redefine property".into()));
                    }
                }
                Ok(Value::Object(object))
            }
            ReflectGet => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                let receiver = args.get(2).cloned().unwrap_or(Value::Object(object));
                self.get_object_property(object, &receiver, &key)
            }
            GetOwnPropertyDescriptor
            | ReflectGetOwnPropertyDescriptor
            | DefineProperty
            | ReflectDefineProperty => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                if matches!(method, DefineProperty | ReflectDefineProperty) {
                    let mut descriptor = self.read_descriptor(native::argument(args, 2))?;
                    if key == "length" && self.heap.is_array(object)? {
                        if let Some(value) = &descriptor.value {
                            descriptor.value = Some(self.array_length_value(value)?);
                        }
                    }
                    let defined = self.object_define_own_property(object, key, descriptor)?;
                    if method == ReflectDefineProperty {
                        return Ok(Value::Bool(defined));
                    }
                    if !defined {
                        return Err(RuntimeError::TypeError("cannot redefine property".into()));
                    }
                    return Ok(Value::Object(object));
                }
                let Some(descriptor) = self.object_get_own_property(object, &key)? else {
                    return Ok(Value::Undefined);
                };
                let prototype = self.object_prototype;
                let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(result));
                for (name, value) in [
                    ("value", descriptor.value),
                    ("writable", descriptor.writable.map(Value::Bool)),
                    ("get", descriptor.get),
                    ("set", descriptor.set),
                    ("enumerable", descriptor.enumerable.map(Value::Bool)),
                    ("configurable", descriptor.configurable.map(Value::Bool)),
                ] {
                    if let Some(value) = value {
                        self.with_roots(|heap| heap.set(result, name, value))?;
                    }
                }
                Ok(Value::Object(result))
            }
            PropertyIsEnumerable => {
                let key = property_query_key.expect("property query key was coerced");
                // [[GetOwnProperty]], not the raw heap record: a lazily
                // materialized intrinsic global is a real own property that
                // merely has not been created yet, and an exotic object
                // (e.g. a Proxy) must have its own trap observed here
                // exactly as `Object.hasOwn`/`Object.getOwnPropertyDescriptor`
                // already do.
                Ok(Value::Bool(
                    self.object_get_own_property(object, &key)?
                        .is_some_and(|descriptor| descriptor.enumerable == Some(true)),
                ))
            }
            HasOwnProperty => {
                let key = property_query_key.expect("property query key was coerced");
                Ok(Value::Bool(
                    self.object_get_own_property(object, &key)?.is_some(),
                ))
            }
            Keys | Values | Entries | GetOwnPropertyNames | GetOwnPropertySymbols | OwnKeys => {
                // Each entry array is created before the final result array.
                // Keep already-created object values on the VM stack while a
                // later getter or allocation can trigger collection.
                let base = self.stack.len();
                let result = (|| {
                    let keys = self.object_own_property_keys(object)?;
                    let mut values = Vec::new();
                    for key in keys {
                        if matches!(method, Keys | Values | Entries) {
                            // EnumerableOwnProperties filters to String keys
                            // before it invokes [[GetOwnProperty]].  A Proxy
                            // descriptor trap must therefore never observe a
                            // symbol that Object.keys/values/entries will omit.
                            if !matches!(key, PropertyName::String(_)) {
                                continue;
                            }
                            // EnumerableOwnProperties snapshots keys, but obtains
                            // a descriptor for each key immediately before it
                            // observes the value.  An earlier getter can delete
                            // a later key, in which case it is simply omitted.
                            let Some(descriptor) = self.object_get_own_property(object, &key)?
                            else {
                                continue;
                            };
                            if descriptor.enumerable != Some(true) {
                                continue;
                            }
                        }
                        if method == GetOwnPropertyNames && !matches!(key, PropertyName::String(_))
                            || method == GetOwnPropertySymbols
                                && !matches!(key, PropertyName::Symbol(_))
                        {
                            continue;
                        }
                        let value = if method == Values {
                            self.get_property(&Value::Object(object), &key)?
                        } else if method == Entries {
                            let value = self.get_property(&Value::Object(object), &key)?;
                            self.array_from(vec![key.value(), value])?
                        } else {
                            key.value()
                        };
                        if matches!(value, Value::Object(_)) {
                            self.stack.push(value.clone());
                        }
                        values.push(value);
                    }
                    self.array_from(values)
                })();
                self.stack.truncate(base);
                result
            }
            GetPrototypeOf | ReflectGetPrototypeOf => Ok(self
                .object_get_prototype(object)?
                .map_or(Value::Null, Value::Object)),
            SetPrototypeOf | ReflectSetPrototypeOf => {
                let prototype = match native::argument(args, 1) {
                    Value::Null => None,
                    Value::Object(id) => Some(*id),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "prototype must be object or null".into(),
                        ))
                    }
                };
                let changed = self.object_set_prototype(object, prototype)?;
                if method == ReflectSetPrototypeOf {
                    return Ok(Value::Bool(changed));
                }
                if !changed {
                    return Err(RuntimeError::TypeError(
                        "cannot set object prototype".into(),
                    ));
                }
                Ok(first.clone())
            }
            Create => {
                let properties = native::argument(args, 1);
                if *properties != Value::Undefined {
                    let properties = self.coerce_object(properties)?;
                    self.stack.push(Value::Object(properties));
                    let mut descriptors = Vec::new();
                    for key in self.object_own_property_keys(properties)? {
                        if self
                            .object_get_own_property(properties, &key)?
                            .is_some_and(|d| d.enumerable == Some(true))
                        {
                            let value = self.get_property(&Value::Object(properties), &key)?;
                            self.stack.push(value.clone());
                            descriptors.push((key, self.read_descriptor(&value)?));
                        }
                    }
                    for (key, descriptor) in descriptors {
                        if !self.object_define_own_property(object, key, descriptor)? {
                            return Err(RuntimeError::TypeError("cannot define property".into()));
                        }
                    }
                }
                Ok(Value::Object(object))
            }
            IsExtensible | ReflectIsExtensible => {
                Ok(Value::Bool(self.object_is_extensible(object)?))
            }
            PreventExtensions => {
                if !self.object_prevent_extensions(object)? {
                    return Err(RuntimeError::TypeError(
                        "cannot prevent object extensions".into(),
                    ));
                }
                Ok(Value::Object(object))
            }
            ReflectPreventExtensions => Ok(Value::Bool(self.object_prevent_extensions(object)?)),
            ReflectSet => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                let value = native::argument(args, 2).clone();
                let receiver = args.get(3).cloned().unwrap_or(Value::Object(object));
                Ok(Value::Bool(self.ordinary_set_with_receiver(
                    object, &receiver, &key, &value,
                )?))
            }
            ReflectDeleteProperty => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                Ok(Value::Bool(self.object_delete(object, &key)?))
            }
            ReflectHas => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                Ok(Value::Bool(self.has_property(object, &key)?))
            }
            Seal | Freeze => {
                // SetIntegrityLevel makes the object non-extensible before
                // attempting its per-property descriptor changes. This is
                // observable for integer-indexed exotics: a resizable or
                // length-tracking TypedArray rejects the first step, while a
                // fixed non-empty view becomes non-extensible and then
                // rejects redefining its indexed properties.
                if !self.object_prevent_extensions(object)? {
                    return Err(RuntimeError::TypeError(
                        "cannot make object non-extensible".into(),
                    ));
                }
                let keys = self.object_own_property_keys(object)?;
                for key in keys {
                    let Some(current) = self.object_get_own_property(object, &key)? else {
                        continue;
                    };
                    let descriptor = PropertyDescriptor {
                        configurable: Some(false),
                        writable: (method == Freeze && current.value.is_some()).then_some(false),
                        ..Default::default()
                    };
                    if !self.object_define_own_property(object, key, descriptor)? {
                        return Err(RuntimeError::TypeError(
                            "cannot make object non-extensible".into(),
                        ));
                    }
                }
                Ok(first.clone())
            }
            IsSealed | IsFrozen => {
                if self.object_is_extensible(object)? {
                    return Ok(Value::Bool(false));
                }
                for key in self.object_own_property_keys(object)? {
                    let Some(descriptor) = self.object_get_own_property(object, &key)? else {
                        continue;
                    };
                    if descriptor.configurable != Some(false)
                        || (method == IsFrozen
                            && descriptor.value.is_some()
                            && descriptor.writable != Some(false))
                    {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
        }
    }

    pub(in super::super) fn read_descriptor(
        &mut self,
        value: &Value,
    ) -> Result<PropertyDescriptor, RuntimeError> {
        let Value::Object(object) = value else {
            return Err(RuntimeError::TypeError(
                "descriptor must be an object".into(),
            ));
        };
        let base = self.stack.len();
        let result = (|| {
            let mut descriptor = PropertyDescriptor::default();
            for name in [
                "enumerable",
                "configurable",
                "value",
                "writable",
                "get",
                "set",
            ] {
                if !self.has_property(*object, &name.into())? {
                    continue;
                }
                let property = self.get_property(value, &name.into())?;
                // Later descriptor getters may allocate and collect earlier values.
                self.stack.push(property.clone());
                match name {
                    "enumerable" => descriptor.enumerable = Some(self.to_boolean(&property)?),
                    "configurable" => descriptor.configurable = Some(self.to_boolean(&property)?),
                    "writable" => descriptor.writable = Some(self.to_boolean(&property)?),
                    "value" => descriptor.value = Some(property),
                    _ => {
                        if property != Value::Undefined && !self.is_callable(&property)? {
                            return Err(RuntimeError::TypeError(
                                "accessor must be callable or undefined".into(),
                            ));
                        }
                        if name == "get" {
                            descriptor.get = Some(property);
                        } else {
                            descriptor.set = Some(property);
                        }
                    }
                }
            }
            if descriptor.accessor()
                && (descriptor.value.is_some() || descriptor.writable.is_some())
            {
                return Err(RuntimeError::TypeError(
                    "invalid mixed property descriptor".into(),
                ));
            }
            Ok(descriptor)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn string_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let iterator_base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(iterator_base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::IteratorNext,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("String Iterator".into()),
                false,
                false,
                true,
            )?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.iterator_prototype = Some(prototype);
        }
        result
    }

    pub(in super::super) fn install_symbol_native(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        symbol: &str,
        length: u32,
        native: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let name = format!("[Symbol.{symbol}]");
        let id = self.with_roots(|heap| heap.alloc_native_function(native, &name, prototype))?;
        self.stack.push(Value::Object(id));
        self.define_data(
            id,
            "name",
            Value::String(format!("[Symbol.{symbol}]").into()),
            false,
            false,
            true,
        )?;
        self.define_data(
            id,
            "length",
            Value::Number(length as f64),
            false,
            false,
            true,
        )?;
        // Function.prototype @@hasInstance is the immutable Symbol method.
        let mutable = native != NativeFunction::HasInstance;
        self.define_data(
            owner,
            JsSymbol::well_known(symbol),
            Value::Object(id),
            mutable,
            false,
            mutable,
        )?;
        self.stack.pop();
        Ok(())
    }

    pub(in super::super) fn string_pattern(
        &mut self,
        method: PatternMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let pattern = native::argument(args, 0);
        let symbol = match method {
            PatternMethod::Match => "match",
            PatternMethod::MatchAll => "matchAll",
            PatternMethod::Search => "search",
        };
        // `String.prototype.{match,matchAll,search}` only obtains a symbol
        // hook from an Object argument. `GetMethod` would box a primitive and
        // incorrectly observe a hook installed on (for example)
        // `Number.prototype[Symbol.match]`; the standard methods must instead
        // create a RegExp from that primitive.
        if matches!(pattern, Value::Object(_)) {
            if method == PatternMethod::MatchAll {
                self.require_global_pattern(pattern)?;
            }
            let method = self.get_method(pattern, &JsSymbol::well_known(symbol).into())?;
            if method != Value::Undefined {
                return self.call_native(method, pattern.clone(), vec![receiver.clone()], false);
            }
        }
        let string = self.coerce_string(receiver)?;
        let regexp = self.regexp_create(
            pattern,
            &if method == PatternMethod::MatchAll {
                Value::String("g".into())
            } else {
                Value::Undefined
            },
        )?;
        self.stack.push(regexp.clone());
        let function = self.get_property(&regexp, &JsSymbol::well_known(symbol).into())?;
        self.call_native(function, regexp, vec![Value::String(string)], false)
    }
}
