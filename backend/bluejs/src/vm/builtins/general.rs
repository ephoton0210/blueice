// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub fn install_test262_done(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        let prototype = self.function_prototype()?;
        self.install_native(global, prototype, "$DONE", 1, NativeFunction::Test262Done)
    }

    pub fn take_test262_done(&mut self) -> Option<Result<(), Value>> {
        self.test262_done.take()
    }

    pub(in super::super) fn is_callable(&self, value: &Value) -> Result<bool, RuntimeError> {
        Ok(if let Value::Object(id) = value {
            if let Some((_, _, callable, _)) = self.test262_foreign_reference(*id) {
                callable
            } else if self.test262_imported_callables.contains(id) {
                // `$262.createRealm()` transports an object owned by the
                // caller as an opaque local stand-in.  Its owner retains the
                // forwarding record, while this realm retains the callable
                // bit so `ShadowRealm` can create its own wrapper around it.
                true
            } else if let Some((callable, _)) = self.heap.proxy_capabilities(*id)? {
                callable
            } else {
                self.heap.native_function(*id)?.is_some()
                    || self.heap.closure(*id)?.is_some()
                    || self.heap.bound_function(*id)?.is_some()
            }
        } else {
            false
        })
    }

    pub(in super::super) fn coerce_primitive(
        &mut self,
        value: &Value,
        hint: &str,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(_) = value else {
            return Ok(value.clone());
        };
        self.string_intrinsics()?;
        let method = self.get_method(value, &JsSymbol::well_known("toPrimitive").into())?;
        if !matches!(method, Value::Undefined) {
            let result = self.call_native(
                method,
                value.clone(),
                vec![Value::String(hint.into())],
                false,
            )?;
            return if matches!(result, Value::Object(_)) {
                Err(RuntimeError::TypeError(
                    "ToPrimitive returned an object".into(),
                ))
            } else {
                Ok(result)
            };
        }
        let names = if hint == "string" {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for name in names {
            let method = self.get_property(value, &name.into())?;
            if self.is_callable(&method)? {
                let result = self.call_native(method, value.clone(), Vec::new(), false)?;
                if !matches!(result, Value::Object(_)) {
                    return Ok(result);
                }
            }
        }
        Err(RuntimeError::TypeError(
            "cannot convert object to primitive".into(),
        ))
    }

    pub(in super::super) fn coerce_string(
        &mut self,
        value: &Value,
    ) -> Result<JsString, RuntimeError> {
        primitive::string(&self.coerce_primitive(value, "string")?)
    }

    pub(in super::super) fn coerce_number(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        primitive::number(&self.coerce_primitive(value, "number")?)
    }

    pub(in super::super) fn coerce_numeric(
        &mut self,
        value: &Value,
    ) -> Result<primitive::Numeric, RuntimeError> {
        primitive::numeric(&self.coerce_primitive(value, "number")?)
    }

    pub(in super::super) fn coerce_length(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        native::length(&Value::Number(self.coerce_number(value)?))
    }

    /// ToBigInt ( argument ). `coerce_primitive` performs the single
    /// observable ToPrimitive(argument, number) call; everything after that
    /// is non-observable dispatch on the resulting primitive's type.
    pub(in super::super) fn coerce_bigint(
        &mut self,
        value: &Value,
    ) -> Result<BigInt, RuntimeError> {
        match self.coerce_primitive(value, "number")? {
            Value::BigInt(value) => Ok(value),
            Value::Bool(value) => Ok(BigInt::from(u8::from(value))),
            Value::String(text) => {
                let text = text
                    .to_utf8()
                    .map_err(|_| RuntimeError::SyntaxError("invalid BigInt string".into()))?;
                primitive::string_to_bigint(&text)
                    .ok_or_else(|| RuntimeError::SyntaxError("invalid BigInt string".into()))
            }
            Value::Number(_) => Err(RuntimeError::TypeError(
                "cannot convert a Number to a BigInt".into(),
            )),
            Value::Null | Value::Undefined | Value::Symbol(_) | Value::Object(_) => Err(
                RuntimeError::TypeError("cannot convert value to a BigInt".into()),
            ),
        }
    }

    /// ToIndex ( value ), for `BigInt.asIntN`/`asUintN`'s `bits` parameter.
    /// The upper bound is the abstract operation's own 2**53-1, independent
    /// of any host object's storage capacity (contrast `buffer_index`, which
    /// bounds by `usize::MAX` for byte offsets/lengths instead).
    pub(in super::super) fn coerce_bigint_index(
        &mut self,
        value: &Value,
    ) -> Result<usize, RuntimeError> {
        let integer = self.coerce_number(value)?;
        let integer = if integer.is_nan() {
            0.0
        } else {
            integer.trunc()
        };
        if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
            return Err(RuntimeError::RangeError("index out of range".into()));
        }
        Ok(integer as usize)
    }

    /// ECMA-262 §19.2.5 parseInt.  The scan is deliberately prefix based:
    /// unlike Number(), trailing non-digits are ignored and an incomplete
    /// exponent is irrelevant because exponent syntax is not part of
    /// StringIntegerLiteral.
    pub(in super::super) fn parse_int(
        &mut self,
        value: &Value,
        radix: &Value,
    ) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        // A lone surrogate is not a digit, a sign or whitespace: it simply
        // ends the numeric prefix, exactly as U+FFFD does.
        let string = String::from_utf16_lossy(string.as_code_units());
        let mut input = string.trim_start_matches(primitive::whitespace);
        let negative = input.starts_with('-');
        if matches!(input.as_bytes().first(), Some(b'+' | b'-')) {
            input = &input[1..];
        }
        let requested = if matches!(radix, Value::Undefined) {
            0
        } else {
            let number = self.coerce_number(radix)?;
            primitive::to_uint32(number) as i32
        };
        if requested != 0 && !(2..=36).contains(&requested) {
            return Ok(Value::Number(f64::NAN));
        }
        let mut radix = requested;
        if (radix == 0 || radix == 16) && (input.starts_with("0x") || input.starts_with("0X")) {
            input = &input[2..];
            radix = 16;
        }
        if radix == 0 {
            radix = 10;
        }
        let mut digits = 0usize;
        let mut number = 0.0;
        for byte in input.bytes() {
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'z' => u32::from(byte - b'a') + 10,
                b'A'..=b'Z' => u32::from(byte - b'A') + 10,
                _ => break,
            };
            if digit >= radix as u32 {
                break;
            }
            digits += 1;
            number = number * f64::from(radix) + f64::from(digit);
        }
        if digits == 0 {
            Ok(Value::Number(f64::NAN))
        } else {
            Ok(Value::Number(if negative { -number } else { number }))
        }
    }

    /// ECMA-262 §19.2.4 parseFloat.  It recognizes only the longest valid
    /// decimal/Infinity prefix after StringTrim; hexadecimal and binary text
    /// therefore stop after their leading decimal zero.
    pub(in super::super) fn parse_float(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        // A lone surrogate is not part of any numeric literal: it simply ends
        // the prefix, exactly as U+FFFD does.
        let input = String::from_utf16_lossy(string.as_code_units());
        let input = input.trim_start_matches(primitive::whitespace);
        let sign_end = usize::from(matches!(input.as_bytes().first(), Some(b'+' | b'-')));
        let negative = input.starts_with('-');
        if input[sign_end..].starts_with("Infinity") {
            return Ok(Value::Number(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }));
        }
        let bytes = input.as_bytes();
        let mut index = sign_end;
        let mut digits = 0usize;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
            digits += 1;
        }
        if bytes.get(index) == Some(&b'.') {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Ok(Value::Number(f64::NAN));
        }
        if matches!(bytes.get(index), Some(b'e' | b'E')) {
            let exponent = index;
            index += 1;
            if matches!(bytes.get(index), Some(b'+' | b'-')) {
                index += 1;
            }
            let exponent_digits = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if index == exponent_digits {
                index = exponent;
            }
        }
        Ok(Value::Number(input[..index].parse().unwrap_or(f64::NAN)))
    }

    pub(in super::super) fn uri_coding_error<T>(
        &mut self,
        error: native::UriCodingError,
    ) -> Result<T, RuntimeError> {
        match error {
            native::UriCodingError::Malformed => Err(RuntimeError::Thrown(
                self.error_object("URIError", "malformed URI".into())?,
            )),
            native::UriCodingError::StringLimit { limit } => {
                Err(RuntimeError::StringLimit { limit })
            }
        }
    }

    pub(in super::super) fn encode_uri(
        &mut self,
        value: &Value,
        component: bool,
    ) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        match native::encode_uri(&string, component, self.config.max_string_bytes) {
            Ok(result) => Ok(Value::String(result)),
            Err(error) => self.uri_coding_error(error),
        }
    }

    pub(in super::super) fn decode_uri(
        &mut self,
        value: &Value,
        component: bool,
    ) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        match native::decode_uri(&string, component, self.config.max_string_bytes) {
            Ok(result) => Ok(Value::String(result)),
            Err(error) => self.uri_coding_error(error),
        }
    }

    pub(in super::super) fn array_length_value(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        // ArraySetLength performs ToUint32 followed by a separate ToNumber.
        // These must remain separate observable conversions: an object can
        // supply a stateful `valueOf` implementation.
        let length = native::uint32(&Value::Number(self.coerce_number(value)?))?;
        let number = self.coerce_number(value)?;
        if f64::from(length) != number {
            return Err(RuntimeError::RangeError("invalid array length".into()));
        }
        Ok(Value::Number(f64::from(length)))
    }

    pub(in super::super) fn coerce_property_key(
        &mut self,
        value: &Value,
    ) -> Result<PropertyName, RuntimeError> {
        let value = self.coerce_primitive(value, "string")?;
        Ok(match value {
            Value::Symbol(symbol) => symbol.into(),
            value => primitive::string(&value)?.into(),
        })
    }

    pub(in super::super) fn string_constructor_argument(
        &mut self,
        value: &Value,
        construct: bool,
    ) -> Result<JsString, RuntimeError> {
        if !construct {
            if let Value::Symbol(symbol) = value {
                return Ok(symbol.descriptive_string());
            }
        }
        self.coerce_string(value)
    }

    pub(in super::super) fn get_method(
        &mut self,
        value: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let method = self.get_property(value, key)?;
        if matches!(method, Value::Undefined | Value::Null) {
            return Ok(Value::Undefined);
        }
        if !self.is_callable(&method)? {
            return Err(RuntimeError::TypeError("property is not callable".into()));
        }
        Ok(method)
    }

    pub(in super::super) fn is_regexp(&mut self, value: &Value) -> Result<bool, RuntimeError> {
        if !matches!(value, Value::Object(_)) {
            return Ok(false);
        }
        let matcher = self.get_property(value, &JsSymbol::well_known("match").into())?;
        if !matches!(matcher, Value::Undefined) {
            return self.to_boolean(&matcher);
        }
        Ok(if let Value::Object(id) = value {
            self.heap.regexp(*id)?.is_some()
        } else {
            false
        })
    }

    pub(in super::super) fn dispatch_string_method(
        &mut self,
        method: StringMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        use StringMethod::*;
        if matches!(method, ToString | ValueOf) {
            return native::string_method(
                method,
                &self.unbox_string(receiver)?,
                &[],
                self.config.max_string_bytes,
            );
        }
        let string = self.string_receiver(receiver)?;
        let mut converted = Vec::new();
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        match method {
            At | CharAt | CharCodeAt | CodePointAt | Repeat => {
                converted.push(Value::Number(self.coerce_number(first)?));
            }
            Slice | Substring | Substr => {
                converted.push(Value::Number(self.coerce_number(first)?));
                converted.push(if matches!(second, Value::Undefined) {
                    Value::Undefined
                } else {
                    Value::Number(self.coerce_number(second)?)
                });
            }
            IndexOf | LastIndexOf | Includes | StartsWith | EndsWith => {
                if matches!(method, Includes | StartsWith | EndsWith) && self.is_regexp(first)? {
                    return Err(RuntimeError::TypeError(
                        "String search argument must not be a RegExp".into(),
                    ));
                }
                converted.push(Value::String(self.coerce_string(first)?));
                converted.push(if matches!(second, Value::Undefined) {
                    Value::Undefined
                } else {
                    Value::Number(self.coerce_number(second)?)
                });
            }
            Concat => {
                for value in args {
                    converted.push(Value::String(self.coerce_string(value)?));
                }
            }
            PadStart | PadEnd => {
                let target = self.coerce_length(first)?;
                if target <= string.len() as f64 {
                    return Ok(Value::String(string));
                }
                converted.push(Value::Number(target));
                converted.push(if matches!(second, Value::Undefined) {
                    Value::Undefined
                } else {
                    Value::String(self.coerce_string(second)?)
                });
            }
            Normalize if !matches!(first, Value::Undefined) => {
                converted.push(Value::String(self.coerce_string(first)?));
            }
            Html { attribute, .. } if !attribute.is_empty() => {
                converted.push(Value::String(self.coerce_string(first)?));
            }
            _ => {}
        }
        native::string_method(
            method,
            &Value::String(string),
            &converted,
            self.config.max_string_bytes,
        )
    }

    pub(in super::super) fn define_data(
        &mut self,
        owner: ObjectId,
        key: impl Into<PropertyName>,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), RuntimeError> {
        let key = key.into();
        let result = self.with_roots(|heap| {
            heap.define_own_property(
                owner,
                key,
                PropertyDescriptor::data(value, writable, enumerable, configurable),
            )
        })?;
        result
            .then_some(())
            .ok_or_else(|| RuntimeError::TypeError("cannot define property".into()))
    }

    pub(in super::super) fn array_from(
        &mut self,
        values: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        self.array_from_with_prototype(values, self.array_prototype)
    }

    /// Create an Array exotic with a caller-selected prototype. Array's
    /// constructor uses this for `Reflect.construct` and subclass `super()`;
    /// the ordinary helper above retains the current Realm's intrinsic.
    pub(in super::super) fn array_from_with_prototype(
        &mut self,
        values: Vec<Value>,
        prototype: ObjectId,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend(values.iter().cloned());
        let result = (|| {
            let array =
                self.with_roots(|heap| heap.alloc_array(values.len() as u32, Some(prototype)))?;
            self.stack.push(Value::Object(array));
            for (index, value) in values.into_iter().enumerate() {
                self.with_roots(|heap| heap.set(array, index.to_string(), value))?;
            }
            Ok(Value::Object(array))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn iterator_result(
        &mut self,
        value: Value,
        done: bool,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        self.stack.push(value.clone());
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        self.with_roots(|heap| heap.set(result, "value", value))?;
        self.with_roots(|heap| heap.set(result, "done", Value::Bool(done)))?;
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(result))
    }
}
