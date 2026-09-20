// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integer, string and offset coercion of Temporal constructor arguments and property-bag
//! fields: `ToIntegerWithTruncation`, `ToBigInt` and the optional-field readers every
//! `from`/`with`/duration-like conversion shares.

use super::super::*;

impl Vm {
    /// `ToIntegerWithTruncation`, plus a range check: every Temporal numeric
    /// date/time field (year, month, day, era year, hour, ...) uses this
    /// same conversion in every context — constructor argument or
    /// property-bag field — truncating a fractional value toward zero
    /// rather than rejecting it (Test262's `PlainDate/argument-convert.js`,
    /// `PlainDate/prototype/with/order-of-operations.js`'s `year: 1.7`).
    pub(in super::super::super) fn temporal_integer(
        &mut self,
        value: &Value,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        let value = self.coerce_number(value)?;
        let value = value.trunc();
        if !value.is_finite() || !(f64::from(minimum)..=f64::from(maximum)).contains(&value) {
            return Err(RuntimeError::RangeError(format!("invalid Temporal {name}")));
        }
        Ok(value as i32)
    }

    /// `ToBigInt`: a Number is a `TypeError` (not a truncation), a Boolean is
    /// `0n`/`1n`, and a String that is not an integer literal is a
    /// `SyntaxError` — the exact set Test262's `Instant/basic.js` and
    /// `Instant/argument.js` pin for the constructor's argument.
    pub(in super::super::super) fn temporal_to_big_int(
        &mut self,
        value: &Value,
    ) -> Result<BigInt, RuntimeError> {
        match self.coerce_primitive(value, "number")? {
            Value::BigInt(value) => Ok(value),
            Value::Bool(flag) => Ok(BigInt::from(u8::from(flag))),
            Value::String(text) => {
                let text = text
                    .to_utf8()
                    .map_err(|_| RuntimeError::SyntaxError("invalid BigInt string".into()))?;
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return Ok(BigInt::from(0));
                }
                BigInt::parse_bytes(trimmed.as_bytes(), 10)
                    .ok_or_else(|| RuntimeError::SyntaxError("invalid BigInt string".into()))
            }
            _ => Err(RuntimeError::TypeError(
                "Temporal.Instant requires epoch nanoseconds as a BigInt".into(),
            )),
        }
    }

    pub(in super::super::super) fn temporal_optional_integer(
        &mut self,
        value: &Value,
        default: i32,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        if *value == Value::Undefined {
            Ok(default)
        } else {
            self.temporal_integer(value, minimum, maximum, name)
        }
    }

    /// `PrepareCalendarFields`/`PreparePartialCalendarFields`'s own
    /// per-field shape for a numeric property: `Get`, then — only if the
    /// result is not `undefined` — immediately `ToIntegerWithTruncation`
    /// it, before moving on to the next field name. Callers that need
    /// several fields from the same object must invoke this (and
    /// `temporal_read_optional_string` below) once per field, **in the
    /// exact alphabetical order of the field names themselves**, and never
    /// batch every `Get` ahead of every conversion — the interleaving
    /// itself is observable (`order-of-operations.js`'s `"get
    /// fields.day"`/`"get fields.day.valueOf"`/`"call fields.day.valueOf"`
    /// triple appearing before the next field's own `"get fields.<next>"`).
    pub(in super::super::super) fn temporal_read_optional_integer(
        &mut self,
        object: &Value,
        name: &'static str,
        minimum: i32,
        maximum: i32,
    ) -> Result<Option<i32>, RuntimeError> {
        let value = self.get_property(object, &name.into())?;
        (!matches!(value, Value::Undefined))
            .then(|| self.temporal_integer(&value, minimum, maximum, name))
            .transpose()
    }

    /// The string-field counterpart of `temporal_read_optional_integer`:
    /// `Get`, then immediately `ToString` a present value, matching
    /// `order-of-operations.js`'s `"get fields.monthCode"`/`"get
    /// fields.monthCode.toString"`/`"call fields.monthCode.toString"`
    /// triple for a `monthCode`/`era` field.
    pub(in super::super::super) fn temporal_read_optional_string(
        &mut self,
        object: &Value,
        name: &'static str,
        error: &str,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(object, &name.into())?;
        (!matches!(value, Value::Undefined))
            .then(|| self.coerce_string(&value))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError(error.into()))
            })
            .transpose()
    }

    /// A property bag's `offset` field: `Get`, then `ToPrimitive` with a
    /// string hint, then require an actual `String` -- a non-object,
    /// non-string primitive (`0`/`null`/`true`/`1000n`) is a `TypeError`
    /// without ever being stringified (`offset-property-invalid-string.js`),
    /// never a `RangeError` from a coerced-then-rejected string like `"0"`.
    /// Done per field, like `temporal_read_optional_string`, so the result
    /// is a plain `String` before anything else is read.
    pub(in super::super::super) fn temporal_read_optional_offset_string(
        &mut self,
        object: &Value,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(object, &"offset".into())?;
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let primitive = self.coerce_primitive(&value, "string")?;
        if !matches!(primitive, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime offset must be a string".into(),
            ));
        }
        let text = self.coerce_string(&primitive)?;
        text.to_utf8()
            .map(Some)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal offset".into()))
    }

    pub(in super::super::super) fn temporal_duration_integer(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<i128, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(0);
        }
        let value = self.coerce_number(value)?;
        if !value.is_finite() || value.fract() != 0.0 || value.abs() >= 2_f64.powi(100) {
            return Err(RuntimeError::RangeError(format!(
                "invalid Temporal.Duration {name}"
            )));
        }
        Ok(value as i128)
    }
}
