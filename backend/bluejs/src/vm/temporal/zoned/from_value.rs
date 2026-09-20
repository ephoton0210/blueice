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
                    self.temporal_overflow_option(&resolved_options)?;
                    self.temporal_disambiguation(&resolved_options)?;
                    self.temporal_offset_option(&resolved_options, "reject")?;
                    return Ok(temporal);
                }
            }
            // A property bag: `timeZone` is required, `offset` optional;
            // every calendar-date and time-of-day field is the same set
            // `temporal_plain_date_from_fields` already resolves for
            // `PlainDateTime` (a `ZonedDateTime`'s own field list per
            // `PrepareCalendarFields`/`CalendarDateFromFields` is identical
            // once `timeZone`/`offset` are set aside), so that resolution is
            // reused rather than re-derived, with the result's `kind`
            // overridden afterward.
            let resolved_options = self.temporal_options(options)?;
            let reject = self.temporal_overflow_option(&resolved_options)?;
            let disambiguation = self.temporal_disambiguation(&resolved_options)?;
            let offset_option = self.temporal_offset_option(&resolved_options, "reject")?;
            // `PrepareCalendarFields` reads and validates `calendar` before
            // any other field -- an invalid `calendar` is a `RangeError`
            // even when `timeZone` is missing entirely
            // (`argument-propertybag-calendar-invalid-iso-string.js`,
            // `argument-propertybag-calendar-year-zero.js`). The result is
            // discarded here (`temporal_plain_date_from_fields` below
            // re-resolves it) -- this call exists purely to get the ordering
            // of *when* a bad calendar throws right; a second, harmless
            // re-read of the same property is an already-documented,
            // separate gap shared with every other field-ordering fixture
            // this file doesn't yet pass (`order-of-operations.js`).
            let calendar_value = self.get_property(value, &"calendar".into())?;
            self.temporal_calendar_identifier(&calendar_value)?;
            let time_zone_value = self.get_property(value, &"timeZone".into())?;
            if time_zone_value == Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Temporal.ZonedDateTime property bag requires timeZone".into(),
                ));
            }
            let zone = self.temporal_time_zone(&time_zone_value)?;
            // `offset`'s own *syntax* is read and validated here, ahead of
            // `year`/`month`/`day`/etc. below -- `offset-string-invalid.js`
            // pins this exact ordering both ways: a syntactically invalid
            // offset (`"--00:00"`) is a `RangeError` even when `year` is a
            // `Symbol` that would otherwise throw `TypeError` first, but a
            // syntactically *valid* offset that merely doesn't match the
            // zone (`"+04:30"` against `"UTC"`) only surfaces *after* `year`
            // has already thrown -- because that later *semantic* mismatch
            // check only runs once every field (including `year`) below has
            // been fully resolved.
            let offset_value = self.get_property(value, &"offset".into())?;
            // A property bag's `offset` field goes through `ToPrimitive`
            // with a string hint (never a blanket `ToString`) and then must
            // *already be* a String -- an object's own `toString`/`valueOf`
            // is genuinely called (`order-of-operations.js`'s "get
            // other.offset.toString" / "call other.offset.toString"), but a
            // non-object, non-string primitive (`Number`/`null`/`Boolean`/
            // `BigInt`) is a `TypeError` without ever being stringified,
            // since `ToPrimitive` on an already-primitive value is the
            // identity (`relativeto-propertybag-invalid-offset-string.js`,
            // reached via `Temporal.Duration`'s own `relativeTo` reuse of
            // this function, still rejects a plain `1000`/`null`/`true`/
            // `1000n`). Matches `temporal_to_instant_epoch`'s own
            // `coerce_primitive`-then-check-`String` pattern.
            let offset_primitive = (!matches!(offset_value, Value::Undefined))
                .then(|| self.coerce_primitive(&offset_value, "string"))
                .transpose()?;
            if let Some(primitive) = &offset_primitive {
                if !matches!(primitive, Value::String(_)) {
                    return Err(RuntimeError::TypeError(
                        "Temporal.ZonedDateTime offset must be a string".into(),
                    ));
                }
            }
            let offset_string = offset_primitive
                .map(|primitive| self.coerce_string(&primitive))
                .transpose()?
                .map(|text| {
                    text.to_utf8()
                        .map_err(|_| RuntimeError::RangeError("invalid Temporal offset".into()))
                })
                .transpose()?;
            let offset_nanoseconds = match offset_string.as_deref() {
                None => None,
                Some(text) => {
                    Some(iso::parse_offset_string_nanoseconds(text).ok_or_else(|| {
                        RuntimeError::RangeError("invalid Temporal offset".into())
                    })?)
                }
            };
            // Now resolve the rest of the calendar-date/time-of-day fields
            // (`year`/`month`/`monthCode`/`day`/`era`/`eraYear`/`hour`../
            // `nanosecond`) -- `year`'s own `TypeError` for a non-convertible
            // value (e.g. a `Symbol`) has to come *after* `offset`'s syntax
            // check above, per this function's own doc comment.
            let mut fields = self.temporal_plain_date_from_fields(
                TemporalKind::PlainDateTime,
                value,
                OverflowInput::Resolved(reject),
            )?;
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
                offset_nanoseconds,
                false,
                disambiguation,
                &offset_option,
                false, // a property-bag `offset` field is always `MatchExactly`.
            )?;
            if !epoch::is_in_instant_range(&epoch_nanoseconds) {
                return Err(RuntimeError::RangeError(
                    "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range"
                        .into(),
                ));
            }
            fields.kind = TemporalKind::ZonedDateTime;
            fields.epoch_nanoseconds = epoch_nanoseconds;
            fields.time_zone = zone.identifier();
            temporal_set_local_fields(&mut fields, &zone);
            return Ok(fields);
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
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        let disambiguation = self.temporal_disambiguation(&resolved_options)?;
        let offset_option = self.temporal_offset_option(&resolved_options, "reject")?;
        Self::temporal_value_from_zoned_date_time_string(&source, disambiguation, &offset_option)
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
