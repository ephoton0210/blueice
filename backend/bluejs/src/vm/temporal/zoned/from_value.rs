// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ZonedDateTime` receiver checks and `ToTemporalZonedDateTime`: the brand
//! check, the `offset` option, and conversion from an object, property bag or
//! ISO string.

use super::super::*;
use super::resolution::{
    temporal_checked_start_of_day, temporal_interpret_offset, temporal_set_local_fields,
};

/// What [`Vm::temporal_parse_zoned_date_time_string`] settles from a
/// `TemporalZonedDateTimeString` before `options` is read.
pub(in super::super::super) struct ParsedZonedDateTimeString {
    parsed: iso::Parsed,
    zone: time_zone::TimeZone,
    calendar: String,
}

impl Vm {
    /// Brand check shared by every `Temporal.ZonedDateTime.prototype` method.
    pub(in super::super::super) fn temporal_zoned_date_time_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.ZonedDateTime method requires a receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.ZonedDateTime method requires a receiver".into())
        })?;
        if value.kind != TemporalKind::ZonedDateTime {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime method requires a receiver".into(),
            ));
        }
        Ok(value)
    }

    /// `ToTemporalOffset`: reads the `offset` option, one of `"prefer"`/
    /// `"use"`/`"ignore"`/`"reject"`.
    pub(in super::super::super) fn temporal_offset_option(
        &mut self,
        options: &Value,
        default: &'static str,
    ) -> Result<String, RuntimeError> {
        Ok(self
            .temporal_string_option(options, "offset", &["prefer", "use", "ignore", "reject"])?
            .unwrap_or_else(|| default.to_string()))
    }

    /// `ToTemporalZonedDateTime`.
    ///
    /// The three argument shapes share one option protocol, and what is
    /// observable is its *order*: the argument itself is fully read (a
    /// `ZonedDateTime` is used as-is, a property bag has every field read and
    /// converted, a string is parsed) strictly before `options` is touched, and
    /// then `disambiguation`, `offset` and `overflow` are read in exactly that
    /// order (`from/order-of-operations.js`, `from/observable-get-overflow-*.js`,
    /// `from/options-read-before-algorithmic-validation.js`). A primitive
    /// `options` therefore throws only after the argument has been accepted.
    pub(in super::super::super) fn temporal_to_zoned_date_time(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    // Options are still read (for validation/ordering parity)
                    // even though a `ZonedDateTime` argument is used as-is.
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_disambiguation(&resolved_options)?;
                    self.temporal_offset_option(&resolved_options, "reject")?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(temporal);
                }
            }
            return self.temporal_zoned_date_time_from_bag(value, options);
        }
        // `ToTemporalZonedDateTime`'s non-object branch requires a literal
        // `String`, never `ToString`-coerced -- a `Number`/`Boolean`/`null`/
        // `BigInt`/`Symbol` argument is a `TypeError`, not an attempt to
        // stringify it first (`argument-wrong-type.js`: `1`/`19761118`/`1n`
        // are all `TypeError`s even though the latter would otherwise parse
        // as a valid-looking string). Matches `temporal_to_plain_date`'s own
        // identical guard.
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime-like value must be an object or a string".into(),
            ));
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.ZonedDateTime string".into())
        })?;
        let parsed = Self::temporal_parse_zoned_date_time_string(&source)?;
        let resolved_options = self.temporal_options(options)?;
        let disambiguation = self.temporal_disambiguation(&resolved_options)?;
        let offset_option = self.temporal_offset_option(&resolved_options, "reject")?;
        self.temporal_overflow_option(&resolved_options)?;
        Self::temporal_interpret_zoned_date_time_string(parsed, disambiguation, &offset_option)
    }

    /// `ToTemporalZonedDateTime`'s property-bag branch. `timeZone` is required,
    /// `offset` optional; every calendar-date and time-of-day field is the same
    /// set `temporal_calendar_date_from_bag` already resolves for
    /// `PlainDateTime` (a `ZonedDateTime`'s own field list per
    /// `PrepareCalendarFields`/`CalendarDateFromFields` is identical once
    /// `timeZone`/`offset` are set aside), so that resolution is reused -- with
    /// `offset` and `timeZone` read *inside* it, in their alphabetical position
    /// -- and only the result's `kind` and instant are filled in here.
    fn temporal_zoned_date_time_from_bag(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        let mut zoned = ZonedBagFields::new();
        let mut fields = self.temporal_calendar_date_from_bag(
            TemporalKind::PlainDateTime,
            value,
            OverflowInput::Options(options),
            Some(&mut zoned),
        )?;
        let zone = zoned
            .time_zone
            .expect("temporal_calendar_date_from_bag requires a timeZone");
        let date = (fields.year, fields.month, fields.day);
        let time = (
            fields.hour,
            fields.minute,
            fields.second,
            fields.millisecond,
            fields.microsecond,
            fields.nanosecond,
        );
        let epoch_nanoseconds = temporal_interpret_offset(
            &zone,
            date,
            time,
            zoned.offset_nanoseconds,
            false,
            zoned.disambiguation,
            &zoned.offset_option,
            false, // a property-bag `offset` field is always `MatchExactly`.
        )?;
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range".into(),
            ));
        }
        fields.kind = TemporalKind::ZonedDateTime;
        fields.epoch_nanoseconds = epoch_nanoseconds;
        fields.time_zone = zone.identifier();
        temporal_set_local_fields(&mut fields, &zone);
        Ok(fields)
    }

    /// `ParseTemporalZonedDateTimeString` + resolution: a
    /// `TemporalZonedDateTimeString` always carries a `TimeZoneAnnotation`
    /// (unlike every other Temporal string production, where one is at most
    /// optional), which is this method's real reason to exist separately
    /// from the generic `temporal_value_from_string` path every other type
    /// shares.
    pub(in super::super::super) fn temporal_value_from_zoned_date_time_string(
        source: &str,
        disambiguation: time_zone::Disambiguation,
        offset_option: &str,
    ) -> Result<TemporalValue, RuntimeError> {
        let parsed = Self::temporal_parse_zoned_date_time_string(source)?;
        Self::temporal_interpret_zoned_date_time_string(parsed, disambiguation, offset_option)
    }

    /// The half of `ToTemporalZonedDateTime`'s string branch that is settled
    /// from the string alone -- its syntax, the mandatory time-zone annotation,
    /// and the calendar -- and so must throw its `RangeError` *before* the
    /// `options` argument is read.
    pub(in super::super::super) fn temporal_parse_zoned_date_time_string(
        source: &str,
    ) -> Result<ParsedZonedDateTimeString, RuntimeError> {
        let parsed = iso::parse_date_time(source).ok_or_else(|| {
            RuntimeError::RangeError("invalid Temporal.ZonedDateTime string".into())
        })?;
        let annotation = parsed.time_zone.as_deref().ok_or_else(|| {
            RuntimeError::RangeError(
                "a Temporal.ZonedDateTime string requires a time zone annotation".into(),
            )
        })?;
        let zone = time_zone::parse_identifier(annotation).ok_or_else(|| {
            RuntimeError::RangeError(format!("invalid Temporal time zone: {annotation}"))
        })?;
        let calendar = match parsed.calendar.as_deref() {
            Some(calendar) => canonical_calendar_id(calendar).ok_or_else(|| {
                RuntimeError::RangeError(format!("unsupported Temporal calendar: {calendar}"))
            })?,
            None => "iso8601".to_string(),
        };
        Ok(ParsedZonedDateTimeString {
            parsed,
            zone,
            calendar,
        })
    }

    /// The other half: interpreting the string's wall-clock reading and offset
    /// against its zone under the `disambiguation`/`offset` options.
    pub(in super::super::super) fn temporal_interpret_zoned_date_time_string(
        string: ParsedZonedDateTimeString,
        disambiguation: time_zone::Disambiguation,
        offset_option: &str,
    ) -> Result<TemporalValue, RuntimeError> {
        let ParsedZonedDateTimeString {
            parsed,
            zone,
            calendar,
        } = string;
        let (year, month, day) = (parsed.year, parsed.month, parsed.day);
        let time = parsed.time.unwrap_or((0, 0, 0, 0, 0, 0));
        let epoch_nanoseconds = if parsed.time.is_none() {
            // A date-only string has no time at all, so the result is the
            // day's `GetStartOfDay` -- which is *not* local midnight resolved
            // through `disambiguation` when midnight is skipped (Toronto's
            // 1919-03-31 gap started at 00:30). A property bag with no time
            // fields, by contrast, really does mean midnight
            // (`from/dst-skipped-cross-midnight.js`).
            temporal_checked_start_of_day(&zone, (year, month, day))?
        } else {
            temporal_interpret_offset(
                &zone,
                (year, month, day),
                time,
                parsed.offset_nanoseconds,
                parsed.utc_designator,
                disambiguation,
                offset_option,
                // `MatchMinutes` unless the leading offset itself was spelled
                // with sub-minute (seconds/fraction) precision -- see
                // `temporal_interpret_offset`'s own doc comment.
                !parsed.offset_sub_minute_precision,
            )?
        };
        if !epoch::is_in_instant_range(&epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime string is outside the supported range".into(),
            ));
        }
        let mut value = TemporalValue {
            kind: TemporalKind::ZonedDateTime,
            duration: None,
            year,
            month,
            day,
            hour: time.0,
            minute: time.1,
            second: time.2,
            millisecond: time.3,
            microsecond: time.4,
            nanosecond: time.5,
            epoch_nanoseconds,
            calendar,
            time_zone: zone.identifier(),
        };
        temporal_set_local_fields(&mut value, &zone);
        Ok(value)
    }
}
