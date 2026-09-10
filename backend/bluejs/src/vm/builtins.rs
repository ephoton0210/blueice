// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::GeneratorState;
use crate::native::{MathMethod, ObjectMethod, PatternMethod, StringMethod};
use std::rc::Rc;

fn math_uint32(value: f64) -> u32 {
    if value.is_finite() { value.trunc().rem_euclid(4_294_967_296.0) as u32 } else { 0 }
}

impl Vm {
    fn direct_eval(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::String(source) = value else { return Ok(value.clone()) };
        let source = source.to_utf8().map_err(|_| RuntimeError::SyntaxError("eval source contains an unpaired surrogate".into()))?;
        let program = crate::parse(&source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compiler::compile_eval(&program, &self.eval_visible_bindings(), self.strict)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let captures = code.captures.iter().map(|slot| self.capture(*slot as usize)).collect::<Result<Vec<_>, _>>()?;
        self.execute_eval(&code, captures)
    }

    pub(super) fn array_push(&mut self, array: &Value, value: &Value, kind: usize) -> Result<(), RuntimeError> {
        let id = array.object_id().unwrap();
        if kind == 2 {
            let record = self.get_iterator(value)?;
            self.stack.push(record.clone());
            while let Some(value) = self.iterator_step(&record, true)? {
                self.charge_step()?;
                self.array_push(array, &value, 0)?;
            }
            self.stack.pop();
        } else {
            let Value::Number(length) = self.heap.get(id, "length")? else { unreachable!() };
            if kind == 1 {
                self.with_roots(|heap| heap.set(id, "length", Value::Number(length + 1.0)))?;
            } else {
                self.with_roots(|heap| heap.set(id, (length as u32).to_string(), value.clone()))?;
                self.with_roots(|heap| heap.set(id, "length", Value::Number(length + 1.0)))?;
            }
        }
        Ok(())
    }

    fn array_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.array_iterator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(prototype, function_prototype, "next", 0, NativeFunction::ArrayIteratorNext)?;
            self.define_data(prototype, JsSymbol::well_known("toStringTag"), Value::String("Array Iterator".into()), false, false, true)?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.array_iterator_prototype = Some(prototype);
        }
        result
    }

    fn generator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.generator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(prototype, function_prototype, "next", 1, NativeFunction::GeneratorNext)?;
            self.install_native(prototype, function_prototype, "return", 1, NativeFunction::GeneratorReturn)?;
            self.define_data(prototype, JsSymbol::well_known("toStringTag"), Value::String("Generator".into()), false, false, true)?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.generator_prototype = Some(prototype);
        }
        result
    }
    pub(super) fn get_from_prototype(&mut self, start: ObjectId, receiver: &Value, key: &PropertyName) -> Result<Value, RuntimeError> {
        let mut current = Some(start);
        while let Some(object) = current {
            if let Some(desc) = self.heap.get_own_property_descriptor(object, key)? {
                if desc.accessor() {
                    let getter = desc.get.unwrap_or(Value::Undefined);
                    return if matches!(getter, Value::Undefined) { Ok(Value::Undefined) } else { self.call_native(getter, receiver.clone(), Vec::new(), false) };
                }
                return Ok(desc.value.unwrap_or(Value::Undefined));
            }
            current = self.heap.prototype(object)?;
        }
        Ok(Value::Undefined)
    }

    pub(super) fn constructor_prototype(&mut self, default: ObjectId) -> Result<ObjectId, RuntimeError> {
        let target = self.new_target.clone();
        let prototype = self.get_property(&target, &"prototype".into())?;
        Ok(prototype.object_id().unwrap_or(default))
    }

    pub(super) fn is_constructor(&self, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(id) = value else { return Ok(false) };
        if let Some(bound) = self.heap.bound_function(*id)? {
            return Ok(bound.constructible);
        }
        if let Some((code, _, _)) = self.heap.closure(*id)? {
            return Ok(code.constructible);
        }
        Ok(matches!(
            self.heap.native_function(*id)?,
            Some(
                NativeFunction::String
                    | NativeFunction::Array
                    | NativeFunction::Object
                    | NativeFunction::RegExp
                    | NativeFunction::Collator
                    | NativeFunction::Locale
                    | NativeFunction::Error(_)
                    | NativeFunction::PrimitiveConstructor(_)
            )
        ))
    }

    pub(super) fn array_like_values(&mut self, value: &Value) -> Result<Vec<Value>, RuntimeError> {
        if !matches!(value, Value::Object(_)) {
            return Err(RuntimeError::TypeError("argument list must be an object".into()));
        }
        let length = self.get_property(value, &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut args = Vec::new();
        for index in 0..length {
            self.charge_step()?;
            let value = self.get_property(value, &index.to_string().into())?;
            self.stack.push(value.clone());
            args.push(value);
        }
        Ok(args)
    }

    pub(super) fn base_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_base {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(prototype)?;
        let result = self.install_symbol_native(prototype, function_prototype, "iterator", 0, NativeFunction::IteratorSelf);
        if let Err(error) = result {
            self.heap.unroot(root)?;
            return Err(error);
        }
        self.iterator_base = Some(prototype);
        Ok(prototype)
    }

    pub(super) fn template_object(&mut self, site: &crate::bytecode::TemplateSite) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.templates.get(&site.id) {
            return Ok(Value::Object(id));
        }
        let base = self.stack.len();
        let result = (|| {
            for string in site.raw.iter().chain(site.cooked.iter().flatten()) {
                self.check_string(&Value::String(string.clone()))?;
            }
            let raw = self.array_from(site.raw.iter().cloned().map(Value::String).collect())?;
            self.stack.push(raw.clone());
            let cooked = self.array_from(site.cooked.iter().cloned().map(|s| s.map_or(Value::Undefined, Value::String)).collect())?;
            self.stack.push(cooked.clone());
            self.define_data(cooked.object_id().unwrap(), "raw", raw.clone(), false, false, false)?;
            for array in [&raw, &cooked] {
                let id = array.object_id().unwrap();
                for key in self.heap.own_property_keys(id)? {
                    let mut desc = self.heap.get_own_property_descriptor(id, &key)?.unwrap();
                    desc.writable = Some(false);
                    desc.configurable = Some(false);
                    self.with_roots(|heap| heap.define_own_property(id, key, desc))?;
                }
                self.heap.prevent_extensions(id)?;
            }
            self.heap.root(cooked.object_id().unwrap())?;
            self.templates.insert(site.id, cooked.object_id().unwrap());
            Ok(cooked)
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn get_iterator(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let method = self.get_method(value, &JsSymbol::well_known("iterator").into())?;
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError("iterator must be an object".into()));
        }
        self.stack.push(iterator.clone());
        let next = self.get_property(&iterator, &"next".into())?;
        self.stack.push(next.clone());
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(record));
        self.with_roots(|heap| heap.set(record, "iterator", iterator))?;
        self.with_roots(|heap| heap.set(record, "next", next))?;
        self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(record))
    }

    /// IteratorStepValue, or IteratorStep without IteratorValue for elisions.
    /// Iterator-origin errors complete this record before outer unwinding.
    pub(super) fn iterator_step(&mut self, record: &Value, read_value: bool) -> Result<Option<Value>, RuntimeError> {
        let Value::Object(record) = record else { unreachable!("compiler only emits iterator records") };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return Ok(None);
        }
        let outcome = (|| {
            let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
            let next = self.get_property(&Value::Object(*record), &"next".into())?;
            let result = self.call_native(next, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError("iterator result must be an object".into()));
            }
            self.stack.push(result.clone());
            let done = self.get_property(&result, &"done".into())?;
            let value = if primitive::truthy(&done) {
                None
            } else if read_value {
                Some(self.get_property(&result, &"value".into())?)
            } else {
                Some(Value::Undefined)
            };
            self.stack.pop();
            Ok(value)
        })();
        if !matches!(&outcome, Ok(Some(_))) {
            let base = self.stack.len();
            if let Err(RuntimeError::Thrown(value)) = &outcome {
                self.stack.push(value.clone());
            }
            let marked = self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)));
            self.stack.truncate(base);
            marked?;
        }
        outcome
    }

    pub(super) fn iterator_close(&mut self, record: &Value) -> Result<(), RuntimeError> {
        let id = record.object_id().expect("compiler only emits iterator records");
        if matches!(self.heap.get_own(id, "done")?, Some(Value::Bool(true))) {
            return Ok(());
        }
        self.with_roots(|heap| heap.set(id, "done", Value::Bool(true)))?;
        let iterator = self.get_property(record, &"iterator".into())?;
        let close = self.get_method(&iterator, &"return".into())?;
        if close != Value::Undefined {
            let result = self.call_native(close, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError("iterator return must return an object".into()));
            }
        }
        Ok(())
    }
    pub(super) fn binding_value(&self, slot: usize) -> Result<Option<Value>, RuntimeError> {
        if let Some(cell) = self.cells.get(&slot) { Ok(self.heap.get_own(*cell, "value")?) } else { Ok(self.bindings[slot].clone()) }
    }

    pub(super) fn store_binding(&mut self, slot: usize, value: Value) -> Result<(), RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            self.with_roots(|heap| heap.set(cell, "value", value))?;
        } else {
            self.bindings[slot] = Some(value);
        }
        Ok(())
    }

    pub(super) fn capture(&mut self, slot: usize) -> Result<ObjectId, RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            return Ok(cell);
        }
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        self.cells.insert(slot, cell);
        if let Some(value) = self.bindings[slot].take() {
            self.with_roots(|heap| heap.set(cell, "value", value))?;
        }
        Ok(cell)
    }

    pub(super) fn call_closure(&mut self, code: Rc<Bytecode>, captures: Vec<ObjectId>, callee: Value, receiver: Value, args: Vec<Value>, construct: bool) -> Result<Value, RuntimeError> {
        if construct && !code.constructible {
            return Err(RuntimeError::TypeError("arrow function is not a constructor".into()));
        }
        let receiver = if construct {
            let prototype = self.constructor_prototype(self.object_prototype)?;
            Value::Object(self.with_roots(|heap| heap.alloc_object(Some(prototype)))?)
        } else if code.arrow || code.strict {
            receiver
        } else if matches!(receiver, Value::Null | Value::Undefined) {
            self.global("globalThis")?
        } else {
            Value::Object(self.coerce_object(&receiver)?)
        };
        if code.generator {
            let prototype = self.generator_prototype()?;
            let state = GeneratorState::Start { code, captures, callee, receiver, args };
            return Ok(Value::Object(self.with_roots(|heap| heap.alloc_generator(state, prototype))?));
        }
        self.stack.push(receiver.clone());
        let constructed = receiver.clone();
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack.extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let mut frame_bindings = vec![None; code.bindings.len()];
        if let Some(slot) = code.self_slot {
            frame_bindings[slot as usize] = Some(callee);
        }
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let result = self.run(&code);
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.this = this;
        self.arguments = arguments;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.stack.truncate(base - 1);
        result.map(|value| if construct && !matches!(value, Value::Object(_)) { constructed } else { value })
    }

    fn generator_next(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let Value::Object(generator) = receiver else { return Err(RuntimeError::TypeError("Generator next requires a generator".into())) };
        let state = self.heap.take_generator_state(*generator)?;
        if matches!(state, GeneratorState::Done) {
            self.heap.set_generator_state(*generator, GeneratorState::Done)?;
            return self.iterator_result(Value::Undefined, true);
        }

        let (code, pc, resume_value, frame_stack, frame_bindings, frame_cells, frame_this, frame_args, frame_completion, frame_completion_empty, frame_scopes) =
            match state {
                GeneratorState::Start { code, captures, callee, receiver, args } => {
                    let mut bindings = vec![None; code.bindings.len()];
                    if let Some(slot) = code.self_slot {
                        bindings[slot as usize] = Some(callee);
                    }
                    (code, 0, None, Vec::new(), bindings, captures.into_iter().enumerate().collect(), receiver, args, Value::Undefined, true, Vec::new())
                }
                GeneratorState::Suspended { code, pc, stack, bindings, cells, this, args, completion, completion_empty, active_scopes } => {
                    (code, pc, Some(Value::Undefined), stack, bindings, cells.into_iter().collect(), this, args, completion, completion_empty, active_scopes)
                }
                GeneratorState::Done => unreachable!("completed generators returned above"),
            };

        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack.extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let frame_base = self.stack.len();
        self.stack.extend(frame_stack.iter().cloned());

        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, frame_cells);
        let this = std::mem::replace(&mut self.this, frame_this);
        let arguments = std::mem::replace(&mut self.arguments, frame_args);
        let completion = std::mem::replace(&mut self.completion, frame_completion);
        let completion_empty = std::mem::replace(&mut self.completion_empty, frame_completion_empty);
        let active_scopes = std::mem::replace(&mut self.active_scopes, frame_scopes);
        let active_scope_slots = std::mem::replace(
            &mut self.active_scope_slots,
            self.active_scopes.iter().map(|scope| code.scopes[*scope as usize].clone()).collect(),
        );
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let mut iterators = Vec::new();
        let outcome = self.interpret(&code, &mut iterators, pc, resume_value);

        let (next_state, result) = match outcome {
            Ok(InterpreterExit::Return(value)) => {
                self.stack.truncate(frame_base);
                (GeneratorState::Done, Ok((value, true)))
            }
            Ok(InterpreterExit::Yield { value, pc }) => {
                let stack = self.stack.split_off(frame_base);
                let state = GeneratorState::Suspended {
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells).into_iter().collect(),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                };
                (state, Ok((value, false)))
            }
            Err(error) => {
                self.stack.truncate(frame_base);
                (GeneratorState::Done, Err(error))
            }
        };
        self.heap.set_generator_state(*generator, next_state)?;
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.this = this;
        self.arguments = arguments;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.stack.truncate(base);
        let (value, done) = result?;
        self.iterator_result(value, done)
    }

    fn generator_return(&mut self, receiver: &Value, value: Value) -> Result<Value, RuntimeError> {
        let Value::Object(generator) = receiver else { return Err(RuntimeError::TypeError("Generator return requires a generator".into())) };
        let _ = self.heap.take_generator_state(*generator)?;
        self.heap.set_generator_state(*generator, GeneratorState::Done)?;
        self.iterator_result(value, true)
    }

    pub(super) fn is_callable(&self, value: &Value) -> Result<bool, RuntimeError> {
        Ok(if let Value::Object(id) = value {
            self.heap.native_function(*id)?.is_some() || self.heap.closure(*id)?.is_some() || self.heap.bound_function(*id)?.is_some()
        } else {
            false
        })
    }

    pub(super) fn coerce_primitive(&mut self, value: &Value, hint: &str) -> Result<Value, RuntimeError> {
        let Value::Object(_) = value else { return Ok(value.clone()) };
        self.string_intrinsics()?;
        let method = self.get_method(value, &JsSymbol::well_known("toPrimitive").into())?;
        if !matches!(method, Value::Undefined) {
            let result = self.call_native(method, value.clone(), vec![Value::String(hint.into())], false)?;
            return if matches!(result, Value::Object(_)) { Err(RuntimeError::TypeError("ToPrimitive returned an object".into())) } else { Ok(result) };
        }
        let names = if hint == "string" { ["toString", "valueOf"] } else { ["valueOf", "toString"] };
        for name in names {
            let method = self.get_property(value, &name.into())?;
            if self.is_callable(&method)? {
                let result = self.call_native(method, value.clone(), Vec::new(), false)?;
                if !matches!(result, Value::Object(_)) {
                    return Ok(result);
                }
            }
        }
        Err(RuntimeError::TypeError("cannot convert object to primitive".into()))
    }

    pub(super) fn coerce_string(&mut self, value: &Value) -> Result<JsString, RuntimeError> {
        primitive::string(&self.coerce_primitive(value, "string")?)
    }

    pub(super) fn coerce_number(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        primitive::number(&self.coerce_primitive(value, "number")?)
    }

    pub(super) fn coerce_length(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        native::length(&Value::Number(self.coerce_number(value)?))
    }

    /// ECMA-262 §19.2.5 parseInt.  The scan is deliberately prefix based:
    /// unlike Number(), trailing non-digits are ignored and an incomplete
    /// exponent is irrelevant because exponent syntax is not part of
    /// StringIntegerLiteral.
    fn parse_int(&mut self, value: &Value, radix: &Value) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        let Ok(string) = string.to_utf8() else { return Ok(Value::Number(f64::NAN)) };
        let mut input = string.trim_start_matches(primitive::whitespace);
        let negative = input.starts_with('-');
        if matches!(input.as_bytes().first(), Some(b'+' | b'-')) {
            input = &input[1..];
        }
        let requested = if matches!(radix, Value::Undefined) {
            0
        } else {
            let number = self.coerce_number(radix)?;
            if number.is_finite() { number.trunc().rem_euclid(4_294_967_296.0) as u32 as i32 } else { 0 }
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
        if digits == 0 { Ok(Value::Number(f64::NAN)) } else { Ok(Value::Number(if negative { -number } else { number })) }
    }

    /// ECMA-262 §19.2.4 parseFloat.  It recognizes only the longest valid
    /// decimal/Infinity prefix after StringTrim; hexadecimal and binary text
    /// therefore stop after their leading decimal zero.
    fn parse_float(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        let Ok(input) = string.to_utf8() else { return Ok(Value::Number(f64::NAN)) };
        let input = input.trim_start_matches(primitive::whitespace);
        let sign_end = usize::from(matches!(input.as_bytes().first(), Some(b'+' | b'-')));
        let negative = input.starts_with('-');
        if input[sign_end..].starts_with("Infinity") {
            return Ok(Value::Number(if negative { f64::NEG_INFINITY } else { f64::INFINITY }));
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

    pub(super) fn array_length_value(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        // ArraySetLength performs ToUint32 followed by a separate ToNumber.
        let length = native::uint32(&Value::Number(self.coerce_number(value)?))?;
        if f64::from(length) != self.coerce_number(value)? {
            return Err(RuntimeError::RangeError("invalid array length".into()));
        }
        Ok(Value::Number(f64::from(length)))
    }

    pub(super) fn coerce_property_key(&mut self, value: &Value) -> Result<PropertyName, RuntimeError> {
        let value = self.coerce_primitive(value, "string")?;
        Ok(match value {
            Value::Symbol(symbol) => symbol.into(),
            value => primitive::string(&value)?.into(),
        })
    }

    pub(super) fn string_constructor_argument(&mut self, value: &Value, construct: bool) -> Result<JsString, RuntimeError> {
        if !construct {
            if let Value::Symbol(symbol) = value {
                return Ok(symbol.descriptive_string());
            }
        }
        self.coerce_string(value)
    }

    pub(super) fn get_method(&mut self, value: &Value, key: &PropertyName) -> Result<Value, RuntimeError> {
        let method = self.get_property(value, key)?;
        if matches!(method, Value::Undefined | Value::Null) {
            return Ok(Value::Undefined);
        }
        if !self.is_callable(&method)? {
            return Err(RuntimeError::TypeError("property is not callable".into()));
        }
        Ok(method)
    }

    pub(super) fn is_regexp(&mut self, value: &Value) -> Result<bool, RuntimeError> {
        if !matches!(value, Value::Object(_)) {
            return Ok(false);
        }
        let matcher = self.get_property(value, &JsSymbol::well_known("match").into())?;
        if !matches!(matcher, Value::Undefined) {
            return Ok(primitive::truthy(&matcher));
        }
        Ok(if let Value::Object(id) = value { self.heap.regexp(*id)?.is_some() } else { false })
    }

    pub(super) fn dispatch_string_method(&mut self, method: StringMethod, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        use StringMethod::*;
        if matches!(method, ToString | ValueOf) {
            return native::string_method(method, &self.unbox_string(receiver)?, &[], self.config.max_string_bytes);
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
                converted.push(if matches!(second, Value::Undefined) { Value::Undefined } else { Value::Number(self.coerce_number(second)?) });
            }
            IndexOf | LastIndexOf | Includes | StartsWith | EndsWith => {
                if matches!(method, Includes | StartsWith | EndsWith) && self.is_regexp(first)? {
                    return Err(RuntimeError::TypeError("String search argument must not be a RegExp".into()));
                }
                converted.push(Value::String(self.coerce_string(first)?));
                converted.push(if matches!(second, Value::Undefined) { Value::Undefined } else { Value::Number(self.coerce_number(second)?) });
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
                converted.push(if matches!(second, Value::Undefined) { Value::Undefined } else { Value::String(self.coerce_string(second)?) });
            }
            Normalize => {
                if !matches!(first, Value::Undefined) {
                    converted.push(Value::String(self.coerce_string(first)?));
                }
            }
            Html { attribute, .. } if !attribute.is_empty() => {
                converted.push(Value::String(self.coerce_string(first)?));
            }
            _ => {}
        }
        native::string_method(method, &Value::String(string), &converted, self.config.max_string_bytes)
    }

    pub(super) fn define_data(
        &mut self,
        owner: ObjectId,
        key: impl Into<PropertyName>,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), RuntimeError> {
        let key = key.into();
        let result = self.with_roots(|heap| heap.define_own_property(owner, key, PropertyDescriptor::data(value, writable, enumerable, configurable)))?;
        assert!(result, "builtin initialization and literal definitions target new or configurable properties");
        Ok(())
    }

    pub(super) fn array_from(&mut self, values: Vec<Value>) -> Result<Value, RuntimeError> {
        let prototype = self.array_prototype;
        let base = self.stack.len();
        self.stack.extend(values.iter().cloned());
        let result = (|| {
            let array = self.with_roots(|heap| heap.alloc_array(values.len() as u32, Some(prototype)))?;
            self.stack.push(Value::Object(array));
            for (index, value) in values.into_iter().enumerate() {
                self.with_roots(|heap| heap.set(array, index.to_string(), value))?;
            }
            Ok(Value::Object(array))
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn iterator_result(&mut self, value: Value, done: bool) -> Result<Value, RuntimeError> {
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

    pub(super) fn global(&mut self, name: &str) -> Result<Value, RuntimeError> {
        self.string_intrinsics()?;
        if matches!(name, "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError" | "URIError") {
            return self.error_global(name);
        }
        if name == "Intl" {
            return self.intl_global();
        }
        if name == "RegExp" {
            return self.regexp_global();
        }
        if name == "Math" {
            return self.math_global();
        }
        if name == "JSON" {
            return self.json_global();
        }
        if let Some(&id) = self.globals.get(name) {
            return Ok(Value::Object(id));
        }
        let constructor = self.string_intrinsics.unwrap().0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let native = match name {
            "Symbol" => NativeFunction::Symbol,
            "Array" => NativeFunction::Array,
            "eval" => NativeFunction::Eval,
            "Object" => NativeFunction::Object,
            "Number" => NativeFunction::PrimitiveConstructor(false),
            "Boolean" => NativeFunction::PrimitiveConstructor(true),
            "isNaN" => NativeFunction::IsNaN,
            "isFinite" => NativeFunction::IsFinite,
            "parseInt" => NativeFunction::ParseInt,
            "parseFloat" => NativeFunction::ParseFloat,
            // The remaining compiler-recognized globals are namespace objects.
            _ => NativeFunction::Empty,
        };
        let object_prototype = self.object_prototype;
        let id = if matches!(name, "Reflect" | "globalThis") {
            self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?
        } else {
            self.with_roots(|heap| heap.alloc_native_function(native, name, prototype))?
        };
        let root = self.heap.root(id)?;
        let result = (|| {
            self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
            self.define_data(id, "length", Value::Number(if name == "Symbol" { 0.0 } else { 1.0 }), false, false, true)?;
            if name == "Symbol" {
                let symbol_prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(id, "prototype", Value::Object(symbol_prototype), false, false, false)?;
                self.define_data(symbol_prototype, "constructor", Value::Object(id), true, false, true)?;
                self.install_native(symbol_prototype, prototype, "toString", 0, NativeFunction::SymbolToString)?;
                self.install_native(symbol_prototype, prototype, "valueOf", 0, NativeFunction::SymbolValueOf)?;
                self.install_symbol_native(symbol_prototype, prototype, "toPrimitive", 1, NativeFunction::SymbolValueOf)?;
                let to_primitive = self.heap.get(symbol_prototype, JsSymbol::well_known("toPrimitive"))?;
                self.define_data(symbol_prototype, JsSymbol::well_known("toPrimitive"), to_primitive, false, false, true)?;
                self.define_data(symbol_prototype, JsSymbol::well_known("toStringTag"), Value::String("Symbol".into()), false, false, true)?;
                for &name in crate::property::WELL_KNOWN {
                    self.define_data(id, name, Value::Symbol(JsSymbol::well_known(name)), false, false, false)?;
                }
            } else if name == "Array" {
                self.define_data(id, "prototype", Value::Object(self.array_prototype), false, false, false)?;
                self.define_data(self.array_prototype, "constructor", Value::Object(id), true, false, true)?;
                self.install_native(id, prototype, "isArray", 1, NativeFunction::ArrayIsArray)?;
            } else if name == "Function" {
                self.define_data(id, "prototype", Value::Object(prototype), false, false, false)?;
            } else if matches!(name, "Number" | "Boolean") {
                let boolean = name == "Boolean";
                let value = if boolean { Value::Bool(false) } else { Value::Number(0.0) };
                let boxed_prototype = self.with_roots(|heap| heap.alloc_boxed_primitive(value, object_prototype))?;
                self.define_data(id, "prototype", Value::Object(boxed_prototype), false, false, false)?;
                self.define_data(boxed_prototype, "constructor", Value::Object(id), true, false, true)?;
                self.install_native(boxed_prototype, prototype, "toString", 0, NativeFunction::PrimitiveMethod { boolean, string: true })?;
                self.install_native(boxed_prototype, prototype, "valueOf", 0, NativeFunction::PrimitiveMethod { boolean, string: false })?;
            } else if name == "globalThis" {
                self.define_data(id, "String", Value::Object(constructor), true, false, true)?;
                self.define_data(id, "globalThis", Value::Object(id), true, false, true)?;
            } else if name == "Reflect" {
                self.install_native(id, prototype, "ownKeys", 1, NativeFunction::ObjectMethod(ObjectMethod::OwnKeys))?;
                self.install_native(id, prototype, "construct", 2, NativeFunction::ReflectConstruct)?;
            } else {
                self.define_data(id, "prototype", Value::Object(self.object_prototype), false, false, false)?;
                use ObjectMethod::*;
                for (name, length, method) in [
                    ("getOwnPropertyDescriptor", 2, GetOwnPropertyDescriptor),
                    ("defineProperty", 3, DefineProperty),
                    ("keys", 1, Keys),
                    ("getOwnPropertyNames", 1, GetOwnPropertyNames),
                    ("getOwnPropertySymbols", 1, GetOwnPropertySymbols),
                    ("getPrototypeOf", 1, GetPrototypeOf),
                    ("setPrototypeOf", 2, SetPrototypeOf),
                    ("create", 2, Create),
                    ("isExtensible", 1, IsExtensible),
                ] {
                    self.install_native(id, prototype, name, length, NativeFunction::ObjectMethod(method))?;
                }
            }
            Ok(Value::Object(id))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert(name.into(), id);
            if name == "globalThis" {
                let globals = self.globals.clone();
                for (global_name, value) in globals {
                    if global_name != "globalThis" {
                        self.define_data(id, global_name, Value::Object(value), true, false, true)?;
                    }
                }
            } else {
                if let Some(&global) = self.globals.get("globalThis") {
                    self.define_data(global, name, Value::Object(id), true, false, true)?;
                }
            }
        }
        result
    }

    pub(super) fn native_call(&mut self, function: NativeFunction, receiver: Value, args: Vec<Value>, construct: bool) -> Result<Value, RuntimeError> {
        let first = native::argument(&args, 0);
        match function {
            NativeFunction::Error(name) => self.error_constructor(name, &args, construct),
            NativeFunction::ErrorToString => self.error_to_string(&receiver),
            NativeFunction::Test262(name) => self.test262_call(name, &args),
            NativeFunction::ToLocaleLowerCase | NativeFunction::ToLocaleUpperCase | NativeFunction::LocaleCompare => {
                let string = self.string_receiver(&receiver)?;
                if function == NativeFunction::LocaleCompare {
                    let other = self.coerce_string(first)?;
                    let collator = self.resolve_collator(native::argument(&args, 1), native::argument(&args, 2))?;
                    Ok(collator.compare(&string, &other))
                } else {
                    let locales = self.canonical_locales(first)?;
                    let locale = locales.first().cloned().unwrap_or(icu_locale_core::locale!("en-US"));
                    crate::intl::case_map(&string, &locale, function == NativeFunction::ToLocaleUpperCase, self.config.max_string_bytes).map(Value::String)
                }
            }
            NativeFunction::Collator => self.create_collator(&args, construct),
            NativeFunction::Locale => self.create_locale(&args, construct),
            NativeFunction::CanonicalLocales => {
                let locales = self.canonical_locales(first)?;
                self.array_from(locales.into_iter().map(|l| Value::String(l.to_string().into())).collect())
            }
            NativeFunction::SupportedLocales => self.supported_locales(&args),
            NativeFunction::CollatorCompareGetter => self.collator_compare_getter(&receiver),
            NativeFunction::CollatorCompare => {
                let collator = self.collator_data(&receiver)?;
                let left = self.coerce_string(first)?;
                let right = self.coerce_string(native::argument(&args, 1))?;
                Ok(collator.compare(&left, &right))
            }
            NativeFunction::CollatorResolvedOptions => self.collator_resolved_options(&receiver),
            NativeFunction::LocaleToString => self.locale_to_string(&receiver),
            NativeFunction::LocaleMaximize => self.locale_transform(&receiver, true),
            NativeFunction::LocaleMinimize => self.locale_transform(&receiver, false),
            NativeFunction::LocaleGetter(name) => self.locale_getter(&receiver, name),
            NativeFunction::LocaleInfo(name) => self.locale_info(&receiver, name),
            NativeFunction::Array => {
                if args.len() == 1 {
                    if let Value::Number(length) = first {
                        let Value::Number(length) = self.array_length_value(&Value::Number(*length))? else { unreachable!() };
                        let prototype = self.array_prototype;
                        return Ok(Value::Object(self.with_roots(|heap| heap.alloc_array(length as u32, Some(prototype)))?));
                    }
                }
                self.array_from(args)
            }
            NativeFunction::ArrayIsArray => Ok(Value::Bool(first.object_id().is_some_and(|id| self.heap.is_array(id).unwrap_or(false)))),
            NativeFunction::ArrayForEach => self.array_for_each(&receiver, first, native::argument(&args, 1)),
            NativeFunction::ArrayIncludes => self.array_includes(&receiver, first, native::argument(&args, 1)),
            NativeFunction::Eval => self.direct_eval(first),
            NativeFunction::IsNaN => Ok(Value::Bool(self.coerce_number(first)?.is_nan())),
            NativeFunction::IsFinite => Ok(Value::Bool(self.coerce_number(first)?.is_finite())),
            NativeFunction::ParseInt => self.parse_int(first, native::argument(&args, 1)),
            NativeFunction::ParseFloat => self.parse_float(first),
            NativeFunction::JsonParse => self.json_parse(first),
            NativeFunction::JsonStringify => self.json_stringify(first),
            NativeFunction::Math(method) => self.math_method(method, &args),
            NativeFunction::Bind => self.bind_function(receiver, &args),
            NativeFunction::HasInstance => self.has_instance(first.clone(), receiver, true).map(Value::Bool),
            NativeFunction::RegExpEscape => self.regexp_escape(first),
            NativeFunction::ArrayIterator => {
                let object = self.coerce_object(&receiver)?;
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| heap.alloc_array_iterator(object, prototype))?))
            }
            NativeFunction::ArrayIteratorNext => {
                let Value::Object(id) = receiver else { return Err(RuntimeError::TypeError("Array iterator next requires an iterator".into())) };
                let Some((object, index, done)) = self.heap.array_iterator(id)? else { return Err(RuntimeError::TypeError("Array iterator next requires an iterator".into())) };
                if done {
                    return self.iterator_result(Value::Undefined, true);
                }
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)?;
                let done = index as f64 >= length;
                self.heap.advance_array_iterator(id, done);
                let value = if done { Value::Undefined } else { self.get_property(&Value::Object(object), &index.to_string().into())? };
                self.iterator_result(value, done)
            }
            NativeFunction::GeneratorNext => self.generator_next(&receiver),
            NativeFunction::GeneratorReturn => self.generator_return(&receiver, first.clone()),
            NativeFunction::Apply => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError("apply requires a callable".into()));
                }
                let list = native::argument(&args, 1);
                let values = if matches!(list, Value::Null | Value::Undefined) { Vec::new() } else { self.array_like_values(list)? };
                self.call_native(receiver, first.clone(), values, false)
            }
            NativeFunction::ReflectConstruct => {
                let new_target = if args.len() > 2 { args[2].clone() } else { first.clone() };
                if !self.is_constructor(first)? || !self.is_constructor(&new_target)? {
                    return Err(RuntimeError::TypeError("Reflect.construct requires constructors".into()));
                }
                let values = self.array_like_values(native::argument(&args, 1))?;
                self.call_with_target(first.clone(), Value::Undefined, values, true, new_target)
            }
            NativeFunction::FunctionToString => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError("Function.toString requires a callable".into()));
                }
                let name = self.heap.function_initial_name(receiver.object_id().unwrap())?;
                let mut result = JsString::from("function ");
                result.push_str(&name);
                result.push_str(&"() { [native code] }".into());
                Ok(Value::String(result))
            }
            NativeFunction::PrimitiveConstructor(boolean) => {
                let value = if boolean { Value::Bool(primitive::truthy(first)) } else { Value::Number(if args.is_empty() { 0.0 } else { self.coerce_number(first)? }) };
                if !construct {
                    return Ok(value);
                }
                let constructor = self.global(if boolean { "Boolean" } else { "Number" })?;
                let default = self.get_property(&constructor, &"prototype".into())?.object_id().unwrap();
                let prototype = self.constructor_prototype(default)?;
                Ok(Value::Object(self.with_roots(|heap| heap.alloc_boxed_primitive(value, prototype))?))
            }
            NativeFunction::PrimitiveMethod { boolean, string } => {
                let value = if let Value::Object(id) = receiver { self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined) } else { receiver };
                if !matches!((&value, boolean), (Value::Bool(_), true) | (Value::Number(_), false)) {
                    return Err(RuntimeError::TypeError("incompatible boxed primitive receiver".into()));
                }
                if string { Ok(Value::String(primitive::string(&value)?)) } else { Ok(value) }
            }
            NativeFunction::SymbolToString | NativeFunction::SymbolValueOf => {
                let value = if let Value::Object(id) = receiver { self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined) } else { receiver };
                let Value::Symbol(symbol) = value else { return Err(RuntimeError::TypeError("Symbol method requires a Symbol".into())) };
                if function == NativeFunction::SymbolToString { Ok(Value::String(symbol.descriptive_string())) } else { Ok(Value::Symbol(symbol)) }
            }
            NativeFunction::RegExp => {
                if !construct && *native::argument(&args, 1) == Value::Undefined && self.is_regexp(first)? {
                    let constructor = self.get_property(first, &"constructor".into())?;
                    if constructor == self.regexp_global()? {
                        return Ok(first.clone());
                    }
                }
                self.regexp_create(first, native::argument(&args, 1))
            }
            NativeFunction::RegExpMethod(method) => self.regexp_method(method, &receiver, &args),
            NativeFunction::RegExpGetter(name) => self.regexp_getter(name, &receiver),
            NativeFunction::RegExpIteratorNext => self.regexp_iterator_next(&receiver),
            NativeFunction::Empty => Ok(Value::Undefined),
            NativeFunction::ObjectValueOf => self.coerce_object(&receiver).map(Value::Object),
            NativeFunction::ObjectToString => {
                let tag = match &receiver {
                    Value::Undefined => "Undefined",
                    Value::Null => "Null",
                    Value::String(_) => "String",
                    Value::Symbol(_) => "Symbol",
                    Value::Number(_) => "Number",
                    Value::Bool(_) => "Boolean",
                    Value::Object(id) => {
                        if self.heap.boxed_string(*id)?.is_some() {
                            "String"
                        } else if self.heap.is_array(*id)? {
                            "Array"
                        } else if self.is_callable(&receiver)? {
                            "Function"
                        } else if self.heap.regexp(*id)?.is_some() {
                            "RegExp"
                        } else if let Some(value) = self.heap.boxed_primitive(*id)? {
                            match value {
                                Value::Number(_) => "Number",
                                Value::Bool(_) => "Boolean",
                                _ => "Symbol",
                            }
                        } else {
                            "Object"
                        }
                    }
                };
                let custom =
                    if matches!(receiver, Value::Undefined | Value::Null) { Value::Undefined } else { self.get_property(&receiver, &JsSymbol::well_known("toStringTag").into())? };
                let mut result = JsString::from("[object ");
                result.push_str(&if let Value::String(custom) = custom { custom } else { tag.into() });
                result.push_str(&"]".into());
                Ok(Value::String(result))
            }
            NativeFunction::ArrayToString => {
                let object = Value::Object(self.coerce_object(&receiver)?);
                self.stack.push(object.clone());
                let join = self.get_property(&object, &"join".into())?;
                if self.is_callable(&join)? { self.call_native(join, object, vec![], false) } else { self.native_call(NativeFunction::ObjectToString, object, vec![], false) }
            }
            NativeFunction::ArrayConcat => self.array_concat(&receiver, &args),
            NativeFunction::ArrayJoin => self.array_join(&receiver, first),
            NativeFunction::Symbol => Ok(Value::Symbol(JsSymbol::new(if matches!(first, Value::Undefined) { None } else { Some(self.coerce_string(first)?) }))),
            NativeFunction::Object => {
                if matches!(first, Value::Undefined | Value::Null) {
                    let proto = self.object_prototype;
                    return Ok(Value::Object(self.with_roots(|heap| heap.alloc_object(Some(proto)))?));
                }
                self.coerce_object(first).map(Value::Object)
            }
            NativeFunction::ObjectMethod(method) => self.object_method(method, &args),
            NativeFunction::StringIterator => {
                let string = self.string_receiver(&receiver)?;
                let prototype = self.string_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| heap.alloc_string_iterator(string, prototype))?))
            }
            NativeFunction::IteratorNext => {
                let Value::Object(id) = receiver else { return Err(RuntimeError::TypeError("iterator next requires an iterator".into())) };
                let Some(value) = self.heap.string_iterator_next(id)? else { return Err(RuntimeError::TypeError("iterator next requires a String iterator".into())) };
                let done = value.is_none();
                self.iterator_result(value.map_or(Value::Undefined, Value::String), done)
            }
            NativeFunction::IteratorSelf => Ok(receiver),
            NativeFunction::Pattern(method) => self.string_pattern(method, &receiver, &args),
            NativeFunction::String => {
                let string = if args.is_empty() { JsString::default() } else { self.string_constructor_argument(native::argument(&args, 0), construct)? };
                self.check_string(&Value::String(string.clone()))?;
                if construct {
                    let (_, prototype) = self.string_intrinsics()?;
                    let prototype = self.constructor_prototype(prototype)?;
                    Ok(Value::Object(self.with_roots(|heap| heap.alloc_string(string, Some(prototype)))?))
                } else {
                    Ok(Value::String(string))
                }
            }
            NativeFunction::FromCharCode | NativeFunction::FromCodePoint => {
                let mut result = JsString::default();
                for arg in &args {
                    let number = Value::Number(self.coerce_number(arg)?);
                    let Value::String(part) = native::from_codes(&[number], function == NativeFunction::FromCodePoint, self.config.max_string_bytes)? else { unreachable!() };
                    native::append(&mut result, &part, self.config.max_string_bytes)?;
                }
                Ok(Value::String(result))
            }
            NativeFunction::Raw => self.string_raw(&args),
            NativeFunction::Split => self.string_split(&receiver, &args),
            NativeFunction::Replace | NativeFunction::ReplaceAll => self.string_replace(&receiver, &args, function == NativeFunction::ReplaceAll),
            NativeFunction::StringMethod(method) => self.dispatch_string_method(method, &receiver, &args),
            NativeFunction::Call => self.call_native(receiver, first.clone(), args.iter().skip(1).cloned().collect(), false),
        }
    }

    fn array_join(&mut self, receiver: &Value, separator: &Value) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let separator = if *separator == Value::Undefined { ",".into() } else { self.coerce_string(separator)? };
        if self.joining.contains(&object) {
            return Ok(Value::String(JsString::default()));
        }
        self.joining.push(object);
        let result = (|| {
            let mut result = JsString::default();
            for index in 0..length {
                self.charge_step()?;
                if index > 0 {
                    native::append(&mut result, &separator, self.config.max_string_bytes)?;
                }
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                if !matches!(value, Value::Null | Value::Undefined) {
                    let value = self.coerce_string(&value)?;
                    native::append(&mut result, &value, self.config.max_string_bytes)?;
                }
            }
            Ok(Value::String(result))
        })();
        self.joining.pop();
        result
    }

    fn array_concat(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        let mut values = Vec::new();
        for value in std::iter::once(receiver).chain(args) {
            if let Some(object) = value.object_id().filter(|id| self.heap.is_array(*id).unwrap_or(false)) {
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)? as u64;
                for index in 0..length {
                    self.charge_step()?;
                    values.push(self.get_property(&Value::Object(object), &index.to_string().into())?);
                }
            } else {
                values.push(value.clone());
            }
        }
        self.array_from(values)
    }

    fn array_for_each(&mut self, receiver: &Value, callback: &Value, this_arg: &Value) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError("Array.prototype.forEach callback must be callable".into()));
        }
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        if let Some(indices) = self.array_own_indices(object, length)? {
            for index in indices {
                self.charge_step()?;
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                self.call_native(callback.clone(), this_arg.clone(), vec![value, Value::Number(index as f64), Value::Object(object)], false)?;
            }
        } else {
            for index in 0..length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                let mut current = Some(object);
                let mut present = false;
                while let Some(id) = current {
                    if self.heap.get_own_property_descriptor(id, &key)?.is_some() {
                        present = true;
                        break;
                    }
                    current = self.heap.prototype(id)?;
                }
                if present {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    self.call_native(callback.clone(), this_arg.clone(), vec![value, Value::Number(index as f64), Value::Object(object)], false)?;
                }
            }
        }
        self.stack.pop();
        Ok(Value::Undefined)
    }

    fn array_own_indices(&self, object: ObjectId, length: u64) -> Result<Option<Vec<u32>>, RuntimeError> {
        // Scanning ordinary arrays preserves properties added by callbacks.  This
        // shortcut is only for the large sparse arrays that would otherwise turn
        // a bounded operation into millions of empty property lookups.
        if length < 65_536 || !self.heap.is_array(object)? {
            return Ok(None);
        }
        let mut prototype = self.heap.prototype(object)?;
        while let Some(id) = prototype {
            if self.heap.own_property_keys(id)?.iter().any(
                |key| matches!(key, PropertyName::String(name) if name.to_utf8().ok().and_then(|name| name.parse::<u32>().ok()).is_some_and(|index| u64::from(index) < length)),
            ) {
                return Ok(None);
            }
            prototype = self.heap.prototype(id)?;
        }
        let mut indices: Vec<_> = self
            .heap
            .own_property_keys(object)?
            .into_iter()
            .filter_map(|key| match key {
                PropertyName::String(name) => name.to_utf8().ok().and_then(|name| name.parse::<u32>().ok()).filter(|index| u64::from(*index) < length),
                PropertyName::Symbol(_) => None,
            })
            .collect();
        indices.sort_unstable();
        Ok(Some(indices))
    }

    fn array_includes(&mut self, receiver: &Value, search: &Value, from_index: &Value) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as i64;
        let from_index = if *from_index == Value::Undefined { 0 } else { self.coerce_number(from_index)? as i64 };
        let mut index = if from_index < 0 { (length + from_index).max(0) } else { from_index.min(length) };
        while index < length {
            self.charge_step()?;
            let value = self.get_property(&Value::Object(object), &(index as u64).to_string().into())?;
            if value == *search || matches!((&value, search), (Value::Number(left), Value::Number(right)) if left.is_nan() && right.is_nan()) {
                self.stack.pop();
                return Ok(Value::Bool(true));
            }
            index += 1;
        }
        self.stack.pop();
        Ok(Value::Bool(false))
    }

    fn math_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("Math") {
            return Ok(Value::Object(id));
        }
        let function_prototype = self.string_intrinsics()?.1;
        let object_prototype = self.object_prototype;
        let math = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(math)?;
        let result = (|| {
            for (name, value) in [
                ("E", std::f64::consts::E),
                ("LN10", std::f64::consts::LN_10),
                ("LN2", std::f64::consts::LN_2),
                ("LOG10E", std::f64::consts::LOG10_E),
                ("LOG2E", std::f64::consts::LOG2_E),
                ("PI", std::f64::consts::PI),
                ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
                ("SQRT2", std::f64::consts::SQRT_2),
            ] {
                self.define_data(math, name, Value::Number(value), false, false, false)?;
            }
            for (name, length, method) in [
                ("abs", 1, MathMethod::Abs),
                ("acos", 1, MathMethod::Acos),
                ("acosh", 1, MathMethod::Acosh),
                ("asin", 1, MathMethod::Asin),
                ("asinh", 1, MathMethod::Asinh),
                ("atan", 1, MathMethod::Atan),
                ("atanh", 1, MathMethod::Atanh),
                ("atan2", 2, MathMethod::Atan2),
                ("ceil", 1, MathMethod::Ceil),
                ("cbrt", 1, MathMethod::Cbrt),
                ("clz32", 1, MathMethod::Clz32),
                ("cos", 1, MathMethod::Cos),
                ("cosh", 1, MathMethod::Cosh),
                ("exp", 1, MathMethod::Exp),
                ("expm1", 1, MathMethod::Expm1),
                ("floor", 1, MathMethod::Floor),
                ("fround", 1, MathMethod::Fround),
                ("hypot", 2, MathMethod::Hypot),
                ("imul", 2, MathMethod::Imul),
                ("log", 1, MathMethod::Log),
                ("log1p", 1, MathMethod::Log1p),
                ("log2", 1, MathMethod::Log2),
                ("log10", 1, MathMethod::Log10),
                ("max", 2, MathMethod::Max),
                ("min", 2, MathMethod::Min),
                ("pow", 2, MathMethod::Pow),
                ("random", 0, MathMethod::Random),
                ("round", 1, MathMethod::Round),
                ("sign", 1, MathMethod::Sign),
                ("sin", 1, MathMethod::Sin),
                ("sinh", 1, MathMethod::Sinh),
                ("sqrt", 1, MathMethod::Sqrt),
                ("tan", 1, MathMethod::Tan),
                ("tanh", 1, MathMethod::Tanh),
                ("trunc", 1, MathMethod::Trunc),
            ] {
                self.install_native(math, function_prototype, name, length, NativeFunction::Math(method))?;
            }
            self.define_data(math, JsSymbol::well_known("toStringTag"), Value::String("Math".into()), false, false, true)?;
            Ok(Value::Object(math))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert("Math".into(), math);
        }
        result
    }

    fn math_method(&mut self, method: MathMethod, args: &[Value]) -> Result<Value, RuntimeError> {
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        let number = |value: &Value, vm: &mut Self| vm.coerce_number(value);
        let result = match method {
            MathMethod::Max | MathMethod::Min => {
                let mut result = if method == MathMethod::Max { f64::NEG_INFINITY } else { f64::INFINITY };
                for value in args {
                    let value = number(value, self)?;
                    if value.is_nan() {
                        return Ok(Value::Number(f64::NAN));
                    }
                    if value == 0.0 && result == 0.0 {
                        if (method == MathMethod::Max && value.is_sign_positive()) || (method == MathMethod::Min && value.is_sign_negative()) {
                            result = value;
                        }
                    } else if (method == MathMethod::Max && value > result) || (method == MathMethod::Min && value < result) {
                        result = value;
                    }
                }
                result
            }
            MathMethod::Hypot => {
                let mut values = Vec::with_capacity(args.len());
                for value in args {
                    let value = number(value, self)?.abs();
                    if value.is_infinite() {
                        return Ok(Value::Number(f64::INFINITY));
                    }
                    values.push(value);
                }
                if values.iter().any(|value| value.is_nan()) {
                    f64::NAN
                } else {
                    let scale = values.iter().copied().fold(0.0_f64, f64::max);
                    if scale == 0.0 { 0.0 } else { scale * values.iter().map(|value| (value / scale).powi(2)).sum::<f64>().sqrt() }
                }
            }
            MathMethod::Imul => {
                let left = math_uint32(number(first, self)?);
                let right = math_uint32(number(second, self)?);
                (left as i32).wrapping_mul(right as i32) as f64
            }
            MathMethod::Clz32 => math_uint32(number(first, self)?).leading_zeros() as f64,
            MathMethod::Atan2 => number(first, self)?.atan2(number(second, self)?),
            MathMethod::Pow => number(first, self)?.powf(number(second, self)?),
            MathMethod::Random => {
                let elapsed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                (elapsed.as_nanos() % 1_000_000_000) as f64 / 1_000_000_000.0
            }
            MathMethod::Round => {
                let value = number(first, self)?;
                if value.is_nan() || !value.is_finite() || value == 0.0 {
                    value
                } else if (-0.5..0.5).contains(&value) {
                    if value.is_sign_negative() { -0.0 } else { 0.0 }
                } else {
                    (value + 0.5).floor()
                }
            }
            MathMethod::Sign => {
                let value = number(first, self)?;
                if value.is_nan() || value == 0.0 { value } else { value.signum() }
            }
            MathMethod::Abs => {
                let value = number(first, self)?;
                value.abs()
            }
            MathMethod::Acos => number(first, self)?.acos(),
            MathMethod::Acosh => number(first, self)?.acosh(),
            MathMethod::Asin => number(first, self)?.asin(),
            MathMethod::Asinh => number(first, self)?.asinh(),
            MathMethod::Atan => number(first, self)?.atan(),
            MathMethod::Atanh => number(first, self)?.atanh(),
            MathMethod::Ceil => number(first, self)?.ceil(),
            MathMethod::Cbrt => number(first, self)?.cbrt(),
            MathMethod::Cos => number(first, self)?.cos(),
            MathMethod::Cosh => number(first, self)?.cosh(),
            MathMethod::Exp => number(first, self)?.exp(),
            MathMethod::Expm1 => number(first, self)?.exp_m1(),
            MathMethod::Floor => number(first, self)?.floor(),
            MathMethod::Fround => (number(first, self)? as f32) as f64,
            MathMethod::Log => number(first, self)?.ln(),
            MathMethod::Log1p => number(first, self)?.ln_1p(),
            MathMethod::Log2 => number(first, self)?.log2(),
            MathMethod::Log10 => number(first, self)?.log10(),
            MathMethod::Sin => number(first, self)?.sin(),
            MathMethod::Sinh => number(first, self)?.sinh(),
            MathMethod::Sqrt => number(first, self)?.sqrt(),
            MathMethod::Tan => number(first, self)?.tan(),
            MathMethod::Tanh => number(first, self)?.tanh(),
            MathMethod::Trunc => number(first, self)?.trunc(),
        };
        Ok(Value::Number(result))
    }

    pub(super) fn coerce_object(&mut self, value: &Value) -> Result<ObjectId, RuntimeError> {
        match value {
            Value::Object(id) => Ok(*id),
            Value::String(s) => {
                let (_, prototype) = self.string_intrinsics()?;
                Ok(self.with_roots(|heap| heap.alloc_string(s.clone(), Some(prototype)))?)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError("cannot convert null or undefined to Object".into())),
            _ => {
                let constructor = self.global(match value {
                    Value::Symbol(_) => "Symbol",
                    Value::Bool(_) => "Boolean",
                    _ => "Number",
                })?;
                let prototype = self.get_property(&constructor, &"prototype".into())?.object_id().unwrap();
                Ok(self.with_roots(|heap| heap.alloc_boxed_primitive(value.clone(), prototype))?)
            }
        }
    }

    fn object_method(&mut self, method: ObjectMethod, args: &[Value]) -> Result<Value, RuntimeError> {
        use ObjectMethod::*;
        let first = native::argument(args, 0);
        if method == IsExtensible && !matches!(first, Value::Object(_)) {
            return Ok(Value::Bool(false));
        }
        if matches!(method, DefineProperty | OwnKeys) && !matches!(first, Value::Object(_)) {
            return Err(RuntimeError::TypeError("operation requires an object".into()));
        }
        let object = if method == Create {
            let prototype = match first {
                Value::Null => None,
                Value::Object(id) => Some(*id),
                _ => return Err(RuntimeError::TypeError("Object.create prototype must be object or null".into())),
            };
            self.with_roots(|heap| heap.alloc_object(prototype))?
        } else {
            self.coerce_object(first)?
        };
        self.stack.push(Value::Object(object));
        match method {
            GetOwnPropertyDescriptor | DefineProperty => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                if method == DefineProperty {
                    let mut descriptor = self.read_descriptor(native::argument(args, 2))?;
                    if key == "length" && self.heap.is_array(object)? {
                        if let Some(value) = &descriptor.value {
                            descriptor.value = Some(self.array_length_value(value)?);
                        }
                    }
                    if !self.with_roots(|heap| heap.define_own_property(object, key, descriptor))? {
                        return Err(RuntimeError::TypeError("cannot redefine property".into()));
                    }
                    return Ok(Value::Object(object));
                }
                let Some(descriptor) = self.heap.get_own_property_descriptor(object, key)? else { return Ok(Value::Undefined) };
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
            Keys | GetOwnPropertyNames | GetOwnPropertySymbols | OwnKeys => {
                let keys = self.heap.own_property_keys(object)?;
                let mut values = Vec::new();
                for key in keys {
                    if method == Keys && (!matches!(key, PropertyName::String(_)) || self.heap.get_own_property_descriptor(object, &key)?.unwrap().enumerable != Some(true)) {
                        continue;
                    }
                    if method == GetOwnPropertyNames && !matches!(key, PropertyName::String(_)) || method == GetOwnPropertySymbols && !matches!(key, PropertyName::Symbol(_)) {
                        continue;
                    }
                    values.push(key.value());
                }
                self.array_from(values)
            }
            GetPrototypeOf => Ok(self.heap.prototype(object)?.map_or(Value::Null, Value::Object)),
            SetPrototypeOf => {
                let prototype = match native::argument(args, 1) {
                    Value::Null => None,
                    Value::Object(id) => Some(*id),
                    _ => return Err(RuntimeError::TypeError("prototype must be object or null".into())),
                };
                match self.heap.set_prototype(object, prototype) {
                    Err(HeapError::PrototypeCycle | HeapError::ReadOnlyProperty) => return Err(RuntimeError::TypeError("cannot set object prototype".into())),
                    result => result?,
                }
                Ok(first.clone())
            }
            Create => {
                let properties = native::argument(args, 1);
                if *properties != Value::Undefined {
                    let properties = self.coerce_object(properties)?;
                    self.stack.push(Value::Object(properties));
                    let mut descriptors = Vec::new();
                    for key in self.heap.own_property_keys(properties)? {
                        if self.heap.get_own_property_descriptor(properties, &key)?.is_some_and(|d| d.enumerable == Some(true)) {
                            let value = self.get_property(&Value::Object(properties), &key)?;
                            self.stack.push(value.clone());
                            descriptors.push((key, self.read_descriptor(&value)?));
                        }
                    }
                    for (key, descriptor) in descriptors {
                        self.with_roots(|heap| heap.define_own_property(object, key, descriptor))?;
                    }
                }
                Ok(Value::Object(object))
            }
            IsExtensible => Ok(Value::Bool(self.heap.is_extensible(object)?)),
        }
    }

    fn read_descriptor(&mut self, value: &Value) -> Result<PropertyDescriptor, RuntimeError> {
        let Value::Object(object) = value else { return Err(RuntimeError::TypeError("descriptor must be an object".into())) };
        let mut descriptor = PropertyDescriptor::default();
        for name in ["enumerable", "configurable", "value", "writable", "get", "set"] {
            let mut current = Some(*object);
            let mut present = false;
            while let Some(id) = current {
                if self.heap.get_own_property_descriptor(id, name)?.is_some() {
                    present = true;
                    break;
                }
                current = self.heap.prototype(id)?;
            }
            if !present {
                continue;
            }
            let property = self.get_property(value, &name.into())?;
            // Later descriptor getters may allocate and collect earlier values.
            self.stack.push(property.clone());
            match name {
                "enumerable" => descriptor.enumerable = Some(primitive::truthy(&property)),
                "configurable" => descriptor.configurable = Some(primitive::truthy(&property)),
                "writable" => descriptor.writable = Some(primitive::truthy(&property)),
                "value" => descriptor.value = Some(property),
                _ => {
                    if property != Value::Undefined && !self.is_callable(&property)? {
                        return Err(RuntimeError::TypeError("accessor must be callable or undefined".into()));
                    }
                    if name == "get" {
                        descriptor.get = Some(property);
                    } else {
                        descriptor.set = Some(property);
                    }
                }
            }
        }
        if descriptor.accessor() && (descriptor.value.is_some() || descriptor.writable.is_some()) {
            return Err(RuntimeError::TypeError("invalid mixed property descriptor".into()));
        }
        Ok(descriptor)
    }

    fn string_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let iterator_base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(iterator_base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(prototype, function_prototype, "next", 0, NativeFunction::IteratorNext)?;
            self.define_data(prototype, JsSymbol::well_known("toStringTag"), Value::String("String Iterator".into()), false, false, true)?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.iterator_prototype = Some(prototype);
        }
        result
    }

    pub(super) fn install_symbol_native(&mut self, owner: ObjectId, prototype: ObjectId, symbol: &str, length: u32, native: NativeFunction) -> Result<(), RuntimeError> {
        let name = format!("[Symbol.{symbol}]");
        let id = self.with_roots(|heap| heap.alloc_native_function(native, &name, prototype))?;
        self.stack.push(Value::Object(id));
        self.define_data(id, "name", Value::String(format!("[Symbol.{symbol}]").into()), false, false, true)?;
        self.define_data(id, "length", Value::Number(length as f64), false, false, true)?;
        // Function.prototype @@hasInstance is the immutable Symbol method.
        let mutable = native != NativeFunction::HasInstance;
        self.define_data(owner, JsSymbol::well_known(symbol), Value::Object(id), mutable, false, mutable)?;
        self.stack.pop();
        Ok(())
    }

    fn string_pattern(&mut self, method: PatternMethod, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError("String method requires a non-null receiver".into()));
        }
        let pattern = native::argument(args, 0);
        let symbol = match method {
            PatternMethod::Match => "match",
            PatternMethod::MatchAll => "matchAll",
            PatternMethod::Search => "search",
        };
        if !matches!(pattern, Value::Null | Value::Undefined) {
            if method == PatternMethod::MatchAll {
                self.require_global_pattern(pattern)?;
            }
            let method = self.get_method(pattern, &JsSymbol::well_known(symbol).into())?;
            if method != Value::Undefined {
                return self.call_native(method, pattern.clone(), vec![receiver.clone()], false);
            }
        }
        let string = self.coerce_string(receiver)?;
        let regexp = self.regexp_create(pattern, &if method == PatternMethod::MatchAll { Value::String("g".into()) } else { Value::Undefined })?;
        self.stack.push(regexp.clone());
        let function = self.get_property(&regexp, &JsSymbol::well_known(symbol).into())?;
        self.call_native(function, regexp, vec![Value::String(string)], false)
    }
}
