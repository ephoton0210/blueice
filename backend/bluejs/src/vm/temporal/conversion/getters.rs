// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Temporal prototype accessors.

use super::super::*;

impl Vm {
    /// Every Temporal prototype accessor. `native_call` has already checked the
    /// receiver against the type of the prototype the getter was installed on
    /// (`NativeFunction::temporal_receiver_kind`), so `value.kind` here is
    /// always one this getter is defined for and no arm re-checks it.
    pub(in super::super::super) fn temporal_getter(
        &mut self,
        receiver: &Value,
        getter: native::TemporalGetter,
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal getter requires a Temporal receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal getter requires a Temporal receiver".into())
        })?;
        match getter {
            native::TemporalGetter::DurationYears
            | native::TemporalGetter::DurationMonths
            | native::TemporalGetter::DurationWeeks
            | native::TemporalGetter::DurationDays
            | native::TemporalGetter::DurationHours
            | native::TemporalGetter::DurationMinutes
            | native::TemporalGetter::DurationSeconds
            | native::TemporalGetter::DurationMilliseconds
            | native::TemporalGetter::DurationMicroseconds
            | native::TemporalGetter::DurationNanoseconds => {
                let duration = value
                    .duration
                    .as_deref()
                    .expect("Temporal.Duration values retain a duration record");
                let field = match getter {
                    native::TemporalGetter::DurationYears => duration.years,
                    native::TemporalGetter::DurationMonths => duration.months,
                    native::TemporalGetter::DurationWeeks => duration.weeks,
                    native::TemporalGetter::DurationDays => duration.days,
                    native::TemporalGetter::DurationHours => duration.hours,
                    native::TemporalGetter::DurationMinutes => duration.minutes,
                    native::TemporalGetter::DurationSeconds => duration.seconds,
                    native::TemporalGetter::DurationMilliseconds => duration.milliseconds,
                    native::TemporalGetter::DurationMicroseconds => duration.microseconds,
                    native::TemporalGetter::DurationNanoseconds => duration.nanoseconds,
                    _ => unreachable!("all Temporal.Duration getters are listed above"),
                };
                Ok(Value::Number(field as f64))
            }
            native::TemporalGetter::DurationSign | native::TemporalGetter::DurationBlank => {
                let sign = value
                    .duration
                    .as_deref()
                    .expect("Temporal.Duration values retain a duration record")
                    .sign();
                Ok(if getter == native::TemporalGetter::DurationSign {
                    Value::Number(sign.into())
                } else {
                    Value::Bool(sign == 0)
                })
            }
            native::TemporalGetter::CalendarId => Ok(Value::String(value.calendar.into())),
            native::TemporalGetter::EpochMilliseconds => {
                // `floor(epochNanoseconds / 10^6)`, not a truncation toward
                // zero: a pre-epoch instant's milliseconds round *down*
                // (Test262's `epochMilliseconds/basic.js`).
                let million = BigInt::from(1_000_000);
                let mut milliseconds = &value.epoch_nanoseconds / &million;
                if &value.epoch_nanoseconds % &million != BigInt::from(0)
                    && value.epoch_nanoseconds < BigInt::from(0)
                {
                    milliseconds -= 1;
                }
                milliseconds
                    .to_f64()
                    .map(Value::Number)
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal instant".into()))
            }
            native::TemporalGetter::EpochNanoseconds => Ok(Value::BigInt(value.epoch_nanoseconds)),
            native::TemporalGetter::TimeZoneId => Ok(Value::String(value.time_zone.into())),
            native::TemporalGetter::Hour
            | native::TemporalGetter::Minute
            | native::TemporalGetter::Second
            | native::TemporalGetter::Millisecond
            | native::TemporalGetter::Microsecond
            | native::TemporalGetter::Nanosecond => Ok(Value::Number(match getter {
                native::TemporalGetter::Hour => value.hour.into(),
                native::TemporalGetter::Minute => value.minute.into(),
                native::TemporalGetter::Second => value.second.into(),
                native::TemporalGetter::Millisecond => value.millisecond.into(),
                native::TemporalGetter::Microsecond => value.microsecond.into(),
                native::TemporalGetter::Nanosecond => value.nanosecond.into(),
                _ => unreachable!("all Temporal.PlainTime getters are listed above"),
            })),
            native::TemporalGetter::DayOfWeek
            | native::TemporalGetter::DayOfYear
            | native::TemporalGetter::WeekOfYear
            | native::TemporalGetter::YearOfWeek
            | native::TemporalGetter::DaysInWeek => {
                // `dayOfWeek`/`daysInWeek` are calendar-invariant: every
                // supported calendar uses the ISO 7-day week. `dayOfYear` is a
                // position within the *calendar's own* year, and the week
                // numbers are ISO 8601's, defined for `iso8601` only -- every
                // other calendar reports `undefined`, `gregory` included.
                let date = (value.year, value.month, value.day);
                let iso = value.calendar == "iso8601";
                Ok(match getter {
                    native::TemporalGetter::DayOfWeek => {
                        Value::Number(plain_date::iso_day_of_week(date).into())
                    }
                    native::TemporalGetter::DayOfYear if iso => {
                        Value::Number(plain_date::iso_day_of_year(date).into())
                    }
                    native::TemporalGetter::DayOfYear => {
                        let calendar = calendar::calendar_kind(&value.calendar)
                            .expect("Temporal values retain a validated calendar identifier");
                        Value::Number(calendar::calendar_day_of_year(calendar, date).into())
                    }
                    native::TemporalGetter::WeekOfYear if iso => {
                        Value::Number(plain_date::iso_week_of_year(date).0.into())
                    }
                    native::TemporalGetter::YearOfWeek if iso => {
                        Value::Number(plain_date::iso_week_of_year(date).1.into())
                    }
                    native::TemporalGetter::WeekOfYear | native::TemporalGetter::YearOfWeek => {
                        Value::Undefined
                    }
                    native::TemporalGetter::DaysInWeek => Value::Number(7.0),
                    _ => unreachable!("all week-date getters are listed above"),
                })
            }
            native::TemporalGetter::OffsetNanoseconds | native::TemporalGetter::Offset => {
                let zone = temporal_zoned_date_time_zone(&value);
                let offset = zone.offset_nanoseconds_for(&value.epoch_nanoseconds);
                Ok(if getter == native::TemporalGetter::OffsetNanoseconds {
                    Value::Number(offset as f64)
                } else {
                    Value::String(format_offset_nanoseconds_exact(offset).into())
                })
            }
            native::TemporalGetter::HoursInDay => {
                let zone = temporal_zoned_date_time_zone(&value);
                let date = (value.year, value.month, value.day);
                // `GetStartOfDay` of today and of tomorrow: at the edge of the
                // representable range either can be unrepresentable, which is a
                // `RangeError` rather than a length
                // (`hoursInDay/get-start-of-day-throws.js`,
                // `hoursInDay/next-day-out-of-range.js`).
                let (start, end) =
                    zoned_date_time::checked_day_bounds(&zone, date).ok_or_else(|| {
                        RuntimeError::RangeError(
                            "Temporal.ZonedDateTime.hoursInDay is outside the supported range"
                                .into(),
                        )
                    })?;
                let length = i128::try_from(&end - &start)
                    .expect("one day's length fits in i128 many times over");
                Ok(Value::Number(length as f64 / 3_600_000_000_000.0))
            }
            getter => {
                let fields = self.temporal_calendar_fields(&value)?;
                match getter {
                    native::TemporalGetter::Year => Ok(Value::Number(fields.year.into())),
                    native::TemporalGetter::Month => Ok(Value::Number(fields.month.into())),
                    native::TemporalGetter::MonthCode => {
                        Ok(Value::String(fields.month_code.into()))
                    }
                    native::TemporalGetter::Day => Ok(Value::Number(fields.day.into())),
                    native::TemporalGetter::Era => Ok(fields
                        .era
                        .map_or(Value::Undefined, |era| Value::String(era.into()))),
                    native::TemporalGetter::EraYear => Ok(fields
                        .era_year
                        .map_or(Value::Undefined, |year| Value::Number(year.into()))),
                    native::TemporalGetter::MonthsInYear => {
                        Ok(Value::Number(fields.months_in_year.into()))
                    }
                    native::TemporalGetter::DaysInMonth => {
                        Ok(Value::Number(fields.days_in_month.into()))
                    }
                    native::TemporalGetter::DaysInYear => {
                        Ok(Value::Number(fields.days_in_year.into()))
                    }
                    native::TemporalGetter::InLeapYear => Ok(Value::Bool(fields.in_leap_year)),
                    _ => unreachable!("every other Temporal getter is handled above"),
                }
            }
        }
    }
}
