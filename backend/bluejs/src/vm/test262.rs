// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit host setup, never installed in an ordinary realm. Test262 permits
//! overriding harness functions; failures remain distinct from engine errors.
use super::*;

impl Vm {
    /// Installs native Test262 assertion functions in this realm. Additional
    /// harness includes and asynchronous/module hosts are the runner's job.
    pub fn install_test262_harness(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let result = self.install_test262_functions();
        self.stack.truncate(base);
        result
    }

    fn install_test262_functions(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        let string = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(string)?.unwrap();
        self.install_native(global, prototype, "assert", 1, NativeFunction::Test262("assert"))?;
        let assert = self.heap.get(global, "assert")?.object_id().unwrap();
        for (name, length) in [("sameValue", 2), ("notSameValue", 2), ("_isSameValue", 2), ("throws", 2), ("compareArray", 2)] {
            self.install_native(assert, prototype, name, length, NativeFunction::Test262(name))?;
        }
        for name in ["isPrimitive", "isNegativeZero", "formatIdentityFreeValue", "formatSimpleValue", "compareArray"] {
            self.install_native(
                global,
                prototype,
                name,
                if name == "compareArray" { 2 } else { 1 },
                NativeFunction::Test262(if name == "compareArray" { "arrayEqual" } else { name }),
            )?;
        }
        for (property, global_name) in [("_formatIdentityFreeValue", "formatIdentityFreeValue"), ("_toString", "formatSimpleValue")] {
            let value = self.heap.get(global, global_name)?;
            self.define_data(assert, property, value, true, true, true)?;
        }
        let compare = self.heap.get(global, "compareArray")?.object_id().unwrap();
        self.install_native(compare, prototype, "format", 1, NativeFunction::Test262("formatArray"))?;
        self.install_native(global, prototype, "$DONOTEVALUATE", 0, NativeFunction::Test262("$DONOTEVALUATE"))?;
        let error = self.error_global("Test262Error")?.object_id().unwrap();
        self.define_data(global, "Test262Error", Value::Object(error), true, false, true)?;
        self.install_native(error, prototype, "thrower", 1, NativeFunction::Test262("thrower"))?;
        Ok(())
    }

    pub(super) fn test262_call(&mut self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        let fail = || RuntimeError::Test262(format!("{name} failed"));
        let passed = match name {
            "isPrimitive" => return Ok(Value::Bool(!matches!(first, Value::Object(_)))),
            "isNegativeZero" => return Ok(Value::Bool(matches!(first, Value::Number(n) if *n == 0.0 && n.is_sign_negative()))),
            "formatIdentityFreeValue" | "formatSimpleValue" => {
                let value = match first {
                    Value::Number(n) if *n == 0.0 && n.is_sign_negative() => Value::String("-0".into()),
                    Value::String(string) => {
                        let mut quoted = JsString::from("\"");
                        native::append(&mut quoted, string, self.config.max_string_bytes)?;
                        native::append(&mut quoted, &"\"".into(), self.config.max_string_bytes)?;
                        Value::String(quoted)
                    }
                    Value::Object(_) | Value::Symbol(_) if name == "formatIdentityFreeValue" => Value::Undefined,
                    Value::Symbol(symbol) => Value::String(symbol.descriptive_string()),
                    _ => match self.coerce_string(first) {
                        Ok(string) => Value::String(string),
                        Err(RuntimeError::TypeError(_)) => self.native_call(NativeFunction::ObjectToString, first.clone(), vec![], false)?,
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
                    let Value::String(string) = self.native_call(NativeFunction::String, Value::Undefined, vec![value], false)? else { unreachable!() };
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
                    return Err(fail());
                }
                let error = self.call_native(second.clone(), Value::Undefined, vec![], false);
                let constructor = match error {
                    Err(RuntimeError::TypeError(_)) => self.error_global("TypeError")?,
                    Err(RuntimeError::RangeError(_)) => self.error_global("RangeError")?,
                    Err(RuntimeError::ReferenceError(_)) => self.error_global("ReferenceError")?,
                    Err(RuntimeError::SyntaxError(_)) => self.error_global("SyntaxError")?,
                    Err(RuntimeError::Test262(_)) => self.error_global("Test262Error")?,
                    Err(RuntimeError::Thrown(value @ Value::Object(_))) => self.get_property(&value, &"constructor".into())?,
                    Ok(_) | Err(RuntimeError::Thrown(_)) => return Err(fail()),
                    // Host resource failures must never satisfy assert.throws.
                    Err(error) => return Err(error),
                };
                constructor == *first
            }
            "compareArray" | "arrayEqual" => {
                if name == "compareArray" && (!matches!(first, Value::Object(_)) || !matches!(second, Value::Object(_))) {
                    return Err(fail());
                }
                let left = self.get_property(first, &"length".into())?;
                let right = self.get_property(second, &"length".into())?;
                if left != right {
                    return if name == "arrayEqual" { Ok(Value::Bool(false)) } else { Err(fail()) };
                }
                let length = self.coerce_length(&left)? as u64;
                for index in 0..length {
                    self.charge_step()?;
                    let key = index.to_string().into();
                    let left = self.get_property(first, &key)?;
                    self.stack.push(left.clone());
                    let right = self.get_property(second, &key)?;
                    if !crate::heap::same_value(&left, &right) {
                        return if name == "arrayEqual" { Ok(Value::Bool(false)) } else { Err(fail()) };
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
            Err(fail())
        }
    }
}
