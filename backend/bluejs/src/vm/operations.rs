// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! String, numeric, comparison, and object-environment VM operations.

use super::*;

impl Vm {
    pub(super) fn unbox_string(&self, value: &Value) -> Result<Value, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(string) = self.heap.boxed_string(*id)? {
                return Ok(Value::String(string.clone()));
            }
        }
        Ok(value.clone())
    }

    pub(super) fn string_receiver(&mut self, value: &Value) -> Result<JsString, RuntimeError> {
        if matches!(value, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        self.coerce_string(value)
    }

    pub(super) fn string_raw(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let template = Value::Object(self.coerce_object(native::argument(args, 0))?);
        self.stack.push(template.clone());
        let raw = self.get_property(&template, &"raw".into())?;
        self.stack.push(raw.clone());
        let raw = Value::Object(self.coerce_object(&raw)?);
        self.stack.push(raw.clone());
        let length = self.get_property(&raw, &"length".into())?;
        let count = self.coerce_length(&length)? as u64;
        let mut result = JsString::default();
        for index in 0..count {
            self.charge_step()?; // A huge array-like length with empty entries must still terminate.
            let literal = self.get_property(&raw, &index.to_string().into())?;
            native::append(
                &mut result,
                &self.coerce_string(&literal)?,
                self.config.max_string_bytes,
            )?;
            if index + 1 < count {
                if let Some(substitution) = args.get(index as usize + 1) {
                    native::append(
                        &mut result,
                        &self.coerce_string(substitution)?,
                        self.config.max_string_bytes,
                    )?;
                }
            }
        }
        Ok(Value::String(result))
    }

    pub(super) fn string_split(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let separator = native::argument(args, 0);
        if !matches!(separator, Value::Null | Value::Undefined) {
            let method = self.get_method(separator, &JsSymbol::well_known("split").into())?;
            if method != Value::Undefined {
                return self.call_native(
                    method,
                    separator.clone(),
                    vec![receiver.clone(), native::argument(args, 1).clone()],
                    false,
                );
            }
        }
        let string = self.string_receiver(receiver)?;
        let separator = native::argument(args, 0);
        let limit = native::argument(args, 1);
        let limit = if matches!(limit, Value::Undefined) {
            u32::MAX
        } else {
            native::uint32(&Value::Number(self.coerce_number(limit)?))?
        };
        // Even a zero limit converts the separator first (§22.1.3.23).
        let search = self.coerce_string(separator)?;
        let prototype = self.array_prototype;
        let array = self.with_roots(|heap| heap.alloc_array(0, Some(prototype)))?;
        self.stack.push(Value::Object(array)); // Root the growing result through every store.
        let mut count = 0u32;
        let units = string.as_code_units();
        let mut start = 0;
        while count < limit {
            self.charge_step()?;
            let end = if matches!(separator, Value::Undefined) {
                units.len()
            } else if search.is_empty() {
                if start == units.len() {
                    break;
                }
                start + 1
            } else if search.len() <= units.len() {
                (start..=units.len() - search.len())
                    .find(|&index| units[index..].starts_with(search.as_code_units()))
                    .unwrap_or(units.len())
            } else {
                units.len()
            };
            let value = Value::String(JsString::from_code_units(units[start..end].to_vec()));
            self.with_roots(|heap| heap.set(array, count.to_string(), value))?;
            count += 1;
            if end == units.len() {
                break;
            }
            start = end + search.len();
        }
        Ok(Value::Object(array))
    }

    pub(super) fn string_replace(
        &mut self,
        receiver: &Value,
        args: &[Value],
        all: bool,
    ) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let search = native::argument(args, 0);
        if !matches!(search, Value::Null | Value::Undefined) {
            if all {
                self.require_global_pattern(search)?;
            }
            let method = self.get_method(search, &JsSymbol::well_known("replace").into())?;
            if method != Value::Undefined {
                return self.call_native(
                    method,
                    search.clone(),
                    vec![receiver.clone(), native::argument(args, 1).clone()],
                    false,
                );
            }
        }
        let string = self.string_receiver(receiver)?;
        let search = self.coerce_string(native::argument(args, 0))?;
        let replace = native::argument(args, 1);
        let callable = self.is_callable(replace)?;
        let template = if callable {
            JsString::default()
        } else {
            self.coerce_string(replace)?
        };
        let mut result = JsString::default();
        let mut end = 0;
        let mut next = 0;
        if search.len() <= string.len() {
            while let Some(position) = (next..=string.len() - search.len())
                .find(|&index| string.as_code_units()[index..].starts_with(search.as_code_units()))
            {
                self.charge_step()?;
                let replacement = if callable {
                    let value = self.call_native(
                        replace.clone(),
                        Value::Undefined,
                        vec![
                            Value::String(search.clone()),
                            Value::Number(position as f64),
                            Value::String(string.clone()),
                        ],
                        false,
                    )?;
                    self.coerce_string(&value)?
                } else {
                    native::substitution(
                        &string,
                        &search,
                        position,
                        &template,
                        self.config.max_string_bytes,
                    )?
                };
                native::append(
                    &mut result,
                    &JsString::from_code_units(string.as_code_units()[end..position].to_vec()),
                    self.config.max_string_bytes,
                )?;
                native::append(&mut result, &replacement, self.config.max_string_bytes)?;
                end = position + search.len();
                if !all {
                    break;
                }
                next = position + search.len().max(1);
            }
        }
        native::append(
            &mut result,
            &JsString::from_code_units(string.as_code_units()[end..].to_vec()),
            self.config.max_string_bytes,
        )?;
        Ok(Value::String(result))
    }

    pub(super) fn binary(
        &mut self,
        operation: impl FnOnce(&mut Self, Value, Value) -> Result<Value, RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 2;
        let value = operation(self, self.stack[base].clone(), self.stack[base + 1].clone())?;
        self.stack.truncate(base);
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn numeric(&mut self, operation: fn(f64, f64) -> f64) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| {
            Ok(Value::Number(operation(
                vm.coerce_number(&a)?,
                vm.coerce_number(&b)?,
            )))
        })
    }

    pub(super) fn exponentiate(&mut self) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    // libm's powf returns 1 for ±1 raised to ±∞, whereas
                    // Number::exponentiate explicitly specifies NaN for
                    // that pair.
                    let value = if right.is_infinite() && left.abs() == 1.0 {
                        f64::NAN
                    } else {
                        left.powf(right)
                    };
                    Ok(Value::Number(value))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    Ok(Value::BigInt(bigint_exponentiate(left, right)?))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in an exponentiation operation".into(),
                )),
            }
        })
    }

    pub(super) fn negate(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        Ok(match self.coerce_numeric(value)? {
            primitive::Numeric::Number(value) => Value::Number(-value),
            primitive::Numeric::BigInt(value) => Value::BigInt(-value),
        })
    }

    pub(super) fn bit_not(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        Ok(match self.coerce_numeric(value)? {
            primitive::Numeric::Number(value) => {
                Value::Number((!primitive::to_int32(value)) as f64)
            }
            primitive::Numeric::BigInt(value) => Value::BigInt(!value),
        })
    }

    pub(super) fn bitwise(&mut self, operation: Opcode) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    let left = primitive::to_int32(left);
                    let right = primitive::to_int32(right);
                    let value = match operation {
                        Opcode::BitAnd => left & right,
                        Opcode::BitXor => left ^ right,
                        Opcode::BitOr => left | right,
                        _ => unreachable!("bitwise caller selects a bitwise opcode"),
                    };
                    Ok(Value::Number(value as f64))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    let value = match operation {
                        Opcode::BitAnd => left & right,
                        Opcode::BitXor => left ^ right,
                        Opcode::BitOr => left | right,
                        _ => unreachable!("bitwise caller selects a bitwise opcode"),
                    };
                    Ok(Value::BigInt(value))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in a bitwise operation".into(),
                )),
            }
        })
    }

    pub(super) fn shift(&mut self, operation: Opcode) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    let left = primitive::to_int32(left);
                    let right = primitive::to_uint32(right) & 0x1f;
                    let value = match operation {
                        Opcode::ShiftLeft => left.wrapping_shl(right) as f64,
                        Opcode::ShiftRight => (left >> right) as f64,
                        Opcode::UnsignedShiftRight => ((left as u32) >> right) as f64,
                        _ => unreachable!("shift caller selects a shift opcode"),
                    };
                    Ok(Value::Number(value))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    if operation == Opcode::UnsignedShiftRight {
                        return Err(RuntimeError::TypeError(
                            "BigInt does not support unsigned right shift".into(),
                        ));
                    }
                    Ok(Value::BigInt(bigint_shift(
                        left,
                        right,
                        operation == Opcode::ShiftLeft,
                    )?))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in a shift operation".into(),
                )),
            }
        })
    }

    pub(super) fn relational(&mut self, accept: fn(Ordering) -> bool) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| {
            let a = vm.coerce_primitive(&a, "number")?;
            let b = vm.coerce_primitive(&b, "number")?;
            Ok(Value::Bool(primitive::compare(&a, &b)?.is_some_and(accept)))
        })
    }

    pub(super) fn loose_equal(&mut self, left: Value, right: Value) -> Result<bool, RuntimeError> {
        if std::mem::discriminant(&left) == std::mem::discriminant(&right) {
            return Ok(left == right);
        }
        if matches!(right, Value::Null | Value::Undefined)
            && matches!(&left, Value::Object(object) if self.heap.is_html_dda(*object)?)
        {
            return Ok(true);
        }
        if matches!(left, Value::Null | Value::Undefined)
            && matches!(&right, Value::Object(object) if self.heap.is_html_dda(*object)?)
        {
            return Ok(true);
        }
        if matches!(
            (&left, &right),
            (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
        ) {
            return Ok(true);
        }
        match (left, right) {
            (Value::Number(left), Value::String(right)) => {
                Ok(left == primitive::number(&Value::String(right))?)
            }
            (Value::String(left), Value::Number(right)) => {
                Ok(primitive::number(&Value::String(left))? == right)
            }
            (Value::Bool(left), right) => {
                self.loose_equal(Value::Number(if left { 1.0 } else { 0.0 }), right)
            }
            (left, Value::Bool(right)) => {
                self.loose_equal(left, Value::Number(if right { 1.0 } else { 0.0 }))
            }
            (
                Value::Object(left),
                right @ (Value::Number(_) | Value::String(_) | Value::Symbol(_)),
            ) => {
                let left = self.coerce_primitive(&Value::Object(left), "default")?;
                self.loose_equal(left, right)
            }
            (
                left @ (Value::Number(_) | Value::String(_) | Value::Symbol(_)),
                Value::Object(right),
            ) => {
                let right = self.coerce_primitive(&Value::Object(right), "default")?;
                self.loose_equal(left, right)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn property_in(
        &mut self,
        key: &Value,
        object: &Value,
    ) -> Result<bool, RuntimeError> {
        let Value::Object(object) = object else {
            return Err(RuntimeError::TypeError(
                "right operand of in must be an object".into(),
            ));
        };
        let key = self.coerce_property_key(key)?;
        if self.heap.proxy(*object)?.is_some() {
            return self.proxy_has(*object, &key);
        }
        self.has_property(*object, &key)
    }

    /// Object Environment Record HasBinding. `with` lookup first observes the
    /// target object's property chain, then gives an object-valued
    /// `Symbol.unscopables` a chance to hide that name from lexical lookup.
    pub(super) fn with_has_binding(
        &mut self,
        object: &Value,
        name: &str,
    ) -> Result<bool, RuntimeError> {
        let key = Value::String(name.into());
        if !self.property_in(&key, object)? {
            return Ok(false);
        }
        let unscopables = self.get_property(object, &JsSymbol::well_known("unscopables").into())?;
        if !matches!(unscopables, Value::Object(_)) {
            return Ok(true);
        }
        // The getter for an unscopables entry can allocate. Keep its receiver
        // live on the VM stack rather than relying on an unrooted Rust Value.
        self.stack.push(unscopables);
        let result = (|| {
            let unscopables = self.stack.last().expect("unscopables is rooted").clone();
            let blocked = self.get_property(&unscopables, &name.into())?;
            Ok(!self.to_boolean(&blocked)?)
        })();
        self.stack.pop();
        result
    }

    pub(super) fn with_get(
        &mut self,
        name: &str,
        fallback: Option<Option<Value>>,
    ) -> Result<Value, RuntimeError> {
        for object in self.with_objects.clone().into_iter().rev() {
            if self.with_has_binding(&object, name)? {
                return self.get_property(&object, &name.into());
            }
        }
        match fallback {
            Some(Some(value)) => Ok(value),
            Some(None) => Err(RuntimeError::ReferenceError(name.into())),
            None => self
                .lookup_global_name(name)?
                .ok_or_else(|| RuntimeError::ReferenceError(name.into())),
        }
    }

    pub(super) fn with_set(&mut self, name: &str, value: Value) -> Result<(), RuntimeError> {
        for object in self.with_objects.clone().into_iter().rev() {
            if self.with_has_binding(&object, name)? {
                return self.set_property(&object, &name.into(), &value);
            }
        }
        Err(RuntimeError::ReferenceError(name.into()))
    }

    pub(super) fn add(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        let left = self.coerce_primitive(&left, "default")?;
        let right = self.coerce_primitive(&right, "default")?;
        if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
            let mut a = primitive::string(&left)?;
            let b = primitive::string(&right)?;
            if a.byte_len()
                .checked_add(b.byte_len())
                .is_none_or(|len| len > self.config.max_string_bytes)
            {
                return Err(RuntimeError::StringLimit {
                    limit: self.config.max_string_bytes,
                });
            }
            a.push_str(&b);
            Ok(Value::String(a))
        } else {
            Ok(Value::Number(
                primitive::number(&left)? + primitive::number(&right)?,
            ))
        }
    }
}
