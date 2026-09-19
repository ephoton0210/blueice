// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    // ---- Stage 2: Temporal.PlainDate / Temporal.PlainDateTime -----------

    pub(in super::super) fn temporal_date_value(
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

    pub(in super::super) fn temporal_date_time_value(
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
    pub(in super::super) fn temporal_date_receiver(
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

    pub(in super::super) fn temporal_unit_to_date_unit(
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
    pub(in super::super) fn temporal_to_plain_date(
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
    pub(in super::super) fn temporal_to_plain_date_time(
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

    pub(in super::super) fn temporal_to_matching(
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

    /// `Temporal.PlainDate.prototype.with`/`Temporal.PlainDateTime.prototype.with`.
    /// A property bag only: a `calendar`/`timeZone` property, or a
    /// Temporal-like object, is a `TypeError`; at least one recognized
    /// calendar/time field must be present.
    pub(in super::super) fn temporal_date_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
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
        let is_date_time = existing.kind == TemporalKind::PlainDateTime;

        // `PrepareCalendarFields`/`PreparePartialCalendarFields` read and
        // immediately coerce every recognized property in strict
        // alphabetical order -- `day`, `era`, `eraYear`, `hour`,
        // `microsecond`, `millisecond`, `minute`, `month`, `monthCode`,
        // `nanosecond`, `second`, `year` -- interleaved with each field's
        // own immediate conversion, never batched into a "read everything,
        // then convert everything" pass; see `temporal_read_optional_integer`'s
        // own doc comment. Verified directly against `order-of-operations.js`
        // (`PlainDate`/`PlainDateTime`), which instruments every property
        // with a getter that records this exact interleave.
        let requested_day = self.temporal_read_optional_integer(like, "day", 1, i32::MAX)?;
        // `iso8601` has no era concept at all -- its own field-name list
        // never includes `era`/`eraYear`, so neither property is even read
        // (confirmed directly by `order-of-operations.js`'s own expected
        // sequence, which has no `era`/`eraYear` entries for an `iso8601`
        // receiver). Every other calendar's field list includes both
        // regardless of whether it individually supports eras --
        // `chinese`/`dangi` still need to *see* a supplied `era`/`eraYear`
        // in order to reject it (`mutually-exclusive-fields-{chinese,
        // dangi}.js`).
        let read_era_fields = existing.calendar != "iso8601";
        let (era_s, era_year_num) = if read_era_fields {
            (
                self.temporal_read_optional_string(like, "era", "invalid Temporal era")?,
                self.temporal_read_optional_integer(like, "eraYear", i32::MIN, i32::MAX)?,
            )
        } else {
            (None, None)
        };
        let (requested_hour, requested_microsecond, requested_millisecond, requested_minute) =
            if is_date_time {
                (
                    self.temporal_read_optional_integer(like, "hour", 0, 23)?,
                    self.temporal_read_optional_integer(like, "microsecond", 0, 999)?,
                    self.temporal_read_optional_integer(like, "millisecond", 0, 999)?,
                    self.temporal_read_optional_integer(like, "minute", 0, 59)?,
                )
            } else {
                (None, None, None, None)
            };
        let requested_month = self.temporal_read_optional_integer(like, "month", 1, 99)?;
        let month_code_s =
            self.temporal_read_optional_string(like, "monthCode", "invalid Temporal month code")?;
        let (requested_nanosecond, requested_second) = if is_date_time {
            (
                self.temporal_read_optional_integer(like, "nanosecond", 0, 999)?,
                self.temporal_read_optional_integer(like, "second", 0, 59)?,
            )
        } else {
            (None, None)
        };
        let requested_year = self.temporal_read_optional_integer(like, "year", -9_999, 9_999)?;

        let any_present = requested_day.is_some()
            || era_s.is_some()
            || era_year_num.is_some()
            || requested_hour.is_some()
            || requested_microsecond.is_some()
            || requested_millisecond.is_some()
            || requested_minute.is_some()
            || requested_month.is_some()
            || month_code_s.is_some()
            || requested_nanosecond.is_some()
            || requested_second.is_some()
            || requested_year.is_some();
        if !any_present {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let mut fields = DateFields::default();
        // `CalendarFields.cpp`'s `NonISOResolveFields`/`NonISOFieldKeysToIgnore`
        // (ported here the same way `temporal_year_month_with` already does
        // for `PlainYearMonth`): the `iso8601` calendar has no eras at all
        // (per the fix in `temporal_calendar_fields` above), so `era`/
        // `eraYear` (not even read above) never applies to field resolution
        // for it, matching Test262's `with/time-units-ignored.js` (`{ day:
        // 30, era: "BC" }` on an ISO `PlainDate` simply changes `day`,
        // `era` is inert). `chinese`/`dangi` are different from `iso8601`
        // here: ICU4X has no era concept for them either, but Temporal's
        // own behavior is to *reject* any use of `era`/`eraYear` rather
        // than silently ignore it (`mutually-exclusive-fields-{chinese,
        // dangi}.js`). On any calendar that *does* support eras, `era` and
        // `eraYear` must be supplied together or not at all — providing
        // exactly one is a `TypeError` (`mutually-exclusive-fields-*.js`'s
        // trailing `assert.throws(TypeError, ...)` pair), and this check
        // must run before any `RangeError` from an out-of-range/conflicting
        // month/day field (`calendarresolvefields-error-ordering-*.js`),
        // which is why it happens here, before the month/day fields below
        // are even resolved (every field has already been *read*, above,
        // in the correct order — only the cross-field validation/merge
        // logic below is free to run in whatever order is convenient, since
        // it never calls back into the user-supplied object again).
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
        // `ToPositiveIntegerWithTruncation`: `day` has no upper bound at the
        // field-reading stage (`CalendarFields.cpp`) — the real range check
        // happens once, below, against the calendar's own `overflow`
        // regulation, matching `plain_month_day.rs`'s identical fix and
        // Test262's `wrapping-at-end-of-month-*.js` (`date.with({ day:
        // daysInMonth + 1 })` constrains rather than field-bound-rejecting).
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
        // See `temporal_plain_date_from_fields`'s identical check for why
        // `month` is only cross-checked when `monthCode` was also supplied.
        if requested_year.is_some_and(|year| year != date.year().extended_year())
            || (month_code_s.is_some()
                && requested_month.is_some_and(|month| month as u8 != date.month().ordinal))
        {
            return Err(RuntimeError::RangeError(
                "inconsistent Temporal calendar fields".into(),
            ));
        }
        let mut result =
            Self::temporal_value_from_calendar_date(existing.kind, existing.calendar.clone(), date);
        if is_date_time {
            result.hour = requested_hour.unwrap_or(i32::from(existing.hour)) as u8;
            result.minute = requested_minute.unwrap_or(i32::from(existing.minute)) as u8;
            result.second = requested_second.unwrap_or(i32::from(existing.second)) as u8;
            result.millisecond =
                requested_millisecond.unwrap_or(i32::from(existing.millisecond)) as u16;
            result.microsecond =
                requested_microsecond.unwrap_or(i32::from(existing.microsecond)) as u16;
            result.nanosecond =
                requested_nanosecond.unwrap_or(i32::from(existing.nanosecond)) as u16;
        }
        self.alloc_temporal_value(result, false)
    }

    /// `Temporal.PlainDate.prototype.add`/`subtract`,
    /// `Temporal.PlainDateTime.prototype.add`/`subtract`. Years/months/weeks
    /// carry through the calendar first; every time-of-day unit (including a
    /// bare `days` field) then folds into a flat day/nanosecond offset —
    /// `PlainDate/prototype/add/balance-smaller-units.js` pins the 24-hour
    /// fold for a receiver with no time to preserve, and a `PlainDateTime`
    /// receiver's own time of day genuinely advances (with day carry) rather
    /// than being discarded.
    pub(in super::super) fn temporal_date_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let mut duration = self.temporal_duration_from_value(duration_value)?;
        if negate {
            duration.years = -duration.years;
            duration.months = -duration.months;
            duration.weeks = -duration.weeks;
            duration.days = -duration.days;
            duration.hours = -duration.hours;
            duration.minutes = -duration.minutes;
            duration.seconds = -duration.seconds;
            duration.milliseconds = -duration.milliseconds;
            duration.microseconds = -duration.microseconds;
            duration.nanoseconds = -duration.nanoseconds;
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let start = (existing.year, existing.month, existing.day);
        let time_total = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        )
        .total_nanoseconds();
        const DAY_NS: i128 = 86_400_000_000_000;
        let (total_days, time_fields) = if existing.kind == TemporalKind::PlainDateTime {
            let existing_ns = duration_math::time_fields_to_nanoseconds(
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            );
            let combined = existing_ns + time_total;
            let day_carry = combined.div_euclid(DAY_NS);
            let ns_of_day = combined.rem_euclid(DAY_NS);
            (
                duration.days + day_carry,
                Some(duration_math::time_fields_from_nanoseconds(ns_of_day)),
            )
        } else {
            (duration.days + time_total / DAY_NS, None)
        };
        let result_date = plain_date::calendar_add_date(
            calendar_kind,
            start,
            duration.years as i64,
            duration.months as i64,
            duration.weeks as i64,
            total_days as i64,
            reject,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        // `calendar_add_date` only range-checks via `regulate_iso_date`/
        // `balance_iso_date` (an i32-year/valid-month-day check), not
        // Temporal's own narrower representable range
        // (`-271821-04-19`..`+275760-09-13`, exclusive at the exact
        // day-and-nanosecond boundary for `PlainDateTime`) -- confirmed by a
        // real `add/limits.js` failure: subtracting one day from the exact
        // minimum `PlainDate` silently produced a valid-but-unrepresentable
        // `-271821-04-18` instead of throwing. `alloc_temporal_value`
        // performs no range validation of its own, matching the same gap
        // `Temporal.PlainDateTime.prototype.round` had.
        let in_range = match &time_fields {
            Some(time) => epoch::is_date_time_within_limits(result_date, *time),
            None => epoch::is_date_within_limits(result_date),
        };
        if !in_range {
            return Err(RuntimeError::RangeError(
                "Temporal date arithmetic is out of range".into(),
            ));
        }
        let value = match time_fields {
            Some(time) => Self::temporal_date_time_value(
                existing.kind,
                existing.calendar.clone(),
                result_date,
                time,
            ),
            None => {
                Self::temporal_date_value(existing.kind, existing.calendar.clone(), result_date)
            }
        };
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainDate.prototype.until`/`since`,
    /// `Temporal.PlainDateTime.prototype.until`/`since`.
    pub(in super::super) fn temporal_date_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let other = self.temporal_to_matching(other_value, existing.kind, &Value::Undefined)?;
        if existing.calendar != other.calendar {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same calendar".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let largest_raw = self.temporal_raw_string_option(&resolved_options, "largestUnit")?;
        let increment_raw =
            self.temporal_raw_number_option(&resolved_options, "roundingIncrement")?;
        let mode_raw = self.temporal_raw_string_option(&resolved_options, "roundingMode")?;
        let smallest_raw = self.temporal_raw_string_option(&resolved_options, "smallestUnit")?;

        let unit_floor = if existing.kind == TemporalKind::PlainDateTime {
            rounding::TemporalUnit::Nanosecond
        } else {
            rounding::TemporalUnit::Day
        };
        let default_smallest = unit_floor;
        let smallest_unit = match smallest_raw.as_deref() {
            None => default_smallest,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        if smallest_unit < unit_floor {
            return Err(RuntimeError::RangeError(
                "smallestUnit is out of range for this receiver".into(),
            ));
        }
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => smallest_unit.max(rounding::TemporalUnit::Day),
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if largest_unit < unit_floor {
            return Err(RuntimeError::RangeError(
                "largestUnit is out of range for this receiver".into(),
            ));
        }
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        let mode = Self::temporal_validated_rounding_mode(
            mode_raw.as_deref(),
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;
        // Both rounding steps below (`round_calendar_duration` for
        // day/week/month/year granularity, `TimeDuration::round` for
        // sub-day granularity) round a *real*, direction-aware signed
        // quantity computed in the fixed receiver-to-argument direction —
        // `Ceil`/`Floor` round toward a fixed end of the real number line
        // (`ceil(-x) == -floor(x)`, not `-ceil(x)`), and `HalfCeil`/
        // `HalfFloor` are the half-mode analogue. Negating the *result* for
        // `since` without also reflecting an asymmetric mode here would
        // silently round the wrong way whenever `since` negates a
        // non-exact value — exactly the same bug
        // `temporal_year_month_difference` (`PlainYearMonth`) already had
        // fixed for it (see that function's own comment). Confirmed via
        // `built-ins/Temporal/{PlainDate,PlainDateTime}/prototype/since/
        // roundingmode-{ceil,floor}.js`. `Trunc`/`Expand`/`HalfExpand`/
        // `HalfTrunc`/`HalfEven` are all symmetric under negation and need
        // no reflection.
        let effective_mode = if since {
            use blueice_ecma402::NumberRoundingMode as Mode;
            match mode {
                Mode::Ceil => Mode::Floor,
                Mode::Floor => Mode::Ceil,
                Mode::HalfCeil => Mode::HalfFloor,
                Mode::HalfFloor => Mode::HalfCeil,
                other => other,
            }
        } else {
            mode
        };

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        // `DifferenceTemporalPlainDate`/`DifferenceTemporalPlainDateTime`
        // always compute `CalendarDateUntil(calendar, temporalDate, other,
        // largestUnit)` — i.e. always in the fixed receiver-to-argument
        // direction, exactly like `until` — and only negate the *resulting*
        // Duration afterward for `since` (step 10). This must not be
        // implemented by swapping which date is `from`/`to` and skipping the
        // negation: `CalendarDateUntil`'s own algorithm anchors on `from`'s
        // day-of-month while walking years/months, so it is not
        // anti-symmetric (`f(other, existing) != -f(existing, other)` in
        // general — verified against Test262's
        // `PlainDate/prototype/since/basic-gregory.js`, whose "23 years, 11
        // months and 29 days" case a swap-based `from`/`to` computes as 30
        // days instead of 29, because it anchors on the wrong date's day
        // field). `from`/`to` are therefore always `existing`/`other`, and
        // the whole result is negated below when `since` is true.
        let from = (existing.year, existing.month, existing.day);
        let to = (other.year, other.month, other.day);
        let from_time = (
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        let to_time = (
            other.hour,
            other.minute,
            other.second,
            other.millisecond,
            other.microsecond,
            other.nanosecond,
        );

        const DAY_NS: i128 = 86_400_000_000_000;
        let from_ns = duration_math::time_fields_to_nanoseconds(
            from_time.0,
            from_time.1,
            from_time.2,
            from_time.3,
            from_time.4,
            from_time.5,
        );
        let to_ns = duration_math::time_fields_to_nanoseconds(
            to_time.0, to_time.1, to_time.2, to_time.3, to_time.4, to_time.5,
        );
        let mut time_diff = to_ns - from_ns;
        let date_sign = match plain_date::compare_iso_date(from, to) {
            std::cmp::Ordering::Less => 1_i64,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal => 0,
        };
        let mut adjusted_to = to;
        if time_diff != 0 && date_sign != 0 && time_diff.signum() != i128::from(date_sign) {
            adjusted_to = plain_date::add_iso_date(to, 0, 0, 0, -date_sign, false)
                .expect("shifting by one day never overflows a representable date");
            time_diff += i128::from(date_sign) * DAY_NS;
        }

        let (years, months, weeks, days, time_fields) = if smallest_unit
            >= rounding::TemporalUnit::Day
        {
            let (years, months, weeks, days) = plain_date::round_calendar_duration(
                calendar_kind,
                from,
                adjusted_to,
                Self::temporal_unit_to_date_unit(largest_unit),
                Self::temporal_unit_to_date_unit(smallest_unit),
                increment,
                effective_mode,
            );
            (years, months, weeks, days, None)
        } else {
            let time_unit = match smallest_unit {
                rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
                rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
                rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
                rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
                rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
                _ => rounding::TimeUnit::Nanosecond,
            };
            let rounded = duration_math::TimeDuration::from_nanoseconds(time_diff).round(
                time_unit,
                increment,
                effective_mode,
            );
            // This is a *duration* (signed magnitude), not a wall-clock time
            // of day, so the day/time split must be sign-consistent
            // (truncating toward zero) rather than the `div_euclid`/
            // `rem_euclid` wraparound `temporal_date_add`/`toString`/`round`
            // use elsewhere for an actual date+time point — otherwise a
            // negative difference's `days` field could end up negative while
            // its time fields stayed non-negative, which
            // `DurationRecord::try_new`'s common-sign rule rejects.
            let total = rounded.total_nanoseconds();
            let day_carry = total / DAY_NS;
            let ns_of_day = total % DAY_NS;
            let time_largest = if largest_unit >= rounding::TemporalUnit::Day {
                rounding::TimeUnit::Hour
            } else {
                match largest_unit {
                    rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
                    rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
                    rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
                    rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
                    rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
                    _ => rounding::TimeUnit::Nanosecond,
                }
            };
            let balanced =
                duration_math::TimeDuration::from_nanoseconds(ns_of_day).balance_to(time_largest);
            let (_, _, _, whole_days) = plain_date::calendar_difference_date(
                calendar_kind,
                from,
                adjusted_to,
                plain_date::DateUnit::Day,
            );
            let total_days = whole_days + day_carry as i64;
            let day_target =
                plain_date::calendar_add_date(calendar_kind, from, 0, 0, 0, total_days, false)
                    .expect("a rounded day-count from a representable date stays representable");
            let (y, m, w, d) = plain_date::calendar_difference_date(
                calendar_kind,
                from,
                day_target,
                Self::temporal_unit_to_date_unit(largest_unit),
            );
            (y, m, w, d, Some(balanced))
        };

        let (hours, minutes, seconds, milliseconds, microseconds, nanoseconds) = time_fields
            .map_or((0, 0, 0, 0, 0, 0), |fields: [i64; 6]| {
                (
                    fields[0], fields[1], fields[2], fields[3], fields[4], fields[5],
                )
            });
        // Step 10 of `DifferenceTemporalPlainDate`/`DifferenceTemporalPlainDateTime`:
        // the whole `years`..`nanoseconds` computation above is always in the
        // fixed `existing` (receiver) -> `other` (argument) direction — see
        // the comment on `from`/`to` above — so `since` negates every field
        // of the finished result rather than the inputs to the computation.
        let (
            years,
            months,
            weeks,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        ) = if since {
            (
                -years,
                -months,
                -weeks,
                -days,
                -hours,
                -minutes,
                -seconds,
                -milliseconds,
                -microseconds,
                -nanoseconds,
            )
        } else {
            (
                years,
                months,
                weeks,
                days,
                hours,
                minutes,
                seconds,
                milliseconds,
                microseconds,
                nanoseconds,
            )
        };
        let record = blueice_ecma402::DurationRecord::try_new(
            i128::from(years),
            i128::from(months),
            i128::from(weeks),
            i128::from(days),
            i128::from(hours),
            i128::from(minutes),
            i128::from(seconds),
            i128::from(milliseconds),
            i128::from(microseconds),
            i128::from(nanoseconds),
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    pub(in super::super) fn temporal_date_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let other = self.temporal_to_matching(other_value, existing.kind, &Value::Undefined)?;
        let mut equal = existing.year == other.year
            && existing.month == other.month
            && existing.day == other.day
            && existing.calendar == other.calendar;
        if equal && existing.kind == TemporalKind::PlainDateTime {
            equal = existing.hour == other.hour
                && existing.minute == other.minute
                && existing.second == other.second
                && existing.millisecond == other.millisecond
                && existing.microsecond == other.microsecond
                && existing.nanosecond == other.nanosecond;
        }
        Ok(Value::Bool(equal))
    }

    pub(in super::super) fn temporal_date_compare(
        &mut self,
        kind: TemporalKind,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let a = self.temporal_to_matching(one, kind, &Value::Undefined)?;
        let b = self.temporal_to_matching(two, kind, &Value::Undefined)?;
        let ord = (
            a.year,
            a.month,
            a.day,
            a.hour,
            a.minute,
            a.second,
            a.millisecond,
            a.microsecond,
            a.nanosecond,
        )
            .cmp(&(
                b.year,
                b.month,
                b.day,
                b.hour,
                b.minute,
                b.second,
                b.millisecond,
                b.microsecond,
                b.nanosecond,
            ));
        Ok(Value::Number(match ord {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    pub(in super::super) fn temporal_date_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let resolved_options = self.temporal_options(options)?;
        // `calendarName` is read before the time-precision options
        // (`fractionalSecondDigits`, `roundingMode`, `smallestUnit`),
        // matching `order-of-operations.js`'s alphabetical expectation --
        // and, for `PlainDate` specifically, it is the *only* option
        // `ISODateToString` ever reads: `Temporal.PlainDate.prototype.
        // toString` has no time component to round, so the other three
        // must not be touched at all for it (confirmed directly against
        // `PlainDate/prototype/toString/order-of-operations.js`'s own
        // expected list, which has no `fractionalSecondDigits`/
        // `roundingMode`/`smallestUnit` entries).
        let show_calendar_raw = self.temporal_string_option(
            &resolved_options,
            "calendarName",
            &["auto", "always", "never", "critical"],
        )?;
        let show_calendar = show_calendar_raw
            .as_deref()
            .map(|value| plain_date::parse_show_calendar(value).expect("already validated"))
            .unwrap_or(plain_date::ShowCalendar::Auto);

        if existing.kind == TemporalKind::PlainDate {
            let mut result =
                plain_date::format_iso_date((existing.year, existing.month, existing.day));
            result.push_str(&plain_date::format_calendar_annotation(
                &existing.calendar,
                show_calendar,
            ));
            return Ok(Value::String(result.into()));
        }

        let explicit_digits = self.temporal_fractional_second_digits(&resolved_options)?;
        let mode = self.temporal_rounding_mode(
            &resolved_options,
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;
        let smallest_unit = self.temporal_unit_option(&resolved_options, "smallestUnit", false)?;
        let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", false)?;

        let (precision, unit, increment) = match smallest_unit {
            Some(rounding::TimeUnit::Minute) => {
                (SecondsPrecision::Minute, rounding::TimeUnit::Minute, 1)
            }
            Some(rounding::TimeUnit::Second) => {
                (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1)
            }
            Some(rounding::TimeUnit::Millisecond) => (
                SecondsPrecision::Digits(3),
                rounding::TimeUnit::Millisecond,
                1,
            ),
            Some(rounding::TimeUnit::Microsecond) => (
                SecondsPrecision::Digits(6),
                rounding::TimeUnit::Microsecond,
                1,
            ),
            Some(rounding::TimeUnit::Nanosecond) | Some(rounding::TimeUnit::Hour) => (
                SecondsPrecision::Digits(9),
                rounding::TimeUnit::Nanosecond,
                1,
            ),
            None => match explicit_digits {
                None => (
                    SecondsPrecision::Auto,
                    rounding::TimeUnit::Nanosecond,
                    1_i128,
                ),
                Some(0) => (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1),
                Some(digits @ 1..=3) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Millisecond,
                    10_i128.pow(u32::from(3 - digits)),
                ),
                Some(digits @ 4..=6) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Microsecond,
                    10_i128.pow(u32::from(6 - digits)),
                ),
                Some(digits) => (
                    SecondsPrecision::Digits(digits),
                    rounding::TimeUnit::Nanosecond,
                    10_i128.pow(u32::from(9 - digits)),
                ),
            },
        };
        let time_ns = duration_math::time_fields_to_nanoseconds(
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        let rounded = duration_math::TimeDuration::from_nanoseconds(time_ns)
            .round(unit, increment, mode)
            .total_nanoseconds();
        const DAY_NS: i128 = 86_400_000_000_000;
        let day_carry = rounded.div_euclid(DAY_NS);
        let ns_of_day = rounded.rem_euclid(DAY_NS);
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
            RuntimeError::RangeError("Temporal.PlainDateTime.toString is out of range".into())
        })?;
        let (hour, minute, second, millisecond, microsecond, nanosecond) =
            duration_math::time_fields_from_nanoseconds(ns_of_day);
        let mut result = plain_date::format_iso_date(date);
        result.push_str(&format!("T{hour:02}:{minute:02}"));
        if precision != SecondsPrecision::Minute {
            result.push_str(&format!(":{second:02}"));
            let nanos_total = u32::from(millisecond) * 1_000_000
                + u32::from(microsecond) * 1_000
                + u32::from(nanosecond);
            match precision {
                SecondsPrecision::Minute | SecondsPrecision::Digits(0) => {}
                SecondsPrecision::Digits(digits) => {
                    let text = format!("{nanos_total:09}");
                    result.push('.');
                    result.push_str(&text[..digits as usize]);
                }
                SecondsPrecision::Auto if nanos_total != 0 => {
                    let text = format!("{nanos_total:09}");
                    result.push('.');
                    result.push_str(text.trim_end_matches('0'));
                }
                SecondsPrecision::Auto => {}
            }
        }
        result.push_str(&plain_date::format_calendar_annotation(
            &existing.calendar,
            show_calendar,
        ));
        Ok(Value::String(result.into()))
    }

    /// `Temporal.PlainDate.prototype.toLocaleString`/
    /// `Temporal.PlainDateTime.prototype.toLocaleString` share this adapter
    /// (the same runtime-`TemporalKind`-dispatch pattern every other shared
    /// `PlainDate`/`PlainDateTime` method here uses), but `CreateDateTimeFormat`'s
    /// `required` parameter differs between the two: `PlainDate`'s own is
    /// DATE, which rejects any `timeStyle` option unconditionally -- even
    /// alongside a `dateStyle` that would otherwise make the value visible
    /// (`intl402/.../PlainDate/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`) -- while `PlainDateTime`'s own is ANY, so
    /// `dateStyle`+`timeStyle` together are legal and must still format
    /// (`intl402/.../PlainDateTime/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`). This is the exact mirror of
    /// `temporal_plain_time_to_locale_string`'s own `required = TIME` check
    /// rejecting `dateStyle` unconditionally, flipped to the date side and
    /// scoped to `PlainDate` only.
    pub(in super::super) fn temporal_date_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let stack_base = self.stack.len();
        let result = (|| {
            let formatter = self.create_date_time_format(
                &Value::Undefined,
                &[
                    native::argument(args, 0).clone(),
                    native::argument(args, 1).clone(),
                ],
                false,
            )?;
            self.stack.push(formatter.clone());
            if existing.kind == TemporalKind::PlainDate
                && self
                    .date_time_format_data(&formatter)?
                    .options()
                    .time_style
                    .is_some()
            {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainDate.prototype.toLocaleString does not accept a timeStyle option"
                        .into(),
                ));
            }
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
    }

    pub(in super::super) fn temporal_date_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainDate/PlainDateTime cannot be converted to a primitive value".into(),
        ))
    }

    pub(in super::super) fn temporal_plain_date_to_plain_date_time(
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

    /// `Temporal.PlainDate.prototype.toPlainYearMonth`: resolves through
    /// `CalendarYearMonthFromFields` (Phase 26 Stage 2's
    /// `plain_year_month.rs`), correct for every calendar -- this used to
    /// pin the ISO reference day at `1` unconditionally, which is only
    /// correct for the `iso8601` calendar.
    pub(in super::super) fn temporal_plain_date_to_plain_year_month(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
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

    /// `Temporal.PlainDate.prototype.toPlainMonthDay`: resolves through
    /// `CalendarMonthDayFromFields` (Phase 26 Stage 2's
    /// `plain_month_day.rs`), correct for every calendar -- this used to pin
    /// the ISO reference year at `1972` unconditionally, which is only
    /// correct for the `iso8601` calendar.
    pub(in super::super) fn temporal_plain_date_to_plain_month_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
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

    pub(in super::super) fn temporal_plain_date_time_to_plain_date(
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

    pub(in super::super) fn temporal_plain_date_time_to_plain_time(
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

    pub(in super::super) fn temporal_plain_date_time_with_plain_time(
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

    pub(in super::super) fn temporal_plain_date_time_round(
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

    /// `SystemUTCEpochNanoseconds`, read from the one wall clock this engine
    /// already has: `Date.now()`'s own `SystemTime` call. Reusing it means
    /// `Temporal.Now.instant()` and `Date.now()` can never disagree, which is
    /// exactly what Test262's `Now/instant/return-value-value.js` checks by
    /// bracketing the call between two `Date.now()` reads.
    ///
    /// Millisecond granularity therefore, not nanosecond. The spec leaves the
    /// clock's resolution implementation-defined and explicitly permits
    /// coarsening it; real engines clamp for the same reason.
    pub(in super::super) fn temporal_now_epoch_nanoseconds() -> BigInt {
        BigInt::from(Self::current_time() as i64) * 1_000_000_u32
    }

    /// `ToTemporalTimeZoneIdentifier`. A `Temporal.ZonedDateTime` contributes
    /// its own zone; every other object is a `TypeError` — note that no
    /// `ToString` coercion happens at all here, so an object with a
    /// `toString` method is rejected rather than consulted.
    pub(in super::super) fn temporal_time_zone_identifier(
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
    pub(in super::super) fn temporal_now_local_fields(
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

    pub(in super::super) fn temporal_now_instant(&mut self) -> Result<Value, RuntimeError> {
        self.instant_from_epoch_nanoseconds(Self::temporal_now_epoch_nanoseconds())
    }

    pub(in super::super) fn temporal_now_time_zone_id(&mut self) -> Result<Value, RuntimeError> {
        Ok(Value::String(time_zone_id::SYSTEM.into()))
    }

    /// `Temporal.Now.plainDateISO`/`plainDateTimeISO`/`plainTimeISO`: the same
    /// wall clock, projected onto whichever of the three ISO-calendar plain
    /// types `kind` names. The fields each type does not carry keep the
    /// constructors' own 1970-01-01T00:00 placeholders.
    pub(in super::super) fn temporal_now_plain(
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
    pub(in super::super) fn temporal_now_zoned_date_time(
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
    pub(in super::super) fn temporal_time_zone(
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
    pub(in super::super) fn temporal_plain_date_zone_and_time(
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
    pub(in super::super) fn temporal_time_of_day(
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
    pub(in super::super) fn temporal_disambiguation(
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

    pub(in super::super) fn temporal_instant_to_zoned_date_time_iso(
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
