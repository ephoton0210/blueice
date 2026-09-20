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

    /// The representable-range check `CreateTemporalDate`,
    /// `CreateTemporalDateTime` and `CreateTemporalYearMonth` all make before
    /// building an object, on the value's stored ISO fields: a date's noon must
    /// be representable (`ISODateWithinLimits`), a date-time itself
    /// (`ISODateTimeWithinLimits`, one day narrower at the low end), a
    /// year-month its own (`ISOYearMonthWithinLimits`). Every creation path --
    /// the numeric constructors, `from`, `with`, `withPlainTime`,
    /// `toPlainDateTime`, `toPlainDate`, `round`, arithmetic -- ends in
    /// [`Vm::alloc_temporal_value`], so making the check there covers them all
    /// instead of each caller remembering (the constructors and several
    /// conversions did not).
    ///
    /// Only these three kinds are judged here: an `Instant`/`ZonedDateTime` is
    /// bounded by its epoch nanoseconds, a `PlainTime` has no range, and a
    /// `PlainMonthDay`'s reference year is chosen by its calendar.
    pub(in super::super::super) fn temporal_check_creation_limits(
        value: &TemporalValue,
    ) -> Result<(), RuntimeError> {
        let date = (value.year, value.month, value.day);
        let within_limits = match value.kind {
            TemporalKind::PlainDate => epoch::is_date_within_limits(date),
            TemporalKind::PlainDateTime => epoch::is_date_time_within_limits(
                date,
                (
                    value.hour,
                    value.minute,
                    value.second,
                    value.millisecond,
                    value.microsecond,
                    value.nanosecond,
                ),
            ),
            TemporalKind::PlainYearMonth => {
                iso::is_year_month_within_limits(value.year, value.month)
            }
            _ => true,
        };
        if within_limits {
            Ok(())
        } else {
            Err(RuntimeError::RangeError(format!(
                "Temporal.{} is outside the supported range",
                value.kind.name()
            )))
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
                    // A `ZonedDateTime`'s stored ISO fields are already its local
                    // wall-clock ones (in any zone, named or fixed-offset), and
                    // `ToTemporalDate` reads exactly those slots.
                    TemporalKind::ZonedDateTime => Some((
                        (temporal.year, temporal.month, temporal.day),
                        temporal.calendar.clone(),
                    )),
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
                    // As for `PlainDate` above: the local wall-clock slots.
                    TemporalKind::ZonedDateTime => Some((
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
