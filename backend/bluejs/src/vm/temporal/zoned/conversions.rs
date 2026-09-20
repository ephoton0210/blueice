// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.ZonedDateTime.prototype` conversions and field accessors: `valueOf`,
//! `toInstant`, the `toPlain*` family, `startOfDay`, `getISOFields` and
//! `getTimeZoneTransition`.

use super::super::*;
use super::formatting::format_offset_nanoseconds_exact;
use super::resolution::{
    temporal_checked_start_of_day, temporal_set_local_fields, temporal_zoned_date_time_zone,
};

impl Vm {
    pub(in super::super::super) fn temporal_zoned_date_time_value_of(
        &mut self,
    ) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.ZonedDateTime cannot be converted to a primitive value".into(),
        ))
    }

    pub(in super::super::super) fn temporal_zoned_date_time_to_instant(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        self.instant_from_epoch_nanoseconds(existing.epoch_nanoseconds)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_to_plain_date(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let value = Self::temporal_date_value(
            TemporalKind::PlainDate,
            existing.calendar,
            (existing.year, existing.month, existing.day),
        );
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_to_plain_time(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let fields = (
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        self.alloc_temporal_value(Self::plain_time_value(fields), false)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_to_plain_date_time(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let value = Self::temporal_date_time_value(
            TemporalKind::PlainDateTime,
            existing.calendar,
            (existing.year, existing.month, existing.day),
            (
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            ),
        );
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.ZonedDateTime.prototype.toPlainYearMonth`: identical
    /// `CalendarYearMonthFromFields` resolution to
    /// `temporal_plain_date_to_plain_year_month`, reused here since a
    /// `ZonedDateTime`'s own stored ISO fields are already its local
    /// calendar date -- `temporal_calendar_fields` does not care which
    /// `TemporalKind` supplied them.
    pub(in super::super::super) fn temporal_zoned_date_time_to_plain_year_month(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let fields = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let ym_fields = plain_year_month::YearMonthFields {
            era: None,
            era_year: None,
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &ym_fields, false)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar year-month".into()))?;
        let value =
            Self::temporal_date_value(TemporalKind::PlainYearMonth, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_to_plain_month_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let fields = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let md_fields = plain_month_day::MonthDayFields {
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
            day: fields.day,
            ..Default::default()
        };
        let date = plain_month_day::month_day_from_fields(calendar_kind, &md_fields, false)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()))?;
        let value = Self::temporal_date_value(TemporalKind::PlainMonthDay, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_start_of_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = temporal_zoned_date_time_zone(&existing);
        let date = (existing.year, existing.month, existing.day);
        existing.epoch_nanoseconds = temporal_checked_start_of_day(&zone, date)?;
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_get_iso_fields(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = temporal_zoned_date_time_zone(&existing);
        let offset_ns = zone.offset_nanoseconds_for(&existing.epoch_nanoseconds);
        let object = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            self.define_data(
                object,
                "calendar",
                Value::String(existing.calendar.clone().into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoDay",
                Value::Number(existing.day.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoHour",
                Value::Number(existing.hour.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMicrosecond",
                Value::Number(existing.microsecond.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMillisecond",
                Value::Number(existing.millisecond.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMinute",
                Value::Number(existing.minute.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoMonth",
                Value::Number(existing.month.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoNanosecond",
                Value::Number(existing.nanosecond.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoSecond",
                Value::Number(existing.second.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "isoYear",
                Value::Number(existing.year.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "offset",
                Value::String(format_offset_nanoseconds_exact(offset_ns).into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                object,
                "timeZone",
                Value::String(existing.time_zone.clone().into()),
                true,
                true,
                true,
            )?;
            Ok(Value::Object(object))
        })();
        self.stack.truncate(base);
        result
    }

    /// `Temporal.ZonedDateTime.prototype.getTimeZoneTransition`
    /// (`GetDirectionOption` + `GetNamedTimeZoneNextTransition`/
    /// `GetNamedTimeZonePreviousTransition`, delegating the actual real-data
    /// lookup to [`time_zone::TimeZone::adjacent_transition`]). `direction`
    /// is required (a `TypeError` if the argument itself is `undefined`,
    /// mirroring `Temporal.Instant.prototype.round`'s own `roundTo` shape);
    /// a bare String is shorthand for `{ direction: <string> }`, the same
    /// pattern [`Self::temporal_round_to`] already establishes for
    /// `roundTo`. `null` is the spec's own result for "no such transition",
    /// distinct from every other Temporal getter/method on this type.
    pub(in super::super::super) fn temporal_zoned_date_time_get_time_zone_transition(
        &mut self,
        receiver: &Value,
        direction_param: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        if *direction_param == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime.prototype.getTimeZoneTransition requires a direction"
                    .into(),
            ));
        }
        let options = if matches!(direction_param, Value::String(_)) {
            let object = self.with_roots(|heap| heap.alloc_object(None))?;
            let result = Value::Object(object);
            self.stack.push(result.clone());
            self.define_data(
                object,
                "direction",
                direction_param.clone(),
                true,
                true,
                true,
            )?;
            result
        } else {
            self.temporal_options(direction_param)?
        };
        let direction_v = self.get_property(&options, &"direction".into())?;
        if direction_v == Value::Undefined {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime.prototype.getTimeZoneTransition requires a direction \
                 option"
                    .into(),
            ));
        }
        let direction_s = self.coerce_string(&direction_v)?;
        let direction_s = direction_s
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid direction option".into()))?;
        let forward = match direction_s.as_str() {
            "next" => true,
            "previous" => false,
            _ => return Err(RuntimeError::RangeError("invalid direction option".into())),
        };
        let zone = temporal_zoned_date_time_zone(&existing);
        let Some(transition_ns) = zone.adjacent_transition(&existing.epoch_nanoseconds, forward)
        else {
            return Ok(Value::Null);
        };
        existing.epoch_nanoseconds = transition_ns;
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
    }
}
