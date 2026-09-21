// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `PlainDateTime`-only operations: extracting its date and time, `withPlainTime`
//! and `round`.

use super::super::*;

impl Vm {
    pub(in super::super::super) fn temporal_plain_date_time_to_plain_date(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let value = Self::temporal_date_value(
            TemporalKind::PlainDate,
            existing.calendar,
            (existing.year, existing.month, existing.day),
        );
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super::super) fn temporal_plain_date_time_to_plain_time(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let value = Self::plain_time_value((
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        ));
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super::super) fn temporal_plain_date_time_with_plain_time(
        &mut self,
        receiver: &Value,
        time_like: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let time = if *time_like == Value::Undefined {
            (0, 0, 0, 0, 0, 0)
        } else {
            self.temporal_to_plain_time(time_like, &Value::Undefined)?
        };
        let value = Self::temporal_date_time_value(
            TemporalKind::PlainDateTime,
            existing.calendar,
            (existing.year, existing.month, existing.day),
            time,
        );
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super::super) fn temporal_plain_date_time_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        if *round_to == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDateTime.round requires a smallestUnit or options argument".into(),
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
            // `PlainDateTime.prototype.round`'s `smallestUnit` spans
            // `"day"`..`"nanosecond"` (`RoundISODateTime`'s own unit range),
            // one wider than a bare `PlainTime`'s `"hour"`..`"nanosecond"` —
            // a real gap this fixed: every `smallestUnit: "day"` call
            // (`round/roundingmode-*.js`, `round/balance.js`,
            // `round/roundingincrement-one-day.js`, `round/limits.js`)
            // threw "invalid smallestUnit option" before this, since only
            // the narrower time-unit vocabulary was ever accepted.
            let smallest_unit_text = smallest_unit.as_deref().ok_or_else(|| {
                RuntimeError::RangeError(
                    "Temporal.PlainDateTime.round requires smallestUnit".into(),
                )
            })?;
            const DAY_NS: i128 = 86_400_000_000_000;
            let time_ns = duration_math::time_fields_to_nanoseconds(
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            );
            let (day_carry, ns_of_day) = if matches!(smallest_unit_text, "day" | "days") {
                // `ValidateTemporalRoundingIncrement(increment, 1, true)`:
                // day granularity has no finer subdivision to increment by
                // within this call (unlike `Temporal.Instant.round`'s own
                // day rule, which allows any divisor of a day) — only `1`
                // is ever valid.
                if increment != 1 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement must be 1 when smallestUnit is \"day\"".into(),
                    ));
                }
                let rounded = rounding::round_to_increment(time_ns, DAY_NS, mode);
                (rounded.div_euclid(DAY_NS), 0_i128)
            } else {
                let smallest_unit =
                    rounding::parse_time_unit(smallest_unit_text).ok_or_else(|| {
                        RuntimeError::RangeError("invalid smallestUnit option".into())
                    })?;
                Self::temporal_validated_plain_time_increment(increment, smallest_unit)?;
                let rounded = duration_math::TimeDuration::from_nanoseconds(time_ns)
                    .round(smallest_unit, increment, mode)
                    .total_nanoseconds();
                (rounded.div_euclid(DAY_NS), rounded.rem_euclid(DAY_NS))
            };
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
                RuntimeError::RangeError("Temporal.PlainDateTime.round is out of range".into())
            })?;
            let time = duration_math::time_fields_from_nanoseconds(ns_of_day);
            // `calendar_add_date` only range-checks the *calendar date*
            // (year/month/day); a rounded result can still fall outside
            // Temporal's exact day-and-nanosecond `PlainDateTime` boundary
            // while landing on an otherwise-representable date -- e.g.
            // flooring `-271821-04-19T00:00:00.000000001` (the actual
            // minimum representable `PlainDateTime`) to any unit rounds
            // its single nanosecond away, landing exactly on
            // `-271821-04-19T00:00:00.000000000`, a representable *date*
            // but not a representable `PlainDateTime` (`PlainDateTime/
            // from/argument-string-limits.js`'s own boundary). Confirmed
            // by a real `round/limits.js` failure — `alloc_temporal_value`
            // performs no range validation of its own, unlike
            // `temporal_value_from_args`'s construction path.
            if !epoch::is_date_time_within_limits(date, time) {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDateTime.round is out of range".into(),
                ));
            }
            let value = Self::temporal_date_time_value(
                TemporalKind::PlainDateTime,
                existing.calendar.clone(),
                date,
                time,
            );
            self.alloc_temporal_value(value, false)
        })();
        self.stack.truncate(base);
        result
    }

    // ---- Stage 1 Track C: Temporal.Now ----------------------------------
}
