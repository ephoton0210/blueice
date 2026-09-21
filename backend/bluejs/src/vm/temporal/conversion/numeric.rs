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
                "Temporal epoch nanoseconds must be a BigInt".into(),
            )),
        }
    }

    /// The `Temporal.ZonedDateTime` constructor's own `timeZone` argument: a
    /// String holding a *bare* time-zone identifier (`ParseTimeZoneIdentifier`),
    /// never the wider `ToTemporalTimeZoneIdentifier` that `from` and friends
    /// use. A non-String is a `TypeError`, and an ISO date-time string that
    /// merely *contains* a zone (`"1997-12-04T12:34[+01:00]"`) is a
    /// `RangeError` here (`ZonedDateTime/timezone-iso-string.js`).
    pub(in super::super::super) fn temporal_constructor_time_zone(
        &mut self,
        value: &Value,
    ) -> Result<time_zone::TimeZone, RuntimeError> {
        let Value::String(source) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime time zone must be a string".into(),
            ));
        };
        let source = source
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
        time_zone::parse_bare_identifier(&source).ok_or_else(|| {
            RuntimeError::RangeError(format!("invalid Temporal time zone: {source}"))
        })
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

    /// `ToPositiveIntegerWithTruncation` for a property-bag `month` or `day`:
    /// `Get`, then -- for a present value -- truncate toward zero and require
    /// at least `1`. There is deliberately **no upper bound** (a calendar's own
    /// `overflow` regulation constrains or rejects a month/day past its range,
    /// not the field reader), so the result saturates at `i32::MAX` rather than
    /// failing: a value that large is out of any calendar's range either way,
    /// which is all the regulation step needs to know. `0`, negatives and
    /// non-finite numbers are always a `RangeError`, whatever `overflow` says.
    pub(in super::super::super) fn temporal_read_optional_positive_integer(
        &mut self,
        object: &Value,
        name: &'static str,
    ) -> Result<Option<i32>, RuntimeError> {
        let value = self.get_property(object, &name.into())?;
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let number = self.coerce_number(&value)?;
        let integer = number.trunc();
        if !number.is_finite() || integer < 1.0 {
            return Err(RuntimeError::RangeError(format!("invalid Temporal {name}")));
        }
        Ok(Some(integer.min(f64::from(i32::MAX)) as i32))
    }

    /// A property-bag time-of-day field (`hour` .. `nanosecond`): `Get`, then --
    /// for a present value -- `ToIntegerWithTruncation` with no range at all.
    /// `RegulateTime` (constrain or reject, per `overflow`) judges the range
    /// once every field, and the `overflow` option, have been read.
    pub(in super::super::super) fn temporal_read_optional_time_field(
        &mut self,
        object: &Value,
        name: &'static str,
    ) -> Result<Option<i64>, RuntimeError> {
        let value = self.get_property(object, &name.into())?;
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_truncated_integer(&value, name).map(Some)
    }

    /// `ToMonthCode`: `Get` `monthCode`, then -- for a present value --
    /// `ToPrimitive` with a string hint, which must yield a *String* (a
    /// number, boolean, `null`, symbol or `BigInt`, or an object whose
    /// `toString` returns one, is a `TypeError`: month codes are never
    /// stringified for the caller). The code's *syntax* (`M` plus two digits,
    /// optionally `L`) is checked immediately, so a malformed code is a
    /// `RangeError` before any later field -- `year`, say -- gets to throw
    /// its own `TypeError`. Whether a well-formed code names a real month of
    /// the calendar is a separate, later question, answered by the caller's
    /// calendar resolution once every field has been read
    /// (`PlainDate/from/{month-code-wrong-type,monthcode-invalid}.js`).
    pub(in super::super::super) fn temporal_read_month_code(
        &mut self,
        object: &Value,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(object, &"monthCode".into())?;
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let Value::String(code) = self.coerce_primitive(&value, "string")? else {
            return Err(RuntimeError::TypeError(
                "Temporal monthCode must be a string".into(),
            ));
        };
        let code = code
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal month code".into()))?;
        if !plain_month_day::is_well_formed_month_code(&code) {
            return Err(RuntimeError::RangeError(
                "invalid Temporal month code".into(),
            ));
        }
        Ok(Some(code))
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

    /// `ToOffsetString` for a `ZonedDateTime` property bag's `offset` field:
    /// [`Self::temporal_read_optional_offset_string`], then the offset's
    /// *syntax* is checked at once (a `RangeError`, ahead of any later field's
    /// own `TypeError`) and returned in nanoseconds.
    pub(in super::super::super) fn temporal_read_offset_nanoseconds(
        &mut self,
        object: &Value,
    ) -> Result<Option<i64>, RuntimeError> {
        self.temporal_read_optional_offset_string(object)?
            .map(|text| {
                iso::parse_offset_string_nanoseconds(&text)
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal offset".into()))
            })
            .transpose()
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
