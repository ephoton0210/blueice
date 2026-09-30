// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native counterparts of Test262's `assert.js`. Spending one interpreter
//! dispatch per assertion instead of dozens is what lets the large loop-driven
//! fixtures fit the ordinary instruction budget, so these stay native; what
//! they must not do is differ from the upstream source. Each helper below
//! follows the corresponding JavaScript step for step: the same message text,
//! the same coercions and property reads in the same order, and the same
//! exceptions. `harness.rs` in the crate's `test262_host` tests holds the
//! differential check that runs the upstream `assert.js` beside these.

use super::*;
use std::cmp::Ordering;

impl Vm {
    /// Dispatches the natives that replace `assert.js`; `None` means `name`
    /// belongs to another Test262 native.
    pub(in super::super) fn test262_assertion_call(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Option<Result<Value, RuntimeError>> {
        if !matches!(
            name,
            "assert"
                | "sameValue"
                | "notSameValue"
                | "_isSameValue"
                | "throws"
                | "compareArray"
                | "arrayEqual"
                | "formatArray"
                | "isPrimitive"
                | "isNegativeZero"
                | "formatIdentityFreeValue"
                | "formatSimpleValue"
        ) {
            return None;
        }
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        // Every value a helper holds across a call that can run user code
        // must stay reachable, so root the arguments for the whole call.
        let base = self.stack.len();
        self.stack.extend(args.iter().cloned());
        let result = match name {
            "assert" => self.test262_assert(first, second),
            "sameValue" => self.test262_assert_same_value(args, true),
            "notSameValue" => self.test262_assert_same_value(args, false),
            "_isSameValue" => Ok(Value::Bool(crate::heap::same_value(first, second))),
            "throws" => self.test262_assert_throws(args),
            "compareArray" => self.test262_assert_compare_array(args),
            "arrayEqual" => self
                .test262_compare_array_values(first, second)
                .map(Value::Bool),
            "formatArray" => self.test262_format_array(first),
            "isPrimitive" => self.test262_is_primitive(first).map(Value::Bool),
            "isNegativeZero" => Ok(Value::Bool(
                matches!(first, Value::Number(n) if *n == 0.0 && n.is_sign_negative()),
            )),
            "formatIdentityFreeValue" => self.test262_format_identity_free_value(first),
            "formatSimpleValue" => self.test262_format_simple_value(first),
            _ => unreachable!("the name filter above admits only the arms listed here"),
        };
        self.stack.truncate(base);
        Some(result)
    }

    /// `new Test262Error(message)` as `sta.js` builds it: an empty message
    /// when `message` is falsy, otherwise `message` itself, unconverted. A
    /// well-formed string message travels as `RuntimeError::Test262`, which is
    /// how an uncaught failure is reported; any other message needs a real
    /// error object so it keeps its type and value.
    fn test262_error(&mut self, message: &Value) -> RuntimeError {
        let message = match self.to_boolean(message) {
            Ok(true) => message.clone(),
            Ok(false) => Value::String(JsString::default()),
            Err(error) => return error,
        };
        if let Value::String(text) = &message {
            if let Ok(text) = text.to_utf8() {
                return RuntimeError::Test262(text);
            }
        }
        match self.test262_error_object(&message) {
            Ok(error) => RuntimeError::Thrown(error),
            Err(error) => error,
        }
    }

    fn test262_error_object(&mut self, message: &Value) -> Result<Value, RuntimeError> {
        let constructor = self.error_global("Test262Error")?;
        let error = self.call_native(constructor, Value::Undefined, vec![], false)?;
        let object = error.object_id().expect("Test262Error builds an object");
        self.stack.push(error.clone());
        self.define_data(object, "message", message.clone(), true, false, true)?;
        Ok(error)
    }

    /// Resolves an identifier the way script code names a global, so a test
    /// that has replaced or removed `String`, `JSON`, `Array` or `Object` sees
    /// the same effect here as it would in the JavaScript helper.
    fn test262_global_value(&mut self, name: &str) -> Result<Value, RuntimeError> {
        self.lookup_global_name(name)?
            .ok_or_else(|| RuntimeError::ReferenceError(name.into()))
    }

    /// `String(value)`.
    fn test262_string(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let string = self.test262_global_value("String")?;
        self.call_native(string, Value::Undefined, vec![value.clone()], false)
    }

    /// `left + right` for a text prefix and any value.
    fn test262_concat(&mut self, left: &str, right: Value) -> Result<Value, RuntimeError> {
        self.add(Value::String(left.into()), right)
    }

    fn test262_is_primitive(&mut self, value: &Value) -> Result<bool, RuntimeError> {
        // `!value || (typeof value !== 'object' && typeof value !== 'function')`
        if !self.to_boolean(value)? {
            return Ok(true);
        }
        Ok(!matches!(self.typeof_value(value)?, "object" | "function"))
    }

    fn test262_format_identity_free_value(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let kind = if *value == Value::Null {
            "null"
        } else {
            self.typeof_value(value)?
        };
        match kind {
            "string" => {
                let json = self.lookup_global_name("JSON")?;
                match json {
                    Some(json) if self.typeof_value(&json)? != "undefined" => {
                        let stringify = self.get_property(&json, &"stringify".into())?;
                        self.call_native(stringify, json, vec![value.clone()], false)
                    }
                    _ => {
                        let quoted = self.test262_concat("\"", value.clone())?;
                        self.add(quoted, Value::String("\"".into()))
                    }
                }
            }
            "bigint" => {
                let text = self.test262_string(value)?;
                self.add(text, Value::String("n".into()))
            }
            "number" | "boolean" | "undefined" | "null" => {
                if matches!(value, Value::Number(n) if *n == 0.0 && n.is_sign_negative()) {
                    return Ok(Value::String("-0".into()));
                }
                self.test262_string(value)
            }
            _ => Ok(Value::Undefined),
        }
    }

    fn test262_format_simple_value(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let basic = self.test262_format_identity_free_value(value)?;
        if self.to_boolean(&basic)? {
            return Ok(basic);
        }
        let error = match self.test262_string(value) {
            Ok(text) => return Ok(text),
            Err(error) => error,
        };
        // `catch (err) { if (err.name === 'TypeError') ... throw err; }`; an
        // error that is not a JavaScript exception (a resource limit) is not
        // something a `catch` sees, so it propagates untouched.
        let thrown = self.error_value(error)?;
        self.stack.push(thrown.clone());
        let name = self.get_property(&thrown, &"name".into())?;
        if name != Value::String("TypeError".into()) {
            return Err(RuntimeError::Thrown(thrown));
        }
        let object = self.test262_global_value("Object")?;
        let prototype = self.get_property(&object, &"prototype".into())?;
        let to_string = self.get_property(&prototype, &"toString".into())?;
        self.call_native(to_string, value.clone(), vec![], false)
    }

    fn test262_assert(
        &mut self,
        condition: &Value,
        message: &Value,
    ) -> Result<Value, RuntimeError> {
        if *condition == Value::Bool(true) {
            return Ok(Value::Undefined);
        }
        let message = if *message == Value::Undefined {
            let shown = self.test262_format_simple_value(condition)?;
            self.test262_concat("Expected true but got ", shown)?
        } else {
            message.clone()
        };
        Err(self.test262_error(&message))
    }

    /// `assert.sameValue` (`expect_same`) and `assert.notSameValue`.
    fn test262_assert_same_value(
        &mut self,
        args: &[Value],
        expect_same: bool,
    ) -> Result<Value, RuntimeError> {
        let actual = native::argument(args, 0);
        let other = native::argument(args, 1);
        let message = native::argument(args, 2);
        if crate::heap::same_value(actual, other) == expect_same {
            return Ok(Value::Undefined);
        }
        let message = if *message == Value::Undefined {
            Value::String(JsString::default())
        } else {
            self.add(message.clone(), Value::String(" ".into()))?
        };
        let shown_actual = self.test262_format_simple_value(actual)?;
        let shown_other = self.test262_format_simple_value(other)?;
        let detail = self.test262_concat("Expected SameValue(«", shown_actual)?;
        let detail = self.add(detail, Value::String("», «".into()))?;
        let detail = self.add(detail, shown_other)?;
        let verdict = if expect_same {
            "») to be true"
        } else {
            "») to be false"
        };
        let detail = self.add(detail, Value::String(verdict.into()))?;
        let message = self.add(message, detail)?;
        Err(self.test262_error(&message))
    }

    fn test262_assert_throws(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let expected = native::argument(args, 0);
        let func = native::argument(args, 1);
        let message = native::argument(args, 2);
        if self.typeof_value(func)? != "function" {
            return Err(self.test262_error(&Value::String(
                "assert.throws requires two arguments: the error constructor and a function to run"
                    .into(),
            )));
        }
        let message = if *message == Value::Undefined {
            Value::String(JsString::default())
        } else {
            self.add(message.clone(), Value::String(" ".into()))?
        };
        let error = match self.call_native(func.clone(), Value::Undefined, vec![], false) {
            Ok(_) => {
                let name = self.get_property(expected, &"name".into())?;
                let detail = self.test262_concat("Expected a ", name)?;
                let detail = self.add(
                    detail,
                    Value::String(" to be thrown but no exception was thrown at all".into()),
                )?;
                let message = self.add(message, detail)?;
                return Err(self.test262_error(&message));
            }
            Err(error) => error,
        };
        // Only an exception a `catch` could observe is inspected; anything
        // else (an exhausted budget, say) must never satisfy the assertion.
        let thrown = self.error_value(error)?;
        self.stack.push(thrown.clone());
        if self.typeof_value(&thrown)? != "object" || thrown == Value::Null {
            let message = self.add(
                message,
                Value::String("Thrown value was not an object!".into()),
            )?;
            return Err(self.test262_error(&message));
        }
        let constructor = self.get_property(&thrown, &"constructor".into())?;
        self.stack.push(constructor.clone());
        if constructor == *expected {
            return Ok(Value::Undefined);
        }
        let expected_name = self.get_property(expected, &"name".into())?;
        let actual_name = self.get_property(&constructor, &"name".into())?;
        let detail = self.test262_concat("Expected a ", expected_name.clone())?;
        let detail = if expected_name == actual_name {
            self.add(
                detail,
                Value::String(" but got a different error constructor with the same name".into()),
            )?
        } else {
            let detail = self.add(detail, Value::String(" but got a ".into()))?;
            self.add(detail, actual_name)?
        };
        let message = self.add(message, detail)?;
        Err(self.test262_error(&message))
    }

    /// The global `compareArray(a, b)`: equal `length`s (by `!==`, so no
    /// coercion) and pairwise `SameValue` elements, reading `b.length`,
    /// `a.length` and then, per element, `a.length` again, `b[i]` and `a[i]`.
    pub(in super::super) fn test262_compare_array_values(
        &mut self,
        first: &Value,
        second: &Value,
    ) -> Result<bool, RuntimeError> {
        let second_length = self.get_property(second, &"length".into())?;
        let first_length = self.get_property(first, &"length".into())?;
        if second_length != first_length {
            return Ok(false);
        }
        let mut index = 0u64;
        loop {
            self.charge_step()?;
            let length = self.get_property(first, &"length".into())?;
            let bound = self.coerce_primitive(&length, "number")?;
            let below = crate::primitive::compare(&Value::Number(index as f64), &bound)?
                .is_some_and(|order| order == Ordering::Less);
            if !below {
                return Ok(true);
            }
            let key: PropertyName = index.to_string().into();
            let expected = self.get_property(second, &key)?;
            self.stack.push(expected.clone());
            let actual = self.get_property(first, &key)?;
            if !crate::heap::same_value(&expected, &actual) {
                return Ok(false);
            }
            index += 1;
        }
    }

    /// `compareArray.format`: `"[" + Array.prototype.map.call(arrayLike,
    /// String).join(", ") + "]"`, including how `map` treats holes.
    fn test262_format_array(&mut self, array_like: &Value) -> Result<Value, RuntimeError> {
        let array = self.test262_global_value("Array")?;
        let prototype = self.get_property(&array, &"prototype".into())?;
        let map = self.get_property(&prototype, &"map".into())?;
        let string = self.test262_global_value("String")?;
        let mapped = self.call_native(map, array_like.clone(), vec![string], false)?;
        self.stack.push(mapped.clone());
        let join = self.get_property(&mapped, &"join".into())?;
        let joined = self.call_native(join, mapped, vec![Value::String(", ".into())], false)?;
        let text = self.test262_concat("[", joined)?;
        self.add(text, Value::String("]".into()))
    }

    /// `assert.compareArray(actual, expected, message)`.
    fn test262_assert_compare_array(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let actual = native::argument(args, 0);
        let expected = native::argument(args, 1);
        let mut message = if *native::argument(args, 2) == Value::Undefined {
            Value::String(JsString::default())
        } else {
            native::argument(args, 2).clone()
        };
        if self.typeof_value(&message)? == "symbol" {
            let to_string = self.get_property(&message, &"toString".into())?;
            message = self.call_native(to_string, message.clone(), vec![], false)?;
        }
        for (label, value) in [("Actual", actual), ("Expected", expected)] {
            if self.test262_is_primitive(value)? {
                let text = self.test262_concat(&format!("{label} argument ["), value.clone())?;
                let text = self.add(text, Value::String("] shouldn't be primitive. ".into()))?;
                let message = self.test262_string(&message)?;
                let text = self.add(text, message)?;
                return Err(self.test262_error(&text));
            }
        }
        if self.test262_compare_array_values(actual, expected)? {
            return Ok(Value::Undefined);
        }
        let compare = self.test262_global_value("compareArray")?;
        let format = self.get_property(&compare, &"format".into())?;
        let shown_actual = self.call_native(
            format.clone(),
            Value::Undefined,
            vec![actual.clone()],
            false,
        )?;
        let shown_expected =
            self.call_native(format, Value::Undefined, vec![expected.clone()], false)?;
        let text = self.test262_concat("Actual ", shown_actual)?;
        let text = self.add(text, Value::String(" and expected ".into()))?;
        let text = self.add(text, shown_expected)?;
        let text = self.add(
            text,
            Value::String(" should have the same contents. ".into()),
        )?;
        let message = self.test262_string(&message)?;
        let text = self.add(text, message)?;
        Err(self.test262_error(&text))
    }
}
