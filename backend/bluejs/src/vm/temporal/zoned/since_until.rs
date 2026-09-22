// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.ZonedDateTime.prototype.{until, since, equals}` and `compare`.
//!
//! The calendar and rounding arithmetic itself lives in the host-neutral
//! [`zoned_difference`](super::super::zoned_difference); this is its
//! option-reading and `Duration`-building adapter.

use super::super::*;
use super::resolution::temporal_zoned_date_time_zone;

impl Vm {
    /// `DifferenceTemporalZonedDateTime`'s field-level core: `TemporalDurationFromInternal`
    /// of `DifferenceZonedDateTimeWithRounding(receiver, other, ...)` — returned
    /// as the ten Duration fields, in the receiver-to-argument direction.
    ///
    /// This is deliberately the same pipeline `Temporal.Duration.prototype.
    /// round`/`total` run for a `ZonedDateTime` `relativeTo` (see
    /// [`zoned_difference`]): the specification defines all four in terms of it,
    /// and none of them may treat a named zone's day as a fixed 24 hours.
    pub(in super::super::super) fn temporal_zoned_date_time_difference_fields(
        origin: &zoned_difference::ZonedOrigin,
        other_epoch_ns: &BigInt,
        largest_unit: rounding::TemporalUnit,
        smallest_unit: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<[i64; 10], RuntimeError> {
        // `DifferenceTemporalZonedDateTime` step 8: equal instants are a blank
        // duration *before* any calendar-day bracketing happens -- not just a
        // fast path but a real spec-ordering requirement (and what keeps
        // `same-epoch-nanoseconds.js`, 660 unit/zone combinations at one
        // instant, inside the Test262 harness's instruction budget).
        if origin.epoch_nanoseconds == other_epoch_ns {
            return Ok([0; 10]);
        }
        let internal = zoned_difference::difference_with_rounding(
            origin,
            other_epoch_ns,
            largest_unit,
            increment,
            smallest_unit,
            mode,
        )
        .ok_or_else(|| RuntimeError::RangeError("Temporal.since/until is out of range".into()))?;
        Ok(internal.into_fields(largest_unit))
    }

    pub(in super::super::super) fn temporal_zoned_date_time_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let other = self.temporal_to_zoned_date_time(other_value, &Value::Undefined)?;
        if existing.calendar != other.calendar {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same calendar".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let largest_raw = self.temporal_raw_string_option(&resolved_options, "largestUnit")?;
        let increment_raw =
            self.temporal_raw_number_option(&resolved_options, "roundingIncrement")?;
        let mode_raw = self.temporal_raw_string_option(&resolved_options, "roundingMode")?;
        let smallest_raw = self.temporal_raw_string_option(&resolved_options, "smallestUnit")?;

        let smallest_unit = match smallest_raw.as_deref() {
            None => rounding::TemporalUnit::Nanosecond,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        // `ZonedDateTime`'s own default `largestUnit` is the larger of
        // `"hour"` and `smallestUnit` -- unlike `Instant`'s `"second"` and
        // `PlainDate`/`PlainDateTime`'s `"day"` defaults.
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => smallest_unit.max(rounding::TemporalUnit::Hour),
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        // `DifferenceTemporalZonedDateTime` only requires `TimeZoneEquals`
        // (canonical zone identity, not raw spelling -- see
        // `TimeZone::time_zone_equals`'s own doc comment) once `largestUnit`
        // is `"day"` or coarser -- a pure time-unit difference (`largestUnit`
        // finer than `"day"`, the branch
        // `temporal_zoned_date_time_difference_fields` itself takes for
        // `largest_unit < TemporalUnit::Day`) is a plain epoch-instant
        // subtraction that never consults either operand's zone at all, so
        // two `ZonedDateTime`s in genuinely different zones may still be
        // diffed that way (`zoneddatetime-string.js`/
        // `argument-string-time-zone-annotation.js`, both using the default
        // `"hour"` largest unit -- checking zone equality unconditionally
        // regressed exactly these). Calendar-unit bracketing below, by
        // contrast, only ever resolves through the *receiver's* own zone, so
        // mismatched zones there must be rejected
        // (`canonicalize-iana-identifiers-before-comparing.js`: two IANA
        // aliases of the same real zone must not throw, but two genuinely
        // different zones must).
        if largest_unit >= rounding::TemporalUnit::Day
            && !temporal_zoned_date_time_zone(&existing)
                .time_zone_equals(&temporal_zoned_date_time_zone(&other))
        {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same time zone".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        // `GetDifferenceSettings`' last step: a time unit's increment must be
        // below, and divide, the count of it in the next larger unit
        // (`MaximumTemporalDurationRoundingIncrement`, `inclusive` false) --
        // `{ smallestUnit: "hours", roundingIncrement: 24 }` throws. Date units
        // have no such bound.
        if let Some(maximum) = smallest_unit.maximum_rounding_increment() {
            if increment >= maximum || maximum % increment != 0 {
                return Err(RuntimeError::RangeError(
                    "roundingIncrement does not divide evenly into the next larger unit".into(),
                ));
            }
        }
        let mode = Self::temporal_validated_rounding_mode(
            mode_raw.as_deref(),
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;
        // Same reflection `Vm::temporal_date_difference` needs, and for the
        // identical reason: `zoned_difference`'s rounding steps
        // (`TimeDuration::round` for a sub-day `smallestUnit`,
        // `nudge_expand_decision` for a calendar-unit one) both round a *real*,
        // direction-aware signed quantity computed in the fixed
        // receiver-to-argument direction — `Ceil`/`Floor` round toward a fixed end of the real
        // number line, not toward a fixed end of whichever internal
        // direction happened to be computed — so negating the *result* for
        // `since` without also reflecting an asymmetric mode here would
        // silently round the wrong way. Confirmed via
        // `built-ins/Temporal/ZonedDateTime/prototype/since/
        // roundingmode-{ceil,floor,halfCeil,halfFloor}.js`.
        let effective_mode = if since {
            use blueice_ecma402::NumberRoundingMode as Mode;
            match mode {
                Mode::Ceil => Mode::Floor,
                Mode::Floor => Mode::Ceil,
                Mode::HalfCeil => Mode::HalfFloor,
                Mode::HalfFloor => Mode::HalfCeil,
                other => other,
            }
        } else {
            mode
        };

        let zone = temporal_zoned_date_time_zone(&existing);
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");

        let origin = zoned_difference::ZonedOrigin {
            zone: &zone,
            calendar: calendar_kind,
            epoch_nanoseconds: &existing.epoch_nanoseconds,
            date: (existing.year, existing.month, existing.day),
            time: (
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            ),
        };
        let fields = Self::temporal_zoned_date_time_difference_fields(
            &origin,
            &other.epoch_nanoseconds,
            largest_unit,
            smallest_unit,
            increment,
            effective_mode,
        )?;
        // `since` is `until` with the finished Duration negated.
        let fields = if since {
            fields.map(|field| -field)
        } else {
            fields
        };
        // `CreateTemporalDuration` rounds every field to the nearest float64
        // before its range check (`temporal_duration_record`): a difference
        // too large for a double to hold exactly is observably rounded
        // (`prototype/{since,until}/float64-representable-integer.js`).
        let record = Self::temporal_duration_record(fields.map(i128::from))?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let other = self.temporal_to_zoned_date_time(other_value, &Value::Undefined)?;
        // `TimeZoneEquals`: compares primary-zone identity, not raw stored
        // spelling -- an IANA alias and its target (`Asia/Calcutta` /
        // `Asia/Kolkata`) are the same zone even though each value's own
        // `time_zone` field preserves whichever spelling was written (see
        // `TimeZone::time_zone_equals`'s own doc comment).
        let existing_zone = temporal_zoned_date_time_zone(&existing);
        let other_zone = temporal_zoned_date_time_zone(&other);
        Ok(Value::Bool(
            existing.epoch_nanoseconds == other.epoch_nanoseconds
                && existing_zone.time_zone_equals(&other_zone)
                && existing.calendar == other.calendar,
        ))
    }

    pub(in super::super::super) fn temporal_zoned_date_time_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_to_zoned_date_time(one, &Value::Undefined)?;
        let two = self.temporal_to_zoned_date_time(two, &Value::Undefined)?;
        Ok(Value::Number(
            match one.epoch_nanoseconds.cmp(&two.epoch_nanoseconds) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.0,
            },
        ))
    }
}
