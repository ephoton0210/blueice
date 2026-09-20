// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    // ---- Stage 1 Track B: Temporal.Duration arithmetic ------------------
    //
    // Every method below implements the *calendar-agnostic* case completely:
    // a duration whose `years`/`months`/`weeks` are all zero, addressed with
    // units of `day` or smaller. `days` participate fully, at Temporal's own
    // fixed 86,400 seconds per day. A request that genuinely needs calendar
    // arithmetic — a non-zero `years`/`months`/`weeks`, a `year`/`month`/
    // `week` unit, or a `relativeTo` anchor this engine cannot resolve — is
    // rejected with a `RangeError` rather than answered approximately; see
    // Phase 26's plan for the Stage 2 boundary.

    /// Reads a validated `Temporal.Duration` receiver's own record.
    pub(in super::super) fn temporal_duration_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Duration method requires a Duration receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.Duration method requires a Duration receiver".into())
        })?;
        if value.kind != TemporalKind::Duration {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration method requires a Duration receiver".into(),
            ));
        }
        Ok(*value
            .duration
            .as_deref()
            .expect("Temporal.Duration values retain a duration record"))
    }

    pub(in super::super) fn temporal_duration_value(
        record: blueice_ecma402::DurationRecord,
    ) -> TemporalValue {
        TemporalValue {
            kind: TemporalKind::Duration,
            duration: Some(Box::new(record)),
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
        }
    }

    /// `CreateTemporalDuration`: stores the ten fields, then validates.
    ///
    /// Every field is a Number on a `Temporal.Duration`, so an exact
    /// nanosecond-accurate balancing result is observably rounded to the
    /// nearest double *before* the range check — and a value that passed the
    /// check exactly can fail it once rounded. Test262 checks this directly
    /// (`prototype/round/{float64-representable-integer,
    /// out-of-range-when-converting-from-normalized-duration}.js`,
    /// `prototype/add/{float64-representable-integer,result-out-of-range-3,
    /// argument-duration-precision-exact-numerical-values}.js`), so the
    /// round-trip is part of the algorithm rather than a lossy shortcut.
    pub(in super::super) fn temporal_duration_record(
        fields: [i128; 10],
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        let fields = fields.map(|value| value as f64 as i128);
        blueice_ecma402::DurationRecord::try_new(
            fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], fields[6], fields[7],
            fields[8], fields[9],
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn temporal_duration_create(
        &mut self,
        fields: [i128; 10],
    ) -> Result<Value, RuntimeError> {
        let record = Self::temporal_duration_record(fields)?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    /// `DefaultTemporalLargestUnit`: the largest unit the record actually uses.
    pub(in super::super) fn temporal_duration_largest_unit(
        record: &blueice_ecma402::DurationRecord,
    ) -> rounding::TemporalUnit {
        for (value, unit) in [
            (record.years, rounding::TemporalUnit::Year),
            (record.months, rounding::TemporalUnit::Month),
            (record.weeks, rounding::TemporalUnit::Week),
            (record.days, rounding::TemporalUnit::Day),
            (record.hours, rounding::TemporalUnit::Hour),
            (record.minutes, rounding::TemporalUnit::Minute),
            (record.seconds, rounding::TemporalUnit::Second),
            (record.milliseconds, rounding::TemporalUnit::Millisecond),
            (record.microseconds, rounding::TemporalUnit::Microsecond),
        ] {
            if value != 0 {
                return unit;
            }
        }
        rounding::TemporalUnit::Nanosecond
    }

    /// `GetOptionsObject`: `undefined` becomes a fresh empty object; any other
    /// non-object throws. Deliberately not `ToObject` — a primitive must be
    /// rejected, not boxed.
    pub(in super::super) fn temporal_duration_options(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        match value {
            Value::Undefined => {
                let object = self.with_roots(|heap| heap.alloc_object(None))?;
                let result = Value::Object(object);
                self.stack.push(result.clone());
                Ok(result)
            }
            Value::Object(_) => Ok(value.clone()),
            _ => Err(RuntimeError::TypeError(
                "Temporal options must be an object".into(),
            )),
        }
    }

    /// The required first argument of `round`/`total`: either a bare unit
    /// string (which the specification turns into a null-prototype object
    /// carrying only that one option, so no other option may be looked up) or
    /// an options object. `undefined` throws a `TypeError`.
    pub(in super::super) fn temporal_duration_round_to(
        &mut self,
        value: &Value,
        method: &str,
    ) -> Result<(Option<String>, Value), RuntimeError> {
        if *value == Value::Undefined {
            return Err(RuntimeError::TypeError(format!(
                "Temporal.Duration.prototype.{method} requires an argument"
            )));
        }
        if let Value::String(text) = value {
            let text = text
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError(format!("invalid {method} unit")))?;
            return Ok((Some(text), Value::Undefined));
        }
        let options = self.temporal_duration_options(value)?;
        Ok((None, options))
    }

    /// `GetTemporalUnitValuedOption`. `auto` is recognised only where
    /// `allow_auto` says so.
    pub(in super::super) fn temporal_duration_unit_option(
        &mut self,
        options: &Value,
        name: &str,
        allow_auto: bool,
    ) -> Result<UnitOption, RuntimeError> {
        let mut allowed = rounding::TEMPORAL_UNIT_NAMES.to_vec();
        if allow_auto {
            allowed.push("auto");
        }
        match self.temporal_string_option(options, name, &allowed)? {
            None => Ok(UnitOption::Unset),
            Some(text) if text == "auto" => Ok(UnitOption::Auto),
            Some(text) => Ok(UnitOption::Unit(
                rounding::parse_temporal_unit(&text)
                    .expect("temporal_string_option already validated the unit name"),
            )),
        }
    }

    pub(in super::super) fn temporal_duration_unit_name(
        text: &str,
        name: &str,
    ) -> Result<rounding::TemporalUnit, RuntimeError> {
        rounding::parse_temporal_unit(text)
            .ok_or_else(|| RuntimeError::RangeError(format!("invalid {name} option")))
    }

    /// `ToRelativeTemporalObject`. Resolves every accepted `relativeTo` shape
    /// (`undefined`, a `PlainDate`/`PlainDateTime`/`ZonedDateTime` object, a
    /// string, or a property bag) to a [`DurationAnchor`]. A `PlainYearMonth`/
    /// `PlainMonthDay` object is a real `TypeError` here, not a missing
    /// feature: `relativeto-wrong-type.js` confirms neither is in
    /// `ToRelativeTemporalObject`'s own accepted-object list, independent of
    /// whether those types themselves are otherwise implemented.
    pub(in super::super) fn temporal_duration_relative_to(
        &mut self,
        value: &Value,
    ) -> Result<Option<DurationAnchor>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(None);
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                let calendar = calendar::calendar_kind(&temporal.calendar)
                    .expect("Temporal values retain a validated calendar identifier");
                return match temporal.kind {
                    TemporalKind::PlainDate | TemporalKind::PlainDateTime => {
                        Ok(Some(DurationAnchor::Plain {
                            calendar,
                            date: (temporal.year, temporal.month, temporal.day),
                        }))
                    }
                    TemporalKind::ZonedDateTime => {
                        let zone = time_zone::parse_identifier(&temporal.time_zone)
                            .expect("Temporal values retain a validated time zone identifier");
                        Ok(Some(DurationAnchor::Zoned {
                            calendar,
                            zone,
                            epoch_ns: temporal.epoch_nanoseconds.clone(),
                            local_date: (temporal.year, temporal.month, temporal.day),
                            local_time: (
                                temporal.hour,
                                temporal.minute,
                                temporal.second,
                                temporal.millisecond,
                                temporal.microsecond,
                                temporal.nanosecond,
                            ),
                        }))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "relativeTo must be a PlainDate, PlainDateTime or ZonedDateTime".into(),
                    )),
                };
            }
            return self
                .temporal_duration_relative_to_property_bag(value)
                .map(Some);
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "relativeTo must be an object or a string".into(),
            ));
        }
        let source = self
            .coerce_string(value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid relativeTo string".into()))?;
        self.temporal_duration_relative_to_string(&source).map(Some)
    }

    /// `ToRelativeTemporalObject`'s property-bag path (`{ year, month, day,
    /// ..., timeZone?, offset?, calendar? }`).
    ///
    /// Deliberately does **not** delegate to `temporal_to_zoned_date_time`/
    /// `temporal_plain_date_from_fields` (as an earlier version of this
    /// function did) — `GetTemporalRelativeToOption`'s real algorithm reads
    /// and immediately coerces every property of the *merged*
    /// calendar-date + time-of-day + `offset`/`timeZone` field-name list,
    /// in strict alphabetical order (`day`, `era`, `eraYear` — only for a
    /// non-`iso8601` calendar — `hour`, `microsecond`, `millisecond`,
    /// `minute`, `month`, `monthCode`, `nanosecond`, `offset`, `second`,
    /// `timeZone`, `year`), entirely *before* ever branching on whether a
    /// `timeZone` was supplied. Delegating would mean reading `timeZone`
    /// first (to pick a branch) and then having each delegate re-derive its
    /// own, different field order — which is exactly what made this
    /// function's own `order-of-operations.js` fixtures fail. Resolves the
    /// date directly against the same `icu_calendar::Date::try_from_fields`/
    /// `temporal_interpret_offset` building blocks those two functions
    /// themselves use, so the actual *values* produced are unchanged; only
    /// the read order and call shape differ. `hour`/`minute`/`second`/
    /// `millisecond`/`microsecond`/`nanosecond` are still read and
    /// range-validated even when the eventual anchor is `Plain` (their
    /// values are simply discarded then), matching
    /// `relativeto-infinity-throws-rangeerror.js`'s time-field cases.
    pub(in super::super) fn temporal_duration_relative_to_property_bag(
        &mut self,
        bag: &Value,
    ) -> Result<DurationAnchor, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar_identifier(&calendar_value)?;

        let requested_day = self.temporal_read_optional_integer(bag, "day", 1, i32::MAX)?;
        // `iso8601` has no era concept at all -- see
        // `temporal_plain_date_from_fields`'s identical gate/comment.
        let read_era_fields = calendar != "iso8601";
        let era_v = if read_era_fields {
            self.get_property(bag, &"era".into())?
        } else {
            Value::Undefined
        };
        let era = (!matches!(era_v, Value::Undefined))
            .then(|| self.coerce_string(&era_v))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
            })
            .transpose()?;
        let era_year_v = if read_era_fields {
            self.get_property(bag, &"eraYear".into())?
        } else {
            Value::Undefined
        };
        let requested_hour = self.temporal_read_optional_integer(bag, "hour", 0, 23)?;
        let requested_microsecond =
            self.temporal_read_optional_integer(bag, "microsecond", 0, 999)?;
        let requested_millisecond =
            self.temporal_read_optional_integer(bag, "millisecond", 0, 999)?;
        let requested_minute = self.temporal_read_optional_integer(bag, "minute", 0, 59)?;
        let requested_month = self.temporal_read_optional_integer(bag, "month", 1, 99)?;
        let month_code =
            self.temporal_read_optional_string(bag, "monthCode", "invalid Temporal month code")?;
        let requested_nanosecond =
            self.temporal_read_optional_integer(bag, "nanosecond", 0, 999)?;
        // `offset` goes through `ToPrimitive` with a string hint and then
        // must *already be* a String -- an object's own `toString`/
        // `valueOf` is genuinely called, but a non-object, non-string
        // primitive is a `TypeError` without ever being stringified,
        // matching `temporal_to_zoned_date_time`'s own identical `offset`
        // handling (`relativeto-propertybag-invalid-offset-string.js`).
        let offset_value = self.get_property(bag, &"offset".into())?;
        let offset_primitive = (!matches!(offset_value, Value::Undefined))
            .then(|| self.coerce_primitive(&offset_value, "string"))
            .transpose()?;
        if let Some(primitive) = &offset_primitive {
            if !matches!(primitive, Value::String(_)) {
                return Err(RuntimeError::TypeError(
                    "Temporal relativeTo offset must be a string".into(),
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
        let requested_second = self.temporal_read_optional_integer(bag, "second", 0, 60)?;
        let time_zone_value = self.get_property(bag, &"timeZone".into())?;
        let requested_year = self.temporal_read_optional_integer(bag, "year", -275_760, 275_760)?;

        // Every field has now been read; nothing below touches `bag` again.
        let mut fields = DateFields::default();
        if let Some(era) = era.as_deref() {
            fields.era = Some(era.as_bytes());
            fields.era_year =
                Some(self.temporal_integer(&era_year_v, -9_999, 9_999, "era year")?);
        } else {
            if !matches!(era_year_v, Value::Undefined) {
                return Err(RuntimeError::RangeError(
                    "Temporal eraYear requires an era".into(),
                ));
            }
            fields.extended_year = Some(requested_year.ok_or_else(|| {
                RuntimeError::TypeError("Temporal date fields require year".into())
            })?);
        }
        if let Some(month_code) = month_code.as_deref() {
            fields.month_code = Some(month_code.as_bytes());
        } else if let Some(month) = requested_month {
            fields.ordinal_month = Some(month as u8);
        } else {
            return Err(RuntimeError::TypeError(
                "Temporal date fields require month or monthCode".into(),
            ));
        }
        let requested_day = requested_day
            .ok_or_else(|| RuntimeError::TypeError("Temporal date fields require day".into()))?;
        fields.day = Some(requested_day as u8);
        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar_identifier validates the calendar identifier");
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(icu_calendar::options::Overflow::Constrain);
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        let actual_year = date.year().extended_year();
        let actual_month = date.month().ordinal;
        if requested_year.is_some_and(|year| year != actual_year)
            || (month_code.is_some()
                && requested_month.is_some_and(|month| month as u8 != actual_month))
        {
            return Err(RuntimeError::RangeError(
                "inconsistent Temporal calendar fields".into(),
            ));
        }
        let iso = date.to_calendar(Iso);
        let local_date: epoch::CivilDate = (
            iso.year().extended_year(),
            iso.month().number(),
            iso.day_of_month().0,
        );

        if time_zone_value != Value::Undefined {
            let zone = self.temporal_time_zone(&time_zone_value)?;
            let offset_nanoseconds = match offset_string.as_deref() {
                None => None,
                Some(text) => {
                    Some(iso::parse_offset_string_nanoseconds(text).ok_or_else(|| {
                        RuntimeError::RangeError("invalid Temporal offset".into())
                    })?)
                }
            };
            let local_time: epoch::CivilTime = (
                requested_hour.unwrap_or(0) as u8,
                requested_minute.unwrap_or(0) as u8,
                requested_second.map(|value| value.min(59)).unwrap_or(0) as u8,
                requested_millisecond.unwrap_or(0) as u16,
                requested_microsecond.unwrap_or(0) as u16,
                requested_nanosecond.unwrap_or(0) as u16,
            );
            let epoch_ns = temporal_interpret_offset(
                &zone,
                local_date,
                local_time,
                offset_nanoseconds,
                false,
                time_zone::Disambiguation::Compatible,
                "reject",
                false,
            )?;
            if !epoch::is_in_instant_range(&epoch_ns) {
                return Err(RuntimeError::RangeError(
                    "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range"
                        .into(),
                ));
            }
            return Ok(DurationAnchor::Zoned {
                calendar: calendar_kind,
                zone,
                epoch_ns,
                local_date,
                local_time,
            });
        }
        Ok(DurationAnchor::Plain {
            calendar: calendar_kind,
            date: local_date,
        })
    }

    /// `ToRelativeTemporalObject`'s string path. A bracketed time-zone
    /// annotation (or a bare `Z`/`z` with none) names a real `Zoned` anchor —
    /// resolved via the same `temporal_value_from_zoned_date_time_string`
    /// `Temporal.ZonedDateTime.from` itself uses, including a named IANA zone
    /// now that Stage 2 supports one. A plain date(-time) string is a `Plain`
    /// anchor, resolved only against `PlainDate`'s own (looser) representable
    /// range at this stage — `temporal_value_from_string(PlainDate, ...)`,
    /// not `PlainDateTime` — since `ToRelativeTemporalObject` itself only
    /// ever needs a valid `PlainDate`; the tighter `PlainDateTime`
    /// (isoDateTime) boundary is a separate, later check that only applies
    /// once real date arithmetic is attempted
    /// (`temporal_duration_anchor_datetime_in_range`), which is exactly what
    /// `relativeto-string-limits.js`'s "valid ... but fails after early
    /// return" cases pin: a blank `Duration` never needs to convert the
    /// anchor to an isoDateTime at all, so it accepts a date string that a
    /// nonblank one rejects.
    pub(in super::super) fn temporal_duration_relative_to_string(
        &mut self,
        source: &str,
    ) -> Result<DurationAnchor, RuntimeError> {
        let parsed = iso::parse_date_time(source)
            .ok_or_else(|| RuntimeError::RangeError("invalid relativeTo string".into()))?;
        // A bare `Z` with no bracketed time-zone annotation names no real
        // zone at all, so it can resolve to neither a `ZonedDateTime` (no
        // identifier) nor a `PlainDateTime` (`Z` asserts an exact instant, a
        // contradiction for a wall-clock type) — a straight `RangeError`,
        // confirmed by `relativeto-string-invalid.js`'s own
        // `"2019-11-01T00:00Z"` case (contrast the accepted
        // `"...Z[-07:00]"`, which does carry a real identifier).
        if parsed.utc_designator && parsed.time_zone.is_none() {
            return Err(RuntimeError::RangeError(
                "a relativeTo string with a UTC designator needs a time-zone annotation".into(),
            ));
        }
        if parsed.time_zone.is_some() {
            let temporal = Self::temporal_value_from_zoned_date_time_string(
                source,
                time_zone::Disambiguation::Compatible,
                "reject",
            )?;
            let calendar = calendar::calendar_kind(&temporal.calendar)
                .expect("temporal_value_from_zoned_date_time_string validates the calendar");
            let zone = time_zone::parse_identifier(&temporal.time_zone)
                .expect("temporal_value_from_zoned_date_time_string validates the time zone");
            return Ok(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns: temporal.epoch_nanoseconds.clone(),
                local_date: (temporal.year, temporal.month, temporal.day),
                local_time: (
                    temporal.hour,
                    temporal.minute,
                    temporal.second,
                    temporal.millisecond,
                    temporal.microsecond,
                    temporal.nanosecond,
                ),
            });
        }
        let temporal = self.temporal_value_from_string(TemporalKind::PlainDate, source)?;
        let calendar = calendar::calendar_kind(&temporal.calendar)
            .expect("temporal_value_from_string validates the calendar identifier");
        Ok(DurationAnchor::Plain {
            calendar,
            date: (temporal.year, temporal.month, temporal.day),
        })
    }

    /// The deferred half of a `Plain` anchor's representable-range check: a
    /// date string/object/property-bag anchor only ever needs to be a valid
    /// `PlainDate` to *resolve* (see `temporal_duration_relative_to_string`'s
    /// own doc comment), but every calendar-aware arithmetic path below
    /// converts it to an isoDateTime (implicit midnight) before use, which is
    /// judged against `PlainDateTime`'s tighter, exact range —
    /// `relativeto-string-limits.js`/`relativeto-date-limits.js` pin exactly
    /// this boundary, including that a *blank* `Duration` (which never
    /// reaches this check, short-circuiting first) accepts a date the tighter
    /// check alone would reject.
    pub(in super::super) fn temporal_duration_anchor_datetime_in_range(
        date: epoch::CivilDate,
    ) -> Result<(), RuntimeError> {
        if !epoch::is_date_time_within_limits(date, (0, 0, 0, 0, 0, 0)) {
            return Err(RuntimeError::RangeError(
                "relativeTo is outside the representable range for a relativeTo parameter after \
                 conversion to DateTime"
                    .into(),
            ));
        }
        Ok(())
    }

    /// The full duration's target instant: `AddZonedDateTime(relativeTo,
    /// internalDuration, constrain)`, per `Duration.prototype.round`/`total`/
    /// static `compare`'s own Step "27.e"/equivalent — every `Zoned`-anchor
    /// arithmetic path below range-checks *this* exact instant first (not an
    /// approximation), matching `throws-if-target-nanoseconds-outside-valid-
    /// limits.js`/`relativeto-zoneddatetime-large-time-component-out-of-
    /// range.js`.
    pub(in super::super) fn temporal_duration_zoned_target(
        zone: &time_zone::TimeZone,
        calendar: AnyCalendarKind,
        anchor_epoch_ns: &BigInt,
        anchor_date: epoch::CivilDate,
        anchor_time: epoch::CivilTime,
        record: &blueice_ecma402::DurationRecord,
    ) -> Result<BigInt, RuntimeError> {
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds();
        let target = zoned_date_time::add_zoned_date_time(
            zone,
            calendar,
            anchor_epoch_ns,
            anchor_date,
            anchor_time,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            record.days as i64,
            time_total,
            false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        if !epoch::is_in_instant_range(&target) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range".into(),
            ));
        }
        Ok(target)
    }

    /// Applies a `Temporal.Duration` record's date part (calendar-aware) and
    /// time part (folded into whole days, exactly — a duration's fields keep
    /// a common sign, so truncating division loses nothing the way it would
    /// for two independent wall-clock endpoints) to `anchor`, returning the
    /// landing date plus the exact leftover sub-day nanosecond remainder.
    /// This is the one place every calendar-aware `round`/`total`/`compare`
    /// path below computes "anchor + this duration".
    pub(in super::super) fn temporal_duration_intermediate(
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
    ) -> Result<(epoch::CivilDate, i128), RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds();
        let time_days = time_total / DAY_NS;
        let ns_of_day = time_total % DAY_NS;
        let total_days = record.days + time_days;
        let intermediate = plain_date::calendar_add_date(
            calendar,
            anchor,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            total_days as i64,
            false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        // `calendar_add_date` only range-checks calendar-day validity (an
        // i32-year/valid-month-day check), not Temporal's own narrower
        // representable range: a huge `days`/`weeks` field can land on a
        // numerically valid but unrepresentable date (e.g.
        // `Math.trunc(2**53/86400)` days from `2000-01-01`) without
        // otherwise erroring —
        // `compare/duration-out-of-range-added-to-relativeto.js`,
        // `round/relativeto-duration-out-of-range-added-to-relative-date.js`.
        if !epoch::is_date_within_limits(intermediate) {
            return Err(RuntimeError::RangeError(
                "Temporal date arithmetic is out of range".into(),
            ));
        }
        Ok((intermediate, ns_of_day))
    }

    /// The two date-times a `Temporal.Duration` with a `Plain` `relativeTo`
    /// is measured between (`Duration.prototype.round` step 28,
    /// `total` step 13): the anchor at midnight, and where the whole duration
    /// lands -- its years/months/weeks through the calendar, its days folded
    /// with the time part at 24 hours each (`AddTime` floors, so a negative
    /// remainder lands on the previous day's evening, and
    /// `DifferenceISODateTime` borrows that day back).
    ///
    /// The range checks are the specification's own: `CalendarDateAdd` must
    /// stay representable, and -- only when the two points differ, since
    /// `DifferencePlainDateTimeWithRounding` returns a blank duration for equal
    /// ones before looking at limits -- both must be within
    /// `ISODateTimeWithinLimits`.
    #[allow(clippy::type_complexity)]
    pub(in super::super) fn temporal_duration_plain_endpoints(
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
    ) -> Result<
        (
            (epoch::CivilDate, epoch::CivilTime),
            (epoch::CivilDate, epoch::CivilTime),
        ),
        RuntimeError,
    > {
        const DAY_NS: i128 = 86_400_000_000_000;
        const MIDNIGHT: epoch::CivilTime = (0, 0, 0, 0, 0, 0);
        let out_of_range =
            || RuntimeError::RangeError("Temporal date arithmetic is out of range".into());
        // `ToInternalDurationRecordWith24HourDays`: the `days` field joins the
        // time part; years, months and weeks stay calendar fields.
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds()
            + record.days * DAY_NS;
        let target_days = i64::try_from(time_total.div_euclid(DAY_NS)).map_err(|_| out_of_range())?;
        let target_time =
            duration_math::time_fields_from_nanoseconds(time_total.rem_euclid(DAY_NS));
        let target_date = plain_date::calendar_add_date(
            calendar,
            anchor,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            target_days,
            false,
        )
        .filter(|date| epoch::is_date_within_limits(*date))
        .ok_or_else(out_of_range)?;
        let origin = (anchor, MIDNIGHT);
        let target = (target_date, target_time);
        if origin != target {
            Self::temporal_duration_anchor_datetime_in_range(anchor)?;
            if !epoch::is_date_time_within_limits(target_date, target_time) {
                return Err(out_of_range());
            }
        }
        Ok((origin, target))
    }
}
