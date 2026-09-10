// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit host setup, never installed in an ordinary realm. Test262 permits
//! overriding harness functions; failures remain distinct from engine errors.
use super::*;

impl Vm {
    /// Installs native Test262 assertion and descriptor helper functions in
    /// this realm. Additional harness includes and asynchronous/module hosts
    /// are the runner's job.
    pub fn install_test262_harness(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let result = self.install_test262_functions();
        self.stack.truncate(base);
        result
    }

    fn install_test262_functions(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        for name in ["isNaN", "isFinite", "parseInt", "parseFloat"] {
            self.global(name)?;
        }
        self.json_global()?;
        let string = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(string)?.unwrap();
        self.install_native(
            global,
            prototype,
            "assert",
            1,
            NativeFunction::Test262("assert"),
        )?;
        let assert = self.heap.get(global, "assert")?.object_id().unwrap();
        for (name, length) in [
            ("sameValue", 2),
            ("notSameValue", 2),
            ("_isSameValue", 2),
            ("throws", 2),
            ("compareArray", 2),
        ] {
            self.install_native(
                assert,
                prototype,
                name,
                length,
                NativeFunction::Test262(name),
            )?;
        }
        self.install_native(
            global,
            prototype,
            "isPrimitive",
            1,
            NativeFunction::Test262("isPrimitive"),
        )?;
        self.install_native(
            global,
            prototype,
            "isNegativeZero",
            1,
            NativeFunction::Test262("isNegativeZero"),
        )?;
        self.install_native(
            global,
            prototype,
            "formatIdentityFreeValue",
            1,
            NativeFunction::Test262("formatIdentityFreeValue"),
        )?;
        self.install_native(
            global,
            prototype,
            "formatSimpleValue",
            1,
            NativeFunction::Test262("formatSimpleValue"),
        )?;
        self.install_native(
            global,
            prototype,
            "compareArray",
            2,
            NativeFunction::Test262("arrayEqual"),
        )?;
        for (property, global_name) in [
            ("_formatIdentityFreeValue", "formatIdentityFreeValue"),
            ("_toString", "formatSimpleValue"),
        ] {
            let value = self.heap.get(global, global_name)?;
            self.define_data(assert, property, value, true, true, true)?;
        }
        let compare = self.heap.get(global, "compareArray")?.object_id().unwrap();
        self.install_native(
            compare,
            prototype,
            "format",
            1,
            NativeFunction::Test262("formatArray"),
        )?;
        for (name, length) in [
            ("verifyProperty", 4),
            ("verifyCallableProperty", 6),
            ("verifyAccessorProperty", 4),
            ("verifyEqualTo", 3),
            ("verifyWritable", 4),
            ("verifyNotWritable", 4),
            ("verifyEnumerable", 2),
            ("verifyNotEnumerable", 2),
            ("verifyConfigurable", 2),
            ("verifyNotConfigurable", 2),
            ("verifyPrimordialProperty", 4),
            ("verifyPrimordialCallableProperty", 6),
            ("verifyPrimordialAccessorProperty", 4),
            ("isConstructor", 1),
        ] {
            self.install_native(
                global,
                prototype,
                name,
                length,
                NativeFunction::Test262(name),
            )?;
        }
        self.install_native(
            global,
            prototype,
            "$DONOTEVALUATE",
            0,
            NativeFunction::Test262("$DONOTEVALUATE"),
        )?;
        let error = self.error_global("Test262Error")?.object_id().unwrap();
        self.define_data(
            global,
            "Test262Error",
            Value::Object(error),
            true,
            false,
            true,
        )?;
        self.install_native(
            error,
            prototype,
            "thrower",
            1,
            NativeFunction::Test262("thrower"),
        )?;
        Ok(())
    }

    pub(super) fn test262_call(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        if matches!(
            name,
            "verifyProperty"
                | "verifyCallableProperty"
                | "verifyAccessorProperty"
                | "verifyEqualTo"
                | "verifyWritable"
                | "verifyNotWritable"
                | "verifyEnumerable"
                | "verifyNotEnumerable"
                | "verifyConfigurable"
                | "verifyNotConfigurable"
                | "verifyPrimordialProperty"
                | "verifyPrimordialCallableProperty"
                | "verifyPrimordialAccessorProperty"
        ) {
            return self.test262_property_helper(name, args);
        }
        if name == "isConstructor" {
            if !self.is_callable(first)? {
                return Err(self.test262_failure(name));
            }
            return Ok(Value::Bool(self.is_constructor(first)?));
        }
        let passed = match name {
            "isPrimitive" => return Ok(Value::Bool(!matches!(first, Value::Object(_)))),
            "isNegativeZero" => {
                return Ok(Value::Bool(
                    matches!(first, Value::Number(n) if *n == 0.0 && n.is_sign_negative()),
                ))
            }
            "formatIdentityFreeValue" | "formatSimpleValue" => {
                let value = match first {
                    Value::Number(n) if *n == 0.0 && n.is_sign_negative() => {
                        Value::String("-0".into())
                    }
                    Value::String(string) => {
                        let mut quoted = JsString::from("\"");
                        native::append(&mut quoted, string, self.config.max_string_bytes)?;
                        native::append(&mut quoted, &"\"".into(), self.config.max_string_bytes)?;
                        Value::String(quoted)
                    }
                    Value::Object(_) | Value::Symbol(_) if name == "formatIdentityFreeValue" => {
                        Value::Undefined
                    }
                    Value::Symbol(symbol) => Value::String(symbol.descriptive_string()),
                    _ => match self.coerce_string(first) {
                        Ok(string) => Value::String(string),
                        Err(RuntimeError::TypeError(_)) => self.native_call(
                            NativeFunction::ObjectToString,
                            first.clone(),
                            vec![],
                            false,
                        )?,
                        Err(error) => return Err(error),
                    },
                };
                return Ok(value);
            }
            "formatArray" => {
                let length = self.get_property(first, &"length".into())?;
                let length = self.coerce_length(&length)? as u64;
                let mut result = JsString::from("[");
                for index in 0..length {
                    self.charge_step()?;
                    if index > 0 {
                        native::append(&mut result, &", ".into(), self.config.max_string_bytes)?;
                    }
                    let value = self.get_property(first, &index.to_string().into())?;
                    let Value::String(string) = self.native_call(
                        NativeFunction::String,
                        Value::Undefined,
                        vec![value],
                        false,
                    )?
                    else {
                        unreachable!()
                    };
                    native::append(&mut result, &string, self.config.max_string_bytes)?;
                }
                native::append(&mut result, &"]".into(), self.config.max_string_bytes)?;
                return Ok(Value::String(result));
            }
            "assert" => *first == Value::Bool(true),
            "sameValue" | "notSameValue" | "_isSameValue" => {
                let same = crate::heap::same_value(first, second);
                if name == "_isSameValue" {
                    return Ok(Value::Bool(same));
                }
                same == (name == "sameValue")
            }
            "throws" => {
                if !self.is_callable(second)? {
                    return Err(self.test262_failure(name));
                }
                let error = self.call_native(second.clone(), Value::Undefined, vec![], false);
                let constructor = match error {
                    Err(RuntimeError::TypeError(_)) => self.error_global("TypeError")?,
                    Err(RuntimeError::RangeError(_)) => self.error_global("RangeError")?,
                    Err(RuntimeError::ReferenceError(_)) => self.error_global("ReferenceError")?,
                    Err(RuntimeError::SyntaxError(_)) => self.error_global("SyntaxError")?,
                    Err(RuntimeError::Test262(_)) => self.error_global("Test262Error")?,
                    Err(RuntimeError::Thrown(value @ Value::Object(_))) => {
                        self.get_property(&value, &"constructor".into())?
                    }
                    Ok(_) | Err(RuntimeError::Thrown(_)) => return Err(self.test262_failure(name)),
                    // Host resource failures must never satisfy assert.throws.
                    Err(error) => return Err(error),
                };
                constructor == *first
            }
            "compareArray" | "arrayEqual" => {
                if name == "compareArray"
                    && (!matches!(first, Value::Object(_)) || !matches!(second, Value::Object(_)))
                {
                    return Err(self.test262_failure(name));
                }
                let left = self.get_property(first, &"length".into())?;
                let right = self.get_property(second, &"length".into())?;
                if left != right {
                    return if name == "arrayEqual" {
                        Ok(Value::Bool(false))
                    } else {
                        Err(self.test262_failure(name))
                    };
                }
                let length = self.coerce_length(&left)? as u64;
                for index in 0..length {
                    self.charge_step()?;
                    let key = index.to_string().into();
                    let left = self.get_property(first, &key)?;
                    self.stack.push(left.clone());
                    let right = self.get_property(second, &key)?;
                    if !crate::heap::same_value(&left, &right) {
                        return if name == "arrayEqual" {
                            Ok(Value::Bool(false))
                        } else {
                            Err(self.test262_failure(name))
                        };
                    }
                }
                if name == "arrayEqual" {
                    return Ok(Value::Bool(true));
                }
                true
            }
            _ => false,
        };
        if passed {
            Ok(Value::Undefined)
        } else {
            Err(self.test262_failure(name))
        }
    }

    fn test262_property_helper(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let target = native::argument(args, 0);
        let key = native::argument(args, 1);
        match name {
            "verifyProperty" | "verifyPrimordialProperty" => {
                self.test262_verify_property(target, key, native::argument(args, 2))
            }
            "verifyCallableProperty" | "verifyPrimordialCallableProperty" => {
                self.test262_verify_callable_property(args)
            }
            "verifyAccessorProperty" | "verifyPrimordialAccessorProperty" => {
                self.test262_verify_accessor_property(target, key, native::argument(args, 2))
            }
            "verifyEqualTo" => {
                let expected = native::argument(args, 2);
                let key = self.coerce_property_key(key)?;
                let actual = self.get_property(target, &key)?;
                if crate::heap::same_value(&actual, expected) {
                    Ok(Value::Undefined)
                } else {
                    Err(self.test262_failure(name))
                }
            }
            _ => {
                let (_, descriptor) = self.test262_own_descriptor(target, key)?;
                let Some(descriptor) = descriptor else {
                    return Err(self.test262_failure(name));
                };
                let actual = if matches!(name, "verifyWritable" | "verifyNotWritable") {
                    descriptor.writable
                } else if matches!(name, "verifyEnumerable" | "verifyNotEnumerable") {
                    descriptor.enumerable
                } else {
                    descriptor.configurable
                };
                let expected = !matches!(
                    name,
                    "verifyNotWritable" | "verifyNotEnumerable" | "verifyNotConfigurable"
                );
                if actual == Some(expected) {
                    Ok(Value::Undefined)
                } else {
                    Err(self.test262_failure(name))
                }
            }
        }
    }

    fn test262_verify_property(
        &mut self,
        target: &Value,
        key: &Value,
        expected: &Value,
    ) -> Result<Value, RuntimeError> {
        let (_, actual) = self.test262_own_descriptor(target, key)?;
        if *expected == Value::Undefined {
            return if actual.is_none() {
                Ok(Value::Bool(true))
            } else {
                Err(self.test262_failure("verifyProperty"))
            };
        }
        let Some(actual) = actual else {
            return Err(self.test262_failure("verifyProperty"));
        };
        self.test262_compare_descriptor(&actual, expected)?;
        Ok(Value::Bool(true))
    }

    fn test262_verify_callable_property(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let target = native::argument(args, 0);
        let key = native::argument(args, 1);
        let expected_name = native::argument(args, 2);
        let expected_length = native::argument(args, 3);
        let expected_descriptor = native::argument(args, 4);
        let (property, actual) = self.test262_own_descriptor(target, key)?;
        let Some(actual) = actual else {
            return Err(self.test262_failure("verifyCallableProperty"));
        };
        let Some(value) = actual.value.clone() else {
            return Err(self.test262_failure("verifyCallableProperty"));
        };
        if !self.is_callable(&value)? {
            return Err(self.test262_failure("verifyCallableProperty"));
        }
        if *expected_descriptor == Value::Undefined {
            if actual.writable != Some(true)
                || actual.enumerable != Some(false)
                || actual.configurable != Some(true)
            {
                return Err(self.test262_failure("verifyCallableProperty"));
            }
        } else {
            self.test262_compare_descriptor(&actual, expected_descriptor)?;
        }
        let expected_name = if *expected_name == Value::Undefined {
            match property {
                PropertyName::String(name) => Value::String(name),
                PropertyName::Symbol(symbol) => {
                    let mut name = JsString::from("[");
                    native::append(
                        &mut name,
                        &symbol.description.unwrap_or_default(),
                        self.config.max_string_bytes,
                    )?;
                    native::append(&mut name, &"]".into(), self.config.max_string_bytes)?;
                    Value::String(name)
                }
            }
        } else {
            expected_name.clone()
        };
        self.test262_compare_function_property(
            &value,
            "name",
            &expected_name,
            expected_descriptor,
        )?;
        self.test262_compare_function_property(
            &value,
            "length",
            expected_length,
            expected_descriptor,
        )?;
        Ok(Value::Bool(true))
    }

    fn test262_verify_accessor_property(
        &mut self,
        target: &Value,
        key: &Value,
        expected: &Value,
    ) -> Result<Value, RuntimeError> {
        let (_, actual) = self.test262_own_descriptor(target, key)?;
        let Some(actual) = actual else {
            return Err(self.test262_failure("verifyAccessorProperty"));
        };
        if !actual.accessor() {
            return Err(self.test262_failure("verifyAccessorProperty"));
        }
        let Value::Object(expected_id) = expected else {
            return Err(self.test262_failure("verifyAccessorProperty"));
        };
        for field in ["get", "set"] {
            if let Some(want) = self.heap.get_own(*expected_id, field)? {
                let got = if field == "get" {
                    actual.get.clone().unwrap_or(Value::Undefined)
                } else {
                    actual.set.clone().unwrap_or(Value::Undefined)
                };
                if !crate::heap::same_value(&got, &want) {
                    return Err(self.test262_failure("verifyAccessorProperty"));
                }
            }
        }
        for (field, actual, fallback) in [
            ("enumerable", actual.enumerable, false),
            ("configurable", actual.configurable, true),
        ] {
            let expected = self
                .heap
                .get_own(*expected_id, field)?
                .unwrap_or(Value::Bool(fallback));
            if expected != Value::Undefined
                && actual.map(Value::Bool).unwrap_or(Value::Undefined) != expected
            {
                return Err(self.test262_failure("verifyAccessorProperty"));
            }
        }
        Ok(Value::Bool(true))
    }

    fn test262_own_descriptor(
        &mut self,
        target: &Value,
        key: &Value,
    ) -> Result<(PropertyName, Option<PropertyDescriptor>), RuntimeError> {
        let object = target
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("property helper requires an object".into()))?;
        let key = self.coerce_property_key(key)?;
        Ok((
            key.clone(),
            self.heap.get_own_property_descriptor(object, key)?,
        ))
    }

    fn test262_compare_descriptor(
        &mut self,
        actual: &PropertyDescriptor,
        expected: &Value,
    ) -> Result<(), RuntimeError> {
        let Value::Object(expected_id) = expected else {
            return Err(self.test262_failure("verifyProperty"));
        };
        for field in self.heap.own_property_keys(*expected_id)? {
            if !matches!(field, PropertyName::String(_))
                || !(field == "value"
                    || field == "writable"
                    || field == "get"
                    || field == "set"
                    || field == "enumerable"
                    || field == "configurable")
            {
                return Err(self.test262_failure("verifyProperty"));
            }
            let expected = self
                .heap
                .get_own(*expected_id, &field)?
                .expect("own property key has a value");
            let observed = if field == "value" {
                actual.value.clone().unwrap_or(Value::Undefined)
            } else if field == "writable" {
                actual.writable.map(Value::Bool).unwrap_or(Value::Undefined)
            } else if field == "get" {
                actual.get.clone().unwrap_or(Value::Undefined)
            } else if field == "set" {
                actual.set.clone().unwrap_or(Value::Undefined)
            } else if field == "enumerable" {
                actual
                    .enumerable
                    .map(Value::Bool)
                    .unwrap_or(Value::Undefined)
            } else {
                actual
                    .configurable
                    .map(Value::Bool)
                    .unwrap_or(Value::Undefined)
            };
            if !crate::heap::same_value(&observed, &expected) {
                return Err(self.test262_failure("verifyProperty"));
            }
        }
        Ok(())
    }

    fn test262_compare_function_property(
        &mut self,
        function: &Value,
        property: &str,
        expected: &Value,
        descriptor: &Value,
    ) -> Result<(), RuntimeError> {
        let object = function.object_id().expect("callable values are objects");
        let Some(actual) = self.heap.get_own_property_descriptor(object, property)? else {
            return Err(self.test262_failure("verifyCallableProperty"));
        };
        if actual.value.as_ref() != Some(expected)
            || actual.writable != Some(false)
            || actual.enumerable != Some(false)
        {
            return Err(self.test262_failure("verifyCallableProperty"));
        }
        if let Value::Object(descriptor) = descriptor {
            if let Some(configurable) = self.heap.get_own(*descriptor, "configurable")? {
                if configurable != Value::Undefined
                    && actual.configurable.map(Value::Bool) != Some(configurable)
                {
                    return Err(self.test262_failure("verifyCallableProperty"));
                }
            }
        } else if actual.configurable != Some(true) {
            return Err(self.test262_failure("verifyCallableProperty"));
        }
        Ok(())
    }

    fn test262_failure(&self, name: &str) -> RuntimeError {
        RuntimeError::Test262(format!("{name} failed"))
    }
}
