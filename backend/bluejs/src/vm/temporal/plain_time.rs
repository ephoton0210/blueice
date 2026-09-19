// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// `ToSecondsStringPrecisionRecord`'s `[[Precision]]`: whole minutes, or a
/// seconds field with either an explicit decimal-place count or `auto` (the
/// shortest form that loses nothing).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum PlainTimePrecision {
    Minute,
    Seconds(Option<u8>),
}

impl Vm {
    // ---- Stage 1 Track D: Temporal.PlainTime arithmetic -----------------

    /// Reads `options` values without validating them, so every property a
    /// method consumes is fetched and coerced *before* any of them is
    /// range-checked. Temporal requires exactly that ordering — Test262's
    /// `PlainTime/prototype/round/options-read-before-algorithmic-validation.js`
    /// reads `smallestUnit` (and throws on the increment) only after
    /// `roundingIncrement`/`roundingMode` have already been read — so the
    /// combined read-and-validate `temporal_string_option` cannot be used
    /// where more than one option participates in a joint check.
    pub(in super::super) fn temporal_raw_string_option(
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

    pub(in super::super) fn temporal_raw_number_option(
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
    pub(in super::super) fn temporal_validated_rounding_increment(
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

    pub(in super::super) fn temporal_validated_rounding_mode(
        mode: Option<&str>,
        default: blueice_ecma402::NumberRoundingMode,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match mode {
            None => Ok(default),
            Some(mode) => rounding::parse_rounding_mode(mode)
                .ok_or_else(|| RuntimeError::RangeError("invalid roundingMode option".into())),
        }
    }

    pub(in super::super) fn temporal_validated_time_unit(
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
    pub(in super::super) fn temporal_validated_plain_time_increment(
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
    pub(in super::super) fn temporal_overflow_option(
        &mut self,
        options: &Value,
    ) -> Result<bool, RuntimeError> {
        Ok(self
            .temporal_string_option(options, "overflow", &["constrain", "reject"])?
            .as_deref()
            == Some("reject"))
    }

    /// `ToIntegerWithTruncation`: a finite number, truncated toward zero.
    /// Temporal's *time* fields use this rather than requiring an already
    /// integral value — `new Temporal.PlainTime(11.9)` is hour 11, per
    /// Test262's `PlainTime/argument-convert.js`.
    pub(in super::super) fn temporal_truncated_integer(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<i64, RuntimeError> {
        let number = self.coerce_number(value)?;
        if !number.is_finite() {
            return Err(RuntimeError::RangeError(format!("invalid Temporal {name}")));
        }
        Ok(number.trunc() as i64)
    }

    pub(in super::super) fn temporal_optional_truncated_integer(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<i64, RuntimeError> {
        if *value == Value::Undefined {
            Ok(0)
        } else {
            self.temporal_truncated_integer(value, name)
        }
    }

    /// `RegulateTime`: clamp each field into range (`"constrain"`), or reject
    /// an out-of-range one outright (`"reject"`).
    pub(in super::super) fn temporal_regulate_time(
        fields: [i64; 6],
        reject: bool,
    ) -> Result<(u8, u8, u8, u16, u16, u16), RuntimeError> {
        const MAXIMUM: [i64; 6] = [23, 59, 59, 999, 999, 999];
        let mut regulated = [0_i64; 6];
        for (index, field) in fields.into_iter().enumerate() {
            if reject && !(0..=MAXIMUM[index]).contains(&field) {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainTime field is out of range".into(),
                ));
            }
            regulated[index] = field.clamp(0, MAXIMUM[index]);
        }
        Ok((
            regulated[0] as u8,
            regulated[1] as u8,
            regulated[2] as u8,
            regulated[3] as u16,
            regulated[4] as u16,
            regulated[5] as u16,
        ))
    }

    /// `ToTemporalTimeRecord`, in the spec's alphabetical read order
    /// (`hour`, `microsecond`, `millisecond`, `minute`, `nanosecond`,
    /// `second` — Test262's `PlainTime/prototype/with/order-of-operations.js`
    /// asserts exactly that sequence). Returns each field's value where it
    /// was present, so `with` can fall back to its receiver and `from` can
    /// fall back to zero. A bag with none of the six throws.
    pub(in super::super) fn temporal_time_record(
        &mut self,
        bag: &Value,
    ) -> Result<[Option<i64>; 6], RuntimeError> {
        let mut fields = [None; 6];
        let mut present = false;
        for (index, name) in [
            (0, "hour"),
            (4, "microsecond"),
            (3, "millisecond"),
            (1, "minute"),
            (5, "nanosecond"),
            (2, "second"),
        ] {
            let value = self.get_property(bag, &name.into())?;
            if value != Value::Undefined {
                present = true;
                fields[index] = Some(self.temporal_truncated_integer(&value, name)?);
            }
        }
        if !present {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime-like value has no time fields".into(),
            ));
        }
        Ok(fields)
    }

    pub(in super::super) fn plain_time_value(fields: (u8, u8, u8, u16, u16, u16)) -> TemporalValue {
        TemporalValue {
            kind: TemporalKind::PlainTime,
            duration: None,
            year: 1970,
            month: 1,
            day: 1,
            hour: fields.0,
            minute: fields.1,
            second: fields.2,
            millisecond: fields.3,
            microsecond: fields.4,
            nanosecond: fields.5,
            epoch_nanoseconds: 0.into(),
            calendar: "iso8601".into(),
            time_zone: "UTC".into(),
        }
    }

    /// Reads a validated `Temporal.PlainTime` receiver's time of day.
    pub(in super::super) fn temporal_plain_time_fields(
        &mut self,
        receiver: &Value,
    ) -> Result<(u8, u8, u8, u16, u16, u16), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainTime method requires a PlainTime receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainTime method requires a PlainTime receiver".into(),
            )
        })?;
        if value.kind != TemporalKind::PlainTime {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime method requires a PlainTime receiver".into(),
            ));
        }
        Ok((
            value.hour,
            value.minute,
            value.second,
            value.millisecond,
            value.microsecond,
            value.nanosecond,
        ))
    }

    /// `ToTemporalTime`: a `PlainTime`/`PlainDateTime`/`ZonedDateTime` carries
    /// its own time of day; a string goes through the `TemporalTimeString`
    /// grammar; anything else object-shaped is read as a property bag.
    pub(in super::super) fn temporal_to_plain_time(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<(u8, u8, u8, u16, u16, u16), RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let carried = match temporal.kind {
                    TemporalKind::PlainTime | TemporalKind::PlainDateTime => Some((
                        temporal.hour,
                        temporal.minute,
                        temporal.second,
                        temporal.millisecond,
                        temporal.microsecond,
                        temporal.nanosecond,
                    )),
                    // A `ZonedDateTime`'s wall-clock time of day is its instant
                    // shifted by the zone's offset. `UTC` and a fixed numeric
                    // offset are resolvable here; a named IANA zone needs the
                    // transition-rule lookup that is Phase 26 Track E's scope.
                    TemporalKind::ZonedDateTime => {
                        let offset = if temporal.time_zone == "UTC" {
                            0
                        } else {
                            iso::parse_offset_identifier_nanoseconds(&temporal.time_zone)
                                .ok_or_else(|| {
                                    RuntimeError::RangeError(
                                    "Temporal.PlainTime conversion supports UTC and fixed offsets"
                                        .into(),
                                )
                                })?
                        };
                        let local = &temporal.epoch_nanoseconds + BigInt::from(offset);
                        Some(epoch::instant_fields(&local).1)
                    }
                    // Every other Temporal type lacks the singular time
                    // fields, so the property-bag path below throws for it
                    // exactly as the spec requires.
                    _ => None,
                };
                if let Some(carried) = carried {
                    // The options object is still read and its `overflow`
                    // value still validated, even though nothing is regulated.
                    let options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&options)?;
                    return Ok(carried);
                }
            }
            let fields = self.temporal_time_record(value)?;
            let options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&options)?;
            return Self::temporal_regulate_time(fields.map(Option::unwrap_or_default), reject);
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime-like value must be an object or a string".into(),
            ));
        }
        let source = self
            .coerce_string(value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal.PlainTime string".into()))?;
        let fields = iso::parse_plain_time(&source)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.PlainTime string".into()))?;
        let options = self.temporal_options(options)?;
        self.temporal_overflow_option(&options)?;
        Ok(fields)
    }

    pub(in super::super) fn plain_time_from_nanoseconds(
        &mut self,
        nanoseconds: i128,
    ) -> Result<Value, RuntimeError> {
        let fields = duration_math::time_fields_from_nanoseconds(nanoseconds);
        self.alloc_temporal_value(Self::plain_time_value(fields), false)
    }

    /// `AddDurationToTime`. Years/months/weeks *and days* are read (and
    /// range-validated) but contribute nothing: a `PlainTime` has no date to
    /// carry them into, so the spec's `ToInternalDurationRecord` leaves them
    /// in the date part that `AddTime` never looks at. Test262's
    /// `PlainTime/prototype/add/argument-higher-units.js` pins this —
    /// `plainTime.add({ days: 1 })` is the *same* time, not 24 hours later,
    /// and unlike `Temporal.Instant.prototype.add` it is not an error either.
    pub(in super::super) fn temporal_plain_time_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let duration = self.temporal_duration_from_value(duration_value)?;
        let time = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        );
        let time = if negate { time.negated() } else { time };
        let total = duration_math::time_fields_to_nanoseconds(
            fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
        ) + time.total_nanoseconds();
        self.plain_time_from_nanoseconds(total)
    }

    /// `Temporal.PlainTime.prototype.round`. Unlike `Instant.round`, `roundTo`
    /// is required, and a bare string is shorthand for `{ smallestUnit }` —
    /// via a *null-prototype* options object, so a polluted
    /// `Object.prototype.roundingMode` is never observed (Test262's
    /// `round/string-shorthand-no-object-prototype-pollution.js`).
    pub(in super::super) fn temporal_plain_time_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        if *round_to == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime.round requires a smallestUnit or options argument".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            let options = if let Value::String(unit) = round_to {
                let options = self.with_roots(|heap| heap.alloc_object(None))?;
                self.stack.push(Value::Object(options));
                self.define_data(
                    options,
                    "smallestUnit",
                    Value::String(unit.clone()),
                    true,
                    true,
                    true,
                )?;
                Value::Object(options)
            } else {
                self.temporal_options(round_to)?
            };
            let increment = self.temporal_raw_number_option(&options, "roundingIncrement")?;
            let mode = self.temporal_raw_string_option(&options, "roundingMode")?;
            let smallest_unit = self.temporal_raw_string_option(&options, "smallestUnit")?;
            let increment = Self::temporal_validated_rounding_increment(increment)?;
            let mode = Self::temporal_validated_rounding_mode(
                mode.as_deref(),
                blueice_ecma402::NumberRoundingMode::HalfExpand,
            )?;
            let smallest_unit =
                Self::temporal_validated_time_unit(smallest_unit.as_deref(), "smallestUnit")?
                    .ok_or_else(|| {
                        RuntimeError::RangeError(
                            "Temporal.PlainTime.round requires smallestUnit".into(),
                        )
                    })?;
            Self::temporal_validated_plain_time_increment(increment, smallest_unit)?;
            let total = duration_math::time_fields_to_nanoseconds(
                fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
            );
            let rounded = duration_math::TimeDuration::from_nanoseconds(total)
                .round(smallest_unit, increment, mode)
                .total_nanoseconds();
            self.plain_time_from_nanoseconds(rounded)
        })();
        self.stack.truncate(base);
        result
    }

    /// `DifferenceTemporalPlainTime`.
    ///
    /// The `since` direction needs no rounding-mode negation: the spec
    /// negates the mode, differences in the opposite order, then negates the
    /// result, and those three cancel to "difference in this order, mode as
    /// given" — the same equivalence Track C confirmed for `Instant` against
    /// `since/roundingmode-ceil.js`, and which `PlainTime`'s own
    /// `since/roundingmode-*.js` fixtures agree with.
    pub(in super::super) fn temporal_plain_time_difference(
        &mut self,
        receiver: &Value,
        other: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let other_fields = self.temporal_to_plain_time(other, &Value::Undefined)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_options(options)?;
            let largest_unit = self.temporal_raw_string_option(&options, "largestUnit")?;
            let increment = self.temporal_raw_number_option(&options, "roundingIncrement")?;
            let mode = self.temporal_raw_string_option(&options, "roundingMode")?;
            let smallest_unit = self.temporal_raw_string_option(&options, "smallestUnit")?;
            let largest_unit = match largest_unit.as_deref() {
                None | Some("auto") => rounding::TimeUnit::Hour,
                unit => Self::temporal_validated_time_unit(unit, "largestUnit")?
                    .expect("a present largestUnit resolves to a unit"),
            };
            let increment = Self::temporal_validated_rounding_increment(increment)?;
            let mode = Self::temporal_validated_rounding_mode(
                mode.as_deref(),
                blueice_ecma402::NumberRoundingMode::Trunc,
            )?;
            let smallest_unit =
                Self::temporal_validated_time_unit(smallest_unit.as_deref(), "smallestUnit")?
                    .unwrap_or(rounding::TimeUnit::Nanosecond);
            if smallest_unit > largest_unit {
                return Err(RuntimeError::RangeError(
                    "smallestUnit must not be larger than largestUnit".into(),
                ));
            }
            Self::temporal_validated_plain_time_increment(increment, smallest_unit)?;
            let self_total = duration_math::time_fields_to_nanoseconds(
                fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
            );
            let other_total = duration_math::time_fields_to_nanoseconds(
                other_fields.0,
                other_fields.1,
                other_fields.2,
                other_fields.3,
                other_fields.4,
                other_fields.5,
            );
            let difference = if since {
                self_total - other_total
            } else {
                other_total - self_total
            };
            let balanced = duration_math::TimeDuration::from_nanoseconds(difference)
                .round(smallest_unit, increment, mode)
                .balance_to(largest_unit);
            self.alloc_time_duration(balanced)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn alloc_time_duration(
        &mut self,
        fields: [i64; 6],
    ) -> Result<Value, RuntimeError> {
        let record = blueice_ecma402::DurationRecord::try_new(
            0,
            0,
            0,
            0,
            i128::from(fields[0]),
            i128::from(fields[1]),
            i128::from(fields[2]),
            i128::from(fields[3]),
            i128::from(fields[4]),
            i128::from(fields[5]),
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let mut value = Self::plain_time_value((0, 0, 0, 0, 0, 0));
        value.kind = TemporalKind::Duration;
        value.duration = Some(Box::new(record));
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super) fn temporal_plain_time_equals(
        &mut self,
        receiver: &Value,
        other: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let other = self.temporal_to_plain_time(other, &Value::Undefined)?;
        Ok(Value::Bool(fields == other))
    }

    pub(in super::super) fn temporal_plain_time_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_to_plain_time(one, &Value::Undefined)?;
        let two = self.temporal_to_plain_time(two, &Value::Undefined)?;
        Ok(Value::Number(match one.cmp(&two) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    /// `Temporal.PlainTime.prototype.with`. A property bag only: any Temporal
    /// value (including another `PlainTime`), a `calendar` property or a
    /// `timeZone` property is a `TypeError`, per Test262's
    /// `with/plaintimelike-invalid.js`.
    pub(in super::super) fn temporal_plain_time_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let object = like.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.PlainTime.with requires a property bag".into())
        })?;
        if self.heap.temporal_value(object)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainTime.with does not accept a Temporal value".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            for name in ["calendar", "timeZone"] {
                if self.get_property(like, &name.into())? != Value::Undefined {
                    return Err(RuntimeError::TypeError(format!(
                        "Temporal.PlainTime.with does not accept a {name} property"
                    )));
                }
            }
            let partial = self.temporal_time_record(like)?;
            let options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&options)?;
            let current = [
                i64::from(fields.0),
                i64::from(fields.1),
                i64::from(fields.2),
                i64::from(fields.3),
                i64::from(fields.4),
                i64::from(fields.5),
            ];
            let merged = std::array::from_fn(|index| partial[index].unwrap_or(current[index]));
            let regulated = Self::temporal_regulate_time(merged, reject)?;
            self.alloc_temporal_value(Self::plain_time_value(regulated), false)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn format_plain_time_string(
        fields: (u8, u8, u8, u16, u16, u16),
        precision: PlainTimePrecision,
    ) -> String {
        let (hour, minute, second, millisecond, microsecond, nanosecond) = fields;
        let mut result = format!("{hour:02}:{minute:02}");
        let digits = match precision {
            PlainTimePrecision::Minute => return result,
            PlainTimePrecision::Seconds(digits) => digits,
        };
        result.push_str(&format!(":{second:02}"));
        let nanos_total = u32::from(millisecond) * 1_000_000
            + u32::from(microsecond) * 1_000
            + u32::from(nanosecond);
        match digits {
            Some(0) => {}
            Some(digits) => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(&text[..digits as usize]);
            }
            None if nanos_total != 0 => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(text.trim_end_matches('0'));
            }
            None => {}
        }
        result
    }

    pub(in super::super) fn temporal_plain_time_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let fields = self.temporal_plain_time_fields(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_options(options)?;
            // Alphabetical read order, per `toString/order-of-operations.js`.
            let digits_value = self.get_property(&options, &"fractionalSecondDigits".into())?;
            let digits = match &digits_value {
                Value::Undefined => None,
                Value::Number(number) => {
                    if !number.is_finite() {
                        return Err(RuntimeError::RangeError(
                            "invalid fractionalSecondDigits option".into(),
                        ));
                    }
                    let count = number.floor();
                    if !(0.0..=9.0).contains(&count) {
                        return Err(RuntimeError::RangeError(
                            "invalid fractionalSecondDigits option".into(),
                        ));
                    }
                    Some(count as u8)
                }
                // `GetTemporalFractionalSecondDigitsOption` only accepts a
                // non-Number if it stringifies to exactly "auto".
                other => {
                    let text = self.coerce_string(other)?.to_utf8().map_err(|_| {
                        RuntimeError::RangeError("invalid fractionalSecondDigits option".into())
                    })?;
                    if text != "auto" {
                        return Err(RuntimeError::RangeError(
                            "invalid fractionalSecondDigits option".into(),
                        ));
                    }
                    None
                }
            };
            let mode = self.temporal_raw_string_option(&options, "roundingMode")?;
            let mode = Self::temporal_validated_rounding_mode(
                mode.as_deref(),
                blueice_ecma402::NumberRoundingMode::Trunc,
            )?;
            // `hour`/`hours` is a valid rounding unit but not a valid
            // serialization unit — `smallestunit-invalid-string.js` lists it
            // among the rejected values.
            let smallest_unit = self.temporal_string_option(
                &options,
                "smallestUnit",
                &[
                    "minute",
                    "minutes",
                    "second",
                    "seconds",
                    "millisecond",
                    "milliseconds",
                    "microsecond",
                    "microseconds",
                    "nanosecond",
                    "nanoseconds",
                ],
            )?;
            let smallest_unit =
                Self::temporal_validated_time_unit(smallest_unit.as_deref(), "smallestUnit")?;
            // `ToSecondsStringPrecisionRecord`: an explicit `smallestUnit`
            // always wins over `fractionalSecondDigits`.
            let (unit, increment, precision) = match smallest_unit {
                Some(rounding::TimeUnit::Minute) => {
                    (rounding::TimeUnit::Minute, 1, PlainTimePrecision::Minute)
                }
                Some(rounding::TimeUnit::Second) => (
                    rounding::TimeUnit::Second,
                    1,
                    PlainTimePrecision::Seconds(Some(0)),
                ),
                Some(rounding::TimeUnit::Millisecond) => (
                    rounding::TimeUnit::Millisecond,
                    1,
                    PlainTimePrecision::Seconds(Some(3)),
                ),
                Some(rounding::TimeUnit::Microsecond) => (
                    rounding::TimeUnit::Microsecond,
                    1,
                    PlainTimePrecision::Seconds(Some(6)),
                ),
                Some(rounding::TimeUnit::Nanosecond) => (
                    rounding::TimeUnit::Nanosecond,
                    1,
                    PlainTimePrecision::Seconds(Some(9)),
                ),
                Some(rounding::TimeUnit::Hour) => unreachable!(
                    "the smallestUnit option list above excludes hour for serialization"
                ),
                None => match digits {
                    None => (
                        rounding::TimeUnit::Nanosecond,
                        1,
                        PlainTimePrecision::Seconds(None),
                    ),
                    Some(0) => (
                        rounding::TimeUnit::Second,
                        1,
                        PlainTimePrecision::Seconds(Some(0)),
                    ),
                    Some(count @ 1..=3) => (
                        rounding::TimeUnit::Millisecond,
                        10_i128.pow(u32::from(3 - count)),
                        PlainTimePrecision::Seconds(Some(count)),
                    ),
                    Some(count @ 4..=6) => (
                        rounding::TimeUnit::Microsecond,
                        10_i128.pow(u32::from(6 - count)),
                        PlainTimePrecision::Seconds(Some(count)),
                    ),
                    Some(count) => (
                        rounding::TimeUnit::Nanosecond,
                        10_i128.pow(u32::from(9 - count)),
                        PlainTimePrecision::Seconds(Some(count)),
                    ),
                },
            };
            let total = duration_math::time_fields_to_nanoseconds(
                fields.0, fields.1, fields.2, fields.3, fields.4, fields.5,
            );
            let rounded = duration_math::TimeDuration::from_nanoseconds(total)
                .round(unit, increment, mode)
                .total_nanoseconds();
            Ok(Value::String(
                Self::format_plain_time_string(
                    duration_math::time_fields_from_nanoseconds(rounded),
                    precision,
                )
                .into(),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    /// `Temporal.PlainTime.prototype.toLocaleString`, which is
    /// `CreateDateTimeFormat(locales, options, TIME, TIME)` followed by
    /// `FormatDateTime` — i.e. exactly what
    /// `new Intl.DateTimeFormat(locales, options).format(plainTime)`
    /// produces, so it is built from the same `Intl.DateTimeFormat` bridge
    /// `Instant`/`ZonedDateTime`'s own `toLocaleString` already use, rather
    /// than aliasing `toString`'s ISO serialization. `create_date_time_format`
    /// -> `date_time_format_format` finds the receiver's `TemporalValue` via
    /// `date_time_format_value`/`date_time_format_input` and routes a
    /// `PlainTime` through `DateTimeFormatInput::TemporalPlain` the same way
    /// a direct `Intl.DateTimeFormat.prototype.format` call already does —
    /// `temporal_format_options`'s `TemporalKind::PlainTime` arm clears date
    /// components/time zone name from the per-value resolved options.
    ///
    /// `required = TIME` is a *formatter-construction-time* rule, separate
    /// from that per-value pruning: it rejects a `dateStyle` option
    /// unconditionally, even with `timeStyle`/individual time fields also
    /// present, because `toLocaleString`'s own freshly-constructed formatter
    /// has no other value to format. Test262's `datestyle-and-timestyle.js`
    /// (`{ dateStyle, timeStyle }` together) pins this. This is deliberately
    /// *not* folded into `temporal_format_options`, which
    /// `Intl.DateTimeFormat.prototype.format`/`formatToParts`/range methods
    /// share too — those construct an ordinary (`required = ANY`) formatter
    /// first and may format *any* value with it, so `dateStyle` there is
    /// simply ignored once `timeStyle` (or another time field) also applies
    /// to a `PlainTime` argument, per
    /// `intl402/DateTimeFormat/prototype/format/
    /// temporal-plaintime-formatting-datetime-style.js` — folding this
    /// check in there regressed that fixture during development.
    pub(in super::super) fn temporal_plain_time_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        // Brand check before any observable option read.
        self.temporal_plain_time_fields(receiver)?;
        let stack_base = self.stack.len();
        let result = (|| {
            let formatter = self.create_date_time_format(
                &Value::Undefined,
                &[
                    native::argument(args, 0).clone(),
                    native::argument(args, 1).clone(),
                ],
                false,
            )?;
            self.stack.push(formatter.clone());
            if self
                .date_time_format_data(&formatter)?
                .options()
                .date_style
                .is_some()
            {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainTime.prototype.toLocaleString does not accept a dateStyle option"
                        .into(),
                ));
            }
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
    }

    pub(in super::super) fn temporal_plain_time_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainTime cannot be converted to a primitive value".into(),
        ))
    }
}
