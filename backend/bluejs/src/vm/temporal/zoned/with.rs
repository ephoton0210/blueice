// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.ZonedDateTime.prototype.{withTimeZone, withPlainTime, with}`.

use super::super::*;
use super::resolution::{
    temporal_checked_start_of_day, temporal_interpret_offset, temporal_require_instant_range,
    temporal_resolution_error, temporal_set_local_fields, temporal_zoned_date_time_zone,
};

impl Vm {
    pub(in super::super::super) fn temporal_zoned_date_time_with_time_zone(
        &mut self,
        receiver: &Value,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut value = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = self.temporal_time_zone(time_zone)?;
        // The instant itself is unchanged; only its presentation zone (and
        // therefore the local fields resolved from it) changes.
        value.time_zone = zone.identifier();
        temporal_set_local_fields(&mut value, &zone);
        self.alloc_temporal_value(value, false)
    }

    pub(in super::super::super) fn temporal_zoned_date_time_with_plain_time(
        &mut self,
        receiver: &Value,
        plain_time_like: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut value = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = temporal_zoned_date_time_zone(&value);
        let date = (value.year, value.month, value.day);
        value.epoch_nanoseconds = if *plain_time_like == Value::Undefined {
            // No time given: the result is the day's `GetStartOfDay`, which is
            // not the `compatible` resolution of local midnight when midnight
            // is skipped (`withPlainTime/dst-skipped-cross-midnight.js`).
            temporal_checked_start_of_day(&zone, date)?
        } else {
            let time = self.temporal_to_plain_time(plain_time_like, &Value::Undefined)?;
            let resolved = zone
                .epoch_nanoseconds_for(date, time, time_zone::Disambiguation::Compatible)
                .map_err(temporal_resolution_error)?;
            temporal_require_instant_range(resolved)?
        };
        temporal_set_local_fields(&mut value, &zone);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.ZonedDateTime.prototype.with`. Mirrors `temporal_date_with`'s
    /// own field-merge shape (deliberately re-derived here rather than
    /// shared, per this block's own "own textual block" rationale above),
    /// extended with the `offset` field and the `disambiguation`/`offset`
    /// options `PlainDate`/`PlainDateTime` have no notion of.
    pub(in super::super::super) fn temporal_zoned_date_time_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let like_object = like
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Temporal.with requires an object".into()))?;
        if self.heap.temporal_value(like_object)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal.with does not accept a Temporal-like object".into(),
            ));
        }
        for banned in ["calendar", "timeZone"] {
            if self.get_property(like, &banned.into())? != Value::Undefined {
                return Err(RuntimeError::TypeError(format!(
                    "Temporal.with does not accept a {banned} property"
                )));
            }
        }
        let existing_fields = self.temporal_calendar_fields(&existing)?;
        // `PrepareCalendarFields` reads and converts one field at a time, in
        // alphabetical order, each conversion straight after its own `Get`
        // (`with/order-of-operations.js`). Nothing object-valued is held in a
        // Rust local across a later call that can allocate, so a collection
        // triggered by the next field's getter or by reading `options` cannot
        // free a value that has not been converted yet.
        let requested_day = self.temporal_read_optional_integer(like, "day", 1, i32::MAX)?;
        // `iso8601` has no eras: its `era`/`eraYear` are never read.
        let (era_s, era_year_num) = if existing.calendar == "iso8601" {
            (None, None)
        } else {
            let era_s = self.temporal_read_optional_string(like, "era", "invalid Temporal era")?;
            let era_year_num =
                self.temporal_read_optional_integer(like, "eraYear", i32::MIN, i32::MAX)?;
            (era_s, era_year_num)
        };
        let requested_hour =
            self.temporal_read_optional_integer(like, "hour", i32::MIN, i32::MAX)?;
        let requested_microsecond =
            self.temporal_read_optional_integer(like, "microsecond", i32::MIN, i32::MAX)?;
        let requested_millisecond =
            self.temporal_read_optional_integer(like, "millisecond", i32::MIN, i32::MAX)?;
        let requested_minute =
            self.temporal_read_optional_integer(like, "minute", i32::MIN, i32::MAX)?;
        let requested_month = self.temporal_read_optional_integer(like, "month", 1, 99)?;
        let month_code_s =
            self.temporal_read_optional_string(like, "monthCode", "invalid Temporal month code")?;
        let requested_nanosecond =
            self.temporal_read_optional_integer(like, "nanosecond", i32::MIN, i32::MAX)?;
        let offset_string = self.temporal_read_optional_offset_string(like)?;
        let requested_second =
            self.temporal_read_optional_integer(like, "second", i32::MIN, i32::MAX)?;
        let requested_year = self.temporal_read_optional_integer(like, "year", -9_999, 9_999)?;
        if [
            requested_day,
            era_year_num,
            requested_hour,
            requested_microsecond,
            requested_millisecond,
            requested_minute,
            requested_month,
            requested_nanosecond,
            requested_second,
            requested_year,
        ]
        .iter()
        .all(Option::is_none)
            && era_s.is_none()
            && month_code_s.is_none()
            && offset_string.is_none()
        {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        // `GetTemporalDisambiguationOption`, `GetTemporalOffsetOption`, then
        // `GetTemporalOverflowOption`: the options object is read only after
        // every field, and in this order.
        let resolved_options = self.temporal_options(options)?;
        let disambiguation = self.temporal_disambiguation(&resolved_options)?;
        let offset_option = self.temporal_offset_option(&resolved_options, "prefer")?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let mut fields = DateFields::default();
        // The same three-way `iso8601`/`!calendar_supports_era`/era-supporting
        // split `temporal_date_with`/`temporal_year_month_with` already use
        // (see those functions' own doc comments for the full rationale):
        // `iso8601` has no eras at all and silently ignores `era`/`eraYear`;
        // `chinese`/`dangi` have no era concept either, but Temporal's own
        // rule is to *reject* any use of them there rather than ignore it;
        // every other calendar requires `era` and `eraYear` together or not
        // at all.
        if existing.calendar == "iso8601" {
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        } else if !calendar::calendar_supports_era(&existing.calendar) {
            if era_s.is_some() || era_year_num.is_some() {
                return Err(RuntimeError::TypeError(
                    "era and eraYear are not valid for this calendar".into(),
                ));
            }
            fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
        } else {
            match (era_s.as_deref(), era_year_num) {
                (Some(era), Some(era_year)) => {
                    fields.era = Some(era.as_bytes());
                    fields.era_year = Some(era_year);
                }
                (Some(_), None) => {
                    return Err(RuntimeError::TypeError(
                        "Temporal.with requires eraYear when era is provided".into(),
                    ));
                }
                (None, Some(_)) => {
                    return Err(RuntimeError::TypeError(
                        "Temporal.with requires era when eraYear is provided".into(),
                    ));
                }
                (None, None) => {
                    fields.extended_year = Some(requested_year.unwrap_or(existing_fields.year));
                }
            }
        }
        if let Some(month_code) = month_code_s.as_deref() {
            fields.month_code = Some(month_code.as_bytes());
        } else if let Some(month) = requested_month {
            fields.ordinal_month = Some(month as u8);
        } else {
            fields.month_code = Some(existing_fields.month_code.as_bytes());
        }
        // `ToPositiveIntegerWithTruncation` has no upper bound: `date.with({
        // day: daysInMonth + 1 })` must reach the calendar's own `overflow`
        // regulation, per `wrapping-at-end-of-month-*.js`.
        fields.day = Some(requested_day.unwrap_or(i32::from(existing_fields.day)) as u8);

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(if reject {
            icu_calendar::options::Overflow::Reject
        } else {
            icu_calendar::options::Overflow::Constrain
        });
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        if requested_year.is_some_and(|year| year != date.year().extended_year())
            || (month_code_s.is_some()
                && requested_month.is_some_and(|month| month as u8 != date.month().ordinal))
        {
            return Err(RuntimeError::RangeError(
                "inconsistent Temporal calendar fields".into(),
            ));
        }
        let mut result = Self::temporal_value_from_calendar_date(
            TemporalKind::ZonedDateTime,
            existing.calendar.clone(),
            date,
        );
        // `RegulateTime`: every time field was read unbounded above (so its own
        // conversion could not throw `RangeError` before `overflow` was known);
        // `constrain` now clamps it into range and `reject` throws.
        let regulate = |requested: Option<i32>, current: i32, maximum: i32| match requested {
            None => Ok(current),
            Some(value) if (0..=maximum).contains(&value) => Ok(value),
            Some(_) if reject => Err(RuntimeError::RangeError("invalid Temporal time".into())),
            Some(value) => Ok(value.clamp(0, maximum)),
        };
        result.hour = regulate(requested_hour, i32::from(existing.hour), 23)? as u8;
        result.minute = regulate(requested_minute, i32::from(existing.minute), 59)? as u8;
        result.second = regulate(requested_second, i32::from(existing.second), 59)? as u8;
        result.millisecond =
            regulate(requested_millisecond, i32::from(existing.millisecond), 999)? as u16;
        result.microsecond =
            regulate(requested_microsecond, i32::from(existing.microsecond), 999)? as u16;
        result.nanosecond =
            regulate(requested_nanosecond, i32::from(existing.nanosecond), 999)? as u16;

        let zone = temporal_zoned_date_time_zone(&existing);
        let offset_nanoseconds = match offset_string.as_deref() {
            Some(text) => Some(
                iso::parse_offset_string_nanoseconds(text)
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal offset".into()))?,
            ),
            // `.with()`'s own default offset behaviour: an omitted `offset`
            // field keeps the receiver's *current real* offset as the
            // preferred one, rather than falling all the way back to plain
            // zone/disambiguation resolution -- what makes `{ hour: 2 }` on
            // a value observing a repeated local hour stay on the same side
            // of the transition it already was on, not jump to
            // `"compatible"`'s default choice.
            None => Some(zone.offset_nanoseconds_for(&existing.epoch_nanoseconds)),
        };
        let date_tuple = (result.year, result.month, result.day);
        let time_tuple = (
            result.hour,
            result.minute,
            result.second,
            result.millisecond,
            result.microsecond,
            result.nanosecond,
        );
        let epoch_nanoseconds = temporal_interpret_offset(
            &zone,
            date_tuple,
            time_tuple,
            offset_nanoseconds,
            false,
            disambiguation,
            &offset_option,
            false, // `.with()`'s own `offset` field is always `MatchExactly`.
        )?;
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime.with is out of range".into(),
            ));
        }
        result.epoch_nanoseconds = epoch_nanoseconds;
        result.time_zone = zone.identifier();
        temporal_set_local_fields(&mut result, &zone);
        self.alloc_temporal_value(result, false)
    }
}
