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
        if let Some(&id) = self.globals.get(name) {
            return Ok(Some(Value::Object(id)));
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
        &self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        let mut current = Some(object);
        while let Some(id) = current {
            if self.heap.get_own_property_descriptor(id, key)?.is_some() {
                return Ok(true);
            }
            current = self.heap.prototype(id)?;
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
                Value::Number(1.0),
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
            Ok(Value::Object(constructor))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert(name.into(), constructor);
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
            self.constructor_prototype(default)?
        } else {
            default
        };
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        let message = native::argument(args, 0);
        if *message != Value::Undefined {
            let message = self.coerce_string(message)?;
            self.define_data(object, "message", Value::String(message), true, false, true)?;
        }
        if let Value::Object(options) = native::argument(args, 1) {
            if self.has_property(*options, &"cause".into())? {
                let cause = self.get_property(&Value::Object(*options), &"cause".into())?;
                self.define_data(object, "cause", cause, true, false, true)?;
            }
        }
        Ok(Value::Object(object))
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
}
