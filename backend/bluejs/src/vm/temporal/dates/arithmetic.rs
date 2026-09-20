// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `add`/`subtract` and the `until`/`since` difference dispatch for `PlainDate`
//! and `PlainDateTime` (option reading and validation; the numeric core is
//! `plain_date_time_difference`).

use super::super::*;

impl Vm {
    /// `Temporal.PlainDate.prototype.add`/`subtract`,
    /// `Temporal.PlainDateTime.prototype.add`/`subtract`. Years/months/weeks
    /// carry through the calendar first; every time-of-day unit (including a
    /// bare `days` field) then folds into a flat day/nanosecond offset —
    /// `PlainDate/prototype/add/balance-smaller-units.js` pins the 24-hour
    /// fold for a receiver with no time to preserve, and a `PlainDateTime`
    /// receiver's own time of day genuinely advances (with day carry) rather
    /// than being discarded.
    pub(in super::super::super) fn temporal_date_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
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
        let start = (existing.year, existing.month, existing.day);
        let time_total = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        )
        .total_nanoseconds();
        const DAY_NS: i128 = 86_400_000_000_000;
        let (total_days, time_fields) = if existing.kind == TemporalKind::PlainDateTime {
            let existing_ns = duration_math::time_fields_to_nanoseconds(
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            );
            let combined = existing_ns + time_total;
            let day_carry = combined.div_euclid(DAY_NS);
            let ns_of_day = combined.rem_euclid(DAY_NS);
            (
                duration.days + day_carry,
                Some(duration_math::time_fields_from_nanoseconds(ns_of_day)),
            )
        } else {
            (duration.days + time_total / DAY_NS, None)
        };
        let result_date = plain_date::calendar_add_date(
            calendar_kind,
            start,
            duration.years as i64,
            duration.months as i64,
            duration.weeks as i64,
            total_days as i64,
            reject,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        // `calendar_add_date` only range-checks via `regulate_iso_date`/
        // `balance_iso_date` (an i32-year/valid-month-day check), not
        // Temporal's own narrower representable range
        // (`-271821-04-19`..`+275760-09-13`, exclusive at the exact
        // day-and-nanosecond boundary for `PlainDateTime`) -- confirmed by a
        // real `add/limits.js` failure: subtracting one day from the exact
        // minimum `PlainDate` silently produced a valid-but-unrepresentable
        // `-271821-04-18` instead of throwing. `alloc_temporal_value`
        // performs no range validation of its own, matching the same gap
        // `Temporal.PlainDateTime.prototype.round` had.
        let in_range = match &time_fields {
            Some(time) => epoch::is_date_time_within_limits(result_date, *time),
            None => epoch::is_date_within_limits(result_date),
        };
        if !in_range {
            return Err(RuntimeError::RangeError(
                "Temporal date arithmetic is out of range".into(),
            ));
        }
        let value = match time_fields {
            Some(time) => Self::temporal_date_time_value(
                existing.kind,
                existing.calendar.clone(),
                result_date,
                time,
            ),
            None => {
                Self::temporal_date_value(existing.kind, existing.calendar.clone(), result_date)
            }
        };
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainDate.prototype.until`/`since`,
    /// `Temporal.PlainDateTime.prototype.until`/`since`.
    pub(in super::super::super) fn temporal_date_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let other = self.temporal_to_matching(other_value, existing.kind, &Value::Undefined)?;
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

        let unit_floor = if existing.kind == TemporalKind::PlainDateTime {
            rounding::TemporalUnit::Nanosecond
        } else {
            rounding::TemporalUnit::Day
        };
        let default_smallest = unit_floor;
        let smallest_unit = match smallest_raw.as_deref() {
            None => default_smallest,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        if smallest_unit < unit_floor {
            return Err(RuntimeError::RangeError(
                "smallestUnit is out of range for this receiver".into(),
            ));
        }
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => smallest_unit.max(rounding::TemporalUnit::Day),
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if largest_unit < unit_floor {
            return Err(RuntimeError::RangeError(
                "largestUnit is out of range for this receiver".into(),
            ));
        }
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        // `ValidateTemporalRoundingIncrement(increment, maximum, false)`: a
        // time-unit increment must be strictly smaller than, and divide
        // evenly into, the count of that unit in the next larger one
        // (`PlainDateTime` is the only receiver whose `smallestUnit` can be
        // a time unit; the date units have no such maximum, so a `PlainDate`
        // never reaches this check with a `Some`).
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
        // Both difference paths below (`difference_plain_date` for a
        // `PlainDate`, `difference_plain_date_time` for a `PlainDateTime`)
        // round a *real*, direction-aware signed
        // quantity computed in the fixed receiver-to-argument direction —
        // `Ceil`/`Floor` round toward a fixed end of the real number line
        // (`ceil(-x) == -floor(x)`, not `-ceil(x)`), and `HalfCeil`/
        // `HalfFloor` are the half-mode analogue. Negating the *result* for
        // `since` without also reflecting an asymmetric mode here would
        // silently round the wrong way whenever `since` negates a
        // non-exact value — exactly the same bug
        // `temporal_year_month_difference` (`PlainYearMonth`) already had
        // fixed for it (see that function's own comment). Confirmed via
        // `built-ins/Temporal/{PlainDate,PlainDateTime}/prototype/since/
        // roundingmode-{ceil,floor}.js`. `Trunc`/`Expand`/`HalfExpand`/
        // `HalfTrunc`/`HalfEven` are all symmetric under negation and need
        // no reflection.
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

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        // `DifferenceTemporalPlainDate`/`DifferenceTemporalPlainDateTime`
        // always compute the difference in the fixed receiver-to-argument
        // direction, exactly like `until`, and only negate the *resulting*
        // Duration afterward for `since` (step 10). This must not be
        // implemented by swapping which date is `from`/`to`:
        // `CalendarDateUntil`'s own algorithm anchors on `from`'s
        // day-of-month while walking years/months, so it is not
        // anti-symmetric (`f(other, existing) != -f(existing, other)` in
        // general — verified against Test262's
        // `PlainDate/prototype/since/basic-gregory.js`, whose "23 years, 11
        // months and 29 days" case a swap-based `from`/`to` computes as 30
        // days instead of 29, because it anchors on the wrong date's day
        // field). `from`/`to` are therefore always `existing`/`other`, and
        // the whole result is negated below when `since` is true.
        let from = (existing.year, existing.month, existing.day);
        let to = (other.year, other.month, other.day);
        let mut fields: [i128; 10] = if existing.kind == TemporalKind::PlainDateTime {
            // The time-of-day borrow, the `largestUnit` folding of whole days
            // into time fields and every rounding step live in the host-neutral
            // `DifferencePlainDateTimeWithRounding` port.
            plain_date_time_difference::difference_plain_date_time(
                calendar_kind,
                (
                    from,
                    (
                        existing.hour,
                        existing.minute,
                        existing.second,
                        existing.millisecond,
                        existing.microsecond,
                        existing.nanosecond,
                    ),
                ),
                (
                    to,
                    (
                        other.hour,
                        other.minute,
                        other.second,
                        other.millisecond,
                        other.microsecond,
                        other.nanosecond,
                    ),
                ),
                largest_unit,
                increment,
                smallest_unit,
                effective_mode,
            )
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?
        } else {
            // `DifferenceTemporalPlainDate` is the same algorithm at midnight.
            plain_date_time_difference::difference_plain_date(
                calendar_kind,
                from,
                to,
                largest_unit,
                increment,
                smallest_unit,
                effective_mode,
            )
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?
        };
        // Step 10 of `DifferenceTemporalPlainDate`/`DifferenceTemporalPlainDateTime`:
        // `since` negates every field of the finished result.
        if since {
            fields = fields.map(|field| -field);
        }
        self.temporal_duration_create(fields)
    }
}
