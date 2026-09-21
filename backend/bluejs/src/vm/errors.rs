// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// ECMAScript ToBoolean, including the host-defined Annex B IsHTMLDDA
    /// override. Ordinary objects remain truthy.
    pub(super) fn to_boolean(&self, value: &Value) -> Result<bool, RuntimeError> {
        if let Value::Object(object) = value {
            if self.heap.is_html_dda(*object)? {
                return Ok(false);
            }
        }
        Ok(primitive::truthy(value))
    }

    pub(super) fn lookup_global_name(&mut self, name: &str) -> Result<Option<Value>, RuntimeError> {
        if self.dynamic_eval_bindings.contains_key(name)
            || self
                .dynamic_eval_outer_bindings
                .iter()
                .rev()
                .any(|bindings| bindings.contains_key(name))
        {
            return self
                .dynamic_eval_binding_value(name)?
                .map(Some)
                .ok_or_else(|| RuntimeError::ReferenceError(name.into()));
        }
        if self.global_bindings.contains_key(name) {
            return self
                .global_binding_value(name)?
                .map(Some)
                .ok_or_else(|| RuntimeError::ReferenceError(name.into()));
        }
        if let Some(&global) = self.globals.get("globalThis") {
            // `with` lookup and indirect name access can reach a standard
            // global without first compiling a direct intrinsic reference.
            // Materialize such lazy globals before testing the object record.
            self.materialize_lexical_global(global, name)?;
        }
        if let Some(&id) = self.globals.get("globalThis") {
            if self.has_property(id, &name.into())? {
                return self
                    .get_property(&Value::Object(id), &name.into())
                    .map(Some);
            }
        }
        Ok(None)
    }

    pub(super) fn has_property(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        self.materialize_string_intrinsics_for_key(key)?;
        let mut current = Some(object);
        while let Some(id) = current {
            if self.heap.proxy(id)?.is_some() {
                return self.proxy_has(id, key);
            }
            self.trigger_deferred_namespace(id, Some(key))?;
            if let Some(numeric) = self.heap.typed_array_numeric_key(id, key)? {
                return match numeric {
                    crate::heap::TypedArrayNumericKey::Index(index) => {
                        Ok(self.heap.typed_array_index_value(id, index)?.is_some())
                    }
                    crate::heap::TypedArrayNumericKey::Invalid => Ok(false),
                };
            }
            match self.heap.get_own_property_descriptor(id, key) {
                Ok(Some(_)) | Err(HeapError::UninitializedModuleExport) => {
                    // A namespace's [[HasProperty]] observes membership in
                    // [[Exports]], not the current value of that binding.
                    // An uninitialized export is therefore present even
                    // though [[Get]] and [[GetOwnProperty]] would throw.
                    return Ok(true);
                }
                Ok(None) => {}
                Err(error) => return Err(error.into()),
            }
            current = self.object_get_prototype(id)?;
        }
        Ok(false)
    }

    pub(super) fn typeof_value(&self, value: &Value) -> Result<&'static str, RuntimeError> {
        if let Value::Object(object) = value {
            if self.heap.is_html_dda(*object)? {
                return Ok("undefined");
            }
        }
        Ok(if self.is_callable(value)? {
            "function"
        } else {
            primitive::type_name(value)
        })
    }

    pub(super) fn error_global(&mut self, name: &str) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get(name) {
            return Ok(Value::Object(id));
        }
        let name = match name {
            "TypeError" => "TypeError",
            "RangeError" => "RangeError",
            "SyntaxError" => "SyntaxError",
            "ReferenceError" => "ReferenceError",
            "EvalError" => "EvalError",
            "URIError" => "URIError",
            "AggregateError" => "AggregateError",
            "SuppressedError" => "SuppressedError",
            "Test262Error" => "Test262Error",
            _ => "Error",
        };
        let string = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(string)?.unwrap();
        let parent = if name == "Error" || name == "Test262Error" {
            self.object_prototype
        } else {
            let error = self.error_global("Error")?;
            self.heap
                .get(error.object_id().unwrap(), "prototype")?
                .object_id()
                .unwrap()
        };
        let constructor_parent = if name == "Error" || name == "Test262Error" {
            function_prototype
        } else {
            self.globals["Error"]
        };
        let constructor = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::Error(name), name, constructor_parent)
        })?;
        let root = self.heap.root(constructor)?;
        let result = (|| {
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(parent)))?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                constructor,
                "name",
                Value::String(name.into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(if name == "AggregateError" {
                    2.0
                } else if name == "SuppressedError" {
                    3.0
                } else {
                    1.0
                }),
                false,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                "name",
                Value::String(name.into()),
                true,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                "message",
                Value::String("".into()),
                true,
                false,
                true,
            )?;
            if name == "Error" || name == "Test262Error" {
                self.install_native(
                    prototype,
                    function_prototype,
                    "toString",
                    0,
                    NativeFunction::ErrorToString,
                )?;
            }
            if name == "Error" {
                self.install_native(
                    constructor,
                    function_prototype,
                    "isError",
                    1,
                    NativeFunction::ErrorIsError,
                )?;
                self.install_native_accessor(
                    prototype,
                    function_prototype,
                    "stack",
                    NativeFunction::ErrorStackGetter,
                    NativeFunction::ErrorStackSetter,
                )?;
            }
            Ok(Value::Object(constructor))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert(name.into(), constructor);
            // Error constructors are lazy globals just like ordinary native
            // constructors. An unqualified lookup can use `globals`
            // directly, but property access through a Test262 realm facade
            // must observe the corresponding own property of globalThis.
            if let Some(&global) = self.globals.get("globalThis") {
                self.define_data(global, name, Value::Object(constructor), true, false, true)?;
            }
        }
        result
    }

    pub(super) fn error_constructor(
        &mut self,
        name: &str,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let constructor = self.error_global(name)?.object_id().unwrap();
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .unwrap();
        let prototype = if construct {
            self.constructor_prototype_for(default, Some(name))?
        } else {
            default
        };
        let object = self.with_roots(|heap| heap.alloc_error(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let aggregate = name == "AggregateError";
            let suppressed_error = name == "SuppressedError";
            // `SuppressedError(error, suppressed, message)` has its own
            // positional shape: `message` is the third argument (installed
            // first, matching Error's own message-before-cause ordering),
            // and there is no `cause`/options argument at all.
            if suppressed_error {
                let message = native::argument(args, 2);
                if *message != Value::Undefined {
                    let message = self.coerce_string(message)?;
                    self.define_data(object, "message", Value::String(message), true, false, true)?;
                }
                self.define_data(
                    object,
                    "error",
                    native::argument(args, 0).clone(),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    object,
                    "suppressed",
                    native::argument(args, 1).clone(),
                    true,
                    false,
                    true,
                )?;
                return Ok(Value::Object(object));
            }
            let message = native::argument(args, if aggregate { 1 } else { 0 });
            if *message != Value::Undefined {
                let message = self.coerce_string(message)?;
                self.define_data(object, "message", Value::String(message), true, false, true)?;
            }
            if let Value::Object(options) = native::argument(args, if aggregate { 2 } else { 1 }) {
                if self.has_property(*options, &"cause".into())? {
                    let cause = self.get_property(&Value::Object(*options), &"cause".into())?;
                    self.define_data(object, "cause", cause, true, false, true)?;
                }
            }
            if aggregate {
                // IteratorToList(GetIterator(errors)) runs after `message`
                // and `cause` are installed, and its result is a fresh array.
                let values = self.iterable_to_list(native::argument(args, 0))?;
                let base = self.stack.len();
                self.stack.extend(values.iter().cloned());
                let errors = self.array_from(values);
                self.stack.truncate(base);
                self.define_data(object, "errors", errors?, true, false, true)?;
            }
            Ok(Value::Object(object))
        })();
        self.stack.pop();
        result
    }

    pub(super) fn error_to_string(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Error.toString requires an object".into(),
            ));
        }
        let name = self.get_property(receiver, &"name".into())?;
        let mut name = if name == Value::Undefined {
            JsString::from("Error")
        } else {
            self.coerce_string(&name)?
        };
        let message = self.get_property(receiver, &"message".into())?;
        let message = if message == Value::Undefined {
            JsString::default()
        } else {
            self.coerce_string(&message)?
        };
        if name.is_empty() {
            return Ok(Value::String(message));
        }
        if !message.is_empty() {
            native::append(&mut name, &": ".into(), self.config.max_string_bytes)?;
            native::append(&mut name, &message, self.config.max_string_bytes)?;
        }
        Ok(Value::String(name))
    }

    /// IteratorToList(GetIterator(value, sync)): every value the iterable
    /// yields, in order. Values are kept on the VM stack while later
    /// iterator steps run user code.
    pub(super) fn iterable_to_list(&mut self, source: &Value) -> Result<Vec<Value>, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(source.clone());
        let result = (|| {
            let record = self.get_iterator(source)?;
            self.stack.push(record.clone());
            let mut values = Vec::new();
            while let Some(value) = self.iterator_step(&record, true)? {
                self.stack.push(value.clone());
                values.push(value);
            }
            Ok(values)
        })();
        self.stack.truncate(base);
        result
    }

    /// Whether `object` has an [[ErrorData]] internal slot, including an
    /// error owned by another Test262 realm behind a facade.
    fn has_error_data(&self, object: ObjectId) -> Result<bool, RuntimeError> {
        if let Some((realm, target, _, _)) = self.test262_foreign_reference(object) {
            let realm = self.test262_realms.get(&realm).ok_or_else(|| {
                RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
            })?;
            return Ok(realm.vm.heap.is_error(target)?);
        }
        Ok(self.heap.is_error(object)?)
    }

    /// `Error.isError ( arg )`.
    pub(super) fn error_is_error(&self, argument: &Value) -> Result<Value, RuntimeError> {
        Ok(Value::Bool(match argument {
            Value::Object(object) => self.has_error_data(*object)?,
            _ => false,
        }))
    }

    /// `get Error.prototype.stack`: an implementation-defined string for an
    /// object with [[ErrorData]], `undefined` for any other object. BlueJS
    /// tracks no call frames, so the string is the error's header line
    /// (`name: message`), read without running user code.
    pub(super) fn error_stack_getter(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let Value::Object(object) = receiver else {
            return Err(RuntimeError::TypeError(
                "Error.prototype.stack getter requires an object receiver".into(),
            ));
        };
        if !self.has_error_data(*object)? {
            return Ok(Value::Undefined);
        }
        let mut header = self
            .error_header_field(*object, "name")?
            .unwrap_or("Error".into());
        if let Some(message) = self.error_header_field(*object, "message")? {
            if !message.is_empty() {
                native::append(&mut header, &": ".into(), self.config.max_string_bytes)?;
                native::append(&mut header, &message, self.config.max_string_bytes)?;
            }
        }
        Ok(Value::String(header))
    }

    /// The String held by the first data property `key` on `object`'s
    /// prototype chain; getters, proxies and non-String values are ignored.
    fn error_header_field(
        &self,
        object: ObjectId,
        key: &str,
    ) -> Result<Option<JsString>, RuntimeError> {
        let key = PropertyName::from(key);
        let mut current = Some(object);
        while let Some(id) = current {
            if self.heap.proxy(id)?.is_some() || self.test262_foreign_reference(id).is_some() {
                return Ok(None);
            }
            if let Some(descriptor) = self.heap.get_own_property_descriptor(id, &key)? {
                return Ok(match descriptor.value {
                    Some(Value::String(text)) => Some(text),
                    _ => None,
                });
            }
            current = self.heap.prototype(id)?;
        }
        Ok(None)
    }

    /// `set Error.prototype.stack ( v )`: rejects non-Objects and non-Strings,
    /// then SetterThatIgnoresPrototypeProperties(this, %Error.prototype%,
    /// "stack", v).
    pub(super) fn error_stack_setter(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(object) = receiver else {
            return Err(RuntimeError::TypeError(
                "Error.prototype.stack setter requires an object receiver".into(),
            ));
        };
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Error.prototype.stack must be assigned a string".into(),
            ));
        }
        let error = self.error_global("Error")?;
        let home = self.get_property(&error, &"prototype".into())?;
        if home.object_id() == Some(*object) {
            return Err(RuntimeError::TypeError(
                "cannot assign Error.prototype.stack".into(),
            ));
        }
        let key: PropertyName = "stack".into();
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = (|| {
            let succeeded = if self.object_get_own_property(*object, &key)?.is_none() {
                self.object_define_own_property(
                    *object,
                    key.clone(),
                    PropertyDescriptor::data(value.clone(), true, true, true),
                )?
            } else {
                self.ordinary_set_with_receiver(*object, receiver, &key, value)?
            };
            if succeeded {
                Ok(Value::Undefined)
            } else {
                Err(RuntimeError::TypeError(
                    "cannot assign the stack property".into(),
                ))
            }
        })();
        self.stack.truncate(base);
        result
    }
}
