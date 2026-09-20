// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn test262_property_helper(
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
            "verifyWritable" | "verifyNotWritable" => {
                let verify_property = native::argument(args, 2);
                // The deprecated Test262 helpers still perform an observable
                // assignment after their descriptor check. In particular,
                // `verifyNotWritable(object, key, alternate)` is valid when
                // `key` is absent: it verifies that an attempted addition
                // cannot change `alternate`. Merely inspecting an own
                // descriptor therefore both rejects valid tests and misses
                // receiver/setter behavior.
                if !self.to_boolean(verify_property)? {
                    let (_, descriptor) = self.test262_own_descriptor(target, key)?;
                    let Some(descriptor) = descriptor else {
                        return Err(self.test262_failure(name));
                    };
                    let expected = name == "verifyWritable";
                    if descriptor.writable.unwrap_or(false) != expected {
                        return Err(self.test262_failure(name));
                    }
                }
                let writable = self.test262_is_writable(target, key, verify_property, args)?;
                if writable == (name == "verifyWritable") {
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
                let actual = if matches!(name, "verifyEnumerable" | "verifyNotEnumerable") {
                    descriptor.enumerable
                } else {
                    descriptor.configurable
                };
                let expected = !matches!(name, "verifyNotEnumerable" | "verifyNotConfigurable");
                if actual == Some(expected) {
                    Ok(Value::Undefined)
                } else {
                    Err(self.test262_failure(name))
                }
            }
        }
    }

    /// Native equivalent of the deprecated `propertyHelper.js` `isWritable`.
    ///
    /// The Test262 runner substitutes this helper before executing test code,
    /// so it must retain the helper's observable write, read, and restoration
    /// steps instead of treating a data descriptor as the complete answer.
    pub(in super::super) fn test262_is_writable(
        &mut self,
        target: &Value,
        key_value: &Value,
        verify_property: &Value,
        args: &[Value],
    ) -> Result<bool, RuntimeError> {
        let base = self.stack.len();
        self.stack
            .extend([target.clone(), key_value.clone(), verify_property.clone()]);
        let result = (|| {
            let object = self.coerce_object(target)?;
            self.stack.push(Value::Object(object));
            let key = self.coerce_property_key(key_value)?;
            let verify_key = if self.to_boolean(verify_property)? {
                self.coerce_property_key(verify_property)?
            } else {
                key.clone()
            };
            let array_length = self.heap.is_array(object)? && key == "length";
            let supplied = args.get(3).cloned().unwrap_or(Value::Undefined);
            let mut new_value = if self.to_boolean(&supplied)? {
                supplied
            } else if array_length {
                Value::Number(f64::from(u32::MAX))
            } else {
                Value::String("unlikelyValue".into())
            };
            let had_value = self.object_get_own_property(object, &key)?.is_some();
            let old_value = self.get_property(target, &key)?;
            if args.len() < 4 && crate::heap::same_value(&new_value, &old_value) {
                let mut string = self.coerce_string(&new_value)?;
                native::append(&mut string, &"2".into(), self.config.max_string_bytes)?;
                new_value = Value::String(string);
            }
            self.stack.extend([old_value.clone(), new_value.clone()]);
            match self.set_property(target, &key, &new_value) {
                Ok(()) | Err(RuntimeError::TypeError(_)) => {}
                Err(_) => return Err(self.test262_failure("verifyWritable")),
            }
            let observed = self.get_property(target, &verify_key)?;
            let writable = crate::heap::same_value(&observed, &new_value);
            if writable {
                if had_value {
                    self.set_property(target, &key, &old_value)?;
                } else {
                    self.object_delete(object, &key)?;
                }
            }
            Ok(writable)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn test262_verify_property(
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

    pub(in super::super) fn test262_verify_callable_property(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
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

    pub(in super::super) fn test262_verify_accessor_property(
        &mut self,
        target: &Value,
        key: &Value,
        expected: &Value,
    ) -> Result<Value, RuntimeError> {
        let (property, actual) = self.test262_own_descriptor(target, key)?;
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
                if matches!(want, Value::Undefined) || self.is_callable(&want)? {
                    if !crate::heap::same_value(&got, &want) {
                        return Err(self.test262_failure("verifyAccessorProperty"));
                    }
                } else {
                    self.test262_verify_accessor_function(&property, field, &got, &want)?;
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

    /// The `{ name?, length? }` form of `verifyAccessorProperty`'s `get`/`set`
    /// expectation: the accessor must be a function whose configurable,
    /// non-writable, non-enumerable `name`/`length` follow the built-in
    /// accessor conventions (`"get "`/`"set "` plus the property key, and
    /// length 0/1), unless the expectation overrides either.
    fn test262_verify_accessor_function(
        &mut self,
        property: &PropertyName,
        field: &str,
        function: &Value,
        expected: &Value,
    ) -> Result<(), RuntimeError> {
        if !self.is_callable(function)? {
            return Err(self.test262_failure("verifyAccessorProperty"));
        }
        let name = self.get_property(expected, &"name".into())?;
        let name = if name == Value::Undefined {
            let maximum = self.config.max_string_bytes;
            let mut name = JsString::from(if field == "get" { "get " } else { "set " });
            match property {
                PropertyName::String(key) => native::append(&mut name, key, maximum)?,
                PropertyName::Symbol(symbol) => {
                    native::append(&mut name, &"[".into(), maximum)?;
                    native::append(
                        &mut name,
                        &symbol.description.clone().unwrap_or_default(),
                        maximum,
                    )?;
                    native::append(&mut name, &"]".into(), maximum)?;
                }
            }
            Value::String(name)
        } else {
            name
        };
        let length = self.get_property(expected, &"length".into())?;
        let length = if length == Value::Undefined {
            Value::Number(if field == "get" { 0.0 } else { 1.0 })
        } else {
            length
        };
        self.test262_compare_function_property(function, "name", &name, &Value::Undefined)?;
        self.test262_compare_function_property(function, "length", &length, &Value::Undefined)
    }

    pub(in super::super) fn test262_own_descriptor(
        &mut self,
        target: &Value,
        key: &Value,
    ) -> Result<(PropertyName, Option<PropertyDescriptor>), RuntimeError> {
        let object = target
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("property helper requires an object".into()))?;
        let key = self.coerce_property_key(key)?;
        let descriptor = self.object_get_own_property(object, &key)?;
        Ok((key, descriptor))
    }

    pub(in super::super) fn test262_compare_descriptor(
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

    pub(in super::super) fn test262_compare_function_property(
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

    pub(in super::super) fn test262_failure(&self, name: &str) -> RuntimeError {
        RuntimeError::Test262(format!("{name} failed"))
    }
}
