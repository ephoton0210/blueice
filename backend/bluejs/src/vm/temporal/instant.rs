// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// Reads a validated `Temporal.Instant` receiver's epoch nanoseconds.
    pub(in super::super) fn temporal_instant_epoch(
        &mut self,
        receiver: &Value,
    ) -> Result<BigInt, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Instant method requires an Instant receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Instant method requires an Instant receiver".into())
        })?;
        if value.kind != TemporalKind::Instant {
            return Err(RuntimeError::TypeError(
                "Temporal.Instant method requires an Instant receiver".into(),
            ));
        }
        Ok(value.epoch_nanoseconds)
    }

    /// Parses a `TemporalInstantString` into epoch nanoseconds.
    pub(in super::super) fn instant_epoch_from_string(
        source: &str,
    ) -> Result<BigInt, RuntimeError> {
        let parts = iso::parse_instant(source)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant string".into()))?;
        // The offset is exact to the nanosecond, so it is applied here rather
        // than through `nanoseconds_since_epoch`'s whole-second parameter.
        let epoch_nanoseconds = epoch::nanoseconds_since_epoch(parts.date, parts.time, 0)
            - BigInt::from(parts.offset_nanoseconds);
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.Instant string is outside the supported range".into(),
            ));
        }
        Ok(epoch_nanoseconds)
    }

    /// `ToTemporalInstant`: an `Instant` or `ZonedDateTime` argument's epoch
    /// nanoseconds are used directly (no observable property reads); any
    /// other object is taken through `ToPrimitive` with a string hint, and a
    /// result that is not a String is a `TypeError` rather than being
    /// stringified — so `instant.equals(1)` throws `TypeError`, not
    /// `RangeError` (Test262's `argument-wrong-type.js`).
    pub(in super::super) fn temporal_to_instant_epoch(
        &mut self,
        value: &Value,
    ) -> Result<BigInt, RuntimeError> {
        let primitive = if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if matches!(
                    temporal.kind,
                    TemporalKind::Instant | TemporalKind::ZonedDateTime
                ) {
                    return Ok(temporal.epoch_nanoseconds);
                }
            }
            self.coerce_primitive(value, "string")?
        } else {
            value.clone()
        };
        let Value::String(text) = primitive else {
            return Err(RuntimeError::TypeError(
                "Temporal.Instant requires an Instant or an ISO 8601 string".into(),
            ));
        };
        let source = text
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal.Instant string".into()))?;
        Self::instant_epoch_from_string(&source)
    }

    pub(in super::super) fn instant_from_epoch_nanoseconds(
        &mut self,
        epoch_nanoseconds: BigInt,
    ) -> Result<Value, RuntimeError> {
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.Instant epoch nanoseconds are outside the supported range".into(),
            ));
        }
        self.alloc_temporal_value(
            TemporalValue {
                kind: TemporalKind::Instant,
                duration: None,
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
                microsecond: 0,
                nanosecond: 0,
                epoch_nanoseconds,
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            },
            false,
        )
    }

    pub(in super::super) fn temporal_instant_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let epoch = self.temporal_instant_epoch(receiver)?;
        let duration = self.temporal_duration_from_value(duration_value)?;
        if duration.years != 0 || duration.months != 0 || duration.weeks != 0 || duration.days != 0
        {
            return Err(RuntimeError::RangeError(
                "Temporal.Instant arithmetic does not accept calendar-unit duration fields".into(),
            ));
        }
        let time = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        );
        let time = if negate { time.negated() } else { time };
        let epoch_i128: i128 = epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let result_i128 = epoch_i128 + time.total_nanoseconds();
        self.instant_from_epoch_nanoseconds(BigInt::from(result_i128))
    }

    pub(in super::super) fn temporal_instant_round(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let epoch = self.temporal_instant_epoch(receiver)?;
        let options = self.temporal_round_to(options)?;
        // Every option is read and coerced in alphabetical order, before any
        // of them is validated against the others.
        let increment = self.temporal_rounding_increment(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::HalfExpand)?;
        let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", true)?
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal.Instant.round requires smallestUnit".into())
            })?;
        let day_nanoseconds = 86_400_000_000_000_i128;
        let step = smallest_unit.nanoseconds() * increment;
        if day_nanoseconds % step != 0 {
            return Err(RuntimeError::RangeError(
                "roundingIncrement does not divide evenly into a day".into(),
            ));
        }
        let epoch_i128: i128 = epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let rounded = duration_math::TimeDuration::from_nanoseconds(epoch_i128)
            .round_as_if_positive(smallest_unit, increment, mode)
            .total_nanoseconds();
        self.instant_from_epoch_nanoseconds(BigInt::from(rounded))
    }

    pub(in super::super) fn temporal_instant_difference(
        &mut self,
        receiver: &Value,
        other: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let self_epoch = self.temporal_instant_epoch(receiver)?;
        let other_epoch = self.temporal_to_instant_epoch(other)?;
        let options = self.temporal_options(options)?;
        // `GetDifferenceSettings` reads largestUnit, roundingIncrement,
        // roundingMode and smallestUnit in that (alphabetical) order, and
        // validates none of them until all four have been read and coerced.
        let largest_unit = self.temporal_unit_option(&options, "largestUnit", true)?;
        let increment = self.temporal_rounding_increment(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
        let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
        let largest_unit = Self::temporal_time_unit(largest_unit, "largestUnit", true)?;
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", true)?
            .unwrap_or(rounding::TimeUnit::Nanosecond);
        // An absent (or explicit `"auto"`) largestUnit is the larger of
        // `Instant`'s own default, `"second"`, and smallestUnit — so
        // `{ smallestUnit: "hours" }` alone balances into hours rather than
        // being rejected as a smaller largestUnit.
        let largest_unit = largest_unit.unwrap_or(smallest_unit.max(rounding::TimeUnit::Second));
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        // `ValidateTemporalRoundingIncrement` with `inclusive = false`: the
        // increment must be strictly smaller than, and divide evenly into,
        // the count of this unit in the next larger one.
        let dividend = smallest_unit.increment_dividend();
        if increment >= dividend || dividend % increment != 0 {
            return Err(RuntimeError::RangeError(
                "roundingIncrement does not divide evenly into the next larger unit".into(),
            ));
        }
        let self_i128: i128 = self_epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let other_i128: i128 = other_epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let difference_ns = if since {
            self_i128 - other_i128
        } else {
            other_i128 - self_i128
        };
        let rounded = duration_math::TimeDuration::from_nanoseconds(difference_ns).round(
            smallest_unit,
            increment,
            mode,
        );
        let [hours, minutes, seconds, milliseconds, microseconds, nanoseconds] =
            rounded.balance_to(largest_unit);
        // `CreateTemporalDuration` (via `temporal_duration_record`) rounds
        // every field to the nearest float64 before the range check, since
        // every `Temporal.Duration` field is a Number — an exact difference
        // that overflows what a double can represent precisely must be
        // observably rounded, not stored exactly
        // (`prototype/{since,until}/float64-representable-integer.js`).
        let record = Self::temporal_duration_record([
            0,
            0,
            0,
            0,
            i128::from(hours),
            i128::from(minutes),
            i128::from(seconds),
            i128::from(milliseconds),
            i128::from(microseconds),
            i128::from(nanoseconds),
        ])?;
        self.alloc_temporal_value(
            TemporalValue {
                kind: TemporalKind::Duration,
                duration: Some(Box::new(record)),
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
                microsecond: 0,
                nanosecond: 0,
                epoch_nanoseconds: 0.into(),
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            },
            false,
        )
    }

    pub(in super::super) fn temporal_instant_equals(
        &mut self,
        receiver: &Value,
        other: &Value,
    ) -> Result<Value, RuntimeError> {
        let self_epoch = self.temporal_instant_epoch(receiver)?;
        let other_epoch = self.temporal_to_instant_epoch(other)?;
        Ok(Value::Bool(self_epoch == other_epoch))
    }

    pub(in super::super) fn temporal_instant_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_to_instant_epoch(one)?;
        let two = self.temporal_to_instant_epoch(two)?;
        Ok(Value::Number(match one.cmp(&two) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    pub(in super::super) fn format_instant_string(
        epoch_nanoseconds: &BigInt,
        precision: SecondsPrecision,
        offset_nanoseconds: Option<i128>,
    ) -> String {
        let local = match offset_nanoseconds {
            Some(offset) => epoch_nanoseconds + BigInt::from(offset),
            None => epoch_nanoseconds.clone(),
        };
        let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
            epoch::instant_fields(&local);
        let mut result = if (0..=9999).contains(&year) {
            format!("{year:04}")
        } else {
            format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
        };
        result.push_str(&format!("-{month:02}-{day:02}T{hour:02}:{minute:02}"));
        if precision != SecondsPrecision::Minute {
            result.push_str(&format!(":{second:02}"));
            let nanos_total = u32::from(millisecond) * 1_000_000
                + u32::from(microsecond) * 1_000
                + u32::from(nanosecond);
            match precision {
                SecondsPrecision::Minute | SecondsPrecision::Digits(0) => {}
                SecondsPrecision::Digits(digits) => {
                    let text = format!("{nanos_total:09}");
                    result.push('.');
                    result.push_str(&text[..digits as usize]);
                }
                SecondsPrecision::Auto if nanos_total != 0 => {
                    let text = format!("{nanos_total:09}");
                    result.push('.');
                    result.push_str(text.trim_end_matches('0'));
                }
                SecondsPrecision::Auto => {}
            }
        }
        match offset_nanoseconds {
            // `FormatDateTimeUTCOffsetRounded`: minutes, never seconds —
            // rounded (half away from zero) to the nearest minute, not
            // truncated. A fixed/UTC offset is always an exact multiple of a
            // minute, so this was unreachable before named zones (which can
            // carry a genuine sub-minute historical offset, e.g. Monrovia's
            // pre-1972 -00:44:30) started flowing through here.
            Some(offset) => {
                let minutes = (offset.abs() + 30_000_000_000) / 60_000_000_000;
                result.push_str(&format!(
                    "{}{:02}:{:02}",
                    if offset < 0 { '-' } else { '+' },
                    minutes / 60,
                    minutes % 60
                ));
            }
            None => result.push('Z'),
        }
        result
    }

    /// `GetTemporalFractionalSecondDigitsOption`, a `GetStringOrNumberOption`
    /// whose only permitted string is `"auto"`: a Number is floored and then
    /// range-checked (so `9.7` is 9 but `-0.6` is out of range), and anything
    /// that is not a Number is stringified and must equal `"auto"`.
    pub(in super::super) fn temporal_fractional_second_digits(
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

    /// `ToTemporalTimeZoneIdentifier` for the `timeZone` option, resolved to
    /// the offset that zone was actually observing at `epoch_nanoseconds` —
    /// the receiver `Instant`'s own epoch, per `GetOffsetNanosecondsFor`.
    /// `Ok(None)` means the option was absent.
    pub(in super::super) fn temporal_to_string_time_zone(
        &mut self,
        options: &Value,
        epoch_nanoseconds: &BigInt,
    ) -> Result<Option<i128>, RuntimeError> {
        let value = self.get_property(options, &"timeZone".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return iso::resolve_time_zone_offset(&temporal.time_zone, epoch_nanoseconds)
                        .map(Some)
                        .map_err(|()| RuntimeError::RangeError("invalid time zone".into()));
                }
            }
        }
        let Value::String(text) = &value else {
            return Err(RuntimeError::TypeError(
                "a Temporal time zone must be a string".into(),
            ));
        };
        let source = text
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid time zone".into()))?;
        iso::resolve_time_zone_offset(&source, epoch_nanoseconds)
            .map(Some)
            .map_err(|()| RuntimeError::RangeError("invalid time zone".into()))
    }

    pub(in super::super) fn temporal_instant_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let epoch = self.temporal_instant_epoch(receiver)?;
        let options = self.temporal_options(options)?;
        // Read (and coerce) every option in alphabetical order first; only
        // then reject a unit this operation does not accept.
        let explicit_digits = self.temporal_fractional_second_digits(&options)?;
        let mode =
            self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
        let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
        let offset = self.temporal_to_string_time_zone(&options, &epoch)?;
        // `hour` is a valid unit name but not a valid `toString` precision.
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", false)?;
        // `ToSecondsStringPrecision`: smallestUnit wins outright, and the
        // digit count implies both the rounding unit and its increment.
        let (precision, unit, increment) = match smallest_unit {
            Some(rounding::TimeUnit::Minute) => {
                (SecondsPrecision::Minute, rounding::TimeUnit::Minute, 1)
            }
            Some(rounding::TimeUnit::Second) => {
                (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1)
            }
            Some(rounding::TimeUnit::Millisecond) => (
                SecondsPrecision::Digits(3),
                rounding::TimeUnit::Millisecond,
                1,
            ),
            Some(rounding::TimeUnit::Microsecond) => (
                SecondsPrecision::Digits(6),
                rounding::TimeUnit::Microsecond,
                1,
            ),
            Some(rounding::TimeUnit::Nanosecond) | Some(rounding::TimeUnit::Hour) => (
                SecondsPrecision::Digits(9),
                rounding::TimeUnit::Nanosecond,
                1,
            ),
            None => match explicit_digits {
                None => (
                    SecondsPrecision::Auto,
                    rounding::TimeUnit::Nanosecond,
                    1_i128,
                ),
                Some(0) => (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1),
                Some(digits @ 1..=3) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Millisecond,
                    10_i128.pow(u32::from(3 - digits)),
                ),
                Some(digits @ 4..=6) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Microsecond,
                    10_i128.pow(u32::from(6 - digits)),
                ),
                Some(digits) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Nanosecond,
                    10_i128.pow(u32::from(9 - digits)),
                ),
            },
        };
        let epoch_i128: i128 = epoch
            .to_i128()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.Instant".into()))?;
        let rounded = duration_math::TimeDuration::from_nanoseconds(epoch_i128)
            .round_as_if_positive(unit, increment, mode)
            .total_nanoseconds();
        Ok(Value::String(
            Self::format_instant_string(&BigInt::from(rounded), precision, offset).into(),
        ))
    }

    /// `Temporal.Instant.prototype.toLocaleString`, which is
    /// `CreateDateTimeFormat(locales, options, ANY, ALL)` followed by
    /// `FormatDateTime` — i.e. exactly what
    /// `new Intl.DateTimeFormat(locales, options).format(instant)` produces,
    /// so it is built from the same `Intl.DateTimeFormat` bridge rather than
    /// aliasing `toString`'s ISO serialization.
    pub(in super::super) fn temporal_instant_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        // Brand check before any observable option read.
        self.temporal_instant_epoch(receiver)?;
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
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
    }

    pub(in super::super) fn temporal_instant_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.Instant cannot be converted to a primitive value".into(),
        ))
    }

    pub(in super::super) fn temporal_from_epoch_milliseconds(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let milliseconds = self.coerce_number(value)?;
        if !milliseconds.is_finite() || milliseconds.fract() != 0.0 {
            return Err(RuntimeError::RangeError(
                "invalid Temporal.Instant epoch milliseconds".into(),
            ));
        }
        let nanoseconds = BigInt::from(milliseconds as i64) * 1_000_000_u32;
        self.instant_from_epoch_nanoseconds(nanoseconds)
    }

    pub(in super::super) fn temporal_from_epoch_nanoseconds(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let nanoseconds = match value {
            Value::BigInt(value) => value.clone(),
            _ => {
                return Err(RuntimeError::TypeError(
                    "Temporal.Instant.fromEpochNanoseconds requires a BigInt".into(),
                ));
            }
        };
        self.instant_from_epoch_nanoseconds(nanoseconds)
    }
}
