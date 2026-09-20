// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

// ---- Stage 2's `zoned_date_time.rs` slice (third and final Stage 2 type) -
//
// A third, textually separate `impl Vm` block, deliberately appended here
// rather than folded into either block above -- this phase's own
// established "git diff misalignment" avoidance: keeping a new type's own
// adapter methods in their own block minimizes the chance a line-based merge
// mistakes one type's method for another's when this file is merged against
// concurrent sibling work (see this document's PLAN.md for the repeated
// pattern this avoids).
//
// `Temporal.ZonedDateTime` composes `PlainDateTime` + `TimeZone` +
// `Instant`: its stored ISO fields are always the *local* wall-clock fields
// its `epoch_nanoseconds` resolves to in its own `time_zone`
// (`temporal_set_local_fields`, Track E's own convention, reused throughout
// below), so every calendar-field/time-of-day getter and every
// `plain_date`/`calendar` helper this module already has for `PlainDate`/
// `PlainDateTime` applies to a `ZonedDateTime` receiver for free once its
// local fields are known to be correct -- which is genuinely new as of this
// slice: the numeric constructor and `from()` previously left every
// `ZonedDateTime`'s fields at their `1970-01-01T00:00:00` defaults
// regardless of its real epoch/zone (see the constructor fix above, and
// `temporal_to_zoned_date_time`/`temporal_value_from_zoned_date_time_string`
// below for the `from()` half of the same gap).
impl Vm {
    /// Brand check shared by every `Temporal.ZonedDateTime.prototype` method.
    pub(in super::super) fn temporal_zoned_date_time_receiver(
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
    pub(in super::super) fn temporal_offset_option(
        &mut self,
        options: &Value,
        default: &'static str,
    ) -> Result<String, RuntimeError> {
        Ok(self
            .temporal_string_option(options, "offset", &["prefer", "use", "ignore", "reject"])?
            .unwrap_or_else(|| default.to_string()))
    }

    /// `ToTemporalZonedDateTime`.
    pub(in super::super) fn temporal_to_zoned_date_time(
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
    pub(in super::super) fn temporal_value_from_zoned_date_time_string(
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
        let epoch_nanoseconds = temporal_interpret_offset(
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
        )?;
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

    pub(in super::super) fn temporal_zoned_date_time_with_time_zone(
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

    pub(in super::super) fn temporal_zoned_date_time_with_plain_time(
        &mut self,
        receiver: &Value,
        plain_time_like: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut value = self.temporal_zoned_date_time_receiver(receiver)?;
        let time = if *plain_time_like == Value::Undefined {
            (0, 0, 0, 0, 0, 0)
        } else {
            self.temporal_to_plain_time(plain_time_like, &Value::Undefined)?
        };
        let zone = temporal_zoned_date_time_zone(&value);
        let date = (value.year, value.month, value.day);
        value.epoch_nanoseconds = zone
            .epoch_nanoseconds_for(date, time, time_zone::Disambiguation::Compatible)
            .map_err(temporal_resolution_error)?;
        temporal_set_local_fields(&mut value, &zone);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.ZonedDateTime.prototype.with`. Mirrors `temporal_date_with`'s
    /// own field-merge shape (deliberately re-derived here rather than
    /// shared, per this block's own "own textual block" rationale above),
    /// extended with the `offset` field and the `disambiguation`/`offset`
    /// options `PlainDate`/`PlainDateTime` have no notion of.
    pub(in super::super) fn temporal_zoned_date_time_with(
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
        let requested_hour = self.temporal_read_optional_integer(like, "hour", 0, 23)?;
        let requested_microsecond =
            self.temporal_read_optional_integer(like, "microsecond", 0, 999)?;
        let requested_millisecond =
            self.temporal_read_optional_integer(like, "millisecond", 0, 999)?;
        let requested_minute = self.temporal_read_optional_integer(like, "minute", 0, 59)?;
        let requested_month = self.temporal_read_optional_integer(like, "month", 1, 99)?;
        let month_code_s =
            self.temporal_read_optional_string(like, "monthCode", "invalid Temporal month code")?;
        let requested_nanosecond =
            self.temporal_read_optional_integer(like, "nanosecond", 0, 999)?;
        let offset_string = self.temporal_read_optional_offset_string(like)?;
        let requested_second = self.temporal_read_optional_integer(like, "second", 0, 59)?;
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
        result.hour = requested_hour.unwrap_or(i32::from(existing.hour)) as u8;
        result.minute = requested_minute.unwrap_or(i32::from(existing.minute)) as u8;
        result.second = requested_second.unwrap_or(i32::from(existing.second)) as u8;
        result.millisecond =
            requested_millisecond.unwrap_or(i32::from(existing.millisecond)) as u16;
        result.microsecond =
            requested_microsecond.unwrap_or(i32::from(existing.microsecond)) as u16;
        result.nanosecond = requested_nanosecond.unwrap_or(i32::from(existing.nanosecond)) as u16;

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

    /// `Temporal.ZonedDateTime.prototype.add`/`subtract`: `AddZonedDateTime`
    /// (`vm/temporal/zoned_date_time.rs`) -- calendar years/months/weeks/days
    /// carried through the calendar at the receiver's own local date/time,
    /// re-resolved through the zone, and only then the exact time-duration
    /// nanoseconds added directly to that resolved instant.
    pub(in super::super) fn temporal_zoned_date_time_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
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
        let zone = temporal_zoned_date_time_zone(&existing);
        let time_total = duration_math::TimeDuration::from_fields(
            duration.hours,
            duration.minutes,
            duration.seconds,
            duration.milliseconds,
            duration.microseconds,
            duration.nanoseconds,
        )
        .total_nanoseconds();
        let local_date = (existing.year, existing.month, existing.day);
        let local_time = (
            existing.hour,
            existing.minute,
            existing.second,
            existing.millisecond,
            existing.microsecond,
            existing.nanosecond,
        );
        let result_ns = zoned_date_time::add_zoned_date_time(
            &zone,
            calendar_kind,
            &existing.epoch_nanoseconds,
            local_date,
            local_time,
            duration.years as i64,
            duration.months as i64,
            duration.weeks as i64,
            duration.days as i64,
            time_total,
            reject,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal.ZonedDateTime arithmetic is out of range".into())
        })?;
        if !epoch::is_in_instant_range(&result_ns) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime arithmetic is out of range".into(),
            ));
        }
        existing.epoch_nanoseconds = result_ns;
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
    }

    /// `Temporal.ZonedDateTime.prototype.round`: `RoundZonedDateTimeInstant`
    /// -- day-unit rounding anchors on `GetStartOfDay`'s real (possibly
    /// 23/25-hour) day boundary rather than a fixed 86,400-second one; every
    /// other unit rounds the local wall-clock time (`RoundISODateTime`'s own
    /// shape, matching `Temporal.PlainDateTime.prototype.round`), then
    /// re-resolves through the zone with `"compatible"` disambiguation.
    pub(in super::super) fn temporal_zoned_date_time_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        if *round_to == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime.round requires a smallestUnit or options argument".into(),
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
            let smallest_unit_text = smallest_unit.as_deref().ok_or_else(|| {
                RuntimeError::RangeError(
                    "Temporal.ZonedDateTime.round requires smallestUnit".into(),
                )
            })?;
            let zone = temporal_zoned_date_time_zone(&existing);
            if matches!(smallest_unit_text, "day" | "days") {
                if increment != 1 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement must be 1 when smallestUnit is \"day\"".into(),
                    ));
                }
                let date = (existing.year, existing.month, existing.day);
                let out_of_range = || {
                    RuntimeError::RangeError("Temporal.ZonedDateTime.round is out of range".into())
                };
                // `dateEnd` must itself be a representable date, and both
                // `GetStartOfDay` results a representable instant -- an
                // instance at the edge of the range has no upper (or lower)
                // bound to round toward (`day-rounding-out-of-range.js`,
                // `get-start-of-day-throws.js`).
                let next = plain_date::add_iso_date(date, 0, 0, 0, 1, false)
                    .filter(|next| epoch::is_date_within_limits(*next))
                    .ok_or_else(out_of_range)?;
                let start = zone.start_of_day(date);
                let end = zone.start_of_day(next);
                if !epoch::is_in_instant_range(&start) || !epoch::is_in_instant_range(&end) {
                    return Err(out_of_range());
                }
                let day_length = i128::try_from(&end - &start)
                    .expect("one day's length fits in i128 many times over");
                // `RoundZonedDateTime` step 19.f: when the wall-clock date's
                // midnight occurs twice (Antarctica/Casey turned its clocks
                // back across 2010-03-05T00:00), an instant on the *second*
                // occurrence is later than the next day's start; clamp it to the
                // last nanosecond of this day so rounding still lands on one of
                // its two start-of-day boundaries
                // (`same-date-starts-twice.js`).
                let last_of_day = &end - BigInt::from(1);
                let this_ns = std::cmp::min(&existing.epoch_nanoseconds, &last_of_day);
                let offset_into_day = i128::try_from(this_ns - &start)
                    .expect("an offset within one day fits in i128");
                let rounded =
                    rounding::round_to_increment_as_if_positive(offset_into_day, day_length, mode);
                existing.epoch_nanoseconds = start + BigInt::from(rounded);
            } else {
                let smallest_unit =
                    rounding::parse_time_unit(smallest_unit_text).ok_or_else(|| {
                        RuntimeError::RangeError("invalid smallestUnit option".into())
                    })?;
                // `ValidateTemporalRoundingIncrement(increment,
                // MaximumTemporalDurationRoundingIncrement(unit), false)`: unlike
                // `Instant.round` (whose increment may be a whole day), the
                // increment must stay *below* the count of this unit in the next
                // larger one and divide it -- `{ smallestUnit: "hour",
                // roundingIncrement: 24 }` throws
                // (`throws-on-invalid-increments.js`).
                let maximum = smallest_unit.increment_dividend();
                if increment >= maximum || maximum % increment != 0 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement does not divide evenly into the next larger unit".into(),
                    ));
                }
                let time_ns = duration_math::time_fields_to_nanoseconds(
                    existing.hour,
                    existing.minute,
                    existing.second,
                    existing.millisecond,
                    existing.microsecond,
                    existing.nanosecond,
                );
                let rounded = duration_math::TimeDuration::from_nanoseconds(time_ns)
                    .round(smallest_unit, increment, mode)
                    .total_nanoseconds();
                let day_carry = rounded.div_euclid(86_400_000_000_000);
                let ns_of_day = rounded.rem_euclid(86_400_000_000_000);
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
                    RuntimeError::RangeError("Temporal.ZonedDateTime.round is out of range".into())
                })?;
                let time = duration_math::time_fields_from_nanoseconds(ns_of_day);
                existing.epoch_nanoseconds = zone
                    .epoch_nanoseconds_for(date, time, time_zone::Disambiguation::Compatible)
                    .map_err(temporal_resolution_error)?;
            }
            if !epoch::is_in_instant_range(&existing.epoch_nanoseconds) {
                return Err(RuntimeError::RangeError(
                    "Temporal.ZonedDateTime.round is out of range".into(),
                ));
            }
            temporal_set_local_fields(&mut existing, &zone);
            self.alloc_temporal_value(existing, false)
        })();
        self.stack.truncate(base);
        result
    }

    /// `DifferenceTemporalZonedDateTime`'s field-level core: `TemporalDurationFromInternal`
    /// of `DifferenceZonedDateTimeWithRounding(receiver, other, ...)` — returned
    /// as the ten Duration fields, in the receiver-to-argument direction.
    ///
    /// This is deliberately the same pipeline `Temporal.Duration.prototype.
    /// round`/`total` run for a `ZonedDateTime` `relativeTo` (see
    /// [`zoned_difference`]): the specification defines all four in terms of it,
    /// and none of them may treat a named zone's day as a fixed 24 hours.
    pub(in super::super) fn temporal_zoned_date_time_difference_fields(
        origin: &zoned_difference::ZonedOrigin,
        other_epoch_ns: &BigInt,
        largest_unit: rounding::TemporalUnit,
        smallest_unit: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<[i64; 10], RuntimeError> {
        // `DifferenceTemporalZonedDateTime` step 8: equal instants are a blank
        // duration *before* any calendar-day bracketing happens -- not just a
        // fast path but a real spec-ordering requirement (and what keeps
        // `same-epoch-nanoseconds.js`, 660 unit/zone combinations at one
        // instant, inside the Test262 harness's instruction budget).
        if origin.epoch_nanoseconds == other_epoch_ns {
            return Ok([0; 10]);
        }
        let internal = zoned_difference::difference_with_rounding(
            origin,
            other_epoch_ns,
            largest_unit,
            increment,
            smallest_unit,
            mode,
        )
        .ok_or_else(|| RuntimeError::RangeError("Temporal.since/until is out of range".into()))?;
        Ok(internal.into_fields(largest_unit))
    }

    pub(in super::super) fn temporal_zoned_date_time_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let other = self.temporal_to_zoned_date_time(other_value, &Value::Undefined)?;
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

        let smallest_unit = match smallest_raw.as_deref() {
            None => rounding::TemporalUnit::Nanosecond,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        // `ZonedDateTime`'s own default `largestUnit` is the larger of
        // `"hour"` and `smallestUnit` -- unlike `Instant`'s `"second"` and
        // `PlainDate`/`PlainDateTime`'s `"day"` defaults.
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => smallest_unit.max(rounding::TemporalUnit::Hour),
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if smallest_unit > largest_unit {
            return Err(RuntimeError::RangeError(
                "smallestUnit must not be larger than largestUnit".into(),
            ));
        }
        // `DifferenceTemporalZonedDateTime` only requires `TimeZoneEquals`
        // (canonical zone identity, not raw spelling -- see
        // `TimeZone::time_zone_equals`'s own doc comment) once `largestUnit`
        // is `"day"` or coarser -- a pure time-unit difference (`largestUnit`
        // finer than `"day"`, the branch
        // `temporal_zoned_date_time_difference_fields` itself takes for
        // `largest_unit < TemporalUnit::Day`) is a plain epoch-instant
        // subtraction that never consults either operand's zone at all, so
        // two `ZonedDateTime`s in genuinely different zones may still be
        // diffed that way (`zoneddatetime-string.js`/
        // `argument-string-time-zone-annotation.js`, both using the default
        // `"hour"` largest unit -- checking zone equality unconditionally
        // regressed exactly these). Calendar-unit bracketing below, by
        // contrast, only ever resolves through the *receiver's* own zone, so
        // mismatched zones there must be rejected
        // (`canonicalize-iana-identifiers-before-comparing.js`: two IANA
        // aliases of the same real zone must not throw, but two genuinely
        // different zones must).
        if largest_unit >= rounding::TemporalUnit::Day
            && !temporal_zoned_date_time_zone(&existing)
                .time_zone_equals(&temporal_zoned_date_time_zone(&other))
        {
            return Err(RuntimeError::RangeError(
                "Temporal.since/until requires the same time zone".into(),
            ));
        }
        let increment = Self::temporal_validated_rounding_increment(increment_raw)?;
        // `GetDifferenceSettings`' last step: a time unit's increment must be
        // below, and divide, the count of it in the next larger unit
        // (`MaximumTemporalDurationRoundingIncrement`, `inclusive` false) --
        // `{ smallestUnit: "hours", roundingIncrement: 24 }` throws. Date units
        // have no such bound.
        if let Some(maximum) = smallest_unit.maximum_rounding_increment() {
            if increment >= maximum || maximum % increment != 0 {
                return Err(RuntimeError::RangeError(
                    "roundingIncrement does not divide evenly into the next larger unit".into(),
                ));
            }
        }
        let mode = Self::temporal_validated_rounding_mode(
            mode_raw.as_deref(),
            blueice_ecma402::NumberRoundingMode::Trunc,
        )?;
        // Same reflection `Vm::temporal_date_difference` needs, and for the
        // identical reason: `zoned_difference`'s rounding steps
        // (`TimeDuration::round` for a sub-day `smallestUnit`,
        // `nudge_expand_decision` for a calendar-unit one) both round a *real*,
        // direction-aware signed quantity computed in the fixed
        // receiver-to-argument direction — `Ceil`/`Floor` round toward a fixed end of the real
        // number line, not toward a fixed end of whichever internal
        // direction happened to be computed — so negating the *result* for
        // `since` without also reflecting an asymmetric mode here would
        // silently round the wrong way. Confirmed via
        // `built-ins/Temporal/ZonedDateTime/prototype/since/
        // roundingmode-{ceil,floor,halfCeil,halfFloor}.js`.
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

        let zone = temporal_zoned_date_time_zone(&existing);
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");

        let origin = zoned_difference::ZonedOrigin {
            zone: &zone,
            calendar: calendar_kind,
            epoch_nanoseconds: &existing.epoch_nanoseconds,
            date: (existing.year, existing.month, existing.day),
            time: (
                existing.hour,
                existing.minute,
                existing.second,
                existing.millisecond,
                existing.microsecond,
                existing.nanosecond,
            ),
        };
        let fields = Self::temporal_zoned_date_time_difference_fields(
            &origin,
            &other.epoch_nanoseconds,
            largest_unit,
            smallest_unit,
            increment,
            effective_mode,
        )?;
        // `since` is `until` with the finished Duration negated.
        let fields = if since {
            fields.map(|field| -field)
        } else {
            fields
        };
        // `CreateTemporalDuration` rounds every field to the nearest float64
        // before its range check (`temporal_duration_record`): a difference
        // too large for a double to hold exactly is observably rounded
        // (`prototype/{since,until}/float64-representable-integer.js`).
        let record = Self::temporal_duration_record(fields.map(i128::from))?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    pub(in super::super) fn temporal_zoned_date_time_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let other = self.temporal_to_zoned_date_time(other_value, &Value::Undefined)?;
        // `TimeZoneEquals`: compares primary-zone identity, not raw stored
        // spelling -- an IANA alias and its target (`Asia/Calcutta` /
        // `Asia/Kolkata`) are the same zone even though each value's own
        // `time_zone` field preserves whichever spelling was written (see
        // `TimeZone::time_zone_equals`'s own doc comment).
        let existing_zone = temporal_zoned_date_time_zone(&existing);
        let other_zone = temporal_zoned_date_time_zone(&other);
        Ok(Value::Bool(
            existing.epoch_nanoseconds == other.epoch_nanoseconds
                && existing_zone.time_zone_equals(&other_zone)
                && existing.calendar == other.calendar,
        ))
    }

    pub(in super::super) fn temporal_zoned_date_time_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_to_zoned_date_time(one, &Value::Undefined)?;
        let two = self.temporal_to_zoned_date_time(two, &Value::Undefined)?;
        Ok(Value::Number(
            match one.epoch_nanoseconds.cmp(&two.epoch_nanoseconds) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.0,
            },
        ))
    }

    /// `TemporalZonedDateTimeToString`: rounds the exact epoch instant first
    /// (`RoundTemporalInstant`'s own magnitude-based rounding, matching
    /// `Instant.prototype.toString` -- not a zone-day-aware rounding), then
    /// formats the *rounded* instant's local fields plus an exact (not
    /// minute-rounded) offset, an optional time-zone annotation and a
    /// calendar annotation.
    pub(in super::super) fn temporal_zoned_date_time_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_options(options)?;
            let show_calendar = self.temporal_string_option(
                &options,
                "calendarName",
                &["auto", "always", "never", "critical"],
            )?;
            let explicit_digits = self.temporal_fractional_second_digits(&options)?;
            let show_offset = self
                .temporal_string_option(&options, "offset", &["auto", "never"])?
                .unwrap_or_else(|| "auto".into());
            let mode =
                self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
            let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
            let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", false)?;
            let show_time_zone = self
                .temporal_string_option(&options, "timeZoneName", &["auto", "never", "critical"])?
                .unwrap_or_else(|| "auto".into());
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
            let epoch_i128: i128 = existing
                .epoch_nanoseconds
                .to_i128()
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal.ZonedDateTime".into()))?;
            let rounded = duration_math::TimeDuration::from_nanoseconds(epoch_i128)
                .round_as_if_positive(unit, increment, mode)
                .total_nanoseconds();
            let rounded_ns = BigInt::from(rounded);
            let zone = temporal_zoned_date_time_zone(&existing);
            let offset_ns = zone.offset_nanoseconds_for(&rounded_ns);
            let local = &rounded_ns + BigInt::from(offset_ns);
            let mut result = format_zoned_date_time_date_time(&local, precision);
            if show_offset != "never" {
                result.push_str(&format_offset_nanoseconds_exact(offset_ns));
            }
            if show_time_zone != "never" {
                result.push('[');
                if show_time_zone == "critical" {
                    result.push('!');
                }
                result.push_str(&existing.time_zone);
                result.push(']');
            }
            let show = plain_date::parse_show_calendar(show_calendar.as_deref().unwrap_or("auto"))
                .ok_or_else(|| RuntimeError::RangeError("invalid calendarName option".into()))?;
            result.push_str(&plain_date::format_calendar_annotation(
                &existing.calendar,
                show,
            ));
            Ok(Value::String(result.into()))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn temporal_zoned_date_time_value_of(
        &mut self,
    ) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.ZonedDateTime cannot be converted to a primitive value".into(),
        ))
    }

    pub(in super::super) fn temporal_zoned_date_time_to_instant(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_zoned_date_time_receiver(receiver)?;
        self.instant_from_epoch_nanoseconds(existing.epoch_nanoseconds)
    }

    pub(in super::super) fn temporal_zoned_date_time_to_plain_date(
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

    pub(in super::super) fn temporal_zoned_date_time_to_plain_time(
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

    pub(in super::super) fn temporal_zoned_date_time_to_plain_date_time(
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
    pub(in super::super) fn temporal_zoned_date_time_to_plain_year_month(
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

    pub(in super::super) fn temporal_zoned_date_time_to_plain_month_day(
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

    pub(in super::super) fn temporal_zoned_date_time_start_of_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let mut existing = self.temporal_zoned_date_time_receiver(receiver)?;
        let zone = temporal_zoned_date_time_zone(&existing);
        let date = (existing.year, existing.month, existing.day);
        existing.epoch_nanoseconds = zone.start_of_day(date);
        temporal_set_local_fields(&mut existing, &zone);
        self.alloc_temporal_value(existing, false)
    }

    pub(in super::super) fn temporal_zoned_date_time_get_iso_fields(
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
    pub(in super::super) fn temporal_zoned_date_time_get_time_zone_transition(
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

/// Rewrites a value's ISO fields to the local wall-clock fields its
/// `epoch_nanoseconds` really has in `zone`. The ISO fields a `ZonedDateTime`
/// carries are local, so they need the offset the zone was really observing at
/// that instant — Track E's whole reason for existing.
pub(super) fn temporal_set_local_fields(value: &mut TemporalValue, zone: &time_zone::TimeZone) {
    let offset = zone.offset_nanoseconds_for(&value.epoch_nanoseconds);
    let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
        epoch::instant_fields(&(&value.epoch_nanoseconds + BigInt::from(offset)));
    value.year = year;
    value.month = month;
    value.day = day;
    value.hour = hour;
    value.minute = minute;
    value.second = second;
    value.millisecond = millisecond;
    value.microsecond = microsecond;
    value.nanosecond = nanosecond;
}

/// Maps a host-neutral zone-resolution failure onto the `RangeError` the spec
/// raises for it.
pub(super) fn temporal_resolution_error(_: time_zone::AmbiguousLocalTime) -> RuntimeError {
    RuntimeError::RangeError(
        "the local time is ambiguous or does not exist in this time zone".into(),
    )
}

/// Re-parses a `ZonedDateTime` value's own stored `time_zone` identifier
/// back into a [`time_zone::TimeZone`]. Always succeeds: the identifier only
/// ever comes from [`time_zone::TimeZone::identifier`] itself (the
/// constructor/`from`/every method below all store it that way), which is
/// always round-trippable through [`time_zone::parse_identifier`].
pub(super) fn temporal_zoned_date_time_zone(value: &TemporalValue) -> time_zone::TimeZone {
    time_zone::parse_identifier(&value.time_zone)
        .expect("a ZonedDateTime value's own stored time zone is always a valid identifier")
}

/// `RoundNumberToIncrement(offsetNanoseconds, 60e9, "halfExpand")`: rounds a
/// real UTC offset to the nearest whole minute, ties rounding away from
/// zero. Only meaningful for [`temporal_interpret_offset`]'s `match_minutes`
/// (`MatchBehaviour::MatchMinutes`) comparison -- see that function's own
/// doc comment.
fn round_offset_nanoseconds_to_minutes(offset_nanoseconds: i64) -> i64 {
    const MINUTE: i64 = 60_000_000_000;
    let quotient = offset_nanoseconds / MINUTE;
    let remainder = offset_nanoseconds % MINUTE;
    let rounded = if remainder.unsigned_abs() * 2 >= MINUTE.unsigned_abs() {
        quotient + if offset_nanoseconds > 0 { 1 } else { -1 }
    } else {
        quotient
    };
    rounded * MINUTE
}

/// `InterpretISODateTimeOffset`, collapsed to this engine's own three
/// offset-behaviour shapes:
///
/// - `utc_exact` (the ISO string `Z` designator only): the offset is exactly
///   zero, and the zone/disambiguation are never consulted at all.
/// - `offset_nanoseconds: None` (`"wall"` behaviour -- no offset spelled at
///   all): resolved purely through the zone and `disambiguation`.
/// - `offset_nanoseconds: Some(_)` (`"option"` behaviour -- a property-bag
///   `offset` field or a string's own numeric offset): used directly
///   whenever it matches one of the zone's real possible instants for that
///   local date/time; otherwise `offset_option` decides -- `"use"` trusts it
///   regardless, `"reject"` throws, and `"ignore"`/`"prefer"` both fall back
///   to zone/disambiguation resolution (the spec's own `InterpretISODateTimeOffset`
///   already collapses those last two into the same branch once no
///   candidate matches, so there is no separate `"prefer"` case to add).
///
/// `match_minutes` (`MatchBehaviour::MatchMinutes` vs. `MatchExactly`):
/// besides an exact match against a real candidate's own offset, also accept
/// a candidate whose real offset *rounded to the nearest minute* equals the
/// given offset -- legacy back-compat for a `ZonedDateTime` string's
/// minute-precision (no seconds spelled) leading offset against a named
/// zone with genuine historical sub-minute precision (`Africa/Monrovia`'s
/// pre-1972 `-00:44:30`, matched by a written `-00:45`). A property-bag
/// `offset` field and `.with()`'s own `offset` property are always
/// `MatchExactly`, per Gecko's `ZonedDateTime.cpp`
/// (`ToTemporalZonedDateTime`'s object overload, and `with`, both construct
/// `MatchBehaviour::MatchExactly` unconditionally -- only the *string*
/// overload of `ToTemporalZonedDateTime` ever picks `MatchMinutes`, and only
/// when the leading offset itself was not spelled with sub-minute
/// precision).
#[allow(clippy::too_many_arguments)]
pub(super) fn temporal_interpret_offset(
    zone: &time_zone::TimeZone,
    date: epoch::CivilDate,
    time: epoch::CivilTime,
    offset_nanoseconds: Option<i64>,
    utc_exact: bool,
    disambiguation: time_zone::Disambiguation,
    offset_option: &str,
    match_minutes: bool,
) -> Result<BigInt, RuntimeError> {
    let local = epoch::nanoseconds_since_epoch(date, time, 0);
    if utc_exact {
        return Ok(local - BigInt::from(offset_nanoseconds.unwrap_or(0)));
    }
    let Some(offset_ns) = offset_nanoseconds else {
        return zone
            .epoch_nanoseconds_for(date, time, disambiguation)
            .map_err(temporal_resolution_error);
    };
    // `InterpretISODateTimeOffset` step 7 (`prefer`/`reject`): unlike `use`
    // and `ignore`, matching the offset against the zone's possible instants
    // starts from the *wall-clock* date itself, which must be within
    // `CheckISODaysRange`'s +/-10^8 days of the epoch -- a day narrower at
    // the start of the range than `PlainDateTime`'s own limits, so
    // `-271821-04-19T23:00-01:00[-01:00]` (an in-range instant) is still
    // rejected (`ZonedDateTime/from/argument-string-limits.js`).
    if matches!(offset_option, "prefer" | "reject")
        && plain_date::iso_date_to_epoch_days(date).abs() > 100_000_000
    {
        return Err(RuntimeError::RangeError(
            "the wall-clock date is outside the representable range of ZonedDateTime".into(),
        ));
    }
    let possible = zone.possible_epoch_nanoseconds(date, time);
    for candidate in &possible {
        let candidate_offset = zone.offset_nanoseconds_for(candidate);
        if candidate_offset == offset_ns
            || (match_minutes && round_offset_nanoseconds_to_minutes(candidate_offset) == offset_ns)
        {
            return Ok(candidate.clone());
        }
    }
    match offset_option {
        "use" => Ok(&local - BigInt::from(offset_ns)),
        "reject" => Err(RuntimeError::RangeError(
            "the given offset does not match the time zone".into(),
        )),
        _ => zone
            .epoch_nanoseconds_for(date, time, disambiguation)
            .map_err(temporal_resolution_error),
    }
}

/// `FormatUTCOffsetNanoseconds`: an *exact* `±HH:MM[:SS[.sssssssss]]`
/// representation -- unlike `format_instant_string`'s own offset formatting
/// (`FormatDateTimeUTCOffsetRounded`, always rounded to the nearest minute,
/// which is what `Instant.prototype.toString`'s optional `timeZone` display
/// specifically calls for). `ZonedDateTime`'s own `offset`
/// getter/`getISOFields`/`toString` all need the real, possibly sub-minute
/// historical offset a named zone can carry (e.g. Monrovia's pre-1972
/// -00:44:30), because round-tripping the string must be exact.
pub(super) fn format_offset_nanoseconds_exact(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let magnitude = offset.unsigned_abs();
    let hours = magnitude / 3_600_000_000_000;
    let minutes = (magnitude / 60_000_000_000) % 60;
    let seconds = (magnitude / 1_000_000_000) % 60;
    let nanoseconds = magnitude % 1_000_000_000;
    let mut result = format!("{sign}{hours:02}:{minutes:02}");
    if seconds != 0 || nanoseconds != 0 {
        result.push_str(&format!(":{seconds:02}"));
        if nanoseconds != 0 {
            let text = format!("{nanoseconds:09}");
            result.push('.');
            result.push_str(text.trim_end_matches('0'));
        }
    }
    result
}

/// The date/time portion of `TemporalZonedDateTimeToString`'s output, given
/// an already-rounded *local* epoch value (`rounded_epoch_nanoseconds +
/// offset`, i.e. what `epoch::instant_fields` decomposes as this zone's own
/// wall-clock fields). Deliberately duplicates
/// `Vm::format_instant_string`'s own date/time formatting (rather than
/// reusing it) since that function always appends its *own* offset/`Z`
/// suffix, which `ZonedDateTime`'s exact (not minute-rounded) offset display
/// cannot reuse -- see [`format_offset_nanoseconds_exact`]'s own doc
/// comment.
fn format_zoned_date_time_date_time(local: &BigInt, precision: SecondsPrecision) -> String {
    let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
        epoch::instant_fields(local);
    let mut result = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
    };
    result.push_str(&format!("-{month:02}-{day:02}T{hour:02}:{minute:02}"));
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
    result
}
