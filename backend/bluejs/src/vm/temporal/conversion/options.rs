// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Option-bag readers shared by every Temporal method: `GetOptionsObject`, the
//! rounding/unit options and `GetRoundToOptionsObject`.

use super::super::*;

impl Vm {
    /// `GetTemporalFractionalSecondDigitsOption`, a `GetStringOrNumberOption`
    /// whose only permitted string is `"auto"`: a Number is floored and then
    /// range-checked (so `9.7` is 9 but `-0.6` is out of range), and anything
    /// that is not a Number is stringified and must equal `"auto"`.
    pub(in super::super::super) fn temporal_fractional_second_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalSecondDigits".into())?;
        match value {
            Value::Undefined => Ok(None),
            Value::Number(digits) => {
                let digits = digits.floor();
                if !digits.is_finite() || !(0.0..=9.0).contains(&digits) {
                    return Err(RuntimeError::RangeError(
                        "invalid fractionalSecondDigits".into(),
                    ));
                }
                Ok(Some(digits as u8))
            }
            value => {
                let text = self.coerce_string(&value)?;
                let text = text.to_utf8().map_err(|_| {
                    RuntimeError::RangeError("invalid fractionalSecondDigits".into())
                })?;
                if text == "auto" {
                    Ok(None)
                } else {
                    Err(RuntimeError::RangeError(
                        "invalid fractionalSecondDigits".into(),
                    ))
                }
            }
        }
    }

    /// Reads `options` values without validating them, so every property a
    /// method consumes is fetched and coerced *before* any of them is
    /// range-checked. Temporal requires exactly that ordering — Test262's
    /// `PlainTime/prototype/round/options-read-before-algorithmic-validation.js`
    /// reads `smallestUnit` (and throws on the increment) only after
    /// `roundingIncrement`/`roundingMode` have already been read — so the
    /// combined read-and-validate `temporal_string_option` cannot be used
    /// where more than one option participates in a joint check.
    pub(in super::super::super) fn temporal_raw_string_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        string
            .to_utf8()
            .map(Some)
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))
    }

    pub(in super::super::super) fn temporal_raw_number_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<f64>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        self.coerce_number(&value).map(Some)
    }

    /// `ToTemporalRoundingIncrement` applied to an already-read raw value.
    pub(in super::super::super) fn temporal_validated_rounding_increment(
        increment: Option<f64>,
    ) -> Result<i128, RuntimeError> {
        let Some(increment) = increment else {
            return Ok(1);
        };
        if !increment.is_finite() {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        let integer = increment.trunc();
        if !(1.0..=1_000_000_000.0).contains(&integer) {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        Ok(integer as i128)
    }

    pub(in super::super::super) fn temporal_validated_rounding_mode(
        mode: Option<&str>,
        default: blueice_ecma402::NumberRoundingMode,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match mode {
            None => Ok(default),
            Some(mode) => rounding::parse_rounding_mode(mode)
                .ok_or_else(|| RuntimeError::RangeError("invalid roundingMode option".into())),
        }
    }

    pub(in super::super::super) fn temporal_validated_time_unit(
        unit: Option<&str>,
        name: &str,
    ) -> Result<Option<rounding::TimeUnit>, RuntimeError> {
        match unit {
            None => Ok(None),
            Some(unit) => rounding::parse_time_unit(unit)
                .map(Some)
                .ok_or_else(|| RuntimeError::RangeError(format!("invalid {name} option"))),
        }
    }

    /// `ValidateTemporalRoundingIncrement(increment, maximum, false)` for a
    /// time-only type.
    ///
    /// This is deliberately *not* `Temporal.Instant.round`'s rule. An
    /// `Instant` is unbounded, so its increment only has to divide a whole
    /// day (`maximum` inclusive); a `PlainTime` is already bounded to one
    /// day, so its increment must divide the *unit's own* place value and
    /// stay strictly below it — `{ smallestUnit: "hours", roundingIncrement:
    /// 24 }` and `{ smallestUnit: "nanoseconds", roundingIncrement: 1000 }`
    /// both throw, per Test262's
    /// `PlainTime/prototype/round/roundingincrement-invalid.js`.
    pub(in super::super::super) fn temporal_validated_plain_time_increment(
        increment: i128,
        unit: rounding::TimeUnit,
    ) -> Result<(), RuntimeError> {
        let maximum = unit.increment_dividend();
        if increment >= maximum || maximum % increment != 0 {
            return Err(RuntimeError::RangeError(
                "roundingIncrement does not divide evenly into the smallestUnit".into(),
            ));
        }
        Ok(())
    }

    /// `GetTemporalOverflowOption`: `true` means `"reject"`.
    pub(in super::super::super) fn temporal_overflow_option(
        &mut self,
        options: &Value,
    ) -> Result<bool, RuntimeError> {
        Ok(self
            .temporal_string_option(options, "overflow", &["constrain", "reject"])?
            .as_deref()
            == Some("reject"))
    }

    /// `GetOptionsObject`: `undefined` becomes a fresh empty object; an
    /// Object is used as-is; any other value is a `TypeError` — it is
    /// deliberately *not* boxed through `ToObject`, so
    /// `instant.toString("some string")` throws rather than reading options
    /// off a String wrapper. Test262's
    /// `Instant/prototype/toString/options-wrong-type.js` and
    /// `PlainTime/prototype/until/options-wrong-type.js` both pass
    /// `"hello"`/`1`/`1n` and require the throw.
    pub(in super::super::super) fn temporal_options(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let result = match value {
            Value::Undefined => Value::Object(self.with_roots(|heap| heap.alloc_object(None))?),
            Value::Object(_) => value.clone(),
            _ => {
                return Err(RuntimeError::TypeError(
                    "Temporal options must be an object or undefined".into(),
                ));
            }
        };
        self.stack.push(result.clone());
        Ok(result)
    }

    /// The `roundTo` parameter of `Temporal.Instant.prototype.round`: a
    /// String is shorthand for `{ smallestUnit: <string> }`, carried on a
    /// null-prototype object so `Object.prototype` accessors for the other
    /// option names are never consulted (Test262's
    /// `string-shorthand-no-object-prototype-pollution.js`).
    pub(in super::super::super) fn temporal_round_to(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        if *value == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.Instant.round requires a roundTo argument".into(),
            ));
        }
        if !matches!(value, Value::String(_)) {
            return self.temporal_options(value);
        }
        let object = self.with_roots(|heap| heap.alloc_object(None))?;
        let result = Value::Object(object);
        self.stack.push(result.clone());
        self.define_data(object, "smallestUnit", value.clone(), true, true, true)?;
        Ok(result)
    }

    pub(in super::super::super) fn temporal_string_option(
        &mut self,
        options: &Value,
        name: &str,
        allowed: &[&str],
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if !allowed.is_empty() && !allowed.contains(&string.as_str()) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(string))
    }

    /// `ToTemporalRoundingIncrement`: an integer in `1..=1e9`, default `1`.
    pub(in super::super::super) fn temporal_rounding_increment(
        &mut self,
        options: &Value,
    ) -> Result<i128, RuntimeError> {
        let value = self.get_property(options, &"roundingIncrement".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        let integer = value.trunc();
        if !(1.0..=1_000_000_000.0).contains(&integer) {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        Ok(integer as i128)
    }

    pub(in super::super::super) fn temporal_rounding_mode(
        &mut self,
        options: &Value,
        default: blueice_ecma402::NumberRoundingMode,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match self.temporal_string_option(
            options,
            "roundingMode",
            &[
                "ceil",
                "floor",
                "expand",
                "trunc",
                "halfCeil",
                "halfFloor",
                "halfExpand",
                "halfTrunc",
                "halfEven",
            ],
        )? {
            None => Ok(default),
            Some(mode) => Ok(rounding::parse_rounding_mode(&mode)
                .expect("temporal_string_option already validated the rounding mode name")),
        }
    }

    /// `GetTemporalUnitValuedOption`: reads a unit-valued option, accepting
    /// **every** unit name (including the calendar units `Temporal.Instant`
    /// itself never allows) plus an optional extra literal such as `"auto"`.
    /// Rejecting a syntactically valid but operation-inappropriate unit is a
    /// separate, later step — the ordering Test262's
    /// `options-read-before-algorithmic-validation.js` fixtures observe.
    pub(in super::super::super) fn temporal_unit_option(
        &mut self,
        options: &Value,
        name: &str,
        auto: bool,
    ) -> Result<Option<rounding::Unit>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if auto && string == "auto" {
            return Ok(None);
        }
        rounding::parse_unit(&string)
            .map(Some)
            .ok_or_else(|| RuntimeError::RangeError(format!("invalid {name} option")))
    }

    /// Narrows an already-read unit option to a time unit, optionally also
    /// rejecting `"hour"` (which `toString` disallows while `round` allows).
    pub(in super::super::super) fn temporal_time_unit(
        unit: Option<rounding::Unit>,
        name: &str,
        allow_hour: bool,
    ) -> Result<Option<rounding::TimeUnit>, RuntimeError> {
        match unit {
            None => Ok(None),
            Some(rounding::Unit::Time(rounding::TimeUnit::Hour)) if !allow_hour => {
                Err(RuntimeError::RangeError(format!("invalid {name} option")))
            }
            Some(rounding::Unit::Time(unit)) => Ok(Some(unit)),
            Some(rounding::Unit::Date(_)) => {
                Err(RuntimeError::RangeError(format!("invalid {name} option")))
            }
        }
    }
}
