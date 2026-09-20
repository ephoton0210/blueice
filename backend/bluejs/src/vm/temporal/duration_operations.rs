// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// The calendar-agnostic gate shared by `add`/`subtract`/`round`/`total`/
    /// `compare`. Where the specification requires a `relativeTo` this engine
    /// cannot honour, the answer is a `RangeError`, never an approximation.
    pub(in super::super) fn temporal_duration_require_no_calendar_units(
        record: &blueice_ecma402::DurationRecord,
        units: &[rounding::TemporalUnit],
    ) -> Result<(), RuntimeError> {
        if Self::temporal_duration_largest_unit(record).is_calendar()
            || units.iter().any(|unit| unit.is_calendar())
        {
            return Err(RuntimeError::RangeError(
                "a Temporal.Duration with years, months or weeks needs a relativeTo anchor".into(),
            ));
        }
        Ok(())
    }

    pub(in super::super) fn temporal_duration_with(
        &mut self,
        receiver: &Value,
        like: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        if !matches!(like, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration.prototype.with requires a Duration-like object".into(),
            ));
        }
        let mut fields = [
            record.years,
            record.months,
            record.weeks,
            record.days,
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        ];
        let mut present = false;
        for (name, index) in DURATION_FIELDS_IN_READ_ORDER {
            let value = self.get_property(like, &name.into())?;
            if value == Value::Undefined {
                continue;
            }
            present = true;
            fields[index] = self.temporal_duration_integer(&value, name)?;
        }
        if !present {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration.prototype.with requires at least one duration field".into(),
            ));
        }
        self.temporal_duration_create(fields)
    }

    pub(in super::super) fn temporal_duration_negated(
        &mut self,
        receiver: &Value,
        absolute: bool,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let map = |value: i128| if absolute { value.abs() } else { -value };
        // Negating or taking the magnitude of every field at once preserves
        // both the common-sign and the range invariants, so this cannot fail.
        self.alloc_temporal_value(
            Self::temporal_duration_value(blueice_ecma402::DurationRecord {
                years: map(record.years),
                months: map(record.months),
                weeks: map(record.weeks),
                days: map(record.days),
                hours: map(record.hours),
                minutes: map(record.minutes),
                seconds: map(record.seconds),
                milliseconds: map(record.milliseconds),
                microseconds: map(record.microseconds),
                nanoseconds: map(record.nanoseconds),
            }),
            false,
        )
    }

    pub(in super::super) fn temporal_duration_add(
        &mut self,
        receiver: &Value,
        other: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_duration_receiver(receiver)?;
        let mut two = self.temporal_duration_from_value(other)?;
        if negate {
            two = blueice_ecma402::DurationRecord {
                years: -two.years,
                months: -two.months,
                weeks: -two.weeks,
                days: -two.days,
                hours: -two.hours,
                minutes: -two.minutes,
                seconds: -two.seconds,
                milliseconds: -two.milliseconds,
                microseconds: -two.microseconds,
                nanoseconds: -two.nanoseconds,
            };
        }
        // `AddDurations` balances the sum up to the larger of the two
        // operands' own largest units — a calendar one has no fixed length,
        // so it is rejected outright rather than balanced.
        let largest = Self::temporal_duration_largest_unit(&one)
            .max(Self::temporal_duration_largest_unit(&two));
        Self::temporal_duration_require_no_calendar_units(&one, &[largest])?;
        Self::temporal_duration_require_no_calendar_units(&two, &[])?;
        let total = duration_math::TimeDuration::from_record_with_24_hour_days(&one)
            .total_nanoseconds()
            + duration_math::TimeDuration::from_record_with_24_hour_days(&two).total_nanoseconds();
        let balanced =
            duration_math::TimeDuration::from_nanoseconds(total).balance_with_days(largest);
        self.temporal_duration_create([
            0,
            0,
            0,
            balanced[0],
            balanced[1],
            balanced[2],
            balanced[3],
            balanced[4],
            balanced[5],
            balanced[6],
        ])
    }

    pub(in super::super) fn temporal_duration_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let (shorthand, options) = self.temporal_duration_round_to(round_to, "round")?;
            // The specification reads every option, in alphabetical order,
            // before any algorithmic validation happens.
            let (requested_largest, anchor, increment, mode, requested_smallest) = match &shorthand
            {
                Some(text) => (
                    UnitOption::Unset,
                    None,
                    1,
                    blueice_ecma402::NumberRoundingMode::HalfExpand,
                    UnitOption::Unit(Self::temporal_duration_unit_name(text, "smallestUnit")?),
                ),
                None => {
                    let largest =
                        self.temporal_duration_unit_option(&options, "largestUnit", true)?;
                    let relative_to = self.get_property(&options, &"relativeTo".into())?;
                    let anchor = self.temporal_duration_relative_to(&relative_to)?;
                    let increment = self.temporal_rounding_increment(&options)?;
                    let mode = self.temporal_rounding_mode(
                        &options,
                        blueice_ecma402::NumberRoundingMode::HalfExpand,
                    )?;
                    let smallest =
                        self.temporal_duration_unit_option(&options, "smallestUnit", false)?;
                    (largest, anchor, increment, mode, smallest)
                }
            };
            if requested_largest == UnitOption::Unset && requested_smallest == UnitOption::Unset {
                return Err(RuntimeError::RangeError(
                    "Temporal.Duration.prototype.round requires largestUnit or smallestUnit".into(),
                ));
            }
            let smallest = requested_smallest
                .unit()
                .unwrap_or(rounding::TemporalUnit::Nanosecond);
            // A smallestUnit larger than the duration's own largest unit
            // raises the default largestUnit with it, so e.g. rounding
            // 86,399 seconds to days yields one day rather than zero.
            let default_largest = Self::temporal_duration_largest_unit(&record).max(smallest);
            let largest = requested_largest.unit().unwrap_or(default_largest);
            if smallest > largest {
                return Err(RuntimeError::RangeError(
                    "smallestUnit must not be larger than largestUnit".into(),
                ));
            }
            if let Some(maximum) = smallest.maximum_rounding_increment() {
                if increment >= maximum || maximum % increment != 0 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement does not divide evenly into the next larger unit".into(),
                    ));
                }
            }
            if increment > 1 && smallest != largest && smallest >= rounding::TemporalUnit::Day {
                return Err(RuntimeError::RangeError(
                    "a date-unit roundingIncrement above 1 cannot also balance to a larger unit"
                        .into(),
                ));
            }
            // A `Zoned` anchor is a different computation from a `Plain` one
            // even for a blank duration or a purely time-granularity round: a
            // day is not a fixed 24 hours there, so the receiver's own real day
            // length decides whether a rounded remainder carries into `days`
            // (`case-where-relativeto-affects-rounding-mode-half-even.js`,
            // `next-day-out-of-range.js`). The specification has `round` add the
            // whole duration to the anchor and then take
            // `DifferenceZonedDateTimeWithRounding` between the two -- exactly
            // what `ZonedDateTime.prototype.until` does.
            if let Some(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns,
                local_date,
                local_time,
            }) = &anchor
            {
                let target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &record,
                )?;
                let origin = zoned_difference::ZonedOrigin {
                    zone,
                    calendar: *calendar,
                    epoch_nanoseconds: epoch_ns,
                    date: *local_date,
                    time: *local_time,
                };
                let internal = zoned_difference::difference_with_rounding(
                    &origin, &target, largest, increment, smallest, mode,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                })?;
                return self
                    .temporal_duration_create(internal.into_fields(largest).map(i128::from));
            }
            // A blank duration rounds to a blank duration in every unit: zero
            // is an exact multiple of any increment, and balancing zero
            // yields zero. Given an anchor, that is the whole answer even for
            // a calendar unit, with no calendar arithmetic involved.
            if anchor.is_some() && record.sign() == 0 {
                return self.temporal_duration_create([0; 10]);
            }
            // A `Plain` anchor: add the whole duration to it (days folded with
            // the time part at 24 hours each) and take
            // `DifferencePlainDateTimeWithRounding` between the anchor at
            // midnight and where that lands -- the same routine
            // `PlainDateTime.prototype.until` runs
            // (`vm/temporal/plain_date_time_difference.rs`).
            if let Some(DurationAnchor::Plain { calendar, date }) = &anchor {
                let (origin, target) =
                    Self::temporal_duration_plain_endpoints(*calendar, *date, &record)?;
                let fields = plain_date_time_difference::difference_plain_date_time(
                    *calendar, origin, target, largest, increment, smallest, mode,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                })?;
                return self.temporal_duration_create(fields);
            }
            // No anchor at all: only a duration with no years/months/weeks
            // (and no such unit requested) has a fixed length.
            let needs_calendar = Self::temporal_duration_largest_unit(&record).is_calendar()
                || largest.is_calendar()
                || smallest.is_calendar();
            if needs_calendar {
                return Err(RuntimeError::RangeError(
                    "a Temporal.Duration with years, months or weeks needs a relativeTo anchor"
                        .into(),
                ));
            }
            let step = smallest
                .nanoseconds()
                .expect("a non-calendar smallestUnit always has an exact length")
                * increment;
            let balanced = duration_math::TimeDuration::from_record_with_24_hour_days(&record)
                .rounded_to_step(step, mode)
                .balance_with_days(largest);
            self.temporal_duration_create([
                0,
                0,
                0,
                balanced[0],
                balanced[1],
                balanced[2],
                balanced[3],
                balanced[4],
                balanced[5],
                balanced[6],
            ])
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn temporal_duration_total(
        &mut self,
        receiver: &Value,
        total_of: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let (shorthand, options) = self.temporal_duration_round_to(total_of, "total")?;
            let (unit, anchor) = match &shorthand {
                Some(text) => (Self::temporal_duration_unit_name(text, "unit")?, None),
                None => {
                    let relative_to = self.get_property(&options, &"relativeTo".into())?;
                    let anchor = self.temporal_duration_relative_to(&relative_to)?;
                    let unit = self
                        .temporal_duration_unit_option(&options, "unit", false)?
                        .unit()
                        .ok_or_else(|| {
                            RuntimeError::RangeError(
                                "Temporal.Duration.prototype.total requires unit".into(),
                            )
                        })?;
                    (unit, anchor)
                }
            };
            // See `round` above for why a `Zoned` anchor is dispatched before
            // even the blank-duration shortcut: resolving its real day
            // length/bracket can itself throw. `total` is the same target
            // instant followed by `DifferenceZonedDateTimeWithTotal`.
            if let Some(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns,
                local_date,
                local_time,
            }) = &anchor
            {
                let target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &record,
                )?;
                let origin = zoned_difference::ZonedOrigin {
                    zone,
                    calendar: *calendar,
                    epoch_nanoseconds: epoch_ns,
                    date: *local_date,
                    time: *local_time,
                };
                let (numerator, denominator) = zoned_difference::difference_with_total(
                    &origin, &target, unit,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                })?;
                return Ok(Value::Number(rounding::exact_ratio_to_f64(
                    numerator,
                    denominator,
                )));
            }
            // A blank duration totals zero in every unit; see `round` above.
            if anchor.is_some() && record.sign() == 0 {
                return Ok(Value::Number(0.0));
            }
            // A `Plain` anchor: `DifferencePlainDateTimeWithTotal` between the
            // anchor at midnight and where the whole duration lands (see `round`).
            if let Some(DurationAnchor::Plain { calendar, date }) = &anchor {
                let (origin, target) =
                    Self::temporal_duration_plain_endpoints(*calendar, *date, &record)?;
                let (numerator, denominator) =
                    plain_date_time_difference::difference_plain_date_time_total(
                        *calendar, origin, target, unit,
                    )
                    .ok_or_else(|| {
                        RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                    })?;
                return Ok(Value::Number(rounding::exact_ratio_to_f64(
                    numerator,
                    denominator,
                )));
            }
            let needs_calendar =
                Self::temporal_duration_largest_unit(&record).is_calendar() || unit.is_calendar();
            if needs_calendar {
                return Err(RuntimeError::RangeError(
                    "a Temporal.Duration with years, months or weeks needs a relativeTo anchor"
                        .into(),
                ));
            }
            Ok(Value::Number(
                duration_math::TimeDuration::from_record_with_24_hour_days(&record).total_in(unit),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn temporal_duration_compare(
        &mut self,
        one: &Value,
        two: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_duration_from_value(one)?;
        let two = self.temporal_duration_from_value(two)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_duration_options(options)?;
            let relative_to = self.get_property(&options, &"relativeTo".into())?;
            let anchor = self.temporal_duration_relative_to(&relative_to)?;
            // Field-identical durations compare equal before any unit is
            // considered, so even a calendar-unit duration compares to itself.
            if one == two {
                return Ok(Value::Number(0.0));
            }
            // A `Zoned` anchor, once either operand has a date part (years,
            // months, weeks or days -- `duration1.date != {}`): a day is not a
            // fixed 24 hours there (`twenty-five-hour-day.js`), so
            // `AddZonedDateTime` each operand's *full* duration to the same
            // anchor and compare the two resulting exact instants. Two purely
            // time-based durations never need the zone at all, so -- unlike the
            // date-part case -- they must not even range-check the anchor's
            // target instant (`relativeto-string-limits.js`: 5 minutes relative
            // to the last representable instant compares fine).
            let has_date_part = |record: &blueice_ecma402::DurationRecord| {
                record.years != 0 || record.months != 0 || record.weeks != 0 || record.days != 0
            };
            if let (
                Some(DurationAnchor::Zoned {
                    calendar,
                    zone,
                    epoch_ns,
                    local_date,
                    local_time,
                }),
                true,
            ) = (&anchor, has_date_part(&one) || has_date_part(&two))
            {
                let one_target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &one,
                )?;
                let two_target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &two,
                )?;
                return Ok(Value::Number(match one_target.cmp(&two_target) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                }));
            }
            let needs_calendar = Self::temporal_duration_largest_unit(&one).is_calendar()
                || Self::temporal_duration_largest_unit(&two).is_calendar();
            if needs_calendar {
                let anchor = anchor.ok_or_else(|| {
                    RuntimeError::RangeError(
                        "a Temporal.Duration with years, months or weeks needs a relativeTo \
                         anchor"
                            .into(),
                    )
                })?;
                // The anchor is only converted to a date-time (and so judged
                // against its tighter range) once calendar arithmetic needs it:
                // `DateDurationDays` returns a plain day count without touching
                // the anchor when there are no years, months or weeks
                // (`relativeto-string-limits.js`).
                Self::temporal_duration_anchor_datetime_in_range(anchor.date())?;
                let calendar = anchor.calendar();
                let anchor_date = anchor.date();
                // Both operands land relative to the *same* anchor, so their
                // exact `(whole days, sub-day nanoseconds)` pairs compare
                // lexicographically exactly as their true nanosecond totals
                // would (each remainder's magnitude stays under one day).
                let (one_date, one_ns) =
                    Self::temporal_duration_intermediate(calendar, anchor_date, &one)?;
                let (two_date, two_ns) =
                    Self::temporal_duration_intermediate(calendar, anchor_date, &two)?;
                let one_days = plain_date::iso_date_to_epoch_days(one_date)
                    - plain_date::iso_date_to_epoch_days(anchor_date);
                let two_days = plain_date::iso_date_to_epoch_days(two_date)
                    - plain_date::iso_date_to_epoch_days(anchor_date);
                return Ok(Value::Number(
                    match (one_days, one_ns).cmp(&(two_days, two_ns)) {
                        std::cmp::Ordering::Less => -1.0,
                        std::cmp::Ordering::Equal => 0.0,
                        std::cmp::Ordering::Greater => 1.0,
                    },
                ));
            }
            let one = duration_math::TimeDuration::from_record_with_24_hour_days(&one)
                .total_nanoseconds();
            let two = duration_math::TimeDuration::from_record_with_24_hour_days(&two)
                .total_nanoseconds();
            Ok(Value::Number(match one.cmp(&two) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            }))
        })();
        self.stack.truncate(base);
        result
    }

    /// `GetTemporalFractionalSecondDigitsOption`: `auto` (the default, `None`
    /// here) or a digit count in `0..=9`. A non-Number value must stringify to
    /// exactly `"auto"`; a Number is floored rather than required to be
    /// integral.
    pub(in super::super) fn temporal_duration_fractional_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalSecondDigits".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        if !matches!(value, Value::Number(_)) {
            let text = self
                .coerce_string(&value)?
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError("invalid fractionalSecondDigits".into()))?;
            if text == "auto" {
                return Ok(None);
            }
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        let digits = self.coerce_number(&value)?;
        if !digits.is_finite() {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        let count = digits.floor();
        if !(0.0..=9.0).contains(&count) {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        Ok(Some(count as u8))
    }

    /// `TemporalDurationToString`. `precision` is the number of fractional
    /// second digits to emit, or `None` for `auto` (emit only as many as the
    /// value needs, and none at all for a whole number of seconds).
    pub(in super::super) fn format_duration_string(
        record: &blueice_ecma402::DurationRecord,
        precision: Option<u8>,
    ) -> String {
        let mut date = String::new();
        for (value, suffix) in [
            (record.years, 'Y'),
            (record.months, 'M'),
            (record.weeks, 'W'),
            (record.days, 'D'),
        ] {
            if value != 0 {
                date.push_str(&value.unsigned_abs().to_string());
                date.push(suffix);
            }
        }
        let mut time = String::new();
        for (value, suffix) in [(record.hours, 'H'), (record.minutes, 'M')] {
            if value != 0 {
                time.push_str(&value.unsigned_abs().to_string());
                time.push(suffix);
            }
        }
        // Seconds and every sub-second field are one exact quantity: 1,500
        // milliseconds serializes as `1.5S`, and 9,007,199,254,740,991
        // milliseconds must not lose precision on the way there.
        let subsecond_total = record.seconds * 1_000_000_000
            + record.milliseconds * 1_000_000
            + record.microseconds * 1_000
            + record.nanoseconds;
        let seconds = subsecond_total / 1_000_000_000;
        let fraction = (subsecond_total % 1_000_000_000).unsigned_abs();
        let only_seconds = date.is_empty() && time.is_empty();
        if seconds != 0 || fraction != 0 || only_seconds || precision.is_some() {
            time.push_str(&seconds.unsigned_abs().to_string());
            let digits = format!("{fraction:09}");
            match precision {
                None if fraction != 0 => {
                    time.push('.');
                    time.push_str(digits.trim_end_matches('0'));
                }
                Some(count) if count > 0 => {
                    time.push('.');
                    time.push_str(&digits[..usize::from(count)]);
                }
                _ => {}
            }
            time.push('S');
        }
        let mut result = String::new();
        if record.sign() < 0 {
            result.push('-');
        }
        result.push('P');
        result.push_str(&date);
        if !time.is_empty() {
            result.push('T');
            result.push_str(&time);
        }
        result
    }

    pub(in super::super) fn temporal_duration_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_duration_options(options)?;
            let digits = self.temporal_duration_fractional_digits(&options)?;
            let mode =
                self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
            let smallest = self.temporal_duration_unit_option(&options, "smallestUnit", false)?;
            // `ToSecondsStringPrecisionRecord`: a smallestUnit pins both the
            // emitted digit count and the rounding unit; a digit count alone
            // pins the digits and derives a unit plus increment from them.
            let (precision, unit, increment) = match smallest.unit() {
                Some(rounding::TemporalUnit::Second) => {
                    (Some(0), rounding::TemporalUnit::Second, 1)
                }
                Some(rounding::TemporalUnit::Millisecond) => {
                    (Some(3), rounding::TemporalUnit::Millisecond, 1)
                }
                Some(rounding::TemporalUnit::Microsecond) => {
                    (Some(6), rounding::TemporalUnit::Microsecond, 1)
                }
                Some(rounding::TemporalUnit::Nanosecond) => {
                    (Some(9), rounding::TemporalUnit::Nanosecond, 1)
                }
                Some(_) => {
                    return Err(RuntimeError::RangeError(
                        "Temporal.Duration.prototype.toString accepts a smallestUnit of second or \
                         smaller"
                            .into(),
                    ));
                }
                None => match digits {
                    None => (None, rounding::TemporalUnit::Nanosecond, 1),
                    Some(0) => (Some(0), rounding::TemporalUnit::Second, 1),
                    Some(count @ 1..=3) => (
                        Some(count),
                        rounding::TemporalUnit::Millisecond,
                        10_i128.pow(u32::from(3 - count)),
                    ),
                    Some(count @ 4..=6) => (
                        Some(count),
                        rounding::TemporalUnit::Microsecond,
                        10_i128.pow(u32::from(6 - count)),
                    ),
                    Some(count) => (
                        Some(count),
                        rounding::TemporalUnit::Nanosecond,
                        10_i128.pow(u32::from(9 - count)),
                    ),
                },
            };
            if unit == rounding::TemporalUnit::Nanosecond && increment == 1 {
                // Nothing to round: serialize the record exactly as stored,
                // which is what keeps a maximal seconds-plus-nanoseconds pair
                // in range instead of balancing it out of range.
                return Ok(Value::String(
                    Self::format_duration_string(&record, precision).into(),
                ));
            }
            // Rounding the time part can carry into `days`, but never past
            // them: `largestUnit` here is the duration's own largest unit (at
            // least `second`), and the date fields are carried through
            // untouched.
            let largest =
                Self::temporal_duration_largest_unit(&record).max(rounding::TemporalUnit::Second);
            let step = unit
                .nanoseconds()
                .expect("second and smaller units have an exact length")
                * increment;
            let balanced = duration_math::TimeDuration::from_fields(
                record.hours,
                record.minutes,
                record.seconds,
                record.milliseconds,
                record.microseconds,
                record.nanoseconds,
            )
            .rounded_to_step(step, mode)
            .balance_with_days(largest.min(rounding::TemporalUnit::Day));
            let rounded = Self::temporal_duration_record([
                record.years,
                record.months,
                record.weeks,
                record.days + balanced[0],
                balanced[1],
                balanced[2],
                balanced[3],
                balanced[4],
                balanced[5],
                balanced[6],
            ])?;
            Ok(Value::String(
                Self::format_duration_string(&rounded, precision).into(),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    /// ECMA-402's `Temporal.Duration.prototype.toLocaleString`: build an
    /// `Intl.DurationFormat` from the same `(locales, options)` arguments and
    /// format this duration with it, rather than returning the ISO string
    /// ECMA-262's own non-402 definition would.
    pub(in super::super) fn temporal_duration_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let formatter = self.duration_format_for_locale_string(args)?;
            self.stack.push(formatter.clone());
            let duration =
                self.alloc_temporal_value(Self::temporal_duration_value(record), false)?;
            self.stack.push(duration.clone());
            self.duration_format_format(&formatter, &duration)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn temporal_duration_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.Duration cannot be converted to a primitive value".into(),
        ))
    }
}
