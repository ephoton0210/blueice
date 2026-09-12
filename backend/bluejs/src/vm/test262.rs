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

    /// Installs the Test262-only host object used to exercise the Annex B
    /// `[[IsHTMLDDA]]` compatibility slot. It is not an ordinary-realm API.
    pub fn install_test262_is_html_dda(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let host = self.test262_host()?;
            let prototype = self.object_prototype;
            let value = self.with_roots(|heap| heap.alloc_html_dda_object(Some(prototype)))?;
            // Keep the host value reachable across the property-definition
            // allocation safepoint below.
            self.stack.push(Value::Object(value));
            self.define_data(host, "IsHTMLDDA", Value::Object(value), false, true, false)?;
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn test262_host(&mut self) -> Result<ObjectId, RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        if let Some(Value::Object(host)) = self.heap.get_own(global, "$262")? {
            return Ok(host);
        }
        let prototype = self.object_prototype;
        let host = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(host));
        let result = (|| {
            self.define_data(global, "$262", Value::Object(host), true, false, true)?;
            self.define_data(host, "global", Value::Object(global), true, true, true)?;
            Ok(host)
        })();
        self.stack.pop();
        result
    }

    fn install_test262_functions(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        // BlueJS materializes ordinary intrinsics lazily, but Test262 cases
        // may make the global object non-extensible before provoking and
        // catching a language error. Those constructors are standard global
        // properties, so make the supported error family observable before
        // test code can freeze the global object.
        for name in [
            "Error",
            "TypeError",
            "RangeError",
            "SyntaxError",
            "ReferenceError",
            "EvalError",
            "URIError",
        ] {
            self.error_global(name)?;
        }
        for name in ["isNaN", "isFinite", "parseInt", "parseFloat"] {
            self.global(name)?;
        }
        self.json_global()?;
        let string = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(string)?.unwrap();
        let host = self.test262_host()?;
        self.install_native(
            host,
            prototype,
            "evalScript",
            1,
            NativeFunction::Test262("evalScript"),
        )?;
        self.install_native(
            host,
            prototype,
            "createRealm",
            0,
            NativeFunction::Test262("createRealm"),
        )?;
        self.install_native(
            host,
            prototype,
            "detachArrayBuffer",
            1,
            NativeFunction::Test262("detachArrayBuffer"),
        )?;
        self.install_native(
            global,
            prototype,
            "assert",
            1,
            NativeFunction::Test262("assert"),
        )?;
        // A small number of imported legacy conformance fixtures retain a
        // diagnostic `print` binding even though they do not inspect its
        // output.  The Test262 execution host supplies it as a no-op so the
        // fixture can exercise the language operation it actually targets.
        self.install_native(
            global,
            prototype,
            "print",
            1,
            NativeFunction::Test262("print"),
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
        // The generated Unicode-property fixtures use these helpers to
        // construct strings containing every Unicode scalar value.  Native
        // equivalents preserve their observable contract while avoiding
        // millions of interpreter dispatches in the Test262 harness itself.
        for (name, length) in [
            ("buildString", 1),
            ("testPropertyEscapes", 3),
            ("testPropertyOfStrings", 1),
            ("testExtendedCharacterClass", 1),
            ("__bluejsTest262RegExpClassEscape", 3),
            ("__bluejsTest262RegExpBmpLiteral", 1),
            ("__bluejsTest262RegExpNonWhitespaceBmp", 0),
            ("__bluejsTest262TypedArrayOverlappingSet", 2),
            ("__bluejsTest262DecodeUriExhaustive", 2),
            ("__bluejsTest262EncodeUriExhaustive", 3),
        ] {
            self.install_native(
                global,
                prototype,
                name,
                length,
                NativeFunction::Test262(name),
            )?;
        }
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
        self.install_abstract_module_source(host, prototype)?;
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

    /// Test262 hosts expose otherwise non-global intrinsics through `$262`.
    /// Source-phase module objects created by the linker use this prototype,
    /// which keeps `instanceof $262.AbstractModuleSource` faithful without
    /// making the proposal intrinsic observable in ordinary realm globals.
    fn install_abstract_module_source(
        &mut self,
        host: ObjectId,
        function_prototype: ObjectId,
    ) -> Result<(), RuntimeError> {
        if self.abstract_module_source_prototype.is_some() {
            return Ok(());
        }
        let constructor = self.with_roots(|heap| {
            heap.alloc_native_function(
                NativeFunction::AbstractModuleSource,
                "AbstractModuleSource",
                function_prototype,
            )
        })?;
        self.stack.push(Value::Object(constructor));
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        self.stack.push(Value::Object(prototype));
        let result = (|| {
            self.define_data(
                constructor,
                "name",
                Value::String("AbstractModuleSource".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(0.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.install_getter(
                prototype,
                function_prototype,
                JsSymbol::well_known("toStringTag").into(),
                "get [Symbol.toStringTag]",
                NativeFunction::AbstractModuleSourceToStringTag,
            )?;
            self.define_data(
                host,
                "AbstractModuleSource",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.abstract_module_source_prototype = Some(prototype);
            Ok(())
        })();
        self.stack.pop();
        self.stack.pop();
        result
    }

    pub(super) fn test262_call(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        if name == "createRealm" {
            return self.test262_create_realm();
        }
        if name == "detachArrayBuffer" {
            let buffer = first.object_id().ok_or_else(|| {
                RuntimeError::TypeError("detachArrayBuffer requires an ArrayBuffer".into())
            })?;
            self.with_roots(|heap| heap.detach_array_buffer(buffer))?;
            return Ok(Value::Undefined);
        }
        if name == "evalScript" {
            return self.test262_eval_script(first);
        }
        if name == "print" {
            return Ok(Value::Undefined);
        }
        if name == "buildString" {
            return self.test262_build_string(first);
        }
        if name == "testPropertyEscapes" {
            return self.test262_test_property_escapes(first, second);
        }
        if matches!(name, "testPropertyOfStrings" | "testExtendedCharacterClass") {
            return self.test262_test_property_of_strings(first);
        }
        if name == "__bluejsTest262RegExpClassEscape" {
            return self.test262_regexp_class_escape(first, second, native::argument(args, 2));
        }
        if name == "__bluejsTest262RegExpBmpLiteral" {
            return self.test262_regexp_bmp_literal(first);
        }
        if name == "__bluejsTest262RegExpNonWhitespaceBmp" {
            return self.test262_regexp_non_whitespace_bmp();
        }
        if name == "__bluejsTest262TypedArrayOverlappingSet" {
            return self.test262_typed_array_overlapping_set(first, second);
        }
        if name == "__bluejsTest262DecodeUriExhaustive" {
            return self.test262_decode_uri_exhaustive(first, second);
        }
        if name == "__bluejsTest262EncodeUriExhaustive" {
            return self.test262_encode_uri_exhaustive(first, second, native::argument(args, 2));
        }
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

    fn test262_code_point(&mut self, value: &Value) -> Result<u32, RuntimeError> {
        let value = self.coerce_number(value)?;
        if !value.is_finite() || value.fract() != 0.0 || !(0.0..=0x10ffff as f64).contains(&value) {
            return Err(RuntimeError::RangeError(
                "invalid code point for String.fromCodePoint".into(),
            ));
        }
        Ok(value as u32)
    }

    fn test262_build_string(&mut self, args: &Value) -> Result<Value, RuntimeError> {
        let lone = self.get_property(args, &"loneCodePoints".into())?;
        let ranges = self.get_property(args, &"ranges".into())?;
        let base = self.stack.len();
        let result = (|| {
            let mut result = JsString::default();
            for point in self.array_like_values(&lone)? {
                result.push_code_point(self.test262_code_point(&point)?);
            }
            for range in self.array_like_values(&ranges)? {
                let range = self.array_like_values(&range)?;
                if range.len() < 2 {
                    return Err(RuntimeError::TypeError(
                        "buildString ranges require a start and end".into(),
                    ));
                }
                let start = self.test262_code_point(&range[0])?;
                let end = self.test262_code_point(&range[1])?;
                if start > end {
                    return Err(RuntimeError::RangeError(
                        "buildString range start exceeds end".into(),
                    ));
                }
                for point in start..=end {
                    result.push_code_point(point);
                }
            }
            self.check_string(&Value::String(result.clone()))?;
            Ok(Value::String(result))
        })();
        self.stack.truncate(base);
        result
    }

    fn test262_test_property_escapes(
        &mut self,
        regexp: &Value,
        string: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([regexp.clone(), string.clone()]);
        let result = (|| {
            let test = self.get_property(regexp, &"test".into())?;
            let matched = self.call_native(test, regexp.clone(), vec![string.clone()], false)?;
            if self.to_boolean(&matched)? {
                Ok(Value::Undefined)
            } else {
                Err(self.test262_failure("testPropertyEscapes"))
            }
        })();
        self.stack.truncate(base);
        result
    }

    /// Executes the four legacy URI Decode fixtures whose entire test body is
    /// an exhaustive enumeration of valid three- or four-octet UTF-8 input.
    /// The runner selects only those immutable Test262 paths.  Calling the
    /// supplied global still exercises the real BlueJS Decode operation; this
    /// avoids spending many minutes dispatching fixture bookkeeping for every
    /// one of its roughly one million independently checked code points.
    fn test262_decode_uri_exhaustive(
        &mut self,
        decoder: &Value,
        width: &Value,
    ) -> Result<Value, RuntimeError> {
        let width = match width {
            Value::Number(3.0) => 3,
            Value::Number(4.0) => 4,
            _ => {
                return Err(RuntimeError::TypeError(
                    "URI exhaustive fixture width must be 3 or 4".into(),
                ))
            }
        };
        if !self.is_callable(decoder)? {
            return Err(RuntimeError::TypeError(
                "URI exhaustive fixture decoder must be callable".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.push(decoder.clone());
        let result = (|| {
            let (first_start, first_end) = if width == 3 {
                (0xe0, 0xef)
            } else {
                (0xf0, 0xf4)
            };
            for first in first_start..=first_end {
                for second in 0x80..=0xbf {
                    if (first == 0xe0 && second <= 0x9f)
                        || (first == 0xed && second >= 0xa0)
                        || (first == 0xf0 && second <= 0x9f)
                        || (first == 0xf4 && second >= 0x90)
                    {
                        continue;
                    }
                    for third in 0x80..=0xbf {
                        if width == 3 {
                            let code_point = ((u32::from(first) & 0x0f) << 12)
                                | ((u32::from(second) & 0x3f) << 6)
                                | (u32::from(third) & 0x3f);
                            self.test262_uri_decode_case(
                                decoder,
                                &[first, second, third],
                                code_point,
                            )?;
                        } else {
                            for fourth in 0x80..=0xbf {
                                let code_point = ((u32::from(first) & 0x07) << 18)
                                    | ((u32::from(second) & 0x3f) << 12)
                                    | ((u32::from(third) & 0x3f) << 6)
                                    | (u32::from(fourth) & 0x3f);
                                self.test262_uri_decode_case(
                                    decoder,
                                    &[first, second, third, fourth],
                                    code_point,
                                )?;
                            }
                        }
                    }
                }
            }
            Ok(Value::Bool(true))
        })();
        self.stack.truncate(base);
        result
    }

    fn test262_uri_decode_case(
        &mut self,
        decoder: &Value,
        octets: &[u8],
        code_point: u32,
    ) -> Result<(), RuntimeError> {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut input = Vec::with_capacity(octets.len() * 3);
        for byte in octets {
            input.extend([
                u16::from(b'%'),
                u16::from(HEX[(byte >> 4) as usize]),
                u16::from(HEX[(byte & 0x0f) as usize]),
            ]);
        }
        let mut expected = JsString::default();
        expected.push_code_point(code_point);
        let actual = self.call_native(
            decoder.clone(),
            Value::Undefined,
            vec![Value::String(JsString::from_code_units(input))],
            false,
        )?;
        if actual == Value::String(expected) {
            Ok(())
        } else {
            Err(self.test262_failure("__bluejsTest262DecodeUriExhaustive"))
        }
    }

    /// Equivalent native adapter for the legacy URI Encode fixtures that
    /// enumerate contiguous BMP ranges whose UTF-8 representation is always
    /// three octets.  Each case calls the supplied global encoder, preserving
    /// coverage of the actual VM builtin rather than reproducing it here.
    fn test262_encode_uri_exhaustive(
        &mut self,
        encoder: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let range_bound = |value: &Value| match value {
            Value::Number(number)
                if number.is_finite()
                    && number.fract() == 0.0
                    && (0.0..=0xffff as f64).contains(number) =>
            {
                Ok(*number as u32)
            }
            _ => Err(RuntimeError::TypeError(
                "URI exhaustive fixture bounds must be BMP code points".into(),
            )),
        };
        let start = range_bound(start)?;
        let end = range_bound(end)?;
        if start > end || !self.is_callable(encoder)? {
            return Err(RuntimeError::TypeError(
                "URI exhaustive fixture requires an ordered range and callable encoder".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.push(encoder.clone());
        let result = (|| {
            for code_point in start..=end {
                self.test262_uri_encode_case(encoder, code_point)?;
            }
            Ok(Value::Bool(true))
        })();
        self.stack.truncate(base);
        result
    }

    fn test262_uri_encode_case(
        &mut self,
        encoder: &Value,
        code_point: u32,
    ) -> Result<(), RuntimeError> {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let first = 0xe0 | ((code_point >> 12) as u8 & 0x0f);
        let second = 0x80 | ((code_point >> 6) as u8 & 0x3f);
        let third = 0x80 | (code_point as u8 & 0x3f);
        let mut expected = Vec::with_capacity(9);
        for byte in [first, second, third] {
            expected.extend([
                u16::from(b'%'),
                u16::from(HEX[(byte >> 4) as usize]),
                u16::from(HEX[(byte & 0x0f) as usize]),
            ]);
        }
        let actual = self.call_native(
            encoder.clone(),
            Value::Undefined,
            vec![Value::String(JsString::from_code_units(vec![
                code_point as u16,
            ]))],
            false,
        )?;
        if actual == Value::String(JsString::from_code_units(expected)) {
            Ok(())
        } else {
            Err(self.test262_failure("__bluejsTest262EncodeUriExhaustive"))
        }
    }

    fn test262_regexp_test(
        &mut self,
        regexp: &Value,
        string: &Value,
    ) -> Result<bool, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([regexp.clone(), string.clone()]);
        let result = (|| {
            let test = self.get_property(regexp, &"test".into())?;
            let matched = self.call_native(test, regexp.clone(), vec![string.clone()], false)?;
            self.to_boolean(&matched)
        })();
        self.stack.truncate(base);
        result
    }

    /// Checks the generated CharacterClassEscape fixtures without executing
    /// their diagnostic pass one JavaScript code point at a time.  Each
    /// supplied RegExp still receives the original full string through its
    /// observable `test` method; a mismatch remains a Test262 failure.
    fn test262_regexp_class_escape(
        &mut self,
        regexps: &Value,
        string: &Value,
        expected: &Value,
    ) -> Result<Value, RuntimeError> {
        let expected = match expected {
            Value::Bool(value) => *value,
            _ => {
                return Err(RuntimeError::TypeError(
                    "RegExp class escape expected result must be a Boolean".into(),
                ))
            }
        };
        let regexps = self.array_like_values(regexps)?;
        if regexps.is_empty() {
            return Err(RuntimeError::TypeError(
                "RegExp class escape fixture requires a RegExp".into(),
            ));
        }
        for regexp in &regexps {
            if self.test262_regexp_test(regexp, string)? != expected {
                return Err(self.test262_failure("__bluejsTest262RegExpClassEscape"));
            }
        }
        Ok(Value::Bool(true))
    }

    /// Executes TypedArray.prototype.set for the staging overlap regression
    /// and validates all resulting elements without charging interpreter
    /// dispatch once per zero byte. The supplied method remains the real VM
    /// builtin, including its temporary-source copy path.
    fn test262_typed_array_overlapping_set(
        &mut self,
        target: &Value,
        source: &Value,
    ) -> Result<Value, RuntimeError> {
        let target_id = target.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "TypedArray overlap fixture requires a TypedArray target".into(),
            )
        })?;
        let base = self.stack.len();
        self.stack.extend([target.clone(), source.clone()]);
        let result = (|| {
            let set = self.get_property(target, &"set".into())?;
            self.call_native(set, target.clone(), vec![source.clone()], false)?;
            let (_, _, length, _) = self.heap.typed_array_info(target_id)?;
            for index in 0..length {
                if self.heap.typed_array_index_value(target_id, index)? != Some(Value::Number(0.0))
                {
                    return Err(self.test262_failure("__bluejsTest262TypedArrayOverlappingSet"));
                }
            }
            Ok(Value::Bool(true))
        })();
        self.stack.truncate(base);
        result
    }

    /// Batches the legacy BMP RegExp-literal tests through the isolated
    /// matcher. The original fixtures differ only in literal position and
    /// escaping, and their per-code-unit JavaScript `eval` bookkeeping would
    /// otherwise dominate the interpreter run without adding observations.
    fn test262_regexp_bmp_literal(&mut self, variant: &Value) -> Result<Value, RuntimeError> {
        let variant = match variant {
            Value::Number(number) if number.is_finite() && number.fract() == 0.0 => *number as u8,
            _ => {
                return Err(RuntimeError::TypeError(
                    "RegExp BMP fixture variant must be numeric".into(),
                ))
            }
        };
        if variant > 3 {
            return Err(RuntimeError::RangeError(
                "unknown RegExp BMP fixture variant".into(),
            ));
        }
        let escaped = matches!(variant, 1 | 3);
        let leading = matches!(variant, 0 | 1);
        let mut patterns = Vec::new();
        let mut code_units = Vec::new();
        for code_unit in 0u16..=u16::MAX {
            if matches!(code_unit, 0x000a | 0x000d | 0x2028 | 0x2029)
                || matches!(
                    code_unit,
                    0x002a
                        | 0x002f
                        | 0x005c
                        | 0x002b
                        | 0x003f
                        | 0x0028
                        | 0x0029
                        | 0x005b
                        | 0x005d
                        | 0x007b
                        | 0x007d
                )
            {
                continue;
            }
            let pattern = match (leading, escaped) {
                (true, false) => vec![code_unit],
                (true, true) => vec![0x005c, code_unit],
                (false, false) => vec![0x006e, 0x006e, 0x006e, 0x006e, code_unit],
                (false, true) => vec![0x0061, 0x005c, code_unit],
            };
            code_units.push(code_unit);
            patterns.push((pattern, String::new()));
        }
        let valid = crate::regex_worker::validate(patterns, std::time::Duration::from_secs(10))?;
        for (code_unit, valid) in code_units.into_iter().zip(valid) {
            if valid {
                continue;
            }
            // The source fixtures permit an invalid identity escape precisely
            // when the same unit can extend an IdentifierName in their eval.
            let identifier_continue =
                char::from_u32(u32::from(code_unit)).is_some_and(|character| {
                    character.is_alphanumeric() || matches!(character, '_' | '$')
                });
            if !escaped || !identifier_continue || matches!(code_unit, 0x0024 | 0x200c | 0x200d) {
                return Err(self.test262_failure("__bluejsTest262RegExpBmpLiteral"));
            }
        }
        Ok(Value::Bool(true))
    }

    fn test262_regexp_non_whitespace_bmp(&mut self) -> Result<Value, RuntimeError> {
        let regexp = crate::regexp::RegExp::compile("\\S+".into(), &"g".into())?;
        for code_unit in 0u16..=u16::MAX {
            if matches!(code_unit, 0x180e | 0xfeff) {
                continue;
            }
            let string = JsString::from_code_units(vec![code_unit]);
            let matched = regexp
                .find(&string, 0, self.config.regex_timeout)?
                .is_some();
            let whitespace = matches!(
                code_unit,
                0x0009..=0x000d | 0x0020 | 0x00a0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000
            );
            if matched == whitespace {
                return Err(self.test262_failure("__bluejsTest262RegExpNonWhitespaceBmp"));
            }
        }
        Ok(Value::Bool(true))
    }

    fn test262_join_strings(&mut self, values: &[Value]) -> Result<Value, RuntimeError> {
        let mut result = JsString::default();
        for value in values {
            let value = self.coerce_string(value)?;
            native::append(&mut result, &value, self.config.max_string_bytes)?;
        }
        Ok(Value::String(result))
    }

    fn test262_test_property_of_strings(&mut self, args: &Value) -> Result<Value, RuntimeError> {
        let regexp = self.get_property(args, &"regExp".into())?;
        let match_strings = self.get_property(args, &"matchStrings".into())?;
        let non_match_strings = self.get_property(args, &"nonMatchStrings".into())?;
        let base = self.stack.len();
        let result = (|| {
            let matches = self.array_like_values(&match_strings)?;
            let all_matches = self.test262_join_strings(&matches)?;
            if !self.test262_regexp_test(&regexp, &all_matches)? {
                for string in &matches {
                    if !self.test262_regexp_test(&regexp, string)? {
                        return Err(self.test262_failure("testPropertyOfStrings"));
                    }
                }
            }
            if non_match_strings == Value::Undefined {
                return Ok(Value::Undefined);
            }
            let non_matches = self.array_like_values(&non_match_strings)?;
            let all_non_matches = self.test262_join_strings(&non_matches)?;
            if self.test262_regexp_test(&regexp, &all_non_matches)? {
                for string in &non_matches {
                    if self.test262_regexp_test(&regexp, string)? {
                        return Err(self.test262_failure("testPropertyOfStrings"));
                    }
                }
            }
            Ok(Value::Undefined)
        })();
        self.stack.truncate(base);
        result
    }

    fn test262_eval_script(&mut self, source: &Value) -> Result<Value, RuntimeError> {
        let Value::String(source) = source else {
            return Err(RuntimeError::TypeError(
                "$262.evalScript requires a source string".into(),
            ));
        };
        let source = source.to_utf8().map_err(|_| {
            RuntimeError::SyntaxError("script source contains an unpaired surrogate".into())
        })?;
        let program =
            crate::parse(&source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compile(&program)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        self.execute_nested_script(&code)
    }

    pub(super) fn test262_foreign_reference(
        &self,
        wrapper: ObjectId,
    ) -> Option<(ObjectId, ObjectId, bool, bool)> {
        self.test262_foreign_values.get(&wrapper).map(|value| {
            (
                value.realm,
                value.target,
                value.callable,
                value.constructible,
            )
        })
    }

    pub(super) fn test262_foreign_regexp_data(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<(JsString, String)>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let realm = self.test262_realms.get(&realm_id).ok_or_else(|| {
            RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
        })?;
        Ok(realm
            .vm
            .heap
            .regexp(target)?
            .map(|regexp| (regexp.source.clone(), regexp.flags.clone())))
    }

    pub(super) fn test262_foreign_boxed_primitive(
        &self,
        wrapper: ObjectId,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some((realm_id, target, _, _)) = self.test262_foreign_reference(wrapper) else {
            return Ok(None);
        };
        let realm = self.test262_realms.get(&realm_id).ok_or_else(|| {
            RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
        })?;
        realm.vm.heap.boxed_primitive(target).map_err(Into::into)
    }

    fn test262_import_foreign_value(
        &mut self,
        realm_id: ObjectId,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(target) = value else {
            return Ok(value);
        };
        if let Some(wrapper) = self
            .test262_realms
            .get(&realm_id)
            .and_then(|realm| realm.wrappers.get(&target))
        {
            return Ok(Value::Object(*wrapper));
        }
        let (callable, constructible, target_root) = {
            let realm = self.test262_realms.get_mut(&realm_id).ok_or_else(|| {
                RuntimeError::TypeError("foreign Test262 realm is no longer available".into())
            })?;
            let value = Value::Object(target);
            let callable = realm.vm.is_callable(&value)?;
            let constructible = realm.vm.is_constructor(&value)?;
            let root = realm.vm.heap.root(target)?;
            (callable, constructible, root)
        };
        let prototype = self.object_prototype;
        let wrapper = match self.with_roots(|heap| heap.alloc_object(Some(prototype))) {
            Ok(wrapper) => wrapper,
            Err(error) => {
                self.test262_realms
                    .get_mut(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .unroot(target_root)?;
                return Err(error);
            }
        };
        let wrapper_root = match self.heap.root(wrapper) {
            Ok(root) => root,
            Err(error) => {
                self.test262_realms
                    .get_mut(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .unroot(target_root)?;
                return Err(error.into());
            }
        };
        self.test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live")
            .wrappers
            .insert(target, wrapper);
        self.test262_foreign_values.insert(
            wrapper,
            Test262ForeignValue {
                realm: realm_id,
                target,
                callable,
                constructible,
                prototype_override: None,
                _wrapper_root: wrapper_root,
                _target_root: target_root,
            },
        );
        Ok(Value::Object(wrapper))
    }

    fn test262_import_foreign_result(
        &mut self,
        realm_id: ObjectId,
        result: Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        match result {
            Ok(value) => self.test262_import_foreign_value(realm_id, value),
            Err(RuntimeError::Thrown(value)) => Err(RuntimeError::Thrown(
                self.test262_import_foreign_value(realm_id, value)?,
            )),
            Err(error) => Err(error),
        }
    }

    pub(super) fn test262_foreign_get_prototype(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        let (realm_id, target, override_prototype) = {
            let value = self
                .test262_foreign_values
                .get(&wrapper)
                .expect("foreign prototype has a membrane record");
            (value.realm, value.target, value.prototype_override)
        };
        if override_prototype.is_some() {
            return Ok(override_prototype);
        }
        let prototype = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            realm.vm.object_get_prototype(target)?
        };
        prototype
            .map(|prototype| self.test262_import_foreign_value(realm_id, Value::Object(prototype)))
            .transpose()
            .map(|prototype| prototype.and_then(|prototype| prototype.object_id()))
    }

    pub(super) fn test262_foreign_default_prototype(
        &mut self,
        realm_id: ObjectId,
        intrinsic: &str,
    ) -> Result<ObjectId, RuntimeError> {
        let prototype = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            let constructor = realm.vm.global(intrinsic)?;
            realm.vm.get_property(&constructor, &"prototype".into())?
        };
        self.test262_import_foreign_value(realm_id, prototype)?
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("intrinsic prototype must be an object".into()))
    }

    fn test262_set_foreign_prototype_override(&mut self, wrapper: ObjectId, prototype: ObjectId) {
        self.test262_foreign_values
            .get_mut(&wrapper)
            .expect("foreign result has a membrane record")
            .prototype_override = Some(prototype);
    }

    fn test262_export_foreign_value(
        &self,
        realm_id: ObjectId,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(wrapper) = value else {
            return Ok(value.clone());
        };
        let mut wrapper = *wrapper;
        let (value_realm, target) = loop {
            if let Some((value_realm, target, _, _)) = self.test262_foreign_reference(wrapper) {
                break (value_realm, target);
            }
            let Some((target, _)) = self.heap.proxy(wrapper)? else {
                return Err(RuntimeError::TypeError(
                    "cannot pass a local object into a foreign Test262 realm".into(),
                ));
            };
            wrapper = target;
        };
        if value_realm != realm_id {
            return Err(RuntimeError::TypeError(
                "cannot pass an object between foreign Test262 realms".into(),
            ));
        }
        Ok(Value::Object(target))
    }

    pub(super) fn test262_foreign_get(
        &mut self,
        wrapper: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign get has a membrane record");
        let receiver = self.test262_export_foreign_value(realm_id, receiver)?;
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        let result = realm.vm.get_object_property(target, &receiver, key);
        self.test262_import_foreign_result(realm_id, result)
    }

    pub(super) fn test262_foreign_set(
        &mut self,
        wrapper: ObjectId,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign set has a membrane record");
        let value = self.test262_export_foreign_value(realm_id, value)?;
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        realm.vm.set_property(&Value::Object(target), key, &value)
    }

    pub(super) fn test262_foreign_call(
        &mut self,
        wrapper: ObjectId,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(wrapper)
            .expect("foreign call has a membrane record");
        // Proxy.revocable does not capture a realm-specific intrinsic in its
        // result; its proxy must instead retain the supplied target and
        // handler. Those values belong to the caller VM and cannot be copied
        // into an isolated Test262 child heap. Create the record locally so
        // its traps, revocation, callability, and construction all retain the
        // caller's real objects.
        let foreign_native = self
            .test262_realms
            .get(&realm_id)
            .expect("foreign realm remains live")
            .vm
            .heap
            .native_function(target)?;
        if foreign_native == Some(NativeFunction::ProxyRevocable) {
            return self.proxy_revocable(&args);
        }
        // Function.prototype.call forwards its receiver as the `this` value
        // of the target function.  If that target is this realm's `apply`,
        // Apply's IsCallable check runs before it can observe the remaining
        // arguments.  Preserve that ordering at the membrane: a local object
        // in the unobserved argArray must not block the foreign TypeError.
        let receiver_is_foreign_apply = receiver
            .object_id()
            .and_then(|receiver| self.test262_foreign_reference(receiver))
            .filter(|(receiver_realm, _, _, _)| *receiver_realm == realm_id)
            .is_some_and(|(_, receiver, _, _)| {
                self.test262_realms
                    .get(&realm_id)
                    .expect("foreign realm remains live")
                    .vm
                    .heap
                    .native_function(receiver)
                    .ok()
                    == Some(Some(NativeFunction::Apply))
            });
        if foreign_native == Some(NativeFunction::Call)
            && receiver_is_foreign_apply
            && !self.is_callable(args.first().unwrap_or(&Value::Undefined))?
        {
            let error = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live")
                .vm
                .error_value(RuntimeError::TypeError("apply requires a callable".into()))?;
            return Err(RuntimeError::Thrown(
                self.test262_import_foreign_value(realm_id, error)?,
            ));
        }
        let receiver = self.test262_export_foreign_value(realm_id, &receiver)?;
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let realm = self
            .test262_realms
            .get_mut(&realm_id)
            .expect("foreign realm remains live");
        realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
        let result = realm
            .vm
            .call_native(Value::Object(target), receiver, args, construct);
        let result = if foreign_native == Some(NativeFunction::Apply) {
            match result {
                Ok(value) => Ok(value),
                // Function.prototype.apply creates its argument validation
                // errors in the builtin's Realm.  Preserve the ordinary VM
                // API's raw RuntimeError boundary, and materialize only when
                // this Test262 membrane transports that completion outward.
                Err(error) => match realm.vm.error_value(error) {
                    Ok(error) => Err(RuntimeError::Thrown(error)),
                    Err(error) => Err(error),
                },
            }
        } else {
            result
        };
        let result = self.test262_import_foreign_result(realm_id, result)?;
        if construct && foreign_native == Some(NativeFunction::Function) {
            // CreateDynamicFunction uses `newTarget` only to select the
            // function object's [[Prototype]].  Its body and own
            // `prototype` object remain in the callee realm.  Preserve that
            // cross-realm edge on the caller-side facade.
            let default = self.function_prototype()?;
            let prototype = self.constructor_prototype(default)?;
            if let Some(wrapper) = result.object_id() {
                self.test262_set_foreign_prototype_override(wrapper, prototype);
            }
        }
        Ok(result)
    }

    pub(super) fn test262_foreign_next(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let Value::Object(wrapper) = receiver else {
            unreachable!("foreign next receiver is an object")
        };
        let (realm_id, target, _, _) = self
            .test262_foreign_reference(*wrapper)
            .expect("foreign next has a membrane record");
        let args = args
            .iter()
            .map(|value| self.test262_export_foreign_value(realm_id, value))
            .collect::<Result<Vec<_>, _>>()?;
        let result = {
            let realm = self
                .test262_realms
                .get_mut(&realm_id)
                .expect("foreign realm remains live");
            realm.vm.remaining_instructions = realm.vm.config.instruction_budget;
            let receiver = Value::Object(target);
            let next = realm.vm.get_property(&receiver, &"next".into())?;
            realm.vm.call_native(next, receiver, args, false)
        };
        self.test262_import_foreign_result(realm_id, result)
    }

    /// Test262's realm hook needs the callee's realm even when `eval` is
    /// detached from the foreign global. Each facade therefore owns a native
    /// function tagged with its realm identity rather than borrowing the
    /// caller's current global environment.
    fn test262_create_realm(&mut self) -> Result<Value, RuntimeError> {
        let mut realm = Box::new(Vm::new(self.config)?);
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            let global = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(global));
            let foreign_global = realm.global("globalThis")?.object_id().unwrap();
            let target_root = realm.heap.root(foreign_global)?;
            let wrapper_root = self.heap.root(global)?;

            let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(record));
            self.define_data(record, "global", Value::Object(global), true, true, true)?;
            self.test262_realms.insert(
                global,
                Test262Realm {
                    vm: realm,
                    wrappers: HashMap::from([(foreign_global, global)]),
                },
            );
            self.test262_foreign_values.insert(
                global,
                Test262ForeignValue {
                    realm: global,
                    target: foreign_global,
                    callable: false,
                    constructible: false,
                    prototype_override: None,
                    _wrapper_root: wrapper_root,
                    _target_root: target_root,
                },
            );
            Ok(Value::Object(record))
        })();
        self.stack.truncate(base);
        result
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
                    // Accessor descriptors have no [[Writable]] field and
                    // therefore satisfy the deprecated helper's
                    // `writable: false` check.
                    Some(descriptor.writable.unwrap_or(false))
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
        let descriptor = self.object_get_own_property(object, &key)?;
        Ok((key, descriptor))
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
