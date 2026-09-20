// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.Now` plain values and the time-zone helpers that turn a `PlainDate`
//! or `Instant` into a zoned value.

use super::super::*;

impl Vm {
    /// `SystemUTCEpochNanoseconds`, read from the one wall clock this engine
    /// already has: `Date.now()`'s own `SystemTime` call. Reusing it means
    /// `Temporal.Now.instant()` and `Date.now()` can never disagree, which is
    /// exactly what Test262's `Now/instant/return-value-value.js` checks by
    /// bracketing the call between two `Date.now()` reads.
    ///
    /// Millisecond granularity therefore, not nanosecond. The spec leaves the
    /// clock's resolution implementation-defined and explicitly permits
    /// coarsening it; real engines clamp for the same reason.
    pub(in super::super::super) fn temporal_now_epoch_nanoseconds() -> BigInt {
        BigInt::from(Self::current_time() as i64) * 1_000_000_u32
    }

    /// `ToTemporalTimeZoneIdentifier`. A `Temporal.ZonedDateTime` contributes
    /// its own zone; every other object is a `TypeError` — note that no
    /// `ToString` coercion happens at all here, so an object with a
    /// `toString` method is rejected rather than consulted.
    pub(in super::super::super) fn temporal_time_zone_identifier(
        &mut self,
        value: &Value,
    ) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(time_zone_id::SYSTEM.into());
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return Ok(temporal.time_zone);
                }
            }
        }
        let Value::String(text) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal time zone must be a string or a Temporal.ZonedDateTime".into(),
            ));
        };
        let text = text
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
        time_zone_id::resolve(&text)
            .map_err(|()| RuntimeError::RangeError(format!("invalid Temporal time zone: {text}")))
    }

    /// `SystemDateTime`: the current instant's wall-clock fields in the zone
    /// `time_zone` names.
    pub(in super::super::super) fn temporal_now_local_fields(
        &mut self,
        time_zone: &Value,
    ) -> Result<(epoch::CivilDate, epoch::CivilTime), RuntimeError> {
        let identifier = self.temporal_time_zone_identifier(time_zone)?;
        let now = Self::temporal_now_epoch_nanoseconds();
        let offset = time_zone_id::offset_seconds(&identifier, &now).ok_or_else(|| {
            RuntimeError::RangeError(format!(
                "Temporal.Now cannot resolve a UTC offset for the time zone {identifier}"
            ))
        })?;
        let local = now + BigInt::from(offset) * 1_000_000_000_u32;
        Ok(epoch::instant_fields(&local))
    }

    pub(in super::super::super) fn temporal_now_instant(&mut self) -> Result<Value, RuntimeError> {
        self.instant_from_epoch_nanoseconds(Self::temporal_now_epoch_nanoseconds())
    }

    pub(in super::super::super) fn temporal_now_time_zone_id(
        &mut self,
    ) -> Result<Value, RuntimeError> {
        Ok(Value::String(time_zone_id::SYSTEM.into()))
    }

    /// `Temporal.Now.plainDateISO`/`plainDateTimeISO`/`plainTimeISO`: the same
    /// wall clock, projected onto whichever of the three ISO-calendar plain
    /// types `kind` names. The fields each type does not carry keep the
    /// constructors' own 1970-01-01T00:00 placeholders.
    pub(in super::super::super) fn temporal_now_plain(
        &mut self,
        kind: TemporalKind,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
            self.temporal_now_local_fields(time_zone)?;
        let dated = kind != TemporalKind::PlainTime;
        let timed = kind != TemporalKind::PlainDate;
        self.alloc_temporal_value(
            TemporalValue {
                kind,
                duration: None,
                year: if dated { year } else { 1970 },
                month: if dated { month } else { 1 },
                day: if dated { day } else { 1 },
                hour: if timed { hour } else { 0 },
                minute: if timed { minute } else { 0 },
                second: if timed { second } else { 0 },
                millisecond: if timed { millisecond } else { 0 },
                microsecond: if timed { microsecond } else { 0 },
                nanosecond: if timed { nanosecond } else { 0 },
                epoch_nanoseconds: 0.into(),
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            },
            false,
        )
    }

    /// `Temporal.Now.zonedDateTimeISO`: unlike the plain variants this needs
    /// only a *valid* zone identifier, never its offset — the epoch value and
    /// the identifier are both exact, so a named IANA zone works here even
    /// while Track E's transition history is still missing. The ISO
    /// wall-clock fields stay at the same 1970-01-01 placeholder
    /// `instant_from_epoch_nanoseconds` leaves on an `Instant`; nothing
    /// observable reads them for a `ZonedDateTime` yet, and Stage 2 will
    /// derive them from the epoch and the zone rather than store them.
    pub(in super::super::super) fn temporal_now_zoned_date_time(
        &mut self,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let identifier = self.temporal_time_zone_identifier(time_zone)?;
        let epoch_nanoseconds = Self::temporal_now_epoch_nanoseconds();
        self.alloc_temporal_value(
            TemporalValue {
                kind: TemporalKind::ZonedDateTime,
                duration: None,
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
                microsecond: 0,
                nanosecond: 0,
                epoch_nanoseconds,
                calendar: "iso8601".into(),
                time_zone: identifier,
            },
            false,
        )
    }

    // ---- Stage 1 Track E: time-zone identifiers and offsets -------------

    /// `ToTemporalTimeZoneIdentifier`: a `ZonedDateTime` contributes its own
    /// stored zone; every other object — and every non-string primitive — is
    /// a `TypeError`, because Temporal deliberately does not run `ToString`
    /// on a time-zone argument. An unparseable string is a `RangeError`.
    pub(in super::super::super) fn temporal_time_zone(
        &mut self,
        value: &Value,
    ) -> Result<time_zone::TimeZone, RuntimeError> {
        let invalid = |source: &str| {
            RuntimeError::RangeError(format!("invalid Temporal time zone: {source}"))
        };
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return time_zone::parse_identifier(&temporal.time_zone)
                        .ok_or_else(|| invalid(&temporal.time_zone));
                }
            }
        }
        let Value::String(source) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal time zone must be a string or a Temporal.ZonedDateTime".into(),
            ));
        };
        let source = source
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
        time_zone::parse_identifier(&source).ok_or_else(|| invalid(&source))
    }

    /// `Temporal.PlainDate.prototype.toZonedDateTime`'s single `item`
    /// argument: either a bare time-zone identifier, or a property bag whose
    /// `timeZone` names the zone and whose optional `plainTime` supplies the
    /// time of day (absent meaning the zone's start of day).
    pub(in super::super::super) fn temporal_plain_date_zone_and_time(
        &mut self,
        item: &Value,
    ) -> Result<(time_zone::TimeZone, Option<epoch::CivilTime>), RuntimeError> {
        if item.object_id().is_none() {
            return Ok((self.temporal_time_zone(item)?, None));
        }
        let requested = self.get_property(item, &"timeZone".into())?;
        if requested == Value::Undefined {
            // No `timeZone` property: the item itself has to be the zone,
            // which only a `ZonedDateTime` can satisfy — a plain object is a
            // `TypeError`, exactly as `ToTemporalTimeZoneIdentifier` says.
            return Ok((self.temporal_time_zone(item)?, None));
        }
        let zone = self.temporal_time_zone(&requested)?;
        let plain_time = self.get_property(item, &"plainTime".into())?;
        Ok((zone, self.temporal_time_of_day(&plain_time)?))
    }

    /// A narrowed `ToTemporalTime`: `undefined` means "start of day", and an
    /// existing `Temporal.PlainTime`/`PlainDateTime` contributes its own time
    /// fields.
    ///
    /// Converting a *string* to a `Temporal.PlainTime` is deliberately not
    /// implemented here — `Temporal.PlainTime` is Phase 26 Stage 1 Track D's
    /// own scope, and this engine has no time-only string parser yet
    /// (`temporal_value_from_string` requires a date). Rather than accept a
    /// time string and silently mis-parse it, this fails closed with the
    /// `RangeError` the spec raises for an invalid one.
    /// `ToTemporalTime`, but optional: `undefined` means no `plainTime` was
    /// given at all (`toZonedDateTime`'s date-only fast path), which is
    /// distinct from a `PlainTime` whose fields happen to all be zero.
    ///
    /// This used to be its own hand-rolled subset (Temporal object/
    /// `PlainDateTime` only, a `RangeError` stub for a string or property
    /// bag) — left that way deliberately, per Phase 26's plan, until Stage 1
    /// Track D's real `Temporal.PlainTime` string/property-bag conversion
    /// landed. It has, as [`Self::temporal_to_plain_time`]; delegate to it
    /// instead of re-deriving the same conversion a second time.
    pub(in super::super::super) fn temporal_time_of_day(
        &mut self,
        value: &Value,
    ) -> Result<Option<epoch::CivilTime>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(None);
        }
        self.temporal_to_plain_time(value, &Value::Undefined)
            .map(Some)
    }

    /// `ToTemporalDisambiguation`: a `"compatible"`-defaulted string option.
    pub(in super::super::super) fn temporal_disambiguation(
        &mut self,
        options: &Value,
    ) -> Result<time_zone::Disambiguation, RuntimeError> {
        let options = self.temporal_options(options)?;
        let Some(name) = self.temporal_string_option(&options, "disambiguation", &[])? else {
            return Ok(time_zone::Disambiguation::Compatible);
        };
        time_zone::parse_disambiguation(&name)
            .ok_or_else(|| RuntimeError::RangeError("invalid disambiguation option".into()))
    }

    pub(in super::super::super) fn temporal_instant_to_zoned_date_time_iso(
        &mut self,
        receiver: &Value,
        time_zone: &Value,
    ) -> Result<Value, RuntimeError> {
        let epoch_nanoseconds = self.temporal_instant_epoch(receiver)?;
        let zone = self.temporal_time_zone(time_zone)?;
        let mut value = TemporalValue {
            kind: TemporalKind::ZonedDateTime,
            duration: None,
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            microsecond: 0,
            nanosecond: 0,
            epoch_nanoseconds,
            calendar: "iso8601".into(),
            time_zone: zone.identifier(),
        };
        temporal_set_local_fields(&mut value, &zone);
        self.alloc_temporal_value(value, false)
    }
}
