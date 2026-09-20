// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Producing Temporal values: the constructors, `from` and the string/property-bag
//! conversions behind it, and allocation of the heap object.

use super::super::*;

impl Vm {
    pub(in super::super::super) fn temporal_value_from_args(
        &mut self,
        kind: TemporalKind,
        args: &[Value],
    ) -> Result<TemporalValue, RuntimeError> {
        let number = |vm: &mut Self, index, minimum, maximum, name| {
            vm.temporal_integer(native::argument(args, index), minimum, maximum, name)
        };
        let mut value = TemporalValue {
            kind,
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
            epoch_nanoseconds: 0.into(),
            calendar: "iso8601".into(),
            time_zone: "UTC".into(),
        };
        match kind {
            TemporalKind::Duration => {
                let values = [
                    self.temporal_duration_integer(native::argument(args, 0), "years")?,
                    self.temporal_duration_integer(native::argument(args, 1), "months")?,
                    self.temporal_duration_integer(native::argument(args, 2), "weeks")?,
                    self.temporal_duration_integer(native::argument(args, 3), "days")?,
                    self.temporal_duration_integer(native::argument(args, 4), "hours")?,
                    self.temporal_duration_integer(native::argument(args, 5), "minutes")?,
                    self.temporal_duration_integer(native::argument(args, 6), "seconds")?,
                    self.temporal_duration_integer(native::argument(args, 7), "milliseconds")?,
                    self.temporal_duration_integer(native::argument(args, 8), "microseconds")?,
                    self.temporal_duration_integer(native::argument(args, 9), "nanoseconds")?,
                ];
                value.duration = Some(Box::new(
                    blueice_ecma402::DurationRecord::try_new(
                        values[0], values[1], values[2], values[3], values[4], values[5],
                        values[6], values[7], values[8], values[9],
                    )
                    .map_err(|error| RuntimeError::RangeError(error.to_string()))?,
                ));
            }
            TemporalKind::Instant => {
                value.epoch_nanoseconds = self.temporal_to_big_int(native::argument(args, 0))?;
                if !epoch::is_in_instant_range(&value.epoch_nanoseconds) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.Instant epoch nanoseconds are outside the supported range".into(),
                    ));
                }
            }
            TemporalKind::ZonedDateTime => {
                value.epoch_nanoseconds = match native::argument(args, 0) {
                    Value::BigInt(value) => value.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "Temporal.ZonedDateTime requires epoch nanoseconds as a BigInt".into(),
                        ));
                    }
                };
                if !epoch::is_in_instant_range(&value.epoch_nanoseconds) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range"
                            .into(),
                    ));
                }
                let zone = self.temporal_time_zone(native::argument(args, 1))?;
                // The constructor's own third positional argument is a bare
                // calendar ID (like `PlainDate`'s), not
                // `ToTemporalCalendarIdentifier`'s wider string grammar --
                // this was previously never read at all, silently ignoring
                // `new Temporal.ZonedDateTime(ns, tz, "gregory")`'s own
                // calendar.
                value.calendar = self.temporal_calendar(native::argument(args, 2))?;
                value.time_zone = zone.identifier();
                // The stored ISO fields are always the *local* wall-clock
                // ones a `ZonedDateTime` presents (`temporal_set_local_fields`'s
                // own doc comment) -- this was previously never called here,
                // silently leaving every numeric-constructor `ZonedDateTime`
                // at its `1970-01-01T00:00:00` field defaults regardless of
                // its real epoch/zone, which broke every getter
                // (`.year`/`.hour`/etc) on a directly-constructed value.
                temporal_set_local_fields(&mut value, &zone);
            }
            TemporalKind::PlainDate | TemporalKind::PlainDateTime => {
                // `temporal_integer` itself is `ToIntegerWithTruncation`, so
                // a fractional year/month/day truncates toward zero rather
                // than being rejected (Test262's `argument-convert.js`).
                value.year = number(self, 0, -271_821, 275_760, "year")?;
                value.month = number(self, 1, 1, 12, "month")? as u8;
                value.day = number(self, 2, 1, 31, "day")? as u8;
                if iso::days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
                    return Err(RuntimeError::RangeError("invalid Temporal day".into()));
                }
                if kind == TemporalKind::PlainDateTime {
                    value.hour = self.temporal_optional_integer(
                        native::argument(args, 3),
                        0,
                        0,
                        23,
                        "hour",
                    )? as u8;
                    value.minute = self.temporal_optional_integer(
                        native::argument(args, 4),
                        0,
                        0,
                        59,
                        "minute",
                    )? as u8;
                    value.second = self.temporal_optional_integer(
                        native::argument(args, 5),
                        0,
                        0,
                        59,
                        "second",
                    )? as u8;
                    value.millisecond = self.temporal_optional_integer(
                        native::argument(args, 6),
                        0,
                        0,
                        999,
                        "millisecond",
                    )? as u16;
                    value.microsecond = self.temporal_optional_integer(
                        native::argument(args, 7),
                        0,
                        0,
                        999,
                        "microsecond",
                    )? as u16;
                    value.nanosecond = self.temporal_optional_integer(
                        native::argument(args, 8),
                        0,
                        0,
                        999,
                        "nanosecond",
                    )? as u16;
                    value.calendar = self.temporal_calendar(native::argument(args, 9))?;
                } else {
                    value.calendar = self.temporal_calendar(native::argument(args, 3))?;
                }
            }
            TemporalKind::PlainMonthDay => {
                value.month = number(self, 0, 1, 12, "month")? as u8;
                value.day = number(self, 1, 1, 31, "day")? as u8;
                value.calendar = self.temporal_calendar(native::argument(args, 2))?;
                value.year = self.temporal_optional_integer(
                    native::argument(args, 3),
                    1972,
                    -271_821,
                    275_760,
                    "reference year",
                )?;
                if iso::days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
                    return Err(RuntimeError::RangeError("invalid Temporal day".into()));
                }
                // The per-field `-271_821..=275_760` bound above is coarser
                // than the exact representable-range boundary (a
                // day-and-nanosecond boundary, not a year one --
                // `+275760-09-13` is the true maximum, so `+275760-09-14`
                // must still throw even though every individual field is
                // itself in range). Pinned by
                // `PlainMonthDay/refisoyear-out-of-range.js`.
                if !epoch::is_date_within_limits((value.year, value.month, value.day)) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay reference year is outside the supported range"
                            .into(),
                    ));
                }
            }
            TemporalKind::PlainTime => {
                // `ToIntegerWithTruncation` then `RejectTime`: a fractional
                // argument truncates rather than throwing (`new
                // Temporal.PlainTime(11.9)` is hour 11), but an out-of-range
                // whole value is still a RangeError.
                let fields = [
                    self.temporal_optional_truncated_integer(native::argument(args, 0), "hour")?,
                    self.temporal_optional_truncated_integer(native::argument(args, 1), "minute")?,
                    self.temporal_optional_truncated_integer(native::argument(args, 2), "second")?,
                    self.temporal_optional_truncated_integer(
                        native::argument(args, 3),
                        "millisecond",
                    )?,
                    self.temporal_optional_truncated_integer(
                        native::argument(args, 4),
                        "microsecond",
                    )?,
                    self.temporal_optional_truncated_integer(
                        native::argument(args, 5),
                        "nanosecond",
                    )?,
                ];
                let fields = Self::temporal_regulate_time(fields, true)?;
                value.hour = fields.0;
                value.minute = fields.1;
                value.second = fields.2;
                value.millisecond = fields.3;
                value.microsecond = fields.4;
                value.nanosecond = fields.5;
            }
            TemporalKind::PlainYearMonth => {
                value.year = number(self, 0, -271_821, 275_760, "year")?;
                value.month = number(self, 1, 1, 12, "month")? as u8;
                value.calendar = self.temporal_calendar(native::argument(args, 2))?;
                value.day = self.temporal_optional_integer(
                    native::argument(args, 3),
                    1,
                    1,
                    31,
                    "reference day",
                )? as u8;
                if iso::days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
                    return Err(RuntimeError::RangeError(
                        "invalid Temporal reference day".into(),
                    ));
                }
                // As with `PlainMonthDay` above: the per-field year bound is
                // coarser than the true representable-range boundary, which
                // is a *month* boundary within the min/max year, not a
                // whole-year one (`-271821-04` is valid, `-271821-03` is
                // not; `+275760-09` is valid, `+275760-10` is not) --
                // `referenceISODay` never affects this, only `year`/`month`
                // do. Pinned by `PlainYearMonth/limits.js`.
                if !iso::is_year_month_within_limits(value.year, value.month) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainYearMonth is outside the supported range".into(),
                    ));
                }
            }
        }
        Ok(value)
    }

    pub(in super::super::super) fn temporal_value_from_string(
        &mut self,
        kind: TemporalKind,
        source: &str,
    ) -> Result<TemporalValue, RuntimeError> {
        if kind == TemporalKind::Duration {
            let duration = iso::parse_duration_record(source).ok_or_else(|| {
                RuntimeError::RangeError("invalid Temporal.Duration string".into())
            })?;
            return Ok(TemporalValue {
                kind,
                duration: Some(Box::new(duration)),
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
                microsecond: 0,
                nanosecond: 0,
                epoch_nanoseconds: 0.into(),
                calendar: "iso8601".into(),
                time_zone: "UTC".into(),
            });
        }
        // Each Temporal type reads a different production of the same
        // grammar: a year-month string may omit the day, a month-day string
        // the year, and a time string the date entirely.
        let invalid =
            || RuntimeError::RangeError(format!("invalid Temporal.{} string", kind.name()));
        let parsed = match kind {
            TemporalKind::PlainYearMonth => iso::parse_year_month(source),
            TemporalKind::PlainMonthDay => iso::parse_month_day(source),
            TemporalKind::PlainTime => iso::parse_time(source),
            _ => iso::parse_date_time(source),
        }
        .ok_or_else(invalid)?;
        // A type with no calendar slot ignores the annotation outright — even
        // an unrecognized or critical one
        // (`Instant/from/argument-string-calendar-annotation.js`,
        // `PlainTime/from/argument-string-calendar-annotation.js`).
        let calendar_slot = !matches!(kind, TemporalKind::Instant | TemporalKind::PlainTime);
        let calendar = match parsed.calendar.as_deref().filter(|_| calendar_slot) {
            Some(calendar) => canonical_calendar_id(calendar).ok_or_else(|| {
                RuntimeError::RangeError(format!("unsupported Temporal calendar: {calendar}"))
            })?,
            None => "iso8601".to_string(),
        };
        // The UTC designator asserts an exact instant, which a wall-clock
        // type has no way to represent, so it is a syntax error there rather
        // than something to ignore.
        if parsed.utc_designator
            && !matches!(kind, TemporalKind::Instant | TemporalKind::ZonedDateTime)
        {
            return Err(invalid());
        }
        let (year, month, day) = (parsed.year, parsed.month, parsed.day);
        let time = parsed.time.unwrap_or((0, 0, 0, 0, 0, 0));
        if kind == TemporalKind::PlainTime {
            return Ok(Self::plain_time_value(time));
        }
        let (hour, minute, second, millisecond, microsecond, nanosecond) = time;
        // A year-month or month-day string that never spelled the missing
        // half cannot be resolved in a calendar whose months do not line up
        // with ISO's, so those combinations are out of range rather than
        // silently reinterpreted.
        let non_iso = calendar != "iso8601";
        match kind {
            TemporalKind::PlainYearMonth => {
                if non_iso && !parsed.day_present {
                    return Err(RuntimeError::RangeError(
                        "a Temporal.PlainYearMonth string without a day requires the ISO calendar"
                            .into(),
                    ));
                }
                if !iso::is_year_month_within_limits(year, month) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainYearMonth string is outside the supported range".into(),
                    ));
                }
            }
            TemporalKind::PlainMonthDay => {
                if non_iso && !parsed.year_present {
                    return Err(RuntimeError::RangeError(
                        "a Temporal.PlainMonthDay string without a year requires the ISO calendar"
                            .into(),
                    ));
                }
                // The reference year carries no range of its own, but a
                // non-ISO calendar still has to convert the spelled date.
                if non_iso && !epoch::is_date_within_limits((year, month, day)) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay string is outside the supported range".into(),
                    ));
                }
            }
            // A `PlainDate`'s range is judged at noon, so it reaches one day
            // further at each end than a `PlainDateTime`'s at midnight.
            TemporalKind::PlainDate if !epoch::is_date_within_limits((year, month, day)) => {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDate string is outside the supported range".into(),
                ));
            }
            TemporalKind::PlainDateTime
                if !epoch::is_date_time_within_limits((year, month, day), time) =>
            {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDateTime string is outside the supported range".into(),
                ));
            }
            _ => {}
        }
        let epoch_nanoseconds = if kind == TemporalKind::Instant {
            // An instant string must pin its offset: a wall-clock reading
            // alone does not identify one.
            if parsed.time.is_none()
                || (!parsed.utc_designator && parsed.offset_nanoseconds.is_none())
            {
                return Err(invalid());
            }
            let epoch_nanoseconds = epoch::nanoseconds_since_epoch((year, month, day), time, 0)
                - parsed.offset_nanoseconds.unwrap_or(0);
            if !epoch::is_in_instant_range(&epoch_nanoseconds) {
                return Err(RuntimeError::RangeError(
                    "Temporal.Instant string is outside the supported range".into(),
                ));
            }
            epoch_nanoseconds
        } else {
            0.into()
        };
        Ok(TemporalValue {
            kind,
            duration: None,
            // The `1972`/`1` reference-field hardcodes below are only
            // correct for the `iso8601` calendar (`ToTemporalMonthDay`'s own
            // literal `referenceISOYear = 1972`, mirrored here for
            // `PlainYearMonth`'s reference day). A non-`iso8601` calendar
            // string keeps its own parsed year/day instead, which
            // `Vm::temporal_to_plain_year_month`/`temporal_to_plain_month_day`
            // then re-resolves through `CalendarYearMonthFromFields`/
            // `CalendarMonthDayFromFields` -- discarding it unconditionally
            // here (as this function previously did) silently dropped a
            // non-ISO calendar string's own explicit year/day.
            year: if kind == TemporalKind::PlainMonthDay && calendar == "iso8601" {
                1972
            } else {
                year
            },
            month,
            day: if kind == TemporalKind::PlainYearMonth && calendar == "iso8601" {
                1
            } else {
                day
            },
            hour,
            minute,
            second,
            millisecond,
            microsecond,
            nanosecond,
            epoch_nanoseconds,
            calendar,
            time_zone: parsed.time_zone.unwrap_or_else(|| "UTC".into()),
        })
    }

    pub(in super::super::super) fn alloc_temporal_value(
        &mut self,
        value: TemporalValue,
        use_new_target: bool,
    ) -> Result<Value, RuntimeError> {
        self.temporal_global()?;
        let constructor = self.globals[&format!("%Temporal.{}%", value.kind.name())];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Temporal constructor prototype is an object");
        let prototype = if use_new_target {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.with_roots(|heap| heap.alloc_temporal(value, Some(prototype)))
            .map(Value::Object)
    }

    pub(in super::super::super) fn temporal_constructor(
        &mut self,
        kind: TemporalKind,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(format!(
                "Temporal.{} must be called with new",
                kind.name()
            )));
        }
        let base = self.stack.len();
        self.stack.extend(args.iter().cloned());
        let result = self
            .temporal_value_from_args(kind, args)
            .and_then(|value| self.alloc_temporal_value(value, true));
        self.stack.truncate(base);
        result
    }

    pub(in super::super::super) fn temporal_from(
        &mut self,
        kind: TemporalKind,
        value: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        if kind == TemporalKind::PlainTime {
            // `ToTemporalTime` covers every accepted argument shape at once
            // (PlainTime/PlainDateTime/ZonedDateTime, property bag, string)
            // and is the only `from` that reads the `overflow` option.
            let base = self.stack.len();
            let result = self
                .temporal_to_plain_time(value, options)
                .and_then(|fields| {
                    self.alloc_temporal_value(Self::plain_time_value(fields), false)
                });
            self.stack.truncate(base);
            return result;
        }
        if kind == TemporalKind::Duration {
            // `Temporal.Duration.from` is exactly `ToTemporalDuration`, which
            // already accepts a Duration, an ISO string and a property bag.
            let record = self.temporal_duration_from_value(value)?;
            return self.alloc_temporal_value(Self::temporal_duration_value(record), false);
        }
        if kind == TemporalKind::Instant {
            // `Temporal.Instant.from` *is* `ToTemporalInstant`, including its
            // ZonedDateTime fast path and its TypeError for non-strings.
            let epoch_nanoseconds = self.temporal_to_instant_epoch(value)?;
            return self.instant_from_epoch_nanoseconds(epoch_nanoseconds);
        }
        if kind == TemporalKind::PlainYearMonth {
            // `Temporal.PlainYearMonth.from` *is* `ToTemporalYearMonth`,
            // which (unlike `PlainDate`/`PlainDateTime`) is handled entirely
            // by one function rather than split between this generic
            // dispatcher's object/string branches below -- both a
            // property-bag object and a calendar-annotated string need the
            // same `CalendarYearMonthFromFields` re-resolution.
            let resolved = self.temporal_to_plain_year_month(value, options)?;
            return self.alloc_temporal_value(resolved, false);
        }
        if kind == TemporalKind::PlainMonthDay {
            // Same rationale as `PlainYearMonth` above, for
            // `ToTemporalMonthDay`.
            let resolved = self.temporal_to_plain_month_day(value, options)?;
            return self.alloc_temporal_value(resolved, false);
        }
        if kind == TemporalKind::ZonedDateTime {
            // `Temporal.ZonedDateTime.from` *is* `ToTemporalZonedDateTime`,
            // which (like `PlainYearMonth`/`PlainMonthDay` above) needs its
            // own dedicated conversion rather than the generic object/string
            // dispatcher below: a property bag needs a `timeZone` (and
            // optional `offset`) read alongside the calendar-date fields,
            // and a string needs a *mandatory* time-zone annotation resolved
            // through real zone/disambiguation logic -- neither of which the
            // generic dispatcher (built for the calendar-only plain types)
            // has any notion of.
            let resolved = self.temporal_to_zoned_date_time(value, options)?;
            return self.alloc_temporal_value(resolved, false);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == kind {
                    // Options are still read (for validation/ordering
                    // parity) even though a same-kind argument is used
                    // as-is -- every other `ToTemporal*` conversion's own
                    // identical fast path already does this
                    // (`temporal_to_plain_date`/`temporal_to_plain_date_time`/
                    // `temporal_to_plain_year_month`/
                    // `temporal_to_plain_month_day`/
                    // `temporal_to_zoned_date_time`); this generic
                    // dispatcher's own fast path had simply never had it
                    // added (`order-of-operations.js`'s "order of
                    // operations when cloning a PlainDate instance" case).
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return self.alloc_temporal_value(temporal, false);
                }
            }
            if matches!(kind, TemporalKind::PlainDate | TemporalKind::PlainDateTime) {
                // `options` is passed through unread here -- `ToTemporalDate`/
                // `ToTemporalDateTime`'s real algorithm reads `fields` before
                // `resolvedOptions`, which `temporal_plain_date_from_fields`
                // itself now does internally (see its own doc comment).
                return self
                    .temporal_plain_date_from_fields(kind, value, OverflowInput::Options(options))
                    .and_then(|temporal| self.alloc_temporal_value(temporal, false));
            }
        }
        let source = self.coerce_string(value)?;
        let source = source
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal string".into()))?;
        // The string is parsed (and, on failure, throws `RangeError`)
        // strictly *before* `options`/`overflow` are ever read --
        // `observable-get-overflow-argument-string-invalid.js` pins this
        // exact ordering: an ISO-invalid string must throw without
        // `options.overflow` ever being read. `options`/`overflow` is
        // still read (for validation/ordering parity) once parsing
        // succeeds, even though a fully-specified ISO string never
        // actually needs to regulate anything -- every other
        // `ToTemporal*` conversion's own string branch already does this
        // (e.g. `temporal_to_plain_date`'s), and `order-of-operations.js`'s
        // "order of operations when parsing a string" case expects it here
        // too.
        let temporal = self.temporal_value_from_string(kind, &source)?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        self.alloc_temporal_value(temporal, false)
    }
}
