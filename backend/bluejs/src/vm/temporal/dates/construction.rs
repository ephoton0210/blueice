// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Building, receiving and coercing `PlainDate`/`PlainDateTime` values: the value
//! constructors, the receiver check, `ToTemporalDate`/`ToTemporalDateTime`.

use super::super::*;

impl Vm {
    // ---- Stage 2: Temporal.PlainDate / Temporal.PlainDateTime -----------

    pub(in super::super::super) fn temporal_date_value(
        kind: TemporalKind,
        calendar: String,
        date: epoch::CivilDate,
    ) -> TemporalValue {
        TemporalValue {
            kind,
            duration: None,
            year: date.0,
            month: date.1,
            day: date.2,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            microsecond: 0,
            nanosecond: 0,
            epoch_nanoseconds: 0.into(),
            calendar,
            time_zone: "UTC".into(),
        }
    }

    pub(in super::super::super) fn temporal_date_time_value(
        kind: TemporalKind,
        calendar: String,
        date: epoch::CivilDate,
        time: epoch::CivilTime,
    ) -> TemporalValue {
        TemporalValue {
            kind,
            duration: None,
            year: date.0,
            month: date.1,
            day: date.2,
            hour: time.0,
            minute: time.1,
            second: time.2,
            millisecond: time.3,
            microsecond: time.4,
            nanosecond: time.5,
            epoch_nanoseconds: 0.into(),
            calendar,
            time_zone: "UTC".into(),
        }
    }

    /// Brand check shared by every `Temporal.PlainDate`/`PlainDateTime`
    /// prototype method (both kinds share one adapter layer, dispatched at
    /// runtime on the receiver's own `TemporalKind`, the same pattern
    /// `temporal_with_calendar`/`temporal_plain_to_zoned_date_time` already
    /// use).
    pub(in super::super::super) fn temporal_date_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainDate/PlainDateTime method requires a matching receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainDate/PlainDateTime method requires a matching receiver".into(),
            )
        })?;
        if !matches!(
            value.kind,
            TemporalKind::PlainDate | TemporalKind::PlainDateTime
        ) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDate/PlainDateTime method requires a matching receiver".into(),
            ));
        }
        Ok(value)
    }

    pub(in super::super::super) fn temporal_unit_to_date_unit(
        unit: rounding::TemporalUnit,
    ) -> plain_date::DateUnit {
        match unit {
            rounding::TemporalUnit::Year => plain_date::DateUnit::Year,
            rounding::TemporalUnit::Month => plain_date::DateUnit::Month,
            rounding::TemporalUnit::Week => plain_date::DateUnit::Week,
            _ => plain_date::DateUnit::Day,
        }
    }

    /// `ToTemporalDate`.
    pub(in super::super::super) fn temporal_to_plain_date(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let resolved = match temporal.kind {
                    TemporalKind::PlainDate | TemporalKind::PlainDateTime => Some((
                        (temporal.year, temporal.month, temporal.day),
                        temporal.calendar.clone(),
                    )),
                    TemporalKind::ZonedDateTime => {
                        let offset = if temporal.time_zone == "UTC" {
                            0
                        } else {
                            iso::parse_offset_identifier_nanoseconds(&temporal.time_zone)
                                .ok_or_else(|| {
                                    RuntimeError::RangeError(
                                        "Temporal.PlainDate conversion supports UTC and fixed \
                                         offsets"
                                            .into(),
                                    )
                                })?
                        };
                        let local = &temporal.epoch_nanoseconds + BigInt::from(offset);
                        Some((epoch::instant_fields(&local).0, temporal.calendar.clone()))
                    }
                    _ => None,
                };
                if let Some((date, calendar)) = resolved {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(Self::temporal_date_value(
                        TemporalKind::PlainDate,
                        calendar,
                        date,
                    ));
                }
            }
            // `options` is passed through unread here -- see
            // `temporal_plain_date_from_fields`'s own doc comment.
            return self.temporal_plain_date_from_fields(
                TemporalKind::PlainDate,
                value,
                OverflowInput::Options(options),
            );
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDate-like value must be an object or a string".into(),
            ));
        }
        let source = self
            .coerce_string(value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal.PlainDate string".into()))?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        self.temporal_value_from_string(TemporalKind::PlainDate, &source)
    }

    /// `ToTemporalDateTime`.
    pub(in super::super::super) fn temporal_to_plain_date_time(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let resolved = match temporal.kind {
                    TemporalKind::PlainDateTime => Some((
                        (temporal.year, temporal.month, temporal.day),
                        (
                            temporal.hour,
                            temporal.minute,
                            temporal.second,
                            temporal.millisecond,
                            temporal.microsecond,
                            temporal.nanosecond,
                        ),
                        temporal.calendar.clone(),
                    )),
                    TemporalKind::PlainDate => Some((
                        (temporal.year, temporal.month, temporal.day),
                        (0, 0, 0, 0, 0, 0),
                        temporal.calendar.clone(),
                    )),
                    TemporalKind::ZonedDateTime => {
                        let offset = if temporal.time_zone == "UTC" {
                            0
                        } else {
                            iso::parse_offset_identifier_nanoseconds(&temporal.time_zone)
                                .ok_or_else(|| {
                                    RuntimeError::RangeError(
                                        "Temporal.PlainDateTime conversion supports UTC and \
                                         fixed offsets"
                                            .into(),
                                    )
                                })?
                        };
                        let local = &temporal.epoch_nanoseconds + BigInt::from(offset);
                        let (date, time) = epoch::instant_fields(&local);
                        Some((date, time, temporal.calendar.clone()))
                    }
                    _ => None,
                };
                if let Some((date, time, calendar)) = resolved {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(Self::temporal_date_time_value(
                        TemporalKind::PlainDateTime,
                        calendar,
                        date,
                        time,
                    ));
                }
            }
            // `options` is passed through unread here -- see
            // `temporal_plain_date_from_fields`'s own doc comment.
            return self.temporal_plain_date_from_fields(
                TemporalKind::PlainDateTime,
                value,
                OverflowInput::Options(options),
            );
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainDateTime-like value must be an object or a string".into(),
            ));
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.PlainDateTime string".into())
        })?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        self.temporal_value_from_string(TemporalKind::PlainDateTime, &source)
    }

    pub(in super::super::super) fn temporal_to_matching(
        &mut self,
        value: &Value,
        kind: TemporalKind,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if kind == TemporalKind::PlainDate {
            self.temporal_to_plain_date(value, options)
        } else {
            self.temporal_to_plain_date_time(value, options)
        }
    }
}
