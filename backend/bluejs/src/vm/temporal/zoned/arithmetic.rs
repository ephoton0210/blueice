// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.ZonedDateTime.prototype.{add, subtract, round}`.

use super::super::*;
use super::resolution::{
    temporal_resolution_error, temporal_set_local_fields, temporal_zoned_date_time_zone,
};

impl Vm {
    /// `Temporal.ZonedDateTime.prototype.add`/`subtract`: `AddZonedDateTime`
    /// (`vm/temporal/zoned_date_time.rs`) -- calendar years/months/weeks/days
    /// carried through the calendar at the receiver's own local date/time,
    /// re-resolved through the zone, and only then the exact time-duration
    /// nanoseconds added directly to that resolved instant.
    pub(in super::super::super) fn temporal_zoned_date_time_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let mut duration = self.temporal_duration_from_value(duration_value)?;
        if negate {
            duration.years = -duration.years;
            duration.months = -duration.months;
            duration.weeks = -duration.weeks;
            duration.days = -duration.days;
            duration.hours = -duration.hours;
            duration.minutes = -duration.minutes;
            duration.seconds = -duration.seconds;
            duration.milliseconds = -duration.milliseconds;
            duration.microseconds = -duration.microseconds;
            duration.nanoseconds = -duration.nanoseconds;
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let zone = temporal_zoned_date_time_zone(&existing);
        let time_total = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        )
        .total_nanoseconds();
        let local_date = (existing.year, existing.month, existing.day);
        let local_time = (
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        let result_ns = zoned_date_time::add_zoned_date_time(
            &zone,
            calendar_kind,
            &existing.epoch_nanoseconds,
            local_date,
            local_time,
            duration.years as i64,
            duration.months as i64,
            duration.weeks as i64,
            duration.days as i64,
            time_total,
            reject,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal.ZonedDateTime arithmetic is out of range".into())
        })?;
        if !epoch::is_in_instant_range(&result_ns) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime arithmetic is out of range".into(),
            ));
        }
        existing.epoch_nanoseconds = result_ns;
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
    }

    /// `Temporal.ZonedDateTime.prototype.round`: `RoundZonedDateTimeInstant`
    /// -- day-unit rounding anchors on `GetStartOfDay`'s real (possibly
    /// 23/25-hour) day boundary rather than a fixed 86,400-second one; every
    /// other unit rounds the local wall-clock time (`RoundISODateTime`'s own
    /// shape, matching `Temporal.PlainDateTime.prototype.round`), then
    /// re-resolves through the zone with `"compatible"` disambiguation.
    pub(in super::super::super) fn temporal_zoned_date_time_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        if *round_to == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime.round requires a smallestUnit or options argument".into(),
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
            let smallest_unit_text = smallest_unit.as_deref().ok_or_else(|| {
                RuntimeError::RangeError(
                    "Temporal.ZonedDateTime.round requires smallestUnit".into(),
                )
            })?;
            let zone = temporal_zoned_date_time_zone(&existing);
            if matches!(smallest_unit_text, "day" | "days") {
                if increment != 1 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement must be 1 when smallestUnit is \"day\"".into(),
                    ));
                }
                let date = (existing.year, existing.month, existing.day);
                let out_of_range = || {
                    RuntimeError::RangeError("Temporal.ZonedDateTime.round is out of range".into())
                };
                // `dateEnd` must itself be a representable date, and both
                // `GetStartOfDay` results a representable instant -- an
                // instance at the edge of the range has no upper (or lower)
                // bound to round toward (`day-rounding-out-of-range.js`,
                // `get-start-of-day-throws.js`).
                let next = plain_date::add_iso_date(date, 0, 0, 0, 1, false)
                    .filter(|next| epoch::is_date_within_limits(*next))
                    .ok_or_else(out_of_range)?;
                let start = zone.start_of_day(date);
                let end = zone.start_of_day(next);
                if !epoch::is_in_instant_range(&start) || !epoch::is_in_instant_range(&end) {
                    return Err(out_of_range());
                }
                let day_length = i128::try_from(&end - &start)
                    .expect("one day's length fits in i128 many times over");
                // `RoundZonedDateTime` step 19.f: when the wall-clock date's
                // midnight occurs twice (Antarctica/Casey turned its clocks
                // back across 2010-03-05T00:00), an instant on the *second*
                // occurrence is later than the next day's start; clamp it to the
                // last nanosecond of this day so rounding still lands on one of
                // its two start-of-day boundaries
                // (`same-date-starts-twice.js`).
                let last_of_day = &end - BigInt::from(1);
                let this_ns = std::cmp::min(&existing.epoch_nanoseconds, &last_of_day);
                let offset_into_day = i128::try_from(this_ns - &start)
                    .expect("an offset within one day fits in i128");
                let rounded =
                    rounding::round_to_increment_as_if_positive(offset_into_day, day_length, mode);
                existing.epoch_nanoseconds = start + BigInt::from(rounded);
            } else {
                let smallest_unit =
                    rounding::parse_time_unit(smallest_unit_text).ok_or_else(|| {
                        RuntimeError::RangeError("invalid smallestUnit option".into())
                    })?;
                // `ValidateTemporalRoundingIncrement(increment,
                // MaximumTemporalDurationRoundingIncrement(unit), false)`: unlike
                // `Instant.round` (whose increment may be a whole day), the
                // increment must stay *below* the count of this unit in the next
                // larger one and divide it -- `{ smallestUnit: "hour",
                // roundingIncrement: 24 }` throws
                // (`throws-on-invalid-increments.js`).
                let maximum = smallest_unit.increment_dividend();
                if increment >= maximum || maximum % increment != 0 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement does not divide evenly into the next larger unit".into(),
                    ));
                }
                let time_ns = duration_math::time_fields_to_nanoseconds(
                    existing.hour,
                    existing.minute,
                    existing.second,
                    existing.millisecond,
                    existing.microsecond,
                    existing.nanosecond,
                );
                let rounded = duration_math::TimeDuration::from_nanoseconds(time_ns)
                    .round(smallest_unit, increment, mode)
                    .total_nanoseconds();
                let day_carry = rounded.div_euclid(86_400_000_000_000);
                let ns_of_day = rounded.rem_euclid(86_400_000_000_000);
                let calendar_kind = calendar::calendar_kind(&existing.calendar)
                    .expect("Temporal values retain a validated calendar identifier");
                let date = plain_date::calendar_add_date(
                    calendar_kind,
                    (existing.year, existing.month, existing.day),
                    0,
                    0,
                    0,
                    day_carry as i64,
                    false,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal.ZonedDateTime.round is out of range".into())
                })?;
                let time = duration_math::time_fields_from_nanoseconds(ns_of_day);
                existing.epoch_nanoseconds = zone
                    .epoch_nanoseconds_for(date, time, time_zone::Disambiguation::Compatible)
                    .map_err(temporal_resolution_error)?;
            }
            if !epoch::is_in_instant_range(&existing.epoch_nanoseconds) {
                return Err(RuntimeError::RangeError(
                    "Temporal.ZonedDateTime.round is out of range".into(),
                ));
            }
            temporal_set_local_fields(&mut existing, &zone);
            self.alloc_temporal_value(existing, false)
        })();
        self.stack.truncate(base);
        result
    }
}
